//! Native Win32 system tray (`Shell_NotifyIconW`) and OS popup context menus
//! (`CreatePopupMenu` / `TrackPopupMenuEx`) with zero external tray dependencies.
//!
//! Features:
//! - Creates 32-bit ARGB `HICON` directly from RGBA8 pixel data (`TrayIconData`).
//! - Uses `NOTIFYICON_VERSION_4` and automatically re-adds the icon after Windows Explorer
//!   restarts (`TaskbarCreated` broadcast).
//! - Cleanly removes the icon on `Drop` so no ghost icon lingers in the notification area.
//! - Runs `TrackPopupMenuEx` with the Microsoft KB135788 `SetForegroundWindow` + `WM_NULL`
//!   sequence so clicking outside dismisses the menu cleanly.

use std::collections::HashMap;
use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;

use anyhow::{Context as _, Result};
use gpui::{SharedString, TrayEvent, TrayIconData, TrayIconOptions, TrayMenuItem};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, CreateBitmap, CreateDIBSection, DIB_RGB_COLORS,
    DeleteObject,
};
use windows::Win32::UI::Shell::{
    NIF_ICON, NIF_MESSAGE, NIF_SHOWTIP, NIF_TIP, NIM_ADD, NIM_DELETE, NIM_MODIFY, NIM_SETVERSION,
    NOTIFYICON_VERSION_4, NOTIFYICONDATAW, Shell_NotifyIconW,
};
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreateIconIndirect, CreatePopupMenu, DestroyIcon, DestroyMenu, GetCursorPos,
    HICON, HMENU, ICONINFO, MF_CHECKED, MF_GRAYED, MF_POPUP, MF_SEPARATOR, MF_STRING,
    PostMessageW, RegisterWindowMessageW, SetForegroundWindow, TPM_NONOTIFY, TPM_RETURNCMD,
    TPM_RIGHTBUTTON, TrackPopupMenuEx, WM_LBUTTONDBLCLK, WM_LBUTTONUP, WM_NULL, WM_RBUTTONUP,
    WM_USER,
};
use windows::core::{PCWSTR, w};

pub(crate) const WM_GPUI_TRAY_EVENT: u32 = WM_USER + 20;
const TRAY_UID: u32 = 1;

pub(crate) fn taskbar_created_message() -> u32 {
    unsafe { RegisterWindowMessageW(w!("TaskbarCreated")) }
}

pub(crate) struct WindowsTrayState {
    hwnd: HWND,
    icon: HICON,
    tooltip: String,
    menu: Vec<TrayMenuItem>,
    menu_on_left_click: bool,
}

impl WindowsTrayState {
    pub(crate) fn create(hwnd: HWND, options: TrayIconOptions) -> Result<Self> {
        let icon = create_hicon_from_rgba(&options.icon)?;
        let state = Self {
            hwnd,
            icon,
            tooltip: options.tooltip.to_string(),
            menu: options.menu,
            menu_on_left_click: options.menu_on_left_click,
        };
        if let Err(err) = state.add_to_shell() {
            unsafe {
                let _ = DestroyIcon(state.icon);
            }
            return Err(err);
        }
        Ok(state)
    }

    pub(crate) fn add_to_shell(&self) -> Result<()> {
        let mut nid = self.build_nid(NIF_MESSAGE | NIF_ICON | NIF_TIP | NIF_SHOWTIP);
        let ok = unsafe { Shell_NotifyIconW(NIM_ADD, &nid) };
        anyhow::ensure!(ok.as_bool(), "Shell_NotifyIconW(NIM_ADD) failed");
        nid.Anonymous.uVersion = NOTIFYICON_VERSION_4;
        let _ = unsafe { Shell_NotifyIconW(NIM_SETVERSION, &nid) };
        Ok(())
    }

    pub(crate) fn set_tooltip(&mut self, tooltip: &str) -> Result<()> {
        self.tooltip = tooltip.to_owned();
        let nid = self.build_nid(NIF_TIP | NIF_SHOWTIP);
        let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
        anyhow::ensure!(ok.as_bool(), "Shell_NotifyIconW(NIM_MODIFY tooltip) failed");
        Ok(())
    }

    pub(crate) fn set_icon(&mut self, data: &TrayIconData) -> Result<()> {
        let next_icon = create_hicon_from_rgba(data)?;
        let old_icon = self.icon;
        self.icon = next_icon;
        let nid = self.build_nid(NIF_ICON);
        let ok = unsafe { Shell_NotifyIconW(NIM_MODIFY, &nid) };
        if ok.as_bool() {
            unsafe {
                let _ = DestroyIcon(old_icon);
            }
            Ok(())
        } else {
            self.icon = old_icon;
            unsafe {
                let _ = DestroyIcon(next_icon);
            }
            anyhow::bail!("Shell_NotifyIconW(NIM_MODIFY icon) failed")
        }
    }

    pub(crate) fn set_menu(&mut self, menu: Vec<TrayMenuItem>) {
        self.menu = menu;
    }

    pub(crate) fn menu(&self) -> &[TrayMenuItem] {
        &self.menu
    }

    pub(crate) fn menu_on_left_click(&self) -> bool {
        self.menu_on_left_click
    }

    pub(crate) fn remove(&self) {
        let nid = self.build_nid(Default::default());
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }

    fn build_nid(&self, flags: windows::Win32::UI::Shell::NOTIFY_ICON_DATA_FLAGS) -> NOTIFYICONDATAW {
        let mut nid = NOTIFYICONDATAW {
            cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32,
            hWnd: self.hwnd,
            uID: TRAY_UID,
            uFlags: flags,
            uCallbackMessage: WM_GPUI_TRAY_EVENT,
            hIcon: self.icon,
            ..Default::default()
        };
        let encoded: Vec<u16> = OsStr::new(&self.tooltip).encode_wide().collect();
        let max_len = nid.szTip.len().saturating_sub(1);
        let copy_len = encoded.len().min(max_len);
        nid.szTip[..copy_len].copy_from_slice(&encoded[..copy_len]);
        nid.szTip[copy_len] = 0;
        nid
    }
}

impl Drop for WindowsTrayState {
    fn drop(&mut self) {
        self.remove();
        unsafe {
            let _ = DestroyIcon(self.icon);
        }
    }
}

/// Decodes the notification mouse message inside a `WM_GPUI_TRAY_EVENT` callback.
pub(crate) fn decode_tray_mouse_msg(lparam: LPARAM) -> Option<TrayEvent> {
    let msg = (lparam.0 & 0xFFFF) as u32;
    match msg {
        WM_LBUTTONUP => Some(TrayEvent::LeftClick),
        WM_LBUTTONDBLCLK => Some(TrayEvent::DoubleClick),
        WM_RBUTTONUP => Some(TrayEvent::RightClick),
        _ => None,
    }
}

/// Displays a native Win32 popup context menu at the current cursor position and returns the
/// selected [`TrayMenuItem`] ID, or `None` if cancelled.
///
/// Must be called outside any `AppCell` borrow (e.g. from a foreground executor task) because
/// `TrackPopupMenuEx` pumps a nested Win32 modal menu loop.
pub(crate) fn show_native_popup_menu(hwnd: HWND, items: &[TrayMenuItem]) -> Option<SharedString> {
    if items.is_empty() {
        return None;
    }
    let hmenu = unsafe { CreatePopupMenu() }.ok()?;
    let mut next_cmd: usize = 1000;
    let mut id_map: HashMap<usize, SharedString> = HashMap::new();
    populate_hmenu(hmenu, items, &mut next_cmd, &mut id_map);

    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
        // Required per Microsoft KB135788 so clicking outside dismisses the popup menu cleanly.
        let _ = SetForegroundWindow(hwnd);
    }

    let chosen = unsafe {
        TrackPopupMenuEx(
            hmenu,
            (TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_NONOTIFY).0,
            pt.x,
            pt.y,
            hwnd,
            None,
        )
    };
    unsafe {
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(hmenu);
    }

    let cmd = chosen.0 as usize;
    if cmd == 0 {
        None
    } else {
        id_map.remove(&cmd)
    }
}

fn populate_hmenu(
    hmenu: HMENU,
    items: &[TrayMenuItem],
    next_cmd: &mut usize,
    id_map: &mut HashMap<usize, SharedString>,
) {
    for item in items {
        match item {
            TrayMenuItem::Separator => unsafe {
                let _ = AppendMenuW(hmenu, MF_SEPARATOR, 0, PCWSTR::null());
            },
            TrayMenuItem::Action { id, label, enabled } => {
                let cmd = *next_cmd;
                *next_cmd += 1;
                if *enabled && !id.is_empty() {
                    id_map.insert(cmd, id.clone());
                }
                let mut flags = MF_STRING;
                if !*enabled {
                    flags |= MF_GRAYED;
                }
                let wide = to_wide(label.as_ref());
                unsafe {
                    let _ = AppendMenuW(hmenu, flags, cmd, PCWSTR(wide.as_ptr()));
                }
            }
            TrayMenuItem::Check {
                id,
                label,
                checked,
                enabled,
            } => {
                let cmd = *next_cmd;
                *next_cmd += 1;
                if *enabled && !id.is_empty() {
                    id_map.insert(cmd, id.clone());
                }
                let mut flags = MF_STRING;
                if *checked {
                    flags |= MF_CHECKED;
                }
                if !*enabled {
                    flags |= MF_GRAYED;
                }
                let wide = to_wide(label.as_ref());
                unsafe {
                    let _ = AppendMenuW(hmenu, flags, cmd, PCWSTR(wide.as_ptr()));
                }
            }
            TrayMenuItem::Submenu {
                label,
                enabled,
                items: children,
            } => {
                if let Ok(sub) = unsafe { CreatePopupMenu() } {
                    populate_hmenu(sub, children, next_cmd, id_map);
                    let mut flags = MF_POPUP;
                    if !*enabled {
                        flags |= MF_GRAYED;
                    }
                    let wide = to_wide(label.as_ref());
                    unsafe {
                        let _ = AppendMenuW(hmenu, flags, sub.0 as usize, PCWSTR(wide.as_ptr()));
                    }
                }
            }
        }
    }
}

fn to_wide(s: &str) -> Vec<u16> {
    OsStr::new(s).encode_wide().chain([0]).collect()
}

fn create_hicon_from_rgba(icon: &TrayIconData) -> Result<HICON> {
    let width = icon.width as i32;
    let height = icon.height as i32;
    let bmi = BITMAPINFO {
        bmiHeader: BITMAPINFOHEADER {
            biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32,
            biWidth: width,
            // Negative height = top-down DIB matching RGBA row order.
            biHeight: -height,
            biPlanes: 1,
            biBitCount: 32,
            biCompression: BI_RGB.0,
            ..Default::default()
        },
        ..Default::default()
    };

    let mut bits_ptr: *mut std::ffi::c_void = std::ptr::null_mut();
    let color_bmp = unsafe { CreateDIBSection(None, &bmi, DIB_RGB_COLORS, &mut bits_ptr, None, 0) }
        .context("CreateDIBSection for tray icon")?;

    if bits_ptr.is_null() {
        unsafe {
            let _ = DeleteObject(color_bmp.into());
        }
        anyhow::bail!("CreateDIBSection returned null pixel buffer");
    }

    // Write BGRA pixels (Win32 32-bit icons expect BGRA byte order).
    let pixel_count = (icon.width as usize) * (icon.height as usize);
    let dst = unsafe { std::slice::from_raw_parts_mut(bits_ptr as *mut u8, pixel_count * 4) };
    for i in 0..pixel_count {
        let s = i * 4;
        dst[s] = icon.rgba[s + 2]; // B
        dst[s + 1] = icon.rgba[s + 1]; // G
        dst[s + 2] = icon.rgba[s]; // R
        dst[s + 3] = icon.rgba[s + 3]; // A
    }

    let mask_bmp = unsafe { CreateBitmap(width, height, 1, 1, None) };
    let info = ICONINFO {
        fIcon: true.into(),
        xHotspot: 0,
        yHotspot: 0,
        hbmMask: mask_bmp,
        hbmColor: color_bmp,
    };
    let hicon = unsafe { CreateIconIndirect(&info) };
    unsafe {
        let _ = DeleteObject(color_bmp.into());
        let _ = DeleteObject(mask_bmp.into());
    }
    hicon.context("CreateIconIndirect for tray icon")
}

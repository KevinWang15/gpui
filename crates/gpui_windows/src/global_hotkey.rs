//! Native Win32 global system hotkeys (`RegisterHotKey` / `UnregisterHotKey`) attached to the
//! hidden `WindowsPlatform` top-level window — zero external crates required.

use std::collections::HashSet;

use anyhow::{Result, bail};
use gpui::{GlobalHotKey, GlobalHotKeyId, Modifiers};
use windows::Win32::Foundation::HWND;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    HOT_KEY_MODIFIERS, MOD_ALT, MOD_CONTROL, MOD_NOREPEAT, MOD_SHIFT, MOD_WIN, RegisterHotKey,
    UnregisterHotKey, VK_BACK, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_F1, VK_HOME, VK_INSERT,
    VK_LEFT, VK_NEXT, VK_OEM_1, VK_OEM_2, VK_OEM_3, VK_OEM_4, VK_OEM_5, VK_OEM_6, VK_OEM_7,
    VK_OEM_COMMA, VK_OEM_MINUS, VK_OEM_PERIOD, VK_OEM_PLUS, VK_PRIOR, VK_RETURN, VK_RIGHT,
    VK_SPACE, VK_TAB, VK_UP,
};

pub(crate) struct WindowsGlobalHotKeys {
    hwnd: HWND,
    next_id: u32,
    active: HashSet<u32>,
}

impl WindowsGlobalHotKeys {
    pub(crate) fn new(hwnd: HWND) -> Self {
        Self {
            hwnd,
            next_id: 1,
            active: HashSet::new(),
        }
    }

    pub(crate) fn register(&mut self, hotkey: &GlobalHotKey) -> Result<GlobalHotKeyId> {
        let vk = key_to_vk(hotkey.key.as_ref())
            .ok_or_else(|| anyhow::anyhow!("unsupported global hotkey key {:?}", hotkey.key))?;
        let mods = modifiers_to_win32(hotkey.modifiers);

        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        // Win32 RegisterHotKey IDs for an application must be in 0x0000..=0xBFFF.
        let win_id = ((id - 1) % 0xBFFF) + 1;
        if self.active.contains(&win_id) {
            bail!("global hotkey ID pool exhausted");
        }

        if let Err(err) = unsafe { RegisterHotKey(Some(self.hwnd), win_id as i32, mods, vk) } {
            bail!(
                "could not register global hotkey {}: {err}",
                hotkey.to_spec()
            );
        }
        self.active.insert(win_id);
        Ok(GlobalHotKeyId(win_id))
    }

    pub(crate) fn unregister(&mut self, id: GlobalHotKeyId) {
        if self.active.remove(&id.0) {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), id.0 as i32);
            }
        }
    }

    pub(crate) fn unregister_all(&mut self) {
        for id in self.active.drain() {
            unsafe {
                let _ = UnregisterHotKey(Some(self.hwnd), id as i32);
            }
        }
    }
}

impl Drop for WindowsGlobalHotKeys {
    fn drop(&mut self) {
        self.unregister_all();
    }
}

fn modifiers_to_win32(mods: Modifiers) -> HOT_KEY_MODIFIERS {
    let mut flags = MOD_NOREPEAT;
    if mods.control {
        flags |= MOD_CONTROL;
    }
    if mods.alt {
        flags |= MOD_ALT;
    }
    if mods.shift {
        flags |= MOD_SHIFT;
    }
    if mods.platform {
        flags |= MOD_WIN;
    }
    flags
}

fn key_to_vk(key: &str) -> Option<u32> {
    if key.len() == 1 {
        let b = key.as_bytes()[0];
        if b.is_ascii_uppercase() || b.is_ascii_digit() {
            return Some(b as u32);
        }
        if b.is_ascii_lowercase() {
            return Some(b.to_ascii_uppercase() as u32);
        }
    }
    if let Some(num) = key.strip_prefix('F')
        && let Ok(n) = num.parse::<u32>()
        && (1..=24).contains(&n)
    {
        return Some(VK_F1.0 as u32 + (n - 1));
    }
    let vk = match key {
        "Space" => VK_SPACE.0,
        "Enter" => VK_RETURN.0,
        "Tab" => VK_TAB.0,
        "Escape" => VK_ESCAPE.0,
        "Backspace" => VK_BACK.0,
        "Delete" => VK_DELETE.0,
        "Insert" => VK_INSERT.0,
        "Home" => VK_HOME.0,
        "End" => VK_END.0,
        "PageUp" => VK_PRIOR.0,
        "PageDown" => VK_NEXT.0,
        "ArrowUp" => VK_UP.0,
        "ArrowDown" => VK_DOWN.0,
        "ArrowLeft" => VK_LEFT.0,
        "ArrowRight" => VK_RIGHT.0,
        "Backquote" => VK_OEM_3.0,
        "Minus" => VK_OEM_MINUS.0,
        "Equal" => VK_OEM_PLUS.0,
        "BracketLeft" => VK_OEM_4.0,
        "BracketRight" => VK_OEM_6.0,
        "Backslash" => VK_OEM_5.0,
        "Semicolon" => VK_OEM_1.0,
        "Quote" => VK_OEM_7.0,
        "Comma" => VK_OEM_COMMA.0,
        "Period" => VK_OEM_PERIOD.0,
        "Slash" => VK_OEM_2.0,
        _ => return None,
    };
    Some(vk as u32)
}

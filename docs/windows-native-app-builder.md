# Building Native Windows & Desktop Apps with GPUI

This fork turns GPUI into a self-contained, zero-dependency (~10 MB single `.exe`) native Windows & cross-platform desktop app builder — covering everything needed for Spotlight/Alfred launchers, always-on-top frosted-glass floating panes, tray utilities, screenshot tools, and telemetry HUDs without writing custom Win32/`HWND` FFI wrappers.

## 1. Window Styles & Native OS Primitives

All new fields on [`WindowOptions`](../crates/gpui/src/platform.rs) default to backwards-compatible standard window behavior:

```rust
use gpui::{
    Bounds, WindowBackgroundAppearance, WindowBounds, WindowCornerPreference,
    WindowDecorations, WindowKind, WindowOptions, point, px, size,
};

let floating_opts = WindowOptions {
    window_bounds: Some(WindowBounds::Windowed(Bounds::new(
        point(px(120.), px(120.)),
        size(px(340.), px(176.)),
    ))),
    kind: WindowKind::PopUp,
    window_background: WindowBackgroundAppearance::Blurred,
    window_decorations: Some(WindowDecorations::Client),
    always_on_top: true,
    skip_taskbar: true,
    non_activating: true,       // WS_EX_NOACTIVATE + MA_NOACTIVATE on click
    click_through: false,       // toggle dynamically via window.set_click_through(bool)
    hide_window_border: true,   // DWMWA_BORDER_COLOR = DWMWA_COLOR_NONE
    window_corner_preference: WindowCornerPreference::Round,
    backdrop_corner_radius: Some(px(14.)),
    backdrop_blur_radius: Some(px(18.)),
    focus: false,
    show: true,
    ..Default::default()
};
```

### Runtime Window Controls (`Window`)

| Method | Purpose |
| --- | --- |
| `window.show()` / `window.show_no_activate()` / `window.hide()` | Zero-teardown instant show/hide without rebuilding GPU swapchains |
| `window.hide_immediately()` | Suppresses Windows DWM minimize/close fade animations + double `DwmFlush()` (ideal before screen capture) |
| `window.set_cloaked(bool)` | `DWMWA_CLOAK` pre-warming so a Spotlight popup stays off-screen until first reveal |
| `window.paint_now()` | Synchronous `WM_PAINT` before uncloaking to prevent a 1-frame stale flash |
| `window.set_always_on_top(bool)` | Toggles `HWND_TOPMOST` / `HWND_NOTOPMOST` live |
| `window.set_click_through(bool)` | Toggles `WS_EX_LAYERED \| WS_EX_TRANSPARENT` + `SetLayeredWindowAttributes(255)` live |
| `window.set_non_activating(bool)` | Toggles `WS_EX_NOACTIVATE` + `WM_MOUSEACTIVATE -> MA_NOACTIVATE` live |
| `window.set_backdrop_blur_radius(Pixels)` | Live WinRT `GaussianBlurEffect` blur sigma adjustment |
| `window.set_window_corner_preference(pref)` | Windows 11 DWM rounded corner preference (`Default`, `DoNotRound`, `Round`, `RoundSmall`) |
| `window.set_border_color(Option<Rgba>)` | Sets or suppresses (`None` -> `DWMWA_COLOR_NONE`) the Windows 11 1 px DWM frame border |
| `window.set_bounds_on_display(display, bounds, activate)` | Cross-DPI monitor hop without Windows DPI-jump scaling glitches |
| `window.set_physical_bounds(bounds, topmost)` | Exact physical-pixel Win32 positioning (`SetWindowPos`) |
| `window.capture_image()` | Captures rendered window pixels (`PrintWindow(PW_RENDERFULLCONTENT)`) to `RgbaImage` |
| `window.show_native_context_menu(items, on_select)` | Native Win32 `TrackPopupMenuEx` context menu at cursor |

---

## 2. Spotlight / Alfred Zero-Latency Launcher Pattern

Pre-create the popup once (cloaked), then reveal on the cursor's active display in `< 2 ms`:

```rust
use gpui::components::{CommandAction, CommandItem, CommandPaletteEvent, CommandPaletteState};
use gpui::window_placement::{ScreenRect, place_launcher_rect};

let display = cx.display_at_cursor().or_else(|| cx.primary_display());
let scale = display.as_ref().map_or(1.0, |d| d.scale_factor());
let work = display.as_ref().map(|d| d.visible_desktop_bounds());
// Place centered horizontally, 1/5 down the active monitor's work area:
let rect = place_launcher_rect(work_screen_rect, (620.0 * scale) as i32, (420.0 * scale) as i32);

let palette = cx.new(|cx| {
    CommandPaletteState::new(
        vec![
            CommandItem::new("vscode", "Visual Studio Code")
                .group("Apps")
                .action(CommandAction::new("new-window", "Open New Window")),
            CommandItem::new("wt", "Windows Terminal").group("Apps"),
        ],
        cx,
    )
    .standalone(true)   // No fullscreen modal backdrop scrim; stays open on execute
    .quick_slots(true)  // Ctrl 1..9 / Cmd 1..9 instant run + active-row ↵ hint
});
```

- **CJK / IME-safe filtering**: `CommandPaletteState` subscribes to `InputEvent::CommittedChange` so uncommitted Pinyin/Japanese marked text never causes search flicker.
- **Tiered camelCase & acronym fuzzy scoring**: `fuzzy_match_detailed` ranks `Exact > Prefix > WordPrefix > Substring > Acronym > Fuzzy` (`"vsc"` -> `"Visual Studio Code"`, `"vc"` -> `"VSCode"`).
- **Restore previous external app**: Capture `let prev = cx.foreground_external_window();` before revealing and call `cx.activate_external_window(prev);` after executing/hiding!

---

## 3. Global Hotkeys + `ShortcutRecorder`

```rust
use gpui::{GlobalHotKey, GlobalHotKeyManager};
use gpui::components::{ShortcutRecorderEvent, ShortcutRecorderState};

let mut hotkeys = GlobalHotKeyManager::new();
let mut map = std::collections::HashMap::new();
map.insert("toggle-spotlight".into(), GlobalHotKey::parse("Alt+Space")?);
hotkeys.apply(map, cx)?;

cx.on_global_hotkey(|id, cx| {
    // Resolve id -> action name via hotkeys.name_for_id(id)
});

// While ShortcutRecorder is capturing a new chord, suspend existing hotkeys:
cx.subscribe(&recorder, |this, _, ev, cx| match ev {
    ShortcutRecorderEvent::StartRecording => this.hotkeys.suspend(cx),
    ShortcutRecorderEvent::CancelRecording => { this.hotkeys.resume(cx); }
    ShortcutRecorderEvent::Change(next) => { /* update map & apply */ }
}).detach();
```

---

## 4. System Tray & Start with Windows

```rust
use gpui::{AutostartOptions, TrayEvent, TrayIconData, TrayIconOptions, TrayMenuItem};

// System tray (Shell_NotifyIconW on Windows with automatic Explorer restart recovery)
cx.set_tray_icon(TrayIconOptions {
    icon: TrayIconData::solid_badge(32, 99, 102, 241),
    tooltip: "My Native App".into(),
    menu: vec![
        TrayMenuItem::action("open", "Open Launcher"),
        TrayMenuItem::separator(),
        TrayMenuItem::action("quit", "Quit"),
    ],
    menu_on_left_click: false,
})?;
cx.on_tray_event(|ev, cx| match ev {
    TrayEvent::LeftClick | TrayEvent::DoubleClick => { /* toggle main window */ }
    TrayEvent::MenuItem(id) if id == "quit" => cx.quit(),
    _ => {}
});

// Start with Windows (HKCU\Software\Microsoft\Windows\CurrentVersion\Run) / macOS LaunchAgent / Linux XDG
let autostart = AutostartOptions::new("MyNativeApp").args(["--minimized"]);
let enabled = cx.is_launch_at_login("MyNativeApp")?;
cx.set_launch_at_login(autostart, !enabled)?;
```

---

## 5. Single-Instance Guard, Atomic Persistence & Filtered Dialogs

- **Single Instance (`SingleInstance::acquire`)**: Zero-dependency Windows Named Pipe (`\\.\pipe\...`) + `AllowSetForegroundWindow(primary_pid)` handoff (Unix domain socket on macOS/Linux) so clicking the `.exe` twice activates the running instance and forwards CLI args.
- **Crash-Safe Atomic Writes (`gpui_util::atomic_write`)**: Unique sibling temp file + `sync_all()` + Windows `MoveFileEx`/rename retry backoff on transient AV/indexer locks (`ERROR_ACCESS_DENIED` / `ERROR_SHARING_VIOLATION`).
- **Multi-Monitor Geometry (`gpui::window_placement`)**: L-shaped staggered drag clamping (`clamp_drag_position`), outer-edge docking (`resolve_drag_end`, `resolve_dock_origin`), disconnected monitor rescue (`clamp_existing_window`), 4-quadrant detached tooltip flipping (`compute_detached_origin`), and anti-jump dual toolbars (`place_dual_panels`).
- **Filtered Open/Save Dialogs**: `cx.prompt_for_open_paths(OpenPromptOptions { filters, .. })` & `cx.prompt_for_save_path(SavePromptOptions { default_extension, filters, .. })` with unowned Win32 modal-anchor support so dialogs never hide behind always-on-top windows.
- **Clipboard Hardening**: Automatic 10-attempt `OpenClipboard` retry backoff, dual `CF_DIB` + `"PNG"` image writes (for WeChat/QQ/Office/browsers), `CF_HDROP` file writes, 32-bit zero-alpha DIB fix, and `ClipboardEntry::Custom { format, bytes }`.

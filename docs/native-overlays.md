# Dual-Mode Dialogs & Toasts — In-Window Overlays vs. Native OS Windows

Because GPUI is a native multi-window desktop UI framework, dialogs and toast notifications do not have to live inside a single application viewport. `gpui::components` provides **both** presentation modes side by side so you can choose per-surface (or let users toggle) between:

1. **In-Window Overlay** (`OverlayPresentation::InWindow`) — `Dialog` / `dialog(...)` and `ToastHost` / `toast_host(...)`, rendered inside the parent window via GPUI's `deferred(...)` layer.
2. **Native OS Window** (`OverlayPresentation::NativeWindow`) — `DialogWindow` / `dialog_window()` (`WindowKind::Dialog`) and `NativeToastManager` (`WindowKind::PopUp`), rendered in dedicated OS-level windows.

Both modes share the same design tokens, Lucide icons, and live global `Theme` synchronization (`cx.refresh_windows()`).

---

## Side-by-Side Comparison

| Dimension | In-Window Overlay (`Dialog`, `ToastHost`) | Native OS Window (`DialogWindow`, `NativeToastManager`) |
| --- | --- | --- |
| **Underlying GPUI primitive** | `deferred(...)` overlay inside the parent window | Separate OS windows (`WindowKind::Dialog` & `WindowKind::PopUp`) |
| **OS Modality** | Application-level backdrop scrim inside one window | True OS dialog modality (parent disabled / sheet / `_NET_WM_STATE_MODAL`) |
| **Desktop bounds** | Clipped to parent window viewport | Can be dragged or stacked anywhere on the display — even outside compact utility windows |
| **Toast focus behavior** | Never steals OS focus (same window) | Non-activating popups (`focus: false`) — floats above all apps without stealing keyboard focus |
| **Backdrop & motion** | Full dimmed scrim (`theme.backdrop`) + spring slide/fade entrance | Crisp opaque card (`theme.popover` + 1px `theme.border`); optional borderless draggable header or OS titlebar |
| **State access** | Direct `&mut Context<Self>` inside parent `render()` | Standard GPUI multi-window callbacks (`on_confirm`, `on_close`, `on_dismiss`, or `WeakEntity<T>`) |
| **Live Theme sync** | Automatic on parent re-render | Automatic across all open dialog & toast windows via `Theme::set_global` → `cx.refresh_windows()` |
| **GPU / Window overhead** | Zero extra swapchains | One lightweight GPU surface per open dialog / toast window |
| **Best suited for** | In-page confirmations, rich stateful forms, web-like modal flows, single-window dashboards | Utility/tray/compact windows, OS-native modal alerts, desktop-wide non-activating notifications |

---

## Platform Behavior (Native OS Window Mode)

GPUI maps `WindowKind::Dialog` and `WindowKind::PopUp` to native windowing primitives on each desktop backend:

| Platform | `DialogWindow` (`WindowKind::Dialog`) | `NativeToastManager` (`WindowKind::PopUp`) |
| --- | --- | --- |
| **Linux (X11)** | `WM_TRANSIENT_FOR`, `_NET_WM_WINDOW_TYPE_DIALOG`, `_NET_WM_STATE_MODAL` | `override_redirect = 1`, `_NET_WM_WINDOW_TYPE_NOTIFICATION`, exact screen-corner coordinates, non-activating |
| **Linux (Wayland)** | `xdg_toplevel::set_parent` + `xdg_dialog::set_modal` | `xdg_toplevel` popup window |
| **macOS** | `NSPanel` attached via `[parent beginSheet:...]` | `NonactivatingPanel` at `NSPopUpWindowLevel` (`CanJoinAllSpaces \| FullScreenAuxiliary`) |
| **Windows** | `WS_EX_DLGMODALFRAME` with parent HWND disabled (`EnableWindow(parent, false)`) | `WS_EX_TOOLWINDOW \| WS_EX_TOPMOST` (frameless, topmost, hidden from taskbar) |

> **Why opaque compact windows for native toasts?**
> Each native toast opens in its own tight-fitted `356 × 82 px` opaque popup window (`WindowBackgroundAppearance::Opaque`) rather than a full-screen transparent overlay window. That guarantees zero invisible click-blocking areas over underlying desktop apps and renders cleanly on both composited desktops (macOS, Windows, Wayland, GNOME/KDE) and non-composited X11 environments.

---

## Modal Dialogs

### In-Window Overlay (`Dialog`)

```rust
use gpui::components::*;
use gpui::icons::LucideIcon;

dialog("publish-dlg", self.dialog_open)
    .size(DialogSize::Md)
    .icon(LucideIcon::Rocket)
    .title("Publish release v0.2.2?")
    .description("All component checks have passed.")
    .on_close(cx.listener(|this, _, _, cx| {
        this.dialog_open = false;
        cx.notify();
    }))
    .footer_action(
        button("cancel")
            .secondary()
            .label("Cancel")
            .on_click(cx.listener(|this, _, _, cx| {
                this.dialog_open = false;
                cx.notify();
            })),
    )
    .footer_action(
        button("confirm")
            .primary()
            .label("Confirm Publish")
            .on_click(cx.listener(|this, _, _, cx| {
                this.dialog_open = false;
                // perform action...
                cx.notify();
            })),
    )
```

### Native OS Window (`DialogWindow`)

```rust
use gpui::components::*;
use gpui::icons::LucideIcon;

let handle = DialogWindow::new()
    .size(DialogSize::Md)
    .native_titlebar(false) // false = borderless draggable header, true = OS titlebar chrome
    .icon(LucideIcon::Rocket)
    .title("Publish release v0.2.2?")
    .description("Runs in a dedicated native WindowKind::Dialog.")
    .center_over(window, cx)
    .cancel_button("Cancel")
    .confirm_button("Confirm Publish", move |dlg_win, cx| {
        // Runs before closing the dialog window; does NOT trigger `on_close`.
    })
    .on_close(move |_, cx| {
        // Fires at most once on Escape, header ×, Cancel, or OS close button.
    })
    .open(cx)?;
```

Key `DialogWindow` options:
- `.size(DialogSize::{Sm, Md, Lg, Xl})` and `.height(Pixels)` (auto-estimates a comfortable height if omitted).
- `.destructive(bool)` — styles the header icon badge and `.confirm_button(...)` with `ButtonVariant::Destructive`.
- `.native_titlebar(bool)` — `false` (default) uses `WindowDecorations::Client` with a draggable header (`WindowControlArea::Drag` + `window.start_window_move()`); `true` uses `WindowDecorations::Server` with OS titlebar chrome.
- `.center_over(window, cx)` — centers over the parent window while clamping to the display's `visible_bounds()`.
- `.body(|window, cx| ...)` / `.footer(|window, cx| ...)` — custom element closures for forms or multi-action footers.

---

## Toast Notifications

Both in-window and native toast modes share `ToastItem`, `ToastVariant`, and `ToastPosition` (`BottomRight`, `TopRight`, `BottomLeft`, `TopLeft`).

### In-Window Overlay (`ToastHost`)

```rust
toast_host(self.toasts.clone())
    .position(ToastPosition::BottomRight)
    .on_dismiss(cx.listener(|this, id, _, cx| {
        this.toasts.retain(|t| t.id != *id);
        cx.notify();
    }))
```

### Native OS Window (`NativeToastManager`)

Store a `NativeToastManager` handle on your root view (it is cheaply `Clone`-able via `Rc<RefCell<_>>`):

```rust
use std::time::Duration;
use gpui::components::*;

// Setup (e.g. in your app struct constructor)
let native_toasts = NativeToastManager::new();
native_toasts
    .position(ToastPosition::BottomRight)
    .max_visible(4)
    .auto_dismiss(Some(Duration::from_secs(5)))
    .on_dismiss(move |id, cx| {
        // Optional hook when dismissed via × click or timer
    });

// Trigger from any click handler or async task
let display_id = window.display(cx).map(|d| d.id());
native_toasts.push(
    ToastItem::new(next_id, "Changes saved")
        .description("Synced to workspace cleanly.")
        .success(),
    display_id,
    cx,
)?;

// Clean up when your main window closes
window.on_window_should_close(cx, move |_, cx| {
    native_toasts.dismiss_all(cx);
    true
});
```

`NativeToastManager` automatically:
- Allocates the lowest available vertical slot (`0..max_visible`) anchored to the chosen `ToastPosition` on `display.visible_bounds()`.
- Evicts and closes the oldest popup window when `max_visible` is exceeded.
- Schedules per-toast auto-dismiss timers via `cx.spawn` when `auto_dismiss(Some(duration))` is active.
- Closes individual toasts cleanly without re-entrant window borrows when clicked via `×` or `manager.dismiss(id, cx)`.

---

## Try Both Modes Live in the Playground

Launch the developer playground and navigate to **Overlays, Table & Feedback**:

```bash
cargo run -p gpui_playground
```

Use the **In-Window / Native OS Window** segmented switchers for Modal Dialog and Toast Notifications to compare focus behavior, dragging dialogs beyond the main window, OS titlebar vs. borderless headers, corner stacking (`BR`, `TR`, `BL`, `TL`), and live Light/Dark theme switching across open OS windows.

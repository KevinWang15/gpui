# Interactive, Loading & Blurred Container Components

This guide documents the loading, GPU backdrop-blur, and high-interactivity components in `gpui::components`.

---

## 1. `BlurContainer` & `LoadingOverlay`

Both components use GPUI's native GPU Dual-Kawase `.backdrop_blur(px(...))` pipeline — accelerated on both **WGPU (Linux/Vulkan)** and **DirectX 11 (`gpui_windows`)** — while keeping any overlaid UI 100% razor-sharp on top and blocking pointer clicks/scrolls underneath.

### `LoadingOverlay`

Wrap any `Card`, `Table`, chart, or form to blur its contents and display a centered spinner, status label, optional determinate `ProgressBar`, optional elevated pill card, and optional Cancel action:

```rust
use gpui::components::*;
use gpui::icons::IconSize;
use gpui::px;

loading_overlay(is_loading)
    .animated("metrics-loader")
    .blur(px(8.))
    .spinner_variant(SpinnerVariant::Bars)
    .spinner_size(IconSize::Xl)
    .card(true)
    .label("Syncing telemetry shards...")
    .description("Pointer events underneath are blocked while loading")
    .progress(progress_pct)
    .on_cancel(cx.listener(|this, _, _, cx| {
        this.cancel_sync(cx);
    }))
    .rounded(theme.radius_xl)
    .child(my_dashboard_card)
```

You can also pass `.custom_indicator(progress_ring(pct))` to swap the default spinner for a circular gauge or custom element while keeping the label, description, card, and cancel button intact.

### `BlurContainer`

A general-purpose frosted-glass blur wrapper for privacy shields ("Click to reveal"), permission/paywall gates, or focus guards:

```rust
blur_container()
    .active(is_masked)
    .animated("secret-blur")
    .blur(px(10.))
    .scrim_opacity(0.42)
    .block_interaction(true)
    .rounded(theme.radius_xl)
    .overlay(
        button("reveal")
            .primary()
            .label("Reveal Credentials")
            .on_click(cx.listener(|this, _, _, cx| this.reveal(cx))),
    )
    .child(sensitive_card)
```

---

## 2. Multi-Variant `Spinner` & `ProgressRing`

### `Spinner`

`Spinner` supports five animation styles via `SpinnerVariant`:

| Variant | Rendering Path | Best For |
| --- | --- | --- |
| `SpinnerVariant::Ring` | GPU-rotated `LoaderCircle` SVG | Inline buttons, general loading |
| `SpinnerVariant::Loader` | GPU-rotated 8-spoke `Loader` SVG | Classic desktop loading states |
| `SpinnerVariant::Dots` | Zero-relayout `QuadAnimationMode::SyncedWave` | Chat/streaming, compact cards |
| `SpinnerVariant::Bars` | Zero-relayout `QuadAnimationMode::SyncedWave` | Audio, telemetry, build tasks |
| `SpinnerVariant::Pulse` | Zero-relayout `QuadAnimationMode::SyncedWave` | Live status beacons, syncing |

All variants automatically respect `App::reduce_motion()` and support `.vertical()`, `.label(...)`, `.description(...)`, `.size(...)`, `.color(...)`, and `.speed(Duration)`.

### `ProgressRing`

Circular vector arc gauge powered by `PathBuilder::stroke` with rounded circular end-caps, optional spring animation (`.animated(id)`), custom center label, and `ComponentSize` presets:

```rust
progress_ring(72.0)
    .animated("upload-ring")
    .size(ComponentSize::Lg)
    .color(theme.success)
    .show_percentage(true)
```

---

## 3. Interactive Overlays: `CommandPalette`, `Drawer`, `ContextMenu`

### `CommandPalette` (`⌘K` / `Ctrl+K`)

Spotlight / VS Code–style fuzzy command runner with grouped sections, `Kbd` shortcut badges, `Up`/`Down`/`Enter`/`Escape` navigation, mouse hover sync, and GPU-blurred scrim:

```rust
let palette = cx.new(|cx| {
    CommandPaletteState::new(
        vec![
            CommandItem::new("new-file", "Create New File")
                .group("Workspace")
                .icon(LucideIcon::FilePlus)
                .shortcut(&["⌘", "N"])
                .keywords("add document"),
        ],
        cx,
    )
});

cx.subscribe(&palette, |this, _, ev: &CommandPaletteEvent, cx| {
    if let CommandPaletteEvent::Execute(id) = ev {
        // dispatch command id
    }
}).detach();

// Open via shortcut or button:
palette.update(cx, |p, cx| p.toggle(window, cx));

// Mount near the root of your view:
command_palette(&self.palette)
```

### `Drawer`

Slide-over panel supporting `DrawerSide::{Right, Left, Top, Bottom}`, `DrawerSize::{Sm, Md, Lg, Xl, Custom}`, GPU backdrop blur, smooth slide entrance, scrollable body, and pinned footer:

```rust
drawer("inspector-drawer", self.drawer_open)
    .side(DrawerSide::Right)
    .size(DrawerSize::Md)
    .backdrop_blur(px(8.))
    .icon(LucideIcon::SlidersHorizontal)
    .title("Properties")
    .on_close(cx.listener(|this, _, _, cx| {
        this.drawer_open = false;
        cx.notify();
    }))
    .child(body_content)
    .footer(button("done").primary().label("Done"))
```

### `ContextMenu`

Wrap any element to attach a right-click floating menu anchored at the cursor position and automatically clamped inside the window:

```rust
context_menu("canvas-ctx", my_canvas_element)
    .entries(vec![
        MenuEntry::Label("Actions".into()),
        MenuEntry::item_with_icon("Copy", LucideIcon::Copy, on_copy).with_shortcut("⌘C"),
        MenuEntry::checkbox_item("Wrap Lines", wrap, on_toggle_wrap),
        MenuEntry::Separator,
        MenuEntry::destructive_item("Delete", Some(LucideIcon::Trash), on_delete),
    ])
```

---

## 4. Interactive Controls & Layout

### `SplitPane`

Drag-resizable `Horizontal` or `Vertical` split container with live mouse dragging, double-click reset, ratio clamping, and keyboard arrow resizing when focused:

```rust
let split = cx.new(|cx| {
    SplitPaneState::new(cx)
        .direction(SplitDirection::Horizontal)
        .clamp_range(0.20, 0.80)
        .default_ratio(0.35)
});

split_pane("workspace-split", &split)
    .first(left_sidebar)
    .second(main_editor)
```

### `NumberInput`

Numeric stepper combining direct editing, `−` / `+` buttons, `Up`/`Down` (+ `Shift` 10×) stepping, clamping, decimal precision, and optional prefix/suffix labels:

```rust
let price = cx.new(|cx| {
    NumberInputState::new(cx)
        .range(0.0, 9999.0, cx)
        .step(5.0)
        .precision(2, cx)
        .default_value(49.0, cx)
        .prefix("$")
});
number_input(&price)
```

### `Stepper`

Horizontal or vertical multi-step workflow indicator with `StepStatus::{Completed, Active, Pending, Error}` and optional clickable step navigation:

```rust
stepper(
    "deploy-steps",
    vec![
        StepItem::new("Configure").description("Region & tier"),
        StepItem::new("Build").description("Release artifacts"),
        StepItem::new("Deploy").description("Production rollout"),
    ],
    active_step,
)
.on_step_click(cx.listener(|this, idx, _, cx| {
    this.active_step = *idx;
    cx.notify();
}))
```

### `Rating`, `CopyButton` & `PinInput`

- **`Rating`**: Interactive star/icon rating with live hover preview, `.max(usize)`, `.icon(LucideIcon)`, `.allow_clear(bool)`, `.read_only(bool)`, and `.show_value(bool)`.
- **`CopyButton`**: One-click clipboard writer (`cx.write_to_clipboard`) that morphs into a timed green `"Copied!"` checkmark state.
- **`PinInput` / `PinInputState`**: Segmented OTP / verification code input with auto-advance, `Backspace` backtracking, `Left`/`Right` arrows, `Cmd/Ctrl+V` multi-digit paste, `.masked(bool)`, `.alphanumeric(bool)`, and `PinInputEvent::Complete(code)`.

# Theming & Component Stylability

`gpui::components` is designed so applications can adopt anything from the default Tailwind Light/Dark themes to deeply customized flat dark or light desktop aesthetics—without forking component internals.

Stylability works at **two complementary layers**:

1. **Global Semantic Design Tokens (`Theme`)** — Customize surface elevation tiers, brand accent, borders, focus rings, elevation shadows, sidebar chrome, control fills, typography, and chart palettes in one place.
2. **Per-Instance Direct `Styled` Passthrough** — Every single `IntoElement` component (**64 / 64**) implements `gpui::Styled`, so standard GPUI modifiers (`.bg(...)`, `.border_color(...)`, `.rounded(...)`, `.w(...)`, `.h(...)`, `.px(...)`, `.text_color(...)`) apply directly to the rendered component frame.

---

## Semantic `Theme` Token Hierarchy

`Theme` (`crates/gpui/src/components/theme.rs`) separates UI chrome into **four distinct surface tiers** plus dedicated interactive and visualization tokens:

| Layer / Group | Tokens | Purpose |
| --- | --- | --- |
| **Surface Elevation Tiers** | `background`, `sidebar_bg`, `surface`, `input_bg`, `popover` | Separates page canvas, navigation sidebar, elevated cards, recessed form fields, and floating overlays. |
| **Surface States** | `surface_muted`, `surface_hover`, `surface_active` | Subtle container tracks, row hover highlights, and pressed fills. |
| **Sidebar Chrome** | `sidebar_bg`, `sidebar_border`, `sidebar_active_bg`, `sidebar_active_foreground`, `sidebar_active_icon` | Full control over navigation panel styling independently of main content cards. |
| **Borders & Focus** | `border`, `input_border`, `input_border_hover`, `ring`, `focus_ring_width`, `selection` | Set `focus_ring_width = px(0.)` for crisp border-only focus rings without outer glow shadows. |
| **Elevation & Radii** | `elevation_shadows`, `radius_sm`, `radius_md`, `radius_lg`, `radius_xl` | Set `elevation_shadows = false` for clean, flat modern desktop surfaces without drop shadows. |
| **Brand & Semantic** | `primary*`, `secondary*`, `destructive*`, `success*`, `warning*`, `info*` | Consistent solid and soft-tinted status colors across buttons, badges, alerts, and stat deltas. |
| **Interactive Controls** | `toggle_track_off`, `toggle_track_on`, `toggle_thumb`, `slider_track`, `slider_fill`, `slider_thumb`, `tooltip_bg`, `tooltip_foreground`, `tooltip_border` | Fine-grained styling of switches, sliders, and tooltips. |
| **Data Visualization** | `chart_colors`, `chart_baseline`, `chart_gridline` | Drives `ChartPalette::from_theme` automatically across all charts. |
| **Typography** | `font_family`, `mono_font_family` | Root UI font family (applied on `AppShell`) and monospace font family (applied on `Kbd`). |

---

## Fluent `Theme` Builders

Instead of mutating 50+ fields manually, chain the semantic builders on `Theme::light()` or `Theme::dark()`:

```rust
use gpui::{Hsla, px, rgb};
use gpui::components::{Theme, init_components};

pub fn build_custom_dark_theme() -> Theme {
    let accent: Hsla = rgb(0xe11d48).into(); // Rose accent

    Theme::dark()
        // Automatically derives primary, primary_hover, primary_soft, ring,
        // selection, sidebar active tint, toggle_track_on, slider_fill, and chart_colors[0]
        .with_accent(accent)
        // Four distinct elevation tiers: canvas, sidebar, card surface, recessed input
        .with_surfaces(
            rgb(0x0d0e14).into(), // background (canvas)
            rgb(0x101119).into(), // sidebar_bg
            rgb(0x141620).into(), // surface (cards & popovers)
            rgb(0x0c0d13).into(), // input_bg (recessed inputs, selects, pickers)
        )
        .with_surface_states(
            rgb(0x1a1d2a).into(), // surface_muted
            rgb(0x1e2230).into(), // surface_hover
            rgb(0x262b3d).into(), // surface_active
        )
        .with_borders(
            rgb(0x222534).into(), // border & sidebar_border
            rgb(0x282b3b).into(), // input_border
            rgb(0x3a3e52).into(), // input_border_hover
        )
        .with_sidebar_colors(
            rgb(0x101119).into(),
            rgb(0x222534).into(),
            accent.opacity(0.18),
            rgb(0xf8fafc).into(),
            rgb(0xf43f5e).into(),
        )
        .with_focus_ring_width(px(0.))    // Crisp 1px accent border focus, no shadow halo
        .with_elevation_shadows(false)     // Flat border-separated chrome
        .with_radii(px(4.), px(6.), px(8.), px(10.))
        .with_font_family("Inter")
        .with_mono_font_family("JetBrains Mono")
}
```

Install once at startup after `init_components(cx)`:

```rust
init_components(cx);
Theme::set_global(build_custom_dark_theme(), cx);
```

---

## Direct `Styled` Passthrough on Entity-Backed Controls

Wrapper elements that delegate rendering to a stateful `Entity` (`Input`, `TextBox`, `Select`, `ComboBox`, `Slider`, `FilterBar`, `TimeRangePicker`) apply `Styled` refinements directly onto their **inner interactive frame**—not onto an outer wrapper `div`—so overrides like `.w(px(72.))`, `.h(px(30.))`, `.bg(...)`, `.border_color(...)`, and `.rounded(...)` work naturally:

```rust
use gpui::{prelude::*, px};
use gpui::components::{input, select, button, ComponentSize};

// Compact numeric input inside a SettingRow
let compact_count = input(&count_state)
    .w(px(72.))
    .h(px(30.))
    .rounded(px(6.));

// Search input that flex-grows inside a toolbar
let search = input(&search_state)
    .flex_1()
    .h(px(34.));
```

> **Performance note**: Entity-backed wrappers (`Input`, `TextBox`, `Select`, `ComboBox`, `Slider`) perform a cheap immutable `state.read(cx).frame_style != self.style` check before mutating `state`, avoiding redundant entity leases and re-notifications on parent re-renders.

---

## Layout & Control Styling Primitives

### `Section` and `SettingRow` (`components/layout.rs`)

Compose structured preference and settings views with `section(...)`, `card().flat()`, `v_stack().divided()`, and `setting_row(...)`:

```rust
use gpui::{prelude::*, px};
use gpui::components::*;

section("Shortcuts")
    .description("Click a shortcut, then press the keys you want.")
    .child(
        card().flat().child(
            v_stack()
                .divided()
                .spacing(px(0.))
                .child(
                    setting_row("Quick launch")
                        .description("Press again to dismiss.")
                        .control(
                            button("shortcut-btn")
                                .soft_outline()
                                .size(ComponentSize::Sm)
                                .label("Press the new shortcut..."),
                        ),
                )
                .child(
                    setting_row("Launch at login")
                        .description("Start in the background.")
                        .control(toggle("autostart", enabled)),
                )
                .child(
                    setting_row("Visible results")
                        .description("Rows in the search window.")
                        .control(input(&rows_input).w(px(72.)).h(px(30.))),
                ),
        ),
    )
```

### Targeted Per-Component Customizers

- **`Button`**:
  - `ButtonVariant::SoftOutline` (`.soft_outline()`): soft accent-tinted fill paired with a 1px accent border (ideal for active recording / highlighted keycap states).
  - `.hover_bg(color)`: custom hover background override.
  - Icons automatically inherit custom `.text_color(...)` overrides applied via `Styled`.
- **`Sidebar` & `SidebarItem`**:
  - `Sidebar::header_bordered(bool)` and `Sidebar::footer_bordered(bool)` toggle internal divider lines.
  - `SidebarItem::{active_bg, active_text_color, active_icon_color, icon_color}` override individual navigation item colors.
- **`Toggle`**:
  - `Toggle::{track_off_color, track_on_color, thumb_color}` override switch colors per instance.
- **`Card`**:
  - `Card::flat()` strips drop shadow while keeping surface background and 1px hairline border.
- **Charts (`StackedBarChart`, `LineChart`, `BreakdownList`)**:
  - `.palette(ChartPalette)` overrides the theme-derived `ChartPalette` on a per-chart basis.

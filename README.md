# GPUI Fork — Tailwind-Inspired UI Components & Lucide Icons

A standalone fork of [`zed-industries/zed/crates/gpui`](https://github.com/zed-industries/zed/tree/main/crates/gpui) (forked from commit `4e161e70b5c02b0f63a50beb6937f9cd249c1678`) bundled with:

- **`gpui::components`** — a comprehensive Tailwind CSS / Tailwind UI–inspired component library and light/dark design system built natively on GPUI.
- **`gpui::icons`** — all **2,092** [Lucide](https://lucide.dev) SVG icons copied verbatim into `crates/gpui/assets/lucide/` with zero-copy static SVG rendering (`Svg::static_data`), a typed `LucideIcon` enum, an ergonomic `Icon` element, and `LucideAssetSource`.

Both modules are enabled by default via Cargo features (`components`, `lucide`) and can be disabled individually (`default-features = false`) when only core GPUI is needed.

---

## Included Components (`gpui::components`)

| Category | Components |
| --- | --- |
| **Theme & Transitions** | `palette` (all 22 Tailwind hues × 11 shades `50..=950`), `Theme::light()` / `Theme::dark()`, fluent `Theme` customizers (`with_accent`, `with_surfaces`, `with_borders`, `with_sidebar_colors`, `with_toggle_colors`, `with_focus_ring_width`, `with_elevation_shadows`, `with_font_family`, `with_chart_colors`), `ThemeMode`, `ThemePreference` (`Light` / `System` / `Dark`), `ThemeSwitcher`, `SegmentedControl`, GPU-composited `ViewTransition` (`Window::start_view_transition` click-origin radial reveal & cross-fade), `ComponentSize`, `ActiveTheme`, `init_components()` |
| **Analytics & Charts (`components/charts/`)** | `ChartCard` (interactive legend toggle, header action slot, collapsible `"Show data table"`), `StackedBarChart` (GPU-batched quads, top-segment rounding, hover tooltips, drag-brush selection `selectable` / `on_brush_select`), `LineChart` (solid & dashed `PathBuilder` strokes, area gradients, endpoint callout, crosshair scrubber, drag-brush window selection), `BreakdownList` (ranked proportional bars with spring widths & click-to-narrow), `LinearScale`, `BandScale`, `nice_ticks`, `ChartPalette`, `StatGroup` / `StatItem` / `StatDelta` (tabular numerals & polarity-aware deltas) |
| **Time Range & Filtering (`components/time_range/`)** | `TimeRangePicker` (`Quick Select` with `Last N unit` + presets, `Start and End` inputs with embedded `CalendarGrid`, `<` / `>` steppers, `🔍-` zoom out), zero-dep `CalendarDate` & `DateRange`, structured faceted `FilterBar` (`=`, `!=`, `is one of` multi-select checklist with live facet counts & segmented chips) |
| **Layout** | `Container` (`MaxWidth`), `Stack` / `VStack` / `HStack` (with `.divided()`), `Grid`, `Card` (`CardHeader`, `CardTitle`, `CardDescription`, `CardContent`, `CardFooter`), `Section`, `SettingRow`, `Divider`, `PageHeader`, `AppShell`, `Sidebar`, `SidebarGroup`, `SidebarItem`, `Navbar`, `ScrollArea` |
| **Buttons** | `Button` (`Primary`, `Secondary`, `Soft`, `SoftOutline`, `Outline`, `Ghost`, `Destructive`, `Link`, icon-only, pill, loading spinner, tooltip, custom `hover_bg`), `ButtonGroup` |
| **Inputs & Forms** | `Input` / `InputState` (single-line, full UTF-16 IME, selection, word nav, clipboard, undo/redo, horizontal scroll, password reveal toggle, clearable, prefix/suffix icons & addons), `TextBox` / `TextArea` (`TextBoxState`, multi-line editing, Up/Down line nav, vertical scroll, counter), `Field`, `Label`, `Kbd` |
| **Selection Controls** | `Toggle` / `Switch` (icons inside thumb, left/right labels), `ToggleButton`, `ToggleGroup`, `Checkbox` (`Checked`, `Unchecked`, `Indeterminate`), `CheckboxGroup`, `RadioGroup` (`List` & Tailwind `Cards`), `Select` / `SelectState` (groups, icons, descriptions, disabled options, keyboard nav, typeahead, anchored popover), `ComboBox` / `ComboBoxState` (searchable single & multi-select with removable chips, keyword matching, custom creation), `Slider` / `SliderState` |
| **Overlays & Menus** | `Dialog` / Modal (in-window overlay with backdrop scrim, Escape & outside-click dismiss, destructive alert variant) & `DialogWindow` (dedicated native OS `WindowKind::Dialog` with optional OS titlebar or borderless draggable header), `Popover`, `DropdownMenu` (`MenuEntry` items, icons, shortcuts, checkboxes, separators, section labels), `Tooltip`, `Toast` / `ToastHost` (4-corner `ToastPosition` in-window stack) & `NativeToastManager` (non-activating OS `WindowKind::PopUp` desktop notifications), `OverlayPresentation` |
| **Data & Navigation** | `Table` (sortable columns, alignment, striped/compact modes), `Tabs` (`Underline`, `Pills`, `Segmented`), `Accordion` / `Disclosure`, `Breadcrumb`, `Pagination` |
| **Display & Feedback** | `Badge` (8 Tailwind colors × `Soft`/`Solid`/`Outline`, dot, icon, removable), `Avatar` & `AvatarGroup` (initials, icon fallback, presence status dot), `Alert` (`Info`, `Success`, `Warning`, `Destructive`), `ProgressBar`, `Spinner`, `Skeleton`, `EmptyState` |

## Lucide Icons (`gpui::icons`)

All 2,092 Lucide SVG files are stored directly under `crates/gpui/assets/lucide/*.svg` alongside the upstream `crates/gpui/assets/lucide/LICENSE` (ISC License, with MIT License for Feather-derived icons).

```rust
use gpui::icons::{Icon, IconSize, LucideIcon};

// Zero-setup rendering (inherits ambient text color automatically)
let check = Icon::new(LucideIcon::Check).size(IconSize::Md);

// Custom color, stroke width, or GPU-animated spin
let spinner = Icon::new(LucideIcon::LoaderCircle)
    .size(IconSize::Lg)
    .stroke_width(1.75)
    .spin();

// Lookup by kebab-case name at runtime
let parsed = LucideIcon::from_name("chevron-down");
assert_eq!(parsed, Some(LucideIcon::ChevronDown));
```

You can also register `LucideAssetSource::new()` with `application().with_assets(...)` to load any icon via `svg().path("lucide/search.svg")`.

---

## Quick Start

```rust
use gpui::components::*;
use gpui::icons::{LucideAssetSource, LucideIcon};
use gpui::{App, Context, Window, WindowOptions, prelude::*, px};
use gpui_platform::application;

struct Demo {
    input: gpui::Entity<InputState>,
    enabled: bool,
}

impl gpui::Render for Demo {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        container().py(px(24.)).child(
            card()
                .child(card_header().bordered().child(card_title("Welcome")))
                .child(
                    card_content()
                        .child(field().label("Name").child(input(&self.input)))
                        .child(
                            toggle("enabled", self.enabled)
                                .label("Enable notifications")
                                .on_change(cx.listener(|this, v, _, cx| {
                                    this.enabled = *v;
                                    cx.notify();
                                })),
                        ),
                )
                .child(
                    card_footer().child(
                        button("save")
                            .primary()
                            .leading_icon(LucideIcon::Check)
                            .label("Save"),
                    ),
                ),
        )
    }
}

fn main() {
    application()
        .with_assets(LucideAssetSource::new())
        .run(|cx: &mut App| {
            init_components(cx);
            cx.open_window(WindowOptions::default(), |_, cx| {
                let input = cx.new(|cx| InputState::new(cx).placeholder("Ada Lovelace"));
                cx.new(|_| Demo { input, enabled: true })
            })
            .unwrap();
        });
}
```

## Documentation

Detailed guides for the architecture, components, theming, and performance benchmarks:

- [**GPU View Transitions & Radial Theme Switching**](docs/view-transitions.md) — Compositor snapshot/shader pipeline, `Window::start_view_transition`, `ViewTransition`, `ThemeSwitcher`, reduced-motion & fallback semantics.
- [**Theming & Component Stylability**](docs/theming-and-stylability.md) — Four-tier `Theme` token hierarchy, fluent builders (`with_accent`, `with_surfaces`, `with_borders`, `with_elevation_shadows`, `with_focus_ring_width`), 100% `Styled` passthrough, `Section` / `SettingRow`, and per-component overrides.
- [**Analytics, Charts, Time Range Picker & Faceted Filtering**](docs/charts-and-analytics.md) — `ChartCard`, `StackedBarChart`, `LineChart`, `BreakdownList`, drag-brush selection, interactive legends, `TimeRangePicker`, `CalendarGrid`, `FilterBar`, `StatGroup`, `SegmentedControl`, and component coordination recipes.
- [**Dual-Mode Dialogs & Toasts (In-Window vs. Native OS Windows)**](docs/native-overlays.md) — Side-by-side comparison of `Dialog` / `ToastHost` vs. `DialogWindow` (`WindowKind::Dialog`) and `NativeToastManager` (`WindowKind::PopUp`), platform behavior across Linux/macOS/Windows, and API recipes.
- [**Performance Benchmarks & Optimizations**](docs/performance.md) — Reproducible `--release` benchmark commands, measured before/after metrics, and 120 FPS authoring guidelines.

## Interactive Developer Playground, Tests & Benchmarks

A dedicated developer playground app lives in [`crates/gpui_playground`](crates/gpui_playground) with an interactive Analytics & Charts dashboard, live prop workbenches, auto-generated copyable Rust snippets, a live GPUI event inspector drawer, brand accent & click-origin radial Light/System/Dark theme switching, a paginated 2,092-icon Lucide explorer, and a 242-swatch Tailwind palette explorer:

```bash
# Launch the interactive Developer Playground app
cargo run -p gpui_playground

# Run all component, Lucide icon, WGSL shader, and playground GPUI tests
cargo test -p gpui --lib -- components::tests icons::tests
cargo test -p gpui_wgpu --lib -- view_transition_shader_is_valid_wgsl
cargo test -p gpui_playground

# Run release-mode component & WGPU ViewTransition performance benchmarks
cargo test --release -p gpui --lib --features components,lucide -- components::perf_tests --ignored --nocapture --test-threads=1
cargo test --release -p gpui_wgpu --lib -- view_transition_perf_30_frames --ignored --nocapture --test-threads=1

# Generate Rust API documentation
cargo doc --no-deps -p gpui --features components,lucide

# Also available as a single-file GPUI example
cargo run -p gpui --example components_showcase

# Regenerate crates/gpui/src/icons/lucide_generated.rs after updating SVGs
python3 scripts/generate_lucide.py

# Regenerate crates/gpui_windows/src/shaders_bytes.rs from WSL after editing .hlsl files
./scripts/regen-windows-shaders.sh
```

## Licenses

- **GPUI & workspace support crates**: Apache-2.0 (see [`LICENSE-APACHE`](LICENSE-APACHE)).
- **Lucide SVG icons** (`crates/gpui/assets/lucide/`): ISC License, with portions derived from Feather Icons under the MIT License (see [`crates/gpui/assets/lucide/LICENSE`](crates/gpui/assets/lucide/LICENSE)).

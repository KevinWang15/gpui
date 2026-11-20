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
| **Layout** | `Container` (`MaxWidth`), `Stack` / `VStack` / `HStack` (with `.divided()`), `Grid`, `SplitPane` / `SplitPaneState` (drag-resizable horizontal/vertical splits with double-click reset & keyboard resize), `Card` (`CardHeader`, `CardTitle`, `CardDescription`, `CardContent`, `CardFooter`), `Section`, `SettingRow`, `Divider`, `PageHeader`, `AppShell`, `Sidebar`, `SidebarGroup`, `SidebarItem`, `Navbar`, `ScrollArea` |
| **Typography & Actions** | `Heading` (`H1`–`H5`), `Text` (`ui_text`, semantic tones, inline `code`, copyable), `EditableText` (click-to-edit inline text), `TruncatedText` (`"Show more"` toggle), `CodeBlock` (line numbers + copy), `Button` (`Primary`, `Secondary`, `Soft`, `SoftOutline`, `Outline`, `Ghost`, `Destructive`, `Link`, icon-only, pill, loading spinner, tooltip, custom `hover_bg`), `ButtonGroup`, `CopyButton`, `FloatButton` & `FloatButtonGroup` (corner speed-dial FAB) |
| **Inputs & Forms** | `Input` / `InputState` (single-line, full UTF-16 IME, selection, word nav, clipboard, undo/redo, horizontal scroll, password reveal toggle, clearable, prefix/suffix icons & addons), `TextBox` / `TextArea` (`TextBoxState`, multi-line editing, Up/Down line nav, vertical scroll, counter), `NumberInput` / `NumberInputState`, `PinInput` / `PinInputState` (segmented OTP/PIN with auto-advance & paste), `TagInput` / `TagInputState` (free-form chip pills), `ColorPicker` / `ColorPickerState` (2D HSVA spectrum, hue/alpha sliders, hex & presets), `FileDropzone` / `FileDropzoneState` (native OS drag-and-drop & file picker), `DatePicker`, `TimePicker` (24h/12h), `Rating`, `FormState` & `FormRule` (declarative validation & dirty tracking), `Field`, `Label`, `Kbd` |
| **Selection & Hierarchies** | `Toggle` / `Switch`, `ToggleButton`, `ToggleGroup`, `Checkbox` (`Checked`, `Unchecked`, `Indeterminate`), `CheckboxGroup`, `RadioGroup` (`List` & Tailwind `Cards`), `Select`, `ComboBox`, `TreeSelect` (searchable hierarchical dropdown), `Cascader` (Miller-column path picker), `Transfer` (dual-column shuttle with search & checkboxes), `Slider` & `RangeSlider` (dual-thumb `[start, end]` + `SliderMark` ticks) |
| **Overlays & Menus** | `CommandPalette` (`⌘K` fuzzy command runner), `Drawer` (`Left`/`Right`/`Top`/`Bottom` slide-over sheet with GPU blur scrim), `Popconfirm` (anchored inline confirmation popover), `Tour` & `tour_target` (interactive spotlight walkthrough), `ImagePreview` (GPU-blurred zoom/rotate lightbox gallery), `ContextMenu` (right-click floating menu), `Dialog` & `DialogWindow` (dedicated native OS `WindowKind::Dialog`), `Popover`, `DropdownMenu`, `Tooltip`, `Toast` / `ToastHost` & `NativeToastManager` (`WindowKind::PopUp`), `OverlayPresentation` |
| **Data & Navigation** | `Tree` (hierarchical tree view with tri-state cascading checkboxes, search & keyboard nav), `Table` (sortable columns, checkbox multi-select with tri-state header, expandable detail rows, sticky-header scrolling) & `VirtualTable` (`uniform_list` 10k+ rows), `List` & `VirtualList`, `Descriptions` (multi-column key-value grid with spans & copy), `Timeline` (`Left` / `Alternate` stream + pending node), `Menu` (`Vertical` collapsible submenus & `Horizontal` menu bar), `Carousel`, `Tabs`, `Stepper`, `Accordion` / `Disclosure`, `Breadcrumb`, `Pagination` |
| **Loading, Blur & Feedback** | `BlurContainer` (GPU Dual-Kawase backdrop blur wrapper on WGPU & Windows DX11 with crisp unblurred foreground overlays & pointer blocking), `LoadingOverlay` (blurred container + spinner/progress/cancel overlay), `Spinner` (5 variants: `Ring`, `Loader`, `Dots`, `Bars`, `Pulse` with zero-relayout GPU `SyncedWave`), `ProgressBar`, `ProgressRing`, `ResultView` (`Success`/`Error`/`Warning`/`Info`/`403`/`404`/`500`), `QRCode` (pure-Rust ISO/IEC 18004 scannable QR renderer), `Watermark`, `Skeleton`, `Alert`, `Badge`, `Avatar` & `AvatarGroup`, `EmptyState` |

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

- [**Extended Desktop Component Catalog (Tier 1–3)**](docs/complete-component-catalog.md) — `Tree`, `Popconfirm`, `ColorPicker`, `FileDropzone`, `Descriptions`, `Timeline`, `VirtualTable`, `RangeSlider`, `DatePicker`/`TimePicker`, `VirtualList`, `Typography`, `ResultView`, `Transfer`, `TagInput`, `ImagePreview`, `TreeSelect`, `Cascader`, `Tour`, `Menu`, `Carousel`, `FloatButton`, `Watermark`, `QRCode`, and `FormState`.
- [**Interactive, Loading & Blurred Container Components**](docs/interactive-and-loading-components.md) — `BlurContainer`, `LoadingOverlay` (GPU Dual-Kawase blur on WGPU & Windows DX11), 5-variant `Spinner`, `ProgressRing`, `CommandPalette` (`⌘K`), `Drawer`, `ContextMenu`, `SplitPane`, `NumberInput`, `Stepper`, `Rating`, `CopyButton`, and `PinInput`.
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

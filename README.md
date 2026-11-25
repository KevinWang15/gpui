# GPUI Fork — UI Components & Lucide Icons

A standalone fork of [`zed-industries/zed/crates/gpui`](https://github.com/zed-industries/zed/tree/main/crates/gpui) (forked from commit `4e161e70b5c02b0f63a50beb6937f9cd249c1678`) bundled with:

- **`gpui::components`** — a comprehensive component library and light/dark design system built natively on GPUI.
- **`gpui::icons`** — all **2,092** [Lucide](https://lucide.dev) SVG icons copied verbatim into `crates/gpui/assets/lucide/` with zero-copy static SVG rendering (`Svg::static_data`), a typed `LucideIcon` enum, an ergonomic `Icon` element, and `LucideAssetSource`.

Both modules are enabled by default via Cargo features (`components`, `lucide`) and can be disabled individually (`default-features = false`) when only core GPUI is needed.

---

## Included Components (`gpui::components`)

| Category | Components |
| --- | --- |
| **AI Components (`components/ai/`)** | Pure-Rust BYOK AI suite powered by **`rig-core` + `rig-agent` + `rmcp` (`rig-rmcp`)**: unified `AskAi` & draggable/resizable `AskAiOverlay` (`AskAiState`, including `AskAiState::build_with_ai` + non-blocking `build_with_ai_overlay` for live in-place editing of host editors like `FreeformCanvas`), `AiConnectionRegistry` (`connection_key`), `rig-core` providers (**OpenAI Responses API**, **Anthropic Messages**, **Google Gemini**, **OpenAI-compatible Chat Completions**, custom `ProviderPreset`, offline `SimulatedModel`), official `rmcp` + `rig-rmcp` `McpClient` (**Streamable HTTP** + native desktop **Stdio**), `rig-agent` multi-step `run_agent_turn` loop (`ByokBudgetHook`, background `ClientTool`s, live foreground `AppTool::for_entity` + `context_provider`, 6 `workspace__*` file tools + sandboxed `BuiltinScriptRunner`), branching conversation trees (`< 2 / 3 >`), multimodal image attachments, pure-GPUI GFM `Markdown` renderer, and extensible generative `AnswerComponentRegistry` (`stat-card`, `callout`, `bar-chart`, `sparkline`, `data-table`, `key-value`, `badge-list`). See [docs/ai-components.md](docs/ai-components.md). |
| **Native Desktop & Windows App Builder** | **Platform primitives**: `WindowOptions` (`always_on_top`, `skip_taskbar`, `non_activating`, `click_through`, `hide_window_border`, `window_corner_preference`, `backdrop_corner_radius`, `backdrop_blur_radius`), live `Window` controls (`show`/`show_no_activate`/`hide`/`hide_immediately`/`set_cloaked`/`paint_now`/`set_click_through`/`set_backdrop_blur_radius`/`capture_image`/`show_native_context_menu`), `GlobalHotKey` & `GlobalHotKeyManager`, System Tray (`set_tray_icon`, `TrayMenuItem`, `on_tray_event`), Launch at Login (`AutostartOptions`, `set_launch_at_login`), `SingleInstance`, `window_placement` multi-monitor docking & Spotlight placement, `OpenPromptOptions`/`SavePromptOptions` filtered dialogs, `gpui_util::atomic_write`. **Components**: `WindowTitleBar`, `StatusBar`, `KeyHint`, `ShortcutRecorder`, `NativeTooltipManager` (detached click-through OS tooltips), `ConfirmButton`, `CompactStepper`, `StatusIndicator`, `TrendBadge`, `StringList`, `ColorSwatchPopover`, `FloatingToolbar`, `SimulatedBackdrop`, `LazyElement` (`lazy`), `Theme::dark_glass()` / `Theme::light_glass()`, standalone `CommandPalette` (`quick_slots`, `CommandAction`, camelCase acronym `fuzzy_match_detailed`), `ToastVariant::Busy` & `ToastAction`. See [docs/windows-native-app-builder.md](docs/windows-native-app-builder.md). |
| **Theme & Transitions** | `palette` (all 22 hues × 11 shades `50..=950`), `Theme::light()` / `Theme::dark()` / `Theme::dark_glass()` / `Theme::light_glass()`, fluent `Theme` customizers (`with_accent`, `with_vibrancy`, `surface_with_opacity`, `edge_for_opacity`, `with_surfaces`, `with_borders`, `with_sidebar_colors`, `with_toggle_colors`, `with_focus_ring_width`, `with_elevation_shadows`, `with_font_family`, `with_chart_colors`), `ThemeMode`, `ThemePreference` (`Light` / `System` / `Dark`), `ThemeSwitcher`, `SegmentedControl`, GPU-composited `ViewTransition` (`Window::start_view_transition` click-origin radial reveal & cross-fade), `ComponentSize`, `ActiveTheme`, `init_components()` |
| **Analytics & Charts (`components/charts/`)** | Composable `CartesianPlot` (`XAxis::category` / `XAxis::continuous`, `YAxis` with explicit/tight/clamped domains, unified hover/drag-brush controller, `FxHasher` geometry cache, auto-aggregated + extensible `tooltip_rows` / `tooltip_builder`) with orthogonal stackable layers (`StackedBarLayer`, `LineLayer` with solid/dashed strokes, monotone smoothing, area gradients, surface haloing over bars, gap splitting & endpoint callouts, `ReferenceLine`, `ReferenceBand`), `ChartCard` (interactive legend toggle, header action slot, collapsible `"Show data table"`), `Sparkline` (`Area`/`Line`/`Bars` + haloed dashed baseline) & `GroupedSparkline`, `BreakdownList` (additive share & `ratio_mode`, subtitles, deltas, spring widths & click-to-narrow), `StatGroup` / `StatItem` / `StatDelta` |
| **Time Range & Filtering (`components/time_range/`)** | `TimeRangePicker` (`Quick Select` with `Last N unit` + presets, `Start and End` inputs with embedded `CalendarGrid`, `<` / `>` steppers, `🔍-` zoom out), zero-dep `CalendarDate` & `DateRange`, structured faceted `FilterBar` (`=`, `!=`, `is one of` multi-select checklist with live facet counts & segmented chips) |
| **Layout** | `Container` (`MaxWidth`), `DragDropContainer` / `DragDropState` (drag items between containers and reorder them, with a half-transparent landing preview), `FreeformCanvas` / `CanvasPaletteItem` / `FreeformCanvasState` / `CanvasHistory` (production 2D freeform editor: 8-way item resize with `Shift` aspect lock, `Shift` orthogonal drag axis lock, canvas-frame resize, dot grid, magnetic sibling/center/gap guides, one-click `align_item`/`align_items`, transactional Undo/Redo, Copy/Cut/Paste/Duplicate, `Ctrl+[/]` z-order, `Tab` cycling, `Escape` gesture cancel, locked items & read-only mode, WYSIWYG glass border preservation, and an extensible right-click / `Shift+F10` context menu), `Stack` / `VStack` / `HStack` (with `.divided()`), `Grid`, `SplitPane` / `SplitPaneState` (drag-resizable horizontal/vertical splits with double-click reset & keyboard resize), `Card` (`CardHeader`, `CardTitle`, `CardDescription`, `CardContent`, `CardFooter`), `Section`, `SettingRow`, `Divider`, `PageHeader`, `AppShell`, `Sidebar`, `SidebarGroup`, `SidebarItem`, `Navbar`, `ScrollArea` |
| **Typography & Actions** | `Heading` (`H1`–`H5`), `Text` (`ui_text`, semantic tones, inline `code`, copyable), `EditableText` (click-to-edit inline text), `TruncatedText` (`"Show more"` toggle), `CodeBlock` (line numbers + copy), `Button` (`Primary`, `Secondary`, `Soft`, `SoftOutline`, `Outline`, `Ghost`, `Destructive`, `Link`, icon-only, pill, loading spinner, tooltip, custom `hover_bg`), `ButtonGroup`, `CopyButton`, `FloatButton` & `FloatButtonGroup` (corner speed-dial FAB) |
| **Inputs & Forms** | `Input` / `InputState` (single-line, full UTF-16 IME, selection, word nav, clipboard, undo/redo, horizontal scroll, password reveal toggle, clearable, prefix/suffix icons & addons), `AutoGrowInput` / `AutoGrowInputState` (starts at 1-line height matching `Input`, grows row-by-row on linebreaks up to `max_rows` then scrolls vertically, with optional `submit_on_enter` + `Shift+Enter` newline), `TextBox` / `TextArea` (`TextBoxState`, multi-line editing, optional `.auto_grow(min, max)` & `.submit_on_enter()`, Up/Down line nav, vertical scroll, counter), `NumberInput` / `NumberInputState`, `PinInput` / `PinInputState` (segmented OTP/PIN with auto-advance & paste), `TagInput` / `TagInputState` (free-form chip pills), `ColorPicker` / `ColorPickerState` (2D HSVA spectrum, hue/alpha sliders, hex & presets), `FileDropzone` / `FileDropzoneState` (native OS drag-and-drop & file picker), `DatePicker`, `TimePicker` (24h/12h), `Rating`, `FormState` & `FormRule` (declarative validation & dirty tracking), `Field`, `Label`, `Kbd` |
| **Selection & Hierarchies** | `Toggle` / `Switch`, `ToggleButton`, `ToggleGroup`, `Checkbox` (`Checked`, `Unchecked`, `Indeterminate`), `CheckboxGroup`, `RadioGroup` (`List` & `Cards`), `Select`, `ComboBox`, `TreeSelect` (searchable hierarchical dropdown), `Cascader` (Miller-column path picker), `Transfer` (dual-column shuttle with search & checkboxes), `Slider` & `RangeSlider` (dual-thumb `[start, end]` + `SliderMark` ticks) |
| **Overlays & Menus** | `FloatingWindow` / `FloatingWindowState` (reusable in-app floating windows with 8-way edge & corner resize, draggable titlebar, maximize/restore, viewport clamping, optional modal scrim, and built-in or custom chrome — also powering `AskAiOverlay` & `build_with_ai_overlay`), `CommandPalette` (`⌘K` fuzzy command runner), `Drawer` (`Left`/`Right`/`Top`/`Bottom` slide-over sheet with GPU blur scrim), `Popconfirm` (anchored inline confirmation popover), `Tour` & `tour_target` (interactive spotlight walkthrough), `ImagePreview` (GPU-blurred zoom/rotate lightbox gallery), `ContextMenu` (right-click floating menu), `Dialog` & `DialogWindow` (dedicated native OS `WindowKind::Dialog`), `Popover`, `DropdownMenu`, `Tooltip`, `Toast` / `ToastHost` & `NativeToastManager` (`WindowKind::PopUp`), `OverlayPresentation` |
| **Data & Navigation** | `Tree` (hierarchical tree view with tri-state cascading checkboxes, search & keyboard nav), `Table` (sortable columns, checkbox multi-select with tri-state header, expandable detail rows, sticky-header scrolling) & `VirtualTable` (`uniform_list` 10k+ rows), `List` & `VirtualList`, `Descriptions` (multi-column key-value grid with spans & copy), `Timeline` (`Left` / `Alternate` stream + pending node), `Menu` (`Vertical` collapsible submenus & `Horizontal` menu bar), `Carousel`, `Tabs`, `Stepper`, `Accordion` / `Disclosure`, `Breadcrumb`, `Pagination` |
| **Loading, Blur & Feedback** | `BlurContainer` (GPU backdrop blur wrapper on DirectX, Metal and WGPU, with crisp unblurred foreground overlays & pointer blocking), `LoadingOverlay` (blurred container + spinner/progress/cancel overlay), `Spinner` (5 variants: `Ring`, `Loader`, `Dots`, `Bars`, `Pulse` with zero-relayout GPU `SyncedWave`), `ProgressBar`, `ProgressRing`, `ResultView` (`Success`/`Error`/`Warning`/`Info`/`403`/`404`/`500`), `QRCode` (pure-Rust ISO/IEC 18004 scannable QR renderer), `Watermark`, `Skeleton`, `Alert`, `Badge`, `Avatar` & `AvatarGroup`, `EmptyState` |

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

- [**AI Components (BYOK Ask AI, Build with AI, Rig Agent & rmcp)**](docs/ai-components.md) — Production `rig-core` providers (OpenAI Responses API, Anthropic, Gemini, OpenAI-compatible), official `rmcp` + `rig-rmcp` Streamable HTTP & Stdio `McpClient`, `rig-agent` multi-step `run_agent_turn`, foreground `AppTool::for_entity` live editor bindings, `ConversationWorkspace`, GFM `Markdown`, generative `AnswerComponentRegistry`, and `AskAi` / draggable `AskAiOverlay` / `build_with_ai_overlay`.
- [**Extended Desktop Component Catalog (Tier 1–3)**](docs/complete-component-catalog.md) — `Tree`, `Popconfirm`, `ColorPicker`, `FileDropzone`, `Descriptions`, `Timeline`, `VirtualTable`, `RangeSlider`, `DatePicker`/`TimePicker`, `VirtualList`, `Typography`, `ResultView`, `Transfer`, `TagInput`, `ImagePreview`, `TreeSelect`, `Cascader`, `Tour`, `Menu`, `Carousel`, `FloatButton`, `Watermark`, `QRCode`, and `FormState`.
- [**Interactive, Loading & Blurred Container Components**](docs/interactive-and-loading-components.md) — `BlurContainer`, `LoadingOverlay` (GPU backdrop blur on DirectX, Metal and WGPU), 5-variant `Spinner`, `ProgressRing`, `CommandPalette` (`⌘K`), `Drawer`, `ContextMenu`, `SplitPane`, `NumberInput`, `Stepper`, `Rating`, `CopyButton`, and `PinInput`.
- [**GPU View Transitions & Radial Theme Switching**](docs/view-transitions.md) — Compositor snapshot/shader pipeline, `Window::start_view_transition`, `ViewTransition`, `ThemeSwitcher`, reduced-motion & fallback semantics.
- [**Theming & Component Stylability**](docs/theming-and-stylability.md) — Four-tier `Theme` token hierarchy, fluent builders (`with_accent`, `with_surfaces`, `with_borders`, `with_elevation_shadows`, `with_focus_ring_width`), 100% `Styled` passthrough, `Section` / `SettingRow`, and per-component overrides.
- [**Analytics, Charts, Time Range Picker & Faceted Filtering**](docs/charts-and-analytics.md) — `ChartCard`, `StackedBarChart`, `LineChart`, `BreakdownList`, drag-brush selection, interactive legends, `TimeRangePicker`, `CalendarGrid`, `FilterBar`, `StatGroup`, `SegmentedControl`, and component coordination recipes.
- [**Dual-Mode Dialogs & Toasts (In-Window vs. Native OS Windows)**](docs/native-overlays.md) — Side-by-side comparison of `Dialog` / `ToastHost` vs. `DialogWindow` (`WindowKind::Dialog`) and `NativeToastManager` (`WindowKind::PopUp`), platform behavior across Linux/macOS/Windows, and API recipes.
- [**Performance Benchmarks & Optimizations**](docs/performance.md) — Reproducible `--release` benchmark commands, measured before/after metrics, and 120 FPS authoring guidelines.

## Interactive Developer Playground, Tests & Benchmarks

A dedicated developer playground app lives in [`crates/gpui_playground`](crates/gpui_playground) with an interactive Analytics & Charts dashboard, live prop workbenches, auto-generated copyable Rust snippets, a live GPUI event inspector drawer, brand accent & click-origin radial Light/System/Dark theme switching, a paginated 2,092-icon Lucide explorer, and a 242-swatch palette explorer:

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

//! Overview landing page with quick stats, live mini-demo, and direct jump cards.

use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideIcon};
use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px};

use crate::state::{Page, PlaygroundApp};

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let mode_label = match theme.mode {
        ThemeMode::Light => "Light",
        ThemeMode::Dark => "Dark",
    };

    let jump_cards: &[(Page, LucideIcon, &str, &str)] = &[
        (
            Page::AiComponents,
            LucideIcon::Bot,
            "AI Components",
            "BYOK Ask AI, Build with AI, MCP client (HTTP & Stdio), pure-Rust agent loop, Markdown & generative Answer Components.",
        ),
        (
            Page::Analytics,
            LucideIcon::ChartColumnStacked,
            "Analytics & Charts",
            "StackedBarChart, LineChart, Sparkline, BreakdownList, FilterBar, TimeRangePicker, StatGroup & radial ThemeSwitcher.",
        ),
        (
            Page::NativeDesktop,
            LucideIcon::AppWindow,
            "Desktop & Windows",
            "Floating translucent windows, standalone launcher, ShortcutRecorder, WindowTitleBar, StatusBar, CompactStepper & OS hooks.",
        ),
        (
            Page::Buttons,
            LucideIcon::MousePointerClick,
            "Buttons & Groups",
            "7 variants, 5 sizes, spinners, icons, ButtonGroup & ToggleGroup with live code generator.",
        ),
        (
            Page::Inputs,
            LucideIcon::TextCursorInput,
            "Input & TextBox",
            "Single-line IME input (masks, addons, clearable) + multi-line TextBox with undo/redo & counter.",
        ),
        (
            Page::Controls,
            LucideIcon::ToggleRight,
            "Toggles, Checks & Radios",
            "Switches with thumb icons, indeterminate checkbox tree, Radio Cards & Slider.",
        ),
        (
            Page::Selection,
            LucideIcon::ListFilter,
            "Select & ComboBox",
            "Keyboard-navigable grouped Select + searchable single & multi-tag ComboBox with creation.",
        ),
        (
            Page::LayoutNavigation,
            LucideIcon::LayoutDashboard,
            "Layout, Tabs & Nav",
            "Container, Grid, VStack/HStack, Cards, Dividers, 3 Tab variants, Accordion, Breadcrumb & Pagination.",
        ),
        (
            Page::OverlaysData,
            LucideIcon::Layers,
            "Overlays, Table & Feedback",
            "Modal Dialog, DropdownMenu, Popover, Toasts, Tooltips, sortable Table, Badges, Avatars & Progress.",
        ),
        (
            Page::LucideIcons,
            LucideIcon::Shapes,
            "Lucide Icons (2,092)",
            "Search, filter by category, tweak stroke & spin, and copy exact Rust enum snippets.",
        ),
        (
            Page::Palette,
            LucideIcon::Palette,
            "Color Palette (242)",
            "Inspect all 22 hues × 11 shades (50–950) and click to copy constants.",
        ),
    ];

    let mut tiles = grid(2).spacing(px(14.));
    for (idx, &(target, icon, title, desc)) in jump_cards.iter().enumerate() {
        tiles = tiles.child(
            div()
                .id(("jump-card", idx))
                .flex()
                .flex_row()
                .items_start()
                .gap(px(14.))
                .p(px(16.))
                .rounded(theme.radius_xl)
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .shadow_2xs()
                .cursor_pointer()
                .hover(|s| {
                    s.border_color(theme.primary)
                        .bg(theme.primary_soft.opacity(0.3))
                })
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.page = target;
                    this.log_event("Nav", format!("Opened {}", target.title()), cx);
                    cx.notify();
                }))
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .flex_shrink_0()
                        .w(px(40.))
                        .h(px(40.))
                        .rounded(theme.radius_lg)
                        .bg(theme.primary_soft)
                        .child(Icon::new(icon).size(IconSize::Lg).color(theme.primary)),
                )
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flex_1()
                        .min_w_0()
                        .child(
                            h_stack()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(15.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.foreground)
                                        .child(title),
                                )
                                .child(
                                    Icon::new(LucideIcon::ArrowRight)
                                        .size(IconSize::Sm)
                                        .color(theme.muted_foreground),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(theme.muted_foreground)
                                .child(desc),
                        ),
                ),
        );
    }

    v_stack()
        .spacing(px(22.))
        .child(
            page_header("GPUI Component Playground")
                .description(
                    "Interactive workbench for every GPUI component and all 2,092 bundled Lucide SVG icons.",
                )
                .action(
                    button("ov-open-icons")
                        .secondary()
                        .leading_icon(LucideIcon::Shapes)
                        .label("Browse 2,092 Icons")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.page = Page::LucideIcons;
                            cx.notify();
                        })),
                )
                .action(
                    button("ov-open-log")
                        .primary()
                        .leading_icon(LucideIcon::Terminal)
                        .label(if app.show_event_log {
                            "Hide Event Inspector"
                        } else {
                            "Open Event Inspector"
                        })
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.show_event_log = !this.show_event_log;
                            cx.notify();
                        })),
                ),
        )
        .child(
            grid(4)
                .spacing(px(14.))
                .child(stat_card(
                    "Components",
                    "35+",
                    "Primitives",
                    LucideIcon::Component,
                    cx,
                ))
                .child(stat_card(
                    "Lucide SVGs",
                    LucideIcon::COUNT.to_string(),
                    "Zero-copy static icons",
                    LucideIcon::Shapes,
                    cx,
                ))
                .child(stat_card(
                    "Color Tokens",
                    "242",
                    "22 hues × 11 shades",
                    LucideIcon::Palette,
                    cx,
                ))
                .child(stat_card(
                    "Active Theme",
                    format!("{mode_label} · {}", app.accent.name()),
                    "Switch live in navbar",
                    LucideIcon::SunMoon,
                    cx,
                )),
        )
        .child(
            alert("Pro tip: Live Theme, Accent & Event Inspector")
                .info()
                .description(
                    "Try clicking the accent color dots or Light/Dark button in the top navbar, open the Event Inspector to watch live GPUI events, or press Cmd/Ctrl+K to filter pages.",
                ),
        )
        .child(tiles)
}

fn stat_card(
    label: &'static str,
    value: impl Into<gpui::SharedString>,
    hint: &'static str,
    icon: LucideIcon,
    cx: &gpui::App,
) -> impl IntoElement {
    let theme = cx.theme();
    card().padded().child(
        v_stack()
            .spacing(px(8.))
            .child(
                h_stack()
                    .justify_between()
                    .child(
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.muted_foreground)
                            .child(label),
                    )
                    .child(Icon::new(icon).size(IconSize::Md).color(theme.primary)),
            )
            .child(
                div()
                    .text_size(px(22.))
                    .font_weight(FontWeight::BOLD)
                    .text_color(theme.foreground)
                    .child(value.into()),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(hint),
            ),
    )
}

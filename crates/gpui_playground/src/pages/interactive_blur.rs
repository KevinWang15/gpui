//! Interactive & Blur Components workbench:
//! `BlurContainer`, `LoadingOverlay`, multi-variant `Spinner`, `ProgressRing`,
//! `CommandPalette`, `Drawer`, `ContextMenu`, drag-resizable `SplitPane`,
//! `NumberInput`, `Stepper`, `Rating`, `CopyButton`, and segmented `PinInput`.

use gpui::components::palette;
use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideIcon};
use gpui::{Context, FontWeight, Hsla, div, prelude::*, px};

use crate::code_box::code_snippet;
use crate::state::PlaygroundApp;

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let h_ratio = (app.split_horizontal.read(cx).ratio() * 100.0).round() as i32;
    let v_ratio = (app.split_vertical.read(cx).ratio() * 100.0).round() as i32;
    let ring_pct = app.loading_demo_progress;
    let otp_val = app.otp_input.read(cx).value();
    let otp_complete = app.otp_input.read(cx).is_complete();

    // 1. LoadingOverlay & BlurContainer Workbench
    let loading_card_inner = card()
        .flat()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("Cluster Telemetry & Deploy Queue"))
                            .child(card_description(
                                "Everything inside this container blurs smoothly on the GPU while loading.",
                            )),
                    )
                    .child(badge("us-east-1").color(BadgeColor::Green).dot(true)),
            ),
        )
        .child(
            card_content()
                .child(
                    stat_group("blur-demo-stats")
                        .item(
                            StatItem::new("Requests / sec", "14,280")
                                .delta(StatDelta::positive("+12.4%")),
                        )
                        .item(
                            StatItem::new("P99 Latency", "24.8 ms")
                                .delta(StatDelta::positive("-3.1 ms")),
                        )
                        .item(
                            StatItem::new("Active Workers", "64 / 64")
                                .delta(StatDelta::neutral("100%")),
                        ),
                )
                .child(
                    h_stack()
                        .justify_between()
                        .pt(px(4.))
                        .child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child(format!(
                                    "Clicks reaching button behind blur: {}",
                                    app.loading_demo_clicks_behind
                                )),
                        )
                        .child(
                            button("behind-blur-btn")
                                .secondary()
                                .size(ComponentSize::Sm)
                                .leading_icon(LucideIcon::MousePointerClick)
                                .label("Click Behind Overlay")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.loading_demo_clicks_behind += 1;
                                    this.log_event(
                                        "LoadingOverlay",
                                        format!(
                                            "underlying button clicked ({})",
                                            this.loading_demo_clicks_behind
                                        ),
                                        cx,
                                    );
                                })),
                        ),
                ),
        );

    let mut wrapped_loading = loading_overlay(app.loading_demo_active)
        .animated("demo-loading-overlay")
        .blur(px(app.loading_demo_blur_px))
        .spinner_variant(app.loading_demo_variant)
        .spinner_size(IconSize::Xl)
        .card(app.loading_demo_card)
        .label("Syncing telemetry shards...")
        .description("GPU Dual-Kawase blur blocks clicks underneath")
        .on_cancel(cx.listener(|this, _, _, cx| {
            this.loading_demo_active = false;
            this.log_event("LoadingOverlay", "cancelled by user", cx);
        }))
        .rounded(theme.radius_xl)
        .child(loading_card_inner);

    if app.loading_demo_show_progress {
        wrapped_loading = wrapped_loading.progress(app.loading_demo_progress);
    }

    let blur_privacy_demo = blur_container()
        .active(app.privacy_blurred)
        .animated("privacy-blur-box")
        .blur(px(10.))
        .rounded(theme.radius_xl)
        .overlay(
            v_stack()
                .items_center()
                .spacing(px(8.))
                .px(px(18.))
                .py(px(14.))
                .rounded(theme.radius_lg)
                .bg(theme.popover.opacity(0.92))
                .border_1()
                .border_color(theme.border)
                .shadow_md()
                .child(
                    Icon::new(LucideIcon::Lock)
                        .size(IconSize::Lg)
                        .color(theme.primary),
                )
                .child(
                    div()
                        .text_size(px(13.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .child("Sensitive API Credentials"),
                )
                .child(
                    button("reveal-secret-btn")
                        .primary()
                        .size(ComponentSize::Xs)
                        .leading_icon(LucideIcon::Eye)
                        .label("Reveal Secret")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.privacy_blurred = false;
                            this.log_event("BlurContainer", "revealed credentials", cx);
                        })),
                ),
        )
        .child(
            card().flat().padded().child(
                v_stack()
                    .spacing(px(10.))
                    .child(
                        h_stack()
                            .justify_between()
                            .child(card_title("Production Signing Key"))
                            .child(
                                button("relock-secret-btn")
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::EyeOff)
                                    .label("Blur Again")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.privacy_blurred = true;
                                        this.log_event("BlurContainer", "re-blurred", cx);
                                    })),
                            ),
                    )
                    .child(
                        div()
                            .p(px(10.))
                            .rounded(theme.radius_md)
                            .bg(theme.surface_muted)
                            .font_family("Lilex")
                            .text_size(px(12.5))
                            .child("sk_live_gpui_98f2c4a1d77b0e394f0192b8e"),
                    )
                    .child(
                        h_stack()
                            .justify_between()
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .text_color(theme.muted_foreground)
                                    .child("Rotated 2 hours ago · Scope: read/write"),
                            )
                            .child(
                                copy_button(
                                    "copy-secret-key",
                                    "sk_live_gpui_98f2c4a1d77b0e394f0192b8e",
                                )
                                .size(ComponentSize::Xs),
                            ),
                    ),
            ),
        );

    let blur_loading_section = card()
        .child(
            card_header()
                .bordered()
                .child(
                    h_stack()
                        .justify_between()
                        .child(
                            v_stack()
                                .spacing(px(2.))
                                .child(card_title("BlurContainer & LoadingOverlay"))
                                .child(card_description(
                                    "GPU Dual-Kawase frosted blur container (WGPU & Windows DX11) with crisp unblurred overlays and interaction blocking.",
                                )),
                        )
                        .child(
                            button("toggle-loading-demo")
                                .variant(if app.loading_demo_active {
                                    ButtonVariant::Destructive
                                } else {
                                    ButtonVariant::Primary
                                })
                                .size(ComponentSize::Sm)
                                .leading_icon(if app.loading_demo_active {
                                    LucideIcon::Square
                                } else {
                                    LucideIcon::Play
                                })
                                .label(if app.loading_demo_active {
                                    "Stop Loading"
                                } else {
                                    "Start Loading"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.loading_demo_active = !this.loading_demo_active;
                                    this.log_event(
                                        "LoadingOverlay",
                                        format!("loading -> {}", this.loading_demo_active),
                                        cx,
                                    );
                                })),
                        ),
                ),
        )
        .child(
            card_content()
                .child(
                    h_stack()
                        .flex_wrap()
                        .spacing(px(14.))
                        .child(
                            h_stack()
                                .spacing(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.muted_foreground)
                                        .child("Variant:"),
                                )
                                .children(
                                    [
                                        (SpinnerVariant::Ring, "Ring"),
                                        (SpinnerVariant::Loader, "Loader"),
                                        (SpinnerVariant::Dots, "Dots"),
                                        (SpinnerVariant::Bars, "Bars"),
                                        (SpinnerVariant::Pulse, "Pulse"),
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, (variant, name))| {
                                        button(("sp-var", i))
                                            .size(ComponentSize::Xs)
                                            .variant(if app.loading_demo_variant == variant {
                                                ButtonVariant::Primary
                                            } else {
                                                ButtonVariant::Secondary
                                            })
                                            .label(name)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.loading_demo_variant = variant;
                                                cx.notify();
                                            }))
                                    }),
                                ),
                        )
                        .child(
                            h_stack()
                                .spacing(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.muted_foreground)
                                        .child("Blur:"),
                                )
                                .children([0.0_f32, 4.0, 8.0, 14.0, 20.0].into_iter().enumerate().map(
                                    |(i, r)| {
                                        button(("blur-r", i))
                                            .size(ComponentSize::Xs)
                                            .variant(
                                                if (app.loading_demo_blur_px - r).abs() < 0.5 {
                                                    ButtonVariant::Soft
                                                } else {
                                                    ButtonVariant::Ghost
                                                },
                                            )
                                            .label(format!("{r:.0}px"))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.loading_demo_blur_px = r;
                                                cx.notify();
                                            }))
                                    },
                                )),
                        )
                        .child(
                            toggle("loading-card-pill", app.loading_demo_card)
                                .size(ComponentSize::Sm)
                                .label("Pill card")
                                .on_change(cx.listener(|this, v, _, cx| {
                                    this.loading_demo_card = *v;
                                    cx.notify();
                                })),
                        )
                        .child(
                            toggle("loading-show-prog", app.loading_demo_show_progress)
                                .size(ComponentSize::Sm)
                                .label("Progress bar")
                                .on_change(cx.listener(|this, v, _, cx| {
                                    this.loading_demo_show_progress = *v;
                                    cx.notify();
                                })),
                        ),
                )
                .child(wrapped_loading)
                .child(blur_privacy_demo)
                .child(code_snippet(
                    "copy-loading-overlay-code",
                    "LoadingOverlay & BlurContainer",
                    "loading_overlay(is_loading)\n    .blur(px(8.))\n    .spinner_variant(SpinnerVariant::Bars)\n    .card(true)\n    .label(\"Syncing telemetry shards...\")\n    .progress(72.0)\n    .on_cancel(cx.listener(|this, _, _, cx| ...))\n    .child(my_dashboard_card)".to_owned(),
                    cx.listener(|this, _, _, cx| {
                        this.log_event("CodeSnippet", "copied LoadingOverlay example", cx);
                    }),
                    cx,
                )),
        );

    // 2. Spinner Gallery & ProgressRing
    let spinners_and_rings = grid(2)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("Spinner Variants (Zero-Relayout GPU)"))
                        .child(card_description(
                            "SVG GPU rotation (Ring, Loader) and scene-level SyncedWave animations (Dots, Bars, Pulse).",
                        )),
                )
                .child(
                    card_content().child(
                        grid(3).spacing(px(12.)).children([
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(16.))
                                .rounded(theme.radius_lg)
                                .bg(theme.surface_muted.opacity(0.45))
                                .child(
                                    spinner()
                                        .variant(SpinnerVariant::Ring)
                                        .size(IconSize::Xl)
                                        .vertical()
                                        .label("Ring"),
                                ),
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(16.))
                                .rounded(theme.radius_lg)
                                .bg(theme.surface_muted.opacity(0.45))
                                .child(
                                    spinner()
                                        .variant(SpinnerVariant::Loader)
                                        .size(IconSize::Xl)
                                        .color(Hsla::from(palette::EMERALD_500))
                                        .vertical()
                                        .label("Loader"),
                                ),
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(16.))
                                .rounded(theme.radius_lg)
                                .bg(theme.surface_muted.opacity(0.45))
                                .child(
                                    spinner()
                                        .variant(SpinnerVariant::Dots)
                                        .size(IconSize::TwoXl)
                                        .color(Hsla::from(palette::VIOLET_500))
                                        .vertical()
                                        .label("Dots"),
                                ),
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(16.))
                                .rounded(theme.radius_lg)
                                .bg(theme.surface_muted.opacity(0.45))
                                .child(
                                    spinner()
                                        .variant(SpinnerVariant::Bars)
                                        .size(IconSize::Xl)
                                        .color(Hsla::from(palette::SKY_500))
                                        .vertical()
                                        .label("Bars"),
                                ),
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(16.))
                                .rounded(theme.radius_lg)
                                .bg(theme.surface_muted.opacity(0.45))
                                .child(
                                    spinner()
                                        .variant(SpinnerVariant::Pulse)
                                        .size(IconSize::Xl)
                                        .color(Hsla::from(palette::ROSE_500))
                                        .vertical()
                                        .label("Pulse"),
                                ),
                            div()
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(16.))
                                .rounded(theme.radius_lg)
                                .bg(theme.surface_muted.opacity(0.45))
                                .child(
                                    spinner()
                                        .variant(SpinnerVariant::Ring)
                                        .size(IconSize::Sm)
                                        .label("Inline"),
                                ),
                        ]),
                    ),
                ),
        )
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("ProgressRing (Circular Vector Gauge)"))
                        .child(card_description(
                            "Smooth rounded-cap PathBuilder arcs with optional spring interpolation.",
                        )),
                )
                .child(
                    card_content()
                        .child(
                            h_stack()
                                .justify_around()
                                .py(px(8.))
                                .child(
                                    progress_ring(ring_pct)
                                        .animated("ring-primary")
                                        .size(ComponentSize::Lg),
                                )
                                .child(
                                    progress_ring((ring_pct * 0.85).min(100.0))
                                        .animated("ring-success")
                                        .size(ComponentSize::Md)
                                        .color(theme.success),
                                )
                                .child(
                                    progress_ring(100.0 - ring_pct * 0.6)
                                        .animated("ring-warn")
                                        .size(ComponentSize::Md)
                                        .color(theme.warning)
                                        .center_label("CPU"),
                                )
                                .child(
                                    progress_ring(ring_pct)
                                        .size(ComponentSize::Sm)
                                        .color(Hsla::from(palette::ROSE_500))
                                        .show_percentage(false),
                                ),
                        )
                        .child(slider(&app.ring_slider)),
                ),
        );

    // 3. CommandPalette, Drawer & ContextMenu
    let ctx_area = context_menu(
        "pg-ctx-demo",
        div()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .w_full()
            .h(px(148.))
            .rounded(theme.radius_lg)
            .border_1()
            .border_dashed()
            .border_color(theme.input_border)
            .bg(theme.surface_muted.opacity(0.45))
            .child(
                Icon::new(LucideIcon::MousePointerClick)
                    .size(IconSize::Xl)
                    .color(theme.primary),
            )
            .child(
                div()
                    .text_size(px(13.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("Right-Click Anywhere Inside This Box"),
            )
            .child(
                div()
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(format!(
                        "Pinned: {} · Wrap lines: {}",
                        if app.ctx_pin_enabled { "On" } else { "Off" },
                        if app.ctx_wrap_lines { "On" } else { "Off" },
                    )),
            ),
    )
    .entries(vec![
        MenuEntry::Label("Canvas Actions".into()),
        MenuEntry::item_with_icon(
            "Open Command Palette",
            LucideIcon::Search,
            cx.listener(|this, _, window, cx| {
                this.command_palette.update(cx, |p, cx| p.open(window, cx));
            }),
        )
        .with_shortcut("⌘K"),
        MenuEntry::item_with_icon(
            "Open Inspector Drawer",
            LucideIcon::PanelRightOpen,
            cx.listener(|this, _, _, cx| {
                this.drawer_open = true;
                this.log_event("ContextMenu", "opened drawer", cx);
            }),
        )
        .with_shortcut("⌘I"),
        MenuEntry::Separator,
        MenuEntry::checkbox_item(
            "Pin to Workspace",
            app.ctx_pin_enabled,
            cx.listener(|this, _, _, cx| {
                this.ctx_pin_enabled = !this.ctx_pin_enabled;
                this.log_event(
                    "ContextMenu",
                    format!("pin -> {}", this.ctx_pin_enabled),
                    cx,
                );
            }),
        ),
        MenuEntry::checkbox_item(
            "Soft Wrap Lines",
            app.ctx_wrap_lines,
            cx.listener(|this, _, _, cx| {
                this.ctx_wrap_lines = !this.ctx_wrap_lines;
                this.log_event(
                    "ContextMenu",
                    format!("wrap_lines -> {}", this.ctx_wrap_lines),
                    cx,
                );
            }),
        ),
        MenuEntry::Separator,
        MenuEntry::destructive_item(
            "Reset Demo State",
            Some(LucideIcon::Trash),
            cx.listener(|this, _, _, cx| {
                this.loading_demo_clicks_behind = 0;
                this.rating_stars = 4;
                this.stepper_active = 1;
                this.log_event("ContextMenu", "reset demo state", cx);
            }),
        ),
    ]);

    let overlays_section = grid(2)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("CommandPalette (⌘K) & Slide-Over Drawer"))
                        .child(card_description(
                            "Fuzzy keyboard command runner and edge-docked sheet panel with GPU backdrop blur.",
                        )),
                )
                .child(
                    card_content()
                        .child(
                            h_stack()
                                .flex_wrap()
                                .spacing(px(8.))
                                .child(
                                    button("launch-cmd-palette")
                                        .primary()
                                        .leading_icon(LucideIcon::Search)
                                        .label("Open Command Palette (⌘K)")
                                        .on_click(cx.listener(|this, _, window, cx| {
                                            this.command_palette
                                                .update(cx, |p, cx| p.open(window, cx));
                                        })),
                                )
                                .child(
                                    button("launch-drawer-btn")
                                        .secondary()
                                        .leading_icon(LucideIcon::PanelRightOpen)
                                        .label("Open Drawer")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.drawer_open = true;
                                            this.log_event(
                                                "Drawer",
                                                format!("opened ({:?})", this.drawer_side),
                                                cx,
                                            );
                                        })),
                                ),
                        )
                        .child(
                            h_stack()
                                .spacing(px(6.))
                                .child(
                                    div()
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.muted_foreground)
                                        .child("Drawer side:"),
                                )
                                .children(
                                    [
                                        (DrawerSide::Right, "Right"),
                                        (DrawerSide::Left, "Left"),
                                        (DrawerSide::Top, "Top"),
                                        (DrawerSide::Bottom, "Bottom"),
                                    ]
                                    .into_iter()
                                    .enumerate()
                                    .map(|(i, (side, lbl))| {
                                        button(("drw-side", i))
                                            .size(ComponentSize::Xs)
                                            .variant(if app.drawer_side == side {
                                                ButtonVariant::Primary
                                            } else {
                                                ButtonVariant::Secondary
                                            })
                                            .label(lbl)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.drawer_side = side;
                                                cx.notify();
                                            }))
                                    }),
                                ),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .text_color(theme.muted_foreground)
                                .child("Press ⌘K / Ctrl+K anywhere in the Playground to jump pages, flip themes, or run actions."),
                        ),
                ),
        )
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("ContextMenu (Right-Click Floating Menu)"))
                        .child(card_description(
                            "Anchors right at the pointer position, auto-clamps inside the window, and supports checkmarks & shortcuts.",
                        )),
                )
                .child(card_content().child(ctx_area)),
        );

    // 4. Drag-Resizable SplitPane
    let split_section = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("SplitPane (Drag-to-Resize & Nested Splits)"))
                            .child(card_description(
                                "Drag either handle smoothly, use Arrow keys when handle is focused, or double-click handle to reset.",
                            )),
                    )
                    .child(
                        h_stack()
                            .spacing(px(8.))
                            .child(
                                badge(format!("H: {h_ratio}% / V: {v_ratio}%"))
                                    .color(BadgeColor::Indigo),
                            )
                            .child(
                                button("reset-splits")
                                    .ghost()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::RotateCcw)
                                    .label("Reset")
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        this.split_horizontal.update(cx, |s, cx| s.reset(cx));
                                        this.split_vertical.update(cx, |s, cx| s.reset(cx));
                                    })),
                            ),
                    ),
            ),
        )
        .child(
            card_content().child(
                div()
                    .w_full()
                    .h(px(230.))
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(
                        split_pane("demo-split-h", &app.split_horizontal)
                            .first(
                                div()
                                    .size_full()
                                    .p(px(16.))
                                    .bg(theme.surface_muted.opacity(0.4))
                                    .flex()
                                    .flex_col()
                                    .gap(px(8.))
                                    .child(
                                        h_stack()
                                            .spacing(px(6.))
                                            .child(
                                                Icon::new(LucideIcon::FolderTree)
                                                    .size(IconSize::Sm)
                                                    .color(theme.primary),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(format!("Explorer ({h_ratio}%)")),
                                            ),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(theme.muted_foreground)
                                            .child("Drag the vertical grip bar to resize this sidebar pane between 20% and 80%."),
                                    ),
                            )
                            .second(
                                split_pane("demo-split-v", &app.split_vertical)
                                    .first(
                                        div()
                                            .size_full()
                                            .p(px(14.))
                                            .bg(theme.surface)
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(13.))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(format!("Editor Pane ({v_ratio}%)")),
                                            )
                                            .child(
                                                div()
                                                    .font_family("Lilex")
                                                    .text_size(px(12.))
                                                    .text_color(theme.muted_foreground)
                                                    .child("split_pane(\"id\", &state).first(a).second(b)"),
                                            ),
                                    )
                                    .second(
                                        div()
                                            .size_full()
                                            .p(px(14.))
                                            .bg(theme.surface_muted.opacity(0.28))
                                            .flex()
                                            .flex_col()
                                            .gap(px(4.))
                                            .child(
                                                div()
                                                    .text_size(px(12.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .child(format!(
                                                        "Output Console ({}%)",
                                                        100 - v_ratio
                                                    )),
                                            )
                                            .child(
                                                div()
                                                    .font_family("Lilex")
                                                    .text_size(px(11.5))
                                                    .text_color(theme.success)
                                                    .child("✓ Built in 0.42s — 0 warnings"),
                                            ),
                                    ),
                            ),
                    ),
            ),
        );

    // 5. Interactive Stepper Wizard
    let mut deploy_steps = vec![
        StepItem::new("Configure").description("Select region & tier"),
        StepItem::new("Build Image").description("Compile release artifacts"),
        StepItem::new("Verify Health").description("Canary traffic checks"),
        StepItem::new("Promote Live").description("100% production rollout"),
    ];
    if app.stepper_error && app.stepper_active < deploy_steps.len() {
        deploy_steps[app.stepper_active] = deploy_steps[app.stepper_active]
            .clone()
            .status(StepStatus::Error);
    }

    let stepper_section = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("Stepper (Interactive Workflow & Wizard)"))
                            .child(card_description(
                                "Click any step directly or step through with Back / Next and toggle Error or Vertical layout.",
                            )),
                    )
                    .child(
                        h_stack()
                            .spacing(px(8.))
                            .child(
                                toggle("step-err", app.stepper_error)
                                    .size(ComponentSize::Sm)
                                    .label("Error state")
                                    .on_change(cx.listener(|this, v, _, cx| {
                                        this.stepper_error = *v;
                                        cx.notify();
                                    })),
                            )
                            .child(
                                toggle("step-vert", app.stepper_vertical)
                                    .size(ComponentSize::Sm)
                                    .label("Vertical")
                                    .on_change(cx.listener(|this, v, _, cx| {
                                        this.stepper_vertical = *v;
                                        cx.notify();
                                    })),
                            ),
                    ),
            ),
        )
        .child(
            card_content()
                .child(
                    stepper("demo-stepper", deploy_steps, app.stepper_active)
                        .orientation(if app.stepper_vertical {
                            StepperOrientation::Vertical
                        } else {
                            StepperOrientation::Horizontal
                        })
                        .on_step_click(cx.listener(|this, idx, _, cx| {
                            this.stepper_active = *idx;
                            this.log_event("Stepper", format!("jumped to step #{}", idx + 1), cx);
                        })),
                )
                .child(
                    h_stack()
                        .justify_end()
                        .spacing(px(8.))
                        .pt(px(6.))
                        .child(
                            button("step-back")
                                .secondary()
                                .size(ComponentSize::Sm)
                                .disabled(app.stepper_active == 0)
                                .label("Previous Step")
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.stepper_active = this.stepper_active.saturating_sub(1);
                                    cx.notify();
                                })),
                        )
                        .child(
                            button("step-next")
                                .primary()
                                .size(ComponentSize::Sm)
                                .label(if app.stepper_active >= 4 {
                                    "Restart"
                                } else {
                                    "Next Step"
                                })
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.stepper_active = (this.stepper_active + 1) % 5;
                                    this.stepper_error = false;
                                    cx.notify();
                                })),
                        ),
                ),
        );

    // 6. NumberInput, Rating, CopyButton & PinInput
    let interactive_controls_grid = grid(2)
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("NumberInput (Stepper + Keyboard)"))
                        .child(card_description(
                            "Click − / +, hold Shift for 10× steps, type directly, or press Up/Down arrows.",
                        )),
                )
                .child(
                    card_content()
                        .child(
                            field()
                                .label("Monthly Price (USD)")
                                .description("Range $0.00 – $9,999.00, step $5.00")
                                .child(number_input(&app.num_price)),
                        )
                        .child(
                            grid(2)
                                .spacing(px(12.))
                                .child(
                                    field()
                                        .label("UI Zoom Scale")
                                        .child(number_input(&app.num_scale)),
                                )
                                .child(
                                    field()
                                        .label("Synced Progress")
                                        .child(number_input(&app.num_budget)),
                                ),
                        ),
                ),
        )
        .child(
            card()
                .child(
                    card_header()
                        .bordered()
                        .child(card_title("PinInput (Segmented OTP) · Rating · CopyButton"))
                        .child(card_description(
                            "Click boxes & type digits (or paste 123456), hover stars to rate, and copy to clipboard.",
                        )),
                )
                .child(
                    card_content()
                        .child(
                            v_stack()
                                .spacing(px(8.))
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .child(
                                            div()
                                                .text_size(px(13.))
                                                .font_weight(FontWeight::MEDIUM)
                                                .child("Two-Factor Verification Code"),
                                        )
                                        .child(
                                            h_stack()
                                                .spacing(px(6.))
                                                .child(
                                                    badge(if otp_complete {
                                                        format!("Verified: {otp_val}")
                                                    } else {
                                                        format!("{}/6 digits", otp_val.len())
                                                    })
                                                    .color(if otp_complete {
                                                        BadgeColor::Green
                                                    } else {
                                                        BadgeColor::Gray
                                                    }),
                                                )
                                                .child(
                                                    toggle("otp-mask-tog", app.otp_masked)
                                                        .size(ComponentSize::Xs)
                                                        .label("Mask")
                                                        .on_change(cx.listener(|this, v, _, cx| {
                                                            this.otp_masked = *v;
                                                            this.otp_input.update(cx, |p, cx| {
                                                                p.set_masked(*v, cx);
                                                            });
                                                            cx.notify();
                                                        })),
                                                )
                                                .child(
                                                    button("otp-clear")
                                                        .ghost()
                                                        .size(ComponentSize::Xs)
                                                        .label("Clear")
                                                        .on_click(cx.listener(|this, _, _, cx| {
                                                            this.otp_input
                                                                .update(cx, |p, cx| p.clear(cx));
                                                        })),
                                                ),
                                        ),
                                )
                                .child(pin_input(&app.otp_input)),
                        )
                        .child(divider())
                        .child(
                            h_stack()
                                .justify_between()
                                .flex_wrap()
                                .spacing(px(12.))
                                .child(
                                    v_stack()
                                        .spacing(px(6.))
                                        .child(
                                            rating("demo-rating-stars", app.rating_stars)
                                                .show_value(true)
                                                .on_change(cx.listener(|this, val, _, cx| {
                                                    this.rating_stars = *val;
                                                    this.log_event(
                                                        "Rating(Stars)",
                                                        format!("{val} / 5"),
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            rating("demo-rating-hearts", app.rating_hearts)
                                                .icon(LucideIcon::Heart)
                                                .color(Hsla::from(palette::ROSE_500))
                                                .size(IconSize::Md)
                                                .show_value(true)
                                                .on_change(cx.listener(|this, val, _, cx| {
                                                    this.rating_hearts = *val;
                                                    this.log_event(
                                                        "Rating(Hearts)",
                                                        format!("{val} / 5"),
                                                        cx,
                                                    );
                                                })),
                                        ),
                                )
                                .child(
                                    h_stack()
                                        .spacing(px(8.))
                                        .child(
                                            copy_button("demo-copy-cmd", "cargo run -p gpui_playground")
                                                .label("Copy Command")
                                                .on_copy(cx.listener(|this, val, _, cx| {
                                                    this.log_event(
                                                        "CopyButton",
                                                        format!("copied \"{val}\""),
                                                        cx,
                                                    );
                                                })),
                                        )
                                        .child(
                                            copy_button("demo-copy-icon", "gpui::components")
                                                .icon_only()
                                                .variant(ButtonVariant::Soft),
                                        ),
                                ),
                        ),
                ),
        );

    // 0. Physical GPU Shader View Transitions Lab
    let active_fx = app.theme_transition_style;
    let accent = app.accent;
    let fx_cards: &[(ThemeTransitionStyle, LucideIcon, &str, &str)] = &[
        (
            ThemeTransitionStyle::ChromaticWarp,
            LucideIcon::Aperture,
            "Chromatic Warp",
            "Hyper-zoom & whip-spin tunnel + mirror tiling + prismatic RGB aberration",
        ),
        (
            ThemeTransitionStyle::PrismaticBlob,
            LucideIcon::Rainbow,
            "Prismatic Blob",
            "Organic double-blob radial reveal with chromatic aberration & prismatic rainbow gap",
        ),
        (
            ThemeTransitionStyle::Glitch,
            LucideIcon::Zap,
            "Glitch (0.3s)",
            "Asynchronous staggered digital blocks & horizontal RGB-split tears snapping into view",
        ),
        (
            ThemeTransitionStyle::BlurFade,
            LucideIcon::CloudFog,
            "Blur Fade (0.3s)",
            "0–0.2s optical blur-out, 0.1–0.2s opacity cross-fade, 0.1–0.3s blur-in",
        ),
        (
            ThemeTransitionStyle::Shockwave,
            LucideIcon::WavesHorizontal,
            "Shockwave",
            "Refractive optical ripple + RGB chromatic dispersion + caustic crest",
        ),
        (
            ThemeTransitionStyle::PageCurl,
            LucideIcon::BookOpen,
            "Page Curl",
            "3D cylindrical page turn + specular ridge + frosted vellum + cast shadow",
        ),
        (
            ThemeTransitionStyle::LiquidDissolve,
            LucideIcon::Droplets,
            "Liquid Dissolve",
            "Domain-warped FBM liquid melt + surface-tension refraction + meniscus rim",
        ),
        (
            ThemeTransitionStyle::RadialReveal,
            LucideIcon::CircleDot,
            "Radial Reveal",
            "Crisp circular wipe expanding from the exact pointer click coordinate",
        ),
        (
            ThemeTransitionStyle::CrossFade,
            LucideIcon::Blend,
            "Cross-Fade",
            "Smooth full-window opacity dissolve between live frames",
        ),
    ];

    let shader_transitions_section = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(3.))
                            .child(
                                h_stack()
                                    .spacing(px(8.))
                                    .child(card_title("Physical GPU Shader View Transitions"))
                                    .child(
                                        badge("1-Pass Fullscreen Shader")
                                            .color(BadgeColor::Indigo)
                                            .pill(true),
                                    ),
                            )
                            .child(card_description(
                                "Click any effect below to select it for the navbar theme switcher and immediately trigger a live Light ↔ Dark shader transition from your cursor.",
                            )),
                    )
                    .child(
                        badge(format!("Active: {}", active_fx.label()))
                            .color(BadgeColor::Green),
                    ),
            ),
        )
        .child(
            card_content().child(
                div()
                    .flex()
                    .flex_row()
                    .flex_wrap()
                    .gap(px(10.))
                    .children(fx_cards.iter().enumerate().map(|(idx, &(style, ic, title, desc))| {
                        let is_selected = active_fx == style;
                        div()
                            .id(("fx-transition-tile", idx))
                            .flex()
                            .flex_col()
                            .gap(px(6.))
                            .flex_1()
                            .min_w(px(176.))
                            .p(px(12.))
                            .rounded(theme.radius_lg)
                            .border_1()
                            .border_color(if is_selected {
                                theme.primary
                            } else {
                                theme.border
                            })
                            .bg(if is_selected {
                                theme.primary_soft.opacity(0.65)
                            } else {
                                theme.surface_muted.opacity(0.45)
                            })
                            .cursor_pointer()
                            .hover(|s| {
                                s.border_color(theme.primary)
                                    .bg(theme.primary_soft.opacity(0.45))
                            })
                            .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                                this.theme_transition_style = style;
                                let next_mode = match cx.theme().mode {
                                    ThemeMode::Light => ThemeMode::Dark,
                                    ThemeMode::Dark => ThemeMode::Light,
                                };
                                let next_pref = match next_mode {
                                    ThemeMode::Light => ThemePreference::Light,
                                    ThemeMode::Dark => ThemePreference::Dark,
                                };
                                let next_theme =
                                    accent.build_theme(next_mode).with_preference(next_pref);
                                Theme::switch_with_transition_style(
                                    next_theme,
                                    style,
                                    Some(ev.position()),
                                    window,
                                    cx,
                                );
                                this.log_event(
                                    "ViewTransition",
                                    format!("{} -> {next_mode:?}", style.label()),
                                    cx,
                                );
                            }))
                            .child(
                                h_stack()
                                    .justify_between()
                                    .child(
                                        h_stack()
                                            .spacing(px(8.))
                                            .child(
                                                Icon::new(ic)
                                                    .size(IconSize::Sm)
                                                    .color(if is_selected {
                                                        theme.primary_soft_foreground
                                                    } else {
                                                        theme.primary
                                                    }),
                                            )
                                            .child(
                                                div()
                                                    .text_size(px(13.5))
                                                    .font_weight(FontWeight::SEMIBOLD)
                                                    .text_color(theme.foreground)
                                                    .child(title),
                                            ),
                                    )
                                    .when(is_selected, |el| {
                                        el.child(
                                            Icon::new(LucideIcon::Check)
                                                .size(IconSize::Xs)
                                                .color(theme.primary),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted_foreground)
                                    .child(desc),
                            )
                    })),
            ),
        );

    // 0a. Component-to-Component GPU Transitions (`TransitionView`) Workbench
    let comp_fx = app.comp_transition_style;
    let card_a_idx = app.comp_transition_card_a % 3;
    let card_b_idx = app.comp_transition_card_b % 3;
    // Secondary card uses a complementary effect when firing both concurrently so the user can
    // see two independent GPU shaders animating side-by-side in the same frame!
    let card_b_fx = match comp_fx {
        TransitionStyle::PrismaticBlob => TransitionStyle::Glitch,
        TransitionStyle::Glitch => TransitionStyle::PrismaticBlob,
        TransitionStyle::ChromaticWarp => TransitionStyle::BlurFade,
        other => other,
    };

    let render_rich_slot_a = |idx: usize| -> gpui::AnyElement {
        match idx {
            0 => div()
                .w_full()
                .h(px(210.))
                .p(px(18.))
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(Hsla::from(palette::INDIGO_500).opacity(0.35))
                .bg(Hsla::from(palette::INDIGO_600).opacity(if theme.mode == ThemeMode::Dark { 0.24 } else { 0.10 }))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    h_stack()
                        .justify_between()
                        .child(
                            h_stack()
                                .spacing(px(8.))
                                .child(
                                    div()
                                        .w(px(34.))
                                        .h(px(34.))
                                        .rounded(theme.radius_md)
                                        .bg(Hsla::from(palette::INDIGO_500))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(Icon::new(LucideIcon::Activity).size(IconSize::Sm).color(Hsla::from(palette::WHITE))),
                                )
                                .child(
                                    v_stack()
                                        .spacing(px(1.))
                                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child("Live Cluster Metrics"))
                                        .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child("Component A · Click anywhere inside to swap")),
                                ),
                        )
                        .child(badge("99.98% SLA").color(BadgeColor::Indigo).pill(true)),
                )
                .child(
                    h_stack()
                        .spacing(px(10.))
                        .items_end()
                        .h(px(88.))
                        .children([42.0_f32, 68.0, 54.0, 84.0, 62.0, 76.0, 48.0, 86.0].into_iter().enumerate().map(|(i, h)| {
                            let c = [palette::INDIGO_500, palette::VIOLET_500, palette::CYAN_500, palette::EMERALD_500][i % 4];
                            div().flex_1().h(px(h)).rounded(px(5.)).bg(Hsla::from(c))
                        })),
                )
                .child(
                    h_stack()
                        .justify_between()
                        .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("p99 latency: 11.4 ms"))
                        .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(Hsla::from(palette::INDIGO_500)).child("48.2k req/s →")),
                )
                .into_any_element(),
            1 => div()
                .w_full()
                .h(px(210.))
                .p(px(18.))
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(Hsla::from(palette::EMERALD_500).opacity(0.40))
                .bg(Hsla::from(palette::SLATE_900))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    h_stack()
                        .justify_between()
                        .child(
                            h_stack()
                                .spacing(px(8.))
                                .child(
                                    div()
                                        .w(px(34.))
                                        .h(px(34.))
                                        .rounded(theme.radius_md)
                                        .bg(Hsla::from(palette::EMERALD_500))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(Icon::new(LucideIcon::Rocket).size(IconSize::Sm).color(Hsla::from(palette::WHITE))),
                                )
                                .child(
                                    v_stack()
                                        .spacing(px(1.))
                                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(Hsla::from(palette::WHITE)).child("Production Release v2.8"))
                                        .child(div().text_size(px(11.5)).text_color(Hsla::from(palette::SLATE_400)).child("Component B · Dark terminal card inside any theme")),
                                ),
                        )
                        .child(badge("DEPLOYED").color(BadgeColor::Green).pill(true)),
                )
                .child(
                    v_stack()
                        .spacing(px(8.))
                        .p(px(12.))
                        .rounded(theme.radius_md)
                        .bg(Hsla::from(palette::SLATE_950))
                        .font_family("Lilex")
                        .text_size(px(12.))
                        .child(div().text_color(Hsla::from(palette::EMERALD_400)).child("✓ wgpu shader pipeline compiled (0.4ms)"))
                        .child(div().text_color(Hsla::from(palette::SKY_400)).child("✓ 10 transition kernels verified across regions"))
                        .child(div().text_color(Hsla::from(palette::AMBER_400)).child("→ edge nodes synced: us-west, eu-central, ap-east")),
                )
                .child(
                    h_stack()
                        .justify_between()
                        .child(div().text_size(px(12.)).text_color(Hsla::from(palette::SLATE_400)).child("Build #4109 · SHA 746ac89"))
                        .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(Hsla::from(palette::EMERALD_400)).child("Click for Component C →")),
                )
                .into_any_element(),
            _ => div()
                .w_full()
                .h(px(210.))
                .p(px(18.))
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(Hsla::from(palette::ROSE_500).opacity(0.40))
                .bg(Hsla::from(palette::AMBER_500).opacity(if theme.mode == ThemeMode::Dark { 0.22 } else { 0.14 }))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    h_stack()
                        .justify_between()
                        .child(
                            h_stack()
                                .spacing(px(8.))
                                .child(
                                    div()
                                        .w(px(34.))
                                        .h(px(34.))
                                        .rounded(theme.radius_md)
                                        .bg(Hsla::from(palette::ROSE_500))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .child(Icon::new(LucideIcon::Palette).size(IconSize::Sm).color(Hsla::from(palette::WHITE))),
                                )
                                .child(
                                    v_stack()
                                        .spacing(px(1.))
                                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child("Prismatic Color Studio"))
                                        .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child("Component C · High-contrast swatches show off dispersion")),
                                ),
                        )
                        .child(badge("HDR P3").color(BadgeColor::Amber).pill(true)),
                )
                .child(
                    h_stack()
                        .spacing(px(8.))
                        .children(
                            [
                                palette::ROSE_500,
                                palette::ORANGE_500,
                                palette::AMBER_400,
                                palette::EMERALD_500,
                                palette::CYAN_500,
                                palette::INDIGO_500,
                                palette::VIOLET_500,
                            ]
                            .into_iter()
                            .map(|c| div().flex_1().h(px(72.)).rounded(theme.radius_md).bg(Hsla::from(c))),
                        ),
                )
                .child(
                    h_stack()
                        .justify_between()
                        .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Zero bleed outside rounded card bounds"))
                        .child(div().text_size(px(12.)).font_weight(FontWeight::SEMIBOLD).text_color(Hsla::from(palette::ROSE_500)).child("Back to Component A →")),
                )
                .into_any_element(),
        }
    };

    let render_rich_slot_b = |idx: usize| -> gpui::AnyElement {
        match idx {
            0 => div()
                .w_full()
                .h(px(210.))
                .p(px(18.))
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(Hsla::from(palette::SKY_500).opacity(0.35))
                .bg(Hsla::from(palette::SKY_500).opacity(if theme.mode == ThemeMode::Dark { 0.22 } else { 0.12 }))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    h_stack()
                        .justify_between()
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child("Audio Waveform Synth"))
                        .child(badge(format!("FX: {}", card_b_fx.label())).color(BadgeColor::Blue).pill(true)),
                )
                .child(
                    h_stack()
                        .spacing(px(6.))
                        .items_center()
                        .justify_center()
                        .h(px(90.))
                        .children([24.0_f32, 56.0, 78.0, 38.0, 86.0, 62.0, 90.0, 44.0, 72.0, 32.0, 64.0, 48.0].into_iter().map(|h| {
                            div().w(px(12.)).h(px(h)).rounded_full().bg(Hsla::from(palette::SKY_500))
                        })),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child("Independent TransitionView · Click to cycle or fire both at once"),
                )
                .into_any_element(),
            1 => div()
                .w_full()
                .h(px(210.))
                .p(px(18.))
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(Hsla::from(palette::VIOLET_500).opacity(0.45))
                .bg(Hsla::from(palette::VIOLET_950))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    h_stack()
                        .justify_between()
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(Hsla::from(palette::WHITE)).child("GPU Shader Telemetry"))
                        .child(badge("120 FPS").color(BadgeColor::Purple).pill(true)),
                )
                .child(
                    grid(2)
                        .child(
                            div()
                                .p(px(10.))
                                .rounded(theme.radius_md)
                                .bg(Hsla::from(palette::VIOLET_500).opacity(0.24))
                                .child(div().text_size(px(11.)).text_color(Hsla::from(palette::VIOLET_200)).child("SNAPSHOT VRAM"))
                                .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(Hsla::from(palette::WHITE)).child("380 KB")),
                        )
                        .child(
                            div()
                                .p(px(10.))
                                .rounded(theme.radius_md)
                                .bg(Hsla::from(palette::CYAN_500).opacity(0.24))
                                .child(div().text_size(px(11.)).text_color(Hsla::from(palette::CYAN_200)).child("SCISSOR PASS"))
                                .child(div().text_size(px(18.)).font_weight(FontWeight::BOLD).text_color(Hsla::from(palette::WHITE)).child("0.18 ms")),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(Hsla::from(palette::VIOLET_200))
                        .child("Only this card's rounded bounds are shaded — rest of UI stays live"),
                )
                .into_any_element(),
            _ => div()
                .w_full()
                .h(px(210.))
                .p(px(18.))
                .rounded(theme.radius_lg)
                .border_1()
                .border_color(Hsla::from(palette::EMERALD_500).opacity(0.40))
                .bg(Hsla::from(palette::EMERALD_500).opacity(if theme.mode == ThemeMode::Dark { 0.20 } else { 0.12 }))
                .flex()
                .flex_col()
                .justify_between()
                .child(
                    h_stack()
                        .justify_between()
                        .child(div().text_size(px(14.)).font_weight(FontWeight::SEMIBOLD).text_color(theme.foreground).child("Security Audit Passed"))
                        .child(badge("VERIFIED").color(BadgeColor::Green).pill(true)),
                )
                .child(
                    h_stack()
                        .spacing(px(12.))
                        .items_center()
                        .child(
                            div()
                                .w(px(56.))
                                .h(px(56.))
                                .rounded_full()
                                .bg(Hsla::from(palette::EMERALD_500))
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(Icon::new(LucideIcon::ShieldCheck).size(IconSize::Xl).color(Hsla::from(palette::WHITE))),
                        )
                        .child(
                            v_stack()
                                .spacing(px(4.))
                                .child(div().text_size(px(15.)).font_weight(FontWeight::BOLD).text_color(theme.foreground).child("All 10 Shaders Unified"))
                                .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child("Same WGSL & HLSL pipeline powers both full-window and component transitions.")),
                        ),
                )
                .child(
                    div()
                        .text_size(px(12.))
                        .text_color(Hsla::from(palette::EMERALD_600))
                        .child("Click again to loop back to Waveform Synth →"),
                )
                .into_any_element(),
        }
    };

    let component_transitions_section = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(3.))
                            .child(
                                h_stack()
                                    .spacing(px(8.))
                                    .child(card_title("Component-to-Component GPU Transitions"))
                                    .child(
                                        badge("TransitionView")
                                            .color(BadgeColor::Purple)
                                            .pill(true),
                                    ),
                            )
                            .child(card_description(
                                "Reuses the exact same GPU shaders scoped to individual rounded components. Click a card (or fire both concurrently) while the rest of the window stays 100% live.",
                            )),
                    )
                    .child(
                        button("comp-fx-both")
                            .primary()
                            .size(ComponentSize::Sm)
                            .leading_icon(LucideIcon::Sparkles)
                            .label("Transition Both")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.comp_transition_card_a = (this.comp_transition_card_a + 1) % 3;
                                this.comp_transition_card_b = (this.comp_transition_card_b + 1) % 3;
                                this.log_event(
                                    "TransitionView",
                                    format!("concurrent dual swap ({})", this.comp_transition_style.label()),
                                    cx,
                                );
                            })),
                    ),
            ),
        )
        .child(
            card_content()
                .child(
                    h_stack()
                        .flex_wrap()
                        .spacing(px(6.))
                        .children(TransitionStyle::ALL.into_iter().enumerate().map(|(i, style)| {
                            button(("comp-fx-pick", i))
                                .size(ComponentSize::Xs)
                                .variant(if comp_fx == style {
                                    ButtonVariant::Primary
                                } else {
                                    ButtonVariant::Secondary
                                })
                                .label(style.label())
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.comp_transition_style = style;
                                    this.comp_transition_card_a = (this.comp_transition_card_a + 1) % 3;
                                    this.log_event(
                                        "TransitionView",
                                        format!("card A -> {} ({})", this.comp_transition_card_a, style.label()),
                                        cx,
                                    );
                                }))
                        })),
                )
                .child(
                    grid(2)
                        .child(
                            div()
                                .id("comp-slot-a-hit")
                                .cursor_pointer()
                                .on_click(cx.listener(|this, _, _, cx| {
                                    this.comp_transition_card_a = (this.comp_transition_card_a + 1) % 3;
                                    this.log_event(
                                        "TransitionView",
                                        format!("card A swapped ({})", this.comp_transition_style.label()),
                                        cx,
                                    );
                                }))
                                .child(
                                    transition_view("pg-comp-transition-a", card_a_idx)
                                        .transition_style(comp_fx)
                                        .origin(TransitionOrigin::Pointer)
                                        .rounded(theme.radius_lg)
                                        .w_full()
                                        .child(render_rich_slot_a(card_a_idx)),
                                ),
                        )
                        .child(
                            div()
                                .id("comp-slot-b-hit")
                                .cursor_pointer()
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.comp_transition_card_b = (this.comp_transition_card_b + 1) % 3;
                                    this.log_event(
                                        "TransitionView",
                                        format!("card B swapped ({})", card_b_fx.label()),
                                        cx,
                                    );
                                }))
                                .child(
                                    transition_view("pg-comp-transition-b", card_b_idx)
                                        .transition_style(card_b_fx)
                                        .origin(TransitionOrigin::Pointer)
                                        .rounded(theme.radius_lg)
                                        .w_full()
                                        .child(render_rich_slot_b(card_b_idx)),
                                ),
                        ),
                ),
        );

    // 0b. Composable Optical LiquidGlass Lens, Floating Bar & Progressive Blur Lab
    let glass_refraction = app.glass_refraction_slider.read(cx).value();
    let glass_dispersion = app.glass_dispersion_slider.read(cx).value();
    let glass_frost = app.glass_frost_slider.read(cx).value();
    let glass_prog = app.glass_progressive_mode;
    let bar_clicks = app.glass_bar_clicks;

    let prog_options: &[(ProgressiveBlur, &str)] = &[
        (ProgressiveBlur::BottomToTop, "Header Fade"),
        (ProgressiveBlur::TopToBottom, "Footer Fade"),
        (ProgressiveBlur::Edges, "Clear Center"),
        (ProgressiveBlur::None, "Uniform"),
    ];

    let bar_colors = [
        palette::INDIGO_500,
        palette::EMERALD_500,
        palette::AMBER_500,
        palette::ROSE_500,
        palette::SKY_500,
        palette::VIOLET_500,
    ];

    let liquid_glass_section = card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(3.))
                            .child(
                                h_stack()
                                    .spacing(px(8.))
                                    .child(card_title("Composable LiquidGlass — Optical Lens, Floating Bar & Progressive Blur"))
                                    .child(
                                        badge("GPU Snell + Prism + Fresnel")
                                            .color(BadgeColor::Blue)
                                            .pill(true),
                                    ),
                            )
                            .child(card_description(
                                "Drag either LiquidGlass container freely over live typography & charts — or click the buttons inside the floating glass bar. Everything is a normal ParentElement + Styled container.",
                            )),
                    )
                    .child(
                        button("lg-reset-pos")
                            .outline()
                            .size(ComponentSize::Xs)
                            .leading_icon(LucideIcon::RotateCcw)
                            .label("Reset Positions")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.glass_lens_drag.update(cx, |s, cx| {
                                    s.set_position(gpui::point(px(36.), px(82.)), cx);
                                });
                                this.glass_bar_drag.update(cx, |s, cx| {
                                    s.set_position(gpui::point(px(290.), px(210.)), cx);
                                });
                            })),
                    ),
            ),
        )
        .child(
            card_content()
                .child(
                    grid(3)
                        .spacing(px(14.))
                        .child(slider(&app.glass_refraction_slider))
                        .child(slider(&app.glass_dispersion_slider))
                        .child(slider(&app.glass_frost_slider)),
                )
                .child(
                    h_stack()
                        .spacing(px(8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child("Header Progressive Blur:"),
                        )
                        .children(prog_options.iter().enumerate().map(|(i, &(mode, lbl))| {
                            let active = glass_prog == mode;
                            button(("lg-prog", i))
                                .size(ComponentSize::Xs)
                                .variant(if active {
                                    ButtonVariant::Primary
                                } else {
                                    ButtonVariant::Outline
                                })
                                .label(lbl)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.glass_progressive_mode = mode;
                                    cx.notify();
                                }))
                        })),
                )
                .child(
                    liquid_glass_stage("pg-liquid-glass-stage")
                        .track(&app.glass_lens_drag)
                        .track(&app.glass_bar_drag)
                        .w_full()
                        .h(px(340.))
                        .rounded(theme.radius_xl)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.background)
                        // Rich live UI scene underneath the optical glass containers
                        .child(
                            div()
                                .flex()
                                .flex_col()
                                .justify_between()
                                .size_full()
                                .p(px(20.))
                                .pt(px(58.))
                                .gap(px(14.))
                                .child(
                                    h_stack()
                                        .justify_between()
                                        .items_start()
                                        .child(
                                            v_stack()
                                                .spacing(px(4.))
                                                .child(
                                                    div()
                                                        .text_size(px(20.))
                                                        .font_weight(FontWeight::BOLD)
                                                        .text_color(theme.foreground)
                                                        .child("Live Subpixel Typography & GPU Telemetry"),
                                                )
                                                .child(
                                                    div()
                                                        .text_size(px(13.))
                                                        .text_color(theme.muted_foreground)
                                                        .child("Drag the magnifying lens or floating toolbar directly across these headings, colored badges, and bars to inspect real-time Snell refraction & RGB prism dispersion."),
                                                ),
                                        )
                                        .child(
                                            h_stack()
                                                .spacing(px(6.))
                                                .child(badge("120 Hz").color(BadgeColor::Green).dot(true))
                                                .child(badge("0-Frame Lag").color(BadgeColor::Indigo))
                                                .child(badge("VRAM Pass").color(BadgeColor::Amber)),
                                        ),
                                )
                                // Crisp multi-color live bar chart + KPI cards underneath
                                .child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_end()
                                        .gap(px(16.))
                                        .flex_1()
                                        .child(
                                            div()
                                                .flex()
                                                .flex_row()
                                                .items_end()
                                                .justify_between()
                                                .flex_1()
                                                .h(px(172.))
                                                .p(px(14.))
                                                .rounded(theme.radius_lg)
                                                .bg(theme.surface)
                                                .border_1()
                                                .border_color(theme.border)
                                                .children((0..16).map(|b| {
                                                    let h_px = 38.0 + ((b * 43 + 19) % 108) as f32;
                                                    let col: Hsla =
                                                        bar_colors[b % bar_colors.len()].into();
                                                    div()
                                                        .w(px(18.))
                                                        .h(px(h_px))
                                                        .rounded_t(px(5.))
                                                        .bg(col)
                                                })),
                                        )
                                        .child(
                                            v_stack()
                                                .w(px(220.))
                                                .spacing(px(10.))
                                                .child(
                                                    div()
                                                        .p(px(12.))
                                                        .rounded(theme.radius_lg)
                                                        .bg(theme.surface)
                                                        .border_1()
                                                        .border_color(theme.border)
                                                        .child(
                                                            v_stack()
                                                                .spacing(px(4.))
                                                                .child(
                                                                    div()
                                                                        .text_size(px(11.5))
                                                                        .text_color(theme.muted_foreground)
                                                                        .child("OPTICAL PASS BUDGET"),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(22.))
                                                                        .font_weight(FontWeight::BOLD)
                                                                        .text_color(theme.primary)
                                                                        .child("0.11 ms"),
                                                                ),
                                                        ),
                                                )
                                                .child(
                                                    div()
                                                        .p(px(12.))
                                                        .rounded(theme.radius_lg)
                                                        .bg(theme.surface)
                                                        .border_1()
                                                        .border_color(theme.border)
                                                        .child(
                                                            v_stack()
                                                                .spacing(px(4.))
                                                                .child(
                                                                    div()
                                                                        .text_size(px(11.5))
                                                                        .text_color(theme.muted_foreground)
                                                                        .child("FLOATING BAR CLICKS"),
                                                                )
                                                                .child(
                                                                    div()
                                                                        .text_size(px(22.))
                                                                        .font_weight(FontWeight::BOLD)
                                                                        .text_color(theme.foreground)
                                                                        .child(format!("{bar_clicks}")),
                                                                ),
                                                        ),
                                                ),
                                        ),
                                ),
                        )
                        // 1. Docked Continuous Progressive-Blur Header (composed with normal children!)
                        .child(
                            liquid_glass("pg-glass-progressive-hdr")
                                .progressive_panel(glass_prog)
                                .blur(px(16.))
                                .elevation_shadow(false)
                                .absolute()
                                .top_0()
                                .left_0()
                                .right_0()
                                .h(px(46.))
                                .rounded_none()
                                .px(px(18.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .justify_between()
                                .child(
                                    h_stack()
                                        .spacing(px(8.))
                                        .child(
                                            Icon::new(LucideIcon::Sparkles)
                                                .size(IconSize::Sm)
                                                .color(theme.primary),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(12.5))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.foreground)
                                                .child("Continuous Progressive-Blur Header"),
                                        ),
                                )
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .child("Zero banding seams · single GPU pass"),
                                ),
                        )
                        // 2. Composable Draggable Crystal Optical Magnifying Lens
                        .child(
                            liquid_glass("pg-glass-lens")
                                .lens()
                                .refraction(glass_refraction)
                                .dispersion(glass_dispersion)
                                .draggable(&app.glass_lens_drag)
                                .w(px(210.))
                                .h(px(132.))
                                .rounded_full()
                                .flex()
                                .items_center()
                                .justify_center()
                                .child(
                                    h_stack()
                                        .spacing(px(6.))
                                        .px(px(10.))
                                        .py(px(4.))
                                        .rounded_full()
                                        .bg(theme.surface.opacity(0.45))
                                        .child(
                                            Icon::new(LucideIcon::Move)
                                                .size(IconSize::Xs)
                                                .color(theme.foreground),
                                        )
                                        .child(
                                            div()
                                                .text_size(px(11.5))
                                                .font_weight(FontWeight::SEMIBOLD)
                                                .text_color(theme.foreground)
                                                .child("Drag Optical Lens"),
                                        ),
                                ),
                        )
                        // 3. Composable Draggable Floating LiquidGlass Action Bar with live interactive buttons!
                        .child(
                            liquid_glass("pg-glass-floating-bar")
                                .floating_bar()
                                .refraction((glass_refraction * 0.85).min(1.4))
                                .dispersion(glass_dispersion)
                                .blur(px(glass_frost))
                                .draggable(&app.glass_bar_drag)
                                .px(px(14.))
                                .py(px(8.))
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    Icon::new(LucideIcon::GripVertical)
                                        .size(IconSize::Sm)
                                        .color(theme.muted_foreground),
                                )
                                .child(
                                    button("lg-bar-action")
                                        .primary()
                                        .size(ComponentSize::Xs)
                                        .pill(true)
                                        .leading_icon(LucideIcon::Zap)
                                        .label(format!("Pulse ({bar_clicks})"))
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.glass_bar_clicks += 1;
                                            this.log_event(
                                                "LiquidGlass",
                                                format!(
                                                    "floating bar button clicked ({})",
                                                    this.glass_bar_clicks
                                                ),
                                                cx,
                                            );
                                        })),
                                )
                                .child(
                                    button("lg-bar-fx")
                                        .soft()
                                        .size(ComponentSize::Xs)
                                        .pill(true)
                                        .leading_icon(LucideIcon::SunMoon)
                                        .label("Flip Theme")
                                        .on_click(cx.listener(move |this, ev: &gpui::ClickEvent, window, cx| {
                                            let next_mode = match cx.theme().mode {
                                                ThemeMode::Light => ThemeMode::Dark,
                                                ThemeMode::Dark => ThemeMode::Light,
                                            };
                                            let next_pref = match next_mode {
                                                ThemeMode::Light => ThemePreference::Light,
                                                ThemeMode::Dark => ThemePreference::Dark,
                                            };
                                            let next_theme = this
                                                .accent
                                                .build_theme(next_mode)
                                                .with_preference(next_pref);
                                            Theme::switch_with_transition_style(
                                                next_theme,
                                                this.theme_transition_style,
                                                Some(ev.position()),
                                                window,
                                                cx,
                                            );
                                        })),
                                )
                                .child(
                                    badge("Draggable").color(BadgeColor::Blue).pill(true),
                                ),
                        ),
                ),
        );

    v_stack()
        .spacing(px(22.))
        .w_full()
        .child(
            page_header("Interactive & Blur Components")
                .description("Composable LiquidGlass optical lens & floating bar, physical GPU ViewTransitions, BlurContainer, LoadingOverlay, Spinners, CommandPalette (⌘K), Drawer, ContextMenu, SplitPane, NumberInput, Stepper, Rating, CopyButton, and PinInput.")
                .action(
                    button("hdr-cmd-k")
                        .secondary()
                        .size(ComponentSize::Sm)
                        .leading_icon(LucideIcon::Search)
                        .label("⌘K Palette")
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.command_palette
                                .update(cx, |p, cx| p.open(window, cx));
                        })),
                )
                .action(
                    button("hdr-open-drawer")
                        .primary()
                        .size(ComponentSize::Sm)
                        .leading_icon(LucideIcon::PanelRightOpen)
                        .label("Open Drawer")
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.drawer_open = true;
                            cx.notify();
                        })),
                ),
        )
        .child(liquid_glass_section)
        .child(component_transitions_section)
        .child(shader_transitions_section)
        .child(blur_loading_section)
        .child(spinners_and_rings)
        .child(overlays_section)
        .child(split_section)
        .child(stepper_section)
        .child(interactive_controls_grid)
}

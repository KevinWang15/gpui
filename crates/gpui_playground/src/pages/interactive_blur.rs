//! Interactive & Blur Components workbench:
//! `BlurContainer`, `LoadingOverlay`, multi-variant `Spinner`, `ProgressRing`,
//! `CommandPalette`, `Drawer`, `ContextMenu`, drag-resizable `SplitPane`,
//! `NumberInput`, `Stepper`, `Rating`, `CopyButton`, and segmented `PinInput`.

use gpui::components::palette as tw;
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
            card()
                .flat()
                .padded()
                .child(
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
                                        .color(Hsla::from(tw::EMERALD_500))
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
                                        .color(Hsla::from(tw::VIOLET_500))
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
                                        .color(Hsla::from(tw::SKY_500))
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
                                        .color(Hsla::from(tw::ROSE_500))
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
                                        .color(Hsla::from(tw::ROSE_500))
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
                this.command_palette
                    .update(cx, |p, cx| p.open(window, cx));
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
                this.log_event("ContextMenu", format!("pin -> {}", this.ctx_pin_enabled), cx);
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
        deploy_steps[app.stepper_active] =
            deploy_steps[app.stepper_active].clone().status(StepStatus::Error);
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
                                                .color(Hsla::from(tw::ROSE_500))
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

    v_stack()
        .spacing(px(22.))
        .w_full()
        .child(
            page_header("Interactive & Blur Components")
                .description("GPU-blurred LoadingOverlay & BlurContainer, multi-variant Spinners, ProgressRing, CommandPalette (⌘K), Drawer, ContextMenu, SplitPane, NumberInput, Stepper, Rating, CopyButton, and PinInput.")
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
        .child(blur_loading_section)
        .child(spinners_and_rings)
        .child(overlays_section)
        .child(split_section)
        .child(stepper_section)
        .child(interactive_controls_grid)
}

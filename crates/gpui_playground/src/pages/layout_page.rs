//! Interactive Layout primitives (Container, Grid, Stack, Card, Divider) + Tabs, Accordion,
//! Breadcrumb, and Pagination workbench.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px};

use crate::state::PlaygroundApp;

/// A dashboard widget tile for the drag-and-drop editor: an icon, a name, a
/// value and a few bars, so the dragged copies show text, icons and quads.
fn widget_tile(key: &str, theme: &Theme) -> impl IntoElement {
    let (icon, title, value, accent, bars): (LucideIcon, &str, &str, gpui::Hsla, [f32; 7]) =
        match key {
            "revenue" => (LucideIcon::DollarSign, "Revenue", "$48.2k", palette::EMERALD_500.into(), [0.4, 0.55, 0.5, 0.7, 0.65, 0.85, 0.9]),
            "latency" => (LucideIcon::Timer, "P95 latency", "182 ms", palette::AMBER_500.into(), [0.7, 0.6, 0.65, 0.5, 0.55, 0.45, 0.4]),
            "users" => (LucideIcon::Users, "Active users", "12,940", palette::INDIGO_500.into(), [0.3, 0.45, 0.5, 0.6, 0.62, 0.75, 0.8]),
            "errors" => (LucideIcon::Bug, "Errors", "0.08%", palette::ROSE_500.into(), [0.2, 0.25, 0.6, 0.3, 0.2, 0.15, 0.1]),
            "cache" => (LucideIcon::Database, "Cache hits", "94.1%", palette::SKY_500.into(), [0.8, 0.82, 0.85, 0.83, 0.9, 0.92, 0.94]),
            "deploys" => (LucideIcon::Rocket, "Deploys", "27", palette::VIOLET_500.into(), [0.2, 0.4, 0.3, 0.6, 0.5, 0.7, 0.4]),
            "traffic" => (LucideIcon::Activity, "Traffic", "3.1M req", palette::INDIGO_500.into(), [0.5, 0.6, 0.55, 0.8, 0.7, 0.9, 0.85]),
            "uptime" => (LucideIcon::ShieldCheck, "Uptime", "99.98%", palette::EMERALD_500.into(), [0.95, 0.96, 0.94, 0.97, 0.98, 0.97, 0.99]),
            _ => (LucideIcon::Bell, "Alerts", "3 open", palette::ROSE_500.into(), [0.1, 0.3, 0.2, 0.5, 0.4, 0.3, 0.6]),
        };
    v_stack()
        .spacing(px(6.))
        .w(px(176.))
        .h(px(104.))
        .p(px(12.))
        .rounded(theme.radius_lg)
        .bg(theme.surface)
        .border_1()
        .border_color(theme.border)
        .shadow_xs()
        .child(
            h_stack()
                .spacing(px(6.))
                .child(gpui::icons::Icon::new(icon).size(gpui::icons::IconSize::Sm).color(accent))
                .child(div().text_size(px(12.)).text_color(theme.muted_foreground).child(title)),
        )
        .child(div().text_size(px(18.)).font_weight(FontWeight::SEMIBOLD).child(value))
        .child(
            h_stack().spacing(px(3.)).items_end().h(px(22.)).children(bars.into_iter().map(
                move |v| div().w(px(14.)).h(px(22. * v)).rounded_sm().bg(accent.opacity(0.75)),
            )),
        )
}

/// The drag-and-drop dashboard editor: a widget palette, a dashboard and a
/// side column, sharing one `DragDropState`.
fn dnd_editor(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let preview = match app.dnd_preview {
        0 => DropPreview::Ghost(0.5),
        1 => DropPreview::Outline,
        _ => DropPreview::Space,
    };
    let container = |id: &str, layout: DragDropLayout, cx: &mut Context<PlaygroundApp>| {
        let items = app
            .dnd_lists
            .iter()
            .find(|(list, _)| list.as_ref() == id)
            .map(|(_, items)| items.clone())
            .unwrap_or_default();
        let theme = theme.clone();
        drag_drop_container(id.to_string(), &app.dnd)
            .layout(layout)
            .gap(px(12.))
            .preview(preview)
            .p(px(12.))
            .min_h(px(128.))
            .rounded(theme.radius_lg)
            .border_1()
            .border_dashed()
            .border_color(theme.border)
            .bg(theme.surface_muted.opacity(0.5))
            .items(items.iter().map(|key| (key.clone(), widget_tile(key, &theme))))
            .empty(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.muted_foreground)
                    .child("Drop widgets here"),
            )
            .on_drop(cx.listener(|this, event: &DropEvent, _, cx| this.apply_drop(event, cx)))
    };
    card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("Drag & Drop Containers (Dashboard Editor)"))
                            .child(card_description(
                                "Drag widgets from the palette into the dashboard or the side column, or reorder them. Where one would land, a half-transparent copy of its own pixels shows.",
                            )),
                    )
                    .child(
                        segmented_control(
                            "dnd-preview",
                            ["ghost", "outline", "space"][app.dnd_preview.min(2)],
                        )
                        .item(SegmentedItem::new("ghost", "Ghost"))
                        .item(SegmentedItem::new("outline", "Outline"))
                        .item(SegmentedItem::new("space", "Space"))
                        .on_change(cx.listener(|this, value: &gpui::SharedString, _, cx| {
                            this.dnd_preview = match value.as_ref() {
                                "ghost" => 0,
                                "outline" => 1,
                                _ => 2,
                            };
                            cx.notify();
                        })),
                    ),
            ),
        )
        .child(
            card_content().child(
                v_stack()
                    .spacing(px(14.))
                    .child(label("Widget palette"))
                    .child(container("palette", DragDropLayout::Wrap, cx))
                    .child(
                        h_stack()
                            .spacing(px(14.))
                            .items_start()
                            .child(
                                v_stack()
                                    .flex_1()
                                    .spacing(px(8.))
                                    .child(label("Dashboard"))
                                    .child(container("main", DragDropLayout::Wrap, cx).min_h(px(248.))),
                            )
                            .child(
                                v_stack()
                                    .w(px(202.))
                                    .spacing(px(8.))
                                    .child(label("Side column"))
                                    .child(container("side", DragDropLayout::Vertical, cx).min_h(px(248.))),
                            ),
                    ),
            ),
        )
}

/// Renders one live widget inside the [`FreeformCanvas`] showcase.
fn freeform_widget_view(kind: &str, theme: &Theme) -> impl IntoElement {
    let (icon, accent, body): (LucideIcon, gpui::Hsla, gpui::AnyElement) = match kind {
        "metric" => (
            LucideIcon::DollarSign,
            palette::INDIGO_500.into(),
            v_stack()
                .spacing(px(2.))
                .child(div().text_size(px(10.5)).text_color(theme.muted_foreground).child("Today's spend"))
                .child(
                    div()
                        .text_size(px(20.))
                        .font_weight(FontWeight::BOLD)
                        .text_color(theme.foreground)
                        .child("$14.80"),
                )
                .into_any_element(),
        ),
        "badge" => (
            LucideIcon::TrendingDown,
            palette::EMERALD_500.into(),
            badge("−18.4% vs avg")
                .color(BadgeColor::Green)
                .pill(true)
                .into_any_element(),
        ),
        "chart" => {
            let bars = [0.28_f32, 0.42, 0.36, 0.58, 0.62, 0.78, 0.92, 0.84];
            let accent: gpui::Hsla = palette::INDIGO_500.into();
            (
                LucideIcon::ChartArea,
                accent,
                v_stack()
                    .size_full()
                    .justify_between()
                    .child(div().text_size(px(10.5)).text_color(theme.muted_foreground).child("Hourly trend"))
                    .child(
                        h_stack()
                            .flex_1()
                            .items_end()
                            .spacing(px(4.))
                            .children(bars.into_iter().map(move |v| {
                                div()
                                    .flex_1()
                                    .h(px((44.0 * v).max(6.0)))
                                    .rounded_sm()
                                    .bg(accent.opacity(0.75))
                            })),
                    )
                    .into_any_element(),
            )
        }
        "status" => (
            LucideIcon::CircleDot,
            palette::EMERALD_500.into(),
            h_stack()
                .spacing(px(6.))
                .items_center()
                .child(div().size(px(7.)).rounded_full().bg(palette::EMERALD_500))
                .child(div().text_size(px(11.5)).text_color(theme.muted_foreground).child("Connected · live"))
                .into_any_element(),
        ),
        _ => (
            LucideIcon::Type,
            palette::AMBER_500.into(),
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.foreground)
                .truncate()
                .child("Updated 14:42")
                .into_any_element(),
        ),
    };
    let _ = (icon, accent);
    div()
        .size_full()
        .px(px(8.))
        .py(px(4.))
        .rounded(theme.radius_md)
        .bg(theme.surface_muted.opacity(0.45))
        .border_1()
        .border_color(theme.border.opacity(0.7))
        .flex()
        .items_center()
        .overflow_hidden()
        .child(body)
}

/// Interactive showcase of [`FreeformCanvas`] + [`CanvasPaletteItem`]: drag new widgets from the
/// palette onto a 2D canvas, move or resize them with magnetic auto-alignment guides, resize the
/// canvas frame itself, and align selected widgets in one click.
fn freeform_canvas_demo(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme().clone();
    let snap_cfg = app.freeform_canvas.read(cx).current_snap();
    let show_grid = app.freeform_canvas.read(cx).is_grid_visible();
    let snap_on = snap_cfg.snap_to_guides || snap_cfg.snap_to_grid;
    let selected = app.freeform_canvas.read(cx).selected().cloned();

    let palette_specs: [(&str, &str, LucideIcon, (f32, f32)); 5] = [
        ("metric", "KPI metric", LucideIcon::DollarSign, (148.0, 56.0)),
        ("badge", "Trend pill", LucideIcon::TrendingDown, (112.0, 28.0)),
        ("chart", "Mini chart", LucideIcon::ChartArea, (164.0, 84.0)),
        ("status", "Live status", LucideIcon::CircleDot, (132.0, 24.0)),
        ("note", "Caption", LucideIcon::Type, (124.0, 24.0)),
    ];

    let mut palette_col = v_stack().w(px(164.)).flex_none().spacing(px(8.)).child(label("Palette (drag)"));
    for (kind, name, icon, default_size) in palette_specs {
        let hover_bg = theme.surface_hover;
        let chip = div()
            .w_full()
            .px(px(10.))
            .py(px(8.))
            .rounded(theme.radius_md)
            .bg(theme.surface_muted.opacity(0.65))
            .border_1()
            .border_color(theme.border)
            .hover(move |s| s.bg(hover_bg))
            .flex()
            .items_center()
            .gap(px(8.))
            .child(gpui::icons::Icon::new(icon).size(gpui::icons::IconSize::Xs).color(theme.primary))
            .child(div().text_size(px(12.5)).font_weight(FontWeight::MEDIUM).child(name));

        palette_col = palette_col.child(
            canvas_palette_item(
                format!("pg-fc-pal-{kind}"),
                &app.freeform_canvas,
                kind,
                default_size,
                chip,
            )
            .w_full(),
        );
    }

    let mut fc = freeform_canvas("pg-freeform-canvas", &app.freeform_canvas)
        .w_full()
        .min_h(px(270.))
        .canvas_size(app.freeform_size.width, app.freeform_size.height)
        .canvas_limits(CanvasSize::new(160.0, 72.0), CanvasSize::new(520.0, 260.0))
        .canvas_radius(theme.radius_lg)
        .empty(
            div()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child("Drag widgets from the left palette"),
        );

    for item in &app.freeform_items {
        fc = fc.item(
            CanvasItemSpec::new(
                item.id.clone(),
                item.rect,
                freeform_widget_view(item.kind.as_ref(), &theme),
            )
            .min_size(64.0, 22.0),
        );
    }

    let align_btn = |id: &'static str, icon: LucideIcon, tip: &'static str, action: AlignAction| {
        button::Button::icon(id, icon)
            .ghost()
            .size(ComponentSize::Xs)
            .disabled(selected.is_none())
            .tooltip_text(tip)
            .on_click(cx.listener(move |this, _, _, cx| this.align_freeform_selected(action, cx)))
    };

    card()
        .child(
            card_header().bordered().child(
                h_stack()
                    .justify_between()
                    .flex_wrap()
                    .gap(px(10.))
                    .child(
                        v_stack()
                            .spacing(px(2.))
                            .child(card_title("Freeform 2D Canvas (Drag, Resize & Auto-Align)"))
                            .child(card_description(
                                "Drag components from the palette, resize items (8 handles) or the canvas frame itself, and snap to sibling edges, centers, and uniform gaps. Hold Alt to move freely.",
                            )),
                    )
                    .child(
                        h_stack()
                            .spacing(px(6.))
                            .child(
                                button("pg-fc-snap")
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::Magnet)
                                    .label(if snap_on { "Snap on" } else { "Snap off" })
                                    .map(|b| if snap_on { b.soft() } else { b.outline() })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let mut cfg = this.freeform_canvas.read(cx).current_snap();
                                        let next = !(cfg.snap_to_guides || cfg.snap_to_grid);
                                        cfg.snap_to_guides = next;
                                        cfg.snap_to_grid = next;
                                        this.freeform_canvas.update(cx, |s, cx| s.set_snap(cfg, cx));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("pg-fc-grid")
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::Grid3x3)
                                    .label("Grid")
                                    .map(|b| if show_grid { b.soft() } else { b.outline() })
                                    .on_click(cx.listener(|this, _, _, cx| {
                                        let next = !this.freeform_canvas.read(cx).is_grid_visible();
                                        this.freeform_canvas
                                            .update(cx, |s, cx| s.set_grid_visible(next, cx));
                                        cx.notify();
                                    })),
                            )
                            .child(
                                button("pg-fc-reset")
                                    .outline()
                                    .size(ComponentSize::Xs)
                                    .leading_icon(LucideIcon::RotateCcw)
                                    .label("Reset")
                                    .on_click(cx.listener(|this, _, _, cx| this.reset_freeform_demo(cx))),
                            ),
                    ),
            ),
        )
        .child(
            card_content().child(
                h_stack()
                    .spacing(px(14.))
                    .items_start()
                    .child(palette_col)
                    .child(
                        v_stack()
                            .flex_1()
                            .min_w_0()
                            .spacing(px(8.))
                            .child(
                                h_stack()
                                    .justify_between()
                                    .px(px(10.))
                                    .py(px(6.))
                                    .rounded(theme.radius_md)
                                    .bg(theme.surface_muted.opacity(0.5))
                                    .border_1()
                                    .border_color(theme.border)
                                    .child(
                                        h_stack()
                                            .spacing(px(4.))
                                            .child(align_btn("pg-fc-al-l", LucideIcon::AlignStartVertical, "Align left", AlignAction::Left))
                                            .child(align_btn("pg-fc-al-ch", LucideIcon::AlignCenterVertical, "Center horizontally", AlignAction::CenterHorizontal))
                                            .child(align_btn("pg-fc-al-r", LucideIcon::AlignEndVertical, "Align right", AlignAction::Right))
                                            .child(align_btn("pg-fc-al-t", LucideIcon::AlignStartHorizontal, "Align top", AlignAction::Top))
                                            .child(align_btn("pg-fc-al-cv", LucideIcon::AlignCenterHorizontal, "Center vertically", AlignAction::CenterVertical))
                                            .child(align_btn("pg-fc-al-b", LucideIcon::AlignEndHorizontal, "Align bottom", AlignAction::Bottom))
                                            .child(align_btn("pg-fc-al-w", LucideIcon::MoveHorizontal, "Fill width", AlignAction::FillWidth))
                                            .child(align_btn("pg-fc-al-g", LucideIcon::Grid3x3, "Snap to grid", AlignAction::SnapToGrid)),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(11.5))
                                            .text_color(theme.muted_foreground)
                                            .font_features(tabular_numerals())
                                            .child(format!(
                                                "Canvas {}×{} px · {} items",
                                                app.freeform_size.width as i32,
                                                app.freeform_size.height as i32,
                                                app.freeform_items.len(),
                                            )),
                                    ),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .rounded(theme.radius_lg)
                                    .border_1()
                                    .border_color(theme.border)
                                    .bg(theme.background.opacity(0.6))
                                    .overflow_hidden()
                                    .child(fc),
                            ),
                    ),
            ),
        )
}

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();

    let mut demo_grid = grid(app.grid_cols).spacing(px(12.));
    for i in 1..=6 {
        demo_grid = demo_grid.child(
            div()
                .flex()
                .items_center()
                .justify_center()
                .h(px(64.))
                .rounded(theme.radius_lg)
                .bg(theme.primary_soft)
                .border_1()
                .border_color(theme.primary.opacity(0.3))
                .text_size(px(13.5))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.primary_soft_foreground)
                .child(format!("Grid Cell {i}")),
        );
    }

    let mut demo_stack = v_stack().spacing(px(app.stack_gap_px));
    if app.stack_divided {
        demo_stack = demo_stack.divided();
    }
    demo_stack = demo_stack
        .child(
            h_stack()
                .justify_between()
                .child(div().font_weight(FontWeight::MEDIUM).child("Row 1 · Core GPUI"))
                .child(badge("v0.2.2").color(BadgeColor::Indigo)),
        )
        .child(
            h_stack()
                .justify_between()
                .child(div().font_weight(FontWeight::MEDIUM).child("Row 2 · Components"))
                .child(badge("25 modules").color(BadgeColor::Green)),
        )
        .child(
            h_stack()
                .justify_between()
                .child(div().font_weight(FontWeight::MEDIUM).child("Row 3 · Lucide SVG Icons"))
                .child(badge("2,092 icons").color(BadgeColor::Purple)),
        );

    let tab_body = match app.tabs_underline_idx {
        0 => "Overview panel: Tabs support Underline, Pills, and Segmented styles with optional Lucide icons and count badges.",
        1 => "Analytics panel: GPU rendering telemetry stays under 1ms per frame across full layouts.",
        _ => "Settings panel: Configure light/dark mode, focus rings, and brand accents globally via Theme.",
    };

    let page = v_stack()
        .spacing(px(20.))
        .child(
            page_header("Layout, Tabs, Accordion & Navigation")
                .description(
                    "Interactive demos of Container, Grid, Stack, ScrollArea, Card, Divider, Tabs (3 variants), Disclosure/Accordion, Breadcrumb, and Pagination.",
                )
                .action(
                    h_stack().spacing(px(4.)).children(
                        [
                            (MaxWidth::TwoXl, "2xl"),
                            (MaxWidth::FourXl, "4xl"),
                            (MaxWidth::SevenXl, "7xl"),
                            (MaxWidth::Full, "Full"),
                        ]
                        .into_iter()
                        .enumerate()
                        .map(|(i, (mw, lbl))| {
                            button(("mw-pick", i))
                                .size(ComponentSize::Xs)
                                .outline()
                                .selected(app.container_max == mw)
                                .label(lbl)
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.container_max = mw;
                                    this.log_event("Container", format!("max_width = {mw:?}"), cx);
                                }))
                        }),
                    ),
                ),
        )
        // Grid & Stack interactive layout controls
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    card()
                        .child(
                            card_header().bordered().child(
                                h_stack()
                                    .justify_between()
                                    .child(card_title("Interactive CSS Grid"))
                                    .child(
                                        h_stack().spacing(px(6.)).children(
                                            [1u16, 2, 3, 4].into_iter().map(|cols| {
                                                button(("grid-c", cols as usize))
                                                    .size(ComponentSize::Xs)
                                                    .outline()
                                                    .selected(app.grid_cols == cols)
                                                    .label(format!("{cols} col"))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.grid_cols = cols;
                                                        cx.notify();
                                                    }))
                                            }),
                                        ),
                                    ),
                            ),
                        )
                        .child(card_content().child(demo_grid)),
                )
                .child(
                    card()
                        .child(
                            card_header().bordered().child(
                                h_stack()
                                    .justify_between()
                                    .child(card_title("VStack + Divided & Divider"))
                                    .child(
                                        toggle("stk-div", app.stack_divided)
                                            .size(ComponentSize::Sm)
                                            .label("Divided")
                                            .on_change(cx.listener(|this, v, _, cx| {
                                                this.stack_divided = *v;
                                                cx.notify();
                                            })),
                                    ),
                            ),
                        )
                        .child(
                            card_content()
                                .child(demo_stack)
                                .child(divider().label("OR CONTINUE WITH"))
                                .child(
                                    h_stack()
                                        .justify_center()
                                        .child(
                                            button("oauth-gh")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .leading_icon(LucideIcon::GitBranch)
                                                .label("GitHub"),
                                        )
                                        .child(
                                            button("oauth-gl")
                                                .outline()
                                                .size(ComponentSize::Sm)
                                                .leading_icon(LucideIcon::KeyRound)
                                                .label("SSO Key"),
                                        ),
                                ),
                        ),
                ),
        )
        // ScrollArea (Vertical, Horizontal & Dual-Axis)
        .child(
            card()
                .child(
                    card_header().bordered().child(
                        h_stack()
                            .justify_between()
                            .child(
                                v_stack()
                                    .spacing(px(2.))
                                    .child(card_title("ScrollArea — Height-Limited, Horizontal & Dual-Axis"))
                                    .child(card_description(
                                        "Constrain viewport size with `.max_height(...)` or `.height(...)`; move cursor near edges to reveal slim auto-hiding scrollbars.",
                                    )),
                            )
                            .child(
                                h_stack()
                                    .spacing(px(6.))
                                    .children([140.0f32, 200.0, 280.0].into_iter().map(|h| {
                                        button(("sa-maxh", h as usize))
                                            .size(ComponentSize::Xs)
                                            .outline()
                                            .selected((app.scroll_area_max_h - h).abs() < 1.0)
                                            .label(format!("max_h {}px", h as usize))
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.scroll_area_max_h = h;
                                                cx.notify();
                                            }))
                                    }))
                                    .child(
                                        button("sa-top")
                                            .size(ComponentSize::Xs)
                                            .ghost()
                                            .label("Top")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.scroll_area_handle.set_offset(gpui::point(px(0.), px(0.)));
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        button("sa-bottom")
                                            .size(ComponentSize::Xs)
                                            .ghost()
                                            .label("Bottom")
                                            .on_click(cx.listener(|this, _, _, cx| {
                                                this.scroll_area_handle.scroll_to_bottom();
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    ),
                )
                .child(
                    card_content().child(
                        grid(2)
                            .spacing(px(16.))
                            .child(
                                v_stack()
                                    .spacing(px(8.))
                                    .child(label(format!(
                                        "Vertical `scroll_area` (.max_height({}px) + ScrollHandle)",
                                        app.scroll_area_max_h as usize
                                    )))
                                    .child(
                                        scroll_area("pg-scroll-v")
                                            .max_height(px(app.scroll_area_max_h))
                                            .bordered(true)
                                            .padding(px(10.))
                                            .spacing(px(6.))
                                            .track_scroll(&app.scroll_area_handle)
                                            .children((1..=16).map(|i| {
                                                let tone = match i % 4 {
                                                    0 => BadgeColor::Green,
                                                    1 => BadgeColor::Indigo,
                                                    2 => BadgeColor::Amber,
                                                    _ => BadgeColor::Purple,
                                                };
                                                let status = match i % 4 {
                                                    0 => "Deployed",
                                                    1 => "Verified",
                                                    2 => "Canary",
                                                    _ => "Queued",
                                                };
                                                h_stack()
                                                    .justify_between()
                                                    .px(px(10.))
                                                    .py(px(8.))
                                                    .rounded(theme.radius_md)
                                                    .bg(theme.surface_muted.opacity(0.45))
                                                    .border_1()
                                                    .border_color(theme.border.opacity(0.7))
                                                    .child(
                                                        h_stack()
                                                            .spacing(px(8.))
                                                            .min_w_0()
                                                            .flex_1()
                                                            .child(
                                                                div()
                                                                    .font_features(tabular_numerals())
                                                                    .text_size(px(12.))
                                                                    .text_color(theme.muted_foreground)
                                                                    .child(format!("#{i:02}")),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(13.))
                                                                    .font_weight(FontWeight::MEDIUM)
                                                                    .truncate()
                                                                    .child(format!("Release build v2.4.{i} — shader pipeline")),
                                                            ),
                                                    )
                                                    .child(badge(status).color(tone))
                                            })),
                                    ),
                            )
                            .child(
                                v_stack()
                                    .spacing(px(12.))
                                    .child(
                                        v_stack()
                                            .spacing(px(6.))
                                            .child(label("Horizontal `ScrollArea::horizontal`"))
                                            .child(
                                                ScrollArea::horizontal("pg-scroll-h")
                                                    .bordered(true)
                                                    .padding(px(10.))
                                                    .spacing(px(10.))
                                                    .children((1..=8).map(|i| {
                                                        v_stack()
                                                            .flex_shrink_0()
                                                            .w(px(132.))
                                                            .p(px(10.))
                                                            .spacing(px(4.))
                                                            .rounded(theme.radius_md)
                                                            .bg(theme.surface_muted.opacity(0.5))
                                                            .border_1()
                                                            .border_color(theme.border)
                                                            .child(
                                                                div()
                                                                    .text_size(px(11.5))
                                                                    .text_color(theme.muted_foreground)
                                                                    .child(format!("Cluster us-east-{i}")),
                                                            )
                                                            .child(
                                                                div()
                                                                    .text_size(px(15.))
                                                                    .font_weight(FontWeight::SEMIBOLD)
                                                                    .font_features(tabular_numerals())
                                                                    .child(format!("{}.{} ms", 10 + i * 3, (i * 7) % 10)),
                                                            )
                                                    })),
                                            ),
                                    )
                                    .child(
                                        v_stack()
                                            .spacing(px(6.))
                                            .child(label("Two-axis `ScrollArea::both` (.height(136px))"))
                                            .child(
                                                ScrollArea::both("pg-scroll-both")
                                                    .height(px(136.))
                                                    .bordered(true)
                                                    .padding(px(10.))
                                                    .spacing(px(6.))
                                                    .children((1..=10).map(|row| {
                                                        let cell_theme = theme.clone();
                                                        h_stack()
                                                            .flex_shrink_0()
                                                            .spacing(px(6.))
                                                            .children((1..=8).map(move |col| {
                                                                div()
                                                                    .flex_shrink_0()
                                                                    .w(px(88.))
                                                                    .px(px(8.))
                                                                    .py(px(5.))
                                                                    .rounded(cell_theme.radius_sm)
                                                                    .bg(if (row + col) % 2 == 0 {
                                                                        cell_theme.surface_muted.opacity(0.6)
                                                                    } else {
                                                                        cell_theme.background
                                                                    })
                                                                    .border_1()
                                                                    .border_color(cell_theme.border.opacity(0.6))
                                                                    .text_size(px(11.5))
                                                                    .font_features(tabular_numerals())
                                                                    .text_color(cell_theme.muted_foreground)
                                                                    .child(format!("R{row:02} · C{col:02}"))
                                                            }))
                                                    })),
                                            ),
                                    ),
                            ),
                    ),
                ),
        )
        // Tabs (Underline, Pills, Segmented)
        .child(
            card()
                .child(card_header().bordered().child(card_title("Tabs — Underline, Pills & Segmented")))
                .child(
                    card_content()
                        .child(
                            tabs(
                                "pg-tabs-u",
                                vec![
                                    TabItem::new("Overview").icon(LucideIcon::LayoutGrid),
                                    TabItem::new("Analytics")
                                        .icon(LucideIcon::ChartBar)
                                        .badge("14"),
                                    TabItem::new("Settings").icon(LucideIcon::Settings),
                                    TabItem::new("Disabled").disabled(true),
                                ],
                            )
                            .selected_index(app.tabs_underline_idx)
                            .on_change(cx.listener(|this, idx, _, cx| {
                                this.tabs_underline_idx = *idx;
                                this.log_event("Tabs(Underline)", format!("tab -> {idx}"), cx);
                            })),
                        )
                        .child(
                            div()
                                .p(px(14.))
                                .rounded(theme.radius_md)
                                .bg(theme.background)
                                .border_1()
                                .border_color(theme.border)
                                .text_size(px(13.5))
                                .text_color(theme.muted_foreground)
                                .child(tab_body),
                        )
                        .child(divider())
                        .child(
                            h_stack()
                                .flex_wrap()
                                .justify_between()
                                .child(
                                    tabs(
                                        "pg-tabs-p",
                                        vec![
                                            TabItem::new("All").badge("48"),
                                            TabItem::new("Active").badge("12"),
                                            TabItem::new("Archived"),
                                        ],
                                    )
                                    .pills()
                                    .selected_index(app.tabs_pills_idx)
                                    .on_change(cx.listener(|this, idx, _, cx| {
                                        this.tabs_pills_idx = *idx;
                                        cx.notify();
                                    })),
                                )
                                .child(
                                    tabs(
                                        "pg-tabs-s",
                                        vec![
                                            TabItem::new("Monthly"),
                                            TabItem::new("Annual (Save 20%)"),
                                        ],
                                    )
                                    .segmented()
                                    .selected_index(app.tabs_seg_idx)
                                    .on_change(cx.listener(|this, idx, _, cx| {
                                        this.tabs_seg_idx = *idx;
                                        cx.notify();
                                    })),
                                ),
                        ),
                ),
        )
        // Accordion + Breadcrumb + Pagination
        .child(
            grid(2)
                .spacing(px(16.))
                .child(
                    v_stack()
                        .spacing(px(10.))
                        .child(
                            h_stack()
                                .justify_between()
                                .child(label("Accordion / Disclosure"))
                                .child(
                                    h_stack()
                                        .spacing(px(6.))
                                        .child(
                                            button("acc-all")
                                                .ghost()
                                                .size(ComponentSize::Xs)
                                                .label("Expand all")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.accordion_open = [true, true, true];
                                                    cx.notify();
                                                })),
                                        )
                                        .child(
                                            button("acc-none")
                                                .ghost()
                                                .size(ComponentSize::Xs)
                                                .label("Collapse all")
                                                .on_click(cx.listener(|this, _, _, cx| {
                                                    this.accordion_open = [false, false, false];
                                                    cx.notify();
                                                })),
                                        ),
                                ),
                        )
                        .child(
                            accordion()
                                .child(
                                    disclosure(
                                        "pg-acc-0",
                                        "How is this GPUI fork structured?",
                                        app.accordion_open[0],
                                    )
                                    .borderless()
                                    .icon(LucideIcon::FolderTree)
                                    .on_toggle(cx.listener(|this, open, _, cx| {
                                        this.accordion_open[0] = *open;
                                        cx.notify();
                                    }))
                                    .child(
                                        "Extracted cleanly from zed-industries/zed with its minimal 26 transitive crates plus gpui::components, gpui::icons, and gpui_playground.",
                                    ),
                                )
                                .child(
                                    disclosure(
                                        "pg-acc-1",
                                        "Are all Lucide SVGs copied directly?",
                                        app.accordion_open[1],
                                    )
                                    .borderless()
                                    .icon(LucideIcon::Shapes)
                                    .on_toggle(cx.listener(|this, open, _, cx| {
                                        this.accordion_open[1] = *open;
                                        cx.notify();
                                    }))
                                    .child(
                                        "Yes! All 2,092 .svg files and the ISC/MIT LICENSE live verbatim under crates/gpui/assets/lucide/.",
                                    ),
                                )
                                .child(
                                    disclosure(
                                        "pg-acc-2",
                                        "Can I disable components or lucide features?",
                                        app.accordion_open[2],
                                    )
                                    .borderless()
                                    .icon(LucideIcon::SlidersHorizontal)
                                    .on_toggle(cx.listener(|this, open, _, cx| {
                                        this.accordion_open[2] = *open;
                                        cx.notify();
                                    }))
                                    .child(
                                        "Both are default-on Cargo features (`components`, `lucide`) so `default-features = false` gives bare upstream GPUI.",
                                    ),
                                ),
                        ),
                )
                .child(
                    card()
                        .child(
                            card_header()
                                .bordered()
                                .child(card_title("Breadcrumb & Pagination")),
                        )
                        .child(
                            card_content()
                                .child(
                                    v_stack()
                                        .spacing(px(8.))
                                        .child(label("Breadcrumb (click ancestors)"))
                                        .child(breadcrumb(
                                            "pg-bc",
                                            vec![
                                                BreadcrumbItem::link(
                                                    "Home",
                                                    cx.listener(|this, _, _, cx| {
                                                        this.active_crumb = "Home".into();
                                                        this.log_event("Breadcrumb", "Home", cx);
                                                    }),
                                                )
                                                .icon(LucideIcon::House),
                                                BreadcrumbItem::link(
                                                    "Components",
                                                    cx.listener(|this, _, _, cx| {
                                                        this.active_crumb = "Components".into();
                                                        this.log_event(
                                                            "Breadcrumb",
                                                            "Components",
                                                            cx,
                                                        );
                                                    }),
                                                ),
                                                BreadcrumbItem::current(app.active_crumb.clone()),
                                            ],
                                        )),
                                )
                                .child(divider())
                                .child(
                                    v_stack()
                                        .spacing(px(8.))
                                        .child(label(format!(
                                            "Pagination (Page {} of 14)",
                                            app.current_page
                                        )))
                                        .child(
                                            pagination("pg-pager", app.current_page, 14)
                                                .on_page_change(cx.listener(|this, p, _, cx| {
                                                    this.current_page = *p;
                                                    this.log_event(
                                                        "Pagination",
                                                        format!("page -> {p}"),
                                                        cx,
                                                    );
                                                })),
                                        ),
                                ),
                        ),
                ),
        );
    page.child(dnd_editor(app, cx))
        .child(freeform_canvas_demo(app, cx))
}

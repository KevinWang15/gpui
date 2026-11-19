//! Interactive Layout primitives (Container, Grid, Stack, Card, Divider) + Tabs, Accordion,
//! Breadcrumb, and Pagination workbench.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, FontWeight, IntoElement, div, prelude::*, px};

use crate::state::PlaygroundApp;

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
                .child(div().font_weight(FontWeight::MEDIUM).child("Row 2 · Tailwind Components"))
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

    v_stack()
        .spacing(px(20.))
        .child(
            page_header("Layout, Tabs, Accordion & Navigation")
                .description(
                    "Interactive demos of Container, Grid, Stack, Card, Divider, Tabs (3 variants), Disclosure/Accordion, Breadcrumb, and Pagination.",
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
        )
}

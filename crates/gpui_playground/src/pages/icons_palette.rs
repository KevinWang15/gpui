//! Full 2,092-icon Lucide SVG Explorer and 242-swatch Color Palette Explorer.

use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideIcon};
use gpui::{ClipboardItem, Context, FontWeight, IntoElement, Rgba, div, prelude::*, px};

use crate::code_box::code_snippet;
use crate::state::PlaygroundApp;

const PAGE_SIZE: usize = 72;

const ICON_CATEGORIES: &[(&str, &str)] = &[
    ("all", "All"),
    ("arrow", "Arrows"),
    ("chevron", "Chevrons"),
    ("file", "Files"),
    ("folder", "Folders"),
    ("user", "Users"),
    ("mail", "Mail"),
    ("calendar", "Calendar"),
    ("chart", "Charts"),
    ("cloud", "Cloud"),
    ("lock", "Security"),
    ("git", "Git"),
    ("sun", "Weather"),
    ("check", "Checks"),
];

pub fn render_icons(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let raw_query = app.icon_search.read(cx).value().trim().to_lowercase();

    let matching: Vec<LucideIcon> = LucideIcon::ALL
        .iter()
        .copied()
        .filter(|icon| {
            let name = icon.name();
            let cat_ok = app.icon_category == "all" || name.contains(app.icon_category);
            let query_ok = raw_query.is_empty() || name.contains(&raw_query);
            cat_ok && query_ok
        })
        .collect();

    let total_matches = matching.len();
    let total_pages = total_matches.div_ceil(PAGE_SIZE).max(1);
    let current_page = app.icon_page.clamp(1, total_pages);
    let start = (current_page - 1) * PAGE_SIZE;
    let page_icons: Vec<LucideIcon> = matching.into_iter().skip(start).take(PAGE_SIZE).collect();

    let selected = app.selected_icon;
    let snippet = {
        let mut s = format!(
            "Icon::new(LucideIcon::{selected:?})\n    .size(IconSize::{:?})",
            app.icon_size
        );
        if (app.icon_stroke - 2.0).abs() > 0.01 {
            s.push_str(&format!("\n    .stroke_width({:.1})", app.icon_stroke));
        }
        if app.icon_spin {
            s.push_str("\n    .spin()");
        }
        s
    };

    let mut icon_grid = grid(6).spacing(px(10.));
    for icon in page_icons {
        let is_selected = icon == selected;
        let mut glyph = Icon::new(icon)
            .size(app.icon_size)
            .stroke_width(app.icon_stroke)
            .color(if is_selected {
                theme.primary
            } else {
                theme.foreground
            });
        if app.icon_spin {
            glyph = glyph.spin();
        }

        icon_grid = icon_grid.child(
            div()
                .id(("ic-tile", icon as usize))
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(8.))
                .h(px(86.))
                .p(px(10.))
                .rounded(theme.radius_lg)
                .bg(if is_selected {
                    theme.primary_soft.opacity(0.65)
                } else {
                    theme.surface
                })
                .border_1()
                .border_color(if is_selected {
                    theme.primary
                } else {
                    theme.border
                })
                .cursor_pointer()
                .hover(|s| s.border_color(theme.primary))
                .on_click(cx.listener(move |this, _, _, cx| {
                    this.selected_icon = icon;
                    let code = format!("LucideIcon::{icon:?}");
                    cx.write_to_clipboard(ClipboardItem::new_string(code.clone()));
                    this.log_event("LucideIcon", format!("selected {code}"), cx);
                    this.push_toast(
                        ToastVariant::Info,
                        format!("Copied `{code}`"),
                        Some(icon.asset_path()),
                        cx,
                    );
                }))
                .child(glyph)
                .child(
                    div()
                        .w_full()
                        .truncate()
                        .text_center()
                        .text_size(px(11.))
                        .text_color(if is_selected {
                            theme.primary_soft_foreground
                        } else {
                            theme.muted_foreground
                        })
                        .child(icon.name()),
                ),
        );
    }

    let size_choices: &[(IconSize, &str)] = &[
        (IconSize::Sm, "14px"),
        (IconSize::Md, "16px"),
        (IconSize::Lg, "20px"),
        (IconSize::Xl, "24px"),
        (IconSize::TwoXl, "32px"),
    ];

    v_stack()
        .spacing(px(18.))
        .child(
            page_header(format!("Lucide Icons Explorer ({} bundled SVGs)", LucideIcon::COUNT))
                .description(
                    "All Lucide SVGs are copied verbatim into crates/gpui/assets/lucide/ (ISC + MIT). Click any icon to inspect & copy.",
                ),
        )
        // Selected Icon Inspector Bar
        .child(
            card().padded().child(
                h_stack()
                    .spacing(px(20.))
                    .items_center()
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(76.))
                            .h(px(76.))
                            .rounded(theme.radius_xl)
                            .bg(theme.primary_soft)
                            .border_1()
                            .border_color(theme.primary.opacity(0.3))
                            .child({
                                let mut preview = Icon::new(selected)
                                    .size(IconSize::TwoXl)
                                    .stroke_width(app.icon_stroke)
                                    .color(theme.primary);
                                if app.icon_spin {
                                    preview = preview.spin();
                                }
                                preview
                            }),
                    )
                    .child(
                        v_stack()
                            .spacing(px(4.))
                            .w(px(220.))
                            .child(
                                div()
                                    .text_size(px(16.))
                                    .font_weight(FontWeight::BOLD)
                                    .child(format!("LucideIcon::{selected:?}")),
                            )
                            .child(
                                div()
                                    .text_size(px(12.5))
                                    .text_color(theme.muted_foreground)
                                    .child(selected.asset_path()),
                            )
                            .child(
                                h_stack()
                                    .spacing(px(8.))
                                    .pt(px(6.))
                                    .child(Icon::new(selected).size(IconSize::Xs))
                                    .child(Icon::new(selected).size(IconSize::Sm))
                                    .child(Icon::new(selected).size(IconSize::Md))
                                    .child(Icon::new(selected).size(IconSize::Lg))
                                    .child(Icon::new(selected).size(IconSize::Xl)),
                            ),
                    )
                    .child(
                        div().flex_1().min_w_0().child(code_snippet(
                            "copy-icon-code",
                            "Selected Icon Snippet",
                            snippet,
                            cx.listener(|this, _, _, cx| {
                                this.push_toast(
                                    ToastVariant::Success,
                                    "Copied Icon Rust code",
                                    None,
                                    cx,
                                );
                            }),
                            cx,
                        )),
                    ),
            ),
        )
        // Search & Customization Bar
        .child(
            card().padded().child(
                v_stack()
                    .spacing(px(12.))
                    .child(
                        h_stack()
                            .flex_wrap()
                            .spacing(px(12.))
                            .child(div().flex_1().min_w(px(260.)).child(input(&app.icon_search)))
                            .child(
                                h_stack().spacing(px(4.)).children(
                                    size_choices.iter().enumerate().map(|(i, &(sz, lbl))| {
                                        button(("isz", i))
                                            .size(ComponentSize::Xs)
                                            .outline()
                                            .selected(app.icon_size == sz)
                                            .label(lbl)
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                this.icon_size = sz;
                                                cx.notify();
                                            }))
                                    }),
                                ),
                            )
                            .child(
                                h_stack().spacing(px(4.)).children(
                                    [1.25f32, 1.5, 2.0, 2.5].into_iter().enumerate().map(
                                        |(i, sw)| {
                                            button(("isw", i))
                                                .size(ComponentSize::Xs)
                                                .outline()
                                                .selected((app.icon_stroke - sw).abs() < 0.05)
                                                .label(format!("{sw}px"))
                                                .on_click(cx.listener(move |this, _, _, cx| {
                                                    this.icon_stroke = sw;
                                                    cx.notify();
                                                }))
                                        },
                                    ),
                                ),
                            )
                            .child(
                                toggle("ic-spin", app.icon_spin)
                                    .size(ComponentSize::Sm)
                                    .label("Spin")
                                    .on_change(cx.listener(|this, v, _, cx| {
                                        this.icon_spin = *v;
                                        cx.notify();
                                    })),
                            ),
                    )
                    .child(
                        h_stack().flex_wrap().spacing(px(6.)).children(
                            ICON_CATEGORIES.iter().enumerate().map(|(i, &(key, title))| {
                                button(("icat", i))
                                    .size(ComponentSize::Xs)
                                    .pill(true)
                                    .variant(if app.icon_category == key {
                                        ButtonVariant::Primary
                                    } else {
                                        ButtonVariant::Soft
                                    })
                                    .label(title)
                                    .on_click(cx.listener(move |this, _, _, cx| {
                                        this.icon_category = key;
                                        this.icon_page = 1;
                                        cx.notify();
                                    }))
                            }),
                        ),
                    ),
            ),
        )
        .child(
            h_stack()
                .justify_between()
                .child(
                    badge(format!(
                        "{total_matches} matching icons · Page {current_page} / {total_pages}"
                    ))
                    .color(BadgeColor::Indigo)
                    .pill(true),
                )
                .child(
                    pagination("ic-pager", current_page, total_pages).on_page_change(cx.listener(
                        |this, p, _, cx| {
                            this.icon_page = *p;
                            cx.notify();
                        },
                    )),
                ),
        )
        .child(if total_matches == 0 {
            empty_state(LucideIcon::SearchX, "No Lucide icons matched")
                .description("Try clearing the search query or selecting the 'All' category.")
                .into_any_element()
        } else {
            icon_grid.into_any_element()
        })
}

pub fn render_palette(_app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    use gpui::components::palette as tw;

    let scales: &[(&str, &[(&str, Rgba)])] = &[
        (
            "Slate",
            &[
                ("SLATE_50", tw::SLATE_50),
                ("SLATE_100", tw::SLATE_100),
                ("SLATE_200", tw::SLATE_200),
                ("SLATE_300", tw::SLATE_300),
                ("SLATE_400", tw::SLATE_400),
                ("SLATE_500", tw::SLATE_500),
                ("SLATE_600", tw::SLATE_600),
                ("SLATE_700", tw::SLATE_700),
                ("SLATE_800", tw::SLATE_800),
                ("SLATE_900", tw::SLATE_900),
                ("SLATE_950", tw::SLATE_950),
            ],
        ),
        (
            "Gray",
            &[
                ("GRAY_50", tw::GRAY_50),
                ("GRAY_100", tw::GRAY_100),
                ("GRAY_200", tw::GRAY_200),
                ("GRAY_300", tw::GRAY_300),
                ("GRAY_400", tw::GRAY_400),
                ("GRAY_500", tw::GRAY_500),
                ("GRAY_600", tw::GRAY_600),
                ("GRAY_700", tw::GRAY_700),
                ("GRAY_800", tw::GRAY_800),
                ("GRAY_900", tw::GRAY_900),
                ("GRAY_950", tw::GRAY_950),
            ],
        ),
        (
            "Zinc",
            &[
                ("ZINC_50", tw::ZINC_50),
                ("ZINC_100", tw::ZINC_100),
                ("ZINC_200", tw::ZINC_200),
                ("ZINC_300", tw::ZINC_300),
                ("ZINC_400", tw::ZINC_400),
                ("ZINC_500", tw::ZINC_500),
                ("ZINC_600", tw::ZINC_600),
                ("ZINC_700", tw::ZINC_700),
                ("ZINC_800", tw::ZINC_800),
                ("ZINC_900", tw::ZINC_900),
                ("ZINC_950", tw::ZINC_950),
            ],
        ),
        (
            "Red",
            &[
                ("RED_50", tw::RED_50),
                ("RED_100", tw::RED_100),
                ("RED_200", tw::RED_200),
                ("RED_300", tw::RED_300),
                ("RED_400", tw::RED_400),
                ("RED_500", tw::RED_500),
                ("RED_600", tw::RED_600),
                ("RED_700", tw::RED_700),
                ("RED_800", tw::RED_800),
                ("RED_900", tw::RED_900),
                ("RED_950", tw::RED_950),
            ],
        ),
        (
            "Orange",
            &[
                ("ORANGE_50", tw::ORANGE_50),
                ("ORANGE_100", tw::ORANGE_100),
                ("ORANGE_200", tw::ORANGE_200),
                ("ORANGE_300", tw::ORANGE_300),
                ("ORANGE_400", tw::ORANGE_400),
                ("ORANGE_500", tw::ORANGE_500),
                ("ORANGE_600", tw::ORANGE_600),
                ("ORANGE_700", tw::ORANGE_700),
                ("ORANGE_800", tw::ORANGE_800),
                ("ORANGE_900", tw::ORANGE_900),
                ("ORANGE_950", tw::ORANGE_950),
            ],
        ),
        (
            "Amber",
            &[
                ("AMBER_50", tw::AMBER_50),
                ("AMBER_100", tw::AMBER_100),
                ("AMBER_200", tw::AMBER_200),
                ("AMBER_300", tw::AMBER_300),
                ("AMBER_400", tw::AMBER_400),
                ("AMBER_500", tw::AMBER_500),
                ("AMBER_600", tw::AMBER_600),
                ("AMBER_700", tw::AMBER_700),
                ("AMBER_800", tw::AMBER_800),
                ("AMBER_900", tw::AMBER_900),
                ("AMBER_950", tw::AMBER_950),
            ],
        ),
        (
            "Emerald",
            &[
                ("EMERALD_50", tw::EMERALD_50),
                ("EMERALD_100", tw::EMERALD_100),
                ("EMERALD_200", tw::EMERALD_200),
                ("EMERALD_300", tw::EMERALD_300),
                ("EMERALD_400", tw::EMERALD_400),
                ("EMERALD_500", tw::EMERALD_500),
                ("EMERALD_600", tw::EMERALD_600),
                ("EMERALD_700", tw::EMERALD_700),
                ("EMERALD_800", tw::EMERALD_800),
                ("EMERALD_900", tw::EMERALD_900),
                ("EMERALD_950", tw::EMERALD_950),
            ],
        ),
        (
            "Teal",
            &[
                ("TEAL_50", tw::TEAL_50),
                ("TEAL_100", tw::TEAL_100),
                ("TEAL_200", tw::TEAL_200),
                ("TEAL_300", tw::TEAL_300),
                ("TEAL_400", tw::TEAL_400),
                ("TEAL_500", tw::TEAL_500),
                ("TEAL_600", tw::TEAL_600),
                ("TEAL_700", tw::TEAL_700),
                ("TEAL_800", tw::TEAL_800),
                ("TEAL_900", tw::TEAL_900),
                ("TEAL_950", tw::TEAL_950),
            ],
        ),
        (
            "Sky",
            &[
                ("SKY_50", tw::SKY_50),
                ("SKY_100", tw::SKY_100),
                ("SKY_200", tw::SKY_200),
                ("SKY_300", tw::SKY_300),
                ("SKY_400", tw::SKY_400),
                ("SKY_500", tw::SKY_500),
                ("SKY_600", tw::SKY_600),
                ("SKY_700", tw::SKY_700),
                ("SKY_800", tw::SKY_800),
                ("SKY_900", tw::SKY_900),
                ("SKY_950", tw::SKY_950),
            ],
        ),
        (
            "Blue",
            &[
                ("BLUE_50", tw::BLUE_50),
                ("BLUE_100", tw::BLUE_100),
                ("BLUE_200", tw::BLUE_200),
                ("BLUE_300", tw::BLUE_300),
                ("BLUE_400", tw::BLUE_400),
                ("BLUE_500", tw::BLUE_500),
                ("BLUE_600", tw::BLUE_600),
                ("BLUE_700", tw::BLUE_700),
                ("BLUE_800", tw::BLUE_800),
                ("BLUE_900", tw::BLUE_900),
                ("BLUE_950", tw::BLUE_950),
            ],
        ),
        (
            "Indigo",
            &[
                ("INDIGO_50", tw::INDIGO_50),
                ("INDIGO_100", tw::INDIGO_100),
                ("INDIGO_200", tw::INDIGO_200),
                ("INDIGO_300", tw::INDIGO_300),
                ("INDIGO_400", tw::INDIGO_400),
                ("INDIGO_500", tw::INDIGO_500),
                ("INDIGO_600", tw::INDIGO_600),
                ("INDIGO_700", tw::INDIGO_700),
                ("INDIGO_800", tw::INDIGO_800),
                ("INDIGO_900", tw::INDIGO_900),
                ("INDIGO_950", tw::INDIGO_950),
            ],
        ),
        (
            "Violet",
            &[
                ("VIOLET_50", tw::VIOLET_50),
                ("VIOLET_100", tw::VIOLET_100),
                ("VIOLET_200", tw::VIOLET_200),
                ("VIOLET_300", tw::VIOLET_300),
                ("VIOLET_400", tw::VIOLET_400),
                ("VIOLET_500", tw::VIOLET_500),
                ("VIOLET_600", tw::VIOLET_600),
                ("VIOLET_700", tw::VIOLET_700),
                ("VIOLET_800", tw::VIOLET_800),
                ("VIOLET_900", tw::VIOLET_900),
                ("VIOLET_950", tw::VIOLET_950),
            ],
        ),
        (
            "Pink",
            &[
                ("PINK_50", tw::PINK_50),
                ("PINK_100", tw::PINK_100),
                ("PINK_200", tw::PINK_200),
                ("PINK_300", tw::PINK_300),
                ("PINK_400", tw::PINK_400),
                ("PINK_500", tw::PINK_500),
                ("PINK_600", tw::PINK_600),
                ("PINK_700", tw::PINK_700),
                ("PINK_800", tw::PINK_800),
                ("PINK_900", tw::PINK_900),
                ("PINK_950", tw::PINK_950),
            ],
        ),
        (
            "Rose",
            &[
                ("ROSE_50", tw::ROSE_50),
                ("ROSE_100", tw::ROSE_100),
                ("ROSE_200", tw::ROSE_200),
                ("ROSE_300", tw::ROSE_300),
                ("ROSE_400", tw::ROSE_400),
                ("ROSE_500", tw::ROSE_500),
                ("ROSE_600", tw::ROSE_600),
                ("ROSE_700", tw::ROSE_700),
                ("ROSE_800", tw::ROSE_800),
                ("ROSE_900", tw::ROSE_900),
                ("ROSE_950", tw::ROSE_950),
            ],
        ),
    ];

    let theme = cx.theme();
    let shades = [
        "50", "100", "200", "300", "400", "500", "600", "700", "800", "900", "950",
    ];

    let mut rows = v_stack().spacing(px(12.));
    for (s_idx, &(scale_name, colors)) in scales.iter().enumerate() {
        let mut swatch_row = grid(11).spacing(px(6.));
        for (c_idx, &(const_name, rgba)) in colors.iter().enumerate() {
            let shade = shades[c_idx];
            swatch_row = swatch_row.child(
                div()
                    .id(("swatch", s_idx * 16 + c_idx))
                    .flex()
                    .flex_col()
                    .gap(px(4.))
                    .cursor_pointer()
                    .on_click(cx.listener(move |this, _, _, cx| {
                        let snippet = format!("gpui::components::palette::{const_name}");
                        cx.write_to_clipboard(ClipboardItem::new_string(snippet));
                        this.log_event("Palette", format!("copied {const_name}"), cx);
                        this.push_toast(
                            ToastVariant::Info,
                            format!("Copied `palette::{const_name}`"),
                            None,
                            cx,
                        );
                    }))
                    .child(
                        div()
                            .w_full()
                            .h(px(38.))
                            .rounded(theme.radius_md)
                            .bg(rgba)
                            .border_1()
                            .border_color(theme.border),
                    )
                    .child(
                        div()
                            .text_center()
                            .text_size(px(10.5))
                            .text_color(theme.muted_foreground)
                            .child(shade),
                    ),
            );
        }

        rows = rows.child(
            h_stack()
                .spacing(px(14.))
                .items_start()
                .child(
                    div()
                        .w(px(72.))
                        .pt(px(8.))
                        .text_size(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(scale_name),
                )
                .child(div().flex_1().child(swatch_row)),
        );
    }

    v_stack()
        .spacing(px(20.))
        .child(
            page_header("Color Palette")
                .description(
                    "All 22 hues × 11 shades (50–950) are defined as zero-cost `Rgba` constants in `gpui::components::palette`. Click any swatch to copy its constant.",
                ),
        )
        .child(card().padded().child(rows))
}

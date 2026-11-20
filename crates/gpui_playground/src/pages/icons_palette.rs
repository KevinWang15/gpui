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
    use gpui::components::palette;

    let scales: &[(&str, &[(&str, Rgba)])] = &[
        (
            "Slate",
            &[
                ("SLATE_50", palette::SLATE_50),
                ("SLATE_100", palette::SLATE_100),
                ("SLATE_200", palette::SLATE_200),
                ("SLATE_300", palette::SLATE_300),
                ("SLATE_400", palette::SLATE_400),
                ("SLATE_500", palette::SLATE_500),
                ("SLATE_600", palette::SLATE_600),
                ("SLATE_700", palette::SLATE_700),
                ("SLATE_800", palette::SLATE_800),
                ("SLATE_900", palette::SLATE_900),
                ("SLATE_950", palette::SLATE_950),
            ],
        ),
        (
            "Gray",
            &[
                ("GRAY_50", palette::GRAY_50),
                ("GRAY_100", palette::GRAY_100),
                ("GRAY_200", palette::GRAY_200),
                ("GRAY_300", palette::GRAY_300),
                ("GRAY_400", palette::GRAY_400),
                ("GRAY_500", palette::GRAY_500),
                ("GRAY_600", palette::GRAY_600),
                ("GRAY_700", palette::GRAY_700),
                ("GRAY_800", palette::GRAY_800),
                ("GRAY_900", palette::GRAY_900),
                ("GRAY_950", palette::GRAY_950),
            ],
        ),
        (
            "Zinc",
            &[
                ("ZINC_50", palette::ZINC_50),
                ("ZINC_100", palette::ZINC_100),
                ("ZINC_200", palette::ZINC_200),
                ("ZINC_300", palette::ZINC_300),
                ("ZINC_400", palette::ZINC_400),
                ("ZINC_500", palette::ZINC_500),
                ("ZINC_600", palette::ZINC_600),
                ("ZINC_700", palette::ZINC_700),
                ("ZINC_800", palette::ZINC_800),
                ("ZINC_900", palette::ZINC_900),
                ("ZINC_950", palette::ZINC_950),
            ],
        ),
        (
            "Red",
            &[
                ("RED_50", palette::RED_50),
                ("RED_100", palette::RED_100),
                ("RED_200", palette::RED_200),
                ("RED_300", palette::RED_300),
                ("RED_400", palette::RED_400),
                ("RED_500", palette::RED_500),
                ("RED_600", palette::RED_600),
                ("RED_700", palette::RED_700),
                ("RED_800", palette::RED_800),
                ("RED_900", palette::RED_900),
                ("RED_950", palette::RED_950),
            ],
        ),
        (
            "Orange",
            &[
                ("ORANGE_50", palette::ORANGE_50),
                ("ORANGE_100", palette::ORANGE_100),
                ("ORANGE_200", palette::ORANGE_200),
                ("ORANGE_300", palette::ORANGE_300),
                ("ORANGE_400", palette::ORANGE_400),
                ("ORANGE_500", palette::ORANGE_500),
                ("ORANGE_600", palette::ORANGE_600),
                ("ORANGE_700", palette::ORANGE_700),
                ("ORANGE_800", palette::ORANGE_800),
                ("ORANGE_900", palette::ORANGE_900),
                ("ORANGE_950", palette::ORANGE_950),
            ],
        ),
        (
            "Amber",
            &[
                ("AMBER_50", palette::AMBER_50),
                ("AMBER_100", palette::AMBER_100),
                ("AMBER_200", palette::AMBER_200),
                ("AMBER_300", palette::AMBER_300),
                ("AMBER_400", palette::AMBER_400),
                ("AMBER_500", palette::AMBER_500),
                ("AMBER_600", palette::AMBER_600),
                ("AMBER_700", palette::AMBER_700),
                ("AMBER_800", palette::AMBER_800),
                ("AMBER_900", palette::AMBER_900),
                ("AMBER_950", palette::AMBER_950),
            ],
        ),
        (
            "Emerald",
            &[
                ("EMERALD_50", palette::EMERALD_50),
                ("EMERALD_100", palette::EMERALD_100),
                ("EMERALD_200", palette::EMERALD_200),
                ("EMERALD_300", palette::EMERALD_300),
                ("EMERALD_400", palette::EMERALD_400),
                ("EMERALD_500", palette::EMERALD_500),
                ("EMERALD_600", palette::EMERALD_600),
                ("EMERALD_700", palette::EMERALD_700),
                ("EMERALD_800", palette::EMERALD_800),
                ("EMERALD_900", palette::EMERALD_900),
                ("EMERALD_950", palette::EMERALD_950),
            ],
        ),
        (
            "Teal",
            &[
                ("TEAL_50", palette::TEAL_50),
                ("TEAL_100", palette::TEAL_100),
                ("TEAL_200", palette::TEAL_200),
                ("TEAL_300", palette::TEAL_300),
                ("TEAL_400", palette::TEAL_400),
                ("TEAL_500", palette::TEAL_500),
                ("TEAL_600", palette::TEAL_600),
                ("TEAL_700", palette::TEAL_700),
                ("TEAL_800", palette::TEAL_800),
                ("TEAL_900", palette::TEAL_900),
                ("TEAL_950", palette::TEAL_950),
            ],
        ),
        (
            "Sky",
            &[
                ("SKY_50", palette::SKY_50),
                ("SKY_100", palette::SKY_100),
                ("SKY_200", palette::SKY_200),
                ("SKY_300", palette::SKY_300),
                ("SKY_400", palette::SKY_400),
                ("SKY_500", palette::SKY_500),
                ("SKY_600", palette::SKY_600),
                ("SKY_700", palette::SKY_700),
                ("SKY_800", palette::SKY_800),
                ("SKY_900", palette::SKY_900),
                ("SKY_950", palette::SKY_950),
            ],
        ),
        (
            "Blue",
            &[
                ("BLUE_50", palette::BLUE_50),
                ("BLUE_100", palette::BLUE_100),
                ("BLUE_200", palette::BLUE_200),
                ("BLUE_300", palette::BLUE_300),
                ("BLUE_400", palette::BLUE_400),
                ("BLUE_500", palette::BLUE_500),
                ("BLUE_600", palette::BLUE_600),
                ("BLUE_700", palette::BLUE_700),
                ("BLUE_800", palette::BLUE_800),
                ("BLUE_900", palette::BLUE_900),
                ("BLUE_950", palette::BLUE_950),
            ],
        ),
        (
            "Indigo",
            &[
                ("INDIGO_50", palette::INDIGO_50),
                ("INDIGO_100", palette::INDIGO_100),
                ("INDIGO_200", palette::INDIGO_200),
                ("INDIGO_300", palette::INDIGO_300),
                ("INDIGO_400", palette::INDIGO_400),
                ("INDIGO_500", palette::INDIGO_500),
                ("INDIGO_600", palette::INDIGO_600),
                ("INDIGO_700", palette::INDIGO_700),
                ("INDIGO_800", palette::INDIGO_800),
                ("INDIGO_900", palette::INDIGO_900),
                ("INDIGO_950", palette::INDIGO_950),
            ],
        ),
        (
            "Violet",
            &[
                ("VIOLET_50", palette::VIOLET_50),
                ("VIOLET_100", palette::VIOLET_100),
                ("VIOLET_200", palette::VIOLET_200),
                ("VIOLET_300", palette::VIOLET_300),
                ("VIOLET_400", palette::VIOLET_400),
                ("VIOLET_500", palette::VIOLET_500),
                ("VIOLET_600", palette::VIOLET_600),
                ("VIOLET_700", palette::VIOLET_700),
                ("VIOLET_800", palette::VIOLET_800),
                ("VIOLET_900", palette::VIOLET_900),
                ("VIOLET_950", palette::VIOLET_950),
            ],
        ),
        (
            "Pink",
            &[
                ("PINK_50", palette::PINK_50),
                ("PINK_100", palette::PINK_100),
                ("PINK_200", palette::PINK_200),
                ("PINK_300", palette::PINK_300),
                ("PINK_400", palette::PINK_400),
                ("PINK_500", palette::PINK_500),
                ("PINK_600", palette::PINK_600),
                ("PINK_700", palette::PINK_700),
                ("PINK_800", palette::PINK_800),
                ("PINK_900", palette::PINK_900),
                ("PINK_950", palette::PINK_950),
            ],
        ),
        (
            "Rose",
            &[
                ("ROSE_50", palette::ROSE_50),
                ("ROSE_100", palette::ROSE_100),
                ("ROSE_200", palette::ROSE_200),
                ("ROSE_300", palette::ROSE_300),
                ("ROSE_400", palette::ROSE_400),
                ("ROSE_500", palette::ROSE_500),
                ("ROSE_600", palette::ROSE_600),
                ("ROSE_700", palette::ROSE_700),
                ("ROSE_800", palette::ROSE_800),
                ("ROSE_900", palette::ROSE_900),
                ("ROSE_950", palette::ROSE_950),
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

//! Shared playground UI helpers: live preview stage, props panel, and copyable Rust snippet box.

use gpui::components::*;
use gpui::icons::{Icon, IconSize, LucideIcon};
use gpui::{
    App, ClickEvent, ClipboardItem, ElementId, FontWeight, IntoElement, SharedString, Window, div,
    prelude::*, px,
};

/// Renders a Storybook-style Rust code block with monospace font and one-click Copy button.
pub fn code_snippet(
    id: impl Into<ElementId>,
    title: impl Into<SharedString>,
    code: String,
    on_copied: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let id = id.into();
    let copy_text = code.clone();
    let is_dark = theme.mode == ThemeMode::Dark;
    let code_bg = if is_dark {
        palette::SLATE_950
    } else {
        palette::SLATE_900
    };

    div()
        .flex()
        .flex_col()
        .w_full()
        .rounded(theme.radius_lg)
        .overflow_hidden()
        .border_1()
        .border_color(if is_dark {
            theme.border
        } else {
            palette::SLATE_800.into()
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .px(px(14.))
                .py(px(8.))
                .bg(palette::SLATE_800)
                .child(
                    h_stack()
                        .spacing(px(8.))
                        .child(
                            Icon::new(LucideIcon::CodeXml)
                                .size(IconSize::Sm)
                                .color(palette::INDIGO_400),
                        )
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(palette::SLATE_200)
                                .child(title.into()),
                        ),
                )
                .child(
                    div()
                        .id(id.clone())
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(5.))
                        .px(px(8.))
                        .py(px(3.))
                        .rounded(theme.radius_sm)
                        .bg(palette::SLATE_700)
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(palette::SLATE_100)
                        .cursor_pointer()
                        .hover(|s| s.bg(palette::SLATE_600))
                        .on_click(move |ev, window, cx| {
                            cx.write_to_clipboard(ClipboardItem::new_string(copy_text.clone()));
                            on_copied(ev, window, cx);
                        })
                        .child(
                            Icon::new(LucideIcon::Copy)
                                .size(IconSize::Xs)
                                .color(palette::SLATE_200),
                        )
                        .child("Copy Rust"),
                ),
        )
        .child(
            // Code keeps its lines: long ones scroll sideways.
            div()
                .id((id, "body"))
                .flex()
                .flex_col()
                .gap(px(2.))
                .w_full()
                .overflow_x_scroll()
                .p(px(14.))
                .bg(code_bg)
                .font_family("Lilex")
                .text_size(px(12.5))
                .line_height(px(19.))
                .text_color(palette::SLATE_100)
                .children(code.lines().map(|line| {
                    div().whitespace_nowrap().child(if line.is_empty() {
                        SharedString::new_static(" ")
                    } else {
                        SharedString::from(line.to_owned())
                    })
                })),
        )
}

/// Centered checkered/subtle stage container for previewing a component cleanly.
pub fn preview_stage(child: impl IntoElement, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex()
        .items_center()
        .justify_center()
        .w_full()
        .min_h(px(128.))
        .p(px(24.))
        .rounded(theme.radius_lg)
        .bg(theme.background)
        .border_1()
        .border_color(theme.border)
        .child(child)
}

/// Compact labeled property control row inside a workbench inspector.
pub fn prop_chip_row(
    label: impl Into<SharedString>,
    control: impl IntoElement,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    div()
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(12.))
        .w_full()
        .child(
            div()
                .text_size(px(13.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(label.into()),
        )
        .child(control)
}

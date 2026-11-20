//! Structured action [`List`] and `uniform_list`-backed [`VirtualList`] for rendering
//! rich list rows (leading avatar/icon, title, subtitle, trailing metadata/actions) cleanly
//! across both small collections and 10,000+ virtualized rows.

use std::ops::Range;

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement, MouseButton, Pixels,
    RenderOnce, SharedString, StyleRefinement, Styled, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};

use super::theme::ActiveTheme;

/// Single structured row inside a [`List`].
pub struct ListItem {
    leading: Option<AnyElement>,
    title: SharedString,
    subtitle: Option<SharedString>,
    trailing: SmallVec<[AnyElement; 2]>,
    selected: bool,
    disabled: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl ListItem {
    /// Creates a new [`ListItem`] with `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            leading: None,
            title: title.into(),
            subtitle: None,
            trailing: SmallVec::new(),
            selected: false,
            disabled: false,
            on_click: None,
        }
    }

    /// Sets a leading visual element (e.g. [`super::Avatar`] or [`crate::icons::Icon`]).
    pub fn leading(mut self, element: impl IntoElement) -> Self {
        self.leading = Some(element.into_any_element());
        self
    }

    /// Sets a secondary muted subtitle line beneath the title.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }

    /// Appends a trailing metadata badge or action button on the right side.
    pub fn trailing(mut self, element: impl IntoElement) -> Self {
        self.trailing.push(element.into_any_element());
        self
    }

    /// Highlights this row as selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Disables clicking this row.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a row click handler.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

/// Structured action list with optional header/footer, dividers, borders, and compact density.
#[derive(IntoElement)]
pub struct List {
    id: ElementId,
    header: Option<AnyElement>,
    footer: Option<AnyElement>,
    items: Vec<ListItem>,
    bordered: bool,
    divided: bool,
    compact: bool,
    empty_message: SharedString,
    style: StyleRefinement,
}

/// Convenience constructor for [`List::new`].
pub fn list(id: impl Into<ElementId>) -> List {
    List::new(id)
}

impl List {
    /// Creates a new bordered & divided [`List`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            header: None,
            footer: None,
            items: Vec::new(),
            bordered: true,
            divided: true,
            compact: false,
            empty_message: "No items.".into(),
            style: StyleRefinement::default(),
        }
    }

    /// Sets an optional top header slot.
    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    /// Sets an optional bottom footer slot.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Appends a single [`ListItem`].
    pub fn item(mut self, item: ListItem) -> Self {
        self.items.push(item);
        self
    }

    /// Appends multiple [`ListItem`]s.
    pub fn items(mut self, items: impl IntoIterator<Item = ListItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Enables or disables the outer rounded border card frame (default `true`).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Enables or disables 1px dividers between consecutive items (default `true`).
    pub fn divided(mut self, divided: bool) -> Self {
        self.divided = divided;
        self
    }

    /// Uses tighter vertical row padding.
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Customizes the empty-list fallback message.
    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.empty_message = message.into();
        self
    }
}

impl Styled for List {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for List {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let pad_y = if self.compact { px(8.) } else { px(12.) };
        let pad_x = px(14.);
        let hover_bg = theme.surface_hover;
        let count = self.items.len();

        let mut root = div()
            .id(self.id.clone())
            .arrow_navigation(crate::ArrowNavigation::Vertical)
            .flex()
            .flex_col()
            .w_full()
            .when(self.bordered, |el| {
                el.rounded(theme.radius_xl)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .overflow_hidden()
            })
            .when_some(self.header, |el, header| {
                el.child(
                    div()
                        .px(pad_x)
                        .py(px(10.))
                        .bg(theme.surface_muted.opacity(0.55))
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(header),
                )
            });

        if self.items.is_empty() {
            root = root.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .py(px(28.))
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .child(self.empty_message),
            );
        } else {
            let selected_hover_bg = theme.primary_soft.opacity(0.8);
            for (idx, item) in self.items.into_iter().enumerate() {
                let is_last = idx + 1 == count;
                let bg = if item.selected {
                    theme.primary_soft.opacity(0.6)
                } else {
                    crate::transparent_black()
                };
                let row_hover = if item.selected {
                    selected_hover_bg
                } else {
                    hover_bg.opacity(0.75)
                };
                let clickable = !item.disabled && item.on_click.is_some();

                // A clickable row is a tab stop, like a link. The bordered
                // list clips its children, so the ring is drawn inside.
                let row = super::focus::tab_stop_inset(div().id(("li", idx)), clickable, &theme);
                root = root.child(
                    row
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .w_full()
                        .px(pad_x)
                        .py(pad_y)
                        .bg(bg)
                        .when(self.divided && !is_last, |el| {
                            el.border_b_1().border_color(theme.border)
                        })
                        .when(clickable, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.bg(row_hover))
                                .when_some(item.on_click, |el, cb| {
                                    el.on_click(move |ev, window, cx| cb(ev, window, cx))
                                })
                        })
                        .when(item.disabled, |el| el.opacity(0.5).cursor_not_allowed())
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.))
                                .flex_1()
                                .min_w_0()
                                .when_some(item.leading, |el, lead| el.child(lead))
                                .child(
                                    div()
                                        .flex()
                                        .flex_col()
                                        .gap(px(2.))
                                        .flex_1()
                                        .min_w_0()
                                        .child(
                                            div()
                                                .text_size(px(13.5))
                                                .font_weight(FontWeight::MEDIUM)
                                                .text_color(theme.foreground)
                                                .truncate()
                                                .child(item.title),
                                        )
                                        .when_some(item.subtitle, |el, sub| {
                                            el.child(
                                                div()
                                                    .text_size(px(12.))
                                                    .text_color(theme.muted_foreground)
                                                    .truncate()
                                                    .child(sub),
                                            )
                                        }),
                                ),
                        )
                        .when(!item.trailing.is_empty(), |el| {
                            el.child(
                                div()
                                    .id(("li-tr", idx))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .gap(px(6.))
                                    .flex_shrink_0()
                                    .when(clickable, |el| {
                                        el.on_mouse_down(MouseButton::Left, |_, _, cx| {
                                            cx.stop_propagation()
                                        })
                                        .on_click(|_, _, cx| cx.stop_propagation())
                                    })
                                    .children(item.trailing),
                            )
                        }),
                );
            }
        }

        if let Some(footer) = self.footer {
            root = root.child(
                div()
                    .px(pad_x)
                    .py(px(10.))
                    .bg(theme.surface_muted.opacity(0.4))
                    .border_t_1()
                    .border_color(theme.border)
                    .text_size(px(12.))
                    .text_color(theme.muted_foreground)
                    .child(footer),
            );
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// High-performance `uniform_list`-backed virtualized list capable of smoothly scrolling
/// tens of thousands of uniform-height rows at O(visible rows) cost.
#[derive(IntoElement)]
pub struct VirtualList {
    id: ElementId,
    item_count: usize,
    viewport_height: Pixels,
    bordered: bool,
    header: Option<AnyElement>,
    footer: Option<AnyElement>,
    empty_message: SharedString,
    scroll_handle: Option<UniformListScrollHandle>,
    render_item: Box<dyn Fn(usize, &mut Window, &mut App) -> AnyElement + 'static>,
    style: StyleRefinement,
}

/// Convenience constructor for [`VirtualList::new`].
pub fn virtual_list(
    id: impl Into<ElementId>,
    item_count: usize,
    render_item: impl Fn(usize, &mut Window, &mut App) -> AnyElement + 'static,
) -> VirtualList {
    VirtualList::new(id, item_count, render_item)
}

impl VirtualList {
    /// Creates a new [`VirtualList`] displaying `item_count` virtualized rows.
    pub fn new(
        id: impl Into<ElementId>,
        item_count: usize,
        render_item: impl Fn(usize, &mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            item_count,
            viewport_height: px(280.),
            bordered: true,
            header: None,
            footer: None,
            empty_message: "No items.".into(),
            scroll_handle: None,
            render_item: Box::new(render_item),
            style: StyleRefinement::default(),
        }
    }

    /// Sets the scrollable list viewport height (default `280px`).
    pub fn viewport_height(mut self, height: Pixels) -> Self {
        self.viewport_height = if height.0.is_finite() {
            height.max(px(0.))
        } else {
            px(280.)
        };
        self
    }

    /// Enables or disables the rounded bordered surface container (default `true`).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Sets an optional pinned header above the virtual scroll area.
    pub fn header(mut self, header: impl IntoElement) -> Self {
        self.header = Some(header.into_any_element());
        self
    }

    /// Sets an optional pinned footer below the virtual scroll area.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }

    /// Customizes message shown when `item_count == 0`.
    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Attaches an external [`UniformListScrollHandle`].
    pub fn track_scroll(mut self, handle: UniformListScrollHandle) -> Self {
        self.scroll_handle = Some(handle);
        self
    }
}

impl Styled for VirtualList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for VirtualList {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let render_item = self.render_item;
        let has_external_height =
            self.style.size.height.is_some() || self.style.flex_grow.is_some();

        let body: AnyElement = if self.item_count == 0 {
            div()
                .flex()
                .items_center()
                .justify_center()
                .w_full()
                .py(px(28.))
                .text_size(px(13.))
                .text_color(theme.muted_foreground)
                .child(self.empty_message)
                .into_any_element()
        } else {
            let mut list = uniform_list(
                "vlist-scroll",
                self.item_count,
                move |range: Range<usize>, window, cx| {
                    range
                        .map(|idx| render_item(idx, window, cx))
                        .collect()
                },
            )
            .w_full()
            .when(has_external_height, |el| el.flex_1().min_h_0())
            .when(!has_external_height, |el| el.h(self.viewport_height));

            if let Some(handle) = self.scroll_handle {
                list = list.track_scroll(&handle);
            }
            list.into_any_element()
        };

        let mut root = div()
            .id(self.id)
            .flex()
            .flex_col()
            .w_full()
            .when(self.bordered, |el| {
                el.rounded(theme.radius_xl)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .overflow_hidden()
            })
            .when_some(self.header, |el, h| {
                el.child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .bg(theme.surface_muted.opacity(0.55))
                        .border_b_1()
                        .border_color(theme.border)
                        .text_size(px(12.5))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .child(h),
                )
            })
            .child(body)
            .when_some(self.footer, |el, f| {
                el.child(
                    div()
                        .px(px(14.))
                        .py(px(10.))
                        .bg(theme.surface_muted.opacity(0.4))
                        .border_t_1()
                        .border_color(theme.border)
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .child(f),
                )
            });

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

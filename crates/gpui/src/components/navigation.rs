//! Navigation components: [`Breadcrumb`] and [`Pagination`].

use std::rc::Rc;

use crate::{
    App, ClickEvent, ElementId, FontWeight, IntoElement, RenderOnce, SharedString, StyleRefinement,
    Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::ActiveTheme;

/// Single segment inside a [`Breadcrumb`] trail.
pub struct BreadcrumbItem {
    /// Segment label.
    pub label: SharedString,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional click handler (omitted for the active terminal page).
    pub on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl BreadcrumbItem {
    /// Creates a terminal (non-clickable) breadcrumb item.
    pub fn current(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            icon: None,
            on_click: None,
        }
    }

    /// Creates a clickable ancestor breadcrumb item.
    pub fn link(
        label: impl Into<SharedString>,
        on_click: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        Self {
            label: label.into(),
            icon: None,
            on_click: Some(Box::new(on_click)),
        }
    }

    /// Adds a leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Horizontal breadcrumb trail separated by [`LucideIcon::ChevronRight`].
#[derive(IntoElement)]
pub struct Breadcrumb {
    id: ElementId,
    items: Vec<BreadcrumbItem>,
    separator: LucideIcon,
    style: StyleRefinement,
}

/// Convenience constructor for [`Breadcrumb::new`].
pub fn breadcrumb(id: impl Into<ElementId>, items: Vec<BreadcrumbItem>) -> Breadcrumb {
    Breadcrumb::new(id, items)
}

impl Breadcrumb {
    /// Creates a new [`Breadcrumb`] bar.
    pub fn new(id: impl Into<ElementId>, items: Vec<BreadcrumbItem>) -> Self {
        Self {
            id: id.into(),
            items,
            separator: LucideIcon::ChevronRight,
            style: StyleRefinement::default(),
        }
    }

    /// Overrides the separator icon (defaults to [`LucideIcon::ChevronRight`]).
    pub fn separator(mut self, icon: LucideIcon) -> Self {
        self.separator = icon;
        self
    }
}

impl Styled for Breadcrumb {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Breadcrumb {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let count = self.items.len();
        let mut root = div()
            .id(self.id.clone())
            .flex()
            .flex_row()
            .flex_wrap()
            .items_center()
            .gap(px(6.));

        for (idx, item) in self.items.into_iter().enumerate() {
            let is_last = idx + 1 == count;
            let fg = if is_last {
                theme.foreground
            } else {
                theme.muted_foreground
            };

            let mut seg = div()
                .id((self.id.clone(), format!("crumb-{idx}")))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(5.))
                .text_size(px(13.))
                .font_weight(if is_last {
                    FontWeight::SEMIBOLD
                } else {
                    FontWeight::MEDIUM
                })
                .text_color(fg)
                .when_some(item.icon, |el, icon| {
                    el.child(Icon::new(icon).size(IconSize::Sm).color(fg))
                })
                .child(item.label);

            if let Some(handler) = item.on_click {
                seg = super::focus::tab_stop(seg, true, &theme)
                    .cursor_pointer()
                    .hover(|s| s.text_color(theme.foreground).underline())
                    .on_click(move |ev, window, cx| handler(ev, window, cx));
            }

            root = root.child(seg);
            if !is_last {
                root = root.child(
                    Icon::new(self.separator)
                        .size(IconSize::Sm)
                        .color(theme.muted_foreground.opacity(0.65)),
                );
            }
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Numeric [`Pagination`] bar with Previous/Next controls and smart ellipsis collapsing.
#[derive(IntoElement)]
pub struct Pagination {
    id: ElementId,
    current_page: usize,
    total_pages: usize,
    on_page_change: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Pagination::new`].
pub fn pagination(
    id: impl Into<ElementId>,
    current_page: usize,
    total_pages: usize,
) -> Pagination {
    Pagination::new(id, current_page, total_pages)
}

impl Pagination {
    /// Creates a 1-indexed [`Pagination`] bar (`current_page` clamped to `1..=total_pages`).
    pub fn new(id: impl Into<ElementId>, current_page: usize, total_pages: usize) -> Self {
        let total = total_pages.max(1);
        Self {
            id: id.into(),
            current_page: current_page.clamp(1, total),
            total_pages: total,
            on_page_change: None,
            style: StyleRefinement::default(),
        }
    }

    /// Registers a callback invoked with the newly chosen 1-based page number.
    pub fn on_page_change(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_page_change = Some(Rc::new(handler));
        self
    }

    fn visible_slots(&self) -> Vec<Option<usize>> {
        if self.total_pages <= 7 {
            return (1..=self.total_pages).map(Some).collect();
        }
        let mut pages = vec![Some(1)];
        let left = self.current_page.saturating_sub(1).max(2);
        let right = (self.current_page + 1).min(self.total_pages - 1);
        if left > 2 {
            pages.push(None);
        }
        for p in left..=right {
            pages.push(Some(p));
        }
        if right < self.total_pages - 1 {
            pages.push(None);
        }
        pages.push(Some(self.total_pages));
        pages
    }
}

impl Styled for Pagination {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Pagination {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let has_prev = self.current_page > 1;
        let has_next = self.current_page < self.total_pages;
        let prev_page = self.current_page.saturating_sub(1).max(1);
        let next_page = (self.current_page + 1).min(self.total_pages);
        let slots = self.visible_slots();

        let on_prev = self.on_page_change.clone();
        let on_next = self.on_page_change.clone();
        let operable = self.on_page_change.is_some();

        let prev_btn = div()
            .id((self.id.clone(), "prev"))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .h(px(34.))
            .px(px(10.))
            .rounded(theme.radius_md)
            .border_1()
            .border_color(theme.input_border)
            .bg(theme.surface)
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.foreground)
            .when(has_prev, |el| {
                el.cursor_pointer()
                    .hover(|s| s.bg(theme.surface_hover))
                    .when_some(on_prev, |el, cb| {
                        el.on_click(move |_, window, cx| cb(&prev_page, window, cx))
                    })
            })
            .when(!has_prev, |el| el.opacity(0.45).cursor_not_allowed())
            .child(Icon::new(LucideIcon::ChevronLeft).size(IconSize::Sm))
            .child("Previous");
        let prev_btn = super::focus::tab_stop(prev_btn, operable && has_prev, &theme);

        let next_btn = div()
            .id((self.id.clone(), "next"))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .h(px(34.))
            .px(px(10.))
            .rounded(theme.radius_md)
            .border_1()
            .border_color(theme.input_border)
            .bg(theme.surface)
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .text_color(theme.foreground)
            .when(has_next, |el| {
                el.cursor_pointer()
                    .hover(|s| s.bg(theme.surface_hover))
                    .when_some(on_next, |el, cb| {
                        el.on_click(move |_, window, cx| cb(&next_page, window, cx))
                    })
            })
            .when(!has_next, |el| el.opacity(0.45).cursor_not_allowed())
            .child("Next")
            .child(Icon::new(LucideIcon::ChevronRight).size(IconSize::Sm));
        let next_btn = super::focus::tab_stop(next_btn, operable && has_next, &theme);

        let mut root = div()
            .id(self.id.clone())
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(prev_btn);

        for (slot_idx, slot) in slots.into_iter().enumerate() {
            match slot {
                None => {
                    root = root.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .w(px(32.))
                            .h(px(34.))
                            .text_size(px(13.))
                            .text_color(theme.muted_foreground)
                            .child("…"),
                    );
                }
                Some(page) => {
                    let active = page == self.current_page;
                    let on_change = self.on_page_change.clone();
                    let page_btn = div()
                        .id((self.id.clone(), format!("page-{slot_idx}")))
                        .flex()
                        .items_center()
                        .justify_center()
                        .min_w(px(34.))
                        .h(px(34.))
                        .px(px(8.))
                        .rounded(theme.radius_md)
                        .border_1()
                        .border_color(if active {
                            theme.primary
                        } else {
                            theme.input_border
                        })
                        .bg(if active { theme.primary } else { theme.surface })
                        .text_color(if active {
                            theme.primary_foreground
                        } else {
                            theme.foreground
                        })
                        .text_size(px(13.))
                        .font_weight(if active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .cursor_pointer()
                        .when(!active, |el| el.hover(|s| s.bg(theme.surface_hover)))
                        .when_some(on_change, |el, cb| {
                            el.on_click(move |_, window, cx| cb(&page, window, cx))
                        })
                        .child(page.to_string());
                    root = root.child(super::focus::tab_stop(page_btn, operable, &theme));
                }
            }
        }

        root = root.child(next_btn);
        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

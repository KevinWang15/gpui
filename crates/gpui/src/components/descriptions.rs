//! Structured multi-column key-value [`Descriptions`] metadata grid (bordered or clean),
//! supporting column spans, header title/extra actions, compact density, and one-click copy.

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ElementId, FontWeight, IntoElement, Pixels, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};

use super::interactive::copy_button;
use super::theme::{ActiveTheme, ComponentSize};

/// Single key-value entry inside a [`Descriptions`] grid.
pub struct DescriptionItem {
    /// Field label shown on the left or top of the cell.
    pub label: SharedString,
    /// Rendered value element.
    pub value: AnyElement,
    /// Number of grid columns this item spans (default `1`).
    pub span: u16,
    /// Optional raw text copied when clicking the inline copy button.
    pub copyable: Option<SharedString>,
}

impl DescriptionItem {
    /// Creates a [`DescriptionItem`] from a text value.
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            value: div().child(value.into()).into_any_element(),
            span: 1,
            copyable: None,
        }
    }

    /// Creates a [`DescriptionItem`] from any custom GPUI element (badge, avatar, link, etc.).
    pub fn element(label: impl Into<SharedString>, value: impl IntoElement) -> Self {
        Self {
            label: label.into(),
            value: value.into_any_element(),
            span: 1,
            copyable: None,
        }
    }

    /// Sets how many columns this entry spans (`1..=columns`).
    pub fn span(mut self, span: u16) -> Self {
        self.span = span.max(1);
        self
    }

    /// Adds a compact inline [`super::CopyButton`] next to the value.
    pub fn copyable(mut self, text: impl Into<SharedString>) -> Self {
        self.copyable = Some(text.into());
        self
    }
}

/// Structured key-value metadata grid ideal for detail pages, cards, and inspector drawers.
#[derive(IntoElement)]
pub struct Descriptions {
    id: ElementId,
    title: Option<SharedString>,
    extra: Option<AnyElement>,
    columns: u16,
    label_width: Pixels,
    bordered: bool,
    compact: bool,
    items: SmallVec<[DescriptionItem; 8]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Descriptions::new`].
pub fn descriptions(id: impl Into<ElementId>) -> Descriptions {
    Descriptions::new(id)
}

impl Descriptions {
    /// Creates a 2-column [`Descriptions`] grid.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            title: None,
            extra: None,
            columns: 2,
            label_width: px(132.),
            bordered: true,
            compact: false,
            items: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Sets the fixed label cell width in bordered mode (default `132px`).
    pub fn label_width(mut self, width: Pixels) -> Self {
        self.label_width = if width.0.is_finite() {
            width.max(px(48.))
        } else {
            px(132.)
        };
        self
    }

    /// Sets an optional heading above the metadata grid.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets an optional right-aligned header element (e.g. an Edit button or status badge).
    pub fn extra(mut self, extra: impl IntoElement) -> Self {
        self.extra = Some(extra.into_any_element());
        self
    }

    /// Sets the number of columns (`1..=4`, default `2`).
    pub fn columns(mut self, cols: u16) -> Self {
        self.columns = cols.clamp(1, 4);
        self
    }

    /// Enables or disables table-style borders and shaded label cells (default `true`).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Reduces vertical padding for dense inspector views.
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Appends a [`DescriptionItem`].
    pub fn item(mut self, item: DescriptionItem) -> Self {
        self.items.push(item);
        self
    }

    /// Appends multiple [`DescriptionItem`]s.
    pub fn items(mut self, items: impl IntoIterator<Item = DescriptionItem>) -> Self {
        self.items.extend(items);
        self
    }
}

impl Styled for Descriptions {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Descriptions {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let cols = self.columns as usize;
        let pad_y = if self.compact { px(8.) } else { px(11.) };
        let pad_x = if self.compact { px(12.) } else { px(14.) };

        // Pack items into rows respecting `span`; expand the final item of any incomplete row
        // so vertical column borders align across all rows (matching Ant Design's Descriptions).
        let mut rows: Vec<Vec<(usize, DescriptionItem, usize)>> = Vec::new();
        let mut current_row: Vec<(usize, DescriptionItem, usize)> = Vec::new();
        let mut used = 0usize;
        for (idx, item) in self.items.into_iter().enumerate() {
            let span = (item.span as usize).min(cols).max(1);
            if used + span > cols && !current_row.is_empty() {
                if let Some(last) = current_row.last_mut() {
                    last.2 += cols - used;
                }
                rows.push(current_row);
                current_row = Vec::new();
                used = 0;
            }
            current_row.push((idx, item, span));
            used += span;
            if used >= cols {
                rows.push(current_row);
                current_row = Vec::new();
                used = 0;
            }
        }
        if !current_row.is_empty() {
            if used < cols
                && let Some(last) = current_row.last_mut()
            {
                last.2 += cols - used;
            }
            rows.push(current_row);
        }

        let row_count = rows.len();
        let body = (row_count > 0).then(|| {
            let mut body = div()
                .flex()
                .flex_col()
                .w_full()
                .when(self.bordered, |el| {
                    el.rounded(theme.radius_lg)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface)
                        .overflow_hidden()
                });

            for (row_idx, row_items) in rows.into_iter().enumerate() {
                let is_last_row = row_idx + 1 == row_count;
                let mut row_el = div()
                    .flex()
                    .flex_row()
                    .w_full()
                    .when(self.bordered && !is_last_row, |el| {
                        el.border_b_1().border_color(theme.border)
                    })
                    .when(!self.bordered, |el| el.gap(px(16.)).py(px(4.)));

                let item_count = row_items.len();
                for (pos, (orig_idx, item, span)) in row_items.into_iter().enumerate() {
                    let is_last_in_row = pos + 1 == item_count;
                    let value_row = div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .flex_1()
                        .min_w_0()
                        .text_size(px(13.))
                        .text_color(theme.foreground)
                        .child(div().flex_1().min_w_0().child(item.value))
                        .when_some(item.copyable, |el, txt| {
                            el.child(
                                copy_button(("desc-cp", orig_idx), txt)
                                    .icon_only()
                                    .variant(super::button::ButtonVariant::Ghost)
                                    .size(ComponentSize::Xs),
                            )
                        });

                    let cell = if self.bordered {
                        div()
                            .flex()
                            .flex_row()
                            .items_stretch()
                            .flex_grow(span as f32)
                            .flex_basis(px(0.))
                            .min_w_0()
                            .when(!is_last_in_row, |el| {
                                el.border_r_1().border_color(theme.border)
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_shrink_0()
                                    .w(self.label_width)
                                    .overflow_hidden()
                                    .px(pad_x)
                                    .py(pad_y)
                                    .bg(theme.surface_muted.opacity(0.6))
                                    .border_r_1()
                                    .border_color(theme.border)
                                    .text_size(px(12.5))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.muted_foreground)
                                    .child(div().min_w_0().truncate().child(item.label)),
                            )
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .flex_1()
                                    .min_w_0()
                                    .px(pad_x)
                                    .py(pad_y)
                                    .child(value_row),
                            )
                    } else {
                        div()
                            .flex()
                            .flex_col()
                            .gap(px(3.))
                            .flex_grow(span as f32)
                            .flex_basis(px(0.))
                            .min_w_0()
                            .py(px(4.))
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.muted_foreground)
                                    .truncate()
                                    .child(item.label),
                            )
                            .child(value_row)
                    };

                    row_el = row_el.child(cell);
                }

                body = body.child(row_el);
            }
            body
        });

        let mut root = div()
            .id(self.id)
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .when(self.title.is_some() || self.extra.is_some(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .child(
                            div()
                                .flex_1()
                                .min_w_0()
                                .truncate()
                                .text_size(px(15.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .children(self.title),
                        )
                        .children(self.extra),
                )
            })
            .children(body);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

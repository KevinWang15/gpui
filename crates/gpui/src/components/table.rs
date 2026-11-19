//! Tailwind UI Data [`Table`] with column definitions, sortable headers, alignment,
//! striped/hoverable rows, compact density, and empty state fallback.

use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement, Pixels, RenderOnce,
    SharedString, StyleRefinement, Styled, Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::ActiveTheme;

/// Horizontal cell alignment for [`TableColumn`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TableAlign {
    /// Left-aligned cell content.
    #[default]
    Left,
    /// Center-aligned cell content.
    Center,
    /// Right-aligned cell content (ideal for numbers & currency).
    Right,
}

/// Sort state indicator for sortable [`TableColumn`] headers.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortDirection {
    /// Ascending sort (`ArrowUp`).
    Ascending,
    /// Descending sort (`ArrowDown`).
    Descending,
}

/// Column definition inside a [`Table`].
#[derive(Clone, Debug)]
pub struct TableColumn {
    /// Header title text.
    pub title: SharedString,
    /// Optional fixed pixel width; flexes equally when `None`.
    pub width: Option<Pixels>,
    /// Cell text/flex alignment.
    pub align: TableAlign,
    /// Whether clicking the header triggers `on_sort`.
    pub sortable: bool,
    /// Current active sort direction on this column.
    pub sort: Option<SortDirection>,
}

impl TableColumn {
    /// Creates a new flexible left-aligned column.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            width: None,
            align: TableAlign::Left,
            sortable: false,
            sort: None,
        }
    }

    /// Assigns a fixed width to this column.
    pub fn width(mut self, width: Pixels) -> Self {
        self.width = Some(width);
        self
    }

    /// Sets horizontal alignment.
    pub fn align(mut self, align: TableAlign) -> Self {
        self.align = align;
        self
    }

    /// Marks this column as sortable with an optional active [`SortDirection`].
    pub fn sortable(mut self, sort: Option<SortDirection>) -> Self {
        self.sortable = true;
        self.sort = sort;
        self
    }
}

/// Single body row inside a [`Table`].
pub struct TableRow {
    cells: SmallVec<[AnyElement; 6]>,
    selected: bool,
    on_click: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
}

impl Default for TableRow {
    fn default() -> Self {
        Self::new()
    }
}

impl TableRow {
    /// Creates an empty row.
    pub fn new() -> Self {
        Self {
            cells: SmallVec::new(),
            selected: false,
            on_click: None,
        }
    }

    /// Appends a cell element to the row.
    pub fn cell(mut self, cell: impl IntoElement) -> Self {
        self.cells.push(cell.into_any_element());
        self
    }

    /// Appends a plain text cell.
    pub fn text(self, text: impl Into<SharedString>) -> Self {
        self.cell(div().truncate().child(text.into()))
    }

    /// Marks this row as selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Registers a row click callback.
    pub fn on_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_click = Some(Box::new(handler));
        self
    }
}

/// Tailwind UI Data Table with header row, striped/hoverable rows, and column sort triggers.
#[derive(IntoElement)]
pub struct Table {
    id: ElementId,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    striped: bool,
    compact: bool,
    empty_message: SharedString,
    on_sort: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Table::new`].
pub fn table(id: impl Into<ElementId>, columns: Vec<TableColumn>) -> Table {
    Table::new(id, columns)
}

impl Table {
    /// Creates a new [`Table`] with `columns`.
    pub fn new(id: impl Into<ElementId>, columns: Vec<TableColumn>) -> Self {
        Self {
            id: id.into(),
            columns,
            rows: Vec::new(),
            striped: false,
            compact: false,
            empty_message: SharedString::new_static("No rows to display."),
            on_sort: None,
            style: StyleRefinement::default(),
        }
    }

    /// Appends a single [`TableRow`].
    pub fn row(mut self, row: TableRow) -> Self {
        self.rows.push(row);
        self
    }

    /// Appends multiple [`TableRow`]s.
    pub fn rows(mut self, rows: impl IntoIterator<Item = TableRow>) -> Self {
        self.rows.extend(rows);
        self
    }

    /// Alternates subtle background stripes on odd rows.
    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    /// Reduces vertical cell padding for dense data tables.
    pub fn compact(mut self, compact: bool) -> Self {
        self.compact = compact;
        self
    }

    /// Customizes message shown when there are zero rows.
    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Registers a callback invoked with the clicked sortable column index.
    pub fn on_sort(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_sort = Some(Rc::new(handler));
        self
    }
}

impl Styled for Table {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Table {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let pad_y = if self.compact { px(8.) } else { px(12.) };
        let pad_x = px(14.);

        let mut header_row = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .bg(theme.surface_muted.opacity(0.65))
            .border_b_1()
            .border_color(theme.border);

        for (col_idx, col) in self.columns.iter().enumerate() {
            let on_sort = self.on_sort.clone();
            let sort_icon = if col.sortable {
                Some(match col.sort {
                    Some(SortDirection::Ascending) => LucideIcon::ArrowUp,
                    Some(SortDirection::Descending) => LucideIcon::ArrowDown,
                    None => LucideIcon::ArrowUpDown,
                })
            } else {
                None
            };

            let cell = div()
                .id((self.id.clone(), format!("col-{col_idx}")))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(4.))
                .px(pad_x)
                .py(px(10.))
                .text_size(px(12.))
                .font_weight(FontWeight::SEMIBOLD)
                .text_color(theme.muted_foreground)
                .when_some(col.width, |el, w| el.w(w).flex_shrink_0())
                .when(col.width.is_none(), |el| el.flex_1().min_w_0())
                .when(col.align == TableAlign::Center, |el| el.justify_center())
                .when(col.align == TableAlign::Right, |el| el.justify_end())
                .when(col.sortable, |el| {
                    el.cursor_pointer()
                        .hover(|s| s.text_color(theme.foreground))
                        .when_some(on_sort, |el, handler| {
                            el.on_click(move |_, window, cx| handler(&col_idx, window, cx))
                        })
                })
                .child(div().truncate().child(col.title.clone()))
                .when_some(sort_icon, |el, icon| {
                    el.child(
                        Icon::new(icon)
                            .size(IconSize::Xs)
                            .color(if col.sort.is_some() {
                                theme.primary
                            } else {
                                theme.muted_foreground.opacity(0.65)
                            }),
                    )
                });

            header_row = header_row.child(cell);
        }

        let mut body = div().flex().flex_col().w_full();
        if self.rows.is_empty() {
            body = body.child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w_full()
                    .py(px(28.))
                    .text_size(px(13.5))
                    .text_color(theme.muted_foreground)
                    .child(self.empty_message),
            );
        } else {
            let row_count = self.rows.len();
            for (row_idx, row) in self.rows.into_iter().enumerate() {
                let row_bg = if row.selected {
                    theme.primary_soft.opacity(0.55)
                } else if self.striped && row_idx % 2 == 1 {
                    theme.surface_muted.opacity(0.35)
                } else {
                    theme.surface
                };

                let mut row_el = div()
                    .id((self.id.clone(), format!("row-{row_idx}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .bg(row_bg)
                    .text_size(px(13.5))
                    .text_color(theme.foreground)
                    .when(row_idx + 1 < row_count, |el| {
                        el.border_b_1().border_color(theme.border)
                    })
                    .hover(|s| s.bg(theme.surface_hover.opacity(0.75)))
                    .when_some(row.on_click, |el, handler| {
                        el.cursor_pointer()
                            .on_click(move |ev, window, cx| handler(ev, window, cx))
                    });

                for (col_idx, cell_el) in row.cells.into_iter().enumerate() {
                    let col = self.columns.get(col_idx);
                    let width = col.and_then(|c| c.width);
                    let align = col.map(|c| c.align).unwrap_or(TableAlign::Left);

                    row_el = row_el.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .px(pad_x)
                            .py(pad_y)
                            .when_some(width, |el, w| el.w(w).flex_shrink_0())
                            .when(width.is_none(), |el| el.flex_1().min_w_0())
                            .when(align == TableAlign::Center, |el| el.justify_center())
                            .when(align == TableAlign::Right, |el| el.justify_end())
                            .child(cell_el),
                    );
                }

                body = body.child(row_el);
            }
        }

        let mut root = div()
            .id(self.id)
            .flex()
            .flex_col()
            .flex_shrink_0()
            .w_full()
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .overflow_hidden()
            .shadow_2xs()
            .child(header_row)
            .child(body);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

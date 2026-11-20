//! Data [`Table`] & [`VirtualTable`] with column definitions, sortable headers,
//! alignment, checkbox multi-selection with tri-state select-all header, expandable detail rows,
//! sticky-header scrollable viewport, and `uniform_list`-virtualized large-dataset rendering.

use std::ops::Range;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    AnyElement, App, ClickEvent, ElementId, FontWeight, IntoElement, MouseButton, Pixels,
    RenderOnce, SharedString, StyleRefinement, Styled, UniformListScrollHandle, Window, div,
    prelude::*, px, uniform_list,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::checkbox::CheckState;
use super::focus::tab_stop_inset;
use super::theme::{ActiveTheme, tabular_numerals};

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
    checked: bool,
    expanded: bool,
    expanded_content: Option<AnyElement>,
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
            checked: false,
            expanded: false,
            expanded_content: None,
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

    /// Marks this row as highlighted/selected.
    pub fn selected(mut self, selected: bool) -> Self {
        self.selected = selected;
        self
    }

    /// Marks this row's leading checkbox checked (used with [`Table::selectable`]).
    pub fn checked(mut self, checked: bool) -> Self {
        self.checked = checked;
        self
    }

    /// Attaches an expandable detail drawer rendered beneath this row when `expanded == true`.
    pub fn expandable(mut self, expanded: bool, content: impl IntoElement) -> Self {
        self.expanded = expanded;
        self.expanded_content = Some(content.into_any_element());
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

/// Data Table with header row, striped/hoverable rows, column sort triggers,
/// checkbox multi-selection with tri-state select-all header, expandable detail rows, and
/// optional sticky-header scrolling via [`Table::max_height`].
#[derive(IntoElement)]
pub struct Table {
    id: ElementId,
    columns: Vec<TableColumn>,
    rows: Vec<TableRow>,
    striped: bool,
    compact: bool,
    selectable: bool,
    max_height: Option<Pixels>,
    empty_message: SharedString,
    on_sort: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    on_select_row: Option<Rc<dyn Fn(&(usize, bool), &mut Window, &mut App) + 'static>>,
    on_select_all: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    on_toggle_expand: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
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
            selectable: false,
            max_height: None,
            empty_message: SharedString::new_static("No rows to display."),
            on_sort: None,
            on_select_row: None,
            on_select_all: None,
            on_toggle_expand: None,
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

    /// Enables a leading checkbox column with automatic tri-state Select All header.
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Fixes the header at the top and makes the table body vertically scrollable up to `height`.
    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = height.0.is_finite().then(|| height.max(px(0.)));
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

    /// Registers a callback invoked with `(row_index, next_checked)` when a row checkbox toggles.
    pub fn on_select_row(
        mut self,
        handler: impl Fn(&(usize, bool), &mut Window, &mut App) + 'static,
    ) -> Self {
        self.selectable = true;
        self.on_select_row = Some(Rc::new(handler));
        self
    }

    /// Registers a callback invoked with the next boolean state when the header select-all toggles.
    pub fn on_select_all(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.selectable = true;
        self.on_select_all = Some(Rc::new(handler));
        self
    }

    /// Registers a callback invoked with `row_index` when an expandable row chevron is clicked.
    pub fn on_toggle_expand(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle_expand = Some(Rc::new(handler));
        self
    }
}

impl Styled for Table {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn render_table_header(
    columns: &[TableColumn],
    pad_x: Pixels,
    has_expand: bool,
    selectable: bool,
    has_rows: bool,
    header_check_state: CheckState,
    on_select_all: Option<Rc<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
    on_sort: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    theme: &super::theme::Theme,
) -> crate::Div {
    let mut header_row = div()
        .flex()
        .flex_row()
        .flex_shrink_0()
        .items_center()
        .w_full()
        .bg(theme.surface_muted.opacity(0.65))
        .border_b_1()
        .border_color(theme.border);

    if has_expand {
        header_row = header_row.child(div().w(px(36.)).flex_shrink_0());
    }

    if selectable {
        let is_on = header_check_state != CheckState::Unchecked;
        let next_all = header_check_state != CheckState::Checked;
        let interactive = has_rows && on_select_all.is_some();
        // The table clips its children, so these rings are drawn inside.
        let select_all = tab_stop_inset(div().id("select-all"), interactive, theme);
        header_row = header_row.child(
            select_all
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .w(px(40.))
                .py(px(10.))
                .when(interactive, |el| {
                    el.cursor_pointer()
                        .when_some(on_select_all, |el, cb| {
                            el.on_click(move |_, window, cx| cb(&next_all, window, cx))
                        })
                })
                .when(!interactive, |el| el.opacity(0.45).cursor_not_allowed())
                .child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(16.))
                        .h(px(16.))
                        .rounded(px(4.))
                        .border_1()
                        .border_color(if is_on {
                            theme.primary
                        } else {
                            theme.input_border
                        })
                        .bg(if is_on { theme.primary } else { theme.surface })
                        .when(header_check_state == CheckState::Checked, |el| {
                            el.child(
                                Icon::new(LucideIcon::Check)
                                    .size(IconSize::Xs)
                                    .color(theme.primary_foreground),
                            )
                        })
                        .when(header_check_state == CheckState::Indeterminate, |el| {
                            el.child(
                                Icon::new(LucideIcon::Minus)
                                    .size(IconSize::Xs)
                                    .color(theme.primary_foreground),
                            )
                        }),
                ),
        );
    }

    for (col_idx, col) in columns.iter().enumerate() {
        let on_sort = on_sort.clone();
        let can_sort = col.sortable && on_sort.is_some();
        let sort_icon = if col.sortable {
            Some(match col.sort {
                Some(SortDirection::Ascending) => LucideIcon::ArrowUp,
                Some(SortDirection::Descending) => LucideIcon::ArrowDown,
                None => LucideIcon::ArrowUpDown,
            })
        } else {
            None
        };

        // A sortable header is a button.
        let cell = tab_stop_inset(div().id(("col", col_idx)), can_sort, theme)
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(pad_x)
            .py(px(10.))
            .text_size(px(12.))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(theme.muted_foreground)
            .when_some(col.width, |el, w| el.w(w).flex_shrink_0().overflow_hidden())
            .when(col.width.is_none(), |el| el.flex_1().min_w_0())
            .when(col.align == TableAlign::Center, |el| el.justify_center())
            .when(col.align == TableAlign::Right, |el| el.justify_end())
            .when(can_sort, |el| {
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

    header_row
}

impl RenderOnce for Table {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let pad_y = if self.compact { px(8.) } else { px(12.) };
        let pad_x = px(14.);
        let has_expand = self.rows.iter().any(|r| r.expanded_content.is_some());
        let has_rows = !self.rows.is_empty();

        let checked_count = self.rows.iter().filter(|r| r.checked).count();
        let header_check_state = if !has_rows || checked_count == 0 {
            CheckState::Unchecked
        } else if checked_count == self.rows.len() {
            CheckState::Checked
        } else {
            CheckState::Indeterminate
        };

        let header_row = render_table_header(
            &self.columns,
            pad_x,
            has_expand,
            self.selectable,
            has_rows,
            header_check_state,
            self.on_select_all,
            self.on_sort,
            &theme,
        );

        let mut body_inner = div().flex().flex_col().w_full();
        if !has_rows {
            body_inner = body_inner.child(
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
            let hover_bg = theme.surface_hover;
            let highlight_hover = theme.primary_soft.opacity(0.75);
            for (row_idx, row) in self.rows.into_iter().enumerate() {
                let is_highlighted = row.selected || row.checked;
                let row_bg = if is_highlighted {
                    theme.primary_soft.opacity(0.55)
                } else if self.striped && row_idx % 2 == 1 {
                    theme.surface_muted.opacity(0.35)
                } else {
                    theme.surface
                };
                let row_hover_bg = if is_highlighted {
                    highlight_hover
                } else {
                    hover_bg.opacity(0.75)
                };
                let is_last = row_idx + 1 == row_count;
                let has_row_expand = row.expanded_content.is_some();
                let is_expanded = row.expanded && has_row_expand;

                let mut row_el = div()
                    .id(("row", row_idx))
                    .flex()
                    .flex_row()
                    .items_center()
                    .w_full()
                    .bg(row_bg)
                    .text_size(px(13.5))
                    .text_color(theme.foreground)
                    .when(!is_last || is_expanded, |el| {
                        el.border_b_1().border_color(theme.border)
                    })
                    .hover(move |s| s.bg(row_hover_bg))
                    .when_some(row.on_click, |el, handler| {
                        el.cursor_pointer()
                            .on_click(move |ev, window, cx| handler(ev, window, cx))
                    });

                if has_expand {
                    let on_expand = self.on_toggle_expand.clone();
                    let can_expand = has_row_expand && on_expand.is_some();
                    let expander = tab_stop_inset(div().id(("exp", row_idx)), can_expand, &theme);
                    row_el = row_el.child(
                        expander
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .w(px(36.))
                            .py(pad_y)
                            .when(can_expand, |el| {
                                el.cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .when_some(on_expand, |el, cb| {
                                        el.on_click(move |_, window, cx| {
                                            cb(&row_idx, window, cx);
                                            cx.stop_propagation();
                                        })
                                    })
                            })
                            .when(has_row_expand, |el| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .w(px(22.))
                                        .h(px(22.))
                                        .rounded(theme.radius_sm)
                                        .when(can_expand, |el| el.hover(move |s| s.bg(hover_bg)))
                                        .child(
                                            Icon::new(if is_expanded {
                                                LucideIcon::ChevronDown
                                            } else {
                                                LucideIcon::ChevronRight
                                            })
                                            .size(IconSize::Xs)
                                            .color(theme.muted_foreground),
                                        ),
                                )
                            }),
                    );
                }

                if self.selectable {
                    let next_checked = !row.checked;
                    let on_sel = self.on_select_row.clone();
                    let can_select = on_sel.is_some();
                    let check = tab_stop_inset(div().id(("chk", row_idx)), can_select, &theme);
                    row_el = row_el.child(
                        check
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .w(px(40.))
                            .py(pad_y)
                            .when(can_select, |el| {
                                el.cursor_pointer()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation()
                                    })
                                    .when_some(on_sel, |el, cb| {
                                        el.on_click(move |_, window, cx| {
                                            cb(&(row_idx, next_checked), window, cx);
                                            cx.stop_propagation();
                                        })
                                    })
                            })
                            .child(
                                div()
                                    .flex()
                                    .items_center()
                                    .justify_center()
                                    .w(px(16.))
                                    .h(px(16.))
                                    .rounded(px(4.))
                                    .border_1()
                                    .border_color(if row.checked {
                                        theme.primary
                                    } else {
                                        theme.input_border
                                    })
                                    .bg(if row.checked {
                                        theme.primary
                                    } else {
                                        theme.surface
                                    })
                                    .when(row.checked, |el| {
                                        el.child(
                                            Icon::new(LucideIcon::Check)
                                                .size(IconSize::Xs)
                                                .color(theme.primary_foreground),
                                        )
                                    }),
                            ),
                    );
                }

                let mut cells_iter = row.cells.into_iter();
                for col in &self.columns {
                    let cell_el = cells_iter.next();
                    row_el = row_el.child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .px(pad_x)
                            .py(pad_y)
                            .when_some(col.width, |el, w| el.w(w).flex_shrink_0().overflow_hidden())
                            .when(col.width.is_none(), |el| el.flex_1().min_w_0())
                            .when(col.align == TableAlign::Center, |el| el.justify_center())
                            .when(col.align == TableAlign::Right, |el| {
                                el.justify_end().font_features(tabular_numerals())
                            })
                            .children(cell_el),
                    );
                }

                body_inner = body_inner.child(row_el);

                if is_expanded
                    && let Some(detail) = row.expanded_content
                {
                    body_inner = body_inner.child(
                        div()
                            .w_full()
                            .px(px(20.))
                            .py(px(12.))
                            .bg(theme.surface_muted.opacity(0.4))
                            .when(!is_last, |el| el.border_b_1().border_color(theme.border))
                            .child(detail),
                    );
                }
            }
        }

        let body_slot: AnyElement = if let Some(max_h) = self.max_height {
            div()
                .id("table-scroll-body")
                .w_full()
                .max_h(max_h)
                .overflow_y_scroll()
                .child(body_inner)
                .into_any_element()
        } else {
            body_inner.into_any_element()
        };

        let mut root = div()
            .id(self.id)
            .flex()
            .flex_col()
            .w_full()
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .overflow_hidden()
            .when(theme.elevation_shadows, |el| el.shadow_2xs())
            .child(header_row)
            .child(body_slot);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// High-performance `uniform_list`-virtualized data table for rendering thousands of fixed-height
/// rows with a pinned sortable header at constant O(visible rows) per-frame cost.
#[derive(IntoElement)]
pub struct VirtualTable {
    id: ElementId,
    columns: Vec<TableColumn>,
    row_count: usize,
    row_height: Pixels,
    viewport_height: Pixels,
    striped: bool,
    empty_message: SharedString,
    scroll_handle: Option<UniformListScrollHandle>,
    on_sort: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    render_row: Box<dyn Fn(usize, &mut Window, &mut App) -> Vec<AnyElement> + 'static>,
    style: StyleRefinement,
}

/// Convenience constructor for [`VirtualTable::new`].
pub fn virtual_table(
    id: impl Into<ElementId>,
    columns: Vec<TableColumn>,
    row_count: usize,
    render_row: impl Fn(usize, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
) -> VirtualTable {
    VirtualTable::new(id, columns, row_count, render_row)
}

impl VirtualTable {
    /// Creates a new [`VirtualTable`] rendering `row_count` virtualized rows.
    pub fn new(
        id: impl Into<ElementId>,
        columns: Vec<TableColumn>,
        row_count: usize,
        render_row: impl Fn(usize, &mut Window, &mut App) -> Vec<AnyElement> + 'static,
    ) -> Self {
        Self {
            id: id.into(),
            columns,
            row_count,
            row_height: px(38.),
            viewport_height: px(320.),
            striped: true,
            empty_message: "No data available.".into(),
            scroll_handle: None,
            on_sort: None,
            render_row: Box::new(render_row),
            style: StyleRefinement::default(),
        }
    }

    /// Sets each uniform row height (default `38px`).
    pub fn row_height(mut self, height: Pixels) -> Self {
        self.row_height = if height.0.is_finite() {
            height.max(px(24.))
        } else {
            px(38.)
        };
        self
    }

    /// Sets the scrollable body viewport height (default `320px`).
    pub fn viewport_height(mut self, height: Pixels) -> Self {
        self.viewport_height = if height.0.is_finite() {
            height.max(px(0.))
        } else {
            px(320.)
        };
        self
    }

    /// Enables or disables alternating row stripes (default `true`).
    pub fn striped(mut self, striped: bool) -> Self {
        self.striped = striped;
        self
    }

    /// Customizes message shown when `row_count == 0`.
    pub fn empty_message(mut self, message: impl Into<SharedString>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Binds an external [`UniformListScrollHandle`] for programmatic scrolling.
    pub fn track_scroll(mut self, handle: UniformListScrollHandle) -> Self {
        self.scroll_handle = Some(handle);
        self
    }

    /// Registers a sortable column click callback.
    pub fn on_sort(mut self, handler: impl Fn(&usize, &mut Window, &mut App) + 'static) -> Self {
        self.on_sort = Some(Rc::new(handler));
        self
    }
}

impl Styled for VirtualTable {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for VirtualTable {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let pad_x = px(14.);
        let header_row = render_table_header(
            &self.columns,
            pad_x,
            false,
            false,
            self.row_count > 0,
            CheckState::Unchecked,
            None,
            self.on_sort,
            &theme,
        );

        let columns = self.columns;
        let row_height = self.row_height;
        let striped = self.striped;
        let render_row = self.render_row;
        let row_count = self.row_count;

        let body_el: AnyElement = if row_count == 0 {
            div()
                .flex()
                .items_center()
                .justify_center()
                .w_full()
                .py(px(28.))
                .text_size(px(13.5))
                .text_color(theme.muted_foreground)
                .child(self.empty_message)
                .into_any_element()
        } else {
            let mut list = uniform_list(
                "vtable-list",
                row_count,
                move |range: Range<usize>, window, cx| {
                    let theme = cx.theme();
                    let hover_bg = theme.surface_hover;
                    range
                        .map(|row_idx| {
                            let mut cells_iter = render_row(row_idx, window, cx).into_iter();
                            let row_bg = if striped && row_idx % 2 == 1 {
                                theme.surface_muted.opacity(0.35)
                            } else {
                                theme.surface
                            };
                            let is_last = row_idx + 1 == row_count;
                            let mut row_el = div()
                                .id(("vrow", row_idx))
                                .flex()
                                .flex_row()
                                .items_center()
                                .w_full()
                                .h(row_height)
                                .bg(row_bg)
                                .text_size(px(13.))
                                .text_color(theme.foreground)
                                .when(!is_last, |el| {
                                    el.border_b_1().border_color(theme.border.opacity(0.65))
                                })
                                .hover(move |s| s.bg(hover_bg.opacity(0.7)));
                            for col in &columns {
                                let cell_el = cells_iter.next();
                                row_el = row_el.child(
                                    div()
                                        .flex()
                                        .flex_row()
                                        .items_center()
                                        .px(pad_x)
                                        .when_some(col.width, |el, w| {
                                            el.w(w).flex_shrink_0().overflow_hidden()
                                        })
                                        .when(col.width.is_none(), |el| el.flex_1().min_w_0())
                                        .when(col.align == TableAlign::Center, |el| {
                                            el.justify_center()
                                        })
                                        .when(col.align == TableAlign::Right, |el| {
                                            el.justify_end().font_features(tabular_numerals())
                                        })
                                        .children(cell_el),
                                );
                            }
                            row_el
                        })
                        .collect()
                },
            )
            .w_full()
            .h(self.viewport_height);

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
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .overflow_hidden()
            .when(theme.elevation_shadows, |el| el.shadow_2xs())
            .child(header_row)
            .child(body_el);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

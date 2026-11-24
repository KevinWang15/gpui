//! Read-only [`SelectableText`] display component supporting mouse drag selection across
//! soft-wrapped and virtualized multi-line text, double-click word selection, triple-click
//! line selection, keyboard shortcuts (`Cmd/Ctrl+C`, `Cmd/Ctrl+A`, `Escape`), search/syntax
//! highlight ranges, and a built-in right-click / keyboard context menu.
//!
//! Long documents are split into bounded line chunks and rendered through GPUI's virtualized
//! [`crate::list`] when scrollable (the default), so megabyte-scale texts lay out only the
//! visible region while keeping byte-exact selection and copying across the entire document.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use crate::icons::LucideIcon;
use crate::{
    AnyElement, App, Bounds, ClipboardItem, Context, CursorStyle, DispatchPhase, Element,
    ElementId, Entity, EventEmitter, FocusHandle, Focusable, GlobalElementId, HighlightStyle,
    Hsla, InspectorElementId, IntoElement, KeyBinding, KeyDownEvent, LayoutId, ListAlignment,
    ListState, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    RenderOnce, ScrollbarVisibility, SharedString, StyleRefinement, Styled, StyledText, Task,
    TextAlign, TextLayout, Window, div, fill, list, point, prelude::*, px, size,
};

use super::context_menu::popup_menu;
use super::input::{clamp_char_boundary, is_word_char};
use super::popover::MenuEntry;
use super::theme::ActiveTheme;

/// Maximum byte length of a single rendered line chunk so extremely long unbroken lines stay
/// fast to shape and wrap.
const MAX_CHUNK_BYTES: usize = 2000;

/// Keyboard actions recognized by [`SelectableTextState`].
#[allow(missing_docs)]
pub mod selectable_text_actions {
    use crate::actions;

    actions!(gpui_selectable_text, [Copy, SelectAll, ClearSelection]);
}
use selectable_text_actions::*;

/// Registers cross-platform keybindings for [`SelectableTextState`] under `"GpuiSelectableText"`.
pub fn bind_selectable_text_keys(cx: &mut App) {
    let ctx = Some("GpuiSelectableText");
    cx.bind_keys([
        KeyBinding::new("cmd-c", Copy, ctx),
        KeyBinding::new("ctrl-c", Copy, ctx),
        KeyBinding::new("cmd-a", SelectAll, ctx),
        KeyBinding::new("ctrl-a", SelectAll, ctx),
        KeyBinding::new("escape", ClearSelection, ctx),
    ]);
}

/// Events emitted by [`SelectableTextState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum SelectableTextEvent {
    /// Emitted when the selected byte range changes.
    SelectionChanged(Range<usize>),
    /// Emitted when selected text is copied to the clipboard via shortcut, context menu,
    /// or [`SelectableTextState::copy_selection`].
    Copy(SharedString),
    /// Emitted when the text view gains focus.
    Focus,
    /// Emitted when the text view loses focus.
    Blur,
}

/// Optional host callback to customize, augment, or replace the default entries shown in a
/// [`SelectableText`] right-click / keyboard context menu.
///
/// Receives the currently selected text (`None` when the selection is empty), the pre-built
/// default [`MenuEntry`] items (`Copy`, `Select All`), and returns the entries to display.
pub type SelectableTextContextMenuBuilder = Rc<
    dyn Fn(Option<SharedString>, Vec<MenuEntry>, &mut Window, &mut App) -> Vec<MenuEntry> + 'static,
>;

#[derive(Clone, Debug, PartialEq, Eq)]
struct TextChunk {
    /// Byte range within the full document text excluding any trailing `\r`/`\n`.
    range: Range<usize>,
    /// Whether this chunk ends at an actual line break (`\n` or `\r\n`) rather than a synthetic
    /// split of an overlong line or the end of the text.
    ends_with_newline: bool,
}

#[derive(Clone)]
struct RenderedChunk {
    chunk: TextChunk,
    bounds: Bounds<Pixels>,
    layout: TextLayout,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum DragMode {
    #[default]
    Char,
    Word(Range<usize>),
    Line(Range<usize>),
}

/// Splits `text` into bounded line chunks on UTF-8 character boundaries while preserving exact
/// byte offsets into the original `text`.
fn split_chunks(text: &str) -> Vec<TextChunk> {
    if text.is_empty() {
        return vec![TextChunk {
            range: 0..0,
            ends_with_newline: false,
        }];
    }
    let mut out = Vec::new();
    let mut start = 0;
    for line in text.split_inclusive('\n') {
        let ends_with_newline = line.ends_with('\n');
        let content = line.trim_end_matches(['\n', '\r']);
        let mut from = 0;
        loop {
            let mut to = (from + MAX_CHUNK_BYTES).min(content.len());
            while !content.is_char_boundary(to) {
                to -= 1;
            }
            let is_last_piece = to >= content.len();
            out.push(TextChunk {
                range: start + from..start + to,
                ends_with_newline: ends_with_newline && is_last_piece,
            });
            from = to;
            if is_last_piece {
                break;
            }
        }
        start += line.len();
    }
    if text.ends_with('\n') {
        out.push(TextChunk {
            range: text.len()..text.len(),
            ends_with_newline: false,
        });
    }
    out
}

/// Clips and normalizes document-level highlight ranges to a chunk's local byte space,
/// guaranteeing sorted, non-overlapping, char-boundary-aligned ranges safe for [`StyledText`].
fn chunk_highlights(
    chunk_str: &str,
    chunk_range: &Range<usize>,
    highlights: &[(Range<usize>, HighlightStyle)],
) -> Vec<(Range<usize>, HighlightStyle)> {
    if highlights.is_empty() || chunk_range.is_empty() {
        return Vec::new();
    }
    let start_idx = highlights.partition_point(|(r, _)| r.end <= chunk_range.start);
    let mut out = Vec::new();
    let mut cursor = 0usize;
    for (range, style) in &highlights[start_idx..] {
        if range.start >= chunk_range.end {
            break;
        }
        if range.end <= chunk_range.start {
            continue;
        }
        let s = clamp_char_boundary(
            chunk_str,
            range.start.saturating_sub(chunk_range.start).max(cursor),
        );
        let e = clamp_char_boundary(
            chunk_str,
            range.end.min(chunk_range.end) - chunk_range.start,
        );
        if s < e {
            out.push((s..e, *style));
            cursor = e;
        }
    }
    out
}

fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let offset = clamp_char_boundary(text, offset);
    let probe = if offset == text.len() {
        text[..offset]
            .char_indices()
            .next_back()
            .map_or(0, |(i, _)| i)
    } else {
        offset
    };
    let Some(ch) = text[probe..].chars().next() else {
        return offset..offset;
    };
    if ch == '\n' || ch == '\r' {
        return probe..probe + ch.len_utf8();
    }
    let class = |c: char| -> u8 {
        if c == '\n' || c == '\r' {
            0
        } else if is_word_char(c) {
            1
        } else if c.is_whitespace() {
            2
        } else {
            3
        }
    };
    let target = class(ch);
    let start = text[..probe]
        .char_indices()
        .rev()
        .take_while(|&(_, c)| class(c) == target)
        .last()
        .map(|(idx, _)| idx)
        .unwrap_or(probe);
    let end = text[probe..]
        .char_indices()
        .take_while(|&(_, c)| class(c) == target)
        .last()
        .map(|(idx, c)| probe + idx + c.len_utf8())
        .unwrap_or(probe + ch.len_utf8());
    start..end
}

fn line_range_at(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let offset = clamp_char_boundary(text, offset);
    let start = text[..offset].rfind('\n').map_or(0, |i| i + 1);
    let end = match text[offset..].find('\n') {
        Some(rel) => offset + rel + 1,
        None => text.len(),
    };
    start..end
}

fn shortcut_hint(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

/// Entity state backing a read-only [`SelectableText`] view.
pub struct SelectableTextState {
    focus_handle: FocusHandle,
    text: SharedString,
    chunks: Arc<Vec<TextChunk>>,
    highlights: Arc<Vec<(Range<usize>, HighlightStyle)>>,
    selected_range: Range<usize>,
    selection_reversed: bool,
    is_selecting: bool,
    drag_mode: DragMode,
    scrollable: bool,
    tab_stop: bool,
    line_height: Pixels,
    list_state: ListState,
    scrollbar_visibility: ScrollbarVisibility,
    scrollbar_color: Option<Hsla>,
    selection_color: Option<Hsla>,
    context_menu_enabled: bool,
    context_menu_position: Option<Point<Pixels>>,
    context_menu_builder: Option<SelectableTextContextMenuBuilder>,
    rendered_chunks: Rc<RefCell<Vec<RenderedChunk>>>,
    viewport_bounds: Rc<Cell<Bounds<Pixels>>>,
    last_drag_position: Option<Point<Pixels>>,
    autoscroll_task: Option<Task<()>>,
    was_focused: bool,
    frame_style: StyleRefinement,
}

impl EventEmitter<SelectableTextEvent> for SelectableTextState {}

impl Focusable for SelectableTextState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl SelectableTextState {
    /// Creates a new empty [`SelectableTextState`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        let line_height = px(20.);
        let chunks = Arc::new(split_chunks(""));
        let list_state =
            ListState::new(chunks.len(), ListAlignment::Top, px(300.)).with_uniform_item_height(line_height);
        Self {
            focus_handle: cx.focus_handle(),
            text: SharedString::new_static(""),
            chunks,
            highlights: Arc::new(Vec::new()),
            selected_range: 0..0,
            selection_reversed: false,
            is_selecting: false,
            drag_mode: DragMode::Char,
            scrollable: true,
            tab_stop: false,
            line_height,
            list_state,
            scrollbar_visibility: ScrollbarVisibility::Auto,
            scrollbar_color: None,
            selection_color: None,
            context_menu_enabled: true,
            context_menu_position: None,
            context_menu_builder: None,
            rendered_chunks: Rc::new(RefCell::new(Vec::new())),
            viewport_bounds: Rc::new(Cell::new(Bounds::default())),
            last_drag_position: None,
            autoscroll_task: None,
            was_focused: false,
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets initial text content.
    pub fn default_value(mut self, text: impl Into<SharedString>) -> Self {
        self.text = text.into();
        self.chunks = Arc::new(split_chunks(self.text.as_ref()));
        self.list_state
            .reset_with_uniform_height(self.chunks.len(), self.line_height);
        self
    }

    /// Sets initial styled highlight ranges (byte offsets into the text).
    pub fn highlights(mut self, highlights: Vec<(Range<usize>, HighlightStyle)>) -> Self {
        self.highlights = Arc::new(self.normalize_highlights(highlights));
        self
    }

    /// Enables or disables virtualized vertical scrolling (default `true`).
    ///
    /// When `false`, text lays out inline at its natural wrapped height.
    pub fn scrollable(mut self, scrollable: bool) -> Self {
        self.scrollable = scrollable;
        self
    }

    /// Controls whether this text view participates in keyboard Tab navigation (default `false`,
    /// matching non-editable browser text).
    pub fn tab_stop(mut self, tab_stop: bool) -> Self {
        self.tab_stop = tab_stop;
        self
    }

    /// Sets the line height used for text rows and virtualized scroll estimation (default `20px`).
    pub fn line_height(mut self, line_height: Pixels) -> Self {
        self.line_height = line_height.max(px(1.));
        self.list_state
            .reset_with_uniform_height(self.chunks.len(), self.line_height);
        self
    }

    /// Overrides the selection highlight color (defaults to `theme.selection`).
    pub fn selection_color(mut self, color: impl Into<Hsla>) -> Self {
        self.selection_color = Some(color.into());
        self
    }

    /// Enables or disables the built-in right-click / keyboard context menu (default `true`).
    pub fn context_menu(mut self, enabled: bool) -> Self {
        self.context_menu_enabled = enabled;
        self
    }

    /// Sets the overlay scrollbar visibility when scrollable (default [`ScrollbarVisibility::Auto`]).
    pub fn scrollbar_visibility(mut self, visibility: ScrollbarVisibility) -> Self {
        self.scrollbar_visibility = visibility;
        self
    }

    /// Overrides the overlay scrollbar thumb base color when scrollable.
    pub fn scrollbar_color(mut self, color: impl Into<Hsla>) -> Self {
        self.scrollbar_color = Some(color.into());
        self
    }

    /// Returns the full text content.
    pub fn text(&self) -> &SharedString {
        &self.text
    }

    /// Replaces the text content, resetting selection, highlights, scroll position, and any open
    /// context menu.
    pub fn set_text(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.text = text.into();
        self.chunks = Arc::new(split_chunks(self.text.as_ref()));
        self.highlights = Arc::new(Vec::new());
        let had_selection = !self.selected_range.is_empty();
        self.selected_range = 0..0;
        self.selection_reversed = false;
        self.is_selecting = false;
        self.drag_mode = DragMode::Char;
        self.last_drag_position = None;
        self.autoscroll_task = None;
        self.context_menu_position = None;
        self.list_state
            .reset_with_uniform_height(self.chunks.len(), self.line_height);
        if had_selection {
            cx.emit(SelectableTextEvent::SelectionChanged(0..0));
        }
        cx.notify();
    }

    /// Updates highlight ranges (byte offsets into [`Self::text`]) without resetting scroll or
    /// selection.
    pub fn set_highlights(
        &mut self,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
        cx: &mut Context<Self>,
    ) {
        self.highlights = Arc::new(self.normalize_highlights(highlights));
        cx.notify();
    }

    /// Returns the currently selected byte range within [`Self::text`].
    pub fn selected_range(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    /// Returns whether a non-empty text range is currently selected.
    pub fn has_selection(&self) -> bool {
        !self.selected_range.is_empty()
    }

    /// Returns the currently selected substring, or `None` when the selection is empty.
    pub fn selected_text(&self) -> Option<&str> {
        if self.selected_range.is_empty() {
            return None;
        }
        let s = clamp_char_boundary(self.text.as_ref(), self.selected_range.start);
        let e = clamp_char_boundary(self.text.as_ref(), self.selected_range.end);
        (s < e).then(|| &self.text.as_ref()[s..e])
    }

    /// Programmatically sets the selected byte range (clamped to valid UTF-8 boundaries).
    pub fn set_selection(&mut self, range: Range<usize>, cx: &mut Context<Self>) {
        let a = clamp_char_boundary(self.text.as_ref(), range.start);
        let b = clamp_char_boundary(self.text.as_ref(), range.end);
        let next = a.min(b)..a.max(b);
        self.selection_reversed = range.end < range.start;
        if next != self.selected_range {
            self.selected_range = next.clone();
            cx.emit(SelectableTextEvent::SelectionChanged(next));
            cx.notify();
        }
    }

    /// Selects the entire text.
    pub fn select_all(&mut self, cx: &mut Context<Self>) {
        self.set_selection(0..self.text.len(), cx);
    }

    /// Clears the current selection.
    pub fn clear_selection(&mut self, cx: &mut Context<Self>) {
        self.is_selecting = false;
        self.autoscroll_task = None;
        self.last_drag_position = None;
        if !self.selected_range.is_empty() {
            self.selected_range = 0..0;
            self.selection_reversed = false;
            cx.emit(SelectableTextEvent::SelectionChanged(0..0));
            cx.notify();
        }
    }

    /// Copies the currently selected text to the system clipboard and emits
    /// [`SelectableTextEvent::Copy`]. Returns `false` if the selection is empty.
    pub fn copy_selection(&mut self, cx: &mut Context<Self>) -> bool {
        let Some(selected) = self.selected_text().map(str::to_owned) else {
            return false;
        };
        let shared: SharedString = selected.into();
        cx.write_to_clipboard(ClipboardItem::new_string(shared.to_string()));
        cx.emit(SelectableTextEvent::Copy(shared));
        true
    }

    /// Returns whether the floating context menu is currently open.
    pub fn is_context_menu_open(&self) -> bool {
        self.context_menu_position.is_some()
    }

    /// Closes the floating context menu if it is open.
    pub fn close_context_menu(&mut self, cx: &mut Context<Self>) {
        if self.context_menu_position.take().is_some() {
            cx.notify();
        }
    }

    /// Scrolls the virtualized text view by `delta` pixels (positive scrolls down).
    pub fn scroll_by(&mut self, delta: Pixels, cx: &mut Context<Self>) {
        if self.scrollable {
            self.list_state.scroll_by(delta);
            cx.notify();
        }
    }

    /// Scrolls the virtualized text view by roughly one viewport page (90% of viewport height).
    pub fn scroll_page(&mut self, down: bool, cx: &mut Context<Self>) {
        if !self.scrollable {
            return;
        }
        let h = self.viewport_bounds().size.height;
        let page = if h > px(0.) {
            h * 0.9
        } else {
            self.line_height * 10.
        };
        self.list_state.scroll_by(if down { page } else { -page });
        cx.notify();
    }

    /// Scrolls to the top of the document.
    pub fn scroll_to_top(&mut self, cx: &mut Context<Self>) {
        if self.scrollable {
            self.list_state.scroll_to(crate::ListOffset {
                item_ix: 0,
                offset_in_item: px(0.),
            });
            cx.notify();
        }
    }

    /// Scrolls to the bottom of the document.
    pub fn scroll_to_bottom(&mut self, cx: &mut Context<Self>) {
        if self.scrollable {
            self.list_state.scroll_to_end();
            cx.notify();
        }
    }

    /// Returns the last painted viewport bounds in window coordinates.
    pub fn viewport_bounds(&self) -> Bounds<Pixels> {
        let b = self.viewport_bounds.get();
        if b.size.height > px(0.) {
            b
        } else {
            self.list_state.viewport_bounds()
        }
    }

    /// Returns the underlying virtualized [`ListState`].
    pub fn list_state(&self) -> &ListState {
        &self.list_state
    }

    fn normalize_highlights(
        &self,
        highlights: Vec<(Range<usize>, HighlightStyle)>,
    ) -> Vec<(Range<usize>, HighlightStyle)> {
        let text = self.text.as_ref();
        let mut normalized: Vec<(Range<usize>, HighlightStyle)> = highlights
            .into_iter()
            .filter_map(|(r, style)| {
                let s = clamp_char_boundary(text, r.start);
                let e = clamp_char_boundary(text, r.end);
                (s < e).then_some((s..e, style))
            })
            .collect();
        normalized.sort_by_key(|(r, _)| (r.start, r.end));
        normalized
    }

    fn anchor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.end
        } else {
            self.selected_range.start
        }
    }

    fn update_selection_to(&mut self, anchor: usize, head: usize, cx: &mut Context<Self>) {
        let text = self.text.as_ref();
        let anchor = clamp_char_boundary(text, anchor);
        let head = clamp_char_boundary(text, head);
        let (next, reversed) = if head < anchor {
            (head..anchor, true)
        } else {
            (anchor..head, false)
        };
        self.selection_reversed = reversed;
        if next != self.selected_range {
            self.selected_range = next.clone();
            cx.emit(SelectableTextEvent::SelectionChanged(next));
            cx.notify();
        }
    }

    fn update_selection_from_drag(&mut self, position: Point<Pixels>, cx: &mut Context<Self>) {
        let Some(idx) = self.byte_offset_for_position(position, true) else {
            return;
        };
        match self.drag_mode.clone() {
            DragMode::Char => {
                let anchor = self.anchor_offset();
                self.update_selection_to(anchor, idx, cx);
            }
            DragMode::Word(base) => {
                let current = word_range_at(self.text.as_ref(), idx);
                let (next, reversed) = if current.start < base.start {
                    (current.start..base.end, true)
                } else {
                    (base.start..current.end, false)
                };
                self.selection_reversed = reversed;
                if next != self.selected_range {
                    self.selected_range = next.clone();
                    cx.emit(SelectableTextEvent::SelectionChanged(next));
                    cx.notify();
                }
            }
            DragMode::Line(base) => {
                let current = line_range_at(self.text.as_ref(), idx);
                let (next, reversed) = if current.start < base.start {
                    (current.start..base.end, true)
                } else {
                    (base.start..current.end, false)
                };
                self.selection_reversed = reversed;
                if next != self.selected_range {
                    self.selected_range = next.clone();
                    cx.emit(SelectableTextEvent::SelectionChanged(next));
                    cx.notify();
                }
            }
        }
    }

    /// Maps a window-coordinate point to a UTF-8 byte offset within [`Self::text`].
    fn byte_offset_for_position(&self, position: Point<Pixels>, clamp_to_viewport: bool) -> Option<usize> {
        let rendered = self.rendered_chunks.borrow();
        if rendered.is_empty() {
            return None;
        }
        let viewport = self.viewport_bounds();
        let has_viewport = self.scrollable && viewport.size.height > px(0.);

        // When dragging outside a scrollable viewport, clamp to the visible edge rather than
        // jumping into hidden overdraw rows.
        if clamp_to_viewport && has_viewport {
            if position.y < viewport.top() {
                let first_visible = rendered
                    .iter()
                    .find(|c| c.bounds.bottom() > viewport.top())
                    .unwrap_or(&rendered[0]);
                return Some(first_visible.chunk.range.start);
            }
            if position.y > viewport.bottom() {
                let last_visible = rendered
                    .iter()
                    .rev()
                    .find(|c| c.bounds.top() < viewport.bottom())
                    .unwrap_or(rendered.last().unwrap());
                return Some(last_visible.chunk.range.end);
            }
        }

        if position.y < rendered[0].bounds.top() {
            return Some(rendered[0].chunk.range.start);
        }
        if let Some(last) = rendered.last()
            && position.y >= last.bounds.bottom()
        {
            return Some(last.chunk.range.end);
        }

        let target = rendered
            .iter()
            .find(|c| position.y >= c.bounds.top() && position.y < c.bounds.bottom())
            .or_else(|| {
                rendered.iter().min_by(|a, b| {
                    let da = if position.y < a.bounds.top() {
                        a.bounds.top() - position.y
                    } else {
                        position.y - a.bounds.bottom()
                    };
                    let db = if position.y < b.bounds.top() {
                        b.bounds.top() - position.y
                    } else {
                        position.y - b.bounds.bottom()
                    };
                    da.partial_cmp(&db).unwrap_or(std::cmp::Ordering::Equal)
                })
            })?;

        if target.chunk.range.is_empty() {
            return Some(target.chunk.range.start);
        }

        let local = match target.layout.closest_index_for_position(position) {
            Ok(i) | Err(i) => i,
        };
        let raw = (target.chunk.range.start + local).min(target.chunk.range.end);
        Some(clamp_char_boundary(self.text.as_ref(), raw))
    }

    fn on_left_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        window.focus(&self.focus_handle, cx);
        window.prevent_default();
        self.context_menu_position = None;
        self.is_selecting = true;
        self.last_drag_position = Some(event.position);
        self.autoscroll_task = None;

        let Some(idx) = self.byte_offset_for_position(event.position, false) else {
            cx.notify();
            return;
        };

        if event.click_count >= 3 {
            let line = line_range_at(self.text.as_ref(), idx);
            self.drag_mode = DragMode::Line(line.clone());
            self.selection_reversed = false;
            if line != self.selected_range {
                self.selected_range = line.clone();
                cx.emit(SelectableTextEvent::SelectionChanged(line));
            }
            cx.notify();
        } else if event.click_count == 2 {
            let word = word_range_at(self.text.as_ref(), idx);
            self.drag_mode = DragMode::Word(word.clone());
            self.selection_reversed = false;
            if word != self.selected_range {
                self.selected_range = word.clone();
                cx.emit(SelectableTextEvent::SelectionChanged(word));
            }
            cx.notify();
        } else if event.modifiers.shift {
            self.drag_mode = DragMode::Char;
            let anchor = self.anchor_offset();
            self.update_selection_to(anchor, idx, cx);
        } else {
            self.drag_mode = DragMode::Char;
            self.selection_reversed = false;
            let next = idx..idx;
            let changed = next != self.selected_range;
            self.selected_range = next.clone();
            if changed {
                cx.emit(SelectableTextEvent::SelectionChanged(next));
            }
            cx.notify();
        }
    }

    fn on_right_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.context_menu_enabled {
            return;
        }
        // Preserve any active selection when opening the context menu.
        self.is_selecting = false;
        self.autoscroll_task = None;
        self.last_drag_position = None;
        window.focus(&self.focus_handle, cx);
        self.context_menu_position = Some(event.position);
        window.prevent_default();
        cx.stop_propagation();
        cx.notify();
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.is_selecting {
            return;
        }
        if event.pressed_button != Some(MouseButton::Left) {
            self.on_mouse_up(cx);
            return;
        }
        self.last_drag_position = Some(event.position);
        self.update_selection_from_drag(event.position, cx);
        self.sync_drag_autoscroll(event.position, window, cx);
    }

    fn on_mouse_up(&mut self, cx: &mut Context<Self>) {
        if self.is_selecting || self.autoscroll_task.is_some() {
            self.is_selecting = false;
            self.autoscroll_task = None;
            self.last_drag_position = None;
            cx.notify();
        }
    }

    fn sync_drag_autoscroll(
        &mut self,
        position: Point<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.scrollable {
            self.autoscroll_task = None;
            return;
        }
        let viewport = self.viewport_bounds();
        let outside = viewport.size.height > px(0.)
            && (position.y < viewport.top() || position.y > viewport.bottom());
        if !outside {
            self.autoscroll_task = None;
            return;
        }
        if self.autoscroll_task.is_some() {
            return;
        }
        let task = cx.spawn_in(window, async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(16))
                    .await;
                let keep_going = this
                    .update(cx, |state, cx| {
                        if !state.is_selecting {
                            state.autoscroll_task = None;
                            return false;
                        }
                        let Some(pos) = state.last_drag_position else {
                            state.autoscroll_task = None;
                            return false;
                        };
                        let vp = state.viewport_bounds();
                        if vp.size.height <= px(0.) {
                            state.autoscroll_task = None;
                            return false;
                        }
                        let delta = if pos.y < vp.top() {
                            -((vp.top() - pos.y) * 0.35).clamp(px(4.), px(36.))
                        } else if pos.y > vp.bottom() {
                            ((pos.y - vp.bottom()) * 0.35).clamp(px(4.), px(36.))
                        } else {
                            state.autoscroll_task = None;
                            return false;
                        };
                        state.list_state.scroll_by(delta);
                        state.update_selection_from_drag(pos, cx);
                        cx.notify();
                        true
                    })
                    .unwrap_or(false);
                if !keep_going {
                    break;
                }
            }
        });
        self.autoscroll_task = Some(task);
    }

    fn on_copy_action(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.copy_selection(cx) {
            // Propagate empty-selection Copy so host views (e.g. list/launcher hosts) can handle it.
            cx.propagate();
        }
    }

    fn on_select_all_action(
        &mut self,
        _: &SelectAll,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.select_all(cx);
    }

    fn on_clear_selection_action(
        &mut self,
        _: &ClearSelection,
        _window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.context_menu_position.take().is_some() {
            cx.notify();
        } else if !self.selected_range.is_empty() {
            self.clear_selection(cx);
        } else {
            cx.propagate();
        }
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if !self.context_menu_enabled {
            return;
        }
        let mods = &event.keystroke.modifiers;
        let key = event.keystroke.key.as_str();
        if (mods.shift && key == "f10" && !mods.control && !mods.alt && !mods.platform)
            || key == "contextmenu"
        {
            let anchor = self.keyboard_context_menu_anchor();
            self.context_menu_position = Some(anchor);
            window.prevent_default();
            cx.stop_propagation();
            cx.notify();
        }
    }

    fn keyboard_context_menu_anchor(&self) -> Point<Pixels> {
        let viewport = self.viewport_bounds();
        if !self.selected_range.is_empty() {
            let rendered = self.rendered_chunks.borrow();
            let sel_start = self.selected_range.start;
            for rc in rendered.iter() {
                if sel_start >= rc.chunk.range.start && sel_start <= rc.chunk.range.end {
                    let local = sel_start - rc.chunk.range.start;
                    if let Some(pt) = rc.layout.position_for_index(local) {
                        return point(pt.x, pt.y + rc.layout.line_height());
                    }
                    return point(rc.bounds.left(), rc.bounds.bottom());
                }
            }
        }
        point(viewport.left() + px(12.), viewport.top() + px(12.))
    }

    fn build_context_menu_entries(&self, window: &mut Window, cx: &mut Context<Self>) -> Vec<MenuEntry> {
        let entity = cx.entity();
        let has_selection = self.has_selection();
        let has_text = !self.text.is_empty();

        let copy_entity = entity.clone();
        let select_all_entity = entity;

        let defaults = vec![
            MenuEntry::item_with_icon("Copy", LucideIcon::Copy, move |_, _, cx| {
                copy_entity.update(cx, |s, cx| {
                    s.copy_selection(cx);
                });
            })
            .with_shortcut(shortcut_hint("⌘C", "Ctrl+C"))
            .with_disabled(!has_selection),
            MenuEntry::item_with_icon(
                "Select All",
                LucideIcon::SquareDashedMousePointer,
                move |_, _, cx| {
                    select_all_entity.update(cx, |s, cx| {
                        s.select_all(cx);
                    });
                },
            )
            .with_shortcut(shortcut_hint("⌘A", "Ctrl+A"))
            .with_disabled(!has_text),
        ];

        if let Some(builder) = self.context_menu_builder.clone() {
            let selected = self.selected_text().map(|s| SharedString::from(s.to_owned()));
            builder(selected, defaults, window, cx)
        } else {
            defaults
        }
    }
}

impl Render for SelectableTextState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.focus_handle.is_focused(window);
        if focused != self.was_focused {
            self.was_focused = focused;
            cx.emit(if focused {
                SelectableTextEvent::Focus
            } else {
                SelectableTextEvent::Blur
            });
        }

        let selection_color = self.selection_color.unwrap_or(theme.selection);
        let entity = cx.entity();
        let entity_id = cx.entity_id();
        let is_selecting = self.is_selecting;
        let text = self.text.clone();
        let chunks = self.chunks.clone();
        let highlights = self.highlights.clone();
        let selected_range = self.selected_range.clone();
        let line_height = self.line_height;
        let rendered_chunks = self.rendered_chunks.clone();
        let viewport_bounds = self.viewport_bounds.clone();

        let content: AnyElement = if self.scrollable {
            let scrollbar_vis = self.scrollbar_visibility;
            let scrollbar_col = self.scrollbar_color;
            let reserve_right = if scrollbar_vis == ScrollbarVisibility::Always {
                px(12.)
            } else {
                px(0.)
            };
            let list_el = list(self.list_state.clone(), move |i, _, _| {
                let chunk = chunks[i].clone();
                let slice = &text.as_ref()[chunk.range.clone()];
                let local_highlights = chunk_highlights(slice, &chunk.range, &highlights);
                let mut styled = StyledText::new(SharedString::from(slice.to_owned()));
                if !local_highlights.is_empty() {
                    styled = styled.with_highlights(local_highlights);
                }
                div()
                    .w_full()
                    .min_h(line_height)
                    .child(SelectableChunkElement {
                        chunk,
                        styled_text: styled,
                        selection: selected_range.clone(),
                        selection_color,
                        rendered_chunks: rendered_chunks.clone(),
                        viewport_bounds: viewport_bounds.clone(),
                    })
                    .into_any_element()
            })
            .size_full()
            .pr(reserve_right)
            .scrollbar_visibility(scrollbar_vis)
            .when_some(scrollbar_col, |el, color| el.scrollbar_color(color));

            div()
                .relative()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(
                    div()
                        .absolute()
                        .inset_0()
                        .child(SelectionViewportElement {
                            child: list_el.into_any_element(),
                            rendered_chunks: self.rendered_chunks.clone(),
                            viewport_bounds: self.viewport_bounds.clone(),
                            entity,
                            is_selecting,
                        }),
                )
                .into_any_element()
        } else {
            let mut col = div().w_full().flex().flex_col();
            for chunk in chunks.iter() {
                let slice = &text.as_ref()[chunk.range.clone()];
                let local_highlights = chunk_highlights(slice, &chunk.range, &highlights);
                let mut styled = StyledText::new(SharedString::from(slice.to_owned()));
                if !local_highlights.is_empty() {
                    styled = styled.with_highlights(local_highlights);
                }
                col = col.child(
                    div()
                        .w_full()
                        .min_h(line_height)
                        .child(SelectableChunkElement {
                            chunk: chunk.clone(),
                            styled_text: styled,
                            selection: selected_range.clone(),
                            selection_color,
                            rendered_chunks: rendered_chunks.clone(),
                            viewport_bounds: viewport_bounds.clone(),
                        }),
                );
            }
            SelectionViewportElement {
                child: col.into_any_element(),
                rendered_chunks: self.rendered_chunks.clone(),
                viewport_bounds: self.viewport_bounds.clone(),
                entity,
                is_selecting,
            }
            .into_any_element()
        };

        let menu_entries = if self.context_menu_enabled && self.context_menu_position.is_some() {
            self.build_context_menu_entries(window, cx)
        } else {
            Vec::new()
        };

        let menu = popup_menu(
            ("selectable-text-ctx", entity_id),
            self.context_menu_position,
        )
        .min_width(px(180.))
        .disabled(!self.context_menu_enabled)
        .entries(menu_entries)
        .on_close(cx.listener(|this, _, _, cx| {
            this.close_context_menu(cx);
        }));

        let mut root = div()
            .id(("selectable-text", entity_id))
            .relative()
            .flex()
            .flex_col()
            .w_full()
            .when(self.scrollable, |el| el.size_full().overflow_hidden())
            .key_context("GpuiSelectableText")
            .track_focus(&self.focus_handle.clone().tab_stop(self.tab_stop))
            .text_size(px(14.))
            .line_height(self.line_height)
            .text_color(theme.foreground)
            .cursor(CursorStyle::IBeam)
            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_left_mouse_down))
            .when(self.context_menu_enabled, |el| {
                el.on_mouse_down(MouseButton::Right, cx.listener(Self::on_right_mouse_down))
            })
            .on_key_down(cx.listener(Self::on_key_down))
            .on_action(cx.listener(Self::on_copy_action))
            .on_action(cx.listener(Self::on_select_all_action))
            .on_action(cx.listener(Self::on_clear_selection_action))
            .child(content)
            .child(menu);

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Wrapper element that captures exact viewport bounds, clears the per-frame rendered-chunk
/// table before its child paints, and registers window-wide drag tracking while actively
/// selecting text (so dragging outside the viewport continues updating and auto-scrolling).
struct SelectionViewportElement {
    child: AnyElement,
    rendered_chunks: Rc<RefCell<Vec<RenderedChunk>>>,
    viewport_bounds: Rc<Cell<Bounds<Pixels>>>,
    entity: Entity<SelectableTextState>,
    is_selecting: bool,
}

impl Element for SelectionViewportElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        (self.child.request_layout(window, cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.viewport_bounds.set(bounds);
        self.child.prepaint(window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.viewport_bounds.set(bounds);
        self.rendered_chunks.borrow_mut().clear();

        if self.is_selecting {
            let entity_move = self.entity.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    entity_move.update(cx, |state, cx| {
                        state.on_mouse_move(event, window, cx);
                    });
                }
            });
            let entity_up = self.entity.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, _window, cx| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    entity_up.update(cx, |state, cx| {
                        state.on_mouse_up(cx);
                    });
                }
            });
        }

        self.child.paint(window, cx);
    }
}

/// Renders one text chunk with background highlights, wrapped multi-line selection quads,
/// foreground glyphs, and records its finalized layout for pointer hit-testing.
struct SelectableChunkElement {
    chunk: TextChunk,
    styled_text: StyledText,
    selection: Range<usize>,
    selection_color: Hsla,
    rendered_chunks: Rc<RefCell<Vec<RenderedChunk>>>,
    viewport_bounds: Rc<Cell<Bounds<Pixels>>>,
}

impl Element for SelectableChunkElement {
    type RequestLayoutState = ();
    type PrepaintState = ();

    fn id(&self) -> Option<ElementId> {
        None
    }

    fn source_location(&self) -> Option<&'static core::panic::Location<'static>> {
        None
    }

    fn request_layout(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        window: &mut Window,
        cx: &mut App,
    ) -> (LayoutId, ()) {
        let (layout_id, _) = self.styled_text.request_layout(None, inspector_id, window, cx);
        (layout_id, ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        self.styled_text
            .prepaint(None, inspector_id, bounds, &mut (), window, cx);
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        _prepaint: &mut (),
        window: &mut Window,
        cx: &mut App,
    ) {
        let layout = self.styled_text.layout().clone();
        let line_height = layout.line_height();
        let hit_bounds = if bounds.size.height <= px(0.) {
            Bounds::new(bounds.origin, size(bounds.size.width, line_height))
        } else {
            bounds
        };
        self.rendered_chunks.borrow_mut().push(RenderedChunk {
            chunk: self.chunk.clone(),
            bounds: hit_bounds,
            layout: layout.clone(),
        });

        let _ = layout.paint_background(window, cx);
        let max_right = self.viewport_bounds.get().right().max(bounds.right());
        paint_chunk_selection(
            &self.chunk,
            &self.selection,
            self.selection_color,
            &layout,
            max_right,
            window,
        );
        let _ = layout.paint_foreground(window, cx);
    }
}

fn aligned_subline_origin_x(
    bounds: Bounds<Pixels>,
    subline_width: Pixels,
    align: TextAlign,
) -> Pixels {
    match align {
        TextAlign::Left => bounds.left(),
        TextAlign::Center => bounds.left() + ((bounds.size.width - subline_width) / 2.).max(px(0.)),
        TextAlign::Right => bounds.left() + (bounds.size.width - subline_width).max(px(0.)),
    }
}

fn paint_chunk_selection(
    chunk: &TextChunk,
    selection: &Range<usize>,
    color: Hsla,
    layout: &TextLayout,
    max_right: Pixels,
    window: &mut Window,
) {
    if selection.is_empty() {
        return;
    }
    let c_start = chunk.range.start;
    let c_end = chunk.range.end;
    let local_start = selection.start.clamp(c_start, c_end) - c_start;
    let local_end = selection.end.clamp(c_start, c_end) - c_start;
    let includes_newline =
        chunk.ends_with_newline && selection.start <= c_end && selection.end > c_end;

    if local_start == local_end && !includes_newline {
        return;
    }

    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let newline_stub = px(6.);

    if chunk.range.is_empty() {
        if includes_newline {
            let x2 = (bounds.left() + newline_stub).min(max_right.max(bounds.left() + newline_stub));
            window.paint_quad(fill(
                Bounds::from_corners(bounds.origin, point(x2, bounds.top() + line_height)),
                color,
            ));
        }
        return;
    }

    let line_layouts = layout.line_layouts();
    let Some(wrapped) = line_layouts.first() else {
        return;
    };
    let unwrapped = &wrapped.unwrapped_layout;
    let align = window.text_style().text_align;
    let wrap_count = wrapped.wrap_boundaries.len();

    for sub_idx in 0..=wrap_count {
        let (sub_start_ix, sub_start_x) = if sub_idx == 0 {
            (0, px(0.))
        } else {
            let b = wrapped.wrap_boundaries[sub_idx - 1];
            let g = &unwrapped.runs[b.run_ix].glyphs[b.glyph_ix];
            (g.index, g.position.x)
        };
        let (sub_end_ix, sub_end_x) = if sub_idx < wrap_count {
            let b = wrapped.wrap_boundaries[sub_idx];
            let g = &unwrapped.runs[b.run_ix].glyphs[b.glyph_ix];
            (g.index, g.position.x)
        } else {
            (unwrapped.len, unwrapped.width)
        };

        let is_last_sub = sub_idx == wrap_count;
        let s = local_start.max(sub_start_ix);
        let e = local_end.min(sub_end_ix);
        let has_span = s < e;
        let has_nl = is_last_sub && includes_newline && local_start <= sub_end_ix;
        if !has_span && !has_nl {
            continue;
        }

        let sub_width = (sub_end_x - sub_start_x).max(px(0.));
        let sub_origin_x = aligned_subline_origin_x(bounds, sub_width, align);
        let y = bounds.top() + line_height * sub_idx as f32;

        let (x1, mut x2) = if has_span {
            let x1 = sub_origin_x + (unwrapped.x_for_index(s) - sub_start_x);
            let x2 = if e == sub_end_ix {
                sub_origin_x + sub_width
            } else {
                sub_origin_x + (unwrapped.x_for_index(e) - sub_start_x)
            };
            (x1, x2)
        } else {
            let end_x = sub_origin_x + sub_width;
            (end_x, end_x)
        };

        if has_nl {
            x2 += newline_stub;
        }

        let x1 = x1.max(bounds.left());
        let x2 = x2.min(max_right);
        if x2 > x1 {
            window.paint_quad(fill(
                Bounds::from_corners(point(x1, y), point(x2, y + line_height)),
                color,
            ));
        }
    }
}

/// Renderable read-only selectable text element wrapping an [`Entity<SelectableTextState>`].
#[derive(IntoElement)]
pub struct SelectableText {
    state: Entity<SelectableTextState>,
    context_menu_builder: Option<SelectableTextContextMenuBuilder>,
    style: StyleRefinement,
}

/// Convenience constructor for [`SelectableText::new`].
pub fn selectable_text(state: &Entity<SelectableTextState>) -> SelectableText {
    SelectableText::new(state)
}

impl SelectableText {
    /// Creates a [`SelectableText`] element backed by `state`.
    pub fn new(state: &Entity<SelectableTextState>) -> Self {
        Self {
            state: state.clone(),
            context_menu_builder: None,
            style: StyleRefinement::default(),
        }
    }

    /// Customizes or extends the default right-click / keyboard context menu entries.
    pub fn context_menu_builder(
        mut self,
        builder: impl Fn(Option<SharedString>, Vec<MenuEntry>, &mut Window, &mut App) -> Vec<MenuEntry>
        + 'static,
    ) -> Self {
        self.context_menu_builder = Some(Rc::new(builder));
        self
    }
}

impl Styled for SelectableText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SelectableText {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let style_changed = self.state.read(cx).frame_style != self.style;
        let has_builder = self.context_menu_builder.is_some();
        let had_builder = self.state.read(cx).context_menu_builder.is_some();
        if style_changed || has_builder || had_builder {
            let style = self.style;
            let builder = self.context_menu_builder;
            self.state.update(cx, |state, _| {
                state.frame_style = style;
                state.context_menu_builder = builder;
            });
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        Modifiers, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, TestAppContext,
        VisualTestContext, actions, rgb,
    };

    actions!(selectable_text_test, [HostCopy, HostEscape]);

    #[test]
    fn split_chunks_handles_newlines_and_overlong_utf8_lines() {
        let t = "one\r\ntwo\n\nthree";
        let chunks = split_chunks(t);
        let slices: Vec<(&str, bool)> = chunks
            .iter()
            .map(|c| (&t[c.range.clone()], c.ends_with_newline))
            .collect();
        assert_eq!(
            slices,
            vec![
                ("one", true),
                ("two", true),
                ("", true),
                ("three", false),
            ]
        );

        // Trailing newline produces a final empty line chunk.
        let trailing = split_chunks("abc\n");
        assert_eq!(trailing.len(), 2);
        assert!(trailing[0].ends_with_newline);
        assert_eq!(trailing[1].range, 4..4);
        assert!(!trailing[1].ends_with_newline);

        // Long multi-byte line splits on char boundaries; only the final piece has ends_with_newline.
        let long = format!("{}\n", "é".repeat(1500)); // 3000 bytes + '\n'
        let chunks = split_chunks(&long);
        assert_eq!(chunks.len(), 3); // 2 pieces + trailing empty line
        assert!(!chunks[0].ends_with_newline);
        assert!(chunks[1].ends_with_newline);
        assert_eq!(chunks[0].range.end, chunks[1].range.start);
        assert_eq!(chunks[1].range.end, 3000);
        assert!(
            chunks
                .iter()
                .all(|c| long.is_char_boundary(c.range.start) && long.is_char_boundary(c.range.end))
        );
    }

    #[test]
    fn chunk_highlights_clips_and_dedups_overlapping_ranges() {
        let style = HighlightStyle {
            color: Some(rgb(0xFFFFFF).into()),
            ..Default::default()
        };
        let chunk = "hello world";
        let global = vec![(0..8, style), (3..11, style), (6..20, style)];
        let local = chunk_highlights(chunk, &(0..chunk.len()), &global);
        assert_eq!(local.len(), 2);
        assert_eq!(local[0].0, 0..8);
        assert_eq!(local[1].0, 8..11);
    }

    #[test]
    fn word_and_line_ranges() {
        let text = "hello world\nsecond_line here\nthird";
        assert_eq!(&text[word_range_at(text, 1)], "hello");
        assert_eq!(&text[word_range_at(text, 6)], "world");
        assert_eq!(&text[word_range_at(text, 15)], "second_line");
        assert_eq!(&text[line_range_at(text, 2)], "hello world\n");
        assert_eq!(&text[line_range_at(text, 15)], "second_line here\n");
        assert_eq!(&text[line_range_at(text, 30)], "third");
    }

    struct HostView {
        text: Entity<SelectableTextState>,
        host_copies: Rc<Cell<usize>>,
        host_escapes: Rc<Cell<usize>>,
        custom_menu_clicks: Rc<Cell<usize>>,
        with_custom_menu: bool,
    }

    impl Render for HostView {
        fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
            let host_copies = self.host_copies.clone();
            let host_escapes = self.host_escapes.clone();
            let custom_clicks = self.custom_menu_clicks.clone();
            let mut st = selectable_text(&self.text).size_full();
            if self.with_custom_menu {
                st = st.context_menu_builder(move |_selected, mut entries, _, _| {
                    let clicks = custom_clicks.clone();
                    entries.push(MenuEntry::Separator);
                    entries.push(MenuEntry::item("Custom Action", move |_, _, _| {
                        clicks.set(clicks.get() + 1);
                    }));
                    entries
                });
            }
            div()
                .key_context("SelectableHost")
                .size_full()
                .on_action(move |_: &HostCopy, _, _| {
                    host_copies.set(host_copies.get() + 1);
                })
                .on_action(move |_: &HostEscape, _, _| {
                    host_escapes.set(host_escapes.get() + 1);
                })
                .child(div().w(px(320.)).h(px(160.)).child(st))
        }
    }

    fn setup_host<'a>(
        cx: &'a mut TestAppContext,
        initial: &str,
        with_custom_menu: bool,
    ) -> (Entity<HostView>, Entity<SelectableTextState>, &'a mut VisualTestContext) {
        cx.update(|cx| {
            crate::components::init_components(cx);
            cx.bind_keys([
                KeyBinding::new("ctrl-c", HostCopy, Some("SelectableHost")),
                KeyBinding::new("cmd-c", HostCopy, Some("SelectableHost")),
                KeyBinding::new("escape", HostEscape, Some("SelectableHost")),
            ]);
        });
        let host_copies = Rc::new(Cell::new(0));
        let host_escapes = Rc::new(Cell::new(0));
        let custom_menu_clicks = Rc::new(Cell::new(0));
        let initial = initial.to_string();
        let (view, cx) = cx.add_window_view(|_, cx| {
            let text = cx.new(|cx| SelectableTextState::new(cx).default_value(initial));
            HostView {
                text,
                host_copies,
                host_escapes,
                custom_menu_clicks,
                with_custom_menu,
            }
        });
        let text = view.read_with(cx, |v, _| v.text.clone());
        cx.refresh().unwrap();
        cx.run_until_parked();
        (view, text, cx)
    }

    #[crate::test]
    fn selectable_text_drag_select_copy_and_shortcuts(cx: &mut TestAppContext) {
        let (view, text, cx) = setup_host(cx, "alpha beta gamma\nsecond line here\nthird line", false);
        let copied_events: Rc<RefCell<Vec<SharedString>>> = Rc::new(RefCell::new(Vec::new()));
        let copied_clone = copied_events.clone();
        cx.update(|_, cx| {
            cx.subscribe(&text, move |_, ev: &SelectableTextEvent, _| {
                if let SelectableTextEvent::Copy(s) = ev {
                    copied_clone.borrow_mut().push(s.clone());
                }
            })
            .detach();
        });

        let vp = text.read_with(cx, |s, _| s.viewport_bounds());
        assert!(vp.size.height > px(0.));

        // Drag across line 0 into line 1.
        let p_start = point(vp.left() + px(2.), vp.top() + px(8.));
        let p_end = point(vp.left() + px(260.), vp.top() + px(28.));
        cx.simulate_event(MouseDownEvent {
            position: p_start,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
            first_mouse: false,
        });
        cx.simulate_event(MouseMoveEvent {
            position: p_end,
            pressed_button: Some(MouseButton::Left),
            modifiers: Modifiers::default(),
        });
        cx.simulate_event(MouseUpEvent {
            position: p_end,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 1,
        });
        cx.run_until_parked();

        let selected = text.read_with(cx, |s, _| s.selected_text().map(str::to_owned));
        assert!(
            selected
                .as_deref()
                .is_some_and(|s| s.contains('\n') && s.contains("second")),
            "expected multi-line selection spanning into line 2, got {selected:?}"
        );

        // Shortcut Copy writes selected text to clipboard without triggering host Copy.
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" });
        cx.run_until_parked();
        let clip = cx.read_from_clipboard().and_then(|i| i.text());
        assert_eq!(clip, selected);
        assert_eq!(copied_events.borrow().len(), 1);
        assert_eq!(view.read_with(cx, |v, _| v.host_copies.get()), 0);

        // Escape clears non-empty selection and does NOT propagate to host.
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert!(!text.read_with(cx, |s, _| s.has_selection()));
        assert_eq!(view.read_with(cx, |v, _| v.host_escapes.get()), 0);

        // With empty selection, Copy propagates to host and Escape propagates to host.
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" });
        cx.simulate_keystrokes("escape");
        cx.run_until_parked();
        assert_eq!(view.read_with(cx, |v, _| v.host_copies.get()), 1);
        assert_eq!(view.read_with(cx, |v, _| v.host_escapes.get()), 1);

        // SelectAll selects full text and Copy copies it all.
        cx.simulate_keystrokes(if cfg!(target_os = "macos") { "cmd-a" } else { "ctrl-a" });
        cx.run_until_parked();
        assert_eq!(
            text.read_with(cx, |s, _| s.selected_text().map(str::to_owned)),
            Some("alpha beta gamma\nsecond line here\nthird line".to_string())
        );
    }

    #[crate::test]
    fn selectable_text_double_triple_click_and_context_menu(cx: &mut TestAppContext) {
        let (_view, text, cx) = setup_host(cx, "hello world\nsecond line\nthird line", true);
        let vp = text.read_with(cx, |s, _| s.viewport_bounds());

        // Double-click selects a word.
        let p = point(vp.left() + px(10.), vp.top() + px(8.));
        cx.simulate_event(MouseDownEvent {
            position: p,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: p,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 2,
        });
        cx.run_until_parked();
        assert_eq!(
            text.read_with(cx, |s, _| s.selected_text().map(str::to_owned)),
            Some("hello".to_string())
        );

        // Right-click opens context menu while preserving the selection.
        let rpos = point(vp.left() + px(20.), vp.top() + px(12.));
        cx.simulate_event(MouseDownEvent {
            position: rpos,
            modifiers: Modifiers::default(),
            button: MouseButton::Right,
            click_count: 1,
            first_mouse: false,
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert!(text.read_with(cx, |s, _| s.is_context_menu_open()));
        assert_eq!(
            text.read_with(cx, |s, _| s.selected_text().map(str::to_owned)),
            Some("hello".to_string())
        );

        // Escape closes context menu while preserving selection.
        cx.simulate_keystrokes("escape");
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert!(!text.read_with(cx, |s, _| s.is_context_menu_open()));
        assert_eq!(
            text.read_with(cx, |s, _| s.selected_text().map(str::to_owned)),
            Some("hello".to_string())
        );

        // Re-open context menu and activate "Copy" via keyboard navigation (Down -> Enter).
        cx.simulate_event(MouseDownEvent {
            position: rpos,
            modifiers: Modifiers::default(),
            button: MouseButton::Right,
            click_count: 1,
            first_mouse: false,
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert!(text.read_with(cx, |s, _| s.is_context_menu_open()));

        super::super::keyboard_tests::keys(cx, "down enter");
        assert!(!text.read_with(cx, |s, _| s.is_context_menu_open()));
        assert_eq!(
            cx.read_from_clipboard().and_then(|i| i.text()),
            Some("hello".to_string())
        );

        // Re-open context menu and activate the custom menu entry (3rd item: Copy, Select All, Custom Action).
        cx.simulate_event(MouseDownEvent {
            position: rpos,
            modifiers: Modifiers::default(),
            button: MouseButton::Right,
            click_count: 1,
            first_mouse: false,
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        super::super::keyboard_tests::keys(cx, "down down down enter");
        assert!(!text.read_with(cx, |s, _| s.is_context_menu_open()));
        assert_eq!(_view.read_with(cx, |v, _| v.custom_menu_clicks.get()), 1);

        // Triple-click selects the entire logical line.
        cx.simulate_event(MouseDownEvent {
            position: p,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 3,
            first_mouse: false,
        });
        cx.simulate_event(MouseUpEvent {
            position: p,
            modifiers: Modifiers::default(),
            button: MouseButton::Left,
            click_count: 3,
        });
        cx.run_until_parked();
        assert_eq!(
            text.read_with(cx, |s, _| s.selected_text().map(str::to_owned)),
            Some("hello world\n".to_string())
        );
    }

    #[crate::test]
    fn selectable_text_highlights_preserve_selection_and_set_text_resets(cx: &mut TestAppContext) {
        let (_view, text, cx) = setup_host(cx, "foo bar baz", false);
        text.update(cx, |s, cx| {
            s.set_selection(4..7, cx);
            assert_eq!(s.selected_text(), Some("bar"));
            s.set_highlights(
                vec![(
                    0..3,
                    HighlightStyle {
                        background_color: Some(rgb(0xFF0000).into()),
                        ..Default::default()
                    },
                )],
                cx,
            );
            // Updating highlights keeps selection intact.
            assert_eq!(s.selected_text(), Some("bar"));
        });
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert_eq!(text.read_with(cx, |s, _| s.selected_text().map(str::to_owned)), Some("bar".into()));

        // Replacing text resets selection.
        text.update(cx, |s, cx| s.set_text("new content", cx));
        cx.refresh().unwrap();
        cx.run_until_parked();
        assert!(!text.read_with(cx, |s, _| s.has_selection()));
        assert_eq!(text.read_with(cx, |s, _| s.text().to_string()), "new content");
    }

    #[crate::test]
    fn selectable_text_inline_mode_and_virtualized_scrolling(cx: &mut TestAppContext) {
        cx.update(crate::components::init_components);

        // Inline (non-scrollable) mode supports selection and copy.
        let (inline_st, cx_inline) = cx.add_window_view(|_, cx| {
            SelectableTextState::new(cx)
                .scrollable(false)
                .default_value("inline line one\ninline line two")
        });
        cx_inline.refresh().unwrap();
        cx_inline.run_until_parked();
        inline_st.update(cx_inline, |s, cx| {
            s.select_all(cx);
            assert!(s.copy_selection(cx));
        });
        assert_eq!(
            cx_inline.read_from_clipboard().and_then(|i| i.text()),
            Some("inline line one\ninline line two".to_string())
        );

        // Virtualized long document scrolls by page and preserves exact selection across lines.
        let body: String = (0..200)
            .map(|i| format!("line number {i:03} with some sample text"))
            .collect::<Vec<_>>()
            .join("\n");
        let (long_st, cx_long) = cx.add_window_view(|_, cx| {
            SelectableTextState::new(cx).default_value(body.clone())
        });
        cx_long.refresh().unwrap();
        cx_long.run_until_parked();

        assert_eq!(
            long_st.read_with(cx_long, |s, _| s.list_state().logical_scroll_top().item_ix),
            0
        );
        long_st.update(cx_long, |s, cx| s.scroll_page(true, cx));
        cx_long.refresh().unwrap();
        cx_long.run_until_parked();
        assert!(
            long_st.read_with(cx_long, |s, _| s.list_state().logical_scroll_top().item_ix) > 0,
            "expected scroll_page(true) to advance virtualized scroll offset"
        );
        long_st.update(cx_long, |s, cx| s.scroll_to_top(cx));
        cx_long.refresh().unwrap();
        cx_long.run_until_parked();
        assert_eq!(
            long_st.read_with(cx_long, |s, _| s.list_state().logical_scroll_top().item_ix),
            0
        );
    }
}

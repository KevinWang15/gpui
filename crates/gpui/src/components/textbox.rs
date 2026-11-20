//! Multi-line [`TextBox`] / [`TextArea`] and [`TextBoxState`] with full
//! multi-line editing, IME support, line & document navigation, multi-line mouse selection,
//! clipboard, undo/redo, vertical scrolling, and character/line counter footer.

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::{
    AnyElement, App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId,
    ElementInputHandler, Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable,
    GlobalElementId, InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render, RenderOnce,
    ScrollEvent, ShapedLine, SharedString, Style, StyleRefinement, Styled, TextAlign,
    TextRun, UTF16Selection, UnderlineStyle, Window, div, fill, point, prelude::*, px,
    relative, size,
};

use super::cursor_blink::{CursorBlink, is_mounted_and_focused};
use super::input::{clamp_char_boundary, is_word_char};
use super::layout::focus_ring_shadow;
use super::theme::ActiveTheme;

/// Keyboard actions recognized by [`TextBoxState`].
#[allow(missing_docs)]
pub mod textbox_actions {
    use crate::actions;

    actions!(
        gpui_textbox,
        [
            Backspace,
            Delete,
            DeleteWordLeft,
            Left,
            Right,
            Up,
            Down,
            SelectLeft,
            SelectRight,
            SelectUp,
            SelectDown,
            SelectAll,
            LineStart,
            LineEnd,
            DocStart,
            DocEnd,
            Enter,
            Submit,
            Paste,
            Cut,
            Copy,
            Undo,
            Redo,
        ]
    );
}
use textbox_actions::*;

/// Registers cross-platform keybindings for [`TextBoxState`] under `"GpuiTextBox"`.
pub fn bind_textbox_keys(cx: &mut App) {
    let ctx = Some("GpuiTextBox");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("alt-backspace", DeleteWordLeft, ctx),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("up", Up, ctx),
        KeyBinding::new("down", Down, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("shift-up", SelectUp, ctx),
        KeyBinding::new("shift-down", SelectDown, ctx),
        KeyBinding::new("home", LineStart, ctx),
        KeyBinding::new("end", LineEnd, ctx),
        KeyBinding::new("cmd-left", LineStart, ctx),
        KeyBinding::new("cmd-right", LineEnd, ctx),
        KeyBinding::new("cmd-up", DocStart, ctx),
        KeyBinding::new("cmd-down", DocEnd, ctx),
        KeyBinding::new("ctrl-home", DocStart, ctx),
        KeyBinding::new("ctrl-end", DocEnd, ctx),
        KeyBinding::new("enter", Enter, ctx),
        KeyBinding::new("cmd-enter", Submit, ctx),
        KeyBinding::new("ctrl-enter", Submit, ctx),
        KeyBinding::new("cmd-a", SelectAll, ctx),
        KeyBinding::new("ctrl-a", SelectAll, ctx),
        KeyBinding::new("cmd-c", Copy, ctx),
        KeyBinding::new("ctrl-c", Copy, ctx),
        KeyBinding::new("cmd-x", Cut, ctx),
        KeyBinding::new("ctrl-x", Cut, ctx),
        KeyBinding::new("cmd-v", Paste, ctx),
        KeyBinding::new("ctrl-v", Paste, ctx),
        KeyBinding::new("cmd-z", Undo, ctx),
        KeyBinding::new("ctrl-z", Undo, ctx),
        KeyBinding::new("cmd-shift-z", Redo, ctx),
        KeyBinding::new("ctrl-shift-z", Redo, ctx),
        KeyBinding::new("ctrl-y", Redo, ctx),
    ]);
}

/// Events emitted by [`TextBoxState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TextBoxEvent {
    /// Emitted whenever the text changes.
    Change(SharedString),
    /// Emitted on `Cmd+Enter` / `Ctrl+Enter`.
    Submit(SharedString),
    /// Emitted on focus.
    Focus,
    /// Emitted on blur.
    Blur,
}

#[derive(Clone)]
struct UndoEntry {
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

struct LineLayout {
    byte_range: Range<usize>,
    shaped: ShapedLine,
}

/// Alias for [`TextBoxState`].
pub type TextAreaState = TextBoxState;

/// Entity state backing a multi-line [`TextBox`] / [`TextArea`].
pub struct TextBoxState {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    preferred_x: Option<Pixels>,
    last_lines: Vec<LineLayout>,
    last_bounds: Option<Bounds<Pixels>>,
    last_line_height: Pixels,
    scroll_x: Pixels,
    scroll_y: Pixels,
    /// Last `(cursor_offset, content.len())` for which scroll auto-followed the caret,
    /// allowing manual mouse-wheel and scrollbar dragging without snapping back.
    last_cursor_scroll_sync: Option<(usize, usize)>,
    /// The scroll position and its largest value, for the window's scroll
    /// routing (`custom_scroll`), which asks between frames.
    scroll_extent: std::rc::Rc<std::cell::Cell<(Pixels, Pixels)>>,
    scrollbar_state: std::rc::Rc<std::cell::RefCell<crate::OverlayScrollbarState>>,
    is_selecting: bool,
    rows: usize,
    disabled: bool,
    read_only: bool,
    invalid: bool,
    max_length: Option<usize>,
    show_counter: bool,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    was_focused: bool,
    blink: CursorBlink,
    frame_style: StyleRefinement,
}

impl EventEmitter<TextBoxEvent> for TextBoxState {}

impl Focusable for TextBoxState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TextBoxState {
    /// Creates a new empty multi-line [`TextBoxState`] defaulting to 4 visible rows.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: SharedString::new_static(""),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            preferred_x: None,
            last_lines: Vec::new(),
            last_bounds: None,
            last_line_height: px(20.),
            scroll_x: px(0.),
            scroll_y: px(0.),
            last_cursor_scroll_sync: None,
            scroll_extent: Default::default(),
            scrollbar_state: std::rc::Rc::new(std::cell::RefCell::new(
                crate::OverlayScrollbarState::default(),
            )),
            is_selecting: false,
            rows: 4,
            disabled: false,
            read_only: false,
            invalid: false,
            max_length: None,
            show_counter: false,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            was_focused: false,
            blink: CursorBlink::new(),
            frame_style: StyleRefinement::default(),
        }
    }

    fn should_blink_cursor(&self, window: &Window) -> bool {
        !self.disabled
            && self.focus_handle.is_focused(window)
            && window.is_window_active()
            && self.selected_range.is_empty()
    }

    #[cfg(test)]
    pub(crate) fn blink(&self) -> &CursorBlink {
        &self.blink
    }

    /// Sets initial text content.
    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        self.content = value.into();
        let len = self.content.len();
        self.selected_range = len..len;
        self
    }

    /// Sets placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the number of visible text rows (minimum 1).
    pub fn rows(mut self, rows: usize) -> Self {
        self.rows = rows.max(1);
        self
    }

    /// Disables interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Allows selection/copying without allowing edits.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Highlights the border with destructive error styling.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Sets an optional maximum Unicode character count.
    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    /// Displays a character/line counter in the bottom bar.
    pub fn show_counter(mut self, show: bool) -> Self {
        self.show_counter = show;
        self
    }

    /// Returns the full multi-line text content.
    pub fn value(&self) -> &str {
        &self.content
    }

    /// Returns the number of lines in the current text.
    pub fn line_count(&self) -> usize {
        self.content.split('\n').count()
    }

    /// Programmatically sets the text value and emits [`TextBoxEvent::Change`].
    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        let mut next = value.into();
        if let Some(max) = self.max_length {
            next = next.chars().take(max).collect();
        }
        self.content = next;
        let len = self.content.len();
        self.selected_range = len..len;
        self.selection_reversed = false;
        self.marked_range = None;
        self.preferred_x = None;
        self.blink.reset(cx);
        cx.emit(TextBoxEvent::Change(SharedString::from(
            self.content.clone(),
        )));
        cx.notify();
    }

    fn can_edit(&self) -> bool {
        !self.disabled && !self.read_only
    }

    fn push_undo(&mut self) {
        self.undo_stack.push(UndoEntry {
            content: self.content.clone(),
            selected_range: self.selected_range.clone(),
            selection_reversed: self.selection_reversed,
        });
        if self.undo_stack.len() > 200 {
            self.undo_stack.remove(0);
        }
        self.redo_stack.clear();
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn line_ranges(&self) -> Vec<Range<usize>> {
        let mut ranges = Vec::new();
        let mut start = 0;
        for (idx, ch) in self.content.char_indices() {
            if ch == '\n' {
                ranges.push(start..idx);
                start = idx + 1;
            }
        }
        ranges.push(start..self.content.len());
        ranges
    }

    fn line_index_for_offset(&self, offset: usize) -> usize {
        self.content[..offset.min(self.content.len())]
            .bytes()
            .filter(|&b| b == b'\n')
            .count()
    }

    fn move_to(&mut self, offset: usize, reset_x: bool, window: &Window, cx: &mut Context<Self>) {
        let offset = clamp_char_boundary(&self.content, offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        if reset_x {
            self.preferred_x = None;
        }
        self.blink.reset(cx);
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, reset_x: bool, window: &Window, cx: &mut Context<Self>) {
        let offset = clamp_char_boundary(&self.content, offset);
        if self.selection_reversed {
            self.selected_range.start = offset;
        } else {
            self.selected_range.end = offset;
        }
        if self.selected_range.end < self.selected_range.start {
            self.selection_reversed = !self.selection_reversed;
            self.selected_range = self.selected_range.end..self.selected_range.start;
        }
        if reset_x {
            self.preferred_x = None;
        }
        self.blink.reset(cx);
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn previous_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .rev()
            .find_map(|(idx, _)| (idx < offset).then_some(idx))
            .unwrap_or(0)
    }

    fn next_boundary(&self, offset: usize) -> usize {
        self.content
            .grapheme_indices(true)
            .find_map(|(idx, _)| (idx > offset).then_some(idx))
            .unwrap_or(self.content.len())
    }

    fn previous_word_boundary(&self, offset: usize) -> usize {
        let slice = &self.content[..offset.min(self.content.len())];
        let mut iter = slice.char_indices().rev().peekable();
        while let Some(&(_, ch)) = iter.peek() {
            if !is_word_char(ch) {
                iter.next();
            } else {
                break;
            }
        }
        for (idx, ch) in iter {
            if !is_word_char(ch) {
                return idx + ch.len_utf8();
            }
        }
        0
    }

    fn vertical_target(&mut self, delta: isize) -> usize {
        let cursor = self.cursor_offset();
        let ranges = self.line_ranges();
        let current_line = self.line_index_for_offset(cursor);
        let target_line = (current_line as isize + delta)
            .clamp(0, ranges.len().saturating_sub(1) as isize) as usize;
        if target_line == current_line {
            return if delta < 0 { 0 } else { self.content.len() };
        }

        // Use shaped pixel X when available, otherwise preserve character column.
        if let (Some(curr_layout), Some(next_layout)) = (
            self.last_lines.get(current_line),
            self.last_lines.get(target_line),
        ) {
            let local = cursor.saturating_sub(curr_layout.byte_range.start);
            let x = self
                .preferred_x
                .unwrap_or_else(|| curr_layout.shaped.x_for_index(local));
            self.preferred_x = Some(x);
            let next_local = next_layout.shaped.closest_index_for_x(x);
            return (next_layout.byte_range.start + next_local).min(next_layout.byte_range.end);
        }

        let col = self.content[ranges[current_line].start..cursor]
            .chars()
            .count();
        let target_range = &ranges[target_line];
        self.content[target_range.clone()]
            .char_indices()
            .nth(col)
            .map(|(i, _)| target_range.start + i)
            .unwrap_or(target_range.end)
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        let Some(bounds) = self.last_bounds else {
            return 0;
        };
        if self.last_lines.is_empty() {
            return 0;
        }
        let rel_y = (position.y - bounds.top() + self.scroll_y).max(px(0.));
        let line_idx = ((rel_y.0 / self.last_line_height.0.max(1.0)).floor() as usize)
            .min(self.last_lines.len().saturating_sub(1));
        let layout = &self.last_lines[line_idx];
        let local_x = (position.x - bounds.left() + self.scroll_x).max(px(0.));
        let local_idx = layout.shaped.closest_index_for_x(local_x);
        (layout.byte_range.start + local_idx).min(layout.byte_range.end)
    }

    fn offset_from_utf16(&self, offset: usize) -> usize {
        let mut utf8_offset = 0;
        let mut utf16_count = 0;
        for ch in self.content.chars() {
            if utf16_count >= offset {
                break;
            }
            utf16_count += ch.len_utf16();
            utf8_offset += ch.len_utf8();
        }
        utf8_offset
    }

    fn offset_to_utf16(&self, offset: usize) -> usize {
        let mut utf16_offset = 0;
        let mut utf8_count = 0;
        for ch in self.content.chars() {
            if utf8_count >= offset {
                break;
            }
            utf8_count += ch.len_utf8();
            utf16_offset += ch.len_utf16();
        }
        utf16_offset
    }

    fn range_to_utf16(&self, range: &Range<usize>) -> Range<usize> {
        self.offset_to_utf16(range.start)..self.offset_to_utf16(range.end)
    }

    fn range_from_utf16(&self, range_utf16: &Range<usize>) -> Range<usize> {
        self.offset_from_utf16(range_utf16.start)..self.offset_from_utf16(range_utf16.end)
    }

    fn left(&mut self, _: &Left, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.previous_boundary(self.cursor_offset()), true, window, cx);
        } else {
            self.move_to(self.selected_range.start, true, window, cx);
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), true, window, cx);
        } else {
            self.move_to(self.selected_range.end, true, window, cx);
        }
    }

    fn up(&mut self, _: &Up, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_target(-1);
        self.move_to(target, false, window, cx);
    }

    fn down(&mut self, _: &Down, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_target(1);
        self.move_to(target, false, window, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), true, window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), true, window, cx);
    }

    fn select_up(&mut self, _: &SelectUp, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_target(-1);
        self.select_to(target, false, window, cx);
    }

    fn select_down(&mut self, _: &SelectDown, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.vertical_target(1);
        self.select_to(target, false, window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, true, window, cx);
        self.select_to(self.content.len(), true, window, cx);
    }

    fn line_start(&mut self, _: &LineStart, window: &mut Window, cx: &mut Context<Self>) {
        let line_idx = self.line_index_for_offset(self.cursor_offset());
        let start = self.line_ranges()[line_idx].start;
        self.move_to(start, true, window, cx);
    }

    fn line_end(&mut self, _: &LineEnd, window: &mut Window, cx: &mut Context<Self>) {
        let line_idx = self.line_index_for_offset(self.cursor_offset());
        let end = self.line_ranges()[line_idx].end;
        self.move_to(end, true, window, cx);
    }

    fn doc_start(&mut self, _: &DocStart, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, true, window, cx);
    }

    fn doc_end(&mut self, _: &DocEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), true, window, cx);
    }

    fn enter(&mut self, _: &Enter, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        self.replace_text_in_range(None, "\n", window, cx);
    }

    fn submit(&mut self, _: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        cx.emit(TextBoxEvent::Submit(SharedString::from(
            self.content.clone(),
        )));
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if prev == self.cursor_offset() {
                if self.blink.reset(cx) {
                    cx.notify();
                }
                return;
            }
            self.select_to(prev, true, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete(&mut self, _: &Delete, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if self.selected_range.is_empty() {
            let next = self.next_boundary(self.cursor_offset());
            if next == self.cursor_offset() {
                if self.blink.reset(cx) {
                    cx.notify();
                }
                return;
            }
            self.select_to(next, true, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn delete_word_left(
        &mut self,
        _: &DeleteWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        if self.selected_range.is_empty() {
            let target = self.previous_word_boundary(self.cursor_offset());
            self.select_to(target, true, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let normalized = text.replace("\r\n", "\n").replace('\r', "\n");
            self.replace_text_in_range(None, &normalized, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
            self.replace_text_in_range(None, "", window, cx);
        }
    }

    fn undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if let Some(entry) = self.undo_stack.pop() {
            self.redo_stack.push(UndoEntry {
                content: self.content.clone(),
                selected_range: self.selected_range.clone(),
                selection_reversed: self.selection_reversed,
            });
            self.content = entry.content;
            self.selected_range = entry.selected_range;
            self.selection_reversed = entry.selection_reversed;
            self.marked_range = None;
            self.blink.reset(cx);
            window.notify_text_input_changed(&self.focus_handle);
            cx.emit(TextBoxEvent::Change(SharedString::from(
                self.content.clone(),
            )));
            cx.notify();
        }
    }

    fn redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if let Some(entry) = self.redo_stack.pop() {
            self.undo_stack.push(UndoEntry {
                content: self.content.clone(),
                selected_range: self.selected_range.clone(),
                selection_reversed: self.selection_reversed,
            });
            self.content = entry.content;
            self.selected_range = entry.selected_range;
            self.selection_reversed = entry.selection_reversed;
            self.marked_range = None;
            self.blink.reset(cx);
            window.notify_text_input_changed(&self.focus_handle);
            cx.emit(TextBoxEvent::Change(SharedString::from(
                self.content.clone(),
            )));
            cx.notify();
        }
    }

    fn on_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled {
            return;
        }
        window.focus(&self.focus_handle, cx);
        window.request_virtual_keyboard();
        self.is_selecting = true;
        let idx = self.index_for_mouse_position(event.position);
        if event.click_count == 3 {
            let line_idx = self.line_index_for_offset(idx);
            let range = self.line_ranges()[line_idx].clone();
            self.selected_range = range;
            self.selection_reversed = false;
            self.blink.reset(cx);
            window.notify_text_selection_changed(&self.focus_handle);
            cx.notify();
        } else if event.modifiers.shift {
            self.select_to(idx, true, window, cx);
        } else {
            self.move_to(idx, true, window, cx);
        }
    }

    fn on_mouse_up(&mut self, _: &MouseUpEvent, _window: &mut Window, _cx: &mut Context<Self>) {
        self.is_selecting = false;
    }

    fn on_mouse_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.is_selecting {
            self.select_to(self.index_for_mouse_position(event.position), true, window, cx);
        }
    }

    fn on_scroll_wheel(
        &mut self,
        event: &ScrollEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let delta = event.delta.pixel_delta(self.last_line_height);
        let content_height = self.last_line_height * self.line_count() as f32;
        let view_height = self
            .last_bounds
            .map(|b| b.size.height)
            .unwrap_or(self.last_line_height * self.rows as f32);
        let max_scroll = (content_height - view_height).max(px(0.));
        let old_scroll_y = self.scroll_y;
        self.scroll_y = (self.scroll_y - delta.y).clamp(px(0.), max_scroll);
        self.scroll_extent.set((self.scroll_y, max_scroll));
        if self.scroll_y != old_scroll_y {
            crate::OverlayScrollbarState::note_scroll(
                &self.scrollbar_state,
                true,
                false,
                cx.entity_id(),
                window,
                cx,
            );
        }
        cx.notify();
    }
}

impl EntityInputHandler for TextBoxState {
    fn text_for_range(
        &mut self,
        range_utf16: Range<usize>,
        actual_range: &mut Option<Range<usize>>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<String> {
        let range = self.range_from_utf16(&range_utf16);
        actual_range.replace(self.range_to_utf16(&range));
        Some(self.content[range].to_string())
    }

    fn selected_text_range(
        &mut self,
        _ignore_disabled_input: bool,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<UTF16Selection> {
        Some(UTF16Selection {
            range: self.range_to_utf16(&self.selected_range),
            reversed: self.selection_reversed,
        })
    }

    fn set_selected_text_range(
        &mut self,
        range_utf16: Range<usize>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.selected_range = self.range_from_utf16(&range_utf16);
        self.selection_reversed = false;
        self.preferred_x = None;
        self.blink.reset(cx);
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn text_length_utf16(
        &mut self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        Some(self.content.encode_utf16().count())
    }

    fn marked_text_range(
        &self,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Range<usize>> {
        self.marked_range
            .as_ref()
            .map(|range| self.range_to_utf16(range))
    }

    fn unmark_text(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.marked_range = None;
        self.blink.reset(cx);
        window.notify_text_input_changed(&self.focus_handle);
        cx.notify();
    }

    fn replace_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());

        let mut insert_text = new_text;
        let owned_truncated;
        if let Some(max_len) = self.max_length {
            let current_chars = self.content.chars().count();
            let replaced_chars = self.content[range.clone()].chars().count();
            let remaining = max_len.saturating_sub(current_chars.saturating_sub(replaced_chars));
            if insert_text.chars().count() > remaining {
                owned_truncated = insert_text.chars().take(remaining).collect::<String>();
                insert_text = &owned_truncated;
            }
        }

        self.push_undo();
        self.content.replace_range(range.clone(), insert_text);
        let new_cursor = range.start + insert_text.len();
        self.selected_range = new_cursor..new_cursor;
        self.selection_reversed = false;
        self.marked_range = None;
        self.preferred_x = None;
        self.blink.reset(cx);
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(TextBoxEvent::Change(SharedString::from(
            self.content.clone(),
        )));
        cx.notify();
    }

    fn replace_and_mark_text_in_range(
        &mut self,
        range_utf16: Option<Range<usize>>,
        new_text: &str,
        new_selected_range_utf16: Option<Range<usize>>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        let range = range_utf16
            .as_ref()
            .map(|r| self.range_from_utf16(r))
            .or_else(|| self.marked_range.clone())
            .unwrap_or_else(|| self.selected_range.clone());

        self.content.replace_range(range.clone(), new_text);
        if !new_text.is_empty() {
            self.marked_range = Some(range.start..range.start + new_text.len());
        } else {
            self.marked_range = None;
        }
        let insertion_start_utf16 = self.offset_to_utf16(range.start);
        let inserted_length_utf16 = new_text.encode_utf16().count();
        self.selected_range = new_selected_range_utf16
            .map(|selected| {
                self.range_from_utf16(
                    &(insertion_start_utf16 + selected.start.min(inserted_length_utf16)
                        ..insertion_start_utf16 + selected.end.min(inserted_length_utf16)),
                )
            })
            .unwrap_or_else(|| range.start + new_text.len()..range.start + new_text.len());
        self.selection_reversed = false;
        self.preferred_x = None;
        self.blink.reset(cx);
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(TextBoxEvent::Change(SharedString::from(
            self.content.clone(),
        )));
        cx.notify();
    }

    fn bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<Bounds<Pixels>> {
        let range = self.range_from_utf16(&range_utf16);
        let line_idx = self.line_index_for_offset(range.start);
        let layout = self.last_lines.get(line_idx)?;
        let start_local = range.start.saturating_sub(layout.byte_range.start);
        let end_local = range
            .end
            .min(layout.byte_range.end)
            .saturating_sub(layout.byte_range.start);
        let top = bounds.top() + self.last_line_height * line_idx as f32 - self.scroll_y;
        Some(Bounds::from_corners(
            point(bounds.left() + layout.shaped.x_for_index(start_local), top),
            point(
                bounds.left() + layout.shaped.x_for_index(end_local),
                top + self.last_line_height,
            ),
        ))
    }

    fn selection_bounds_for_range(
        &mut self,
        range_utf16: Range<usize>,
        bounds: Bounds<Pixels>,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Vec<Bounds<Pixels>> {
        self.bounds_for_range(range_utf16, bounds, window, cx)
            .into_iter()
            .collect()
    }

    fn character_index_for_point(
        &mut self,
        point: Point<Pixels>,
        _window: &mut Window,
        _cx: &mut Context<Self>,
    ) -> Option<usize> {
        let idx = self.index_for_mouse_position(point);
        Some(self.offset_to_utf16(idx))
    }
}

struct MultiLineTextElement {
    state: Entity<TextBoxState>,
}

struct MultiLinePrepaint {
    lines: Vec<LineLayout>,
    placeholder_line: Option<ShapedLine>,
    selections: Vec<PaintQuad>,
    cursor: Option<PaintQuad>,
    line_height: Pixels,
    scroll_x: Pixels,
    scroll_y: Pixels,
    max_scroll_y: Pixels,
    cursor_scroll_sync: (usize, usize),
    container_hitbox: crate::Hitbox,
    scrollbar_prepaint: Option<Box<crate::OverlayScrollbarPrepaint>>,
}

impl Element for MultiLineTextElement {
    type RequestLayoutState = ();
    type PrepaintState = MultiLinePrepaint;

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
    ) -> (LayoutId, Self::RequestLayoutState) {
        let rows = self.state.read(cx).rows;
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = (window.line_height() * rows as f32).into();
        (window.request_layout(style, [], cx), ())
    }

    fn prepaint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        window: &mut Window,
        cx: &mut App,
    ) -> Self::PrepaintState {
        let theme = cx.theme();
        let state = self.state.read(cx);
        let style = window.text_style();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let line_height = window.line_height();
        let is_empty = state.content.is_empty();
        let ranges = state.line_ranges();
        let selected_range = state.selected_range.clone();
        let cursor_offset = state.cursor_offset();
        let marked_range = state.marked_range.clone();
        let mut scroll_x = state.scroll_x;
        let mut scroll_y = state.scroll_y;

        let mut lines = Vec::with_capacity(ranges.len());
        for range in ranges {
            let slice = &state.content[range.clone()];
            let text = SharedString::from(slice.to_owned());
            let base_run = TextRun {
                len: text.len(),
                font: style.font(),
                color: style.color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs = if let Some(ref marked) = marked_range {
                let overlap_start = marked.start.max(range.start);
                let overlap_end = marked.end.min(range.end);
                if overlap_start < overlap_end {
                    let s = overlap_start - range.start;
                    let e = overlap_end - range.start;
                    vec![
                        TextRun {
                            len: s,
                            ..base_run.clone()
                        },
                        TextRun {
                            len: e - s,
                            underline: Some(UnderlineStyle {
                                color: Some(style.color),
                                thickness: px(1.),
                                wavy: false,
                            }),
                            ..base_run.clone()
                        },
                        TextRun {
                            len: text.len().saturating_sub(e),
                            ..base_run
                        },
                    ]
                    .into_iter()
                    .filter(|r| r.len > 0)
                    .collect()
                } else if text.is_empty() {
                    Vec::new()
                } else {
                    vec![base_run]
                }
            } else if text.is_empty() {
                Vec::new()
            } else {
                vec![base_run]
            };

            let shaped = window
                .text_system()
                .shape_line(text, font_size, &runs, None);
            lines.push(LineLayout {
                byte_range: range,
                shaped,
            });
        }

        let placeholder_line = if is_empty && !state.placeholder.is_empty() {
            let run = TextRun {
                len: state.placeholder.len(),
                font: style.font(),
                color: theme.placeholder,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            Some(window.text_system().shape_line(
                state.placeholder.clone(),
                font_size,
                &[run],
                None,
            ))
        } else {
            None
        };

        let cursor_line = state.line_index_for_offset(cursor_offset);
        let cursor_top_abs = line_height * cursor_line as f32;
        let content_height = line_height * lines.len() as f32;
        let max_scroll_y = (content_height - bounds.size.height).max(px(0.));
        let cursor_scroll_sync = (cursor_offset, state.content.len());
        let follow_cursor = state.last_cursor_scroll_sync != Some(cursor_scroll_sync);
        if follow_cursor {
            if cursor_top_abs - scroll_y + line_height > bounds.size.height {
                scroll_y = (cursor_top_abs + line_height - bounds.size.height).max(px(0.));
            } else if cursor_top_abs < scroll_y {
                scroll_y = cursor_top_abs.max(px(0.));
            }
        }
        scroll_y = scroll_y.clamp(px(0.), max_scroll_y);

        let visible_width = bounds.size.width.max(px(1.));
        let max_line_width = lines
            .iter()
            .map(|l| l.shaped.width)
            .fold(px(0.), |a, b| a.max(b));
        if let Some(layout) = lines.get(cursor_line) {
            let local = cursor_offset.saturating_sub(layout.byte_range.start);
            let cursor_x = layout.shaped.x_for_index(local);
            let pad = px(6.);
            if cursor_x - scroll_x > visible_width - pad {
                scroll_x = (cursor_x - visible_width + pad).max(px(0.));
            } else if cursor_x < scroll_x {
                scroll_x = cursor_x.max(px(0.));
            }
        }
        if max_line_width <= visible_width {
            scroll_x = px(0.);
        }

        let mut selections = Vec::new();
        let mut cursor = None;

        if selected_range.is_empty() {
            if let Some(layout) = lines.get(cursor_line) {
                let local = cursor_offset.saturating_sub(layout.byte_range.start);
                let x = bounds.left() + layout.shaped.x_for_index(local) - scroll_x;
                let y = bounds.top() + cursor_top_abs - scroll_y;
                if y + line_height >= bounds.top() && y <= bounds.bottom() {
                    cursor = Some(fill(
                        Bounds::new(point(x, y), size(px(1.5), line_height)),
                        theme.primary,
                    ));
                }
            }
        } else {
            for (idx, layout) in lines.iter().enumerate() {
                let line_start = layout.byte_range.start;
                let line_end = layout.byte_range.end;
                let includes_newline =
                    selected_range.start <= line_end && selected_range.end > line_end;
                let sel_start = selected_range.start.max(line_start);
                let sel_end = selected_range.end.min(line_end);
                if sel_start < sel_end || includes_newline {
                    let y = bounds.top() + line_height * idx as f32 - scroll_y;
                    if y + line_height < bounds.top() || y > bounds.bottom() {
                        continue;
                    }
                    let x1 = (bounds.left()
                        + layout.shaped.x_for_index(sel_start.saturating_sub(line_start))
                        - scroll_x)
                        .max(bounds.left());
                    let mut x2 = bounds.left()
                        + layout.shaped.x_for_index(sel_end.saturating_sub(line_start))
                        - scroll_x;
                    if includes_newline {
                        x2 = x2 + px(6.);
                    }
                    x2 = x2.min(bounds.right());
                    if x2 > x1 {
                        selections.push(fill(
                            Bounds::from_corners(point(x1, y), point(x2, y + line_height)),
                            theme.selection,
                        ));
                    }
                }
            }
        }

        let container_hitbox = window.insert_hitbox(bounds, crate::HitboxBehavior::Normal);
        let scrollbar_prepaint = crate::prepaint_overlay_scrollbars(
            bounds,
            crate::Edges::default(),
            point(crate::Overflow::Visible, crate::Overflow::Scroll),
            point(px(0.), max_scroll_y),
            crate::ScrollbarVisibility::Auto,
            window,
            cx,
        );

        MultiLinePrepaint {
            lines,
            placeholder_line,
            selections,
            cursor,
            line_height,
            scroll_x,
            scroll_y,
            max_scroll_y,
            cursor_scroll_sync,
            container_hitbox,
            scrollbar_prepaint,
        }
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut Self::RequestLayoutState,
        prepaint: &mut Self::PrepaintState,
        window: &mut Window,
        cx: &mut App,
    ) {
        let (focus_handle, disabled, cursor_visible, scrollbar_state, scroll_extent) = {
            let state = self.state.read(cx);
            (
                state.focus_handle.clone(),
                state.disabled,
                state.blink.is_visible(),
                state.scrollbar_state.clone(),
                state.scroll_extent.clone(),
            )
        };
        let focused = !disabled && focus_handle.is_focused(window);
        if focused {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.state.clone()),
                cx,
            );
        }

        for quad in prepaint.selections.drain(..) {
            window.paint_quad(quad);
        }

        if let Some(placeholder) = prepaint.placeholder_line.take() {
            let _ = placeholder.paint(
                bounds.origin,
                prepaint.line_height,
                TextAlign::Left,
                None,
                window,
                cx,
            );
        } else {
            for (idx, layout) in prepaint.lines.iter().enumerate() {
                let y = bounds.top() + prepaint.line_height * idx as f32 - prepaint.scroll_y;
                if y + prepaint.line_height < bounds.top() || y > bounds.bottom() {
                    continue;
                }
                let _ = layout.shaped.paint(
                    point(bounds.left() - prepaint.scroll_x, y),
                    prepaint.line_height,
                    TextAlign::Left,
                    None,
                    window,
                    cx,
                );
            }
        }

        if focused
            && window.is_window_active()
            && cursor_visible
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        let lines = std::mem::take(&mut prepaint.lines);
        let line_height = prepaint.line_height;
        let scroll_x = prepaint.scroll_x;
        let scroll_y = prepaint.scroll_y;
        let max_scroll_y = prepaint.max_scroll_y;
        let cursor_scroll_sync = prepaint.cursor_scroll_sync;
        self.state.update(cx, |state, _cx| {
            state.last_lines = lines;
            state.last_bounds = Some(bounds);
            state.last_line_height = line_height;
            state.scroll_x = scroll_x;
            state.scroll_y = scroll_y;
            state.last_cursor_scroll_sync = Some(cursor_scroll_sync);
            state.scroll_extent.set((scroll_y, max_scroll_y));
        });

        if let Some(scrollbar_prepaint) = prepaint.scrollbar_prepaint.as_ref() {
            let controller = TextBoxScrollbarController {
                state: self.state.clone(),
                scroll_extent,
            };
            crate::paint_overlay_scrollbars(
                scrollbar_prepaint,
                &prepaint.container_hitbox,
                crate::ScrollbarVisibility::Auto,
                None,
                scrollbar_state,
                controller,
                window,
                cx,
            );
        }
    }
}

#[derive(Clone)]
struct TextBoxScrollbarController {
    state: Entity<TextBoxState>,
    scroll_extent: std::rc::Rc<std::cell::Cell<(Pixels, Pixels)>>,
}

impl crate::ScrollbarController for TextBoxScrollbarController {
    fn offset(&self) -> Point<Pixels> {
        let (y, _) = self.scroll_extent.get();
        point(px(0.), -y)
    }

    fn max_offset(&self) -> Point<Pixels> {
        let (_, max) = self.scroll_extent.get();
        point(px(0.), max)
    }

    fn set_offset(&self, offset: Point<Pixels>, _window: &mut Window, cx: &mut App) {
        self.state.update(cx, |state, cx| {
            let (_, max) = state.scroll_extent.get();
            state.scroll_y = (-offset.y).clamp(px(0.), max);
            state.scroll_extent.set((state.scroll_y, max));
            cx.notify();
        });
    }
}

impl Render for TextBoxState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        {
            let content_height = self.last_line_height * self.line_count() as f32;
            let view_height = self
                .last_bounds
                .map(|b| b.size.height)
                .unwrap_or(self.last_line_height * self.rows as f32);
            self.scroll_extent.set((self.scroll_y, (content_height - view_height).max(px(0.))));
        }
        let theme = cx.theme();
        let focused = self.focus_handle.is_focused(window);
        if focused != self.was_focused {
            self.was_focused = focused;
            cx.emit(if focused {
                TextBoxEvent::Focus
            } else {
                TextBoxEvent::Blur
            });
        }
        let should_blink = self.should_blink_cursor(window) && !cx.reduce_motion();
        self.blink.sync(
            should_blink,
            window,
            cx,
            |this| &mut this.blink,
            |this, window, _cx| {
                is_mounted_and_focused(&this.focus_handle, window)
                    && this.should_blink_cursor(window)
            },
        );

        let border_color = if self.invalid {
            theme.destructive
        } else if focused {
            theme.ring
        } else {
            theme.input_border
        };

        let bg = if self.disabled {
            theme.disabled_bg
        } else {
            theme.input_bg
        };

        let text_color = if self.disabled {
            theme.disabled_foreground
        } else {
            theme.foreground
        };

        let counter_text = if self.show_counter {
            let chars = self.content.chars().count();
            if let Some(max) = self.max_length {
                Some(format!("{chars} / {max}"))
            } else {
                let lines = self.line_count();
                Some(format!("{lines} lines · {chars} chars"))
            }
        } else {
            None
        };

        let mut frame = div()
            .flex()
            .flex_col()
            .w_full()
            .rounded(theme.radius_md)
            .bg(bg)
            .border_1()
            .border_color(border_color)
            .overflow_hidden()
            .when(!self.disabled && theme.elevation_shadows, |el| el.shadow_2xs())
            .when(focused && !self.disabled, |el| {
                let ring = focus_ring_shadow(&theme, self.invalid);
                el.when(!ring.is_empty(), |el| el.shadow(ring))
            })
            .when(!focused && !self.disabled, |el| {
                let hover_border = if self.invalid {
                    theme.destructive
                } else {
                    theme.input_border_hover
                };
                el.hover(move |s| s.border_color(hover_border))
            })
            .child(
                div()
                    .w_full()
                    .px(px(12.))
                    .py(px(8.))
                    .overflow_hidden()
                    .key_context("GpuiTextBox")
                    // One tab stop, unless disabled.
                    .track_focus(&self.focus_handle.clone().tab_stop(!self.disabled))
                    .text_size(px(14.))
                    .line_height(px(20.))
                    .text_color(text_color)
                    .when(!self.disabled, |el| {
                        el.cursor(CursorStyle::IBeam)
                            .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
                            .custom_scroll(cx.entity_id(), {
                                // Scrolls itself; can it still move this way?
                                let extent = self.scroll_extent.clone();
                                move |delta: crate::Point<Pixels>| {
                                    let (y, max) = extent.get();
                                    (delta.y > px(0.) && y > px(0.)) || (delta.y < px(0.) && y < max)
                                }
                            }, cx.listener(Self::on_scroll_wheel))
                    })
                    .when(!self.disabled && focused, |el| {
                        el.on_action(cx.listener(Self::backspace))
                            .on_action(cx.listener(Self::delete))
                            .on_action(cx.listener(Self::delete_word_left))
                            .on_action(cx.listener(Self::left))
                            .on_action(cx.listener(Self::right))
                            .on_action(cx.listener(Self::up))
                            .on_action(cx.listener(Self::down))
                            .on_action(cx.listener(Self::select_left))
                            .on_action(cx.listener(Self::select_right))
                            .on_action(cx.listener(Self::select_up))
                            .on_action(cx.listener(Self::select_down))
                            .on_action(cx.listener(Self::select_all))
                            .on_action(cx.listener(Self::line_start))
                            .on_action(cx.listener(Self::line_end))
                            .on_action(cx.listener(Self::doc_start))
                            .on_action(cx.listener(Self::doc_end))
                            .on_action(cx.listener(Self::enter))
                            .on_action(cx.listener(Self::submit))
                            .on_action(cx.listener(Self::paste))
                            .on_action(cx.listener(Self::cut))
                            .on_action(cx.listener(Self::copy))
                            .on_action(cx.listener(Self::undo))
                            .on_action(cx.listener(Self::redo))
                    })
                    .when(!self.disabled && (focused || self.is_selecting), |el| {
                        el.on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                            .on_mouse_move(cx.listener(Self::on_mouse_move))
                    })
                    .when(self.disabled, |el| el.cursor_not_allowed())
                    .child(MultiLineTextElement { state: cx.entity() }),
            )
            .when_some(counter_text, |el, text| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .justify_end()
                        .px(px(12.))
                        .py(px(5.))
                        .border_t_1()
                        .border_color(theme.border)
                        .bg(theme.surface_muted.opacity(0.45))
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .child(text),
                )
            });
        crate::Refineable::refine(frame.style(), &self.frame_style);
        frame
    }
}

/// Alias for [`TextBox`].
pub type TextArea = TextBox;

/// Renderable multi-line text box element wrapping an [`Entity<TextBoxState>`],
/// with optional footer action toolbar (a "textarea with actions").
#[derive(IntoElement)]
pub struct TextBox {
    state: Entity<TextBoxState>,
    footer: Option<AnyElement>,
    style: StyleRefinement,
}

/// Convenience constructor for [`TextBox::new`].
pub fn text_box(state: &Entity<TextBoxState>) -> TextBox {
    TextBox::new(state)
}

/// Convenience constructor alias for [`TextBox::new`].
pub fn text_area(state: &Entity<TextBoxState>) -> TextBox {
    TextBox::new(state)
}

impl TextBox {
    /// Creates a [`TextBox`] element backed by `state`.
    pub fn new(state: &Entity<TextBoxState>) -> Self {
        Self {
            state: state.clone(),
            footer: None,
            style: StyleRefinement::default(),
        }
    }

    /// Attaches a custom bottom toolbar (e.g. attach/post buttons) below the text box.
    pub fn footer(mut self, footer: impl IntoElement) -> Self {
        self.footer = Some(footer.into_any_element());
        self
    }
}

impl Styled for TextBox {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TextBox {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            self.state.update(cx, |state, cx| {
                state.frame_style = self.style;
                cx.notify();
            });
        }
        if let Some(footer) = self.footer {
            div()
                .flex()
                .flex_col()
                .gap(px(6.))
                .w_full()
                .child(self.state)
                .child(footer)
                .into_any_element()
        } else {
            self.state.into_any_element()
        }
    }
}

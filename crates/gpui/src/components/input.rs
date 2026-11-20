//! Single-line text [`Input`] and [`InputState`] with full IME support,
//! selection, clipboard, undo/redo, word navigation, horizontal scrolling, password masking,
//! clear button, and prefix/suffix icons & addons.

use std::cell::RefCell;
use std::ops::Range;
use std::rc::Rc;

use unicode_segmentation::UnicodeSegmentation;

use crate::{
    App, Bounds, ClipboardItem, Context, CursorStyle, Element, ElementId, ElementInputHandler,
    Entity, EntityInputHandler, EventEmitter, FocusHandle, Focusable, Font, GlobalElementId, Hsla,
    InspectorElementId, IntoElement, KeyBinding, LayoutId, MouseButton, MouseDownEvent,
    MouseMoveEvent, MouseUpEvent, PaintQuad, Pixels, Point, Render, RenderOnce, ShapedLine,
    SharedString, Style, StyleRefinement, Styled, TextAlign, TextRun, UTF16Selection,
    UnderlineStyle, Window, div, fill, point, prelude::*, px, relative, size,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::layout::focus_ring_shadow;
use super::theme::{ActiveTheme, ComponentSize};

/// Keyboard actions recognized by [`InputState`].
#[allow(missing_docs)]
pub mod input_actions {
    use crate::actions;

    actions!(
        gpui_input,
        [
            Backspace,
            Delete,
            DeleteWordLeft,
            Left,
            Right,
            WordLeft,
            WordRight,
            SelectLeft,
            SelectRight,
            SelectWordLeft,
            SelectWordRight,
            SelectToStart,
            SelectToEnd,
            SelectAll,
            Home,
            End,
            Paste,
            Cut,
            Copy,
            Undo,
            Redo,
            Submit,
        ]
    );
}
use input_actions::*;

/// Registers cross-platform keybindings for [`InputState`] under the `"GpuiInput"` key context.
pub fn bind_input_keys(cx: &mut App) {
    let ctx = Some("GpuiInput");
    cx.bind_keys([
        KeyBinding::new("backspace", Backspace, ctx),
        KeyBinding::new("delete", Delete, ctx),
        KeyBinding::new("alt-backspace", DeleteWordLeft, ctx),
        KeyBinding::new("ctrl-backspace", DeleteWordLeft, ctx),
        KeyBinding::new("left", Left, ctx),
        KeyBinding::new("right", Right, ctx),
        KeyBinding::new("alt-left", WordLeft, ctx),
        KeyBinding::new("alt-right", WordRight, ctx),
        KeyBinding::new("ctrl-left", WordLeft, ctx),
        KeyBinding::new("ctrl-right", WordRight, ctx),
        KeyBinding::new("shift-left", SelectLeft, ctx),
        KeyBinding::new("shift-right", SelectRight, ctx),
        KeyBinding::new("alt-shift-left", SelectWordLeft, ctx),
        KeyBinding::new("alt-shift-right", SelectWordRight, ctx),
        KeyBinding::new("ctrl-shift-left", SelectWordLeft, ctx),
        KeyBinding::new("ctrl-shift-right", SelectWordRight, ctx),
        KeyBinding::new("shift-home", SelectToStart, ctx),
        KeyBinding::new("shift-end", SelectToEnd, ctx),
        KeyBinding::new("cmd-shift-left", SelectToStart, ctx),
        KeyBinding::new("cmd-shift-right", SelectToEnd, ctx),
        KeyBinding::new("home", Home, ctx),
        KeyBinding::new("end", End, ctx),
        KeyBinding::new("cmd-left", Home, ctx),
        KeyBinding::new("cmd-right", End, ctx),
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
        KeyBinding::new("enter", Submit, ctx),
    ]);
}

/// Events emitted by [`InputState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum InputEvent {
    /// Emitted whenever the input text changes.
    Change(SharedString),
    /// Emitted when the user presses Enter inside the input.
    Submit(SharedString),
    /// Emitted when the input gains focus.
    Focus,
    /// Emitted when the input loses focus.
    Blur,
}

#[derive(Clone)]
struct UndoEntry {
    content: String,
    selected_range: Range<usize>,
    selection_reversed: bool,
}

#[derive(PartialEq)]
struct ShapedLineCacheKey {
    text: SharedString,
    font: Font,
    font_size: Pixels,
    color: Hsla,
    scale_factor_bits: u32,
}

#[derive(Default)]
struct InputPaintCache {
    last_layout: Option<Rc<ShapedLine>>,
    last_bounds: Option<Bounds<Pixels>>,
    scroll_offset: Pixels,
    shaped_key: Option<ShapedLineCacheKey>,
}

/// Entity state backing a single-line [`Input`].
pub struct InputState {
    focus_handle: FocusHandle,
    content: String,
    placeholder: SharedString,
    selected_range: Range<usize>,
    selection_reversed: bool,
    marked_range: Option<Range<usize>>,
    paint_cache: Rc<RefCell<InputPaintCache>>,
    is_selecting: bool,
    masked: bool,
    mask_revealed: bool,
    clearable: bool,
    disabled: bool,
    read_only: bool,
    invalid: bool,
    max_length: Option<usize>,
    size: ComponentSize,
    leading_icon: Option<LucideIcon>,
    trailing_icon: Option<LucideIcon>,
    prefix_text: Option<SharedString>,
    suffix_text: Option<SharedString>,
    undo_stack: Vec<UndoEntry>,
    redo_stack: Vec<UndoEntry>,
    was_focused: bool,
    frame_style: StyleRefinement,
}

impl EventEmitter<InputEvent> for InputState {}

impl Focusable for InputState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl InputState {
    /// Creates a new empty [`InputState`].
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            content: String::new(),
            placeholder: SharedString::new_static(""),
            selected_range: 0..0,
            selection_reversed: false,
            marked_range: None,
            paint_cache: Rc::new(RefCell::new(InputPaintCache::default())),
            is_selecting: false,
            masked: false,
            mask_revealed: false,
            clearable: false,
            disabled: false,
            read_only: false,
            invalid: false,
            max_length: None,
            size: ComponentSize::Md,
            leading_icon: None,
            trailing_icon: None,
            prefix_text: None,
            suffix_text: None,
            undo_stack: Vec::new(),
            redo_stack: Vec::new(),
            was_focused: false,
            frame_style: StyleRefinement::default(),
        }
    }

    /// Builder setter for initial value.
    pub fn default_value(mut self, value: impl Into<String>) -> Self {
        self.content = value.into();
        let len = self.content.len();
        self.selected_range = len..len;
        self
    }

    /// Builder setter for placeholder text.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Enables password masking (with an interactive show/hide eye toggle).
    pub fn masked(mut self, masked: bool) -> Self {
        self.masked = masked;
        self
    }

    /// Enables an inline clear (`X`) button whenever the input is non-empty.
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Disables editing and dims the control.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Allows selection/copying but prevents mutations.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Highlights the input with destructive invalid styling.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Sets an optional maximum character (Unicode scalar) length.
    pub fn max_length(mut self, max_length: usize) -> Self {
        self.max_length = Some(max_length);
        self
    }

    /// Sets the control size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Sets a leading [`LucideIcon`] inside the input frame.
    pub fn leading_icon(mut self, icon: LucideIcon) -> Self {
        self.leading_icon = Some(icon);
        self
    }

    /// Sets a trailing [`LucideIcon`] inside the input frame.
    pub fn trailing_icon(mut self, icon: LucideIcon) -> Self {
        self.trailing_icon = Some(icon);
        self
    }

    /// Sets an input-group leading addon label (e.g. `"https://"`).
    pub fn prefix(mut self, prefix: impl Into<SharedString>) -> Self {
        self.prefix_text = Some(prefix.into());
        self
    }

    /// Sets an input-group trailing addon label (e.g. `"USD"`).
    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix_text = Some(suffix.into());
        self
    }

    /// Returns the current text value.
    pub fn value(&self) -> &str {
        &self.content
    }

    /// Returns the current selection byte range within [`Self::value`].
    pub fn selected_range(&self) -> Range<usize> {
        self.selected_range.clone()
    }

    /// Programmatically replaces the value and emits [`InputEvent::Change`].
    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.set_value_silent(value, cx);
        cx.emit(InputEvent::Change(SharedString::from(self.content.clone())));
    }

    /// Programmatically replaces the value without emitting [`InputEvent::Change`].
    ///
    /// Use this when synchronizing an embedded `InputState` from a parent widget (such as
    /// `ColorPicker`, `NumberInput`, or `TagInput`) to prevent re-entrant deferred change events.
    pub fn set_value_silent(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        let mut next = value.into();
        if let Some(max) = self.max_length {
            next = next.chars().take(max).collect();
        }
        self.content = next;
        let len = self.content.len();
        self.selected_range = len..len;
        self.selection_reversed = false;
        self.marked_range = None;
        self.paint_cache.borrow_mut().scroll_offset = px(0.);
        cx.notify();
    }

    /// Updates the visual [`ComponentSize`] dynamically.
    pub fn set_size(&mut self, size: ComponentSize, cx: &mut Context<Self>) {
        if self.size != size {
            self.size = size;
            cx.notify();
        }
    }

    /// Clears the input value and resets selection.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.content.is_empty() {
            return;
        }
        self.push_undo();
        self.set_value(String::new(), cx);
    }

    /// Updates the invalid error state dynamically.
    pub fn set_invalid(&mut self, invalid: bool, cx: &mut Context<Self>) {
        self.invalid = invalid;
        cx.notify();
    }

    /// Returns whether this input is currently disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Updates the disabled state dynamically.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        self.disabled = disabled;
        cx.notify();
    }

    /// Updates the placeholder text dynamically.
    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        self.placeholder = placeholder.into();
        cx.notify();
    }

    fn is_effectively_masked(&self) -> bool {
        self.masked && !self.mask_revealed
    }

    fn display_string(&self) -> SharedString {
        if self.is_effectively_masked() {
            "•".repeat(self.content.chars().count()).into()
        } else {
            SharedString::from(self.content.clone())
        }
    }

    fn content_offset_to_display(&self, byte_offset: usize) -> usize {
        if !self.is_effectively_masked() {
            return byte_offset.min(self.content.len());
        }
        let char_count = self.content[..byte_offset.min(self.content.len())]
            .chars()
            .count();
        char_count * '•'.len_utf8()
    }

    fn display_offset_to_content(&self, display_offset: usize) -> usize {
        if !self.is_effectively_masked() {
            return clamp_char_boundary(&self.content, display_offset);
        }
        let bullet_len = '•'.len_utf8();
        let char_idx = display_offset / bullet_len;
        self.content
            .char_indices()
            .nth(char_idx)
            .map(|(idx, _)| idx)
            .unwrap_or(self.content.len())
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

    fn can_edit(&self) -> bool {
        !self.disabled && !self.read_only
    }

    fn cursor_offset(&self) -> usize {
        if self.selection_reversed {
            self.selected_range.start
        } else {
            self.selected_range.end
        }
    }

    fn move_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
        let offset = clamp_char_boundary(&self.content, offset);
        self.selected_range = offset..offset;
        self.selection_reversed = false;
        window.notify_text_selection_changed(&self.focus_handle);
        cx.notify();
    }

    fn select_to(&mut self, offset: usize, window: &Window, cx: &mut Context<Self>) {
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
        let mut boundary = 0;
        for (idx, ch) in iter {
            if !is_word_char(ch) {
                boundary = idx + ch.len_utf8();
                break;
            }
        }
        boundary
    }

    fn next_word_boundary(&self, offset: usize) -> usize {
        let start = offset.min(self.content.len());
        let slice = &self.content[start..];
        let mut iter = slice.char_indices().peekable();
        while let Some(&(_, ch)) = iter.peek() {
            if !is_word_char(ch) {
                iter.next();
            } else {
                break;
            }
        }
        for (idx, ch) in iter {
            if !is_word_char(ch) {
                return start + idx;
            }
        }
        self.content.len()
    }

    fn word_range_at(&self, offset: usize) -> Range<usize> {
        if self.content.is_empty() {
            return 0..0;
        }
        let offset = clamp_char_boundary(&self.content, offset);
        let probe = if offset == self.content.len() {
            self.previous_boundary(offset)
        } else {
            offset
        };
        let is_word = self.content[probe..]
            .chars()
            .next()
            .is_some_and(is_word_char);
        let start = self.content[..probe]
            .char_indices()
            .rev()
            .take_while(|&(_, ch)| is_word_char(ch) == is_word)
            .last()
            .map(|(idx, _)| idx)
            .unwrap_or(probe);
        let end = self.content[probe..]
            .char_indices()
            .take_while(|&(_, ch)| is_word_char(ch) == is_word)
            .last()
            .map(|(idx, ch)| probe + idx + ch.len_utf8())
            .unwrap_or(probe);
        start..end
    }

    fn index_for_mouse_position(&self, position: Point<Pixels>) -> usize {
        if self.content.is_empty() {
            return 0;
        }
        let cache = self.paint_cache.borrow();
        let (Some(bounds), Some(line)) = (cache.last_bounds.as_ref(), cache.last_layout.as_ref())
        else {
            return 0;
        };
        if position.y < bounds.top() {
            return 0;
        }
        if position.y > bounds.bottom() {
            return self.content.len();
        }
        let display_idx =
            line.closest_index_for_x(position.x - bounds.left() + cache.scroll_offset);
        drop(cache);
        self.display_offset_to_content(display_idx)
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
            self.move_to(self.previous_boundary(self.cursor_offset()), window, cx);
        } else {
            self.move_to(self.selected_range.start, window, cx);
        }
    }

    fn right(&mut self, _: &Right, window: &mut Window, cx: &mut Context<Self>) {
        if self.selected_range.is_empty() {
            self.move_to(self.next_boundary(self.selected_range.end), window, cx);
        } else {
            self.move_to(self.selected_range.end, window, cx);
        }
    }

    fn word_left(&mut self, _: &WordLeft, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.previous_word_boundary(self.cursor_offset());
        self.move_to(target, window, cx);
    }

    fn word_right(&mut self, _: &WordRight, window: &mut Window, cx: &mut Context<Self>) {
        let target = self.next_word_boundary(self.cursor_offset());
        self.move_to(target, window, cx);
    }

    fn select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.previous_boundary(self.cursor_offset()), window, cx);
    }

    fn select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.next_boundary(self.cursor_offset()), window, cx);
    }

    fn select_word_left(
        &mut self,
        _: &SelectWordLeft,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.previous_word_boundary(self.cursor_offset());
        self.select_to(target, window, cx);
    }

    fn select_word_right(
        &mut self,
        _: &SelectWordRight,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let target = self.next_word_boundary(self.cursor_offset());
        self.select_to(target, window, cx);
    }

    fn select_to_start(&mut self, _: &SelectToStart, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(0, window, cx);
    }

    fn select_to_end(&mut self, _: &SelectToEnd, window: &mut Window, cx: &mut Context<Self>) {
        self.select_to(self.content.len(), window, cx);
    }

    fn select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
        self.select_to(self.content.len(), window, cx);
    }

    fn home(&mut self, _: &Home, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(0, window, cx);
    }

    fn end(&mut self, _: &End, window: &mut Window, cx: &mut Context<Self>) {
        self.move_to(self.content.len(), window, cx);
    }

    fn backspace(&mut self, _: &Backspace, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if self.selected_range.is_empty() {
            let prev = self.previous_boundary(self.cursor_offset());
            if prev == self.cursor_offset() {
                return;
            }
            self.select_to(prev, window, cx);
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
                return;
            }
            self.select_to(next, window, cx);
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
            self.select_to(target, window, cx);
        }
        self.replace_text_in_range(None, "", window, cx);
    }

    fn paste(&mut self, _: &Paste, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        if let Some(text) = cx.read_from_clipboard().and_then(|item| item.text()) {
            let single_line = text.replace(['\r', '\n'], " ");
            self.replace_text_in_range(None, &single_line, window, cx);
        }
    }

    fn copy(&mut self, _: &Copy, _window: &mut Window, cx: &mut Context<Self>) {
        if self.is_effectively_masked() {
            return;
        }
        if !self.selected_range.is_empty() {
            cx.write_to_clipboard(ClipboardItem::new_string(
                self.content[self.selected_range.clone()].to_string(),
            ));
        }
    }

    fn cut(&mut self, _: &Cut, window: &mut Window, cx: &mut Context<Self>) {
        if !self.can_edit() || self.is_effectively_masked() {
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
            window.notify_text_input_changed(&self.focus_handle);
            cx.emit(InputEvent::Change(SharedString::from(self.content.clone())));
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
            window.notify_text_input_changed(&self.focus_handle);
            cx.emit(InputEvent::Change(SharedString::from(self.content.clone())));
            cx.notify();
        }
    }

    fn submit(&mut self, _: &Submit, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        cx.emit(InputEvent::Submit(SharedString::from(self.content.clone())));
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

        let index = self.index_for_mouse_position(event.position);
        match event.click_count {
            2 => {
                let range = self.word_range_at(index);
                self.selected_range = range;
                self.selection_reversed = false;
                window.notify_text_selection_changed(&self.focus_handle);
                cx.notify();
            }
            3 => {
                self.selected_range = 0..self.content.len();
                self.selection_reversed = false;
                window.notify_text_selection_changed(&self.focus_handle);
                cx.notify();
            }
            _ if event.modifiers.shift => self.select_to(index, window, cx),
            _ => self.move_to(index, window, cx),
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
            self.select_to(self.index_for_mouse_position(event.position), window, cx);
        }
    }
}

impl EntityInputHandler for InputState {
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

        let sanitized = new_text.replace(['\r', '\n'], "");
        let mut insert_text = sanitized.as_str();
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
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(InputEvent::Change(SharedString::from(self.content.clone())));
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
        window.notify_text_input_changed(&self.focus_handle);
        cx.emit(InputEvent::Change(SharedString::from(self.content.clone())));
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
        let start = self.content_offset_to_display(range.start);
        let end = self.content_offset_to_display(range.end);
        let cache = self.paint_cache.borrow();
        let last_layout = cache.last_layout.as_ref()?;
        let scroll_offset = cache.scroll_offset;
        Some(Bounds::from_corners(
            point(
                bounds.left() + last_layout.x_for_index(start) - scroll_offset,
                bounds.top(),
            ),
            point(
                bounds.left() + last_layout.x_for_index(end) - scroll_offset,
                bounds.bottom(),
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
        let display_idx = {
            let cache = self.paint_cache.borrow();
            let line_point = cache.last_bounds?.localize(&point)?;
            let last_layout = cache.last_layout.as_ref()?;
            last_layout.closest_index_for_x(line_point.x + cache.scroll_offset)
        };
        let utf8_idx = self.display_offset_to_content(display_idx);
        Some(self.offset_to_utf16(utf8_idx))
    }
}

struct InputTextElement {
    state: Entity<InputState>,
}

struct InputPrepaintState {
    line: Option<Rc<ShapedLine>>,
    shaped_key: Option<ShapedLineCacheKey>,
    cursor: Option<PaintQuad>,
    selection: Option<PaintQuad>,
    scroll_offset: Pixels,
    paint_cache: Rc<RefCell<InputPaintCache>>,
}

impl Element for InputTextElement {
    type RequestLayoutState = ();
    type PrepaintState = InputPrepaintState;

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
        let mut style = Style::default();
        style.size.width = relative(1.).into();
        style.size.height = window.line_height().into();
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
        let input = self.state.read(cx);
        let is_empty = input.content.is_empty();
        let selected_range = input.selected_range.clone();
        let cursor_display = input.content_offset_to_display(input.cursor_offset());
        let sel_start_display = input.content_offset_to_display(selected_range.start);
        let sel_end_display = input.content_offset_to_display(selected_range.end);
        let marked_display = input.marked_range.as_ref().map(|r| {
            input.content_offset_to_display(r.start)..input.content_offset_to_display(r.end)
        });
        let paint_cache = input.paint_cache.clone();
        let mut scroll_offset = paint_cache.borrow().scroll_offset;

        let style = window.text_style();
        let (display_text, text_color) = if is_empty {
            (input.placeholder.clone(), theme.placeholder)
        } else {
            (input.display_string(), style.color)
        };
        let font = style.font();
        let font_size = style.font_size.to_pixels(window.rem_size());
        let scale_factor_bits = window.scale_factor().to_bits();

        let (line, shaped_key) = if !is_empty
            && let Some(marked_range) = marked_display
        {
            let run = TextRun {
                len: display_text.len(),
                font,
                color: text_color,
                background_color: None,
                underline: None,
                strikethrough: None,
            };
            let runs: Vec<TextRun> = vec![
                TextRun {
                    len: marked_range.start,
                    ..run.clone()
                },
                TextRun {
                    len: marked_range.end.saturating_sub(marked_range.start),
                    underline: Some(UnderlineStyle {
                        color: Some(run.color),
                        thickness: px(1.0),
                        wavy: false,
                    }),
                    ..run.clone()
                },
                TextRun {
                    len: display_text.len().saturating_sub(marked_range.end),
                    ..run
                },
            ]
            .into_iter()
            .filter(|r| r.len > 0)
            .collect();
            let shaped = window
                .text_system()
                .shape_line(display_text, font_size, &runs, None);
            (Rc::new(shaped), None)
        } else {
            let key = ShapedLineCacheKey {
                text: display_text.clone(),
                font: font.clone(),
                font_size,
                color: text_color,
                scale_factor_bits,
            };
            let cached = {
                let cache = paint_cache.borrow();
                if cache.shaped_key.as_ref() == Some(&key) {
                    cache.last_layout.clone()
                } else {
                    None
                }
            };
            let line = match cached {
                Some(line) => line,
                None => {
                    let run = TextRun {
                        len: display_text.len(),
                        font,
                        color: text_color,
                        background_color: None,
                        underline: None,
                        strikethrough: None,
                    };
                    Rc::new(window.text_system().shape_line(
                        display_text,
                        font_size,
                        std::slice::from_ref(&run),
                        None,
                    ))
                }
            };
            (line, Some(key))
        };

        let cursor_x = if is_empty {
            px(0.)
        } else {
            line.x_for_index(cursor_display)
        };
        let visible_width = bounds.size.width.max(px(1.));
        let pad = px(4.);
        if cursor_x - scroll_offset > visible_width - pad {
            scroll_offset = (cursor_x - visible_width + pad).max(px(0.));
        } else if cursor_x < scroll_offset {
            scroll_offset = cursor_x.max(px(0.));
        }
        if line.width <= visible_width {
            scroll_offset = px(0.);
        }

        let (selection, cursor) = if selected_range.is_empty() || is_empty {
            (
                None,
                Some(fill(
                    Bounds::new(
                        point(bounds.left() + cursor_x - scroll_offset, bounds.top()),
                        size(px(1.5), bounds.bottom() - bounds.top()),
                    ),
                    theme.primary,
                )),
            )
        } else {
            let x1 = (bounds.left() + line.x_for_index(sel_start_display) - scroll_offset)
                .max(bounds.left());
            let x2 = (bounds.left() + line.x_for_index(sel_end_display) - scroll_offset)
                .min(bounds.right());
            let quad = if x2 > x1 {
                Some(fill(
                    Bounds::from_corners(point(x1, bounds.top()), point(x2, bounds.bottom())),
                    theme.selection,
                ))
            } else {
                None
            };
            (quad, None)
        };

        InputPrepaintState {
            line: Some(line),
            shaped_key,
            cursor,
            selection,
            scroll_offset,
            paint_cache,
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
        let (focus_handle, disabled) = {
            let input = self.state.read(cx);
            (input.focus_handle.clone(), input.disabled)
        };
        let focused = !disabled && focus_handle.is_focused(window);
        if focused {
            window.handle_input(
                &focus_handle,
                ElementInputHandler::new(bounds, self.state.clone()),
                cx,
            );
        }

        if let Some(selection) = prepaint.selection.take() {
            window.paint_quad(selection);
        }

        let line = prepaint.line.take().unwrap();
        let origin = point(bounds.origin.x - prepaint.scroll_offset, bounds.origin.y);
        let _ = line.paint(
            origin,
            window.line_height(),
            TextAlign::Left,
            None,
            window,
            cx,
        );

        if focused
            && let Some(cursor) = prepaint.cursor.take()
        {
            window.paint_quad(cursor);
        }

        // Write layout/bounds back via RefCell cache instead of taking a mutable entity lease
        // during paint (saves N Entity::update + flush_effects passes per frame on large forms).
        let mut cache = prepaint.paint_cache.borrow_mut();
        cache.last_layout = Some(line);
        cache.last_bounds = Some(bounds);
        cache.scroll_offset = prepaint.scroll_offset;
        cache.shaped_key = prepaint.shaped_key.take();
    }
}

impl Render for InputState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let focused = self.focus_handle.is_focused(window);
        if focused != self.was_focused {
            self.was_focused = focused;
            cx.emit(if focused {
                InputEvent::Focus
            } else {
                InputEvent::Blur
            });
        }

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

        let height = self.size.height();
        let font_size = self.size.font_size();
        let icon_size = self.size.icon_size();
        let line_height = px(font_size.0 * 1.4);

        let show_clear = self.clearable && !self.content.is_empty() && !self.disabled && !self.read_only;
        let show_mask_toggle = self.masked && !self.disabled;

        let mut frame = div()
            .flex()
            .flex_row()
            .items_center()
            .w_full()
            .h(height)
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
            });

        if let Some(prefix) = self.prefix_text.clone() {
            frame = frame.child(
                div()
                    .flex()
                    .items_center()
                    .h_full()
                    .px(px(10.))
                    .bg(theme.surface_muted)
                    .border_r_1()
                    .border_color(theme.border)
                    .text_size(font_size)
                    .text_color(theme.muted_foreground)
                    .whitespace_nowrap()
                    .child(prefix),
            );
        }

        let mut inner = div()
            .flex()
            .flex_row()
            .items_center()
            .flex_1()
            .min_w_0()
            .h_full()
            .px(self.size.px_x())
            .gap(px(8.))
            .key_context("GpuiInput")
            .track_focus(&self.focus_handle)
            .text_size(font_size)
            .line_height(line_height)
            .text_color(text_color)
            .when(!self.disabled, |el| {
                el.cursor(CursorStyle::IBeam)
                    .on_mouse_down(MouseButton::Left, cx.listener(Self::on_mouse_down))
            })
            .when(!self.disabled && focused, |el| {
                el.on_action(cx.listener(Self::backspace))
                    .on_action(cx.listener(Self::delete))
                    .on_action(cx.listener(Self::delete_word_left))
                    .on_action(cx.listener(Self::left))
                    .on_action(cx.listener(Self::right))
                    .on_action(cx.listener(Self::word_left))
                    .on_action(cx.listener(Self::word_right))
                    .on_action(cx.listener(Self::select_left))
                    .on_action(cx.listener(Self::select_right))
                    .on_action(cx.listener(Self::select_word_left))
                    .on_action(cx.listener(Self::select_word_right))
                    .on_action(cx.listener(Self::select_to_start))
                    .on_action(cx.listener(Self::select_to_end))
                    .on_action(cx.listener(Self::select_all))
                    .on_action(cx.listener(Self::home))
                    .on_action(cx.listener(Self::end))
                    .on_action(cx.listener(Self::paste))
                    .on_action(cx.listener(Self::cut))
                    .on_action(cx.listener(Self::copy))
                    .on_action(cx.listener(Self::undo))
                    .on_action(cx.listener(Self::redo))
                    .on_action(cx.listener(Self::submit))
            })
            .when(!self.disabled && (focused || self.is_selecting), |el| {
                el.on_mouse_up(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_up_out(MouseButton::Left, cx.listener(Self::on_mouse_up))
                    .on_mouse_move(cx.listener(Self::on_mouse_move))
            })
            .when(self.disabled, |el| el.cursor_not_allowed());

        if let Some(icon) = self.leading_icon {
            inner = inner.child(
                Icon::new(icon)
                    .size(icon_size)
                    .color(theme.muted_foreground),
            );
        }

        inner = inner.child(
            div()
                .flex_1()
                .min_w_0()
                .overflow_hidden()
                .child(InputTextElement { state: cx.entity() }),
        );

        if show_clear {
            let entity = cx.entity();
            inner = inner.child(
                div()
                    .id("gpui-input-clear")
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(2.))
                    .rounded_full()
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.surface_hover))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| {
                        entity.update(cx, |state, cx| state.clear(cx));
                    })
                    .child(
                        Icon::new(LucideIcon::X)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    ),
            );
        }

        if show_mask_toggle {
            let entity = cx.entity();
            let eye_icon = if self.mask_revealed {
                LucideIcon::EyeOff
            } else {
                LucideIcon::Eye
            };
            inner = inner.child(
                div()
                    .id("gpui-input-mask-toggle")
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(2.))
                    .rounded(theme.radius_sm)
                    .cursor_pointer()
                    .hover(|s| s.bg(theme.surface_hover))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(move |_, _, cx| {
                        entity.update(cx, |state, cx| {
                            state.mask_revealed = !state.mask_revealed;
                            cx.notify();
                        });
                    })
                    .child(
                        Icon::new(eye_icon)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    ),
            );
        }

        if let Some(icon) = self.trailing_icon {
            inner = inner.child(
                Icon::new(icon)
                    .size(icon_size)
                    .color(if self.invalid {
                        theme.destructive
                    } else {
                        theme.muted_foreground
                    }),
            );
        }

        frame = frame.child(inner);

        if let Some(suffix) = self.suffix_text.clone() {
            frame = frame.child(
                div()
                    .flex()
                    .items_center()
                    .h_full()
                    .px(px(10.))
                    .bg(theme.surface_muted)
                    .border_l_1()
                    .border_color(theme.border)
                    .text_size(font_size)
                    .text_color(theme.muted_foreground)
                    .whitespace_nowrap()
                    .child(suffix),
            );
        }

        crate::Refineable::refine(frame.style(), &self.frame_style);
        frame
    }
}

/// Renderable single-line input element wrapping an [`Entity<InputState>`].
///
/// Implements [`Styled`] with direct passthrough to the underlying input frame, so `.bg(...)`,
/// `.border_color(...)`, `.rounded(...)`, `.h(...)`, `.w(...)`, and typography overrides apply
/// directly to the visible control while preserving GPUI view caching.
#[derive(IntoElement)]
pub struct Input {
    state: Entity<InputState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Input::new`].
pub fn input(state: &Entity<InputState>) -> Input {
    Input::new(state)
}

impl Input {
    /// Wraps an [`Entity<InputState>`] as a styled element.
    pub fn new(state: &Entity<InputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Input {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Input {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            self.state.update(cx, |state, cx| {
                state.frame_style = self.style;
                cx.notify();
            });
        }
        self.state
    }
}

pub(crate) fn is_word_char(ch: char) -> bool {
    ch.is_alphanumeric() || ch == '_'
}

pub(crate) fn clamp_char_boundary(s: &str, mut index: usize) -> usize {
    if index >= s.len() {
        return s.len();
    }
    while index > 0 && !s.is_char_boundary(index) {
        index -= 1;
    }
    index
}

//! Standardized [`Typography`](self) primitives: [`Heading`], [`Text`], click-to-edit
//! [`EditableText`], expandable [`TruncatedText`], and syntax-framed copyable [`CodeBlock`].

use std::ops::Range;

use unicode_segmentation::UnicodeSegmentation;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    App, Context, ElementId, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement,
    KeyDownEvent, Pixels, Render, RenderOnce, SharedString, StyleRefinement, Styled, StyledText,
    Window, div, prelude::*, px, uniform_list,
};

use super::button::{Button, ButtonVariant, button};
use super::input::{Input, InputEvent, InputState};
use super::interactive::copy_button;
use super::syntax::{SyntaxTheme, friendly_language_label, highlight_code_cached};
use super::theme::{ActiveTheme, ComponentSize, tabular_numerals};

// ============================================================================
// Heading
// ============================================================================

/// Typographic heading scale (`H1`–`H5`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum HeadingLevel {
    /// `30px` bold page hero title.
    H1,
    /// `24px` bold section title.
    #[default]
    H2,
    /// `19px` semibold card/subsection title.
    H3,
    /// `16px` semibold block title.
    H4,
    /// `14px` semibold compact heading.
    H5,
}

impl HeadingLevel {
    /// Font size in pixels for this level.
    pub const fn font_size(self) -> Pixels {
        match self {
            Self::H1 => px(30.),
            Self::H2 => px(24.),
            Self::H3 => px(19.),
            Self::H4 => px(16.),
            Self::H5 => px(14.),
        }
    }

    /// Font weight for this level.
    pub const fn weight(self) -> FontWeight {
        match self {
            Self::H1 | Self::H2 => FontWeight::BOLD,
            Self::H3 | Self::H4 | Self::H5 => FontWeight::SEMIBOLD,
        }
    }
}

/// Standardized themed heading element (`H1`–`H5`).
#[derive(IntoElement)]
pub struct Heading {
    text: SharedString,
    level: HeadingLevel,
    style: StyleRefinement,
}

/// Convenience constructor for [`Heading::new`].
pub fn heading(level: HeadingLevel, text: impl Into<SharedString>) -> Heading {
    Heading::new(level, text)
}

impl Heading {
    /// Creates a [`Heading`] at the specified [`HeadingLevel`].
    pub fn new(level: HeadingLevel, text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            level,
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Heading {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Heading {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .text_size(self.level.font_size())
            .font_weight(self.level.weight())
            .text_color(theme.foreground)
            .child(self.text);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

// ============================================================================
// Text
// ============================================================================

/// Semantic color tone for [`Text`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TextTone {
    /// Primary theme foreground (default).
    #[default]
    Default,
    /// Muted secondary foreground.
    Muted,
    /// Brand primary accent.
    Primary,
    /// Success green.
    Success,
    /// Warning amber.
    Warning,
    /// Destructive red.
    Destructive,
}

/// Styled inline/block text supporting semantic tones, inline code pill, italic,
/// strikethrough, underline, and optional one-click copy.
#[derive(IntoElement)]
pub struct Text {
    text: SharedString,
    tone: TextTone,
    bold: bool,
    italic: bool,
    underline: bool,
    strikethrough: bool,
    code: bool,
    copy_id: Option<ElementId>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Text::new`].
pub fn ui_text(text: impl Into<SharedString>) -> Text {
    Text::new(text)
}

impl Text {
    /// Creates a body [`Text`] element.
    pub fn new(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: TextTone::Default,
            bold: false,
            italic: false,
            underline: false,
            strikethrough: false,
            code: false,
            copy_id: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the semantic color tone.
    pub fn tone(mut self, tone: TextTone) -> Self {
        self.tone = tone;
        self
    }

    /// Renders muted secondary text.
    pub fn muted(mut self) -> Self {
        self.tone = TextTone::Muted;
        self
    }

    /// Renders semibold text.
    pub fn bold(mut self) -> Self {
        self.bold = true;
        self
    }

    /// Renders italic text.
    pub fn italic(mut self) -> Self {
        self.italic = true;
        self
    }

    /// Underlines the text.
    pub fn underline(mut self) -> Self {
        self.underline = true;
        self
    }

    /// Strikes through the text.
    pub fn strikethrough(mut self) -> Self {
        self.strikethrough = true;
        self
    }

    /// Styles as an inline monospace `code` pill.
    pub fn code(mut self) -> Self {
        self.code = true;
        self
    }

    /// Appends a compact one-click [`super::CopyButton`] next to the text.
    pub fn copyable(mut self, id: impl Into<ElementId>) -> Self {
        self.copy_id = Some(id.into());
        self
    }
}

impl Styled for Text {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Text {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let color = match self.tone {
            TextTone::Default => theme.foreground,
            TextTone::Muted => theme.muted_foreground,
            TextTone::Primary => theme.primary,
            TextTone::Success => theme.success,
            TextTone::Warning => theme.warning,
            TextTone::Destructive => theme.destructive,
        };
        let copy_str = self.text.clone();

        let mut text_el = div()
            .text_size(if self.code { px(12.5) } else { px(14.) })
            .text_color(color)
            .when(self.bold, |el| el.font_weight(FontWeight::SEMIBOLD))
            .when(self.italic, |el| el.italic())
            .when(self.underline, |el| el.underline())
            .when(self.strikethrough, |el| el.line_through())
            .when(self.code, |el| {
                el.px(px(6.))
                    .py(px(1.5))
                    .rounded(theme.radius_sm)
                    .bg(theme.surface_muted)
                    .border_1()
                    .border_color(theme.border)
                    .when_some(theme.mono_font_family.clone(), |el, f| el.font_family(f))
            })
            .child(self.text);

        if let Some(copy_id) = self.copy_id {
            // Forward caller text styles (`text_size`, `text_color`, `font_weight`, etc.) directly
            // onto `text_el` while applying box/layout styles onto the outer `row`.
            let mut text_style = StyleRefinement::default();
            text_style.text = self.style.text.clone();
            crate::Refineable::refine(text_el.style(), &text_style);

            let mut row = div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .child(text_el.min_w_0())
                .child(
                    copy_button(copy_id, copy_str)
                        .icon_only()
                        .variant(ButtonVariant::Ghost)
                        .size(ComponentSize::Xs),
                );
            let mut layout_style = self.style;
            layout_style.text = Default::default();
            crate::Refineable::refine(row.style(), &layout_style);
            row
        } else {
            crate::Refineable::refine(text_el.style(), &self.style);
            text_el
        }
    }
}

// ============================================================================
// EditableText (Click-to-Edit Inline Text)
// ============================================================================

/// Events emitted by [`EditableTextState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditableTextEvent {
    /// Emitted when the user commits an edited value via Enter, blur, or Check button.
    Commit(SharedString),
    /// Emitted when editing is cancelled via Escape or X button.
    Cancel,
}

/// Entity state backing click-to-edit inline [`EditableText`].
pub struct EditableTextState {
    focus_handle: FocusHandle,
    value: SharedString,
    editing: bool,
    disabled: bool,
    allow_empty: bool,
    input: Entity<InputState>,
    placeholder: SharedString,
    font_size: Pixels,
    font_weight: FontWeight,
    frame_style: StyleRefinement,
}

impl EventEmitter<EditableTextEvent> for EditableTextState {}

impl Focusable for EditableTextState {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if self.editing {
            self.input.read(cx).focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl EditableTextState {
    /// Creates an [`EditableTextState`] initialized with `value`.
    pub fn new(value: impl Into<SharedString>, cx: &mut Context<Self>) -> Self {
        let initial: SharedString = value.into();
        let input = cx.new(|cx| {
            InputState::new(cx)
                .default_value(initial.to_string())
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&input, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Submit(_) => this.commit(cx),
            InputEvent::Blur if this.editing => this.commit(cx),
            _ => {}
        })
        .detach();

        Self {
            focus_handle: cx.focus_handle(),
            value: initial,
            editing: false,
            disabled: false,
            allow_empty: false,
            input,
            placeholder: "Click to edit...".into(),
            font_size: px(15.),
            font_weight: FontWeight::MEDIUM,
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets display font size (default `15px`).
    pub fn font_size(mut self, size: Pixels) -> Self {
        self.font_size = size;
        self
    }

    /// Sets display font weight.
    pub fn font_weight(mut self, weight: FontWeight) -> Self {
        self.font_weight = weight;
        self
    }

    /// Sets placeholder shown when empty.
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Allows committing an empty value (default `false` preserves previous non-empty text).
    pub fn allow_empty(mut self, allow: bool) -> Self {
        self.allow_empty = allow;
        self
    }

    /// Disables entering edit mode.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Programmatically updates the value outside edit mode.
    pub fn set_value(&mut self, value: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.value = value.into();
        let val = self.value.to_string();
        self.input
            .update(cx, |inp, cx| inp.set_value_silent(val, cx));
        cx.notify();
    }

    /// Returns the committed value.
    pub fn value(&self) -> &SharedString {
        &self.value
    }

    /// Returns whether the control is currently in inline edit mode.
    pub fn is_editing(&self) -> bool {
        self.editing
    }

    /// Enters edit mode and focuses the inline input.
    pub fn start_editing(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.disabled && !self.editing {
            self.editing = true;
            let val = self.value.to_string();
            self.input
                .update(cx, |inp, cx| inp.set_value_silent(val, cx));
            let fh = self.input.read(cx).focus_handle(cx);
            window.focus(&fh, cx);
            cx.notify();
        }
    }

    /// Commits the current input value and exits edit mode.
    pub fn commit(&mut self, cx: &mut Context<Self>) {
        if self.editing {
            let next: SharedString = self.input.read(cx).value().trim().to_string().into();
            let accepted = !next.is_empty() || self.allow_empty;
            if accepted {
                self.value = next;
                self.editing = false;
                cx.emit(EditableTextEvent::Commit(self.value.clone()));
            } else {
                self.editing = false;
                cx.emit(EditableTextEvent::Cancel);
            }
            cx.notify();
        }
    }

    /// Cancels editing without modifying the committed value.
    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.editing {
            self.editing = false;
            cx.emit(EditableTextEvent::Cancel);
            cx.notify();
        }
    }
}

impl Render for EditableTextState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let hover_bg = theme.surface_hover;
        let entity_id = cx.entity_id();
        let focus_outline = super::focus::focus_visible_outline(&self.focus_handle, window, &theme);

        let mut root = if self.editing {
            div()
                .id(("editable-text-editing", entity_id))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .w_full()
                .on_mouse_down_out(cx.listener(|this, _, window, cx| {
                    this.commit(cx);
                    if this.input.read(cx).focus_handle(cx).is_focused(window) {
                        window.blur(cx);
                    }
                }))
                .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" {
                        this.cancel(cx);
                        window.focus(&this.focus_handle, cx);
                        cx.stop_propagation();
                    }
                }))
                .child(div().flex_1().min_w_0().child(Input::new(&self.input)))
                .child(
                    button(("et-save", entity_id))
                        .primary()
                        .size(ComponentSize::Xs)
                        .icon_only(true)
                        .leading_icon(LucideIcon::Check)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.commit(cx);
                            window.focus(&this.focus_handle, cx);
                        })),
                )
                .child(
                    button(("et-cancel", entity_id))
                        .ghost()
                        .size(ComponentSize::Xs)
                        .icon_only(true)
                        .leading_icon(LucideIcon::X)
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.cancel(cx);
                            window.focus(&this.focus_handle, cx);
                        })),
                )
        } else {
            let is_empty = self.value.is_empty();
            div()
                .id(("editable-text-display", entity_id))
                .when(!self.disabled, |el| {
                    el.track_focus(&self.focus_handle)
                        .tab_stop(true)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        // Enter and Space come here too, as a keyboard click.
                        .on_click(cx.listener(|this, _, window, cx| {
                            this.start_editing(window, cx);
                        }))
                })
                .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed())
                .when(!self.disabled, |el| el.when_some(focus_outline, |el, outline| el.outline(outline)))
                .flex()
                .flex_row()
                .items_center()
                .gap(px(6.))
                .max_w_full()
                .px(px(6.))
                .py(px(3.))
                .rounded(theme.radius_md)
                .child(
                    div()
                        .min_w_0()
                        .truncate()
                        .text_size(self.font_size)
                        .font_weight(self.font_weight)
                        .text_color(if is_empty {
                            theme.placeholder
                        } else {
                            theme.foreground
                        })
                        .child(if is_empty {
                            self.placeholder.clone()
                        } else {
                            self.value.clone()
                        }),
                )
                .when(!self.disabled, |el| {
                    el.child(
                        Icon::new(LucideIcon::Pencil)
                            .size(IconSize::Xs)
                            .color(theme.muted_foreground),
                    )
                })
        };

        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Click-to-edit inline text wrapper bound to [`Entity<EditableTextState>`].
#[derive(IntoElement)]
pub struct EditableText {
    state: Entity<EditableTextState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`EditableText::new`].
pub fn editable_text(state: &Entity<EditableTextState>) -> EditableText {
    EditableText::new(state)
}

impl EditableText {
    /// Creates an [`EditableText`] bound to `state`.
    pub fn new(state: &Entity<EditableTextState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for EditableText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EditableText {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.frame_style = style;
            });
        }
        self.state
    }
}

// ============================================================================
// TruncatedText (Expandable Clamp / "Show more")
// ============================================================================

#[derive(Default)]
struct TruncateExpandState {
    expanded: bool,
}

/// Paragraph text that truncates cleanly after `max_chars` grapheme clusters with an inline
/// `"Show more"` / `"Show less"` toggle.
#[derive(IntoElement)]
pub struct TruncatedText {
    id: ElementId,
    text: SharedString,
    max_chars: usize,
    style: StyleRefinement,
}

/// Convenience constructor for [`TruncatedText::new`].
pub fn truncated_text(
    id: impl Into<ElementId>,
    text: impl Into<SharedString>,
    max_chars: usize,
) -> TruncatedText {
    TruncatedText::new(id, text, max_chars)
}

impl TruncatedText {
    /// Creates a [`TruncatedText`] block collapsing after `max_chars` grapheme clusters.
    pub fn new(id: impl Into<ElementId>, text: impl Into<SharedString>, max_chars: usize) -> Self {
        Self {
            id: id.into(),
            text: text.into(),
            max_chars: max_chars.max(1),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for TruncatedText {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TruncatedText {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let state = window.use_keyed_state((self.id.clone(), "trunc-state"), cx, |_, _| {
            TruncateExpandState::default()
        });
        let expanded = state.read(cx).expanded;

        // Single-pass O(max_chars) grapheme-safe cutoff without scanning the tail of huge strings!
        let cutoff_byte = self
            .text
            .as_ref()
            .grapheme_indices(true)
            .nth(self.max_chars)
            .map(|(byte_idx, _)| byte_idx);
        let needs_trunc = cutoff_byte.is_some();

        let display: SharedString = if let (Some(byte_idx), false) = (cutoff_byte, expanded) {
            let mut s = String::with_capacity(byte_idx + 3);
            s.push_str(&self.text.as_ref()[..byte_idx]);
            s.push('…');
            s.into()
        } else {
            self.text
        };

        let mut root = div()
            .id(self.id.clone())
            .flex()
            .flex_col()
            .items_start()
            .gap(px(4.))
            .text_size(px(13.5))
            .text_color(theme.foreground)
            .child(div().w_full().child(display))
            .when(needs_trunc, |el| {
                el.child(
                    super::focus::tab_stop(div().id((self.id, "trunc-toggle")), true, &theme)
                        .self_start()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.primary)
                        .cursor_pointer()
                        .hover(|s| s.underline())
                        .on_click(move |_, _, cx| {
                            state.update(cx, |s, cx| {
                                s.expanded = !s.expanded;
                                cx.notify();
                            });
                        })
                        .child(if expanded { "Show less" } else { "Show more" }),
                )
            });

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

// ============================================================================
// CodeBlock (Language Header, Optional Line Numbers, Virtualized Scrolling, CopyButton)
// ============================================================================

const CODE_LINE_HEIGHT: f32 = 19.0;

/// Styled monospace code block with optional header title/language badge, line numbers,
/// automatic `uniform_list` virtualization for long snippets, and integrated one-click [`super::CopyButton`].
#[derive(IntoElement)]
pub struct CodeBlock {
    id: ElementId,
    code: SharedString,
    language: Option<SharedString>,
    show_line_numbers: bool,
    wrap: bool,
    max_height: Pixels,
    syntax_theme: Option<SyntaxTheme>,
    style: StyleRefinement,
}

/// Convenience constructor for [`CodeBlock::new`].
pub fn code_block(id: impl Into<ElementId>, code: impl Into<SharedString>) -> CodeBlock {
    CodeBlock::new(id, code)
}

impl CodeBlock {
    /// Creates a new [`CodeBlock`] containing `code`.
    pub fn new(id: impl Into<ElementId>, code: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            code: code.into(),
            language: None,
            show_line_numbers: true,
            wrap: false,
            max_height: px(460.),
            syntax_theme: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the language or filename label in the header bar (e.g. `"rust"`, `"Cargo.toml"`).
    pub fn language(mut self, lang: impl Into<SharedString>) -> Self {
        self.language = Some(lang.into());
        self
    }

    /// Shows or hides line numbers along the left gutter (default `true`).
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Enables or disables initial soft line wrapping (default `false`, scrolls horizontally).
    pub fn wrap(mut self, wrap: bool) -> Self {
        self.wrap = wrap;
        self
    }

    /// Overrides the active [`SyntaxTheme`] palette for this code block.
    pub fn syntax_theme(mut self, syntax_theme: SyntaxTheme) -> Self {
        self.syntax_theme = Some(syntax_theme);
        self
    }

    /// Sets maximum body viewport height before scrolling/virtualizing (default `460px`).
    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = if height.0.is_finite() {
            height.max(px(64.))
        } else {
            px(460.)
        };
        self
    }
}

impl Styled for CodeBlock {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CodeBlock {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let syntax = self
            .syntax_theme
            .unwrap_or_else(|| theme.syntax.clone());
        let bg = syntax.code_bg;
        let header_bg = syntax.code_header_bg;
        let border = syntax.code_border;
        let code_copy = self.code.clone();
        let lang_raw = self.language.as_deref().unwrap_or("");
        let lang_label = friendly_language_label(lang_raw);
        let highlighted = highlight_code_cached(lang_raw, self.code.as_ref());
        let line_count = highlighted.len();
        let digits = ((line_count.max(1) as f32).log10().floor() as usize) + 1;
        let gutter_w = px((digits.max(2) as f32 * 8.5 + 8.0).ceil());
        let show_nums = self.show_line_numbers;
        let mono_font = theme.mono_font_family.clone();

        let initial_wrap = self.wrap;
        let wrap_state = window.use_keyed_state(
            (self.id.clone(), "cb-wrap-state"),
            cx,
            |_, _| initial_wrap,
        );
        let is_wrapped = *wrap_state.read(cx);

        let body_el: crate::AnyElement = if !is_wrapped && line_count > 32 {
            // Virtualize large snippets so 400+ line code blocks render at constant O(visible lines)
            // with zero per-frame re-lexing thanks to the cached `Arc<Vec<HighlightedLine>>`.
            let list_h = px((line_count as f32 * CODE_LINE_HEIGHT + 16.0).min(self.max_height.0));
            let lines_shared = highlighted;
            let syntax_for_list = syntax.clone();
            uniform_list(
                (self.id.clone(), "cb-vlines"),
                line_count,
                move |range: Range<usize>, _window, _cx| {
                    range
                        .map(|idx| {
                            let line = &lines_shared[idx];
                            let row_bg = line
                                .diff_kind
                                .map(|kind| syntax_for_list.diff_line_bg(kind));
                            let rendered_text: crate::AnyElement = if line.text.is_empty() {
                                SharedString::new_static(" ").into_any_element()
                            } else {
                                let hls = line.to_highlights(&syntax_for_list);
                                let mut st = StyledText::new(line.text.clone());
                                if !hls.is_empty() {
                                    st = st.with_highlights(hls);
                                }
                                st.into_any_element()
                            };
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(12.))
                                .w_full()
                                .h(px(CODE_LINE_HEIGHT))
                                .px(px(12.))
                                .when_some(row_bg, |el, fill| el.bg(fill))
                                .text_size(px(12.5))
                                .line_height(px(CODE_LINE_HEIGHT))
                                .text_color(syntax_for_list.plain)
                                .when_some(mono_font.clone(), |el, f| el.font_family(f))
                                .when(show_nums, |el| {
                                    el.child(
                                        div()
                                            .w(gutter_w)
                                            .flex_shrink_0()
                                            .text_right()
                                            .font_features(tabular_numerals())
                                            .text_color(syntax_for_list.gutter_foreground)
                                            .child(format!("{}", idx + 1)),
                                    )
                                })
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .whitespace_nowrap()
                                        .truncate()
                                        .child(rendered_text),
                                )
                        })
                        .collect()
                },
            )
            .w_full()
            .h(list_h)
            .py(px(8.))
            .bg(bg)
            .into_any_element()
        } else {
            let mut body = div()
                .id((self.id.clone(), "cb-body-scroll"))
                .flex()
                .flex_col()
                .gap(px(2.))
                .w_full()
                .py(px(10.))
                .bg(bg)
                .text_size(px(12.5))
                .line_height(px(CODE_LINE_HEIGHT))
                .text_color(syntax.plain)
                .when_some(mono_font, |el, f| el.font_family(f));

            if is_wrapped {
                body = body.max_h(self.max_height).overflow_y_scroll();
            } else {
                body = body.overflow_x_scroll();
            }

            body.children(highlighted.iter().enumerate().map(|(idx, line)| {
                let row_bg = line.diff_kind.map(|kind| syntax.diff_line_bg(kind));
                let rendered_text: crate::AnyElement = if line.text.is_empty() {
                    SharedString::new_static(" ").into_any_element()
                } else {
                    let hls = line.to_highlights(&syntax);
                    let mut st = StyledText::new(line.text.clone());
                    if !hls.is_empty() {
                        st = st.with_highlights(hls);
                    }
                    st.into_any_element()
                };
                let mut text_cell = div().flex_1().min_w_0();
                if !is_wrapped {
                    text_cell = text_cell.whitespace_nowrap();
                }
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(12.))
                    .w_full()
                    .px(px(12.))
                    .when_some(row_bg, |el, fill| el.bg(fill))
                    .when(show_nums, |el| {
                        el.child(
                            div()
                                .w(gutter_w)
                                .flex_shrink_0()
                                .text_right()
                                .font_features(tabular_numerals())
                                .text_color(syntax.gutter_foreground)
                                .child(format!("{}", idx + 1)),
                        )
                    })
                    .child(text_cell.child(rendered_text))
            }))
            .into_any_element()
        };

        let wrap_state_click = wrap_state;
        let mut root = div()
            .id(self.id.clone())
            .flex()
            .flex_col()
            .w_full()
            .rounded(theme.radius_lg)
            .border_1()
            .border_color(border)
            .overflow_hidden()
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .px(px(12.))
                    .py(px(6.))
                    .bg(header_bg)
                    .border_b_1()
                    .border_color(border)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .child(
                                Icon::new(LucideIcon::CodeXml)
                                    .size(IconSize::Sm)
                                    .color(theme.primary),
                            )
                            .child(
                                div()
                                    .text_size(px(12.))
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(syntax.plain)
                                    .child(lang_label),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(4.))
                            .child(
                                Button::icon((self.id.clone(), "cb-wrap"), LucideIcon::TextWrap)
                                    .size(ComponentSize::Xs)
                                    .variant(if is_wrapped {
                                        ButtonVariant::Soft
                                    } else {
                                        ButtonVariant::Ghost
                                    })
                                    .on_click(move |_, window, cx| {
                                        wrap_state_click.update(cx, |w, _| *w = !*w);
                                        window.refresh();
                                    }),
                            )
                            .child(
                                copy_button((self.id, "cb-copy"), code_copy)
                                    .size(ComponentSize::Xs)
                                    .variant(ButtonVariant::Ghost),
                            ),
                    ),
            )
            .child(body_el);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

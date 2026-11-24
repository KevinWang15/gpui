//! Production-grade, pure-GPUI GitHub-Flavored [`Markdown`] renderer with `pulldown-cmark` 0.13
//! parsing, pure-Rust syntax highlighting ([`SyntaxTheme`]), memoized & streaming incremental
//! caching, native single-flow text wrapping, compact inline-code pills, unified cross-block
//! selection & copy, and prompt-injection hardening.
//!
//! # Features
//! - **Full CommonMark + GFM (`pulldown-cmark` 0.13)**: headings (`H1`–`H6`), paragraphs,
//!   nested **bold**, *italic*, ~~strikethrough~~, `` `inline code` ``, reference & inline
//!   `[links](url)` (with safe scheme filtering and optional `resolve_link` / `on_open_link` hooks),
//!   recursive nested lists (unordered, ordered with custom start numbers, GFM task lists, and
//!   multi-paragraph / code-block list items), nested blockquotes, GitHub Callout Alerts
//!   (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`, `[!WARNING]`, `[!CAUTION]`), aligned GFM tables with
//!   rich inline cells & horizontal overflow scrolling, footnotes, horizontal rules, and TeX math
//!   spans (`$inline$` and `$$display$$`, currency-safe for `$26.96`).
//! - **Syntax-highlighted fenced code blocks**: powered by GPUI's shared [`syntax`](super::syntax)
//!   subsystem (~200+ languages in pure Rust), adaptive light/dark [`SyntaxTheme`] palettes,
//!   diff `+/-` row tinting, non-selectable tabular line numbers, soft-wrap toggle, horizontal
//!   scrolling, and one-click copy.
//! - **Memoized & streaming-friendly**: parsed ASTs and highlighted code blocks are cached in
//!   keyed state (and incremental line-level lexing for actively streaming code fences), keeping
//!   hover, scroll, and drag-selection frames O(1).
//! - **Unified selection & copy**: click-and-drag selection across headings, prose, inline code,
//!   lists, alerts, blockquotes, table headers & cells, display math, and syntax-highlighted code
//!   lines (excluding line numbers), double-click word / triple-click line selection, `Cmd/Ctrl+C`,
//!   `Cmd/Ctrl+A`, `Escape`, and a right-click / keyboard context menu (`Copy`, `Select All`,
//!   `Open Link`, `Copy Link`), plus live link URL hover preview.
//! - **Prompt-injection safe**: raw HTML is rendered as literal plain text, `javascript:` /
//!   `vbscript:` / `data:` links are neutralized, and external images `![alt](url)` are never
//!   fetched automatically unless an explicit `.on_image(...)` hook opts in.
//! - **Streaming Answer Components**: detects ```` ```component:<name> ```` fences for
//!   [`AnswerComponentRegistry`] and shows a pulsing `<label>…` placeholder while streaming.

use std::cell::{Cell, RefCell};
use std::collections::{HashMap, HashSet};
use std::ops::Range;
use std::rc::Rc;
use std::sync::Arc;

use pulldown_cmark::{
    Alignment, BlockQuoteKind, CodeBlockKind, Event, HeadingLevel, Options, Parser, Tag, TagEnd,
};
use serde_json::Value;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, BorderStyle, Bounds, ClipboardItem, Context, Corners, CursorStyle,
    DispatchPhase, Edges, Element, ElementId, Entity, FocusHandle, Focusable, FontStyle,
    FontWeight, GlobalElementId, HighlightStyle, Hitbox, HitboxBehavior, Hsla, InspectorElementId,
    IntoElement, KeyBinding, KeyDownEvent, LayoutId, MouseButton, MouseDownEvent, MouseMoveEvent,
    MouseUpEvent, Pixels, Point, RenderOnce, SharedString, StrikethroughStyle, StyleRefinement,
    Styled, StyledText, Subscription, TextAlign, TextLayout, UnderlineStyle, Window, div, fill,
    point, prelude::*, px, quad, size,
};

use super::ai::answer_components::{AnswerComponentRegistry, split_open_component};
use super::button::{Button, ButtonVariant};
use super::context_menu::popup_menu;
use super::input::{clamp_char_boundary, is_word_char};
use super::interactive::copy_button;
use super::popover::MenuEntry;
use super::selectable_text::selectable_text_actions::{ClearSelection, Copy, SelectAll};
use super::syntax::{
    HighlightedLine, IncrementalSyntaxHighlighter, SyntaxTheme, friendly_language_label,
    highlight_code_cached,
};
use super::table::TableAlign;
use super::theme::{ActiveTheme, ComponentSize, Theme, ThemeMode, tabular_numerals};

/// Optional URL resolver for Markdown links (return `None` to render the link as plain text).
pub type LinkResolver = Rc<dyn Fn(&str) -> Option<String> + 'static>;

/// Optional click handler for Markdown links (`href`, `window`, `cx`).
pub type LinkOpenHandler = Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>;

/// Optional trusted image renderer hook (`url`, `alt`, `window`, `cx`).
///
/// Returning `None` falls back to the safe inline placeholder without fetching external URLs.
pub type MarkdownImageRenderer =
    Rc<dyn Fn(&str, &str, &mut Window, &mut App) -> Option<AnyElement> + 'static>;

/// Optional custom math renderer hook (`tex_source`, `is_display`, `window`, `cx`).
///
/// Returning `None` uses GPUI's built-in clean typographic math presentation.
pub type MarkdownMathRenderer =
    Rc<dyn Fn(&str, bool, &mut Window, &mut App) -> Option<AnyElement> + 'static>;

/// GitHub-Flavored Markdown callout alert kind (`> [!NOTE]`, `[!TIP]`, `[!IMPORTANT]`,
/// `[!WARNING]`, `[!CAUTION]`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum MdAlertKind {
    /// `> [!NOTE]` — informational context.
    Note,
    /// `> [!TIP]` — helpful advice or shortcut.
    Tip,
    /// `> [!IMPORTANT]` — key requirement or must-know callout.
    Important,
    /// `> [!WARNING]` — urgent caution or potential pitfall.
    Warning,
    /// `> [!CAUTION]` — destructive or high-risk action warning.
    Caution,
}

impl MdAlertKind {
    fn from_cmark(kind: BlockQuoteKind) -> Self {
        match kind {
            BlockQuoteKind::Note => Self::Note,
            BlockQuoteKind::Tip => Self::Tip,
            BlockQuoteKind::Important => Self::Important,
            BlockQuoteKind::Warning => Self::Warning,
            BlockQuoteKind::Caution => Self::Caution,
        }
    }

    /// Human-readable title label for this alert variant.
    pub const fn title(self) -> &'static str {
        match self {
            Self::Note => "Note",
            Self::Tip => "Tip",
            Self::Important => "Important",
            Self::Warning => "Warning",
            Self::Caution => "Caution",
        }
    }

    fn style_tokens(self, theme: &Theme) -> (LucideIcon, Hsla, Hsla, Hsla) {
        match self {
            Self::Note => (
                LucideIcon::Info,
                theme.info,
                theme.info_soft,
                theme.info_soft_foreground,
            ),
            Self::Tip => (
                LucideIcon::Lightbulb,
                theme.success,
                theme.success_soft,
                theme.success_soft_foreground,
            ),
            Self::Important => (
                LucideIcon::MessageSquareExclamationPoint,
                theme.primary,
                theme.primary_soft,
                theme.primary_soft_foreground,
            ),
            Self::Warning => (
                LucideIcon::TriangleExclamationPoint,
                theme.warning,
                theme.warning_soft,
                theme.warning_soft_foreground,
            ),
            Self::Caution => (
                LucideIcon::OctagonExclamationPoint,
                theme.destructive,
                theme.destructive_soft,
                theme.destructive_soft_foreground,
            ),
        }
    }
}

/// Registers cross-platform keyboard shortcuts (`Cmd/Ctrl+C`, `Cmd/Ctrl+A`, `Escape`) for
/// selectable [`Markdown`] elements under the `"GpuiMarkdown"` key context.
///
/// Called automatically by [`crate::components::init_components`].
pub fn bind_markdown_keys(cx: &mut App) {
    let ctx = Some("GpuiMarkdown");
    cx.bind_keys([
        KeyBinding::new("cmd-c", Copy, ctx),
        KeyBinding::new("ctrl-c", Copy, ctx),
        KeyBinding::new("cmd-a", SelectAll, ctx),
        KeyBinding::new("ctrl-a", SelectAll, ctx),
        KeyBinding::new("escape", ClearSelection, ctx),
    ]);
}

/// Pure-GPUI GitHub-Flavored Markdown & Answer Component element.
#[derive(IntoElement)]
pub struct Markdown {
    id: SharedString,
    source: SharedString,
    streaming: bool,
    selectable: bool,
    math_enabled: bool,
    show_line_numbers: bool,
    syntax_theme: Option<SyntaxTheme>,
    components: Option<AnswerComponentRegistry>,
    resolve_link: Option<LinkResolver>,
    on_open_link: Option<LinkOpenHandler>,
    on_image: Option<MarkdownImageRenderer>,
    on_math: Option<MarkdownMathRenderer>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Markdown::new`].
pub fn markdown(id: impl Into<SharedString>, source: impl Into<SharedString>) -> Markdown {
    Markdown::new(id, source)
}

impl Markdown {
    /// Creates a new [`Markdown`] element (selectable, math-enabled, with syntax-highlighted
    /// code blocks and line numbers by default).
    pub fn new(id: impl Into<SharedString>, source: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            streaming: false,
            selectable: true,
            math_enabled: true,
            show_line_numbers: true,
            syntax_theme: None,
            components: None,
            resolve_link: None,
            on_open_link: None,
            on_image: None,
            on_math: None,
            style: StyleRefinement::default(),
        }
    }

    /// Marks the Markdown as actively streaming so unclosed ```` ```component:<name> ```` fences
    /// render as `<label>…` placeholders and trailing code blocks highlight incrementally.
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Enables or disables text selection, copy shortcuts, and context menu (defaults to `true`).
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
        self
    }

    /// Enables or disables `$inline$` and `$$display$$` TeX math span parsing (defaults to `true`).
    pub fn math_enabled(mut self, enabled: bool) -> Self {
        self.math_enabled = enabled;
        self
    }

    /// Shows or hides the non-selectable line-number gutter on fenced code blocks (default `true`).
    pub fn line_numbers(mut self, show: bool) -> Self {
        self.show_line_numbers = show;
        self
    }

    /// Overrides the [`SyntaxTheme`] palette used for fenced code blocks (defaults to `theme.syntax`).
    pub fn syntax_theme(mut self, syntax_theme: SyntaxTheme) -> Self {
        self.syntax_theme = Some(syntax_theme);
        self
    }

    /// Attaches an [`AnswerComponentRegistry`] for rendering ```` ```component:<name> ```` blocks.
    pub fn answer_components(mut self, registry: AnswerComponentRegistry) -> Self {
        self.components = Some(registry);
        self
    }

    /// Custom link URL resolver (return `None` to neutralize a link into plain text).
    pub fn resolve_link(mut self, f: impl Fn(&str) -> Option<String> + 'static) -> Self {
        self.resolve_link = Some(Rc::new(f));
        self
    }

    /// Custom click handler invoked when the user clicks a resolved link (defaults to `cx.open_url`).
    pub fn on_open_link(mut self, f: impl Fn(&str, &mut Window, &mut App) + 'static) -> Self {
        self.on_open_link = Some(Rc::new(f));
        self
    }

    /// Optional trusted image renderer invoked for standalone Markdown images `![alt](url)`.
    pub fn on_image(
        mut self,
        f: impl Fn(&str, &str, &mut Window, &mut App) -> Option<AnyElement> + 'static,
    ) -> Self {
        self.on_image = Some(Rc::new(f));
        self
    }

    /// Optional custom renderer for display math blocks `$$...$$`.
    pub fn on_math(
        mut self,
        f: impl Fn(&str, bool, &mut Window, &mut App) -> Option<AnyElement> + 'static,
    ) -> Self {
        self.on_math = Some(Rc::new(f));
        self
    }
}

impl Styled for Markdown {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

/// Default link filter blocking `javascript:`, `vbscript:`, and `data:` URLs.
pub fn is_safe_link_url(href: &str) -> bool {
    let trimmed = href.trim();
    if trimmed.is_empty() {
        return false;
    }
    let lower = trimmed.to_ascii_lowercase();
    if lower.starts_with("javascript:")
        || lower.starts_with("vbscript:")
        || lower.starts_with("data:")
    {
        return false;
    }
    true
}

// ---------------------------------------------------------------------------
// Recursive CommonMark + GFM IR powered by `pulldown-cmark` 0.13
// ---------------------------------------------------------------------------

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MdBlock {
    Heading {
        level: u8,
        inlines: Vec<MdInlineNode>,
    },
    Paragraph(Vec<MdInlineNode>),
    Blockquote {
        kind: Option<MdAlertKind>,
        blocks: Vec<MdBlock>,
    },
    List {
        start: Option<u64>,
        items: Vec<MdListItem>,
    },
    Table {
        alignments: Vec<TableAlign>,
        headers: Vec<Vec<MdInlineNode>>,
        rows: Vec<Vec<Vec<MdInlineNode>>>,
    },
    CodeBlock {
        language: String,
        code: String,
    },
    ComponentBlock {
        name: String,
        json_body: String,
    },
    DisplayMath(String),
    ImageBlock {
        alt: String,
        url: String,
    },
    FootnoteDefinition {
        label: String,
        blocks: Vec<MdBlock>,
    },
    Rule,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct MdListItem {
    pub(crate) checked: Option<bool>,
    pub(crate) blocks: Vec<MdBlock>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum MdInlineNode {
    Text(String),
    SoftBreak,
    HardBreak,
    Bold(Vec<MdInlineNode>),
    Italic(Vec<MdInlineNode>),
    Strike(Vec<MdInlineNode>),
    Code(String),
    InlineMath(String),
    DisplayMathInline(String),
    Link {
        children: Vec<MdInlineNode>,
        href: String,
        title: String,
    },
    Image {
        alt: String,
        url: String,
        title: String,
    },
    FootnoteReference(String),
}

enum BlockFrame {
    Document(Vec<MdBlock>),
    Heading {
        level: u8,
        inlines: Vec<MdInlineNode>,
    },
    Paragraph {
        inlines: Vec<MdInlineNode>,
    },
    Blockquote {
        kind: Option<MdAlertKind>,
        blocks: Vec<MdBlock>,
    },
    List {
        start: Option<u64>,
        items: Vec<MdListItem>,
    },
    ListItem {
        checked: Option<bool>,
        blocks: Vec<MdBlock>,
        loose_inlines: Vec<MdInlineNode>,
    },
    Table {
        alignments: Vec<TableAlign>,
        headers: Vec<Vec<MdInlineNode>>,
        rows: Vec<Vec<Vec<MdInlineNode>>>,
        in_head: bool,
        current_row: Vec<Vec<MdInlineNode>>,
    },
    TableCell {
        inlines: Vec<MdInlineNode>,
    },
    CodeBlock {
        info: String,
        code: String,
    },
    HtmlBlock {
        raw: String,
    },
    FootnoteDefinition {
        label: String,
        blocks: Vec<MdBlock>,
    },
}

enum InlineFrame {
    Bold(Vec<MdInlineNode>),
    Italic(Vec<MdInlineNode>),
    Strike(Vec<MdInlineNode>),
    Link {
        href: String,
        title: String,
        children: Vec<MdInlineNode>,
    },
    Image {
        url: String,
        title: String,
        alt: String,
    },
}

fn cmark_options(math_enabled: bool) -> Options {
    let mut opts = Options::ENABLE_TABLES
        | Options::ENABLE_STRIKETHROUGH
        | Options::ENABLE_TASKLISTS
        | Options::ENABLE_FOOTNOTES
        | Options::ENABLE_GFM;
    if math_enabled {
        opts |= Options::ENABLE_MATH;
    }
    opts
}

fn heading_level_u8(level: HeadingLevel) -> u8 {
    match level {
        HeadingLevel::H1 => 1,
        HeadingLevel::H2 => 2,
        HeadingLevel::H3 => 3,
        HeadingLevel::H4 => 4,
        HeadingLevel::H5 => 5,
        HeadingLevel::H6 => 6,
    }
}

fn map_alignment(a: Alignment) -> TableAlign {
    match a {
        Alignment::None | Alignment::Left => TableAlign::Left,
        Alignment::Center => TableAlign::Center,
        Alignment::Right => TableAlign::Right,
    }
}

/// Guards against accidental `$10-$20` or `$12.50 / $26.96` currency ranges being mistaken for
/// single-dollar inline TeX math. Double-dollar `$$...$$` display math is never altered.
fn looks_like_currency_false_positive(tex: &str, next_starts_with_digit: bool) -> bool {
    let trimmed = tex.trim();
    if trimmed.is_empty() {
        return true;
    }
    let starts_digit = trimmed.as_bytes()[0].is_ascii_digit();
    if !starts_digit {
        return false;
    }
    let has_math_signal = trimmed
        .chars()
        .any(|c| matches!(c, '\\' | '^' | '_' | '=' | '{' | '}' | '<' | '>'));
    if has_math_signal {
        return false;
    }
    let ends_separator = trimmed
        .chars()
        .next_back()
        .is_some_and(|c| matches!(c, '-' | '–' | '—' | '/' | ',' | '(' | '~') || c.is_whitespace());
    if ends_separator || next_starts_with_digit {
        return true;
    }
    // Plain currency phrase like `$12.50 to $26.96` (starts with money figure + words).
    let has_space_and_alpha =
        trimmed.contains(char::is_whitespace) && trimmed.chars().any(char::is_alphabetic);
    has_space_and_alpha
}

fn push_inline_to_active(
    block_stack: &mut [BlockFrame],
    inline_stack: &mut [InlineFrame],
    node: MdInlineNode,
) {
    if let Some(top_inline) = inline_stack.last_mut() {
        match top_inline {
            InlineFrame::Bold(v)
            | InlineFrame::Italic(v)
            | InlineFrame::Strike(v)
            | InlineFrame::Link { children: v, .. } => v.push(node),
            InlineFrame::Image { alt, .. } => append_plain_alt(alt, &node),
        }
        return;
    }

    if let Some(top_block) = block_stack.last_mut() {
        match top_block {
            BlockFrame::Heading { inlines, .. }
            | BlockFrame::Paragraph { inlines }
            | BlockFrame::TableCell { inlines } => inlines.push(node),
            BlockFrame::ListItem { loose_inlines, .. } => loose_inlines.push(node),
            BlockFrame::CodeBlock { code, .. } => {
                if let MdInlineNode::Text(s) = node {
                    code.push_str(&s);
                }
            }
            BlockFrame::HtmlBlock { raw } => {
                if let MdInlineNode::Text(s) = node {
                    raw.push_str(&s);
                }
            }
            BlockFrame::Document(blocks)
            | BlockFrame::Blockquote { blocks, .. }
            | BlockFrame::FootnoteDefinition { blocks, .. } => {
                if let Some(MdBlock::Paragraph(v)) = blocks.last_mut() {
                    v.push(node);
                } else {
                    blocks.push(MdBlock::Paragraph(vec![node]));
                }
            }
            BlockFrame::List { .. } | BlockFrame::Table { .. } => {}
        }
    }
}

fn append_plain_alt(alt: &mut String, node: &MdInlineNode) {
    match node {
        MdInlineNode::Text(s) | MdInlineNode::Code(s) | MdInlineNode::InlineMath(s) => {
            alt.push_str(s)
        }
        MdInlineNode::SoftBreak | MdInlineNode::HardBreak => alt.push(' '),
        MdInlineNode::Bold(c)
        | MdInlineNode::Italic(c)
        | MdInlineNode::Strike(c)
        | MdInlineNode::Link { children: c, .. } => {
            for child in c {
                append_plain_alt(alt, child);
            }
        }
        MdInlineNode::Image { alt: inner, .. } => alt.push_str(inner),
        MdInlineNode::DisplayMathInline(s) => alt.push_str(s),
        MdInlineNode::FootnoteReference(l) => {
            alt.push('[');
            alt.push_str(l);
            alt.push(']');
        }
    }
}

fn flush_list_item_loose_inlines(block_stack: &mut [BlockFrame]) {
    if let Some(BlockFrame::ListItem {
        blocks,
        loose_inlines,
        ..
    }) = block_stack.last_mut()
        && !loose_inlines.is_empty()
    {
        let taken = std::mem::take(loose_inlines);
        emit_paragraph_or_split_blocks(taken, blocks);
    }
}

fn push_block_to_parent(block_stack: &mut [BlockFrame], block: MdBlock) {
    if let Some(parent) = block_stack.last_mut() {
        match parent {
            BlockFrame::Document(blocks)
            | BlockFrame::Blockquote { blocks, .. }
            | BlockFrame::FootnoteDefinition { blocks, .. } => blocks.push(block),
            BlockFrame::ListItem {
                blocks,
                loose_inlines,
                ..
            } => {
                if !loose_inlines.is_empty() {
                    let taken = std::mem::take(loose_inlines);
                    emit_paragraph_or_split_blocks(taken, blocks);
                }
                blocks.push(block);
            }
            _ => {}
        }
    }
}

fn is_whitespace_only_inlines(inlines: &[MdInlineNode]) -> bool {
    inlines.iter().all(|n| match n {
        MdInlineNode::Text(s) => s.trim().is_empty(),
        MdInlineNode::SoftBreak | MdInlineNode::HardBreak => true,
        _ => false,
    })
}

/// Splits standalone `$$display math$$` or standalone `![alt](url)` images out of paragraphs so
/// they render as dedicated visual blocks while preserving any surrounding prose.
fn emit_paragraph_or_split_blocks(inlines: Vec<MdInlineNode>, out: &mut Vec<MdBlock>) {
    if inlines.is_empty() {
        return;
    }

    // Check if the paragraph contains only a single image (common `![alt](url)` paragraph).
    let non_ws: Vec<&MdInlineNode> = inlines
        .iter()
        .filter(|n| !matches!(n, MdInlineNode::Text(s) if s.trim().is_empty()) && !matches!(n, MdInlineNode::SoftBreak | MdInlineNode::HardBreak))
        .collect();
    if non_ws.len() == 1
        && let MdInlineNode::Image { alt, url, .. } = non_ws[0]
    {
        out.push(MdBlock::ImageBlock {
            alt: alt.clone(),
            url: url.clone(),
        });
        return;
    }

    let has_display_math = inlines
        .iter()
        .any(|n| matches!(n, MdInlineNode::DisplayMathInline(_)));
    if !has_display_math {
        out.push(MdBlock::Paragraph(inlines));
        return;
    }

    let mut cur = Vec::new();
    for node in inlines {
        match node {
            MdInlineNode::DisplayMathInline(tex) => {
                if !is_whitespace_only_inlines(&cur) {
                    out.push(MdBlock::Paragraph(std::mem::take(&mut cur)));
                } else {
                    cur.clear();
                }
                out.push(MdBlock::DisplayMath(tex.trim().to_owned()));
            }
            other => cur.push(other),
        }
    }
    if !is_whitespace_only_inlines(&cur) {
        out.push(MdBlock::Paragraph(cur));
    }
}

#[cfg(test)]
pub(crate) fn parse_blocks(src: &str) -> Vec<MdBlock> {
    parse_blocks_with_options(src, true)
}

/// Escapes unambiguous standalone currency `$` figures (e.g. `$12.50`, `**$26.96**`, `$10-$20`,
/// `$1,234.56`) outside code fences, inline code spans, and `$$...$$` display math blocks so
/// `pulldown-cmark`'s `ENABLE_MATH` never pairs two currency signs across prose or emphasis
/// markers (like `**$12.50** to **$26.96**`) while keeping real TeX math (`$O(L)$`, `$E = mc^2$`,
/// `$2x + 1$`, `$10^6$`, `$3 \times 4$`, `$42$`, `$$...$$`) completely intact.
fn escape_currency_dollars_for_math(src: &str) -> std::borrow::Cow<'_, str> {
    if !src.contains('$') {
        return std::borrow::Cow::Borrowed(src);
    }

    let bytes = src.as_bytes();
    let len = bytes.len();
    let mut out: Option<String> = None;
    let mut last_copied = 0usize;
    let mut i = 0usize;
    let mut at_line_start = true;
    let mut active_fence: Option<(u8, usize)> = None;

    while i < len {
        // Track fenced code blocks (``` or ~~~) at line starts so we never touch code.
        if at_line_start {
            let mut j = i;
            let mut indent = 0usize;
            while j < len && bytes[j] == b' ' && indent < 3 {
                j += 1;
                indent += 1;
            }
            if j < len && (bytes[j] == b'`' || bytes[j] == b'~') {
                let ch = bytes[j];
                let mut run = 0usize;
                while j + run < len && bytes[j + run] == ch {
                    run += 1;
                }
                if run >= 3 {
                    match active_fence {
                        None => {
                            // Backtick fence info string may not contain backticks per CommonMark.
                            let line_end = bytes[j + run..]
                                .iter()
                                .position(|&b| b == b'\n')
                                .map(|rel| j + run + rel)
                                .unwrap_or(len);
                            let info = &bytes[j + run..line_end];
                            if ch == b'~' || !info.contains(&b'`') {
                                active_fence = Some((ch, run));
                            }
                            i = (line_end + 1).min(len);
                            at_line_start = true;
                            continue;
                        }
                        Some((fch, flen)) if ch == fch && run >= flen => {
                            let line_end = bytes[j + run..]
                                .iter()
                                .position(|&b| b == b'\n')
                                .map(|rel| j + run + rel)
                                .unwrap_or(len);
                            let tail = &bytes[j + run..line_end];
                            if tail.iter().all(|b| b.is_ascii_whitespace()) {
                                active_fence = None;
                                i = (line_end + 1).min(len);
                                at_line_start = true;
                                continue;
                            }
                        }
                        _ => {}
                    }
                }
            }
        }

        if active_fence.is_some() {
            let nl = bytes[i..]
                .iter()
                .position(|&b| b == b'\n')
                .map(|rel| i + rel + 1)
                .unwrap_or(len);
            i = nl;
            at_line_start = true;
            continue;
        }

        let b = bytes[i];
        if b == b'\n' {
            i += 1;
            at_line_start = true;
            continue;
        }
        at_line_start = false;

        // Skip backslash escapes (including `\$`, `` \` ``).
        if b == b'\\' && i + 1 < len {
            i += 2;
            continue;
        }

        // Skip inline code spans atomically.
        if b == b'`' {
            let mut ticks = 0usize;
            while i + ticks < len && bytes[i + ticks] == b'`' {
                ticks += 1;
            }
            let mut k = i + ticks;
            let mut found = None;
            while k < len {
                if bytes[k] == b'`' {
                    let mut close_run = 0usize;
                    while k + close_run < len && bytes[k + close_run] == b'`' {
                        close_run += 1;
                    }
                    if close_run == ticks {
                        found = Some(k + close_run);
                        break;
                    }
                    k += close_run;
                } else {
                    k += 1;
                }
            }
            i = found.unwrap_or(i + ticks);
            continue;
        }

        // Skip `$$...$$` display math spans atomically.
        if b == b'$' && i + 1 < len && bytes[i + 1] == b'$' {
            let mut k = i + 2;
            while k + 1 < len {
                if bytes[k] == b'\\' {
                    k += 2;
                    continue;
                }
                if bytes[k] == b'$' && bytes[k + 1] == b'$' {
                    k += 2;
                    break;
                }
                k += 1;
            }
            i = k.min(len);
            continue;
        }

        // Single `$` followed by an ASCII digit: check if this is a standalone currency figure.
        if b == b'$' && i + 1 < len && bytes[i + 1].is_ascii_digit() {
            let mut num_end = i + 1;
            while num_end < len
                && (bytes[num_end].is_ascii_digit()
                    || matches!(bytes[num_end], b'.' | b','))
            {
                num_end += 1;
            }
            // Optional magnitude suffix (`$50k`, `$1.2M`, `$3B`, `$4T`) when bounded.
            let mut tail = num_end;
            if tail < len
                && matches!(bytes[tail], b'k' | b'K' | b'm' | b'M' | b'b' | b'B' | b't' | b'T')
                && (tail + 1 >= len || !bytes[tail + 1].is_ascii_alphanumeric())
            {
                tail += 1;
            }

            let is_currency = if tail >= len {
                true
            } else {
                let next = bytes[tail];
                if next == b'$' {
                    // Explicit numeric math like `$42$`
                    false
                } else if matches!(next, b'^' | b'_' | b'\\' | b'=' | b'<' | b'>' | b'+' | b'/') {
                    // Immediate math operator like `$10^6$`, `$2+2$`
                    false
                } else if next == b'-' {
                    // `$10-$20` is a currency range; `$10-x$` is math
                    tail + 1 >= len || bytes[tail + 1] == b'$' || bytes[tail + 1].is_ascii_whitespace()
                } else if next.is_ascii_alphabetic() {
                    // `$2x$` or `$3n$` is algebra, not currency
                    false
                } else if next == b' ' {
                    // Check for `$3 \times 4$` or `$2 + 2$` vs `$12.50 to ...`
                    let after_ws = bytes[tail..]
                        .iter()
                        .position(|&c| c != b' ')
                        .map(|rel| tail + rel)
                        .unwrap_or(len);
                    let looks_like_math_rhs = after_ws < len
                        && matches!(
                            bytes[after_ws],
                            b'\\' | b'+' | b'=' | b'<' | b'>' | b'^' | b'_' | b'*' | b'/'
                        );
                    !looks_like_math_rhs
                } else {
                    // Followed by punctuation/emphasis (`**`, `,`, `)`, `]`, `;`, `:`, newline, etc.)
                    true
                }
            };

            if is_currency {
                let buf = out.get_or_insert_with(|| String::with_capacity(len + 8));
                buf.push_str(&src[last_copied..i]);
                buf.push_str("\\$");
                last_copied = i + 1;
            }
            i = tail;
            continue;
        }

        i += 1;
    }

    match out {
        Some(mut buf) => {
            buf.push_str(&src[last_copied..]);
            std::borrow::Cow::Owned(buf)
        }
        None => std::borrow::Cow::Borrowed(src),
    }
}

pub(crate) fn parse_blocks_with_options(src: &str, math_enabled: bool) -> Vec<MdBlock> {
    let preprocessed = if math_enabled {
        escape_currency_dollars_for_math(src)
    } else {
        std::borrow::Cow::Borrowed(src)
    };
    let parser = Parser::new_ext(preprocessed.as_ref(), cmark_options(math_enabled));
    let events: Vec<Event<'_>> = parser.collect();

    let mut block_stack: Vec<BlockFrame> = vec![BlockFrame::Document(Vec::new())];
    let mut inline_stack: Vec<InlineFrame> = Vec::new();

    for (idx, event) in events.iter().enumerate() {
        match event {
            Event::Start(tag) => match tag {
                Tag::Paragraph => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    block_stack.push(BlockFrame::Paragraph {
                        inlines: Vec::new(),
                    });
                }
                Tag::Heading { level, .. } => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    block_stack.push(BlockFrame::Heading {
                        level: heading_level_u8(*level),
                        inlines: Vec::new(),
                    });
                }
                Tag::BlockQuote(kind) => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    block_stack.push(BlockFrame::Blockquote {
                        kind: kind.map(MdAlertKind::from_cmark),
                        blocks: Vec::new(),
                    });
                }
                Tag::CodeBlock(kind) => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    let info = match kind {
                        CodeBlockKind::Fenced(info) => info.trim().to_owned(),
                        CodeBlockKind::Indented => String::new(),
                    };
                    block_stack.push(BlockFrame::CodeBlock {
                        info,
                        code: String::new(),
                    });
                }
                Tag::HtmlBlock => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    block_stack.push(BlockFrame::HtmlBlock { raw: String::new() });
                }
                Tag::List(start) => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    block_stack.push(BlockFrame::List {
                        start: *start,
                        items: Vec::new(),
                    });
                }
                Tag::Item => {
                    block_stack.push(BlockFrame::ListItem {
                        checked: None,
                        blocks: Vec::new(),
                        loose_inlines: Vec::new(),
                    });
                }
                Tag::FootnoteDefinition(label) => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    block_stack.push(BlockFrame::FootnoteDefinition {
                        label: label.as_ref().to_owned(),
                        blocks: Vec::new(),
                    });
                }
                Tag::Table(aligns) => {
                    flush_list_item_loose_inlines(&mut block_stack);
                    block_stack.push(BlockFrame::Table {
                        alignments: aligns.iter().copied().map(map_alignment).collect(),
                        headers: Vec::new(),
                        rows: Vec::new(),
                        in_head: false,
                        current_row: Vec::new(),
                    });
                }
                Tag::TableHead => {
                    if let Some(BlockFrame::Table {
                        in_head,
                        current_row,
                        ..
                    }) = block_stack.last_mut()
                    {
                        *in_head = true;
                        current_row.clear();
                    }
                }
                Tag::TableRow => {
                    if let Some(BlockFrame::Table { current_row, .. }) = block_stack.last_mut() {
                        current_row.clear();
                    }
                }
                Tag::TableCell => {
                    block_stack.push(BlockFrame::TableCell {
                        inlines: Vec::new(),
                    });
                }
                Tag::Strong => inline_stack.push(InlineFrame::Bold(Vec::new())),
                Tag::Emphasis => inline_stack.push(InlineFrame::Italic(Vec::new())),
                Tag::Strikethrough => inline_stack.push(InlineFrame::Strike(Vec::new())),
                Tag::Link {
                    dest_url, title, ..
                } => inline_stack.push(InlineFrame::Link {
                    href: dest_url.as_ref().to_owned(),
                    title: title.as_ref().to_owned(),
                    children: Vec::new(),
                }),
                Tag::Image {
                    dest_url, title, ..
                } => inline_stack.push(InlineFrame::Image {
                    url: dest_url.as_ref().to_owned(),
                    title: title.as_ref().to_owned(),
                    alt: String::new(),
                }),
                _ => {}
            },
            Event::End(tag_end) => match tag_end {
                TagEnd::Strong => {
                    if let Some(InlineFrame::Bold(children)) = inline_stack.pop() {
                        push_inline_to_active(
                            &mut block_stack,
                            &mut inline_stack,
                            MdInlineNode::Bold(children),
                        );
                    }
                }
                TagEnd::Emphasis => {
                    if let Some(InlineFrame::Italic(children)) = inline_stack.pop() {
                        push_inline_to_active(
                            &mut block_stack,
                            &mut inline_stack,
                            MdInlineNode::Italic(children),
                        );
                    }
                }
                TagEnd::Strikethrough => {
                    if let Some(InlineFrame::Strike(children)) = inline_stack.pop() {
                        push_inline_to_active(
                            &mut block_stack,
                            &mut inline_stack,
                            MdInlineNode::Strike(children),
                        );
                    }
                }
                TagEnd::Link => {
                    if let Some(InlineFrame::Link {
                        href,
                        title,
                        children,
                    }) = inline_stack.pop()
                    {
                        push_inline_to_active(
                            &mut block_stack,
                            &mut inline_stack,
                            MdInlineNode::Link {
                                children,
                                href,
                                title,
                            },
                        );
                    }
                }
                TagEnd::Image => {
                    if let Some(InlineFrame::Image { url, title, alt }) = inline_stack.pop() {
                        push_inline_to_active(
                            &mut block_stack,
                            &mut inline_stack,
                            MdInlineNode::Image {
                                alt: alt.trim().to_owned(),
                                url,
                                title,
                            },
                        );
                    }
                }
                TagEnd::Paragraph => {
                    inline_stack.clear();
                    if let Some(BlockFrame::Paragraph { inlines }) = block_stack.pop() {
                        let mut emitted = Vec::new();
                        emit_paragraph_or_split_blocks(inlines, &mut emitted);
                        for blk in emitted {
                            push_block_to_parent(&mut block_stack, blk);
                        }
                    }
                }
                TagEnd::Heading(_) => {
                    inline_stack.clear();
                    if let Some(BlockFrame::Heading { level, inlines }) = block_stack.pop() {
                        push_block_to_parent(&mut block_stack, MdBlock::Heading { level, inlines });
                    }
                }
                TagEnd::BlockQuote(_) => {
                    inline_stack.clear();
                    if let Some(BlockFrame::Blockquote { kind, blocks }) = block_stack.pop() {
                        push_block_to_parent(&mut block_stack, MdBlock::Blockquote { kind, blocks });
                    }
                }
                TagEnd::CodeBlock => {
                    inline_stack.clear();
                    if let Some(BlockFrame::CodeBlock { info, mut code }) = block_stack.pop() {
                        if code.ends_with('\n') {
                            code.pop();
                            if code.ends_with('\r') {
                                code.pop();
                            }
                        }
                        if let Some(comp_name) = info
                            .strip_prefix("component:")
                            .and_then(|s| s.split_whitespace().next())
                            .filter(|s| !s.is_empty())
                        {
                            push_block_to_parent(
                                &mut block_stack,
                                MdBlock::ComponentBlock {
                                    name: comp_name.to_owned(),
                                    json_body: code,
                                },
                            );
                        } else {
                            let language = info.split_whitespace().next().unwrap_or("").to_owned();
                            push_block_to_parent(
                                &mut block_stack,
                                MdBlock::CodeBlock { language, code },
                            );
                        }
                    }
                }
                TagEnd::HtmlBlock => {
                    inline_stack.clear();
                    if let Some(BlockFrame::HtmlBlock { raw }) = block_stack.pop() {
                        let trimmed = raw.trim_end_matches(['\r', '\n']);
                        if !trimmed.is_empty() {
                            push_block_to_parent(
                                &mut block_stack,
                                MdBlock::Paragraph(vec![MdInlineNode::Text(trimmed.to_owned())]),
                            );
                        }
                    }
                }
                TagEnd::Item => {
                    inline_stack.clear();
                    if let Some(BlockFrame::ListItem {
                        checked,
                        mut blocks,
                        loose_inlines,
                    }) = block_stack.pop()
                    {
                        if !loose_inlines.is_empty() {
                            emit_paragraph_or_split_blocks(loose_inlines, &mut blocks);
                        }
                        if let Some(BlockFrame::List { items, .. }) = block_stack.last_mut() {
                            items.push(MdListItem { checked, blocks });
                        }
                    }
                }
                TagEnd::List(_) => {
                    inline_stack.clear();
                    if let Some(BlockFrame::List { start, items }) = block_stack.pop() {
                        push_block_to_parent(&mut block_stack, MdBlock::List { start, items });
                    }
                }
                TagEnd::FootnoteDefinition => {
                    inline_stack.clear();
                    if let Some(BlockFrame::FootnoteDefinition { label, blocks }) =
                        block_stack.pop()
                    {
                        push_block_to_parent(
                            &mut block_stack,
                            MdBlock::FootnoteDefinition { label, blocks },
                        );
                    }
                }
                TagEnd::TableCell => {
                    inline_stack.clear();
                    if let Some(BlockFrame::TableCell { inlines }) = block_stack.pop()
                        && let Some(BlockFrame::Table { current_row, .. }) = block_stack.last_mut()
                    {
                        current_row.push(inlines);
                    }
                }
                TagEnd::TableHead => {
                    if let Some(BlockFrame::Table {
                        in_head,
                        headers,
                        current_row,
                        ..
                    }) = block_stack.last_mut()
                    {
                        *headers = std::mem::take(current_row);
                        *in_head = false;
                    }
                }
                TagEnd::TableRow => {
                    if let Some(BlockFrame::Table {
                        in_head,
                        rows,
                        current_row,
                        ..
                    }) = block_stack.last_mut()
                        && !*in_head
                    {
                        rows.push(std::mem::take(current_row));
                    }
                }
                TagEnd::Table => {
                    inline_stack.clear();
                    if let Some(BlockFrame::Table {
                        alignments,
                        headers,
                        rows,
                        ..
                    }) = block_stack.pop()
                    {
                        push_block_to_parent(
                            &mut block_stack,
                            MdBlock::Table {
                                alignments,
                                headers,
                                rows,
                            },
                        );
                    }
                }
                _ => {}
            },
            Event::Text(s) => {
                push_inline_to_active(
                    &mut block_stack,
                    &mut inline_stack,
                    MdInlineNode::Text(s.as_ref().to_owned()),
                );
            }
            Event::Code(s) => {
                push_inline_to_active(
                    &mut block_stack,
                    &mut inline_stack,
                    MdInlineNode::Code(s.as_ref().to_owned()),
                );
            }
            Event::InlineMath(tex) => {
                let next_starts_digit = events.get(idx + 1).is_some_and(|ev| {
                    matches!(ev, Event::Text(t) if t.as_bytes().first().is_some_and(u8::is_ascii_digit))
                });
                let node = if looks_like_currency_false_positive(tex.as_ref(), next_starts_digit) {
                    MdInlineNode::Text(format!("${}$", tex.as_ref()))
                } else {
                    MdInlineNode::InlineMath(tex.as_ref().to_owned())
                };
                push_inline_to_active(&mut block_stack, &mut inline_stack, node);
            }
            Event::DisplayMath(tex) => {
                push_inline_to_active(
                    &mut block_stack,
                    &mut inline_stack,
                    MdInlineNode::DisplayMathInline(tex.as_ref().to_owned()),
                );
            }
            // Prompt-injection safety: raw HTML is always rendered as literal plain text.
            Event::Html(raw) | Event::InlineHtml(raw) => {
                push_inline_to_active(
                    &mut block_stack,
                    &mut inline_stack,
                    MdInlineNode::Text(raw.as_ref().to_owned()),
                );
            }
            Event::FootnoteReference(label) => {
                push_inline_to_active(
                    &mut block_stack,
                    &mut inline_stack,
                    MdInlineNode::FootnoteReference(label.as_ref().to_owned()),
                );
            }
            Event::SoftBreak => {
                push_inline_to_active(
                    &mut block_stack,
                    &mut inline_stack,
                    MdInlineNode::SoftBreak,
                );
            }
            Event::HardBreak => {
                push_inline_to_active(
                    &mut block_stack,
                    &mut inline_stack,
                    MdInlineNode::HardBreak,
                );
            }
            Event::Rule => {
                flush_list_item_loose_inlines(&mut block_stack);
                push_block_to_parent(&mut block_stack, MdBlock::Rule);
            }
            Event::TaskListMarker(done) => {
                for frame in block_stack.iter_mut().rev() {
                    if let BlockFrame::ListItem { checked, .. } = frame {
                        *checked = Some(*done);
                        break;
                    }
                }
            }
        }
    }

    match block_stack.into_iter().next() {
        Some(BlockFrame::Document(blocks)) => blocks,
        _ => Vec::new(),
    }
}

#[cfg(test)]
pub(crate) fn parse_inlines(text: &str) -> Vec<MdInlineNode> {
    let blocks = parse_blocks(text);
    for blk in blocks {
        if let MdBlock::Paragraph(inlines) = blk {
            return inlines;
        }
    }
    Vec::new()
}

// ---------------------------------------------------------------------------
// Single-StyledText Inline Compilation
// ---------------------------------------------------------------------------

#[derive(Clone, Debug)]
pub(crate) struct CompiledInlineText {
    pub(crate) text: String,
    pub(crate) highlights: Vec<(Range<usize>, HighlightStyle)>,
    pub(crate) font_family_overrides: Vec<(Range<usize>, SharedString)>,
    pub(crate) code_ranges: Vec<Range<usize>>,
    pub(crate) link_ranges: Vec<(Range<usize>, String)>,
}

#[derive(Clone, Copy)]
struct InlineCompileContext {
    base_weight: FontWeight,
    code_color: Hsla,
    math_color: Hsla,
    link_color: Hsla,
    muted_color: Hsla,
}

#[derive(Clone, Default)]
struct ActiveInlineStyle {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    math: bool,
    footnote_ref: bool,
    muted_placeholder: bool,
    link: Option<String>,
}

#[cfg(test)]
pub(crate) fn compile_inline_text(
    src: &str,
    base_weight: FontWeight,
    code_color: Hsla,
    link_color: Hsla,
    muted_color: Hsla,
    mono_font: Option<&SharedString>,
    resolve_link: &Option<LinkResolver>,
) -> CompiledInlineText {
    let nodes = parse_inlines(src);
    compile_inline_nodes(
        &nodes,
        base_weight,
        code_color,
        link_color,
        link_color,
        muted_color,
        mono_font,
        resolve_link,
    )
}

#[allow(clippy::too_many_arguments)]
fn compile_inline_nodes(
    nodes: &[MdInlineNode],
    base_weight: FontWeight,
    code_color: Hsla,
    math_color: Hsla,
    link_color: Hsla,
    muted_color: Hsla,
    mono_font: Option<&SharedString>,
    resolve_link: &Option<LinkResolver>,
) -> CompiledInlineText {
    let mut out = CompiledInlineText {
        text: String::new(),
        highlights: Vec::new(),
        font_family_overrides: Vec::new(),
        code_ranges: Vec::new(),
        link_ranges: Vec::new(),
    };
    let ctx = InlineCompileContext {
        base_weight,
        code_color,
        math_color,
        link_color,
        muted_color,
    };
    let style = ActiveInlineStyle::default();
    emit_inline_nodes(nodes, &style, ctx, mono_font, resolve_link, &mut out);
    out
}

fn emit_inline_nodes(
    nodes: &[MdInlineNode],
    active: &ActiveInlineStyle,
    ctx: InlineCompileContext,
    mono_font: Option<&SharedString>,
    resolve_link: &Option<LinkResolver>,
    out: &mut CompiledInlineText,
) {
    for node in nodes {
        match node {
            MdInlineNode::Text(s) => {
                push_compiled_leaf(s, active, ctx, mono_font, out);
            }
            MdInlineNode::SoftBreak => {
                push_compiled_leaf(" ", active, ctx, mono_font, out);
            }
            MdInlineNode::HardBreak => {
                push_compiled_leaf("\n", active, ctx, mono_font, out);
            }
            MdInlineNode::Bold(children) => {
                let mut next = active.clone();
                next.bold = true;
                emit_inline_nodes(children, &next, ctx, mono_font, resolve_link, out);
            }
            MdInlineNode::Italic(children) => {
                let mut next = active.clone();
                next.italic = true;
                emit_inline_nodes(children, &next, ctx, mono_font, resolve_link, out);
            }
            MdInlineNode::Strike(children) => {
                let mut next = active.clone();
                next.strike = true;
                emit_inline_nodes(children, &next, ctx, mono_font, resolve_link, out);
            }
            MdInlineNode::Code(code) => {
                let mut next = active.clone();
                next.code = true;
                push_compiled_leaf(code, &next, ctx, mono_font, out);
            }
            MdInlineNode::InlineMath(tex) | MdInlineNode::DisplayMathInline(tex) => {
                let mut next = active.clone();
                next.math = true;
                push_compiled_leaf(tex, &next, ctx, mono_font, out);
            }
            MdInlineNode::Link { children, href, .. } => {
                let resolved = resolve_safe_url(href, resolve_link);
                let mut next = active.clone();
                next.link = resolved;
                emit_inline_nodes(children, &next, ctx, mono_font, resolve_link, out);
            }
            MdInlineNode::Image { alt, .. } => {
                let caption = if alt.is_empty() {
                    "[Image not shown]".to_owned()
                } else {
                    format!("[Image not shown: {alt}]")
                };
                let mut next = active.clone();
                next.muted_placeholder = true;
                push_compiled_leaf(&caption, &next, ctx, mono_font, out);
            }
            MdInlineNode::FootnoteReference(label) => {
                let mut next = active.clone();
                next.footnote_ref = true;
                push_compiled_leaf(&format!("[{label}]"), &next, ctx, mono_font, out);
            }
        }
    }
}

fn resolve_safe_url(href: &str, resolve_link: &Option<LinkResolver>) -> Option<String> {
    if !is_safe_link_url(href) {
        return None;
    }
    if let Some(resolver) = resolve_link {
        resolver(href).filter(|u| is_safe_link_url(u))
    } else {
        Some(href.to_owned())
    }
}

fn push_compiled_leaf(
    leaf: &str,
    active: &ActiveInlineStyle,
    ctx: InlineCompileContext,
    mono_font: Option<&SharedString>,
    out: &mut CompiledInlineText,
) {
    if leaf.is_empty() {
        return;
    }
    let start = out.text.len();
    out.text.push_str(leaf);
    let end = out.text.len();

    let mut hl = HighlightStyle::default();
    let mut has_hl = false;

    if active.bold {
        hl.font_weight = Some(if ctx.base_weight >= FontWeight::SEMIBOLD {
            FontWeight::BOLD
        } else {
            FontWeight::SEMIBOLD
        });
        has_hl = true;
    }
    if active.italic || active.muted_placeholder || active.math {
        hl.font_style = Some(FontStyle::Italic);
        has_hl = true;
    }
    if active.strike {
        hl.strikethrough = Some(StrikethroughStyle {
            thickness: px(1.),
            color: Some(ctx.muted_color),
        });
        hl.color = Some(ctx.muted_color);
        has_hl = true;
    }
    if active.muted_placeholder {
        hl.color = Some(ctx.muted_color);
        has_hl = true;
    }
    if active.code {
        if hl.color.is_none() {
            hl.color = Some(ctx.code_color);
        }
        has_hl = true;
        out.code_ranges.push(start..end);
        if let Some(mono) = mono_font {
            out.font_family_overrides.push((start..end, mono.clone()));
        }
    }
    if active.math {
        if hl.color.is_none() {
            hl.color = Some(ctx.math_color);
        }
        has_hl = true;
        out.code_ranges.push(start..end);
        if let Some(mono) = mono_font {
            out.font_family_overrides.push((start..end, mono.clone()));
        }
    }
    if active.footnote_ref {
        hl.color = Some(ctx.link_color);
        hl.font_weight = Some(FontWeight::MEDIUM);
        has_hl = true;
    }
    if let Some(ref href) = active.link {
        hl.color = Some(ctx.link_color);
        hl.underline = Some(UnderlineStyle {
            thickness: px(1.),
            color: Some(ctx.link_color),
            wavy: false,
        });
        has_hl = true;
        if let Some((last_range, last_href)) = out.link_ranges.last_mut()
            && last_range.end == start
            && last_href == href
        {
            last_range.end = end;
        } else {
            out.link_ranges.push((start..end, href.clone()));
        }
    }

    if has_hl {
        if let Some((last_range, last_hl)) = out.highlights.last_mut()
            && last_range.end == start
            && *last_hl == hl
        {
            last_range.end = end;
        } else {
            out.highlights.push((start..end, hl));
        }
    }
}

// ---------------------------------------------------------------------------
// Memoized Document + Streaming + Selection State
// ---------------------------------------------------------------------------

#[derive(Clone)]
struct RenderedSegment {
    doc_range: Range<usize>,
    bounds: Bounds<Pixels>,
    layout: TextLayout,
    links: Vec<(Range<usize>, String)>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
enum DragMode {
    #[default]
    Char,
    Word(Range<usize>),
    Line(Range<usize>),
}

struct MarkdownState {
    // Memoized parse cache (prevents per-frame re-parsing on hover, scroll, selection, resize).
    cached_source: Option<SharedString>,
    cached_streaming: bool,
    cached_math_enabled: bool,
    cached_blocks: Arc<Vec<MdBlock>>,
    cached_pending_component: Option<String>,
    parse_count: usize,
    // Incremental line-level highlighters keyed by code-block ordinal for streaming code fences.
    stream_highlighters: HashMap<usize, IncrementalSyntaxHighlighter>,
    // Per-code-block soft-wrap toggle state keyed by code-block ordinal.
    wrapped_code_blocks: HashSet<usize>,
    // Hovered link URL for subtle status preview (updated only on hover boundary transitions).
    hovered_link: Option<String>,

    // Selection, focus & context-menu state.
    focus_handle: FocusHandle,
    doc_text: String,
    selected_range: Range<usize>,
    anchor_offset: usize,
    is_selecting: bool,
    drag_mode: DragMode,
    pending_link: Option<(String, Point<Pixels>)>,
    context_menu_position: Option<Point<Pixels>>,
    context_menu_link: Option<String>,
    on_open_link: Option<LinkOpenHandler>,
    rendered_segments: Rc<RefCell<Vec<RenderedSegment>>>,
    viewport_bounds: Rc<Cell<Bounds<Pixels>>>,
    _focus_out: Subscription,
}

impl Focusable for MarkdownState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

#[cfg(test)]
fn test_parse_counts() -> &'static std::sync::Mutex<HashMap<String, usize>> {
    static COUNTS: std::sync::OnceLock<std::sync::Mutex<HashMap<String, usize>>> =
        std::sync::OnceLock::new();
    COUNTS.get_or_init(|| std::sync::Mutex::new(HashMap::new()))
}

impl MarkdownState {
    fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        let weak = cx.entity().downgrade();
        let focus_out = window.on_focus_out(&focus_handle, cx, move |_, window, cx| {
            if let Some(entity) = weak.upgrade() {
                let changed = entity.update(cx, |state, _| state.handle_focus_out());
                if changed {
                    window.refresh();
                }
            }
        });
        Self {
            cached_source: None,
            cached_streaming: false,
            cached_math_enabled: true,
            cached_blocks: Arc::new(Vec::new()),
            cached_pending_component: None,
            parse_count: 0,
            stream_highlighters: HashMap::new(),
            wrapped_code_blocks: HashSet::new(),
            hovered_link: None,
            focus_handle,
            doc_text: String::new(),
            selected_range: 0..0,
            anchor_offset: 0,
            is_selecting: false,
            drag_mode: DragMode::Char,
            pending_link: None,
            context_menu_position: None,
            context_menu_link: None,
            on_open_link: None,
            rendered_segments: Rc::new(RefCell::new(Vec::new())),
            viewport_bounds: Rc::new(Cell::new(Bounds::default())),
            _focus_out: focus_out,
        }
    }

    fn sync_parsed(
        &mut self,
        id_root: &SharedString,
        source: &SharedString,
        streaming: bool,
        math_enabled: bool,
    ) -> (Arc<Vec<MdBlock>>, Option<String>) {
        if self.cached_source.as_ref() == Some(source)
            && self.cached_streaming == streaming
            && self.cached_math_enabled == math_enabled
        {
            return (
                self.cached_blocks.clone(),
                self.cached_pending_component.clone(),
            );
        }
        let split = split_open_component(source.as_ref(), streaming);
        let blocks = Arc::new(parse_blocks_with_options(split.shown, math_enabled));
        self.cached_source = Some(source.clone());
        self.cached_streaming = streaming;
        self.cached_math_enabled = math_enabled;
        self.cached_blocks = blocks.clone();
        self.cached_pending_component = split.pending_component.clone();
        self.parse_count += 1;
        #[cfg(test)]
        {
            *test_parse_counts()
                .lock()
                .unwrap()
                .entry(id_root.to_string())
                .or_insert(0) += 1;
        }
        #[cfg(not(test))]
        let _ = id_root;
        if !streaming {
            self.stream_highlighters.clear();
        }
        (blocks, split.pending_component)
    }

    fn highlight_code_block(
        &mut self,
        code_idx: usize,
        language: &str,
        code: &str,
        streaming: bool,
    ) -> Arc<Vec<HighlightedLine>> {
        if streaming {
            let needs_new = self
                .stream_highlighters
                .get(&code_idx)
                .is_none_or(|h| h.language() != language);
            if needs_new {
                self.stream_highlighters
                    .insert(code_idx, IncrementalSyntaxHighlighter::new(language));
            }
            let hl = self.stream_highlighters.get_mut(&code_idx).unwrap();
            Arc::new(hl.update(code).to_vec())
        } else {
            highlight_code_cached(language, code)
        }
    }

    fn toggle_code_wrap(&mut self, code_idx: usize) {
        if !self.wrapped_code_blocks.remove(&code_idx) {
            self.wrapped_code_blocks.insert(code_idx);
        }
    }

    fn set_hovered_link(&mut self, next: Option<String>) -> bool {
        if self.hovered_link == next {
            return false;
        }
        self.hovered_link = next;
        true
    }

    fn sync_document(&mut self, next_doc: String, on_open_link: Option<LinkOpenHandler>) {
        self.on_open_link = on_open_link;
        if self.doc_text == next_doc {
            return;
        }
        // Preserve active selection while streaming if the selected slice is still identical.
        let keep_selection = !self.selected_range.is_empty()
            && self.selected_range.end <= next_doc.len()
            && next_doc.is_char_boundary(self.selected_range.start)
            && next_doc.is_char_boundary(self.selected_range.end)
            && next_doc[self.selected_range.clone()] == self.doc_text[self.selected_range.clone()];

        if !keep_selection {
            self.selected_range = 0..0;
            self.anchor_offset = 0;
            self.is_selecting = false;
            self.pending_link = None;
            self.context_menu_position = None;
            self.context_menu_link = None;
        }
        self.doc_text = next_doc;
    }

    fn handle_focus_out(&mut self) -> bool {
        // Keep selection intact while the right-click context menu is open so clicking "Copy"
        // copies the highlighted range cleanly.
        if self.context_menu_position.is_some() {
            return false;
        }
        let had_selection = !self.selected_range.is_empty() || self.is_selecting;
        self.selected_range = 0..0;
        self.is_selecting = false;
        self.pending_link = None;
        had_selection
    }

    fn selected_text(&self) -> Option<&str> {
        if self.selected_range.is_empty() || self.selected_range.end > self.doc_text.len() {
            return None;
        }
        self.doc_text.get(self.selected_range.clone())
    }

    fn copy_selection(&self, cx: &mut App) -> bool {
        let Some(slice) = self.selected_text() else {
            return false;
        };
        cx.write_to_clipboard(ClipboardItem::new_string(slice.to_owned()));
        true
    }

    fn select_all(&mut self) -> bool {
        let next = 0..self.doc_text.len();
        if self.selected_range == next {
            return false;
        }
        self.selected_range = next;
        self.anchor_offset = 0;
        true
    }

    fn clear_selection(&mut self) -> bool {
        if self.selected_range.is_empty() {
            return false;
        }
        self.selected_range = 0..0;
        self.anchor_offset = 0;
        true
    }

    fn open_link(&self, href: &str, window: &mut Window, cx: &mut App) {
        if let Some(ref handler) = self.on_open_link {
            handler(href, window, cx);
        } else {
            cx.open_url(href);
        }
    }

    fn link_at_position(&self, position: Point<Pixels>) -> Option<String> {
        let segments = self.rendered_segments.borrow();
        for seg in segments.iter() {
            if !seg.links.is_empty() && seg.bounds.contains(&position) {
                if let Ok(local_idx) = seg.layout.index_for_position(position) {
                    for (range, href) in &seg.links {
                        if range.contains(&local_idx) {
                            return Some(href.clone());
                        }
                    }
                }
            }
        }
        None
    }

    fn hit_test_offset(&self, position: Point<Pixels>) -> Option<usize> {
        let segments = self.rendered_segments.borrow();
        if segments.is_empty() {
            return None;
        }

        // First: check segments whose vertical band contains `position.y`.
        // When multiple segments share a vertical band (e.g. table cells on the same row), pick
        // the one containing `position.x` or horizontally closest to it.
        let mut best_band: Option<(&RenderedSegment, Pixels)> = None;
        for seg in segments.iter() {
            if position.y >= seg.bounds.top() && position.y <= seg.bounds.bottom() {
                let h_dist = if position.x < seg.bounds.left() {
                    seg.bounds.left() - position.x
                } else if position.x > seg.bounds.right() {
                    position.x - seg.bounds.right()
                } else {
                    px(0.)
                };
                if best_band.is_none_or(|(_, d)| h_dist < d) {
                    best_band = Some((seg, h_dist));
                }
            }
        }

        if let Some((seg, _)) = best_band {
            return Some(self.offset_in_segment(seg, position));
        }

        // Otherwise, snap vertically to the nearest segment above/below.
        let mut above: Option<&RenderedSegment> = None;
        let mut below: Option<&RenderedSegment> = None;
        for seg in segments.iter() {
            if seg.bounds.bottom() <= position.y {
                if above.is_none_or(|a| seg.bounds.bottom() >= a.bounds.bottom()) {
                    above = Some(seg);
                }
            } else if seg.bounds.top() >= position.y {
                if below.is_none_or(|b| seg.bounds.top() < b.bounds.top()) {
                    below = Some(seg);
                }
            }
        }

        match (above, below) {
            (None, Some(first)) => Some(first.doc_range.start),
            (Some(last), None) => Some(last.doc_range.end),
            (Some(a), Some(b)) => {
                let mid = (a.bounds.bottom() + b.bounds.top()) * 0.5;
                if position.y < mid {
                    Some(a.doc_range.end)
                } else {
                    Some(b.doc_range.start)
                }
            }
            (None, None) => Some(0),
        }
    }

    fn offset_in_segment(&self, seg: &RenderedSegment, position: Point<Pixels>) -> usize {
        if seg.doc_range.is_empty() {
            return seg.doc_range.start;
        }
        let local = match seg.layout.closest_index_for_position(position) {
            Ok(i) | Err(i) => i,
        };
        let clamped = (seg.doc_range.start + local).min(seg.doc_range.end);
        clamp_char_boundary(&self.doc_text, clamped)
    }

    fn on_left_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        self.context_menu_position = None;
        self.context_menu_link = None;
        window.focus(&self.focus_handle, cx);

        let Some(offset) = self.hit_test_offset(event.position) else {
            return false;
        };

        if event.click_count >= 3 {
            let range = line_range_at(&self.doc_text, offset);
            self.selected_range = range.clone();
            self.anchor_offset = range.start;
            self.drag_mode = DragMode::Line(range);
            self.pending_link = None;
            self.is_selecting = true;
            return true;
        }

        if event.click_count == 2 {
            let range = word_range_at(&self.doc_text, offset);
            self.selected_range = range.clone();
            self.anchor_offset = range.start;
            self.drag_mode = DragMode::Word(range);
            self.pending_link = None;
            self.is_selecting = true;
            return true;
        }

        self.drag_mode = DragMode::Char;
        if event.modifiers.shift {
            let a = self.anchor_offset.min(self.doc_text.len());
            self.selected_range = a.min(offset)..a.max(offset);
            self.pending_link = None;
        } else {
            self.anchor_offset = offset;
            self.selected_range = offset..offset;
            self.pending_link = self
                .link_at_position(event.position)
                .map(|href| (href, event.position));
        }
        self.is_selecting = true;
        true
    }

    fn on_right_mouse_down(
        &mut self,
        event: &MouseDownEvent,
        window: &mut Window,
        cx: &mut App,
    ) -> bool {
        window.focus(&self.focus_handle, cx);
        self.is_selecting = false;
        self.pending_link = None;
        self.context_menu_position = Some(event.position);
        self.context_menu_link = self.link_at_position(event.position);
        true
    }

    fn on_mouse_move(&mut self, event: &MouseMoveEvent) -> bool {
        if !self.is_selecting {
            return false;
        }
        if let Some((_, press_pos)) = &self.pending_link {
            let dx = (event.position.x - press_pos.x).abs();
            let dy = (event.position.y - press_pos.y).abs();
            if dx > px(4.) || dy > px(4.) {
                self.pending_link = None;
            }
        }
        let Some(offset) = self.hit_test_offset(event.position) else {
            return false;
        };
        let next = match &self.drag_mode {
            DragMode::Char => {
                let a = self.anchor_offset.min(self.doc_text.len());
                a.min(offset)..a.max(offset)
            }
            DragMode::Word(anchor_word) => {
                let cur = word_range_at(&self.doc_text, offset);
                anchor_word.start.min(cur.start)..anchor_word.end.max(cur.end)
            }
            DragMode::Line(anchor_line) => {
                let cur = line_range_at(&self.doc_text, offset);
                anchor_line.start.min(cur.start)..anchor_line.end.max(cur.end)
            }
        };
        if !next.is_empty() {
            self.pending_link = None;
        }
        if next != self.selected_range {
            self.selected_range = next;
            return true;
        }
        false
    }

    fn on_mouse_up(&mut self, window: &mut Window, cx: &mut App) -> bool {
        let was_selecting = self.is_selecting;
        self.is_selecting = false;
        if let Some((href, _)) = self.pending_link.take()
            && self.selected_range.is_empty()
        {
            self.open_link(&href, window, cx);
            return true;
        }
        was_selecting
    }

    fn keyboard_context_menu_anchor(&self) -> Point<Pixels> {
        let viewport = self.viewport_bounds.get();
        if !self.selected_range.is_empty() {
            let segments = self.rendered_segments.borrow();
            let sel_start = self.selected_range.start;
            for seg in segments.iter() {
                if sel_start >= seg.doc_range.start && sel_start <= seg.doc_range.end {
                    let local = sel_start - seg.doc_range.start;
                    if let Some(pt) = seg.layout.position_for_index(local) {
                        return point(pt.x, pt.y + seg.layout.line_height());
                    }
                    return point(seg.bounds.left(), seg.bounds.bottom());
                }
            }
        }
        point(viewport.left() + px(12.), viewport.top() + px(12.))
    }

    fn build_context_menu_entries(&self, entity: Entity<Self>) -> Vec<MenuEntry> {
        let has_selection = !self.selected_range.is_empty();
        let has_text = !self.doc_text.is_empty();
        let mut entries = Vec::new();

        if let Some(link_href) = self.context_menu_link.clone() {
            let open_entity = entity.clone();
            let open_href = link_href.clone();
            entries.push(MenuEntry::item_with_icon(
                "Open Link",
                LucideIcon::ExternalLink,
                move |_, window, cx| {
                    let on_open = open_entity.update(cx, |s, _| {
                        s.context_menu_position = None;
                        s.context_menu_link = None;
                        s.on_open_link.clone()
                    });
                    if let Some(handler) = on_open {
                        handler(&open_href, window, cx);
                    } else {
                        cx.open_url(&open_href);
                    }
                    window.refresh();
                },
            ));
            let copy_link_entity = entity.clone();
            entries.push(MenuEntry::item_with_icon(
                "Copy Link",
                LucideIcon::Link,
                move |_, window, cx| {
                    cx.write_to_clipboard(ClipboardItem::new_string(link_href.clone()));
                    copy_link_entity.update(cx, |s, _| {
                        s.context_menu_position = None;
                        s.context_menu_link = None;
                    });
                    window.refresh();
                },
            ));
            entries.push(MenuEntry::Separator);
        }

        let copy_entity = entity.clone();
        entries.push(
            MenuEntry::item_with_icon("Copy", LucideIcon::Copy, move |_, window, cx| {
                let focus_handle = copy_entity.update(cx, |s, cx| {
                    s.copy_selection(cx);
                    s.focus_handle.clone()
                });
                window.focus(&focus_handle, cx);
                copy_entity.update(cx, |s, _| {
                    s.context_menu_position = None;
                    s.context_menu_link = None;
                });
                window.refresh();
            })
            .with_shortcut(shortcut_hint("⌘C", "Ctrl+C"))
            .with_disabled(!has_selection),
        );

        entries.push(
            MenuEntry::item_with_icon(
                "Select All",
                LucideIcon::SquareDashedMousePointer,
                move |_, window, cx| {
                    let focus_handle = entity.update(cx, |s, _| {
                        s.select_all();
                        s.focus_handle.clone()
                    });
                    window.focus(&focus_handle, cx);
                    entity.update(cx, |s, _| {
                        s.context_menu_position = None;
                        s.context_menu_link = None;
                    });
                    window.refresh();
                },
            )
            .with_shortcut(shortcut_hint("⌘A", "Ctrl+A"))
            .with_disabled(!has_text),
        );

        entries
    }
}

fn shortcut_hint(mac: &'static str, other: &'static str) -> &'static str {
    if cfg!(target_os = "macos") {
        mac
    } else {
        other
    }
}

fn word_range_at(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let idx = clamp_char_boundary(text, offset.min(text.len()));
    let probe = if idx < text.len() {
        idx
    } else {
        text[..idx]
            .char_indices()
            .next_back()
            .map(|(i, _)| i)
            .unwrap_or(0)
    };
    let Some(ch) = text[probe..].chars().next() else {
        return idx..idx;
    };
    let classify = |c: char| -> u8 {
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
    let target = classify(ch);
    if target == 0 {
        return probe..probe + ch.len_utf8();
    }
    let mut start = probe;
    for (i, c) in text[..probe].char_indices().rev() {
        if classify(c) == target {
            start = i;
        } else {
            break;
        }
    }
    let after = probe + ch.len_utf8();
    let mut end = after;
    for (rel, c) in text[after..].char_indices() {
        if classify(c) == target {
            end = after + rel + c.len_utf8();
        } else {
            break;
        }
    }
    start..end
}

fn line_range_at(text: &str, offset: usize) -> Range<usize> {
    if text.is_empty() {
        return 0..0;
    }
    let idx = clamp_char_boundary(text, offset.min(text.len()));
    let start = text[..idx].rfind('\n').map(|p| p + 1).unwrap_or(0);
    let end = text[idx..]
        .find('\n')
        .map(|rel| idx + rel)
        .unwrap_or(text.len());
    start..end
}

// ---------------------------------------------------------------------------
// Custom Text & Selection Viewport Elements
// ---------------------------------------------------------------------------

struct MarkdownSelectionViewportElement {
    child: AnyElement,
    rendered_segments: Rc<RefCell<Vec<RenderedSegment>>>,
    viewport_bounds: Rc<Cell<Bounds<Pixels>>>,
    state: Entity<MarkdownState>,
    is_selecting: bool,
}

impl Element for MarkdownSelectionViewportElement {
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
        self.rendered_segments.borrow_mut().clear();

        if self.is_selecting {
            let state_move = self.state.clone();
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture {
                    let changed = state_move.update(cx, |s, _| s.on_mouse_move(event));
                    if changed {
                        window.refresh();
                    }
                }
            });
            let state_up = self.state.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Capture && event.button == MouseButton::Left {
                    let changed = state_up.update(cx, |s, cx| s.on_mouse_up(window, cx));
                    if changed {
                        window.refresh();
                    }
                }
            });
        }

        self.child.paint(window, cx);
    }
}

/// Renders one single-flow [`StyledText`] Markdown block or syntax-highlighted code line with:
/// - compact glyph-metric-aligned rounded inline-code pills (zero injected spaces),
/// - multi-line wrapped & newline-aware selection highlight quads,
/// - dynamic `IBeam` / `PointingHand` cursor switching & URL preview updates over links,
/// - segment hitbox registration for cross-block pointer hit-testing.
struct MarkdownTextElement {
    text: SharedString,
    styled_text: StyledText,
    doc_range: Range<usize>,
    ends_with_newline: bool,
    code_ranges: Vec<Range<usize>>,
    links: Vec<(Range<usize>, String)>,
    code_pill_bg: Hsla,
    code_pill_border: Hsla,
    selection: Range<usize>,
    selection_color: Hsla,
    selectable: bool,
    is_selecting: bool,
    on_open_link: Option<LinkOpenHandler>,
    state: Entity<MarkdownState>,
    rendered_segments: Option<Rc<RefCell<Vec<RenderedSegment>>>>,
    viewport_bounds: Option<Rc<Cell<Bounds<Pixels>>>>,
}

impl Element for MarkdownTextElement {
    type RequestLayoutState = ();
    type PrepaintState = Hitbox;

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
    ) -> Hitbox {
        self.styled_text
            .prepaint(None, inspector_id, bounds, &mut (), window, cx);
        window.insert_hitbox(bounds, HitboxBehavior::Normal)
    }

    fn paint(
        &mut self,
        _id: Option<&GlobalElementId>,
        _inspector_id: Option<&InspectorElementId>,
        bounds: Bounds<Pixels>,
        _request_layout: &mut (),
        hitbox: &mut Hitbox,
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

        if let Some(ref segments) = self.rendered_segments {
            segments.borrow_mut().push(RenderedSegment {
                doc_range: self.doc_range.clone(),
                bounds: hit_bounds,
                layout: layout.clone(),
                links: self.links.clone(),
            });
        }

        // Dynamic cursor: `PointingHand` directly over link glyphs, `IBeam` over selectable text.
        let hovered_href_now: Option<String> = if !self.links.is_empty() && hitbox.is_hovered(window)
        {
            layout
                .index_for_position(window.mouse_position())
                .ok()
                .and_then(|ix| {
                    self.links
                        .iter()
                        .find(|(r, _)| r.contains(&ix))
                        .map(|(_, href)| href.clone())
                })
        } else {
            None
        };

        if hovered_href_now.is_some() {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        } else if self.selectable {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }

        // Refresh only on link hover boundary transitions so cursor & URL preview update
        // immediately without causing per-pixel repaints during normal mouse movement.
        if !self.links.is_empty() && !self.is_selecting {
            let links_for_hover = self.links.clone();
            let layout_for_hover = layout.clone();
            let hitbox_for_hover = hitbox.clone();
            let state_for_hover = self.state.clone();
            let prev_href = RefCell::new(hovered_href_now);
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble {
                    let next_href = if hitbox_for_hover.is_hovered(window) {
                        layout_for_hover
                            .index_for_position(event.position)
                            .ok()
                            .and_then(|ix| {
                                links_for_hover
                                    .iter()
                                    .find(|(r, _)| r.contains(&ix))
                                    .map(|(_, href)| href.clone())
                            })
                    } else {
                        None
                    };
                    if *prev_href.borrow() != next_href {
                        *prev_href.borrow_mut() = next_href.clone();
                        state_for_hover.update(cx, |s, _| {
                            s.set_hovered_link(next_href);
                        });
                        window.refresh();
                    }
                }
            });
        }

        // When non-selectable, still allow clicking links directly.
        if !self.selectable && !self.links.is_empty() {
            let links_click = self.links.clone();
            let layout_click = layout.clone();
            let hitbox_click = hitbox.clone();
            let on_open = self.on_open_link.clone();
            window.on_mouse_event(move |event: &MouseUpEvent, phase, window, cx| {
                if phase == DispatchPhase::Bubble
                    && event.button == MouseButton::Left
                    && hitbox_click.is_hovered(window)
                    && let Ok(ix) = layout_click.index_for_position(event.position)
                    && let Some((_, href)) = links_click.iter().find(|(r, _)| r.contains(&ix))
                {
                    if let Some(ref handler) = on_open {
                        handler(href, window, cx);
                    } else {
                        cx.open_url(href);
                    }
                    cx.stop_propagation();
                }
            });
        }

        // Paint order: run backgrounds -> compact rounded inline-code pills -> selection -> glyphs.
        let _ = layout.paint_background(window, cx);
        if !self.code_ranges.is_empty() {
            paint_inline_code_pills(
                self.text.as_ref(),
                &self.code_ranges,
                &layout,
                self.code_pill_bg,
                self.code_pill_border,
                window,
            );
        }
        if self.selectable && !self.selection.is_empty() {
            let max_right = self
                .viewport_bounds
                .as_ref()
                .map(|vb| vb.get().right())
                .unwrap_or(bounds.right())
                .max(bounds.right());
            paint_segment_selection(
                &self.doc_range,
                self.ends_with_newline,
                &self.selection,
                self.selection_color,
                &layout,
                max_right,
                window,
            );
        }
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

/// Computes compact, glyph-centered vertical pill bounds `(top, bottom)` inside a single line
/// `[y, y + line_height]`, leaving clean vertical whitespace between wrapped lines.
pub(crate) fn compute_inline_code_pill_vertical_bounds(
    y: Pixels,
    line_height: Pixels,
    ascent: Pixels,
    descent: Pixels,
) -> (Pixels, Pixels) {
    let glyph_h = (ascent + descent).max(px(0.));
    let padding_top = ((line_height - glyph_h) * 0.5).max(px(0.));
    let min_v_inset = (line_height * 0.09).clamp(px(1.), px(2.2));
    let pill_v_pad = px(1.0);
    let mut top = (y + padding_top - pill_v_pad).max(y + min_v_inset);
    let mut bottom = (y + padding_top + glyph_h + pill_v_pad).min(y + line_height - min_v_inset);
    if bottom <= top + px(4.) {
        top = y + px(1.);
        bottom = (y + line_height - px(1.)).max(top + px(1.));
    }
    (top, bottom)
}

fn paint_inline_code_pills(
    text: &str,
    code_ranges: &[Range<usize>],
    layout: &TextLayout,
    bg: Hsla,
    border: Hsla,
    window: &mut Window,
) {
    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let align = window.text_style().text_align;
    let line_layouts = layout.line_layouts();
    let mut line_byte_offset = 0usize;
    let mut line_y = bounds.top();

    for wrapped in line_layouts.iter() {
        let unwrapped = &wrapped.unwrapped_layout;
        let line_len = unwrapped.len;
        let line_end_byte = line_byte_offset + line_len;
        let wrap_count = wrapped.wrap_boundaries.len();

        let line_has_code = code_ranges
            .iter()
            .any(|r| r.start < line_end_byte && r.end > line_byte_offset);

        if line_has_code {
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
                    (line_len, unwrapped.width)
                };

                let sub_doc_start = line_byte_offset + sub_start_ix;
                let sub_doc_end = line_byte_offset + sub_end_ix;
                let sub_width = (sub_end_x - sub_start_x).max(px(0.));
                let sub_origin_x = aligned_subline_origin_x(bounds, sub_width, align);
                let y = line_y + line_height * sub_idx as f32;
                let (pill_top, pill_bottom) = compute_inline_code_pill_vertical_bounds(
                    y,
                    line_height,
                    unwrapped.ascent,
                    unwrapped.descent,
                );

                for code_range in code_ranges {
                    if code_range.end <= sub_doc_start || code_range.start >= sub_doc_end {
                        continue;
                    }
                    let s = code_range.start.max(sub_doc_start) - line_byte_offset;
                    let e = code_range.end.min(sub_doc_end) - line_byte_offset;
                    if s >= e {
                        continue;
                    }

                    let raw_x1 = sub_origin_x + (unwrapped.x_for_index(s) - sub_start_x);
                    let raw_x2 = if e == sub_end_ix {
                        sub_origin_x + sub_width
                    } else {
                        sub_origin_x + (unwrapped.x_for_index(e) - sub_start_x)
                    };

                    let is_code_start = line_byte_offset + s == code_range.start;
                    let is_code_end = line_byte_offset + e == code_range.end;

                    let left_outset = if !is_code_start {
                        px(1.5)
                    } else if s == sub_start_ix {
                        px(2.0)
                    } else if text[..code_range.start]
                        .chars()
                        .next_back()
                        .is_none_or(char::is_whitespace)
                    {
                        px(1.75)
                    } else {
                        px(0.75)
                    };

                    let right_outset = if !is_code_end {
                        px(1.5)
                    } else if e == sub_end_ix
                        || text[code_range.end..]
                            .chars()
                            .next()
                            .is_none_or(char::is_whitespace)
                    {
                        px(1.75)
                    } else {
                        px(0.75)
                    };

                    let x1 = raw_x1 - left_outset;
                    let x2 = raw_x2 + right_outset;
                    if x2 <= x1 {
                        continue;
                    }

                    let r_left = if is_code_start { px(4.) } else { px(2.) };
                    let r_right = if is_code_end { px(4.) } else { px(2.) };
                    let radii = Corners {
                        top_left: r_left,
                        bottom_left: r_left,
                        top_right: r_right,
                        bottom_right: r_right,
                    };

                    window.paint_quad(quad(
                        Bounds::from_corners(point(x1, pill_top), point(x2, pill_bottom)),
                        radii,
                        bg,
                        Edges::all(px(1.)),
                        border,
                        BorderStyle::Solid,
                    ));
                }
            }
        }

        line_y += wrapped.size(line_height).height;
        line_byte_offset = line_end_byte + 1;
    }
}

fn paint_segment_selection(
    doc_range: &Range<usize>,
    ends_with_newline: bool,
    selection: &Range<usize>,
    color: Hsla,
    layout: &TextLayout,
    max_right: Pixels,
    window: &mut Window,
) {
    if selection.is_empty() {
        return;
    }
    let seg_start = doc_range.start;
    let seg_end = doc_range.end;
    let local_start = selection.start.clamp(seg_start, seg_end) - seg_start;
    let local_end = selection.end.clamp(seg_start, seg_end) - seg_start;
    let includes_trailing_nl =
        ends_with_newline && selection.start <= seg_end && selection.end > seg_end;

    if local_start == local_end && !includes_trailing_nl {
        return;
    }

    let bounds = layout.bounds();
    let line_height = layout.line_height();
    let newline_stub = px(6.);

    if doc_range.is_empty() {
        if includes_trailing_nl {
            let x2 = (bounds.left() + newline_stub).min(max_right.max(bounds.left() + newline_stub));
            window.paint_quad(fill(
                Bounds::from_corners(bounds.origin, point(x2, bounds.top() + line_height)),
                color,
            ));
        }
        return;
    }

    let line_layouts = layout.line_layouts();
    let line_count = line_layouts.len();
    let align = window.text_style().text_align;
    let mut line_byte_offset = 0usize;
    let mut line_y = bounds.top();

    for (line_idx, wrapped) in line_layouts.iter().enumerate() {
        let unwrapped = &wrapped.unwrapped_layout;
        let line_len = unwrapped.len;
        let line_end = line_byte_offset + line_len;
        let is_last_line = line_idx + 1 == line_count;
        let line_has_nl = if is_last_line {
            includes_trailing_nl && local_start <= line_end
        } else {
            local_start <= line_end && local_end > line_end
        };

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
                (line_len, unwrapped.width)
            };

            let sub_local_start = line_byte_offset + sub_start_ix;
            let sub_local_end = line_byte_offset + sub_end_ix;
            let s = local_start.max(sub_local_start);
            let e = local_end.min(sub_local_end);
            let has_span = s < e;
            let is_last_sub = sub_idx == wrap_count;
            let has_nl = is_last_sub && line_has_nl;

            if !has_span && !has_nl {
                continue;
            }

            let sub_width = (sub_end_x - sub_start_x).max(px(0.));
            let sub_origin_x = aligned_subline_origin_x(bounds, sub_width, align);
            let y = line_y + line_height * sub_idx as f32;

            let (x1, mut x2) = if has_span {
                let s_in_line = s - line_byte_offset;
                let e_in_line = e - line_byte_offset;
                let x1 = sub_origin_x + (unwrapped.x_for_index(s_in_line) - sub_start_x);
                let x2 = if e_in_line == sub_end_ix {
                    sub_origin_x + sub_width
                } else {
                    sub_origin_x + (unwrapped.x_for_index(e_in_line) - sub_start_x)
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

        line_y += wrapped.size(line_height).height;
        line_byte_offset = line_end + 1;
    }
}

// ---------------------------------------------------------------------------
// Recursive Document Preparation & Rendering
// ---------------------------------------------------------------------------

enum PreparedBlock {
    Heading {
        level: u8,
        compiled: CompiledInlineText,
        doc_range: Range<usize>,
    },
    Paragraph {
        compiled: CompiledInlineText,
        doc_range: Range<usize>,
    },
    Blockquote {
        kind: Option<MdAlertKind>,
        blocks: Vec<PreparedBlock>,
    },
    List {
        start: Option<u64>,
        items: Vec<PreparedListItem>,
    },
    Table {
        alignments: Vec<TableAlign>,
        headers: Vec<(CompiledInlineText, Range<usize>)>,
        rows: Vec<Vec<(CompiledInlineText, Range<usize>)>>,
    },
    CodeBlock {
        code_idx: usize,
        language: String,
        raw_code: String,
        lines: Vec<(HighlightedLine, Range<usize>)>,
    },
    ComponentBlock {
        name: String,
        json_body: String,
    },
    DisplayMath {
        tex: String,
        doc_range: Range<usize>,
    },
    ImageBlock {
        alt: String,
        url: String,
        placeholder: CompiledInlineText,
        doc_range: Range<usize>,
    },
    FootnoteDefinition {
        label: String,
        blocks: Vec<PreparedBlock>,
    },
    Rule,
}

struct PreparedListItem {
    checked: Option<bool>,
    blocks: Vec<PreparedBlock>,
}

fn push_sep(doc: &mut String, sep: &str) {
    if !doc.is_empty() {
        doc.push_str(sep);
    }
}

fn append_doc_slice(doc: &mut String, slice: &str) -> Range<usize> {
    let start = doc.len();
    doc.push_str(slice);
    start..doc.len()
}

struct PrepareEnv<'a> {
    theme: &'a Theme,
    syntax: &'a SyntaxTheme,
    resolve_link: &'a Option<LinkResolver>,
    streaming: bool,
    state: &'a mut MarkdownState,
    code_counter: usize,
}

impl PrepareEnv<'_> {
    fn compile_inlines(&self, nodes: &[MdInlineNode], base_weight: FontWeight) -> CompiledInlineText {
        compile_inline_nodes(
            nodes,
            base_weight,
            self.theme.foreground,
            self.syntax.function,
            self.theme.primary,
            self.theme.muted_foreground,
            self.theme.mono_font_family.as_ref(),
            self.resolve_link,
        )
    }

    fn prepare_blocks(
        &mut self,
        blocks: &[MdBlock],
        doc_text: &mut String,
        sep: &str,
    ) -> Vec<PreparedBlock> {
        let mut out = Vec::with_capacity(blocks.len());
        for block in blocks {
            match block {
                MdBlock::Heading { level, inlines } => {
                    push_sep(doc_text, sep);
                    let compiled = self.compile_inlines(inlines, FontWeight::SEMIBOLD);
                    let doc_range = append_doc_slice(doc_text, &compiled.text);
                    out.push(PreparedBlock::Heading {
                        level: *level,
                        compiled,
                        doc_range,
                    });
                }
                MdBlock::Paragraph(inlines) => {
                    push_sep(doc_text, sep);
                    let compiled = self.compile_inlines(inlines, FontWeight::NORMAL);
                    let doc_range = append_doc_slice(doc_text, &compiled.text);
                    out.push(PreparedBlock::Paragraph {
                        compiled,
                        doc_range,
                    });
                }
                MdBlock::Blockquote { kind, blocks: inner } => {
                    let prepared_inner = self.prepare_blocks(inner, doc_text, "\n\n");
                    out.push(PreparedBlock::Blockquote {
                        kind: *kind,
                        blocks: prepared_inner,
                    });
                }
                MdBlock::List { start, items } => {
                    let mut prep_items = Vec::with_capacity(items.len());
                    for (idx, item) in items.iter().enumerate() {
                        let item_sep = if idx == 0 { sep } else { "\n" };
                        let mut item_blocks = Vec::with_capacity(item.blocks.len());
                        for (b_idx, blk) in item.blocks.iter().enumerate() {
                            let s = if b_idx == 0 { item_sep } else { "\n" };
                            let mut single =
                                self.prepare_blocks(std::slice::from_ref(blk), doc_text, s);
                            item_blocks.append(&mut single);
                        }
                        prep_items.push(PreparedListItem {
                            checked: item.checked,
                            blocks: item_blocks,
                        });
                    }
                    out.push(PreparedBlock::List {
                        start: *start,
                        items: prep_items,
                    });
                }
                MdBlock::Table {
                    alignments,
                    headers,
                    rows,
                } => {
                    push_sep(doc_text, sep);
                    let mut prep_headers = Vec::with_capacity(headers.len());
                    for (c_idx, cell) in headers.iter().enumerate() {
                        if c_idx > 0 {
                            doc_text.push('\t');
                        }
                        let compiled = self.compile_inlines(cell, FontWeight::SEMIBOLD);
                        let doc_range = append_doc_slice(doc_text, &compiled.text);
                        prep_headers.push((compiled, doc_range));
                    }
                    let mut prep_rows = Vec::with_capacity(rows.len());
                    for row in rows {
                        if !prep_headers.is_empty() || !prep_rows.is_empty() {
                            doc_text.push('\n');
                        }
                        let mut prep_cells = Vec::with_capacity(row.len());
                        for (c_idx, cell) in row.iter().enumerate() {
                            if c_idx > 0 {
                                doc_text.push('\t');
                            }
                            let compiled = self.compile_inlines(cell, FontWeight::NORMAL);
                            let doc_range = append_doc_slice(doc_text, &compiled.text);
                            prep_cells.push((compiled, doc_range));
                        }
                        prep_rows.push(prep_cells);
                    }
                    out.push(PreparedBlock::Table {
                        alignments: alignments.clone(),
                        headers: prep_headers,
                        rows: prep_rows,
                    });
                }
                MdBlock::CodeBlock { language, code } => {
                    push_sep(doc_text, sep);
                    let code_idx = self.code_counter;
                    self.code_counter += 1;
                    let highlighted =
                        self.state
                            .highlight_code_block(code_idx, language, code, self.streaming);
                    let mut prep_lines = Vec::with_capacity(highlighted.len());
                    for (ln, line) in highlighted.iter().enumerate() {
                        if ln > 0 {
                            doc_text.push('\n');
                        }
                        let doc_range = append_doc_slice(doc_text, line.text.as_ref());
                        prep_lines.push((line.clone(), doc_range));
                    }
                    out.push(PreparedBlock::CodeBlock {
                        code_idx,
                        language: language.clone(),
                        raw_code: code.clone(),
                        lines: prep_lines,
                    });
                }
                MdBlock::ComponentBlock { name, json_body } => {
                    out.push(PreparedBlock::ComponentBlock {
                        name: name.clone(),
                        json_body: json_body.clone(),
                    });
                }
                MdBlock::DisplayMath(tex) => {
                    push_sep(doc_text, sep);
                    let doc_range = append_doc_slice(doc_text, tex);
                    out.push(PreparedBlock::DisplayMath {
                        tex: tex.clone(),
                        doc_range,
                    });
                }
                MdBlock::ImageBlock { alt, url } => {
                    push_sep(doc_text, sep);
                    let placeholder_nodes = vec![MdInlineNode::Image {
                        alt: alt.clone(),
                        url: url.clone(),
                        title: String::new(),
                    }];
                    let placeholder = self.compile_inlines(&placeholder_nodes, FontWeight::NORMAL);
                    let doc_range = append_doc_slice(doc_text, &placeholder.text);
                    out.push(PreparedBlock::ImageBlock {
                        alt: alt.clone(),
                        url: url.clone(),
                        placeholder,
                        doc_range,
                    });
                }
                MdBlock::FootnoteDefinition {
                    label,
                    blocks: inner,
                } => {
                    let prepared_inner = self.prepare_blocks(inner, doc_text, "\n");
                    out.push(PreparedBlock::FootnoteDefinition {
                        label: label.clone(),
                        blocks: prepared_inner,
                    });
                }
                MdBlock::Rule => {
                    out.push(PreparedBlock::Rule);
                }
            }
        }
        out
    }
}

fn build_styled_text(compiled: &CompiledInlineText) -> (SharedString, StyledText) {
    let shared = SharedString::from(compiled.text.clone());
    let mut st = StyledText::new(shared.clone());
    if !compiled.highlights.is_empty() {
        st = st.with_highlights(compiled.highlights.clone());
    }
    if !compiled.font_family_overrides.is_empty() {
        st = st.with_font_family_overrides(compiled.font_family_overrides.clone());
    }
    (shared, st)
}

struct RenderEnv<'a> {
    id_root: &'a SharedString,
    theme: &'a Theme,
    syntax: &'a SyntaxTheme,
    show_line_numbers: bool,
    code_pill_bg: Hsla,
    code_pill_border: Hsla,
    selection: Range<usize>,
    selection_color: Hsla,
    selectable: bool,
    is_selecting: bool,
    wrapped_code_blocks: HashSet<usize>,
    state: Entity<MarkdownState>,
    registry: Option<AnswerComponentRegistry>,
    on_open_link: Option<LinkOpenHandler>,
    on_image: Option<MarkdownImageRenderer>,
    on_math: Option<MarkdownMathRenderer>,
    rendered_segments: Option<Rc<RefCell<Vec<RenderedSegment>>>>,
    viewport_bounds: Option<Rc<Cell<Bounds<Pixels>>>>,
}

impl RenderEnv<'_> {
    fn inline_element(
        &self,
        compiled: CompiledInlineText,
        doc_range: Range<usize>,
        ends_with_newline: bool,
    ) -> MarkdownTextElement {
        let (text, styled_text) = build_styled_text(&compiled);
        MarkdownTextElement {
            text,
            styled_text,
            doc_range,
            ends_with_newline,
            code_ranges: compiled.code_ranges,
            links: compiled.link_ranges,
            code_pill_bg: self.code_pill_bg,
            code_pill_border: self.code_pill_border,
            selection: self.selection.clone(),
            selection_color: self.selection_color,
            selectable: self.selectable,
            is_selecting: self.is_selecting,
            on_open_link: self.on_open_link.clone(),
            state: self.state.clone(),
            rendered_segments: self.rendered_segments.clone(),
            viewport_bounds: self.viewport_bounds.clone(),
        }
    }

    fn highlighted_code_line(
        &self,
        line: &HighlightedLine,
        doc_range: Range<usize>,
        ends_with_newline: bool,
    ) -> MarkdownTextElement {
        let text = line.text.clone();
        let highlights = line.to_highlights(self.syntax);
        let mut styled_text = StyledText::new(text.clone());
        if !highlights.is_empty() {
            styled_text = styled_text.with_highlights(highlights);
        }
        MarkdownTextElement {
            text,
            styled_text,
            doc_range,
            ends_with_newline,
            code_ranges: Vec::new(),
            links: Vec::new(),
            code_pill_bg: self.code_pill_bg,
            code_pill_border: self.code_pill_border,
            selection: self.selection.clone(),
            selection_color: self.selection_color,
            selectable: self.selectable,
            is_selecting: self.is_selecting,
            on_open_link: None,
            state: self.state.clone(),
            rendered_segments: self.rendered_segments.clone(),
            viewport_bounds: self.viewport_bounds.clone(),
        }
    }

    fn render_blocks(
        &self,
        blocks: Vec<PreparedBlock>,
        prefix: &str,
        list_depth: usize,
        has_following_sibling: bool,
        gap_px: Pixels,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let mut col = div().w_full().flex().flex_col().gap(gap_px);
        let count = blocks.len();
        for (idx, block) in blocks.into_iter().enumerate() {
            let block_id = format!("{prefix}-b{idx}");
            let has_following = idx + 1 < count || has_following_sibling;
            col = col.child(self.render_block(
                block,
                &block_id,
                list_depth,
                has_following,
                window,
                cx,
            ));
        }
        col.into_any_element()
    }

    fn render_block(
        &self,
        block: PreparedBlock,
        block_id: &str,
        list_depth: usize,
        has_following: bool,
        window: &mut Window,
        cx: &mut App,
    ) -> AnyElement {
        let theme = self.theme;
        match block {
            PreparedBlock::Heading {
                level,
                compiled,
                doc_range,
            } => {
                let (sz, lh, top_pad) = match level {
                    1 => (px(19.), px(26.), px(4.)),
                    2 => (px(16.5), px(23.5), px(4.)),
                    3 => (px(14.5), px(21.), px(2.)),
                    4 => (px(13.5), px(20.), px(2.)),
                    _ => (px(13.), px(19.), px(1.)),
                };
                let mut el = div()
                    .w_full()
                    .pt(top_pad)
                    .text_size(sz)
                    .line_height(lh)
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(if level >= 5 {
                        theme.muted_foreground
                    } else {
                        theme.foreground
                    });
                if level <= 2 {
                    el = el
                        .pb(px(4.))
                        .border_b_1()
                        .border_color(theme.border.opacity(0.75));
                }
                el.child(self.inline_element(compiled, doc_range, has_following))
                    .into_any_element()
            }
            PreparedBlock::Paragraph {
                compiled,
                doc_range,
            } => div()
                .w_full()
                .text_size(px(13.5))
                .line_height(px(21.))
                .text_color(theme.foreground)
                .child(self.inline_element(compiled, doc_range, has_following))
                .into_any_element(),
            PreparedBlock::Rule => div()
                .w_full()
                .h(px(1.))
                .my(px(4.))
                .bg(theme.border)
                .into_any_element(),
            PreparedBlock::Blockquote { kind, blocks } => {
                if let Some(alert_kind) = kind {
                    let (icon, accent, soft_bg, title_fg) = alert_kind.style_tokens(theme);
                    let body = self.render_blocks(
                        blocks,
                        block_id,
                        list_depth,
                        has_following,
                        px(6.),
                        window,
                        cx,
                    );
                    div()
                        .w_full()
                        .rounded(theme.radius_md)
                        .bg(soft_bg)
                        .border_1()
                        .border_color(accent.opacity(0.24))
                        .border_l(px(3.))
                        .px(px(12.))
                        .py(px(9.))
                        .flex()
                        .flex_col()
                        .gap(px(5.))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(6.))
                                .child(Icon::new(icon).size(IconSize::Sm).color(accent))
                                .child(
                                    div()
                                        .text_size(px(12.5))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(title_fg)
                                        .child(alert_kind.title()),
                                ),
                        )
                        .child(body)
                        .into_any_element()
                } else {
                    let body = self.render_blocks(
                        blocks,
                        block_id,
                        list_depth,
                        has_following,
                        px(6.),
                        window,
                        cx,
                    );
                    div()
                        .w_full()
                        .pl(px(12.))
                        .py(px(3.))
                        .border_l(px(3.))
                        .border_color(theme.primary.opacity(0.55))
                        .text_size(px(13.))
                        .line_height(px(20.))
                        .text_color(theme.muted_foreground)
                        .child(body)
                        .into_any_element()
                }
            }
            PreparedBlock::List { start, items } => {
                let item_count = items.len();
                let mut list_col = div()
                    .w_full()
                    .flex()
                    .flex_col()
                    .gap(px(5.))
                    .pl(if list_depth == 0 { px(4.) } else { px(2.) });

                for (item_i, item) in items.into_iter().enumerate() {
                    let marker = if let Some(done) = item.checked {
                        div()
                            .pt(px(3.))
                            .child(
                                Icon::new(if done {
                                    LucideIcon::SquareCheck
                                } else {
                                    LucideIcon::Square
                                })
                                .size(IconSize::Xs)
                                .color(if done {
                                    theme.primary
                                } else {
                                    theme.muted_foreground
                                }),
                            )
                            .into_any_element()
                    } else if let Some(start_num) = start {
                        div()
                            .min_w(px(18.))
                            .text_right()
                            .font_features(tabular_numerals())
                            .text_size(px(12.5))
                            .line_height(px(21.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.muted_foreground)
                            .child(format!("{}.", start_num + item_i as u64))
                            .into_any_element()
                    } else {
                        match list_depth % 3 {
                            0 => div()
                                .w(px(5.))
                                .h(px(5.))
                                .mt(px(8.))
                                .rounded_full()
                                .bg(theme.muted_foreground)
                                .into_any_element(),
                            1 => div()
                                .w(px(5.5))
                                .h(px(5.5))
                                .mt(px(7.5))
                                .rounded_full()
                                .border_1()
                                .border_color(theme.muted_foreground)
                                .into_any_element(),
                            _ => div()
                                .w(px(4.5))
                                .h(px(4.5))
                                .mt(px(8.5))
                                .rounded(px(1.))
                                .bg(theme.muted_foreground.opacity(0.85))
                                .into_any_element(),
                        }
                    };

                    let item_nl = item_i + 1 < item_count || has_following;
                    let item_prefix = format!("{block_id}-li{item_i}");
                    let content = self.render_blocks(
                        item.blocks,
                        &item_prefix,
                        list_depth + 1,
                        item_nl,
                        px(6.),
                        window,
                        cx,
                    );

                    list_col = list_col.child(
                        div()
                            .w_full()
                            .flex()
                            .flex_row()
                            .items_start()
                            .gap(px(8.))
                            .child(div().flex_shrink_0().child(marker))
                            .child(div().flex_1().min_w_0().child(content)),
                    );
                }
                list_col.into_any_element()
            }
            PreparedBlock::Table {
                alignments,
                headers,
                rows,
            } => self.render_table(block_id, alignments, headers, rows, has_following),
            PreparedBlock::CodeBlock {
                code_idx,
                language,
                raw_code,
                lines,
            } => self.render_code_block(block_id, code_idx, language, raw_code, lines, has_following),
            PreparedBlock::DisplayMath { tex, doc_range } => {
                if let Some(ref hook) = self.on_math
                    && let Some(custom) = hook(&tex, true, window, cx)
                {
                    return custom;
                }
                let text = SharedString::from(tex);
                let st = StyledText::new(text.clone());
                let math_el = MarkdownTextElement {
                    text,
                    styled_text: st,
                    doc_range,
                    ends_with_newline: has_following,
                    code_ranges: Vec::new(),
                    links: Vec::new(),
                    code_pill_bg: self.code_pill_bg,
                    code_pill_border: self.code_pill_border,
                    selection: self.selection.clone(),
                    selection_color: self.selection_color,
                    selectable: self.selectable,
                    is_selecting: self.is_selecting,
                    on_open_link: None,
                    state: self.state.clone(),
                    rendered_segments: self.rendered_segments.clone(),
                    viewport_bounds: self.viewport_bounds.clone(),
                };
                div()
                    .id(SharedString::from(format!("{block_id}-math")))
                    .w_full()
                    .px(px(14.))
                    .py(px(10.))
                    .rounded(theme.radius_md)
                    .bg(theme.surface_muted.opacity(0.55))
                    .border_1()
                    .border_color(theme.border)
                    .overflow_x_scroll()
                    .text_center()
                    .text_size(px(13.5))
                    .line_height(px(21.))
                    .italic()
                    .text_color(theme.foreground)
                    .when_some(theme.mono_font_family.clone(), |el, f| el.font_family(f))
                    .child(math_el)
                    .into_any_element()
            }
            PreparedBlock::ImageBlock {
                alt,
                url,
                placeholder,
                doc_range,
            } => {
                if let Some(ref hook) = self.on_image
                    && let Some(custom) = hook(&url, &alt, window, cx)
                {
                    return custom;
                }
                div()
                    .w_full()
                    .text_size(px(13.))
                    .line_height(px(20.))
                    .text_color(theme.muted_foreground)
                    .child(self.inline_element(placeholder, doc_range, has_following))
                    .into_any_element()
            }
            PreparedBlock::FootnoteDefinition { label, blocks } => {
                let body = self.render_blocks(
                    blocks,
                    block_id,
                    list_depth,
                    has_following,
                    px(4.),
                    window,
                    cx,
                );
                div()
                    .w_full()
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(theme.border.opacity(0.7))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex_shrink_0()
                            .text_size(px(11.5))
                            .line_height(px(21.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.primary)
                            .child(format!("[{label}]")),
                    )
                    .child(div().flex_1().min_w_0().child(body))
                    .into_any_element()
            }
            PreparedBlock::ComponentBlock { name, json_body } => {
                let rendered = match self.registry.as_ref().and_then(|r| r.get(&name)) {
                    None => render_component_fallback(
                        "This part of the answer cannot be shown.",
                        &name,
                        &json_body,
                        cx,
                    ),
                    Some(spec) => match serde_json::from_str::<Value>(&json_body) {
                        Ok(val) => match (spec.render)(&val, window, cx) {
                            Ok(el) => el,
                            Err(_) => render_component_fallback(
                                "This part of the answer could not be drawn.",
                                &name,
                                &json_body,
                                cx,
                            ),
                        },
                        Err(_) => render_component_fallback(
                            "This part of the answer could not be drawn.",
                            &name,
                            &json_body,
                            cx,
                        ),
                    },
                };
                div()
                    .w_full()
                    .cursor_default()
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .child(rendered)
                    .into_any_element()
            }
        }
    }

    fn render_table(
        &self,
        block_id: &str,
        alignments: Vec<TableAlign>,
        headers: Vec<(CompiledInlineText, Range<usize>)>,
        rows: Vec<Vec<(CompiledInlineText, Range<usize>)>>,
        has_following: bool,
    ) -> AnyElement {
        let theme = self.theme;
        let col_count = headers
            .len()
            .max(rows.iter().map(Vec::len).max().unwrap_or(0))
            .max(1);
        let wide_scroll = col_count >= 4;
        let min_col_w = if wide_scroll { px(96.) } else { px(0.) };

        let apply_align = |el: crate::Div, align: TableAlign| match align {
            TableAlign::Left => el.text_left(),
            TableAlign::Center => el.text_center(),
            TableAlign::Right => el.text_right(),
        };

        let mut table_col = div().flex().flex_col().w_full();
        if wide_scroll {
            table_col = table_col.min_w(px(col_count as f32 * 108.));
        }

        // Header row with rich inline formatting & selection support.
        if !headers.is_empty() {
            let header_count = headers.len();
            let mut h_row = div()
                .flex()
                .flex_row()
                .items_stretch()
                .w_full()
                .bg(theme.surface_muted.opacity(0.75))
                .border_b_1()
                .border_color(theme.border);
            for (c_i, (compiled, doc_range)) in headers.into_iter().enumerate() {
                let align = alignments.get(c_i).copied().unwrap_or(TableAlign::Left);
                let ends_nl = c_i + 1 == header_count && (!rows.is_empty() || has_following);
                let cell = apply_align(
                    div()
                        .flex_1()
                        .min_w(min_col_w)
                        .px(px(10.))
                        .py(px(7.))
                        .text_size(px(12.5))
                        .line_height(px(18.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground),
                    align,
                )
                .child(self.inline_element(compiled, doc_range, ends_nl));
                h_row = h_row.child(cell);
            }
            table_col = table_col.child(h_row);
        }

        // Body rows with alignment, subtle striping, and unified selection.
        let row_count = rows.len();
        for (r_i, row) in rows.into_iter().enumerate() {
            let is_last_row = r_i + 1 == row_count;
            let cell_count = row.len();
            let mut b_row = div()
                .flex()
                .flex_row()
                .items_stretch()
                .w_full()
                .when(r_i % 2 == 1, |el| el.bg(theme.surface_muted.opacity(0.32)))
                .when(!is_last_row, |el| {
                    el.border_b_1().border_color(theme.border.opacity(0.7))
                });
            for (c_i, (compiled, doc_range)) in row.into_iter().enumerate() {
                let align = alignments.get(c_i).copied().unwrap_or(TableAlign::Left);
                let ends_nl = c_i + 1 == cell_count && (!is_last_row || has_following);
                let cell = apply_align(
                    div()
                        .flex_1()
                        .min_w(min_col_w)
                        .px(px(10.))
                        .py(px(7.))
                        .text_size(px(13.))
                        .line_height(px(19.))
                        .text_color(theme.foreground),
                    align,
                )
                .child(self.inline_element(compiled, doc_range, ends_nl));
                b_row = b_row.child(cell);
            }
            table_col = table_col.child(b_row);
        }

        let inner = if wide_scroll {
            div()
                .id(SharedString::from(format!("{block_id}-tbl-scroll")))
                .w_full()
                .overflow_x_scroll()
                .child(table_col)
                .into_any_element()
        } else {
            table_col.into_any_element()
        };

        div()
            .w_full()
            .rounded(theme.radius_md)
            .border_1()
            .border_color(theme.border)
            .bg(theme.surface)
            .overflow_hidden()
            .child(inner)
            .into_any_element()
    }

    fn render_code_block(
        &self,
        block_id: &str,
        code_idx: usize,
        language: String,
        raw_code: String,
        lines: Vec<(HighlightedLine, Range<usize>)>,
        has_following: bool,
    ) -> AnyElement {
        let theme = self.theme;
        let syntax = self.syntax;
        let lang_label = friendly_language_label(&language);
        let is_wrapped = self.wrapped_code_blocks.contains(&code_idx);
        let line_count = lines.len();
        let digits = ((line_count.max(1) as f32).log10().floor() as usize) + 1;
        let gutter_w = px((digits.max(2) as f32 * 8.0 + 18.0).ceil());
        let line_h = px(20.);

        let state_for_wrap = self.state.clone();
        let header = div()
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .px(px(10.))
            .py(px(5.))
            .border_b_1()
            .border_color(syntax.code_border)
            .bg(syntax.code_header_bg)
            .cursor_default()
            .on_mouse_down(MouseButton::Left, |_, _, cx| {
                cx.stop_propagation();
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(6.))
                    .child(
                        Icon::new(LucideIcon::CodeXml)
                            .size(IconSize::Xs)
                            .color(theme.primary),
                    )
                    .child(
                        div()
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(syntax.plain.opacity(0.85))
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
                        Button::icon(
                            SharedString::from(format!("{block_id}-wrap")),
                            LucideIcon::TextWrap,
                        )
                        .size(ComponentSize::Xs)
                        .variant(if is_wrapped {
                            ButtonVariant::Soft
                        } else {
                            ButtonVariant::Ghost
                        })
                        .on_click(move |_, window, cx| {
                            state_for_wrap.update(cx, |s, _| s.toggle_code_wrap(code_idx));
                            window.refresh();
                        }),
                    )
                    .child(
                        copy_button(SharedString::from(format!("{block_id}-copy")), raw_code)
                            .size(ComponentSize::Xs)
                            .variant(ButtonVariant::Ghost),
                    ),
            );

        let body: AnyElement = if is_wrapped {
            let mut rows = div()
                .w_full()
                .py(px(8.))
                .flex()
                .flex_col()
                .text_size(px(12.25))
                .line_height(line_h)
                .text_color(syntax.plain)
                .when_some(theme.mono_font_family.clone(), |el, f| el.font_family(f));

            for (ln, (line, doc_range)) in lines.into_iter().enumerate() {
                let line_nl = ln + 1 < line_count || has_following;
                let row_bg = line.diff_kind.map(|k| syntax.diff_line_bg(k));
                let code_el = self.highlighted_code_line(&line, doc_range, line_nl);
                rows = rows.child(
                    div()
                        .w_full()
                        .px(px(10.))
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(10.))
                        .when_some(row_bg, |el, fill| el.bg(fill))
                        .when(self.show_line_numbers, |el| {
                            el.child(
                                div()
                                    .w(gutter_w)
                                    .flex_shrink_0()
                                    .text_right()
                                    .font_features(tabular_numerals())
                                    .text_color(syntax.gutter_foreground)
                                    .child(format!("{}", ln + 1)),
                            )
                        })
                        .child(div().flex_1().min_w_0().min_h(line_h).child(code_el)),
                );
            }
            rows.into_any_element()
        } else {
            let mut gutter_col = div()
                .w(gutter_w)
                .flex_shrink_0()
                .py(px(8.))
                .border_r_1()
                .border_color(syntax.code_border.opacity(0.6))
                .flex()
                .flex_col()
                .text_size(px(12.25))
                .line_height(line_h)
                .text_right()
                .font_features(tabular_numerals())
                .text_color(syntax.gutter_foreground);

            let mut code_lines_col = div()
                .min_w_full()
                .py(px(8.))
                .flex()
                .flex_col()
                .text_size(px(12.25))
                .line_height(line_h)
                .text_color(syntax.plain);

            if let Some(mono) = theme.mono_font_family.clone() {
                gutter_col = gutter_col.font_family(mono.clone());
                code_lines_col = code_lines_col.font_family(mono);
            }

            for (ln, (line, doc_range)) in lines.into_iter().enumerate() {
                let row_bg = line.diff_kind.map(|k| syntax.diff_line_bg(k));
                if self.show_line_numbers {
                    gutter_col = gutter_col.child(
                        div()
                            .w_full()
                            .min_h(line_h)
                            .pl(px(8.))
                            .pr(px(8.))
                            .whitespace_nowrap()
                            .when_some(row_bg, |el, fill| el.bg(fill))
                            .child(format!("{}", ln + 1)),
                    );
                }
                let line_nl = ln + 1 < line_count || has_following;
                let code_el = self.highlighted_code_line(&line, doc_range, line_nl);
                code_lines_col = code_lines_col.child(
                    div()
                        .min_h(line_h)
                        .px(px(10.))
                        .whitespace_nowrap()
                        .when_some(row_bg, |el, fill| el.bg(fill))
                        .child(code_el),
                );
            }

            let code_scroll = div()
                .id(SharedString::from(format!("{block_id}-code-scroll")))
                .flex_1()
                .min_w_0()
                .overflow_x_scroll()
                .child(code_lines_col);

            div()
                .w_full()
                .flex()
                .flex_row()
                .items_start()
                .when(self.show_line_numbers, |el| el.child(gutter_col))
                .child(code_scroll)
                .into_any_element()
        };

        div()
            .w_full()
            .rounded(theme.radius_md)
            .bg(syntax.code_bg)
            .border_1()
            .border_color(syntax.code_border)
            .overflow_hidden()
            .child(header)
            .child(body)
            .into_any_element()
    }
}

impl RenderOnce for Markdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let syntax = self
            .syntax_theme
            .unwrap_or_else(|| theme.syntax.clone());
        let id_root = self.id;

        // Keyed state handles both memoized parsing/highlighting and selection/context-menu state.
        let md_state = window.use_keyed_state(
            SharedString::from(format!("{id_root}-selection")),
            cx,
            |window, cx| MarkdownState::new(window, cx),
        );

        let (blocks, pending_component) = md_state.update(cx, |s, _| {
            s.sync_parsed(&id_root, &self.source, self.streaming, self.math_enabled)
        });

        // Phase 1: Prepare recursive render tree & unified `doc_text` for cross-block selection.
        let mut doc_text = String::new();
        let prepared = md_state.update(cx, |state, _| {
            let mut prep_env = PrepareEnv {
                theme: &theme,
                syntax: &syntax,
                resolve_link: &self.resolve_link,
                streaming: self.streaming,
                state,
                code_counter: 0,
            };
            let prepared = prep_env.prepare_blocks(&blocks, &mut doc_text, "\n\n");
            if self.selectable {
                state.sync_document(doc_text, self.on_open_link.clone());
            }
            prepared
        });

        let (
            selected_range,
            is_selecting,
            wrapped_code_blocks,
            hovered_link,
            rendered_segments,
            viewport_bounds,
        ) = {
            let s = md_state.read(cx);
            (
                if self.selectable {
                    s.selected_range.clone()
                } else {
                    0..0
                },
                self.selectable && s.is_selecting,
                s.wrapped_code_blocks.clone(),
                s.hovered_link.clone(),
                self.selectable.then(|| s.rendered_segments.clone()),
                self.selectable.then(|| s.viewport_bounds.clone()),
            )
        };

        let (code_pill_bg, code_pill_border) = match theme.mode {
            ThemeMode::Light => (
                theme.foreground.opacity(0.065),
                theme.foreground.opacity(0.11),
            ),
            ThemeMode::Dark => (
                theme.foreground.opacity(0.09),
                theme.foreground.opacity(0.14),
            ),
        };

        let render_env = RenderEnv {
            id_root: &id_root,
            theme: &theme,
            syntax: &syntax,
            show_line_numbers: self.show_line_numbers,
            code_pill_bg,
            code_pill_border,
            selection: selected_range,
            selection_color: theme.selection,
            selectable: self.selectable,
            is_selecting,
            wrapped_code_blocks,
            state: md_state.clone(),
            registry: self.components.clone(),
            on_open_link: self.on_open_link.clone(),
            on_image: self.on_image.clone(),
            on_math: self.on_math.clone(),
            rendered_segments: rendered_segments.clone(),
            viewport_bounds: viewport_bounds.clone(),
        };

        // Phase 2: Build visual element tree.
        let mut col = div().w_full().flex().flex_col().gap(px(10.));
        col = col.child(render_env.render_blocks(
            prepared,
            render_env.id_root.as_ref(),
            0,
            false,
            px(10.),
            window,
            cx,
        ));

        if let Some(pending) = pending_component {
            let label = self
                .components
                .as_ref()
                .and_then(|r| r.get(&pending))
                .map(|s| s.label.to_string())
                .unwrap_or(pending);
            col = col.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(8.))
                    .rounded(theme.radius_md)
                    .bg(theme.surface_muted.opacity(0.6))
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(12.5))
                    .text_color(theme.muted_foreground)
                    .child(
                        Icon::new(LucideIcon::LoaderCircle)
                            .size(IconSize::Xs)
                            .color(theme.primary)
                            .spin(),
                    )
                    .child(format!("{label}…")),
            );
        }

        if let Some(href) = hovered_link {
            col = col.child(
                div()
                    .w_full()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(5.))
                    .px(px(8.))
                    .py(px(3.))
                    .rounded(theme.radius_sm)
                    .bg(theme.surface_muted.opacity(0.85))
                    .border_1()
                    .border_color(theme.border.opacity(0.8))
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .child(
                        Icon::new(LucideIcon::Link)
                            .size(IconSize::Xs)
                            .color(theme.primary),
                    )
                    .child(div().flex_1().min_w_0().truncate().child(href)),
            );
        }

        if !self.selectable {
            crate::Refineable::refine(col.style(), &self.style);
            return col.into_any_element();
        }

        let (focus_handle, menu_pos, menu_entries) = {
            let s = md_state.read(cx);
            let entries = if s.context_menu_position.is_some() {
                s.build_context_menu_entries(md_state.clone())
            } else {
                Vec::new()
            };
            (s.focus_handle.clone(), s.context_menu_position, entries)
        };

        let state_close = md_state.clone();
        let menu = popup_menu(SharedString::from(format!("{id_root}-ctx")), menu_pos)
            .min_width(px(180.))
            .entries(menu_entries)
            .on_close(move |_, window, cx| {
                state_close.update(cx, |s, _| {
                    s.context_menu_position = None;
                    s.context_menu_link = None;
                });
                window.refresh();
            });

        let viewport_wrapped = MarkdownSelectionViewportElement {
            child: col.into_any_element(),
            rendered_segments: rendered_segments.expect("selectable markdown has segments"),
            viewport_bounds: viewport_bounds.expect("selectable markdown has bounds"),
            state: md_state.clone(),
            is_selecting,
        };

        let state_left = md_state.clone();
        let state_right = md_state.clone();
        let state_key = md_state.clone();
        let state_copy = md_state.clone();
        let state_all = md_state.clone();
        let state_clear = md_state;

        let mut root = div()
            .id(SharedString::from(format!("{id_root}-md-root")))
            .w_full()
            .key_context("GpuiMarkdown")
            .track_focus(&focus_handle.tab_stop(false))
            .on_mouse_down(MouseButton::Left, move |event, window, cx| {
                let changed = state_left.update(cx, |s, cx| s.on_left_mouse_down(event, window, cx));
                cx.stop_propagation();
                if changed {
                    window.refresh();
                }
            })
            .on_mouse_down(MouseButton::Right, move |event, window, cx| {
                let changed =
                    state_right.update(cx, |s, cx| s.on_right_mouse_down(event, window, cx));
                cx.stop_propagation();
                if changed {
                    window.refresh();
                }
            })
            .on_key_down(move |event: &KeyDownEvent, window, cx| {
                let mods = &event.keystroke.modifiers;
                let key = event.keystroke.key.as_str();
                if (mods.shift && key == "f10" && !mods.control && !mods.alt && !mods.platform)
                    || key == "contextmenu"
                {
                    state_key.update(cx, |s, _| {
                        s.context_menu_position = Some(s.keyboard_context_menu_anchor());
                        s.context_menu_link = None;
                    });
                    window.prevent_default();
                    cx.stop_propagation();
                    window.refresh();
                }
            })
            .on_action(move |_: &Copy, _window, cx| {
                let copied = state_copy.update(cx, |s, cx| s.copy_selection(cx));
                if !copied {
                    cx.propagate();
                }
            })
            .on_action(move |_: &SelectAll, window, cx| {
                let changed = state_all.update(cx, |s, _| s.select_all());
                if changed {
                    window.refresh();
                }
            })
            .on_action(move |_: &ClearSelection, window, cx| {
                let consumed = state_clear.update(cx, |s, _| {
                    if s.context_menu_position.take().is_some() {
                        s.context_menu_link = None;
                        true
                    } else {
                        s.clear_selection()
                    }
                });
                if consumed {
                    window.refresh();
                } else {
                    cx.propagate();
                }
            })
            .child(viewport_wrapped)
            .child(menu);

        crate::Refineable::refine(root.style(), &self.style);
        root.into_any_element()
    }
}

fn render_component_fallback(message: &str, name: &str, raw_json: &str, cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .w_full()
        .p(px(10.))
        .rounded(theme.radius_md)
        .bg(theme.surface_muted.opacity(0.6))
        .border_1()
        .border_dashed()
        .border_color(theme.border)
        .flex()
        .flex_col()
        .gap(px(4.))
        .child(
            div()
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .text_color(theme.muted_foreground)
                .child(format!("{message} ({name})")),
        )
        .when(!raw_json.trim().is_empty(), |el| {
            let preview: String = raw_json.chars().take(240).collect();
            el.child(
                div()
                    .text_size(px(11.))
                    .text_color(theme.muted_foreground)
                    .child(preview),
            )
        })
        .into_any_element()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::components::theme::init_components;
    use crate::TestAppContext;

    #[test]
    fn pulldown_cmark_parses_gfm_alerts_nested_lists_tables_math_and_safe_html() {
        let md = concat!(
            "> [!WARNING]\n",
            "> Watch out for `$26.96` currency and $x_i + y_i = z_i$ math.\n",
            ">\n",
            "> - Nested item 1\n",
            ">   - Deep nested item\n",
            "\n",
            "Spend moved from **$12.50** to **$26.96** (range $10-$20), while $E = mc^2$ and $2x + 1 = 5$ and $10^6$ are math.\n\n",
            "$$\n\\sum_{i=1}^n x_i^2\n$$\n\n",
            "| Metric | Alignment | Value |\n",
            "|:-------|:---------:|------:|\n",
            "| `cost` | **bold** | $26.96 |\n\n",
            "1. First item\n",
            "\n",
            "   ```rust\n",
            "   let nested = true;\n",
            "   ```\n",
            "2. [x] Done task\n\n",
            "Raw <script>alert(1)</script> stays literal text[^1].\n\n",
            "[^1]: Footnote body text.\n",
        );

        let blocks = parse_blocks(md);

        // 1. GFM Alert with nested list inside!
        let MdBlock::Blockquote {
            kind: Some(MdAlertKind::Warning),
            blocks: alert_blocks,
        } = &blocks[0]
        else {
            panic!("expected Warning alert blockquote, got {:?}", blocks[0]);
        };
        assert!(
            alert_blocks
                .iter()
                .any(|b| matches!(b, MdBlock::List { .. })),
            "alert should contain nested list: {alert_blocks:?}"
        );

        // 2. Currency stays literal text while real `$E = mc^2$` becomes InlineMath!
        let MdBlock::Paragraph(currency_inlines) = &blocks[1] else {
            panic!("expected currency paragraph, got {:?}", blocks[1]);
        };
        let compiled = compile_inline_nodes(
            currency_inlines,
            FontWeight::NORMAL,
            crate::black(),
            crate::blue(),
            crate::blue(),
            crate::black(),
            None,
            &None,
        );
        assert!(
            compiled.text.contains("Spend moved from $12.50 to $26.96")
                && !compiled.text.contains("**")
                && compiled.text.contains("$10-$20")
                && compiled.text.contains("E = mc^2")
                && compiled.text.contains("2x + 1 = 5")
                && compiled.text.contains("10^6"),
            "bold currency must not leak `**` or get corrupted into math while real math parses cleanly: {:?}",
            compiled.text
        );
        assert!(
            currency_inlines
                .iter()
                .any(|n| matches!(n, MdInlineNode::Bold(c) if matches!(c.first(), Some(MdInlineNode::Text(t)) if t == "$12.50"))),
            "expected Bold($12.50): {currency_inlines:?}"
        );
        assert!(
            currency_inlines
                .iter()
                .any(|n| matches!(n, MdInlineNode::InlineMath(t) if t == "E = mc^2"))
                && currency_inlines
                    .iter()
                    .any(|n| matches!(n, MdInlineNode::InlineMath(t) if t == "2x + 1 = 5"))
                && currency_inlines
                    .iter()
                    .any(|n| matches!(n, MdInlineNode::InlineMath(t) if t == "10^6")),
            "expected InlineMath spans: {currency_inlines:?}"
        );

        // 3. Display math block
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, MdBlock::DisplayMath(t) if t.contains("\\sum_{i=1}^n"))),
            "expected DisplayMath block in {blocks:?}"
        );

        // 4. GFM Table with Left, Center, Right alignments and rich inline cells
        let Some(MdBlock::Table {
            alignments,
            headers,
            rows,
        }) = blocks.iter().find(|b| matches!(b, MdBlock::Table { .. }))
        else {
            panic!("expected Table block");
        };
        assert_eq!(
            alignments,
            &vec![TableAlign::Left, TableAlign::Center, TableAlign::Right]
        );
        assert_eq!(headers.len(), 3);
        assert_eq!(rows.len(), 1);
        assert!(matches!(rows[0][0].first(), Some(MdInlineNode::Code(c)) if c == "cost"));
        assert!(matches!(rows[0][1].first(), Some(MdInlineNode::Bold(_))));

        // 5. Ordered list item containing a nested Rust code block + task item
        let Some(MdBlock::List { start, items }) =
            blocks.iter().find(|b| matches!(b, MdBlock::List { .. }))
        else {
            panic!("expected ordered list block");
        };
        assert_eq!(*start, Some(1));
        assert_eq!(items.len(), 2);
        assert!(
            items[0]
                .blocks
                .iter()
                .any(|b| matches!(b, MdBlock::CodeBlock { language, code } if language == "rust" && code == "let nested = true;")),
            "list item should contain nested fenced code block: {:?}",
            items[0].blocks
        );
        assert_eq!(items[1].checked, Some(true));

        // 6. Raw HTML preserved as literal text + footnote definition parsed
        assert!(
            blocks.iter().any(|b| matches!(
                b,
                MdBlock::Paragraph(inlines) if inlines.iter().any(|n| matches!(n, MdInlineNode::Text(t) if t.contains("<script>")))
            )),
            "raw HTML must remain visible literal text: {blocks:?}"
        );
        assert!(
            blocks
                .iter()
                .any(|b| matches!(b, MdBlock::FootnoteDefinition { label, .. } if label == "1")),
            "expected FootnoteDefinition(1): {blocks:?}"
        );
    }

    #[test]
    fn unclosed_streaming_code_fence_parses_for_live_highlighting() {
        let streaming_src = "Here is code:\n\n```rust\nfn main() {\n    let x = 42;";
        let blocks = parse_blocks(streaming_src);
        assert_eq!(blocks.len(), 2);
        let MdBlock::CodeBlock { language, code } = &blocks[1] else {
            panic!("expected streaming CodeBlock, got {:?}", blocks[1]);
        };
        assert_eq!(language, "rust");
        assert_eq!(code, "fn main() {\n    let x = 42;");
    }

    #[crate::test]
    fn markdown_state_memoizes_parsing_across_identical_renders(cx: &mut TestAppContext) {
        cx.update(|cx| init_components(cx));
        let key = "cache-memoize-isolated-test";
        test_parse_counts().lock().unwrap().remove(key);

        struct CacheHost {
            key: &'static str,
            source: SharedString,
            tick: usize,
        }
        impl crate::Render for CacheHost {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                let _ = self.tick;
                div()
                    .size_full()
                    .child(markdown(self.key, self.source.clone()))
            }
        }

        let count_for = |k: &str| *test_parse_counts().lock().unwrap().get(k).unwrap_or(&0);

        let win = cx.open_window(size(px(500.), px(400.)), |_, _| CacheHost {
            key,
            source: SharedString::new_static("# Title\n\n```rust\nlet a = 1;\n```"),
            tick: 0,
        });
        cx.run_until_parked();
        assert_eq!(
            count_for(key),
            1,
            "initial render should parse Markdown exactly once"
        );

        // Trigger 3 extra re-renders with identical source (simulating hover/scroll/selection frames).
        for next_tick in 1..=3 {
            win.update(cx, |host, _, cx| {
                host.tick = next_tick;
                cx.notify();
            })
            .unwrap();
            cx.run_until_parked();
        }
        assert_eq!(
            count_for(key),
            1,
            "re-rendering with identical Markdown source must hit memoized parse cache"
        );

        // Changing source triggers exactly one new parse.
        win.update(cx, |host, _, cx| {
            host.source = SharedString::new_static("# Title\n\nUpdated paragraph.");
            cx.notify();
        })
        .unwrap();
        cx.run_until_parked();
        assert_eq!(
            count_for(key),
            2,
            "changing Markdown source should increment parse count exactly once"
        );
    }

    #[crate::test]
    fn markdown_rich_gfm_alerts_tables_diff_math_and_code_block_render_and_select(
        cx: &mut TestAppContext,
    ) {
        use crate::components::typography::code_block;
        cx.update(|cx| {
            init_components(cx);
            Theme::set_global(Theme::dark().with_mono_font_family("Consolas"), cx);
        });

        let rich_md = concat!(
            "# Architecture Overview\n\n",
            "> [!TIP]\n",
            "> Spend stayed between $12.50 and $26.96 while optimizing $O(n \\log n)$ queries.\n\n",
            "| `Column` | Align | Cost |\n",
            "|:---------|:-----:|-----:|\n",
            "| **Rust** | `ok` | $12.50 |\n\n",
            "```diff\n",
            "@@ -1,2 +1,2 @@\n",
            "-let old = 1;\n",
            "+let new = 2;\n",
            "```\n\n",
            "$$\n\\sum_{i=1}^n x_i\n$$\n\n",
            "1. Outer item\n",
            "   - Nested bullet\n",
        );

        let long_rust: String = (1..=45)
            .map(|i| format!("let v_{i}: usize = {i} * 2; // line {i}"))
            .collect::<Vec<_>>()
            .join("\n");

        struct RichHost {
            md: SharedString,
            long_code: SharedString,
        }
        impl crate::Render for RichHost {
            fn render(
                &mut self,
                _window: &mut Window,
                _cx: &mut Context<Self>,
            ) -> impl IntoElement {
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .p(px(16.))
                    .child(markdown("rich-md", self.md.clone()))
                    .child(
                        code_block("rich-cb", self.long_code.clone())
                            .language("rust")
                            .max_height(px(220.)),
                    )
            }
        }

        let win = cx.open_window(size(px(680.), px(820.)), |_, _| RichHost {
            md: SharedString::from(rich_md),
            long_code: SharedString::from(long_rust),
        });
        cx.run_until_parked();

        let copy_stroke = if cfg!(target_os = "macos") { "cmd-c" } else { "ctrl-c" };
        let select_all_stroke = if cfg!(target_os = "macos") { "cmd-a" } else { "ctrl-a" };

        let mut vcx = crate::VisualTestContext::from_window(win.into(), cx);
        // Click into the Markdown heading, Select All, and Copy.
        let pt = point(px(28.), px(26.));
        vcx.simulate_event(MouseDownEvent {
            position: pt,
            button: MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
            first_mouse: false,
        });
        vcx.simulate_event(MouseUpEvent {
            position: pt,
            button: MouseButton::Left,
            modifiers: crate::Modifiers::none(),
            click_count: 1,
        });
        vcx.run_until_parked();
        vcx.simulate_keystrokes(select_all_stroke);
        vcx.run_until_parked();
        vcx.simulate_keystrokes(copy_stroke);
        vcx.run_until_parked();

        let copied = vcx
            .read_from_clipboard()
            .and_then(|c| c.text())
            .expect("Select All + Copy on rich GFM Markdown should copy text");
        assert!(copied.contains("Architecture Overview"), "{copied:?}");
        assert!(copied.contains("$12.50 and $26.96"), "{copied:?}");
        assert!(copied.contains("Column\tAlign\tCost"), "table header should be selectable: {copied:?}");
        assert!(copied.contains("Rust\tok\t$12.50"), "table row should be selectable: {copied:?}");
        assert!(copied.contains("-let old = 1;\n+let new = 2;"), "diff code block should copy cleanly: {copied:?}");
        assert!(copied.contains("\\sum_{i=1}^n x_i"), "display math should copy cleanly: {copied:?}");
        assert!(copied.contains("Outer item\nNested bullet"), "nested list should copy cleanly: {copied:?}");

        // Switch theme from Dark to Light and re-render cleanly.
        vcx.update(|_, cx| Theme::set_global(Theme::light(), cx));
        vcx.run_until_parked();
    }
}

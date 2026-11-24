//! Pure-GPUI GitHub-Flavored [`Markdown`] renderer with embedded generative
//! [`AnswerComponentRegistry`] blocks, native single-flow text wrapping, compact inline-code
//! pills, multi-block text selection & copy, and security hardening:
//!
//! - Renders headings, paragraphs, nested inline **bold**, *italic*, ~~strikethrough~~,
//!   `` `code` ``, clickable `[links](url)` (filtering `javascript:` / `vbscript:` / `data:`
//!   schemes with optional `resolve_link` and `on_open_link` hooks), blockquotes,
//!   ordered/unordered/GFM task lists, tables, horizontal rules, and fenced code blocks with
//!   language badge, line numbers, and copy.
//! - **Native single-flow wrapping**: each prose block shapes and wraps as a single GPUI
//!   [`StyledText`], eliminating flexbox line-break artifacts across mixed bold, link, and inline
//!   code runs. Inline code pills are painted directly from glyph metrics as compact rounded
//!   highlights without injecting invisible padding characters into copied text.
//! - **Selectable & copyable**: supports click-and-drag selection across headings, paragraphs,
//!   inline code, lists, blockquotes, tables, and fenced code blocks (excluding line numbers),
//!   double-click word selection, triple-click line selection, `Cmd/Ctrl+C`, `Cmd/Ctrl+A`,
//!   `Escape`, and a right-click / keyboard context menu (`Copy`, `Select All`, plus `Open Link` /
//!   `Copy Link` over links).
//! - **Prompt-injection safe**: raw HTML is displayed as plain text, and external Markdown images
//!   `![alt](url)` are never fetched automatically (rendered as safe inline placeholders).
//! - **Streaming Answer Components**: detects unclosed ```` ```component:<name> ```` fences via
//!   [`split_open_component`] and displays a pulsing `<label>…` placeholder until the fence closes.

use std::cell::{Cell, RefCell};
use std::ops::Range;
use std::rc::Rc;

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

use super::answer_components::{AnswerComponentRegistry, split_open_component};
use crate::components::badge::{BadgeColor, badge};
use crate::components::context_menu::popup_menu;
use crate::components::input::{clamp_char_boundary, is_word_char};
use crate::components::interactive::copy_button;
use crate::components::popover::MenuEntry;
use crate::components::selectable_text::selectable_text_actions::{
    ClearSelection, Copy, SelectAll,
};
use crate::components::table::{Table, TableColumn, TableRow};
use crate::components::theme::{ActiveTheme, ThemeMode};

/// Optional URL resolver for Markdown links (return `None` to render the link as plain text).
pub type LinkResolver = Rc<dyn Fn(&str) -> Option<String> + 'static>;

/// Optional click handler for Markdown links (`href`, `window`, `cx`).
pub type LinkOpenHandler = Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>;

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
    components: Option<AnswerComponentRegistry>,
    resolve_link: Option<LinkResolver>,
    on_open_link: Option<LinkOpenHandler>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Markdown::new`].
pub fn markdown(id: impl Into<SharedString>, source: impl Into<SharedString>) -> Markdown {
    Markdown::new(id, source)
}

impl Markdown {
    /// Creates a new [`Markdown`] element (selectable by default).
    pub fn new(id: impl Into<SharedString>, source: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            streaming: false,
            selectable: true,
            components: None,
            resolve_link: None,
            on_open_link: None,
            style: StyleRefinement::default(),
        }
    }

    /// Marks the Markdown as actively streaming so unclosed ```` ```component:<name> ```` fences
    /// render as `<label>…` placeholders instead of partial JSON.
    pub fn streaming(mut self, streaming: bool) -> Self {
        self.streaming = streaming;
        self
    }

    /// Enables or disables text selection, copy shortcuts, and context menu (defaults to `true`).
    pub fn selectable(mut self, selectable: bool) -> Self {
        self.selectable = selectable;
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
// Block & Nested Inline Markdown Parser
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MdBlock {
    Heading { level: u8, text: String },
    Paragraph(String),
    CodeBlock { language: String, code: String },
    ComponentBlock { name: String, json_body: String },
    Blockquote(String),
    List { ordered: bool, items: Vec<MdListItem> },
    Table { headers: Vec<String>, rows: Vec<Vec<String>> },
    Rule,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) struct MdListItem {
    pub(crate) checked: Option<bool>,
    pub(crate) text: String,
}

#[derive(Debug, PartialEq, Eq)]
pub(crate) enum MdInlineNode {
    Text(String),
    Bold(Vec<MdInlineNode>),
    Italic(Vec<MdInlineNode>),
    Strike(Vec<MdInlineNode>),
    Code(String),
    Link {
        children: Vec<MdInlineNode>,
        href: String,
    },
    ImagePlaceholder {
        alt: String,
    },
}

pub(crate) fn parse_blocks(src: &str) -> Vec<MdBlock> {
    let lines: Vec<&str> = src.lines().collect();
    let mut blocks = Vec::new();
    let mut i = 0;

    while i < lines.len() {
        let raw = lines[i].trim_end_matches('\r');
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            i += 1;
            continue;
        }

        // Fenced code or component block (``` or ~~~)
        if let Some((fence_ch, fence_len, info)) = parse_fence_open(trimmed) {
            i += 1;
            let mut body = Vec::new();
            while i < lines.len() {
                let l = lines[i].trim_end_matches('\r');
                if is_fence_close(l.trim(), fence_ch, fence_len) {
                    i += 1;
                    break;
                }
                body.push(l);
                i += 1;
            }
            let joined = body.join("\n");
            if let Some(comp_name) = info
                .strip_prefix("component:")
                .and_then(|s| s.split_whitespace().next())
                .filter(|s| !s.is_empty())
            {
                blocks.push(MdBlock::ComponentBlock {
                    name: comp_name.to_owned(),
                    json_body: joined,
                });
            } else {
                blocks.push(MdBlock::CodeBlock {
                    language: info.split_whitespace().next().unwrap_or("").to_owned(),
                    code: joined,
                });
            }
            continue;
        }

        // Horizontal rule
        if is_horizontal_rule(trimmed) {
            blocks.push(MdBlock::Rule);
            i += 1;
            continue;
        }

        // ATX Heading
        if let Some((level, text)) = parse_heading(trimmed) {
            blocks.push(MdBlock::Heading { level, text });
            i += 1;
            continue;
        }

        // Blockquote
        if trimmed.starts_with('>') {
            let mut quote_lines = Vec::new();
            while i < lines.len() {
                let l = lines[i].trim_end_matches('\r').trim();
                if let Some(rest) = l.strip_prefix('>') {
                    quote_lines.push(rest.trim_start().to_owned());
                    i += 1;
                } else {
                    break;
                }
            }
            blocks.push(MdBlock::Blockquote(quote_lines.join("\n")));
            continue;
        }

        // GFM Table (header row + separator row containing ---)
        if trimmed.contains('|')
            && i + 1 < lines.len()
            && is_table_separator(lines[i + 1].trim())
        {
            let headers = split_table_row(trimmed);
            i += 2;
            let mut rows = Vec::new();
            while i < lines.len() {
                let row_line = lines[i].trim_end_matches('\r').trim();
                if row_line.is_empty() || !row_line.contains('|') {
                    break;
                }
                rows.push(split_table_row(row_line));
                i += 1;
            }
            blocks.push(MdBlock::Table { headers, rows });
            continue;
        }

        // Unordered / Task / Ordered list (with indented continuation line support)
        if let Some((ordered, first_item)) = parse_list_item(trimmed) {
            let mut items = vec![first_item];
            i += 1;
            while i < lines.len() {
                let raw_l = lines[i].trim_end_matches('\r');
                let l = raw_l.trim();
                if l.is_empty() {
                    break;
                }
                if let Some((next_ord, item)) = parse_list_item(l)
                    && next_ord == ordered
                {
                    items.push(item);
                    i += 1;
                } else if (raw_l.starts_with("  ") || raw_l.starts_with('\t'))
                    && parse_fence_open(l).is_none()
                    && parse_heading(l).is_none()
                    && !l.starts_with('>')
                    && !is_horizontal_rule(l)
                    && parse_list_item(l).is_none()
                {
                    if let Some(last) = items.last_mut() {
                        if !last.text.is_empty() {
                            last.text.push(' ');
                        }
                        last.text.push_str(l);
                    }
                    i += 1;
                } else {
                    break;
                }
            }
            blocks.push(MdBlock::List { ordered, items });
            continue;
        }

        // Paragraph (accumulate consecutive plain lines; preserve hard breaks via `\` or `  `)
        let mut para = String::new();
        append_paragraph_line(&mut para, raw);
        i += 1;
        while i < lines.len() {
            let raw_l = lines[i].trim_end_matches('\r');
            let l = raw_l.trim();
            if l.is_empty()
                || parse_fence_open(l).is_some()
                || parse_heading(l).is_some()
                || l.starts_with('>')
                || is_horizontal_rule(l)
                || parse_list_item(l).is_some()
                || (l.contains('|')
                    && i + 1 < lines.len()
                    && is_table_separator(lines[i + 1].trim()))
            {
                break;
            }
            append_paragraph_line(&mut para, raw_l);
            i += 1;
        }
        blocks.push(MdBlock::Paragraph(para));
    }

    blocks
}

fn append_paragraph_line(para: &mut String, raw_line: &str) {
    let hard_break = raw_line.ends_with("  ") || raw_line.trim_end().ends_with('\\');
    let mut content = raw_line.trim();
    if hard_break && content.ends_with('\\') {
        content = content[..content.len() - 1].trim_end();
    }
    if !para.is_empty() && !para.ends_with('\n') {
        para.push(' ');
    }
    para.push_str(content);
    if hard_break {
        para.push('\n');
    }
}

fn parse_fence_open(trimmed: &str) -> Option<(u8, usize, &str)> {
    let ch = *trimmed.as_bytes().first()?;
    if ch != b'`' && ch != b'~' {
        return None;
    }
    let len = trimmed.bytes().take_while(|&b| b == ch).count();
    if len < 3 {
        return None;
    }
    let info = trimmed[len..].trim();
    if ch == b'`' && info.contains('`') {
        return None;
    }
    Some((ch, len, info))
}

fn is_fence_close(trimmed: &str, ch: u8, min_len: usize) -> bool {
    let len = trimmed.bytes().take_while(|&b| b == ch).count();
    len >= min_len && trimmed[len..].trim().is_empty()
}

fn is_horizontal_rule(trimmed: &str) -> bool {
    let chars: Vec<char> = trimmed.chars().filter(|c| !c.is_whitespace()).collect();
    chars.len() >= 3
        && matches!(chars[0], '-' | '*' | '_')
        && chars.iter().all(|&c| c == chars[0])
}

fn parse_heading(trimmed: &str) -> Option<(u8, String)> {
    let hashes = trimmed.bytes().take_while(|&b| b == b'#').count();
    if (1..=6).contains(&hashes) {
        let rest = &trimmed[hashes..];
        if rest.is_empty() || rest.starts_with(' ') || rest.starts_with('\t') {
            return Some((
                hashes as u8,
                rest.trim().trim_end_matches('#').trim().to_owned(),
            ));
        }
    }
    None
}

fn is_table_separator(trimmed: &str) -> bool {
    if !trimmed.contains('-') {
        return false;
    }
    trimmed
        .bytes()
        .all(|b| matches!(b, b'|' | b'-' | b':' | b' ' | b'\t'))
}

fn split_table_row(line: &str) -> Vec<String> {
    let inner = line.trim().trim_start_matches('|').trim_end_matches('|');
    inner.split('|').map(|c| c.trim().to_owned()).collect()
}

fn parse_list_item(trimmed: &str) -> Option<(bool, MdListItem)> {
    // Unordered: - / * / +
    for prefix in ["- ", "* ", "+ "] {
        if let Some(rest) = trimmed.strip_prefix(prefix) {
            let (checked, body) = if let Some(r) = rest
                .strip_prefix("[x] ")
                .or_else(|| rest.strip_prefix("[X] "))
            {
                (Some(true), r)
            } else if let Some(r) = rest.strip_prefix("[ ] ") {
                (Some(false), r)
            } else {
                (None, rest)
            };
            return Some((
                false,
                MdListItem {
                    checked,
                    text: body.trim().to_owned(),
                },
            ));
        }
    }
    // Ordered: 1. / 2)
    let digits = trimmed.bytes().take_while(|b| b.is_ascii_digit()).count();
    if digits > 0 && digits <= 4 {
        let rest = &trimmed[digits..];
        if let Some(after) = rest.strip_prefix(". ").or_else(|| rest.strip_prefix(") ")) {
            return Some((
                true,
                MdListItem {
                    checked: None,
                    text: after.trim().to_owned(),
                },
            ));
        }
    }
    None
}

fn is_escapable_punctuation(b: u8) -> bool {
    matches!(
        b,
        b'\\' | b'`' | b'*' | b'_' | b'{' | b'}' | b'[' | b']' | b'(' | b')' | b'#' | b'+'
            | b'-' | b'.' | b'!' | b'~' | b'|' | b'>'
    )
}

/// Finds the closing backtick run matching an opening run of exact length `ticks`, returning
/// `(content_end_in_rest, total_consumed_bytes)`.
fn find_code_span_end(rest: &str, ticks: usize) -> Option<(usize, usize)> {
    let bytes = rest.as_bytes();
    let mut j = ticks;
    while j < bytes.len() {
        if bytes[j] == b'`' {
            let run = bytes[j..].iter().take_while(|&&b| b == b'`').count();
            if run == ticks {
                return Some((j, j + run));
            }
            j += run;
        } else {
            j += 1;
        }
    }
    None
}

/// Finds the closing `delimiter` (`**`, `__`, `~~`, `*`, `_`) in `text` starting after `open_end`,
/// skipping backslash escapes and inline code spans so `` **`**`** `` parses cleanly.
/// Returns the byte index of the closing delimiter within `text`.
fn find_closing_delim(text: &str, open_end: usize, delim: &str, word_bounded: bool) -> Option<usize> {
    let bytes = text.as_bytes();
    let delim_bytes = delim.as_bytes();
    let delim_len = delim_bytes.len();
    let mut j = open_end;

    while j < bytes.len() {
        // Skip backslash-escaped ASCII punctuation
        if bytes[j] == b'\\' && j + 1 < bytes.len() && is_escapable_punctuation(bytes[j + 1]) {
            j += 2;
            continue;
        }
        // Skip inline code spans atomically
        if bytes[j] == b'`' {
            let ticks = bytes[j..].iter().take_while(|&&b| b == b'`').count();
            if let Some((_, consumed)) = find_code_span_end(&text[j..], ticks) {
                j += consumed;
                continue;
            }
            j += ticks;
            continue;
        }

        if text[j..].starts_with(delim) && j > open_end {
            // For single `*` / `_`, don't match inside a double `**` / `__` run.
            if delim_len == 1 {
                let d = delim_bytes[0];
                let prev_same = j > 0 && bytes[j - 1] == d;
                let next_same = j + 1 < bytes.len() && bytes[j + 1] == d;
                if prev_same || next_same {
                    j += 1;
                    continue;
                }
            }
            // Closing delimiter must not be immediately preceded by whitespace.
            let prev_char = text[open_end..j].chars().next_back();
            let prev_ok = prev_char.is_some_and(|c| !c.is_whitespace());
            // Word-bounded delimiters (`_`, `__`) must not be immediately followed by an alphanumeric.
            let next_ok = if word_bounded {
                text[j + delim_len..]
                    .chars()
                    .next()
                    .is_none_or(|c| !c.is_alphanumeric())
            } else {
                true
            };
            if prev_ok && next_ok {
                return Some(j);
            }
        }

        if let Some(ch) = text[j..].chars().next() {
            j += ch.len_utf8();
        } else {
            break;
        }
    }
    None
}

/// Parses `[label](href)` or `![alt](url)` starting at the `[` byte index, handling nested
/// brackets, inline code spans inside the label, and balanced parentheses inside the URL.
/// Returns `(label_slice, href_slice, total_bytes_from_open_bracket)`.
fn parse_bracket_link(from_bracket: &str) -> Option<(&str, &str, usize)> {
    debug_assert!(from_bracket.starts_with('['));
    let bytes = from_bracket.as_bytes();
    let mut j = 1usize;
    let mut bracket_depth = 1usize;

    while j < bytes.len() {
        if bytes[j] == b'\\' && j + 1 < bytes.len() && is_escapable_punctuation(bytes[j + 1]) {
            j += 2;
            continue;
        }
        if bytes[j] == b'`' {
            let ticks = bytes[j..].iter().take_while(|&&b| b == b'`').count();
            if let Some((_, consumed)) = find_code_span_end(&from_bracket[j..], ticks) {
                j += consumed;
                continue;
            }
            j += ticks;
            continue;
        }
        if bytes[j] == b'[' {
            bracket_depth += 1;
        } else if bytes[j] == b']' {
            bracket_depth -= 1;
            if bracket_depth == 0 {
                break;
            }
        }
        j += 1;
    }

    if bracket_depth != 0 || j + 1 >= bytes.len() || bytes[j + 1] != b'(' {
        return None;
    }

    let label = &from_bracket[1..j];
    let url_start = j + 2;
    let mut k = url_start;
    let mut paren_depth = 1usize;

    while k < bytes.len() {
        if bytes[k] == b'\\' && k + 1 < bytes.len() && is_escapable_punctuation(bytes[k + 1]) {
            k += 2;
            continue;
        }
        if bytes[k] == b'(' {
            paren_depth += 1;
        } else if bytes[k] == b')' {
            paren_depth -= 1;
            if paren_depth == 0 {
                let href = from_bracket[url_start..k].trim();
                return Some((label, href, k + 1));
            }
        }
        k += 1;
    }

    None
}

pub(crate) fn parse_inlines(text: &str) -> Vec<MdInlineNode> {
    parse_inlines_depth(text, 0)
}

fn parse_inlines_depth(text: &str, depth: u8) -> Vec<MdInlineNode> {
    if depth >= 6 {
        return if text.is_empty() {
            Vec::new()
        } else {
            vec![MdInlineNode::Text(text.to_owned())]
        };
    }

    let mut out = Vec::new();
    let mut buf = String::new();
    let bytes = text.as_bytes();
    let mut i = 0usize;

    let flush = |buf: &mut String, out: &mut Vec<MdInlineNode>| {
        if !buf.is_empty() {
            out.push(MdInlineNode::Text(std::mem::take(buf)));
        }
    };

    while i < bytes.len() {
        let rest = &text[i..];

        // Backslash escape for Markdown punctuation
        if bytes[i] == b'\\' && i + 1 < bytes.len() && is_escapable_punctuation(bytes[i + 1]) {
            buf.push(bytes[i + 1] as char);
            i += 2;
            continue;
        }

        // Inline code span (`code` or ``code with `backtick` ``)
        if bytes[i] == b'`' {
            let ticks = bytes[i..].iter().take_while(|&&b| b == b'`').count();
            if let Some((content_end, consumed)) = find_code_span_end(rest, ticks) {
                flush(&mut buf, &mut out);
                let mut inner = &rest[ticks..content_end];
                // CommonMark §6.1: strip one matching leading & trailing space if not all spaces.
                if inner.len() >= 2
                    && inner.starts_with(' ')
                    && inner.ends_with(' ')
                    && inner.bytes().any(|b| b != b' ')
                {
                    inner = &inner[1..inner.len() - 1];
                }
                out.push(MdInlineNode::Code(inner.to_owned()));
                i += consumed;
                continue;
            }
            // Unmatched backtick run: emit literally so we don't rescan each backtick.
            buf.push_str(&rest[..ticks]);
            i += ticks;
            continue;
        }

        // Image syntax `![alt](url)` -> blocked inline placeholder (never auto-fetched!)
        if rest.starts_with("![")
            && let Some((alt, _url, consumed)) = parse_bracket_link(&rest[1..])
        {
            flush(&mut buf, &mut out);
            out.push(MdInlineNode::ImagePlaceholder {
                alt: alt.trim().to_owned(),
            });
            i += 1 + consumed;
            continue;
        }

        // Link `[label](href)` with nested inline formatting inside `label`
        if bytes[i] == b'['
            && let Some((label, href, consumed)) = parse_bracket_link(rest)
        {
            flush(&mut buf, &mut out);
            let children = parse_inlines_depth(label, depth + 1);
            out.push(MdInlineNode::Link {
                children,
                href: href.to_owned(),
            });
            i += consumed;
            continue;
        }

        // Bold `**...**`
        if rest.starts_with("**")
            && rest[2..].chars().next().is_some_and(|c| !c.is_whitespace())
            && let Some(close) = find_closing_delim(text, i + 2, "**", false)
        {
            flush(&mut buf, &mut out);
            let children = parse_inlines_depth(&text[i + 2..close], depth + 1);
            out.push(MdInlineNode::Bold(children));
            i = close + 2;
            continue;
        }

        // Word-bounded bold `__...__` (never triggers inside `foo__bar__`)
        if rest.starts_with("__")
            && text[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric())
            && rest[2..].chars().next().is_some_and(|c| !c.is_whitespace())
            && let Some(close) = find_closing_delim(text, i + 2, "__", true)
        {
            flush(&mut buf, &mut out);
            let children = parse_inlines_depth(&text[i + 2..close], depth + 1);
            out.push(MdInlineNode::Bold(children));
            i = close + 2;
            continue;
        }

        // Strikethrough `~~...~~`
        if rest.starts_with("~~")
            && rest[2..].chars().next().is_some_and(|c| !c.is_whitespace())
            && let Some(close) = find_closing_delim(text, i + 2, "~~", false)
        {
            flush(&mut buf, &mut out);
            let children = parse_inlines_depth(&text[i + 2..close], depth + 1);
            out.push(MdInlineNode::Strike(children));
            i = close + 2;
            continue;
        }

        // Italic `*...*` (not `**`, not followed by whitespace)
        if rest.starts_with('*')
            && !rest.starts_with("**")
            && rest[1..].chars().next().is_some_and(|c| !c.is_whitespace())
            && let Some(close) = find_closing_delim(text, i + 1, "*", false)
        {
            flush(&mut buf, &mut out);
            let children = parse_inlines_depth(&text[i + 1..close], depth + 1);
            out.push(MdInlineNode::Italic(children));
            i = close + 1;
            continue;
        }

        // Word-bounded italic `_..._` (never triggers inside `pane_render_preview`)
        if rest.starts_with('_')
            && !rest.starts_with("__")
            && text[..i]
                .chars()
                .next_back()
                .is_none_or(|c| !c.is_alphanumeric())
            && rest[1..].chars().next().is_some_and(|c| !c.is_whitespace())
            && let Some(close) = find_closing_delim(text, i + 1, "_", true)
        {
            flush(&mut buf, &mut out);
            let children = parse_inlines_depth(&text[i + 1..close], depth + 1);
            out.push(MdInlineNode::Italic(children));
            i = close + 1;
            continue;
        }

        if let Some(ch) = rest.chars().next() {
            buf.push(ch);
            i += ch.len_utf8();
        } else {
            break;
        }
    }

    flush(&mut buf, &mut out);
    out
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
    link_color: Hsla,
    muted_color: Hsla,
}

#[derive(Clone, Default)]
struct ActiveInlineStyle {
    bold: bool,
    italic: bool,
    strike: bool,
    code: bool,
    muted_placeholder: bool,
    link: Option<String>,
}

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
    let mut out = CompiledInlineText {
        text: String::with_capacity(src.len()),
        highlights: Vec::new(),
        font_family_overrides: Vec::new(),
        code_ranges: Vec::new(),
        link_ranges: Vec::new(),
    };
    let ctx = InlineCompileContext {
        base_weight,
        code_color,
        link_color,
        muted_color,
    };
    let style = ActiveInlineStyle::default();
    emit_inline_nodes(&nodes, &style, ctx, mono_font, resolve_link, &mut out);
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
            MdInlineNode::Link { children, href } => {
                let resolved = resolve_safe_url(href, resolve_link);
                let mut next = active.clone();
                next.link = resolved;
                emit_inline_nodes(children, &next, ctx, mono_font, resolve_link, out);
            }
            MdInlineNode::ImagePlaceholder { alt } => {
                let caption = if alt.is_empty() {
                    "[Image not shown]".to_owned()
                } else {
                    format!("[Image not shown: {alt}]")
                };
                let mut next = active.clone();
                next.muted_placeholder = true;
                push_compiled_leaf(&caption, &next, ctx, mono_font, out);
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
    if active.italic || active.muted_placeholder {
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
// Selection State & Pointer / Keyboard / Context-Menu Handling
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

struct MarkdownSelectionState {
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

impl Focusable for MarkdownSelectionState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl MarkdownSelectionState {
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
                copy_entity.update(cx, |s, cx| {
                    s.copy_selection(cx);
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
                    entity.update(cx, |s, _| {
                        s.select_all();
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

/// Wrapper around the Markdown block column that clears per-frame segment hitboxes before children
/// paint and captures window-wide drag tracking while actively selecting text.
struct MarkdownSelectionViewportElement {
    child: AnyElement,
    rendered_segments: Rc<RefCell<Vec<RenderedSegment>>>,
    viewport_bounds: Rc<Cell<Bounds<Pixels>>>,
    state: Entity<MarkdownSelectionState>,
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

/// Renders one single-flow [`StyledText`] Markdown block with:
/// - compact glyph-metric-aligned rounded inline-code pills (zero injected spaces),
/// - multi-line wrapped & newline-aware selection highlight quads,
/// - dynamic `IBeam` / `PointingHand` cursor switching over links,
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
        let over_link_now = if !self.links.is_empty() && hitbox.is_hovered(window) {
            layout
                .index_for_position(window.mouse_position())
                .ok()
                .is_some_and(|ix| self.links.iter().any(|(r, _)| r.contains(&ix)))
        } else {
            false
        };

        if over_link_now {
            window.set_cursor_style(CursorStyle::PointingHand, hitbox);
        } else if self.selectable {
            window.set_cursor_style(CursorStyle::IBeam, hitbox);
        }

        // Refresh only on link hover boundary transitions so cursor switches immediately without
        // causing per-pixel repaints during normal mouse movement.
        if !self.links.is_empty() && !self.is_selecting {
            let links_for_hover = self.links.clone();
            let layout_for_hover = layout.clone();
            let hitbox_for_hover = hitbox.clone();
            let was_over_link = Cell::new(over_link_now);
            window.on_mouse_event(move |event: &MouseMoveEvent, phase, window, _| {
                if phase == DispatchPhase::Bubble {
                    let is_over = hitbox_for_hover.is_hovered(window)
                        && layout_for_hover
                            .index_for_position(event.position)
                            .ok()
                            .is_some_and(|ix| links_for_hover.iter().any(|(r, _)| r.contains(&ix)));
                    if is_over != was_over_link.get() {
                        was_over_link.set(is_over);
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

        // Skip lines with no overlapping inline-code ranges.
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
// Document Preparation & Rendering
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
        compiled: CompiledInlineText,
        doc_range: Range<usize>,
    },
    List {
        ordered: bool,
        items: Vec<(Option<bool>, CompiledInlineText, Range<usize>)>,
    },
    Table {
        headers: Vec<String>,
        rows: Vec<Vec<(CompiledInlineText, Range<usize>)>>,
    },
    CodeBlock {
        language: String,
        raw_code: String,
        lines: Vec<(String, Range<usize>)>,
    },
    ComponentBlock {
        name: String,
        json_body: String,
    },
    Rule,
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

struct TextRenderCtx {
    code_pill_bg: Hsla,
    code_pill_border: Hsla,
    selection: Range<usize>,
    selection_color: Hsla,
    selectable: bool,
    is_selecting: bool,
    on_open_link: Option<LinkOpenHandler>,
    rendered_segments: Option<Rc<RefCell<Vec<RenderedSegment>>>>,
    viewport_bounds: Option<Rc<Cell<Bounds<Pixels>>>>,
}

impl TextRenderCtx {
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
            rendered_segments: self.rendered_segments.clone(),
            viewport_bounds: self.viewport_bounds.clone(),
        }
    }

    fn plain_code_line(
        &self,
        line: String,
        doc_range: Range<usize>,
        ends_with_newline: bool,
    ) -> MarkdownTextElement {
        let text = SharedString::from(line);
        let styled_text = StyledText::new(text.clone());
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
            rendered_segments: self.rendered_segments.clone(),
            viewport_bounds: self.viewport_bounds.clone(),
        }
    }
}

impl RenderOnce for Markdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let split = split_open_component(self.source.as_ref(), self.streaming);
        let blocks = parse_blocks(split.shown);
        let id_root = self.id;
        let registry = self.components;
        let mono_font = theme.mono_font_family.as_ref();

        // Phase 1: Compile inline ASTs into single-flow StyledText payloads & unified doc_text.
        let mut doc_text = String::new();
        let mut prepared = Vec::with_capacity(blocks.len());

        for block in blocks {
            match block {
                MdBlock::Heading { level, text } => {
                    push_sep(&mut doc_text, "\n\n");
                    let compiled = compile_inline_text(
                        &text,
                        FontWeight::SEMIBOLD,
                        theme.foreground,
                        theme.primary,
                        theme.muted_foreground,
                        mono_font,
                        &self.resolve_link,
                    );
                    let doc_range = append_doc_slice(&mut doc_text, &compiled.text);
                    prepared.push(PreparedBlock::Heading {
                        level,
                        compiled,
                        doc_range,
                    });
                }
                MdBlock::Paragraph(text) => {
                    push_sep(&mut doc_text, "\n\n");
                    let compiled = compile_inline_text(
                        &text,
                        FontWeight::NORMAL,
                        theme.foreground,
                        theme.primary,
                        theme.muted_foreground,
                        mono_font,
                        &self.resolve_link,
                    );
                    let doc_range = append_doc_slice(&mut doc_text, &compiled.text);
                    prepared.push(PreparedBlock::Paragraph {
                        compiled,
                        doc_range,
                    });
                }
                MdBlock::Blockquote(text) => {
                    push_sep(&mut doc_text, "\n\n");
                    let compiled = compile_inline_text(
                        &text,
                        FontWeight::NORMAL,
                        theme.foreground,
                        theme.primary,
                        theme.muted_foreground,
                        mono_font,
                        &self.resolve_link,
                    );
                    let doc_range = append_doc_slice(&mut doc_text, &compiled.text);
                    prepared.push(PreparedBlock::Blockquote {
                        compiled,
                        doc_range,
                    });
                }
                MdBlock::List { ordered, items } => {
                    push_sep(&mut doc_text, "\n\n");
                    let mut prep_items = Vec::with_capacity(items.len());
                    for (idx, item) in items.into_iter().enumerate() {
                        if idx > 0 {
                            doc_text.push('\n');
                        }
                        let compiled = compile_inline_text(
                            &item.text,
                            FontWeight::NORMAL,
                            theme.foreground,
                            theme.primary,
                            theme.muted_foreground,
                            mono_font,
                            &self.resolve_link,
                        );
                        let doc_range = append_doc_slice(&mut doc_text, &compiled.text);
                        prep_items.push((item.checked, compiled, doc_range));
                    }
                    prepared.push(PreparedBlock::List {
                        ordered,
                        items: prep_items,
                    });
                }
                MdBlock::Table { headers, rows } => {
                    push_sep(&mut doc_text, "\n\n");
                    let compiled_headers: Vec<String> = headers
                        .into_iter()
                        .map(|h| {
                            compile_inline_text(
                                &h,
                                FontWeight::SEMIBOLD,
                                theme.foreground,
                                theme.primary,
                                theme.muted_foreground,
                                mono_font,
                                &self.resolve_link,
                            )
                            .text
                        })
                        .collect();
                    let mut prep_rows = Vec::with_capacity(rows.len());
                    for (r_idx, row) in rows.into_iter().enumerate() {
                        if r_idx > 0 {
                            doc_text.push('\n');
                        }
                        let mut prep_cells = Vec::with_capacity(row.len());
                        for (c_idx, cell) in row.into_iter().enumerate() {
                            if c_idx > 0 {
                                doc_text.push('\t');
                            }
                            let compiled = compile_inline_text(
                                &cell,
                                FontWeight::NORMAL,
                                theme.foreground,
                                theme.primary,
                                theme.muted_foreground,
                                mono_font,
                                &self.resolve_link,
                            );
                            let doc_range = append_doc_slice(&mut doc_text, &compiled.text);
                            prep_cells.push((compiled, doc_range));
                        }
                        prep_rows.push(prep_cells);
                    }
                    prepared.push(PreparedBlock::Table {
                        headers: compiled_headers,
                        rows: prep_rows,
                    });
                }
                MdBlock::CodeBlock { language, code } => {
                    push_sep(&mut doc_text, "\n\n");
                    let mut prep_lines = Vec::new();
                    for (ln, line_str) in code.split('\n').enumerate() {
                        if ln > 0 {
                            doc_text.push('\n');
                        }
                        let doc_range = append_doc_slice(&mut doc_text, line_str);
                        prep_lines.push((line_str.to_owned(), doc_range));
                    }
                    prepared.push(PreparedBlock::CodeBlock {
                        language,
                        raw_code: code,
                        lines: prep_lines,
                    });
                }
                MdBlock::ComponentBlock { name, json_body } => {
                    prepared.push(PreparedBlock::ComponentBlock { name, json_body });
                }
                MdBlock::Rule => {
                    prepared.push(PreparedBlock::Rule);
                }
            }
        }

        // Phase 2: Sync keyed selection state when selectable.
        let sel_state = if self.selectable {
            let state = window.use_keyed_state(
                SharedString::from(format!("{id_root}-selection")),
                cx,
                |window, cx| MarkdownSelectionState::new(window, cx),
            );
            state.update(cx, |s, _| {
                s.sync_document(doc_text, self.on_open_link.clone());
            });
            Some(state)
        } else {
            None
        };

        let (selected_range, is_selecting, rendered_segments, viewport_bounds) =
            if let Some(ref state) = sel_state {
                let s = state.read(cx);
                (
                    s.selected_range.clone(),
                    s.is_selecting,
                    Some(s.rendered_segments.clone()),
                    Some(s.viewport_bounds.clone()),
                )
            } else {
                (0..0, false, None, None)
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

        let render_ctx = TextRenderCtx {
            code_pill_bg,
            code_pill_border,
            selection: selected_range,
            selection_color: theme.selection,
            selectable: self.selectable,
            is_selecting,
            on_open_link: self.on_open_link.clone(),
            rendered_segments: rendered_segments.clone(),
            viewport_bounds: viewport_bounds.clone(),
        };

        // Phase 3: Build visual element tree.
        let mut col = div().w_full().flex().flex_col().gap(px(10.));
        let block_count = prepared.len();

        for (idx, block) in prepared.into_iter().enumerate() {
            let block_id = format!("{id_root}-b{idx}");
            let has_following = idx + 1 < block_count;
            match block {
                PreparedBlock::Heading {
                    level,
                    compiled,
                    doc_range,
                } => {
                    let (sz, lh) = match level {
                        1 => (px(18.), px(25.)),
                        2 => (px(16.), px(23.)),
                        3 => (px(14.5), px(21.)),
                        _ => (px(13.5), px(20.)),
                    };
                    col = col.child(
                        div()
                            .w_full()
                            .pt(px(2.))
                            .text_size(sz)
                            .line_height(lh)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(render_ctx.inline_element(compiled, doc_range, has_following)),
                    );
                }
                PreparedBlock::Paragraph {
                    compiled,
                    doc_range,
                } => {
                    col = col.child(
                        div()
                            .w_full()
                            .text_size(px(13.5))
                            .line_height(px(21.))
                            .text_color(theme.foreground)
                            .child(render_ctx.inline_element(compiled, doc_range, has_following)),
                    );
                }
                PreparedBlock::Rule => {
                    col = col.child(div().w_full().h(px(1.)).my(px(4.)).bg(theme.border));
                }
                PreparedBlock::Blockquote {
                    compiled,
                    doc_range,
                } => {
                    col = col.child(
                        div()
                            .w_full()
                            .pl(px(12.))
                            .py(px(4.))
                            .border_l_2()
                            .border_color(theme.primary.opacity(0.6))
                            .text_size(px(13.))
                            .line_height(px(20.))
                            .text_color(theme.muted_foreground)
                            .child(render_ctx.inline_element(compiled, doc_range, has_following)),
                    );
                }
                PreparedBlock::List { ordered, items } => {
                    let item_count = items.len();
                    let mut list_col = div().w_full().flex().flex_col().gap(px(5.)).pl(px(4.));
                    for (item_i, (checked, compiled, doc_range)) in items.into_iter().enumerate() {
                        let marker = if let Some(done) = checked {
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
                        } else if ordered {
                            div()
                                .min_w(px(16.))
                                .text_size(px(12.5))
                                .line_height(px(21.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child(format!("{}.", item_i + 1))
                                .into_any_element()
                        } else {
                            div()
                                .w(px(5.))
                                .h(px(5.))
                                .mt(px(8.))
                                .rounded_full()
                                .bg(theme.muted_foreground)
                                .into_any_element()
                        };
                        let item_nl = item_i + 1 < item_count || has_following;
                        list_col = list_col.child(
                            div()
                                .w_full()
                                .flex()
                                .flex_row()
                                .items_start()
                                .gap(px(8.))
                                .child(div().flex_shrink_0().child(marker))
                                .child(
                                    div()
                                        .flex_1()
                                        .min_w_0()
                                        .text_size(px(13.5))
                                        .line_height(px(21.))
                                        .text_color(theme.foreground)
                                        .child(render_ctx.inline_element(
                                            compiled,
                                            doc_range,
                                            item_nl,
                                        )),
                                ),
                        );
                    }
                    col = col.child(list_col);
                }
                PreparedBlock::Table { headers, rows } => {
                    let cols: Vec<TableColumn> =
                        headers.into_iter().map(TableColumn::new).collect();
                    let row_count = rows.len();
                    let t_rows: Vec<TableRow> = rows
                        .into_iter()
                        .enumerate()
                        .map(|(r_i, r)| {
                            let mut tr = TableRow::new();
                            let cell_count = r.len();
                            for (c_i, (compiled, doc_range)) in r.into_iter().enumerate() {
                                let ends_nl = c_i + 1 == cell_count
                                    && (r_i + 1 < row_count || has_following);
                                tr = tr.cell(
                                    div()
                                        .w_full()
                                        .text_size(px(13.))
                                        .line_height(px(19.))
                                        .text_color(theme.foreground)
                                        .child(render_ctx.inline_element(
                                            compiled,
                                            doc_range,
                                            ends_nl,
                                        )),
                                );
                            }
                            tr
                        })
                        .collect();
                    col = col.child(
                        Table::new(SharedString::from(format!("{block_id}-tbl")), cols)
                            .compact(true)
                            .striped(true)
                            .rows(t_rows),
                    );
                }
                PreparedBlock::CodeBlock {
                    language,
                    raw_code,
                    lines,
                } => {
                    let lang_label = if language.is_empty() {
                        "TEXT".to_owned()
                    } else {
                        language.to_ascii_uppercase()
                    };
                    let line_count = lines.len();
                    let digits = ((line_count.max(1) as f32).log10().floor() as usize) + 1;
                    let gutter_w = px((digits.max(2) as f32 * 8.0 + 4.0).ceil());

                    let mut gutter_col = div()
                        .w(gutter_w)
                        .flex_shrink_0()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_right()
                        .text_color(theme.muted_foreground.opacity(0.65));

                    let mut code_lines_col = div()
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(theme.foreground);

                    if let Some(mono) = theme.mono_font_family.clone() {
                        gutter_col = gutter_col.font_family(mono.clone());
                        code_lines_col = code_lines_col.font_family(mono);
                    }

                    for (ln, (line_str, doc_range)) in lines.into_iter().enumerate() {
                        gutter_col = gutter_col.child(
                            div()
                                .min_h(px(18.))
                                .whitespace_nowrap()
                                .child(format!("{}", ln + 1)),
                        );
                        let line_nl = ln + 1 < line_count || has_following;
                        code_lines_col = code_lines_col.child(
                            div()
                                .min_h(px(18.))
                                .whitespace_nowrap()
                                .child(render_ctx.plain_code_line(line_str, doc_range, line_nl)),
                        );
                    }

                    let code_scroll = div()
                        .id(SharedString::from(format!("{block_id}-code-scroll")))
                        .flex_1()
                        .min_w_0()
                        .overflow_x_scroll()
                        .child(code_lines_col);

                    col = col.child(
                        div()
                            .w_full()
                            .rounded(theme.radius_md)
                            .bg(theme.surface_muted.opacity(0.65))
                            .border_1()
                            .border_color(theme.border)
                            .overflow_hidden()
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .px(px(10.))
                                    .py(px(5.))
                                    .border_b_1()
                                    .border_color(theme.border)
                                    .bg(theme.surface)
                                    .cursor_default()
                                    .on_mouse_down(MouseButton::Left, |_, _, cx| {
                                        cx.stop_propagation();
                                    })
                                    .child(badge(lang_label).color(BadgeColor::Gray))
                                    .child(copy_button(
                                        SharedString::from(format!("{block_id}-copy")),
                                        raw_code,
                                    )),
                            )
                            .child(
                                div()
                                    .w_full()
                                    .p(px(10.))
                                    .flex()
                                    .flex_row()
                                    .items_start()
                                    .gap(px(12.))
                                    .child(gutter_col)
                                    .child(code_scroll),
                            ),
                    );
                }
                PreparedBlock::ComponentBlock { name, json_body } => {
                    let rendered = match registry.as_ref().and_then(|r| r.get(&name)) {
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
                    col = col.child(
                        div()
                            .w_full()
                            .cursor_default()
                            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                            .child(rendered),
                    );
                }
            }
        }

        if let Some(pending) = split.pending_component {
            let label = registry
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

        let Some(state) = sel_state else {
            crate::Refineable::refine(col.style(), &self.style);
            return col.into_any_element();
        };

        let (focus_handle, menu_pos, menu_entries) = {
            let s = state.read(cx);
            let entries = if s.context_menu_position.is_some() {
                s.build_context_menu_entries(state.clone())
            } else {
                Vec::new()
            };
            (s.focus_handle.clone(), s.context_menu_position, entries)
        };

        let state_close = state.clone();
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
            state: state.clone(),
            is_selecting,
        };

        let state_left = state.clone();
        let state_right = state.clone();
        let state_key = state.clone();
        let state_copy = state.clone();
        let state_all = state.clone();
        let state_clear = state;

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

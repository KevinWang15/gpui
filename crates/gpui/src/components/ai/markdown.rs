//! Pure-GPUI GitHub-Flavored [`Markdown`] renderer with embedded generative
//! [`AnswerComponentRegistry`] blocks and security hardening ported from `byok-ai`:
//!
//! - Renders headings, paragraphs, inline **bold**, *italic*, ~~strikethrough~~, `` `code` ``,
//!   clickable `[links](url)` (filtering `javascript:` / `data:` schemes with optional
//!   `resolve_link` and `on_open_link` hooks), blockquotes, ordered/unordered/GFM task lists,
//!   tables, horizontal rules, and fenced code blocks with language badge, line numbers, and copy.
//! - **Prompt-injection safe**: raw HTML is displayed as plain text, and external Markdown images
//!   `![alt](url)` are never fetched automatically (rendered as safe placeholders).
//! - **Streaming Answer Components**: detects unclosed ```` ```component:<name> ```` fences via
//!   [`split_open_component`] and displays a pulsing `<label>…` placeholder until the fence closes.

use std::rc::Rc;

use serde_json::Value;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, FontStyle, FontWeight, IntoElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};

use super::answer_components::{AnswerComponentRegistry, split_open_component};
use crate::components::badge::{BadgeColor, badge};
use crate::components::interactive::copy_button;
use crate::components::table::{Table, TableColumn, TableRow};
use crate::components::theme::ActiveTheme;

/// Optional URL resolver for Markdown links (return `None` to render the link as plain text).
pub type LinkResolver = Rc<dyn Fn(&str) -> Option<String> + 'static>;

/// Optional click handler for Markdown links (`href`, `window`, `cx`).
pub type LinkOpenHandler = Rc<dyn Fn(&str, &mut Window, &mut App) + 'static>;

/// Pure-GPUI GitHub-Flavored Markdown & Answer Component element.
#[derive(IntoElement)]
pub struct Markdown {
    id: SharedString,
    source: SharedString,
    streaming: bool,
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
    /// Creates a new [`Markdown`] element.
    pub fn new(id: impl Into<SharedString>, source: impl Into<SharedString>) -> Self {
        Self {
            id: id.into(),
            source: source.into(),
            streaming: false,
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
    pub fn on_open_link(
        mut self,
        f: impl Fn(&str, &mut Window, &mut App) + 'static,
    ) -> Self {
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
// Block & Inline Markdown Parser
// ---------------------------------------------------------------------------

#[derive(Debug, PartialEq, Eq)]
enum MdBlock {
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
struct MdListItem {
    checked: Option<bool>,
    text: String,
}

#[derive(Debug, PartialEq, Eq)]
enum MdInline {
    Text(String),
    Bold(String),
    Italic(String),
    Strike(String),
    Code(String),
    Link { label: String, href: String },
    ImagePlaceholder { alt: String },
}

fn parse_blocks(src: &str) -> Vec<MdBlock> {
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

        // Unordered / Task / Ordered list
        if let Some((ordered, first_item)) = parse_list_item(trimmed) {
            let mut items = vec![first_item];
            i += 1;
            while i < lines.len() {
                let l = lines[i].trim_end_matches('\r').trim();
                if let Some((next_ord, item)) = parse_list_item(l)
                    && next_ord == ordered
                {
                    items.push(item);
                    i += 1;
                } else {
                    break;
                }
            }
            blocks.push(MdBlock::List { ordered, items });
            continue;
        }

        // Paragraph (accumulate consecutive plain lines)
        let mut para = vec![trimmed];
        i += 1;
        while i < lines.len() {
            let l = lines[i].trim_end_matches('\r').trim();
            if l.is_empty()
                || parse_fence_open(l).is_some()
                || parse_heading(l).is_some()
                || l.starts_with('>')
                || is_horizontal_rule(l)
                || parse_list_item(l).is_some()
            {
                break;
            }
            para.push(l);
            i += 1;
        }
        blocks.push(MdBlock::Paragraph(para.join(" ")));
    }

    blocks
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
            return Some((hashes as u8, rest.trim().trim_end_matches('#').trim().to_owned()));
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
            let (checked, body) = if let Some(r) = rest.strip_prefix("[x] ").or_else(|| rest.strip_prefix("[X] ")) {
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

fn parse_inlines(text: &str) -> Vec<MdInline> {
    let mut out = Vec::new();
    let mut buf = String::new();
    let bytes = text.as_bytes();
    let mut i = 0;

    let flush = |buf: &mut String, out: &mut Vec<MdInline>| {
        if !buf.is_empty() {
            out.push(MdInline::Text(std::mem::take(buf)));
        }
    };

    while i < bytes.len() {
        let rest = &text[i..];

        // Image syntax `![alt](url)` -> blocked placeholder (never auto-fetched!)
        if rest.starts_with("![")
            && let Some(close_alt) = rest[2..].find("](")
        {
            let after_alt = &rest[2 + close_alt + 2..];
            if let Some(close_url) = after_alt.find(')') {
                flush(&mut buf, &mut out);
                let alt = rest[2..2 + close_alt].trim().to_owned();
                out.push(MdInline::ImagePlaceholder { alt });
                i += 2 + close_alt + 2 + close_url + 1;
                continue;
            }
        }

        // Link `[label](href)`
        if rest.starts_with('[')
            && let Some(close_lbl) = rest[1..].find("](")
        {
            let after_lbl = &rest[1 + close_lbl + 2..];
            if let Some(close_url) = after_lbl.find(')') {
                flush(&mut buf, &mut out);
                let label = rest[1..1 + close_lbl].to_owned();
                let href = after_lbl[..close_url].trim().to_owned();
                out.push(MdInline::Link { label, href });
                i += 1 + close_lbl + 2 + close_url + 1;
                continue;
            }
        }

        // Inline code `` `code` ``
        if rest.starts_with('`')
            && let Some(end) = rest[1..].find('`')
        {
            flush(&mut buf, &mut out);
            out.push(MdInline::Code(rest[1..1 + end].to_owned()));
            i += 1 + end + 1;
            continue;
        }

        // Bold `**text**`
        if rest.starts_with("**")
            && let Some(end) = rest[2..].find("**")
            && end > 0
        {
            flush(&mut buf, &mut out);
            out.push(MdInline::Bold(rest[2..2 + end].to_owned()));
            i += 2 + end + 2;
            continue;
        }

        // Strikethrough `~~text~~`
        if rest.starts_with("~~")
            && let Some(end) = rest[2..].find("~~")
            && end > 0
        {
            flush(&mut buf, &mut out);
            out.push(MdInline::Strike(rest[2..2 + end].to_owned()));
            i += 2 + end + 2;
            continue;
        }

        // Italic `*text*`
        if rest.starts_with('*')
            && !rest.starts_with("**")
            && let Some(end) = rest[1..].find('*')
            && end > 0
        {
            flush(&mut buf, &mut out);
            out.push(MdInline::Italic(rest[1..1 + end].to_owned()));
            i += 1 + end + 1;
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

fn render_inline_flow(
    id_prefix: &str,
    text: &str,
    resolve_link: &Option<LinkResolver>,
    on_open_link: &Option<LinkOpenHandler>,
    cx: &App,
) -> AnyElement {
    let theme = cx.theme();
    let runs = parse_inlines(text);

    // Fast path: plain text with no formatting wraps natively in one GPUI text node.
    if runs.len() == 1
        && let MdInline::Text(ref plain) = runs[0]
    {
        return div()
            .text_size(px(13.5))
            .line_height(px(21.))
            .text_color(theme.foreground)
            .child(plain.clone())
            .into_any_element();
    }

    let mut row = div()
        .flex()
        .flex_row()
        .flex_wrap()
        .items_baseline()
        .gap_x(px(0.))
        .text_size(px(13.5))
        .line_height(px(21.))
        .text_color(theme.foreground);

    for (idx, run) in runs.into_iter().enumerate() {
        match run {
            MdInline::Text(s) => {
                row = row.child(div().child(s));
            }
            MdInline::Bold(s) => {
                row = row.child(div().font_weight(FontWeight::SEMIBOLD).child(s));
            }
            MdInline::Italic(s) => {
                row = row.child(div().italic().child(s));
            }
            MdInline::Strike(s) => {
                row = row.child(
                    div()
                        .line_through()
                        .text_color(theme.muted_foreground)
                        .child(s),
                );
            }
            MdInline::Code(code) => {
                let mut chip = div()
                    .px(px(5.))
                    .py(px(1.))
                    .mx(px(1.))
                    .rounded(theme.radius_sm)
                    .bg(theme.surface_muted)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(12.))
                    .text_color(theme.foreground)
                    .child(code);
                if let Some(mono) = theme.mono_font_family.clone() {
                    chip = chip.font_family(mono);
                }
                row = row.child(chip);
            }
            MdInline::Link { label, href } => {
                let resolved = if !is_safe_link_url(&href) {
                    None
                } else if let Some(resolver) = resolve_link {
                    resolver(&href).filter(|u| is_safe_link_url(u))
                } else {
                    Some(href)
                };
                if let Some(target_url) = resolved {
                    let on_open = on_open_link.clone();
                    let link_id = SharedString::from(format!("{id_prefix}-lnk-{idx}"));
                    row = row.child(
                        div()
                            .id(link_id)
                            .text_color(theme.primary)
                            .underline()
                            .cursor_pointer()
                            .on_click(move |_, window, cx| {
                                if let Some(ref handler) = on_open {
                                    handler(&target_url, window, cx);
                                } else {
                                    cx.open_url(&target_url);
                                }
                            })
                            .child(label),
                    );
                } else {
                    row = row.child(div().child(label));
                }
            }
            MdInline::ImagePlaceholder { alt } => {
                let caption = if alt.is_empty() {
                    "Image not shown".to_owned()
                } else {
                    format!("Image not shown: {alt}")
                };
                row = row.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .px(px(6.))
                        .py(px(2.))
                        .rounded(theme.radius_sm)
                        .border_1()
                        .border_dashed()
                        .border_color(theme.border)
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .child(
                            Icon::new(LucideIcon::ImageOff)
                                .size(IconSize::Xs)
                                .color(theme.muted_foreground),
                        )
                        .child(caption),
                );
            }
        }
    }

    row.into_any_element()
}

impl RenderOnce for Markdown {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let split = split_open_component(self.source.as_ref(), self.streaming);
        let blocks = parse_blocks(split.shown);
        let id_root = self.id;
        let registry = self.components;

        let mut col = div().w_full().flex().flex_col().gap(px(10.));

        for (idx, block) in blocks.into_iter().enumerate() {
            let block_id = format!("{id_root}-b{idx}");
            match block {
                MdBlock::Heading { level, text } => {
                    let sz = match level {
                        1 => px(18.),
                        2 => px(16.),
                        3 => px(14.5),
                        _ => px(13.5),
                    };
                    col = col.child(
                        div()
                            .pt(px(2.))
                            .text_size(sz)
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(text),
                    );
                }
                MdBlock::Paragraph(text) => {
                    col = col.child(render_inline_flow(
                        &block_id,
                        &text,
                        &self.resolve_link,
                        &self.on_open_link,
                        cx,
                    ));
                }
                MdBlock::Rule => {
                    col = col.child(div().w_full().h(px(1.)).my(px(4.)).bg(theme.border));
                }
                MdBlock::Blockquote(text) => {
                    col = col.child(
                        div()
                            .w_full()
                            .pl(px(12.))
                            .py(px(4.))
                            .border_l_2()
                            .border_color(theme.primary.opacity(0.6))
                            .text_size(px(13.))
                            .text_color(theme.muted_foreground)
                            .child(text),
                    );
                }
                MdBlock::List { ordered, items } => {
                    let mut list_col = div().w_full().flex().flex_col().gap(px(5.)).pl(px(4.));
                    for (item_i, item) in items.into_iter().enumerate() {
                        let marker = if let Some(done) = item.checked {
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
                            })
                            .into_any_element()
                        } else if ordered {
                            div()
                                .text_size(px(12.5))
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
                        let item_id = format!("{block_id}-li{item_i}");
                        list_col = list_col.child(
                            div()
                                .flex()
                                .flex_row()
                                .items_start()
                                .gap(px(8.))
                                .child(div().flex_shrink_0().child(marker))
                                .child(div().flex_1().min_w_0().child(render_inline_flow(
                                    &item_id,
                                    &item.text,
                                    &self.resolve_link,
                                    &self.on_open_link,
                                    cx,
                                ))),
                        );
                    }
                    col = col.child(list_col);
                }
                MdBlock::Table { headers, rows } => {
                    let cols: Vec<TableColumn> =
                        headers.into_iter().map(TableColumn::new).collect();
                    let t_rows: Vec<TableRow> = rows
                        .into_iter()
                        .map(|r| {
                            let mut tr = TableRow::new();
                            for cell in r {
                                tr = tr.text(cell);
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
                MdBlock::CodeBlock { language, code } => {
                    let lang_label = if language.is_empty() {
                        "TEXT".to_owned()
                    } else {
                        language.to_ascii_uppercase()
                    };
                    let mut code_body = div()
                        .id(SharedString::from(format!("{block_id}-code-scroll")))
                        .w_full()
                        .overflow_x_scroll()
                        .p(px(10.))
                        .flex()
                        .flex_col()
                        .gap(px(2.))
                        .text_size(px(12.))
                        .line_height(px(18.))
                        .text_color(theme.foreground);
                    if let Some(mono) = theme.mono_font_family.clone() {
                        code_body = code_body.font_family(mono);
                    }
                    for (ln, line_str) in code.lines().enumerate() {
                        code_body = code_body.child(
                            div()
                                .flex()
                                .flex_row()
                                .gap(px(12.))
                                .child(
                                    div()
                                        .w(px(24.))
                                        .flex_shrink_0()
                                        .text_right()
                                        .text_color(theme.muted_foreground.opacity(0.65))
                                        .child(format!("{}", ln + 1)),
                                )
                                .child(div().flex_1().min_w_0().child(line_str.to_owned())),
                        );
                    }

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
                                    .child(badge(lang_label).color(BadgeColor::Gray))
                                    .child(copy_button(
                                        SharedString::from(format!("{block_id}-copy")),
                                        code,
                                    )),
                            )
                            .child(code_body),
                    );
                }
                MdBlock::ComponentBlock { name, json_body } => {
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
                    col = col.child(rendered);
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

        let _ = FontStyle::Normal;
        crate::Refineable::refine(col.style(), &self.style);
        col
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

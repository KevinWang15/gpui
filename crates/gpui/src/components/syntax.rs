//! Pure-Rust syntax highlighting engine, semantic [`SyntaxToken`] model, and adaptive light/dark
//! [`SyntaxTheme`] palettes shared by [`Markdown`](super::Markdown) and [`CodeBlock`](super::CodeBlock).
//!
//! Powered by `syntect` (`fancy-regex`, zero C/C++ dependencies) and `two-face`'s extended grammar
//! bundle (~200+ languages including Rust, TypeScript, TSX/JSX, Python, Go, SQL, TOML, YAML, JSON,
//! Bash, Diff, Dockerfile, WGSL, HTML, CSS, C/C++, Swift, Kotlin, and more) behind the default-on
//! `syntax-highlighting` Cargo feature.
//!
//! Highlighting produces theme-independent [`HighlightedLine`]s (byte ranges tagged with semantic
//! [`SyntaxToken`] variants), so switching between light and dark themes requires zero re-lexing,
//! and repeated frames hit an internal bounded cache in O(1).

use std::collections::{HashMap, VecDeque};
use std::ops::Range;
use std::sync::{Arc, Mutex, OnceLock};

use crate::{FontStyle, FontWeight, HighlightStyle, Hsla, SharedString, UnderlineStyle, px, rgb};

/// Maximum single-line byte length fed into regex grammars; longer minified lines fall back to
/// plain text so pathological inputs can never stall UI layout.
const MAX_HIGHLIGHT_LINE_BYTES: usize = 8192;

/// Maximum number of highlighted code blocks retained in the global bounded cache.
const GLOBAL_CACHE_CAPACITY: usize = 256;

/// Semantic syntax token categories mapped from TextMate scopes (or custom highlighters) onto
/// [`SyntaxTheme`] colors.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SyntaxToken {
    /// Default unhighlighted source code text.
    #[default]
    Plain,
    /// Line, block, or documentation comments.
    Comment,
    /// Language keywords, control flow, and storage declarations (`fn`, `let`, `if`, `async`, `class`).
    Keyword,
    /// Symbolic operators (`+`, `->`, `=>`, `::`, `&&`, `=`).
    Operator,
    /// Structural punctuation (`(`, `)`, `{`, `}`, `,`, `;`).
    Punctuation,
    /// String, character, or regex literals.
    String,
    /// Escape sequences (`\n`, `\"`) and interpolation delimiters (`${`, `}`).
    Escape,
    /// Numeric literals (`42`, `3.14`, `0xff`).
    Number,
    /// Language boolean / nullish literals (`true`, `false`, `null`, `None`).
    Boolean,
    /// Named constants, enum variants, and built-in symbols.
    Constant,
    /// Function and method definitions or invocations.
    Function,
    /// Macros (`println!`), preprocessor directives (`#include`), and decorators (`@route`).
    Macro,
    /// Types, structs, traits, classes, interfaces, and primitives (`usize`, `String`).
    Type,
    /// Parameters, `self`/`this`, and language-special variables.
    Variable,
    /// Object fields, struct members, JSON/YAML/TOML keys, and CSS properties.
    Property,
    /// HTML/JSX attributes and compiler attributes (`#[derive(...)]`).
    Attribute,
    /// HTML, XML, and JSX element tags (`<div>`).
    Tag,
    /// Lifetimes (`'a`) and loop/jump labels.
    Label,
    /// Inserted diff text (`+ ...`).
    DiffInserted,
    /// Deleted diff text (`- ...`).
    DiffDeleted,
    /// Diff file and hunk headers (`@@ ... @@`, `---`, `+++`).
    DiffHeader,
    /// Markup headings (inside fenced Markdown/AsciiDoc blocks).
    Heading,
    /// Markup italic emphasis.
    Emphasis,
    /// Markup strong/bold emphasis.
    Strong,
    /// Hyperlinks / URLs inside markup or comments.
    Link,
    /// Invalid or deprecated syntax tokens.
    Invalid,
}

/// Optional diff row classification used for subtle full-line background tinting in `diff` blocks.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum DiffLineKind {
    /// Added line (`+...`).
    Inserted,
    /// Removed line (`-...`).
    Deleted,
    /// Hunk or file header (`@@ ... @@`, `diff --git ...`).
    HunkHeader,
}

/// A single source line paired with strictly sorted, non-overlapping semantic [`SyntaxToken`] spans.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct HighlightedLine {
    /// Line text without trailing `\n` or `\r`.
    pub text: SharedString,
    /// Byte ranges within `text` tagged with semantic tokens (omits `SyntaxToken::Plain`).
    pub spans: Vec<(Range<usize>, SyntaxToken)>,
    /// Optional diff line tinting classification for `diff` / `patch` blocks.
    pub diff_kind: Option<DiffLineKind>,
}

impl HighlightedLine {
    /// Constructs an unhighlighted plain line.
    pub fn plain(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            spans: Vec::new(),
            diff_kind: None,
        }
    }

    /// Converts this line's semantic token spans into GPUI [`HighlightStyle`] runs for `theme`,
    /// coalescing adjacent spans with identical styles and guaranteeing valid UTF-8 boundaries.
    pub fn to_highlights(&self, theme: &SyntaxTheme) -> Vec<(Range<usize>, HighlightStyle)> {
        if self.spans.is_empty() || self.text.is_empty() {
            return Vec::new();
        }
        let len = self.text.len();
        let mut out: Vec<(Range<usize>, HighlightStyle)> = Vec::with_capacity(self.spans.len());
        let mut prev_end = 0usize;

        for (range, token) in &self.spans {
            let Some(style) = theme.style_for(*token) else {
                continue;
            };
            let start = range.start.max(prev_end).min(len);
            let end = range.end.min(len);
            if start >= end
                || !self.text.is_char_boundary(start)
                || !self.text.is_char_boundary(end)
            {
                continue;
            }
            if let Some((last_range, last_style)) = out.last_mut()
                && last_range.end == start
                && *last_style == style
            {
                last_range.end = end;
            } else {
                out.push((start..end, style));
            }
            prev_end = end;
        }
        out
    }
}

/// Complete light or dark syntax highlighting color palette attached to [`Theme`](super::Theme).
#[derive(Clone, Debug, PartialEq)]
pub struct SyntaxTheme {
    /// Code block body surface background.
    pub code_bg: Hsla,
    /// Code block header bar background.
    pub code_header_bg: Hsla,
    /// Code block outer border and header divider color.
    pub code_border: Hsla,
    /// Non-selectable line number gutter ink.
    pub gutter_foreground: Hsla,
    /// Subtle full-row background tint for inserted (`+`) diff lines.
    pub diff_inserted_bg: Hsla,
    /// Subtle full-row background tint for deleted (`-`) diff lines.
    pub diff_deleted_bg: Hsla,
    /// Subtle full-row background tint for diff hunk headers (`@@`).
    pub diff_header_bg: Hsla,
    /// Default code foreground (`SyntaxToken::Plain`).
    pub plain: Hsla,
    /// Comment ink (`SyntaxToken::Comment`).
    pub comment: Hsla,
    /// Keyword ink (`SyntaxToken::Keyword`).
    pub keyword: Hsla,
    /// Operator ink (`SyntaxToken::Operator`).
    pub operator: Hsla,
    /// Punctuation ink (`SyntaxToken::Punctuation`).
    pub punctuation: Hsla,
    /// String literal ink (`SyntaxToken::String`).
    pub string: Hsla,
    /// Escape & interpolation ink (`SyntaxToken::Escape`).
    pub escape: Hsla,
    /// Number literal ink (`SyntaxToken::Number`).
    pub number: Hsla,
    /// Boolean / nullish literal ink (`SyntaxToken::Boolean`).
    pub boolean: Hsla,
    /// Named constant / enum variant ink (`SyntaxToken::Constant`).
    pub constant: Hsla,
    /// Function / method ink (`SyntaxToken::Function`).
    pub function: Hsla,
    /// Macro / decorator / preprocessor ink (`SyntaxToken::Macro`).
    pub macro_name: Hsla,
    /// Type / struct / trait / class ink (`SyntaxToken::Type`).
    pub type_name: Hsla,
    /// Parameter / special variable ink (`SyntaxToken::Variable`).
    pub variable: Hsla,
    /// Property / member / JSON-YAML-TOML key ink (`SyntaxToken::Property`).
    pub property: Hsla,
    /// Attribute ink (`SyntaxToken::Attribute`).
    pub attribute: Hsla,
    /// HTML / XML / JSX tag ink (`SyntaxToken::Tag`).
    pub tag: Hsla,
    /// Lifetime / label ink (`SyntaxToken::Label`).
    pub label: Hsla,
    /// Diff inserted foreground (`SyntaxToken::DiffInserted`).
    pub diff_inserted: Hsla,
    /// Diff deleted foreground (`SyntaxToken::DiffDeleted`).
    pub diff_deleted: Hsla,
    /// Diff header foreground (`SyntaxToken::DiffHeader`).
    pub diff_header: Hsla,
    /// Markup heading foreground (`SyntaxToken::Heading`).
    pub heading: Hsla,
    /// Markup link foreground (`SyntaxToken::Link`).
    pub link: Hsla,
    /// Invalid syntax foreground (`SyntaxToken::Invalid`).
    pub invalid: Hsla,
}

fn c(hex: u32) -> Hsla {
    rgb(hex).into()
}

impl Default for SyntaxTheme {
    fn default() -> Self {
        Self::light()
    }
}

impl SyntaxTheme {
    /// Crisp, daylight-comfortable light syntax palette inspired by GitHub Light & Zed One Light.
    pub fn light() -> Self {
        Self {
            code_bg: c(0xf8fafc),
            code_header_bg: c(0xf1f5f9),
            code_border: c(0xe2e8f0),
            gutter_foreground: c(0x94a3b8),
            diff_inserted_bg: c(0x16a34a).opacity(0.11),
            diff_deleted_bg: c(0xdc2626).opacity(0.10),
            diff_header_bg: c(0x4f46e5).opacity(0.08),
            plain: c(0x1e293b),
            comment: c(0x64748b),
            keyword: c(0x7c3aed),
            operator: c(0x475569),
            punctuation: c(0x64748b),
            string: c(0x15803d),
            escape: c(0x0e7490),
            number: c(0xc2410c),
            boolean: c(0xc2410c),
            constant: c(0x0369a1),
            function: c(0x2563eb),
            macro_name: c(0x0e7490),
            type_name: c(0x0f766e),
            variable: c(0xb45309),
            property: c(0x0369a1),
            attribute: c(0x6d28d9),
            tag: c(0xbe123c),
            label: c(0x9333ea),
            diff_inserted: c(0x15803d),
            diff_deleted: c(0xb91c1c),
            diff_header: c(0x4338ca),
            heading: c(0x1d4ed8),
            link: c(0x2563eb),
            invalid: c(0xdc2626),
        }
    }

    /// Deep obsidian dark syntax palette inspired by One Dark Pro & Tokyo Night.
    pub fn dark() -> Self {
        Self {
            code_bg: c(0x0d1117),
            code_header_bg: c(0x161b22),
            code_border: c(0x273043),
            gutter_foreground: c(0x64748b),
            diff_inserted_bg: c(0x22c55e).opacity(0.15),
            diff_deleted_bg: c(0xef4444).opacity(0.16),
            diff_header_bg: c(0x818cf8).opacity(0.14),
            plain: c(0xe2e8f0),
            comment: c(0x6b7280),
            keyword: c(0xc084fc),
            operator: c(0x94a3b8),
            punctuation: c(0x94a3b8),
            string: c(0x4ade80),
            escape: c(0x22d3ee),
            number: c(0xfb923c),
            boolean: c(0xfb923c),
            constant: c(0x38bdf8),
            function: c(0x60a5fa),
            macro_name: c(0x22d3ee),
            type_name: c(0xfbbf24),
            variable: c(0xfca5a5),
            property: c(0x7dd3fc),
            attribute: c(0xa78bfa),
            tag: c(0xfb7185),
            label: c(0xe879f9),
            diff_inserted: c(0x4ade80),
            diff_deleted: c(0xf87171),
            diff_header: c(0xa5b4fc),
            heading: c(0x93c5fd),
            link: c(0x60a5fa),
            invalid: c(0xf87171),
        }
    }

    /// Resolves a semantic [`SyntaxToken`] into a GPUI [`HighlightStyle`], returning `None` for
    /// [`SyntaxToken::Plain`] so unhighlighted runs inherit `self.plain` without extra spans.
    pub fn style_for(&self, token: SyntaxToken) -> Option<HighlightStyle> {
        let style = match token {
            SyntaxToken::Plain => return None,
            SyntaxToken::Comment => HighlightStyle {
                color: Some(self.comment),
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
            SyntaxToken::Keyword => HighlightStyle {
                color: Some(self.keyword),
                ..Default::default()
            },
            SyntaxToken::Operator => HighlightStyle {
                color: Some(self.operator),
                ..Default::default()
            },
            SyntaxToken::Punctuation => HighlightStyle {
                color: Some(self.punctuation),
                ..Default::default()
            },
            SyntaxToken::String => HighlightStyle {
                color: Some(self.string),
                ..Default::default()
            },
            SyntaxToken::Escape => HighlightStyle {
                color: Some(self.escape),
                ..Default::default()
            },
            SyntaxToken::Number => HighlightStyle {
                color: Some(self.number),
                ..Default::default()
            },
            SyntaxToken::Boolean => HighlightStyle {
                color: Some(self.boolean),
                ..Default::default()
            },
            SyntaxToken::Constant => HighlightStyle {
                color: Some(self.constant),
                ..Default::default()
            },
            SyntaxToken::Function => HighlightStyle {
                color: Some(self.function),
                ..Default::default()
            },
            SyntaxToken::Macro => HighlightStyle {
                color: Some(self.macro_name),
                ..Default::default()
            },
            SyntaxToken::Type => HighlightStyle {
                color: Some(self.type_name),
                ..Default::default()
            },
            SyntaxToken::Variable => HighlightStyle {
                color: Some(self.variable),
                ..Default::default()
            },
            SyntaxToken::Property => HighlightStyle {
                color: Some(self.property),
                ..Default::default()
            },
            SyntaxToken::Attribute => HighlightStyle {
                color: Some(self.attribute),
                ..Default::default()
            },
            SyntaxToken::Tag => HighlightStyle {
                color: Some(self.tag),
                ..Default::default()
            },
            SyntaxToken::Label => HighlightStyle {
                color: Some(self.label),
                ..Default::default()
            },
            SyntaxToken::DiffInserted => HighlightStyle {
                color: Some(self.diff_inserted),
                ..Default::default()
            },
            SyntaxToken::DiffDeleted => HighlightStyle {
                color: Some(self.diff_deleted),
                ..Default::default()
            },
            SyntaxToken::DiffHeader => HighlightStyle {
                color: Some(self.diff_header),
                font_weight: Some(FontWeight::MEDIUM),
                ..Default::default()
            },
            SyntaxToken::Heading => HighlightStyle {
                color: Some(self.heading),
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
            SyntaxToken::Emphasis => HighlightStyle {
                font_style: Some(FontStyle::Italic),
                ..Default::default()
            },
            SyntaxToken::Strong => HighlightStyle {
                font_weight: Some(FontWeight::SEMIBOLD),
                ..Default::default()
            },
            SyntaxToken::Link => HighlightStyle {
                color: Some(self.link),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(self.link),
                    wavy: false,
                }),
                ..Default::default()
            },
            SyntaxToken::Invalid => HighlightStyle {
                color: Some(self.invalid),
                underline: Some(UnderlineStyle {
                    thickness: px(1.),
                    color: Some(self.invalid),
                    wavy: true,
                }),
                ..Default::default()
            },
        };
        Some(style)
    }

    /// Returns the subtle row background fill for a [`DiffLineKind`].
    pub const fn diff_line_bg(&self, kind: DiffLineKind) -> Hsla {
        match kind {
            DiffLineKind::Inserted => self.diff_inserted_bg,
            DiffLineKind::Deleted => self.diff_deleted_bg,
            DiffLineKind::HunkHeader => self.diff_header_bg,
        }
    }
}

/// Extensible backend trait for applications that want to plug in a custom highlighter
/// (for example Tree-sitter or a domain-specific DSL lexer).
pub trait SyntaxBackend: Send + Sync + 'static {
    /// Highlights `code` tagged with `language` into line-aligned [`HighlightedLine`] entries.
    fn highlight(&self, language: &str, code: &str) -> Vec<HighlightedLine>;
}

/// Returns a crisp, human-friendly display label for a code fence info tag or filename
/// (e.g. `"rs"` -> `"Rust"`, `"tsx"` -> `"TSX"`, `"Cargo.toml"` -> `"Cargo.toml"`, `""` -> `"Text"`).
pub fn friendly_language_label(raw: &str) -> SharedString {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return SharedString::new_static("Text");
    }
    // Preserve explicit filenames like `Cargo.toml`, `Dockerfile`, `src/main.rs`.
    if trimmed.contains('/') || trimmed.contains('\\') || trimmed.contains('.') {
        return SharedString::from(trimmed.to_owned());
    }
    match trimmed.to_ascii_lowercase().as_str() {
        "rs" | "rust" => SharedString::new_static("Rust"),
        "ts" | "typescript" => SharedString::new_static("TypeScript"),
        "tsx" => SharedString::new_static("TSX"),
        "js" | "javascript" | "mjs" | "cjs" => SharedString::new_static("JavaScript"),
        "jsx" => SharedString::new_static("JSX"),
        "py" | "python" | "python3" | "pyi" => SharedString::new_static("Python"),
        "sh" | "bash" | "shell" | "zsh" | "shellsession" | "console" => {
            SharedString::new_static("Bash")
        }
        "fish" => SharedString::new_static("Fish"),
        "ps1" | "powershell" | "pwsh" => SharedString::new_static("PowerShell"),
        "bat" | "cmd" => SharedString::new_static("Batch"),
        "json" => SharedString::new_static("JSON"),
        "jsonc" | "json5" => SharedString::new_static("JSONC"),
        "toml" => SharedString::new_static("TOML"),
        "yaml" | "yml" => SharedString::new_static("YAML"),
        "xml" | "svg" | "plist" => SharedString::new_static("XML"),
        "html" | "htm" => SharedString::new_static("HTML"),
        "css" => SharedString::new_static("CSS"),
        "scss" | "sass" => SharedString::new_static("SCSS"),
        "sql" | "postgres" | "postgresql" | "sqlite" | "mysql" => SharedString::new_static("SQL"),
        "md" | "markdown" | "gfm" => SharedString::new_static("Markdown"),
        "diff" | "patch" => SharedString::new_static("Diff"),
        "c" | "h" => SharedString::new_static("C"),
        "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" => SharedString::new_static("C++"),
        "cs" | "csharp" | "c#" => SharedString::new_static("C#"),
        "go" | "golang" => SharedString::new_static("Go"),
        "java" => SharedString::new_static("Java"),
        "kt" | "kotlin" => SharedString::new_static("Kotlin"),
        "swift" => SharedString::new_static("Swift"),
        "objc" | "objective-c" | "m" => SharedString::new_static("Objective-C"),
        "rb" | "ruby" => SharedString::new_static("Ruby"),
        "php" => SharedString::new_static("PHP"),
        "lua" => SharedString::new_static("Lua"),
        "zig" => SharedString::new_static("Zig"),
        "nix" => SharedString::new_static("Nix"),
        "dockerfile" | "docker" => SharedString::new_static("Dockerfile"),
        "makefile" | "make" | "mk" => SharedString::new_static("Makefile"),
        "cmake" => SharedString::new_static("CMake"),
        "wgsl" => SharedString::new_static("WGSL"),
        "glsl" | "hlsl" | "metal" => SharedString::from(trimmed.to_ascii_uppercase()),
        "graphql" | "gql" => SharedString::new_static("GraphQL"),
        "proto" | "protobuf" => SharedString::new_static("Protobuf"),
        "tf" | "terraform" | "hcl" => SharedString::new_static("Terraform"),
        "ini" | "cfg" | "conf" => SharedString::new_static("INI"),
        "env" | "dotenv" => SharedString::new_static("Dotenv"),
        "csv" => SharedString::new_static("CSV"),
        "tex" | "latex" => SharedString::new_static("LaTeX"),
        "typst" | "typ" => SharedString::new_static("Typst"),
        "vue" => SharedString::new_static("Vue"),
        "svelte" => SharedString::new_static("Svelte"),
        "elixir" | "ex" | "exs" => SharedString::new_static("Elixir"),
        "haskell" | "hs" => SharedString::new_static("Haskell"),
        "ocaml" | "ml" => SharedString::new_static("OCaml"),
        "scala" => SharedString::new_static("Scala"),
        "dart" => SharedString::new_static("Dart"),
        "r" => SharedString::new_static("R"),
        "text" | "txt" | "plain" | "plaintext" => SharedString::new_static("Text"),
        _ => SharedString::from(trimmed.to_owned()),
    }
}

/// Normalizes a language fence tag (or filename) into a canonical lookup key.
pub fn normalize_language_key(raw: &str) -> String {
    let token = raw.split_whitespace().next().unwrap_or("").trim();
    let lower = token.to_ascii_lowercase();
    match lower.as_str() {
        "rs" | "rust" => "rust".to_owned(),
        "ts" | "typescript" => "typescript".to_owned(),
        "tsx" => "tsx".to_owned(),
        "jsx" => "jsx".to_owned(),
        "js" | "javascript" | "mjs" | "cjs" => "javascript".to_owned(),
        "py" | "python" | "python3" | "pyi" => "python".to_owned(),
        "sh" | "bash" | "zsh" | "shell" | "console" | "shellsession" => "bash".to_owned(),
        "yml" | "yaml" => "yaml".to_owned(),
        "md" | "markdown" | "gfm" => "markdown".to_owned(),
        "diff" | "patch" => "diff".to_owned(),
        "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" => "cpp".to_owned(),
        "cs" | "csharp" | "c#" => "csharp".to_owned(),
        "go" | "golang" => "go".to_owned(),
        "kt" | "kotlin" => "kotlin".to_owned(),
        "rb" | "ruby" => "ruby".to_owned(),
        "htm" | "html" => "html".to_owned(),
        "postgres" | "postgresql" | "sqlite" | "mysql" | "sql" => "sql".to_owned(),
        _ => lower,
    }
}

fn is_diff_language(lang_key: &str) -> bool {
    matches!(lang_key, "diff" | "patch")
}

fn classify_diff_line(line: &str) -> Option<DiffLineKind> {
    if line.starts_with("@@")
        || line.starts_with("diff --git")
        || line.starts_with("index ")
        || line.starts_with("--- ")
        || line.starts_with("+++ ")
    {
        Some(DiffLineKind::HunkHeader)
    } else if line.starts_with('+') {
        Some(DiffLineKind::Inserted)
    } else if line.starts_with('-') {
        Some(DiffLineKind::Deleted)
    } else {
        None
    }
}

fn highlight_plain_lines(code: &str, is_diff: bool) -> Vec<HighlightedLine> {
    let mut lines = Vec::new();
    for raw in code.split('\n') {
        let line = raw.strip_suffix('\r').unwrap_or(raw);
        let diff_kind = is_diff.then(|| classify_diff_line(line)).flatten();
        let spans = match diff_kind {
            Some(DiffLineKind::Inserted) if !line.is_empty() => {
                vec![(0..line.len(), SyntaxToken::DiffInserted)]
            }
            Some(DiffLineKind::Deleted) if !line.is_empty() => {
                vec![(0..line.len(), SyntaxToken::DiffDeleted)]
            }
            Some(DiffLineKind::HunkHeader) if !line.is_empty() => {
                vec![(0..line.len(), SyntaxToken::DiffHeader)]
            }
            _ => Vec::new(),
        };
        lines.push(HighlightedLine {
            text: SharedString::from(line.to_owned()),
            spans,
            diff_kind,
        });
    }
    if lines.is_empty() {
        lines.push(HighlightedLine::plain(""));
    }
    lines
}

// ---------------------------------------------------------------------------
// Syntect + two-face pure-Rust highlighting backend
// ---------------------------------------------------------------------------

#[cfg(feature = "syntax-highlighting")]
mod engine {
    use super::*;
    use syntect::parsing::{ParseState, Scope, ScopeStack, SyntaxReference, SyntaxSet};

    struct ScopeMatchers {
        // Structural / context scopes
        comment: Scope,
        string: Scope,
        meta_mapping_key: Scope,
        meta_diff_header: Scope,
        meta_diff_range: Scope,
        meta_annotation: Scope,
        meta_attribute: Scope,
        meta_macro: Scope,
        meta_preprocessor: Scope,
        // Punctuation (special cases vs generic)
        punct_def_comment: Scope,
        punct_def_string: Scope,
        punct_def_tag: Scope,
        punct_def_heading: Scope,
        punct_def_inserted: Scope,
        punct_def_deleted: Scope,
        punct_def_bold: Scope,
        punct_def_italic: Scope,
        punct_sep_decimal: Scope,
        punct_section_interpolation: Scope,
        punctuation: Scope,
        // Escapes & constants
        constant_escape: Scope,
        constant_placeholder: Scope,
        constant_numeric: Scope,
        constant_language: Scope,
        constant_character: Scope,
        constant: Scope,
        // Keywords & storage
        keyword_operator: Scope,
        keyword_control: Scope,
        keyword_declaration: Scope,
        keyword_key: Scope,
        keyword: Scope,
        storage_type_primitive: Scope,
        storage_type_numeric: Scope,
        storage_type_builtin: Scope,
        storage_type_rust: Scope,
        storage_type_c: Scope,
        storage_type_go: Scope,
        storage_type_java: Scope,
        storage_type_cs: Scope,
        storage_type_function: Scope,
        storage_type_struct: Scope,
        storage_type_enum: Scope,
        storage_type_trait: Scope,
        storage_type_impl: Scope,
        storage_type_class: Scope,
        storage_type_type: Scope,
        storage_modifier_lifetime: Scope,
        storage: Scope,
        // Entities & support
        entity_name_function: Scope,
        entity_name_method: Scope,
        entity_name_macro: Scope,
        entity_name_type: Scope,
        entity_name_struct: Scope,
        entity_name_enum: Scope,
        entity_name_trait: Scope,
        entity_name_class: Scope,
        entity_name_interface: Scope,
        entity_name_tag: Scope,
        entity_name_label: Scope,
        entity_name_section: Scope,
        entity_other_inherited_class: Scope,
        entity_other_attribute_name: Scope,
        support_function: Scope,
        support_macro: Scope,
        support_type_property_name: Scope,
        support_type: Scope,
        support_class: Scope,
        support_constant: Scope,
        support_other_property: Scope,
        // Variables
        variable_function: Scope,
        variable_annotation: Scope,
        variable_parameter: Scope,
        variable_language: Scope,
        variable_other_member: Scope,
        variable_other_property: Scope,
        variable_other_constant: Scope,
        // Markup & diffs
        markup_inserted: Scope,
        markup_deleted: Scope,
        markup_changed: Scope,
        markup_heading: Scope,
        markup_italic: Scope,
        markup_bold: Scope,
        markup_underline_link: Scope,
        markup_raw: Scope,
        invalid: Scope,
    }

    impl ScopeMatchers {
        fn new() -> Self {
            let s = |name: &str| Scope::new(name).expect("valid static TextMate scope");
            Self {
                comment: s("comment"),
                string: s("string"),
                meta_mapping_key: s("meta.mapping.key"),
                meta_diff_header: s("meta.diff.header"),
                meta_diff_range: s("meta.diff.range"),
                meta_annotation: s("meta.annotation"),
                meta_attribute: s("meta.attribute"),
                meta_macro: s("meta.macro"),
                meta_preprocessor: s("meta.preprocessor"),
                punct_def_comment: s("punctuation.definition.comment"),
                punct_def_string: s("punctuation.definition.string"),
                punct_def_tag: s("punctuation.definition.tag"),
                punct_def_heading: s("punctuation.definition.heading"),
                punct_def_inserted: s("punctuation.definition.inserted"),
                punct_def_deleted: s("punctuation.definition.deleted"),
                punct_def_bold: s("punctuation.definition.bold"),
                punct_def_italic: s("punctuation.definition.italic"),
                punct_sep_decimal: s("punctuation.separator.decimal"),
                punct_section_interpolation: s("punctuation.section.interpolation"),
                punctuation: s("punctuation"),
                constant_escape: s("constant.character.escape"),
                constant_placeholder: s("constant.other.placeholder"),
                constant_numeric: s("constant.numeric"),
                constant_language: s("constant.language"),
                constant_character: s("constant.character"),
                constant: s("constant"),
                keyword_operator: s("keyword.operator"),
                keyword_control: s("keyword.control"),
                keyword_declaration: s("keyword.declaration"),
                keyword_key: s("keyword.key"),
                keyword: s("keyword"),
                storage_type_primitive: s("storage.type.primitive"),
                storage_type_numeric: s("storage.type.numeric"),
                storage_type_builtin: s("storage.type.builtin"),
                storage_type_rust: s("storage.type.rust"),
                storage_type_c: s("storage.type.c"),
                storage_type_go: s("storage.type.go"),
                storage_type_java: s("storage.type.java"),
                storage_type_cs: s("storage.type.cs"),
                storage_type_function: s("storage.type.function"),
                storage_type_struct: s("storage.type.struct"),
                storage_type_enum: s("storage.type.enum"),
                storage_type_trait: s("storage.type.trait"),
                storage_type_impl: s("storage.type.impl"),
                storage_type_class: s("storage.type.class"),
                storage_type_type: s("storage.type.type"),
                storage_modifier_lifetime: s("storage.modifier.lifetime"),
                storage: s("storage"),
                entity_name_function: s("entity.name.function"),
                entity_name_method: s("entity.name.method"),
                entity_name_macro: s("entity.name.macro"),
                entity_name_type: s("entity.name.type"),
                entity_name_struct: s("entity.name.struct"),
                entity_name_enum: s("entity.name.enum"),
                entity_name_trait: s("entity.name.trait"),
                entity_name_class: s("entity.name.class"),
                entity_name_interface: s("entity.name.interface"),
                entity_name_tag: s("entity.name.tag"),
                entity_name_label: s("entity.name.label"),
                entity_name_section: s("entity.name.section"),
                entity_other_inherited_class: s("entity.other.inherited-class"),
                entity_other_attribute_name: s("entity.other.attribute-name"),
                support_function: s("support.function"),
                support_macro: s("support.macro"),
                support_type_property_name: s("support.type.property-name"),
                support_type: s("support.type"),
                support_class: s("support.class"),
                support_constant: s("support.constant"),
                support_other_property: s("support.other.property"),
                variable_function: s("variable.function"),
                variable_annotation: s("variable.annotation"),
                variable_parameter: s("variable.parameter"),
                variable_language: s("variable.language"),
                variable_other_member: s("variable.other.member"),
                variable_other_property: s("variable.other.property"),
                variable_other_constant: s("variable.other.constant"),
                markup_inserted: s("markup.inserted"),
                markup_deleted: s("markup.deleted"),
                markup_changed: s("markup.changed"),
                markup_heading: s("markup.heading"),
                markup_italic: s("markup.italic"),
                markup_bold: s("markup.bold"),
                markup_underline_link: s("markup.underline.link"),
                markup_raw: s("markup.raw"),
                invalid: s("invalid"),
            }
        }

        #[inline]
        fn classify_stack(&self, stack: &[Scope]) -> SyntaxToken {
            // Check stack-wide context flags in a single quick bitwise pass.
            let mut in_comment = false;
            let mut in_mapping_key = false;
            let mut in_diff_header = false;
            let mut in_annotation = false;
            for &sc in stack {
                if self.comment.is_prefix_of(sc) {
                    in_comment = true;
                }
                if self.meta_mapping_key.is_prefix_of(sc)
                    || self.support_type_property_name.is_prefix_of(sc)
                {
                    in_mapping_key = true;
                }
                if self.meta_diff_header.is_prefix_of(sc) || self.meta_diff_range.is_prefix_of(sc) {
                    in_diff_header = true;
                }
                if self.meta_annotation.is_prefix_of(sc) || self.meta_attribute.is_prefix_of(sc) {
                    in_annotation = true;
                }
            }

            if in_comment {
                return SyntaxToken::Comment;
            }

            // Walk innermost -> outermost so string escapes, interpolations, and nested tokens
            // naturally take priority over enclosing containers.
            for &sc in stack.iter().rev() {
                if self.invalid.is_prefix_of(sc) {
                    return SyntaxToken::Invalid;
                }
                if self.markup_inserted.is_prefix_of(sc) {
                    return SyntaxToken::DiffInserted;
                }
                if self.markup_deleted.is_prefix_of(sc) {
                    return SyntaxToken::DiffDeleted;
                }
                if self.markup_changed.is_prefix_of(sc) {
                    return SyntaxToken::DiffHeader;
                }
                if self.constant_escape.is_prefix_of(sc)
                    || self.constant_placeholder.is_prefix_of(sc)
                    || self.punct_section_interpolation.is_prefix_of(sc)
                {
                    return SyntaxToken::Escape;
                }
                if self.support_type_property_name.is_prefix_of(sc)
                    || self.keyword_key.is_prefix_of(sc)
                {
                    return SyntaxToken::Property;
                }
                if self.string.is_prefix_of(sc) {
                    return if in_mapping_key {
                        SyntaxToken::Property
                    } else {
                        SyntaxToken::String
                    };
                }
                if self.constant_numeric.is_prefix_of(sc) {
                    return SyntaxToken::Number;
                }
                if self.constant_language.is_prefix_of(sc) {
                    return SyntaxToken::Boolean;
                }
                if self.constant_character.is_prefix_of(sc) {
                    return SyntaxToken::String;
                }
                if self.constant.is_prefix_of(sc) || self.support_constant.is_prefix_of(sc) {
                    return SyntaxToken::Constant;
                }
                if self.entity_name_macro.is_prefix_of(sc)
                    || self.support_macro.is_prefix_of(sc)
                    || self.meta_preprocessor.is_prefix_of(sc)
                {
                    return SyntaxToken::Macro;
                }
                if self.entity_name_function.is_prefix_of(sc)
                    || self.entity_name_method.is_prefix_of(sc)
                    || self.support_function.is_prefix_of(sc)
                    || self.variable_function.is_prefix_of(sc)
                {
                    return if in_annotation {
                        SyntaxToken::Attribute
                    } else {
                        SyntaxToken::Function
                    };
                }
                if self.entity_name_tag.is_prefix_of(sc) {
                    return if in_mapping_key {
                        SyntaxToken::Property
                    } else {
                        SyntaxToken::Tag
                    };
                }
                if self.entity_other_attribute_name.is_prefix_of(sc)
                    || self.variable_annotation.is_prefix_of(sc)
                {
                    return SyntaxToken::Attribute;
                }
                if self.storage_modifier_lifetime.is_prefix_of(sc)
                    || self.entity_name_label.is_prefix_of(sc)
                {
                    return SyntaxToken::Label;
                }
                if self.storage_type_function.is_prefix_of(sc)
                    || self.storage_type_struct.is_prefix_of(sc)
                    || self.storage_type_enum.is_prefix_of(sc)
                    || self.storage_type_trait.is_prefix_of(sc)
                    || self.storage_type_impl.is_prefix_of(sc)
                    || self.storage_type_class.is_prefix_of(sc)
                    || self.storage_type_type.is_prefix_of(sc)
                {
                    return SyntaxToken::Keyword;
                }
                if self.entity_name_type.is_prefix_of(sc)
                    || self.entity_name_struct.is_prefix_of(sc)
                    || self.entity_name_enum.is_prefix_of(sc)
                    || self.entity_name_trait.is_prefix_of(sc)
                    || self.entity_name_class.is_prefix_of(sc)
                    || self.entity_name_interface.is_prefix_of(sc)
                    || self.entity_other_inherited_class.is_prefix_of(sc)
                    || self.support_type.is_prefix_of(sc)
                    || self.support_class.is_prefix_of(sc)
                    || self.storage_type_primitive.is_prefix_of(sc)
                    || self.storage_type_numeric.is_prefix_of(sc)
                    || self.storage_type_builtin.is_prefix_of(sc)
                    || self.storage_type_rust.is_prefix_of(sc)
                    || self.storage_type_c.is_prefix_of(sc)
                    || self.storage_type_go.is_prefix_of(sc)
                    || self.storage_type_java.is_prefix_of(sc)
                    || self.storage_type_cs.is_prefix_of(sc)
                {
                    return SyntaxToken::Type;
                }
                if self.keyword_operator.is_prefix_of(sc) {
                    return SyntaxToken::Operator;
                }
                if self.keyword_control.is_prefix_of(sc)
                    || self.keyword_declaration.is_prefix_of(sc)
                    || self.keyword.is_prefix_of(sc)
                    || self.storage.is_prefix_of(sc)
                {
                    return SyntaxToken::Keyword;
                }
                if self.variable_other_member.is_prefix_of(sc)
                    || self.variable_other_property.is_prefix_of(sc)
                    || self.support_other_property.is_prefix_of(sc)
                {
                    return SyntaxToken::Property;
                }
                if self.variable_other_constant.is_prefix_of(sc) {
                    return SyntaxToken::Constant;
                }
                if self.variable_parameter.is_prefix_of(sc)
                    || self.variable_language.is_prefix_of(sc)
                {
                    return SyntaxToken::Variable;
                }
                if self.markup_heading.is_prefix_of(sc) || self.entity_name_section.is_prefix_of(sc)
                {
                    return SyntaxToken::Heading;
                }
                if self.markup_underline_link.is_prefix_of(sc) {
                    return SyntaxToken::Link;
                }
                if self.markup_bold.is_prefix_of(sc) {
                    return SyntaxToken::Strong;
                }
                if self.markup_italic.is_prefix_of(sc) {
                    return SyntaxToken::Emphasis;
                }
                if self.markup_raw.is_prefix_of(sc) {
                    return SyntaxToken::String;
                }
                if self.meta_macro.is_prefix_of(sc) {
                    return SyntaxToken::Macro;
                }

                // Transparent delimiter punctuations delegate to their enclosing semantic scope
                // (so string quotes share string color, diff `+` shares inserted color, etc.).
                if self.punct_def_comment.is_prefix_of(sc)
                    || self.punct_def_string.is_prefix_of(sc)
                    || self.punct_def_tag.is_prefix_of(sc)
                    || self.punct_def_heading.is_prefix_of(sc)
                    || self.punct_def_inserted.is_prefix_of(sc)
                    || self.punct_def_deleted.is_prefix_of(sc)
                    || self.punct_def_bold.is_prefix_of(sc)
                    || self.punct_def_italic.is_prefix_of(sc)
                    || self.punct_sep_decimal.is_prefix_of(sc)
                {
                    continue;
                }
                if self.punctuation.is_prefix_of(sc) {
                    return if in_annotation {
                        SyntaxToken::Attribute
                    } else if in_diff_header {
                        SyntaxToken::DiffHeader
                    } else {
                        SyntaxToken::Punctuation
                    };
                }
            }

            if in_diff_header {
                SyntaxToken::DiffHeader
            } else if in_annotation {
                SyntaxToken::Attribute
            } else {
                SyntaxToken::Plain
            }
        }
    }

    pub(super) struct SyntectEngine {
        syntax_set: SyntaxSet,
        matchers: ScopeMatchers,
    }

    impl SyntectEngine {
        pub(super) fn global() -> &'static Self {
            static ENGINE: OnceLock<SyntectEngine> = OnceLock::new();
            ENGINE.get_or_init(|| Self {
                syntax_set: two_face::syntax::extra_newlines(),
                matchers: ScopeMatchers::new(),
            })
        }

        pub(super) fn resolve_syntax(&self, raw_lang: &str) -> Option<&SyntaxReference> {
            let trimmed = raw_lang.split_whitespace().next().unwrap_or("").trim();
            if trimmed.is_empty() {
                return None;
            }
            let lower = trimmed.to_ascii_lowercase();
            if matches!(lower.as_str(), "text" | "txt" | "plain" | "plaintext") {
                return None;
            }

            // Explicit filename lookup (e.g. `Cargo.toml`, `Dockerfile`, `src/main.rs`).
            if let Some(base) = trimmed.rsplit(['/', '\\']).next() {
                if let Some(syn) = self.syntax_set.find_syntax_for_file(base).ok().flatten() {
                    return Some(syn);
                }
            }

            // Explicit high-signal aliases for modern ecosystems (including pure-Rust fancy-regex
            // grammars where `TypeScriptReact` handles both TSX and JSX cleanly).
            let alias_token: Option<&str> = match lower.as_str() {
                "rs" | "rust" => Some("rs"),
                "ts" | "typescript" => Some("ts"),
                "tsx" | "jsx" => Some("tsx"),
                "js" | "javascript" | "mjs" | "cjs" => Some("js"),
                "py" | "python" | "python3" | "pyi" => Some("py"),
                "sh" | "bash" | "zsh" | "shell" | "console" | "shellsession" => Some("sh"),
                "fish" => Some("fish"),
                "bat" | "cmd" => Some("bat"),
                "json" | "jsonc" | "json5" => Some("json"),
                "toml" => Some("toml"),
                "yml" | "yaml" => Some("yaml"),
                "sql" | "postgres" | "postgresql" | "sqlite" | "mysql" => Some("sql"),
                "md" | "markdown" | "gfm" => Some("md"),
                "diff" | "patch" => Some("diff"),
                "c" | "h" => Some("c"),
                "cpp" | "c++" | "cc" | "cxx" | "hpp" | "hh" => Some("cpp"),
                "cs" | "csharp" | "c#" => Some("cs"),
                "go" | "golang" => Some("go"),
                "java" => Some("java"),
                "kt" | "kotlin" => Some("kt"),
                "swift" => Some("swift"),
                "objc" | "objective-c" => Some("m"),
                "rb" | "ruby" => Some("rb"),
                "php" => Some("php"),
                "html" | "htm" => Some("html"),
                "css" => Some("css"),
                "scss" => Some("scss"),
                "xml" | "svg" | "plist" => Some("xml"),
                "dockerfile" | "docker" => Some("Dockerfile"),
                "makefile" | "make" | "mk" => Some("Makefile"),
                "cmake" => Some("cmake"),
                "wgsl" => Some("wgsl"),
                "glsl" => Some("glsl"),
                "hlsl" => Some("c"),
                "metal" => Some("cpp"),
                "zig" => Some("zig"),
                "nix" => Some("nix"),
                "lua" => Some("lua"),
                "graphql" | "gql" => Some("graphql"),
                "proto" | "protobuf" => Some("proto"),
                "tf" | "terraform" | "hcl" => Some("tf"),
                "ini" | "cfg" | "conf" => Some("ini"),
                "env" | "dotenv" => Some("env"),
                "csv" => Some("csv"),
                "tex" | "latex" => Some("tex"),
                "typst" | "typ" => Some("typ"),
                "vue" => Some("vue"),
                "svelte" => Some("svelte"),
                "elixir" | "ex" | "exs" => Some("ex"),
                "haskell" | "hs" => Some("hs"),
                "ocaml" | "ml" => Some("ml"),
                "scala" => Some("scala"),
                "dart" => Some("dart"),
                "r" => Some("r"),
                _ => None,
            };

            if let Some(tok) = alias_token {
                if let Some(syn) = self
                    .syntax_set
                    .find_syntax_by_token(tok)
                    .or_else(|| self.syntax_set.find_syntax_by_extension(tok))
                    .or_else(|| self.syntax_set.find_syntax_by_name(tok))
                {
                    return Some(syn);
                }
            }

            self.syntax_set
                .find_syntax_by_token(trimmed)
                .or_else(|| self.syntax_set.find_syntax_by_extension(&lower))
                .or_else(|| self.syntax_set.find_syntax_by_name(trimmed))
        }

        pub(super) fn highlight_one_line(
            &self,
            line: &str,
            is_diff: bool,
            parse_state: &mut ParseState,
            scope_stack: &mut ScopeStack,
        ) -> HighlightedLine {
            let diff_kind = is_diff.then(|| classify_diff_line(line)).flatten();

            if line.is_empty() || line.len() > MAX_HIGHLIGHT_LINE_BYTES {
                return HighlightedLine {
                    text: SharedString::from(line.to_owned()),
                    spans: Vec::new(),
                    diff_kind,
                };
            }

            // `extra_newlines()` grammars expect lines terminated with `\n`.
            let mut with_nl = String::with_capacity(line.len() + 1);
            with_nl.push_str(line);
            with_nl.push('\n');

            let Ok(ops) = parse_state.parse_line(&with_nl, &self.syntax_set) else {
                return HighlightedLine {
                    text: SharedString::from(line.to_owned()),
                    spans: Vec::new(),
                    diff_kind,
                };
            };

            let line_len = line.len();
            let mut spans: Vec<(Range<usize>, SyntaxToken)> = Vec::new();
            let mut cursor = 0usize;

            for (pos, op) in ops {
                let end = pos.min(line_len);
                if end > cursor {
                    let token = self.matchers.classify_stack(scope_stack.as_slice());
                    push_token_span(&mut spans, cursor..end, token);
                }
                let _ = scope_stack.apply(&op);
                cursor = end;
            }
            if cursor < line_len {
                let token = self.matchers.classify_stack(scope_stack.as_slice());
                push_token_span(&mut spans, cursor..line_len, token);
            }

            HighlightedLine {
                text: SharedString::from(line.to_owned()),
                spans,
                diff_kind,
            }
        }

        pub(super) fn highlight_code(&self, language: &str, code: &str) -> Vec<HighlightedLine> {
            let lang_key = normalize_language_key(language);
            let is_diff = is_diff_language(&lang_key);
            let Some(syntax) = self.resolve_syntax(language) else {
                return highlight_plain_lines(code, is_diff);
            };

            let mut parse_state = ParseState::new(syntax);
            let mut scope_stack = ScopeStack::new();
            let mut lines = Vec::new();

            for raw in code.split('\n') {
                let line = raw.strip_suffix('\r').unwrap_or(raw);
                lines.push(self.highlight_one_line(
                    line,
                    is_diff,
                    &mut parse_state,
                    &mut scope_stack,
                ));
            }
            if lines.is_empty() {
                lines.push(HighlightedLine::plain(""));
            }
            lines
        }
    }

    fn push_token_span(
        spans: &mut Vec<(Range<usize>, SyntaxToken)>,
        range: Range<usize>,
        token: SyntaxToken,
    ) {
        if token == SyntaxToken::Plain || range.is_empty() {
            return;
        }
        if let Some((last_range, last_token)) = spans.last_mut()
            && *last_token == token
            && last_range.end == range.start
        {
            last_range.end = range.end;
        } else {
            spans.push((range, token));
        }
    }

    /// Stateful line-by-line incremental highlighter that caches completed line parse states so
    /// streaming code blocks only re-lex newly appended or modified lines in O(new lines).
    pub struct IncrementalSyntaxHighlighter {
        language: String,
        is_diff: bool,
        has_syntax: bool,
        cached_raw_lines: Vec<String>,
        cached_highlighted: Vec<HighlightedLine>,
        /// Parser & scope state at the END of each cached line.
        end_states: Vec<(ParseState, ScopeStack)>,
    }

    impl IncrementalSyntaxHighlighter {
        /// Creates a new incremental highlighter for `language`.
        pub fn new(language: &str) -> Self {
            let lang_key = normalize_language_key(language);
            let is_diff = is_diff_language(&lang_key);
            let has_syntax = SyntectEngine::global().resolve_syntax(language).is_some();
            Self {
                language: language.to_owned(),
                is_diff,
                has_syntax,
                cached_raw_lines: Vec::new(),
                cached_highlighted: Vec::new(),
                end_states: Vec::new(),
            }
        }

        /// Returns the language tag this incremental highlighter is bound to.
        pub fn language(&self) -> &str {
            &self.language
        }

        /// Incrementally updates highlighted lines for `code`, reusing the longest unchanged
        /// prefix of previously parsed lines and resuming TextMate parser state from there.
        pub fn update(&mut self, code: &str) -> &[HighlightedLine] {
            if !self.has_syntax {
                let raw_lines: Vec<&str> = code
                    .split('\n')
                    .map(|l| l.strip_suffix('\r').unwrap_or(l))
                    .collect();
                let unchanged = self.cached_raw_lines.len() == raw_lines.len()
                    && self
                        .cached_raw_lines
                        .iter()
                        .zip(raw_lines.iter())
                        .all(|(a, b)| a == b);
                if !unchanged {
                    self.cached_raw_lines = raw_lines.iter().map(|s| (*s).to_owned()).collect();
                    self.cached_highlighted = highlight_plain_lines(code, self.is_diff);
                }
                return &self.cached_highlighted;
            }

            let engine = SyntectEngine::global();
            let Some(syntax) = engine.resolve_syntax(&self.language) else {
                self.cached_highlighted = highlight_plain_lines(code, self.is_diff);
                return &self.cached_highlighted;
            };

            let new_lines: Vec<&str> = code
                .split('\n')
                .map(|l| l.strip_suffix('\r').unwrap_or(l))
                .collect();

            // Find the longest unchanged prefix of lines.
            let mut common = 0usize;
            while common < self.cached_raw_lines.len()
                && common < new_lines.len()
                && self.cached_raw_lines[common] == new_lines[common]
            {
                common += 1;
            }

            if common == self.cached_raw_lines.len() && common == new_lines.len() {
                return &self.cached_highlighted;
            }

            self.cached_raw_lines.truncate(common);
            self.cached_highlighted.truncate(common);
            self.end_states.truncate(common);

            let (mut parse_state, mut scope_stack) = if common > 0 {
                self.end_states[common - 1].clone()
            } else {
                (ParseState::new(syntax), ScopeStack::new())
            };

            for &line in &new_lines[common..] {
                let hl = engine.highlight_one_line(
                    line,
                    self.is_diff,
                    &mut parse_state,
                    &mut scope_stack,
                );
                self.cached_raw_lines.push(line.to_owned());
                self.cached_highlighted.push(hl);
                self.end_states
                    .push((parse_state.clone(), scope_stack.clone()));
            }

            if self.cached_highlighted.is_empty() {
                self.cached_highlighted.push(HighlightedLine::plain(""));
            }

            &self.cached_highlighted
        }
    }
}

#[cfg(not(feature = "syntax-highlighting"))]
mod engine {
    use super::*;

    pub struct IncrementalSyntaxHighlighter {
        language: String,
        is_diff: bool,
        cached_source: String,
        cached_highlighted: Vec<HighlightedLine>,
    }

    impl IncrementalSyntaxHighlighter {
        pub fn new(language: &str) -> Self {
            let lang_key = normalize_language_key(language);
            Self {
                language: language.to_owned(),
                is_diff: is_diff_language(&lang_key),
                cached_source: String::new(),
                cached_highlighted: Vec::new(),
            }
        }

        pub fn language(&self) -> &str {
            &self.language
        }

        pub fn update(&mut self, code: &str) -> &[HighlightedLine] {
            if self.cached_highlighted.is_empty() || self.cached_source != code {
                self.cached_source = code.to_owned();
                self.cached_highlighted = highlight_plain_lines(code, self.is_diff);
            }
            &self.cached_highlighted
        }
    }
}

pub use engine::IncrementalSyntaxHighlighter;

/// Default pure-Rust [`SyntaxBackend`] powered by `syntect` + `two-face`.
#[derive(Clone, Copy, Debug, Default)]
pub struct DefaultSyntaxBackend;

impl SyntaxBackend for DefaultSyntaxBackend {
    fn highlight(&self, language: &str, code: &str) -> Vec<HighlightedLine> {
        highlight_code(language, code)
    }
}

/// Highlights `code` in `language` directly without consulting the global cache.
pub fn highlight_code(language: &str, code: &str) -> Vec<HighlightedLine> {
    #[cfg(feature = "syntax-highlighting")]
    {
        engine::SyntectEngine::global().highlight_code(language, code)
    }
    #[cfg(not(feature = "syntax-highlighting"))]
    {
        let lang_key = normalize_language_key(language);
        highlight_plain_lines(code, is_diff_language(&lang_key))
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct CacheKey {
    language: String,
    code_hash: u64,
    code_len: usize,
}

struct HighlightCache {
    map: HashMap<CacheKey, Arc<Vec<HighlightedLine>>>,
    order: VecDeque<CacheKey>,
}

impl HighlightCache {
    fn new() -> Self {
        Self {
            map: HashMap::with_capacity(GLOBAL_CACHE_CAPACITY),
            order: VecDeque::with_capacity(GLOBAL_CACHE_CAPACITY),
        }
    }

    fn get(&self, key: &CacheKey) -> Option<Arc<Vec<HighlightedLine>>> {
        self.map.get(key).cloned()
    }

    fn insert(&mut self, key: CacheKey, value: Arc<Vec<HighlightedLine>>) {
        if self.map.contains_key(&key) {
            return;
        }
        if self.order.len() >= GLOBAL_CACHE_CAPACITY
            && let Some(evicted) = self.order.pop_front()
        {
            self.map.remove(&evicted);
        }
        self.order.push_back(key.clone());
        self.map.insert(key, value);
    }
}

fn global_cache() -> &'static Mutex<HighlightCache> {
    static CACHE: OnceLock<Mutex<HighlightCache>> = OnceLock::new();
    CACHE.get_or_init(|| Mutex::new(HighlightCache::new()))
}

/// Highlights `code` in `language`, returning a shared cached [`Arc<Vec<HighlightedLine>>`].
///
/// Repeated calls with the same `(language, code)` return an instant `Arc` clone without
/// re-running regex grammars.
pub fn highlight_code_cached(language: &str, code: &str) -> Arc<Vec<HighlightedLine>> {
    let key = CacheKey {
        language: normalize_language_key(language),
        code_hash: seahash::hash(code.as_bytes()),
        code_len: code.len(),
    };

    if let Ok(cache) = global_cache().lock()
        && let Some(hit) = cache.get(&key)
    {
        return hit;
    }

    let highlighted = Arc::new(highlight_code(language, code));
    if let Ok(mut cache) = global_cache().lock() {
        cache.insert(key, highlighted.clone());
    }
    highlighted
}

/// Returns `true` if the built-in highlighter recognizes `language` (extension, alias, or filename).
pub fn supports_language(language: &str) -> bool {
    #[cfg(feature = "syntax-highlighting")]
    {
        engine::SyntectEngine::global()
            .resolve_syntax(language)
            .is_some()
    }
    #[cfg(not(feature = "syntax-highlighting"))]
    {
        let _ = language;
        false
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn token_at(line: &HighlightedLine, needle: &str) -> Option<SyntaxToken> {
        let start = line.text.find(needle)?;
        let end = start + needle.len();
        line.spans
            .iter()
            .find(|(r, _)| r.start <= start && r.end >= end)
            .map(|(_, tok)| *tok)
    }

    #[test]
    fn highlights_rust_typescript_python_json_toml_sql_bash_and_diff() {
        // Rust
        let rust_code = concat!(
            "// Greeting helper\n",
            "pub fn greet(name: &str) -> usize {\n",
            "    println!(\"hello {}\", name);\n",
            "    42\n",
            "}"
        );
        let rust_lines = highlight_code("rust", rust_code);
        assert_eq!(rust_lines.len(), 5);
        assert_eq!(token_at(&rust_lines[0], "// Greeting helper"), Some(SyntaxToken::Comment));
        assert_eq!(token_at(&rust_lines[1], "fn"), Some(SyntaxToken::Keyword));
        assert_eq!(token_at(&rust_lines[1], "greet"), Some(SyntaxToken::Function));
        assert_eq!(token_at(&rust_lines[1], "usize"), Some(SyntaxToken::Type));
        assert_eq!(token_at(&rust_lines[2], "hello"), Some(SyntaxToken::String));
        assert_eq!(token_at(&rust_lines[3], "42"), Some(SyntaxToken::Number));

        // TypeScript & TSX/JSX
        let tsx_code = "const count: number = 3;\nexport const el = <Button label=\"Go\" />;";
        let tsx_lines = highlight_code("tsx", tsx_code);
        assert_eq!(tsx_lines.len(), 2);
        assert_eq!(token_at(&tsx_lines[0], "const"), Some(SyntaxToken::Keyword));
        assert_eq!(token_at(&tsx_lines[0], "3"), Some(SyntaxToken::Number));
        assert!(
            !tsx_lines[1].spans.is_empty(),
            "TSX JSX line should produce syntax token spans"
        );
        assert!(supports_language("jsx"));

        // Python
        let py_code = "def add(x: int, y: int) -> int:\n    return x + y  # sum";
        let py_lines = highlight_code("py", py_code);
        assert_eq!(token_at(&py_lines[0], "def"), Some(SyntaxToken::Keyword));
        assert_eq!(token_at(&py_lines[0], "add"), Some(SyntaxToken::Function));
        assert_eq!(token_at(&py_lines[1], "return"), Some(SyntaxToken::Keyword));
        assert_eq!(token_at(&py_lines[1], "# sum"), Some(SyntaxToken::Comment));

        // JSON keys get Property token while values get String / Number / Boolean!
        let json_code = "{\n  \"cost\": 26.96,\n  \"live\": true,\n  \"mode\": \"pane\"\n}";
        let json_lines = highlight_code("json", json_code);
        assert_eq!(token_at(&json_lines[1], "cost"), Some(SyntaxToken::Property));
        assert_eq!(token_at(&json_lines[1], "26.96"), Some(SyntaxToken::Number));
        assert_eq!(token_at(&json_lines[2], "live"), Some(SyntaxToken::Property));
        assert_eq!(token_at(&json_lines[2], "true"), Some(SyntaxToken::Boolean));
        assert_eq!(token_at(&json_lines[3], "mode"), Some(SyntaxToken::Property));
        assert_eq!(token_at(&json_lines[3], "pane"), Some(SyntaxToken::String));

        // TOML (by language tag AND by filename Cargo.toml)
        let toml_lines = highlight_code("Cargo.toml", "[package]\nname = \"gpui\"\nversion = 1");
        assert!(toml_lines.iter().any(|l| !l.spans.is_empty()));
        assert_eq!(token_at(&toml_lines[1], "gpui"), Some(SyntaxToken::String));

        // SQL & Bash
        let sql_lines = highlight_code(
            "sql",
            "SELECT count(*) FROM usage WHERE provider = 'openai' AND cost > 10;",
        );
        assert_eq!(token_at(&sql_lines[0], "SELECT"), Some(SyntaxToken::Keyword));
        assert_eq!(token_at(&sql_lines[0], "FROM"), Some(SyntaxToken::Keyword));
        assert_eq!(token_at(&sql_lines[0], "openai"), Some(SyntaxToken::String));

        let sh_lines = highlight_code("bash", "# build\ncargo test --locked");
        assert_eq!(token_at(&sh_lines[0], "# build"), Some(SyntaxToken::Comment));

        // Diff detects Inserted, Deleted, and HunkHeader line kinds
        let diff_code = "@@ -1,2 +1,2 @@\n-old line\n+new line\n context";
        let diff_lines = highlight_code("diff", diff_code);
        assert_eq!(diff_lines[0].diff_kind, Some(DiffLineKind::HunkHeader));
        assert_eq!(diff_lines[1].diff_kind, Some(DiffLineKind::Deleted));
        assert_eq!(diff_lines[2].diff_kind, Some(DiffLineKind::Inserted));
        assert_eq!(diff_lines[3].diff_kind, None);
    }

    #[test]
    fn incremental_highlighter_matches_full_highlight_across_streaming_chunks() {
        let full = "fn demo() {\n    /* multi-line\n       comment */\n    let s = \"hi\\n\";\n}";
        let expected = highlight_code("rust", full);

        let mut inc = IncrementalSyntaxHighlighter::new("rust");
        let prefixes = [
            "fn demo(",
            "fn demo() {\n    /* multi",
            "fn demo() {\n    /* multi-line\n       comment */",
            full,
        ];
        for p in prefixes {
            let _ = inc.update(p);
        }
        assert_eq!(inc.update(full), expected.as_slice());
    }

    #[test]
    fn cache_returns_shared_arc_and_theme_highlights_are_sorted_and_bounded() {
        let a = highlight_code_cached("rust", "let total = 100;\n");
        let b = highlight_code_cached("RS", "let total = 100;\n");
        assert!(Arc::ptr_eq(&a, &b));

        for theme in [SyntaxTheme::light(), SyntaxTheme::dark()] {
            for line in a.iter() {
                let hls = line.to_highlights(&theme);
                let mut prev = 0;
                for (r, _) in &hls {
                    assert!(r.start >= prev && r.start < r.end && r.end <= line.text.len());
                    assert!(line.text.is_char_boundary(r.start));
                    assert!(line.text.is_char_boundary(r.end));
                    prev = r.end;
                }
            }
        }
    }

    #[test]
    fn friendly_labels_and_long_line_guard() {
        assert_eq!(friendly_language_label("rs"), "Rust");
        assert_eq!(friendly_language_label("tsx"), "TSX");
        assert_eq!(friendly_language_label("Cargo.toml"), "Cargo.toml");
        assert_eq!(friendly_language_label(""), "Text");

        let huge = "x".repeat(MAX_HIGHLIGHT_LINE_BYTES + 256);
        let lines = highlight_code("rust", &huge);
        assert_eq!(lines.len(), 1);
        assert!(lines[0].spans.is_empty());
    }
}

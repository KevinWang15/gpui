//! Generative Answer Components rendered inline inside AI Markdown responses.
//!
//! Ported from `byok-ai/frontend/byok/answer-components.ts`:
//! - The model emits a fenced block ```` ```component:<name>\n{...JSON...}\n``` ````.
//! - [`answer_component_instructions`] appends schemas and examples to the system prompt.
//! - [`split_open_component`] holds back an unclosed component fence while streaming so the UI
//!   renders a clean `<label>…` placeholder without partial-JSON flicker.
//! - Built-in GPUI answer components (`stat-card`, `callout`, `bar-chart`, `sparkline`,
//!   `data-table`, `key-value`, `badge-list`) work out of the box and hosts can register any
//!   custom GPUI element via [`AnswerComponentSpec`].

use std::collections::HashMap;
use std::rc::Rc;

use serde_json::{Value, json};

use crate::{AnyElement, App, FontWeight, SharedString, Window, div, prelude::*, px};

use crate::components::alert::{Alert, AlertVariant};
use crate::components::app_controls::trend_badge;
use crate::components::badge::{BadgeColor, badge};
use crate::components::charts::{
    BreakdownItem, ChartValueFormat, SparklineKind, breakdown_list, sparkline,
};
use crate::components::descriptions::{DescriptionItem, descriptions};
use crate::components::stat::TrendPolarity;
use crate::components::table::{Table, TableColumn, TableRow};
use crate::components::theme::{ActiveTheme, tabular_numerals};

/// Validates that an answer component name is lower-case kebab-case (`<= 100` chars).
pub fn is_valid_component_name(name: &str) -> bool {
    if name.is_empty() || name.len() > 100 {
        return false;
    }
    let bytes = name.as_bytes();
    if !bytes[0].is_ascii_lowercase() || bytes[bytes.len() - 1] == b'-' {
        return false;
    }
    let mut prev_dash = false;
    for &b in bytes {
        if b == b'-' {
            if prev_dash {
                return false;
            }
            prev_dash = true;
        } else if b.is_ascii_lowercase() || b.is_ascii_digit() {
            prev_dash = false;
        } else {
            return false;
        }
    }
    true
}

/// Renderer closure invoked with validated JSON props to construct a live GPUI [`AnyElement`].
pub type AnswerComponentRenderFn =
    Rc<dyn Fn(&Value, &mut Window, &mut App) -> Result<AnyElement, String> + 'static>;

/// Definition of one generative UI component the model may embed in Markdown answers.
#[derive(Clone)]
pub struct AnswerComponentSpec {
    /// Kebab-case identifier used in ```` ```component:<name> ```` fences.
    pub name: SharedString,
    /// Short human-readable label displayed while the fence is still streaming.
    pub label: SharedString,
    /// Guidance for the model describing when to use this component.
    pub description: SharedString,
    /// JSON Schema describing the expected props object.
    pub schema: Value,
    /// Concrete JSON example included in system prompt instructions.
    pub example: Value,
    /// Synchronous validator + GPUI element renderer.
    pub render: AnswerComponentRenderFn,
}

impl std::fmt::Debug for AnswerComponentSpec {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AnswerComponentSpec")
            .field("name", &self.name)
            .field("label", &self.label)
            .finish()
    }
}

/// Registry of [`AnswerComponentSpec`]s shared by [`super::markdown::Markdown`] and
/// [`super::ask_ai::AskAi`].
#[derive(Clone, Default)]
pub struct AnswerComponentRegistry {
    order: Vec<SharedString>,
    by_name: HashMap<SharedString, AnswerComponentSpec>,
}

impl AnswerComponentRegistry {
    /// Creates an empty registry.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Creates a registry pre-populated with all built-in GPUI answer components
    /// (`stat-card`, `callout`, `bar-chart`, `sparkline`, `data-table`, `key-value`, `badge-list`).
    pub fn with_defaults() -> Self {
        let mut reg = Self::empty();
        for spec in default_answer_components() {
            let _ = reg.register(spec);
        }
        reg
    }

    /// Registers a component specification, validating kebab-case naming and uniqueness.
    pub fn register(&mut self, spec: AnswerComponentSpec) -> Result<(), String> {
        if !is_valid_component_name(spec.name.as_ref()) {
            return Err(format!(
                "answer component name '{}' must be lower-case kebab-case (1..=100 chars)",
                spec.name
            ));
        }
        if self.by_name.contains_key(&spec.name) {
            return Err(format!("duplicate answer component '{}'", spec.name));
        }
        self.order.push(spec.name.clone());
        self.by_name.insert(spec.name.clone(), spec);
        Ok(())
    }

    /// Looks up a registered component by name.
    pub fn get(&self, name: &str) -> Option<&AnswerComponentSpec> {
        self.by_name.get(name)
    }

    /// Returns all registered components in registration order.
    pub fn iter(&self) -> impl Iterator<Item = &AnswerComponentSpec> {
        self.order.iter().filter_map(|k| self.by_name.get(k))
    }

    /// Returns `true` if no components are registered.
    pub fn is_empty(&self) -> bool {
        self.order.is_empty()
    }
}

/// Builds the `# Components in answers` system-prompt section instructing the model how to emit
/// ```` ```component:<name> ```` fenced JSON blocks.
pub fn answer_component_instructions(registry: &AnswerComponentRegistry) -> String {
    if registry.is_empty() {
        return String::new();
    }
    let mut out = String::from(
        "# Components in answers\n\
You may embed structured visual components inside Markdown answers using a fenced code block named `component:<name>` containing a single strict JSON object (no comments or trailing commas). Use a component only when it communicates more clearly than plain text, never invent data, and only use the component names listed below.\n",
    );
    for spec in registry.iter() {
        let schema_str = serde_json::to_string(&spec.schema).unwrap_or_else(|_| "{}".to_owned());
        let example_str =
            serde_json::to_string_pretty(&spec.example).unwrap_or_else(|_| "{}".to_owned());
        out.push_str(&format!(
            "\n## {}\n{}\nProps (JSON Schema): {}\nExample:\n```component:{}\n{}\n```\n",
            spec.name, spec.description, schema_str, spec.name, example_str
        ));
    }
    out
}

/// Result of [`split_open_component`] while streaming Markdown.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplitStreamingMarkdown<'a> {
    /// Complete Markdown prefix safe to render immediately.
    pub shown: &'a str,
    /// Component name currently streaming inside an unclosed ```` ```component:<name> ```` fence.
    pub pending_component: Option<String>,
}

/// Detects an unclosed ```` ```component:<name> ```` fence at the end of a streaming Markdown string
/// (properly tracking CommonMark backtick/tilde fence nesting).
pub fn split_open_component<'a>(markdown: &'a str, streaming: bool) -> SplitStreamingMarkdown<'a> {
    if !streaming {
        return SplitStreamingMarkdown {
            shown: markdown,
            pending_component: None,
        };
    }

    let mut open_fence: Option<(u8, usize, usize, Option<String>)> = None; // (ch, len, byte_start, component_name)
    let mut offset = 0usize;

    for raw_line in markdown.split_inclusive('\n') {
        let line = raw_line.trim_end_matches(['\r', '\n']);
        let trimmed = line.trim_start_matches(' ');
        let leading_spaces = line.len() - trimmed.len();

        if leading_spaces <= 3 {
            let first = trimmed.as_bytes().first().copied();
            if let Some(ch @ (b'`' | b'~')) = first {
                let run_len = trimmed.bytes().take_while(|&b| b == ch).count();
                if run_len >= 3 {
                    let rest = trimmed[run_len..].trim();
                    match open_fence {
                        None => {
                            let comp = rest
                                .strip_prefix("component:")
                                .map(|s| s.split_whitespace().next().unwrap_or("").to_owned())
                                .filter(|s| !s.is_empty());
                            open_fence = Some((ch, run_len, offset, comp));
                        }
                        Some((open_ch, open_len, _, _)) => {
                            if ch == open_ch && run_len >= open_len && rest.is_empty() {
                                open_fence = None;
                            }
                        }
                    }
                }
            }
        }
        offset += raw_line.len();
    }

    if let Some((_, _, start, Some(comp_name))) = open_fence {
        return SplitStreamingMarkdown {
            shown: markdown[..start].trim_end(),
            pending_component: Some(comp_name),
        };
    }

    SplitStreamingMarkdown {
        shown: markdown,
        pending_component: None,
    }
}

/// Extracts all closed ```` ```component:<name>\n<json>\n``` ```` blocks from Markdown text.
pub fn extract_component_blocks(markdown: &str) -> Vec<(String, String)> {
    let mut blocks = Vec::new();
    let mut open: Option<(u8, usize, String, String)> = None;

    for raw_line in markdown.lines() {
        let line = raw_line.trim_end_matches('\r');
        let trimmed = line.trim_start_matches(' ');
        let leading = line.len() - trimmed.len();
        if leading <= 3
            && let Some(ch @ (b'`' | b'~')) = trimmed.as_bytes().first().copied()
        {
            let run_len = trimmed.bytes().take_while(|&b| b == ch).count();
            if run_len >= 3 {
                let rest = trimmed[run_len..].trim();
                match open {
                    None => {
                        if let Some(name) = rest
                            .strip_prefix("component:")
                            .and_then(|s| s.split_whitespace().next())
                            .filter(|s| !s.is_empty())
                        {
                            open = Some((ch, run_len, name.to_owned(), String::new()));
                            continue;
                        }
                    }
                    Some((och, olen, ref name, ref body))
                        if ch == och && run_len >= olen && rest.is_empty() =>
                    {
                        blocks.push((name.clone(), body.clone()));
                        open = None;
                        continue;
                    }
                    _ => {}
                }
            }
        }
        if let Some((_, _, _, ref mut body)) = open {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(line);
        }
    }
    blocks
}

// ---------------------------------------------------------------------------
// Built-in GPUI Answer Components
// ---------------------------------------------------------------------------

/// Returns the default suite of GPUI-native generative Answer Components.
pub fn default_answer_components() -> Vec<AnswerComponentSpec> {
    vec![
        stat_card_component(),
        callout_component(),
        bar_chart_component(),
        sparkline_component(),
        data_table_component(),
        key_value_component(),
        badge_list_component(),
    ]
}

fn stat_card_component() -> AnswerComponentSpec {
    AnswerComponentSpec {
        name: "stat-card".into(),
        label: "Metric figure".into(),
        description: "One highlighted KPI figure with label, value, optional note, and optional numeric percentage delta.".into(),
        schema: json!({
            "type": "object",
            "required": ["label", "value"],
            "properties": {
                "label": { "type": "string" },
                "value": { "type": "string" },
                "note": { "type": "string" },
                "delta": { "type": "number" },
                "lowerIsBetter": { "type": "boolean" }
            }
        }),
        example: json!({
            "label": "Open issues",
            "value": "12",
            "delta": -8.5,
            "note": "Down from last week"
        }),
        render: Rc::new(|props, _window, cx| {
            let label = props
                .get("label")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "stat-card requires string 'label'".to_owned())?
                .to_owned();
            let value = props
                .get("value")
                .and_then(Value::as_str)
                .filter(|s| !s.trim().is_empty())
                .ok_or_else(|| "stat-card requires string 'value'".to_owned())?
                .to_owned();
            let note = props.get("note").and_then(Value::as_str).map(ToOwned::to_owned);
            let delta = props.get("delta").and_then(Value::as_f64);
            let lower_is_better = props
                .get("lowerIsBetter")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let theme = cx.theme();

            Ok(div()
                .w_full()
                .p(px(14.))
                .rounded(theme.radius_lg)
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap(px(4.))
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.muted_foreground)
                                .child(label),
                        )
                        .when_some(delta, |el, d| {
                            el.child(trend_badge(d).polarity(if lower_is_better {
                                TrendPolarity::LowerIsBetter
                            } else {
                                TrendPolarity::HigherIsBetter
                            }))
                        }),
                )
                .child(
                    div()
                        .text_size(px(22.))
                        .font_weight(FontWeight::BOLD)
                        .font_features(tabular_numerals())
                        .text_color(theme.foreground)
                        .child(value),
                )
                .when_some(note, |el, n| {
                    el.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .child(n),
                    )
                })
                .into_any_element())
        }),
    }
}

fn callout_component() -> AnswerComponentSpec {
    AnswerComponentSpec {
        name: "callout".into(),
        label: "Callout".into(),
        description: "Highlighted notice card (`info`, `success`, `warning`, or `destructive`) with title and body text.".into(),
        schema: json!({
            "type": "object",
            "required": ["title", "body"],
            "properties": {
                "variant": { "type": "string", "enum": ["info", "success", "warning", "destructive"] },
                "title": { "type": "string" },
                "body": { "type": "string" }
            }
        }),
        example: json!({
            "variant": "info",
            "title": "Tip",
            "body": "Settings are persisted locally."
        }),
        render: Rc::new(|props, _window, _cx| {
            let title = props
                .get("title")
                .and_then(Value::as_str)
                .ok_or_else(|| "callout requires string 'title'".to_owned())?
                .to_owned();
            let body = props
                .get("body")
                .and_then(Value::as_str)
                .ok_or_else(|| "callout requires string 'body'".to_owned())?
                .to_owned();
            let variant = match props.get("variant").and_then(Value::as_str).unwrap_or("info") {
                "success" => AlertVariant::Success,
                "warning" => AlertVariant::Warning,
                "destructive" | "error" => AlertVariant::Destructive,
                _ => AlertVariant::Info,
            };
            Ok(Alert::new(title)
                .variant(variant)
                .description(body)
                .into_any_element())
        }),
    }
}

fn bar_chart_component() -> AnswerComponentSpec {
    AnswerComponentSpec {
        name: "bar-chart".into(),
        label: "Bar chart".into(),
        description: "Ranked horizontal proportional bar chart from an array of `{ label, value }` items.".into(),
        schema: json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "title": { "type": "string" },
                "format": { "type": "string", "enum": ["compact", "integer", "percent", "currency"] },
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["label", "value"],
                        "properties": {
                            "label": { "type": "string" },
                            "value": { "type": "number" },
                            "subtitle": { "type": "string" }
                        }
                    }
                }
            }
        }),
        example: json!({
            "title": "Breakdown",
            "items": [{ "label": "Alpha", "value": 64 }, { "label": "Beta", "value": 36 }]
        }),
        render: Rc::new(|props, _window, cx| {
            let raw_items = props
                .get("items")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty())
                .ok_or_else(|| "bar-chart requires non-empty 'items' array".to_owned())?;
            let title = props.get("title").and_then(Value::as_str).map(ToOwned::to_owned);
            let format = match props.get("format").and_then(Value::as_str).unwrap_or("compact") {
                "currency" => ChartValueFormat::Currency,
                "percent" => ChartValueFormat::Percent,
                "integer" => ChartValueFormat::Integer,
                _ => ChartValueFormat::Compact,
            };
            let mut items = Vec::with_capacity(raw_items.len());
            for row in raw_items.iter().take(24) {
                let lbl = row
                    .get("label")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "bar-chart item missing 'label'".to_owned())?;
                let val = row
                    .get("value")
                    .and_then(Value::as_f64)
                    .filter(|v| v.is_finite())
                    .ok_or_else(|| "bar-chart item missing numeric 'value'".to_owned())?;
                let mut item = BreakdownItem::new(lbl.to_owned(), val);
                if let Some(sub) = row.get("subtitle").and_then(Value::as_str) {
                    item = item.subtitle(sub.to_owned());
                }
                items.push(item);
            }
            let theme = cx.theme();
            let id_hash = format!("ac-bar-{}", title.as_deref().unwrap_or("chart"));
            Ok(div()
                .w_full()
                .p(px(14.))
                .rounded(theme.radius_lg)
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap(px(8.))
                .when_some(title, |el, t| {
                    el.child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(t),
                    )
                })
                .child(breakdown_list(id_hash).items(items).value_format(format))
                .into_any_element())
        }),
    }
}

fn sparkline_component() -> AnswerComponentSpec {
    AnswerComponentSpec {
        name: "sparkline".into(),
        label: "Sparkline chart".into(),
        description: "Compact trend chart (`area`, `line`, or `bars`) over an array of numbers with optional baseline.".into(),
        schema: json!({
            "type": "object",
            "required": ["values"],
            "properties": {
                "title": { "type": "string" },
                "kind": { "type": "string", "enum": ["area", "line", "bars"] },
                "values": { "type": "array", "items": { "type": "number" } },
                "baseline": { "type": "array", "items": { "type": "number" } }
            }
        }),
        example: json!({
            "title": "Weekly trend",
            "kind": "area",
            "values": [12, 18, 16, 24, 29, 34]
        }),
        render: Rc::new(|props, _window, cx| {
            let values: Vec<f64> = props
                .get("values")
                .and_then(Value::as_array)
                .ok_or_else(|| "sparkline requires numeric 'values' array".to_owned())?
                .iter()
                .filter_map(Value::as_f64)
                .filter(|v| v.is_finite())
                .take(200)
                .collect();
            if values.len() < 2 {
                return Err("sparkline requires at least 2 numeric values".to_owned());
            }
            let baseline: Option<Vec<f64>> = props
                .get("baseline")
                .and_then(Value::as_array)
                .map(|arr| {
                    arr.iter()
                        .filter_map(Value::as_f64)
                        .filter(|v| v.is_finite())
                        .take(200)
                        .collect()
                })
                .filter(|v: &Vec<f64>| v.len() >= 2);
            let title = props.get("title").and_then(Value::as_str).map(ToOwned::to_owned);
            let kind = match props.get("kind").and_then(Value::as_str).unwrap_or("area") {
                "line" => SparklineKind::Line,
                "bars" => SparklineKind::Bars,
                _ => SparklineKind::Area,
            };
            let theme = cx.theme();
            let id_str = format!("ac-spark-{}", title.as_deref().unwrap_or("series"));
            let mut chart = sparkline(id_str, values).kind(kind).height(px(58.));
            if let Some(base) = baseline {
                chart = chart.baseline(base);
            }
            Ok(div()
                .w_full()
                .p(px(14.))
                .rounded(theme.radius_lg)
                .bg(theme.surface)
                .border_1()
                .border_color(theme.border)
                .flex()
                .flex_col()
                .gap(px(8.))
                .when_some(title, |el, t| {
                    el.child(
                        div()
                            .text_size(px(13.))
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(t),
                    )
                })
                .child(chart)
                .into_any_element())
        }),
    }
}

fn data_table_component() -> AnswerComponentSpec {
    AnswerComponentSpec {
        name: "data-table".into(),
        label: "Data table".into(),
        description: "Structured table rendered from `columns` (string headers) and `rows` (2D array of strings).".into(),
        schema: json!({
            "type": "object",
            "required": ["columns", "rows"],
            "properties": {
                "columns": { "type": "array", "items": { "type": "string" } },
                "rows": { "type": "array", "items": { "type": "array", "items": { "type": "string" } } }
            }
        }),
        example: json!({
            "columns": ["Item", "Status"],
            "rows": [["API", "Healthy"], ["Worker", "Idle"]]
        }),
        render: Rc::new(|props, _window, _cx| {
            let cols: Vec<TableColumn> = props
                .get("columns")
                .and_then(Value::as_array)
                .filter(|c| !c.is_empty())
                .ok_or_else(|| "data-table requires non-empty 'columns'".to_owned())?
                .iter()
                .filter_map(Value::as_str)
                .take(12)
                .map(|c| TableColumn::new(c.to_owned()))
                .collect();
            if cols.is_empty() {
                return Err("data-table requires string column names".to_owned());
            }
            let raw_rows = props
                .get("rows")
                .and_then(Value::as_array)
                .ok_or_else(|| "data-table requires 'rows' array".to_owned())?;
            let mut rows = Vec::with_capacity(raw_rows.len());
            for raw_row in raw_rows.iter().take(50) {
                let mut tr = TableRow::new();
                if let Some(cells) = raw_row.as_array() {
                    for cell in cells.iter().take(cols.len()) {
                        let text = match cell {
                            Value::String(s) => s.clone(),
                            other => other.to_string(),
                        };
                        tr = tr.text(text);
                    }
                }
                rows.push(tr);
            }
            Ok(Table::new("ac-data-table", cols)
                .compact(true)
                .striped(true)
                .rows(rows)
                .into_any_element())
        }),
    }
}

fn key_value_component() -> AnswerComponentSpec {
    AnswerComponentSpec {
        name: "key-value".into(),
        label: "Summary grid".into(),
        description: "Multi-column key/value metadata summary card.".into(),
        schema: json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "title": { "type": "string" },
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["label", "value"],
                        "properties": {
                            "label": { "type": "string" },
                            "value": { "type": "string" }
                        }
                    }
                }
            }
        }),
        example: json!({
            "title": "Configuration",
            "items": [{ "label": "Region", "value": "us-east-1" }, { "label": "Mode", "value": "Live" }]
        }),
        render: Rc::new(|props, _window, _cx| {
            let raw = props
                .get("items")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty())
                .ok_or_else(|| "key-value requires non-empty 'items'".to_owned())?;
            let mut desc = descriptions("ac-key-value").columns(2).bordered(true);
            if let Some(title) = props.get("title").and_then(Value::as_str) {
                desc = desc.title(title.to_owned());
            }
            for item in raw.iter().take(24) {
                let label = item
                    .get("label")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "key-value item missing 'label'".to_owned())?;
                let value = match item.get("value") {
                    Some(Value::String(s)) => s.clone(),
                    Some(other) => other.to_string(),
                    None => return Err("key-value item missing 'value'".to_owned()),
                };
                desc = desc.item(DescriptionItem::new(label.to_owned(), value));
            }
            Ok(desc.into_any_element())
        }),
    }
}

fn badge_list_component() -> AnswerComponentSpec {
    AnswerComponentSpec {
        name: "badge-list".into(),
        label: "Tags".into(),
        description: "Horizontal wrap of semantic pill badges.".into(),
        schema: json!({
            "type": "object",
            "required": ["items"],
            "properties": {
                "items": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["label"],
                        "properties": {
                            "label": { "type": "string" },
                            "color": { "type": "string", "enum": ["gray", "indigo", "green", "amber", "rose"] }
                        }
                    }
                }
            }
        }),
        example: json!({
            "items": [{ "label": "Verified", "color": "green" }, { "label": "v1.2", "color": "indigo" }]
        }),
        render: Rc::new(|props, _window, _cx| {
            let raw = props
                .get("items")
                .and_then(Value::as_array)
                .filter(|a| !a.is_empty())
                .ok_or_else(|| "badge-list requires non-empty 'items'".to_owned())?;
            let mut row = div().flex().flex_row().flex_wrap().gap(px(6.));
            for item in raw.iter().take(32) {
                let label = item
                    .get("label")
                    .and_then(Value::as_str)
                    .ok_or_else(|| "badge-list item missing 'label'".to_owned())?;
                let color = match item.get("color").and_then(Value::as_str).unwrap_or("gray") {
                    "indigo" | "blue" => BadgeColor::Indigo,
                    "green" | "emerald" => BadgeColor::Green,
                    "amber" | "yellow" => BadgeColor::Amber,
                    "rose" | "red" => BadgeColor::Red,
                    _ => BadgeColor::Gray,
                };
                row = row.child(badge(label.to_owned()).color(color).pill(true));
            }
            Ok(row.into_any_element())
        }),
    }
}

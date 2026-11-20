//! Shared completion / suggestion engine for text inputs ([`super::input::InputState`],
//! [`super::transfer_tags::TagInputState`]) with synchronous and asynchronous data sources,
//! fuzzy filtering & ranking, grouped virtualized popovers, and active-descendant keyboard
//! navigation (`Down`/`Up`/`PageDown`/`PageUp`/`Enter`/`Escape`/`Tab`).

use std::cell::Cell;
use std::collections::HashMap;
use std::ops::Range;
use std::rc::Rc;
use std::time::Duration;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, AsyncApp, Bounds, Context, Entity,
    FontWeight, IntoElement, KeyDownEvent, MouseButton, MouseDownEvent, Pixels, ScrollStrategy,
    SharedString, Task, UniformListScrollHandle, WeakEntity, Window, anchored, canvas, deferred,
    div, ease_out_quint, point, prelude::*, px, transparent_black, uniform_list,
};

use super::feedback::{Spinner, SpinnerVariant};
use super::focus::list_step;
use super::theme::{ANIM_FAST, ActiveTheme};

const DEFAULT_ASYNC_DEBOUNCE: Duration = Duration::from_millis(150);
const COMPACT_ROW_HEIGHT: f32 = 34.0;
const DETAILED_ROW_HEIGHT: f32 = 46.0;
const PAGE_STEP_SIZE: usize = 6;

/// A single completion / autocomplete suggestion item.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SuggestionItem {
    /// Canonical value inserted into an [`super::input::Input`] or added as a chip in a
    /// [`super::transfer_tags::TagInput`].
    pub value: SharedString,
    /// Primary display label shown in the suggestion row.
    pub label: SharedString,
    /// Optional muted secondary description shown below the label.
    pub description: Option<SharedString>,
    /// Optional section group header (items sharing a group are rendered together).
    pub group: Option<SharedString>,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
    /// Optional trailing badge / hint text (e.g. `"recent"`, `"tag"`, `"v2"`).
    pub badge: Option<SharedString>,
    /// Additional search keywords matched during local fuzzy filtering.
    pub keywords: Vec<SharedString>,
    /// Prevents highlighting or accepting this suggestion.
    pub disabled: bool,
}

impl SuggestionItem {
    /// Creates a suggestion whose `value` and `label` are both `value`.
    pub fn new(value: impl Into<SharedString>) -> Self {
        let value: SharedString = value.into();
        Self {
            label: value.clone(),
            value,
            description: None,
            group: None,
            icon: None,
            badge: None,
            keywords: Vec::new(),
            disabled: false,
        }
    }

    /// Creates a suggestion with distinct `value` and display `label`.
    pub fn with_label(value: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            value: value.into(),
            label: label.into(),
            description: None,
            group: None,
            icon: None,
            badge: None,
            keywords: Vec::new(),
            disabled: false,
        }
    }

    /// Overrides the display label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    /// Sets a muted description line shown below the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Assigns this suggestion to a visual section group.
    pub fn group(mut self, group: impl Into<SharedString>) -> Self {
        self.group = Some(group.into());
        self
    }

    /// Sets a leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets a trailing pill hint / badge label.
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Sets extra search keywords matched during local fuzzy filtering.
    pub fn keywords(mut self, keywords: impl IntoIterator<Item = impl Into<SharedString>>) -> Self {
        self.keywords = keywords.into_iter().map(Into::into).collect();
        self
    }

    /// Disables highlighting and accepting this suggestion.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

impl From<&'static str> for SuggestionItem {
    fn from(value: &'static str) -> Self {
        Self::new(value)
    }
}

impl From<String> for SuggestionItem {
    fn from(value: String) -> Self {
        Self::new(value)
    }
}

impl From<SharedString> for SuggestionItem {
    fn from(value: SharedString) -> Self {
        Self::new(value)
    }
}

struct IndexedSuggestion {
    item: SuggestionItem,
    label_low: String,
    value_low: String,
    desc_low: Option<String>,
    keywords_low: Vec<String>,
    group_low: Option<String>,
}

impl IndexedSuggestion {
    fn new(item: SuggestionItem) -> Self {
        let label_low = item.label.to_lowercase();
        let value_low = item.value.to_lowercase();
        let desc_low = item.description.as_deref().map(str::to_lowercase);
        let keywords_low = item.keywords.iter().map(|k| k.to_lowercase()).collect();
        let group_low = item.group.as_deref().map(str::to_lowercase);
        Self {
            item,
            label_low,
            value_low,
            desc_low,
            keywords_low,
            group_low,
        }
    }

    fn score(&self, q_low: &str) -> Option<i32> {
        if q_low.is_empty() {
            return Some(0);
        }
        let label_score = fuzzy_match_score_lower(q_low, &self.label_low);
        let value_score = (self.value_low != self.label_low)
            .then(|| fuzzy_match_score_lower(q_low, &self.value_low))
            .flatten()
            .map(|s| s - 5);
        let desc_score = self
            .desc_low
            .as_deref()
            .and_then(|d| fuzzy_match_score_lower(q_low, d))
            .map(|s| s - 15);
        let kw_score = self
            .keywords_low
            .iter()
            .filter_map(|kw| fuzzy_match_score_lower(q_low, kw))
            .max()
            .map(|s| s - 20);
        let grp_score = self
            .group_low
            .as_deref()
            .and_then(|g| fuzzy_match_score_lower(q_low, g))
            .map(|s| s - 25);

        [label_score, value_score, desc_score, kw_score, grp_score]
            .into_iter()
            .flatten()
            .max()
    }
}

/// Scores a fuzzy subsequence / substring match where `q_low` and `hay_low` are already lowercased.
///
/// Exact matches rank highest (`1000`), followed by prefix matches (`600`), substring matches
/// (`350`), and ordered character subsequences with word-boundary and adjacency bonuses (capped
/// at `240` so contiguous matches always outrank scattered ones).
pub(crate) fn fuzzy_match_score_lower(q_low: &str, hay_low: &str) -> Option<i32> {
    if q_low.is_empty() {
        return Some(0);
    }
    if hay_low == q_low {
        return Some(1000);
    }
    if let Some(pos) = hay_low.find(q_low) {
        let base = if pos == 0 { 600 } else { 350 };
        return Some(base - (pos as i32).min(80));
    }
    let mut score = 0i32;
    let mut q_chars = q_low.chars().peekable();
    let mut prev_matched = false;
    let mut prev_char = ' ';
    for ch in hay_low.chars() {
        if let Some(&target) = q_chars.peek()
            && ch == target
        {
            q_chars.next();
            score += 10;
            if prev_matched {
                score += 8;
            }
            if matches!(prev_char, ' ' | '-' | '_' | '/' | '.' | ':') {
                score += 6;
            }
            prev_matched = true;
        } else {
            prev_matched = false;
        }
        prev_char = ch;
    }
    if q_chars.peek().is_none() {
        Some(score.min(240))
    } else {
        None
    }
}

/// Scores a case-insensitive fuzzy match of `query` against `text`.
///
/// Returns `None` if `query` does not match `text`. Higher scores indicate stronger matches
/// (exact > prefix > substring > ordered subsequence).
pub fn fuzzy_match_score(query: &str, text: &str) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return Some(0);
    }
    let hay = text.to_lowercase();
    fuzzy_match_score_lower(&q, &hay)
}

/// Filters and ranks `items` against `query` using [`fuzzy_match_score`] across each item's
/// `label`, `value`, `description`, `keywords`, and `group`.
///
/// Useful both internally for local filtering and inside custom background async loaders.
pub fn filter_suggestions(query: &str, items: Vec<SuggestionItem>) -> Vec<SuggestionItem> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return items;
    }
    let mut scored: Vec<(i32, usize, SuggestionItem)> = items
        .into_iter()
        .enumerate()
        .filter_map(|(idx, item)| {
            let indexed = IndexedSuggestion::new(item);
            let score = indexed.score(&q)?;
            Some((score, idx, indexed.item))
        })
        .collect();
    scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    scored.into_iter().map(|(_, _, item)| item).collect()
}

type DynamicSuggestionLoader = Rc<dyn Fn(&App) -> Vec<SuggestionItem>>;
type SyncSuggestionLoader = Rc<dyn Fn(&str, &App) -> Vec<SuggestionItem>>;
type AsyncSuggestionLoader =
    Rc<dyn Fn(&str, &App) -> Task<Result<Vec<SuggestionItem>, SharedString>>>;

#[derive(Clone)]
enum SuggestionSourceKind {
    Static(Rc<Vec<IndexedSuggestion>>),
    Dynamic(DynamicSuggestionLoader),
    Sync(SyncSuggestionLoader),
    Async(AsyncSuggestionLoader),
}

/// Data source powering completion suggestions (static, dynamic sync, query-aware sync, or async).
#[derive(Clone)]
pub struct SuggestionSource {
    kind: SuggestionSourceKind,
    debounce: Duration,
    filter_locally: bool,
}

impl SuggestionSource {
    /// Static catalog of suggestions, pre-indexed and fuzzy-filtered locally on every keystroke.
    pub fn items(items: impl IntoIterator<Item = impl Into<SuggestionItem>>) -> Self {
        let indexed: Vec<IndexedSuggestion> = items
            .into_iter()
            .map(|item| IndexedSuggestion::new(item.into()))
            .collect();
        Self {
            kind: SuggestionSourceKind::Static(Rc::new(indexed)),
            debounce: Duration::ZERO,
            filter_locally: true,
        }
    }

    /// Synchronous dynamic catalog read from `&App` on each query and fuzzy-filtered locally.
    ///
    /// Use this when suggestions live in a GPUI entity or global and change over time, while
    /// delegating on-type filtering and ranking to GPUI's built-in fuzzy matcher.
    pub fn dynamic(loader: impl Fn(&App) -> Vec<SuggestionItem> + 'static) -> Self {
        Self {
            kind: SuggestionSourceKind::Dynamic(Rc::new(loader)),
            debounce: Duration::ZERO,
            filter_locally: true,
        }
    }

    /// Synchronous query-aware provider invoked immediately on every edit.
    ///
    /// By default `filter_locally` is `false` because the provider receives `query` directly;
    /// chain `.filter_locally(true)` to also fuzzy-filter and rank its returned items locally.
    pub fn sync(loader: impl Fn(&str, &App) -> Vec<SuggestionItem> + 'static) -> Self {
        Self {
            kind: SuggestionSourceKind::Sync(Rc::new(loader)),
            debounce: Duration::ZERO,
            filter_locally: false,
        }
    }

    /// Asynchronous fallible provider returning a GPUI [`Task`].
    ///
    /// Defaults to a `150ms` debounce window; in-flight tasks are automatically cancelled and
    /// stale out-of-order responses discarded whenever the query changes or the popup closes.
    pub fn async_loader(
        loader: impl Fn(&str, &App) -> Task<Result<Vec<SuggestionItem>, SharedString>> + 'static,
    ) -> Self {
        Self {
            kind: SuggestionSourceKind::Async(Rc::new(loader)),
            debounce: DEFAULT_ASYNC_DEBOUNCE,
            filter_locally: false,
        }
    }

    /// Asynchronous infallible convenience provider returning `Task<Vec<SuggestionItem>>`.
    ///
    /// Defaults to a `150ms` debounce window.
    pub fn async_fn(loader: impl Fn(&str, &App) -> Task<Vec<SuggestionItem>> + 'static) -> Self {
        Self::async_loader(move |query, cx| {
            let task = loader(query, cx);
            cx.foreground_executor()
                .spawn(async move { Ok(task.await) })
        })
    }

    /// Sets the debounce duration before invoking an async loader (default `150ms`).
    pub fn debounce(mut self, duration: Duration) -> Self {
        self.debounce = duration;
        self
    }

    /// Controls whether GPUI applies built-in fuzzy filtering and ranking to items returned
    /// by this source.
    pub fn filter_locally(mut self, filter: bool) -> Self {
        self.filter_locally = filter;
        self
    }
}

/// Configuration controlling how a completion / suggestion popover opens, highlights, and accepts.
#[derive(Clone, Debug)]
pub struct CompletionConfig {
    /// Minimum trimmed character count required before querying/opening (default `0`).
    pub min_chars: usize,
    /// Open suggestions automatically when the host input receives focus.
    pub open_on_focus: bool,
    /// Automatically highlight the first enabled suggestion whenever results update.
    ///
    /// Defaults to `false` for [`Self::search`] (so `Enter` submits raw text unless the user
    /// arrows down) and `true` for [`Self::tags`] (so typing + `Enter` completes the top tag).
    pub auto_highlight: bool,
    /// When `true`, pressing `Tab` while a suggestion is highlighted accepts it and keeps focus.
    /// When `false` (default, matching browser focus rules), `Tab` closes the popover and moves
    /// focus to the next control.
    pub accept_on_tab: bool,
    /// Close the popover after accepting a suggestion (`true` for search, `false` for tags).
    pub close_on_accept: bool,
    /// Maximum scrollable height of the suggestion list (default `260px`).
    pub max_height: Pixels,
    /// Optional message shown when open with 0 matching suggestions. When `None`, the popover
    /// hides cleanly when there are no matches.
    pub empty_message: Option<SharedString>,
    /// Label displayed beside the spinner while an async source is loading.
    pub loading_message: SharedString,
    /// Optional upper bound on how many suggestions are shown (`None` = unlimited).
    pub max_items: Option<usize>,
}

impl Default for CompletionConfig {
    fn default() -> Self {
        Self::search()
    }
}

impl CompletionConfig {
    /// Preset tuned for search boxes and autocomplete text fields.
    ///
    /// - `open_on_focus: false` (opens on typing or pressing `Down`/`Up`)
    /// - `auto_highlight: false` (pressing `Enter` submits the typed query unless you arrow down)
    /// - `close_on_accept: true`
    pub fn search() -> Self {
        Self {
            min_chars: 0,
            open_on_focus: false,
            auto_highlight: false,
            accept_on_tab: false,
            close_on_accept: true,
            max_height: px(260.),
            empty_message: None,
            loading_message: SharedString::new_static("Loading suggestions..."),
            max_items: None,
        }
    }

    /// Preset tuned for multi-tag chip inputs ([`super::transfer_tags::TagInput`]).
    ///
    /// - `open_on_focus: true` (shows available tags immediately on focus)
    /// - `auto_highlight: true` (typing + `Enter` accepts the top matching suggestion)
    /// - `close_on_accept: false` (stays open to pick additional tags)
    pub fn tags() -> Self {
        Self {
            min_chars: 0,
            open_on_focus: true,
            auto_highlight: true,
            accept_on_tab: false,
            close_on_accept: false,
            max_height: px(240.),
            empty_message: None,
            loading_message: SharedString::new_static("Loading tags..."),
            max_items: None,
        }
    }

    /// Sets the minimum trimmed character count before triggering suggestions.
    pub fn min_chars(mut self, min_chars: usize) -> Self {
        self.min_chars = min_chars;
        self
    }

    /// Controls whether focusing the input automatically opens suggestions.
    pub fn open_on_focus(mut self, open: bool) -> Self {
        self.open_on_focus = open;
        self
    }

    /// Controls whether the first enabled suggestion is automatically highlighted on update.
    pub fn auto_highlight(mut self, auto: bool) -> Self {
        self.auto_highlight = auto;
        self
    }

    /// Controls whether `Tab` accepts a highlighted suggestion instead of moving focus on.
    pub fn accept_on_tab(mut self, accept: bool) -> Self {
        self.accept_on_tab = accept;
        self
    }

    /// Controls whether accepting a suggestion closes the popover.
    pub fn close_on_accept(mut self, close: bool) -> Self {
        self.close_on_accept = close;
        self
    }

    /// Sets the maximum height of the scrollable popover list.
    pub fn max_height(mut self, height: Pixels) -> Self {
        self.max_height = if height.0.is_finite() {
            height.max(px(80.))
        } else {
            px(260.)
        };
        self
    }

    /// Shows `message` when no suggestions match (pass `None` to hide the popover when empty).
    pub fn empty_message(mut self, message: impl Into<Option<SharedString>>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Sets the label shown next to the loading spinner during async fetches.
    pub fn loading_message(mut self, message: impl Into<SharedString>) -> Self {
        self.loading_message = message.into();
        self
    }

    /// Limits the maximum number of suggestions displayed.
    pub fn max_items(mut self, max_items: Option<usize>) -> Self {
        self.max_items = max_items;
        self
    }
}

/// Which edge of the list to highlight once suggestions arrive.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum HighlightEdge {
    First,
    Last,
}

/// Result of routing a [`KeyDownEvent`] through [`CompletionController::handle_key_down`].
pub(crate) enum CompletionKeyOutcome {
    /// Key does not belong to completion navigation; let normal handlers run.
    Ignored,
    /// Key was consumed (e.g. arrow movement or Escape closing a visible popover).
    Consumed,
    /// Popover was closed on Tab and focus should continue moving naturally.
    ClosedPassThrough,
    /// Popover was closed and the user pressed Down/Up to open and highlight an edge item.
    RequestOpen(HighlightEdge),
    /// User accepted the highlighted suggestion (via fallback Enter or `accept_on_tab` Tab).
    Accept(SuggestionItem),
}

#[derive(Clone)]
enum CompletionRow {
    GroupHeader(SharedString),
    Item(usize),
}

/// Internal controller owned by inputs and tag inputs that have completions enabled.
pub(crate) struct CompletionController {
    source: SuggestionSource,
    config: CompletionConfig,
    open: bool,
    loading: bool,
    error: Option<SharedString>,
    query: SharedString,
    /// Items ordered strictly in top-to-bottom visual display order (matching rendered groups).
    items: Rc<Vec<SuggestionItem>>,
    rows: Rc<Vec<CompletionRow>>,
    item_to_row: Vec<usize>,
    has_descriptions: bool,
    highlighted: Option<usize>,
    pending_highlight_on_load: Option<HighlightEdge>,
    scroll_handle: UniformListScrollHandle,
    anchor_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    generation: u64,
    pending_task: Option<Task<()>>,
}

impl CompletionController {
    pub(crate) fn new(source: SuggestionSource, config: CompletionConfig) -> Self {
        Self {
            source,
            config,
            open: false,
            loading: false,
            error: None,
            query: SharedString::default(),
            items: Rc::new(Vec::new()),
            rows: Rc::new(Vec::new()),
            item_to_row: Vec::new(),
            has_descriptions: false,
            highlighted: None,
            pending_highlight_on_load: None,
            scroll_handle: UniformListScrollHandle::new(),
            anchor_bounds: Rc::new(Cell::new(None)),
            generation: 0,
            pending_task: None,
        }
    }

    pub(crate) fn config(&self) -> &CompletionConfig {
        &self.config
    }

    pub(crate) fn set_config(&mut self, config: CompletionConfig) {
        self.config = config;
    }

    pub(crate) fn set_source(&mut self, source: SuggestionSource) {
        self.source = source;
        self.generation = self.generation.wrapping_add(1);
        self.pending_task = None;
        self.loading = false;
        self.error = None;
    }

    pub(crate) fn is_open(&self) -> bool {
        self.open
    }

    /// Returns `true` when the popover has something visible to render (items, loading, error,
    /// or an explicit empty message).
    pub(crate) fn is_visible_popover(&self) -> bool {
        self.open
            && (self.loading
                || self.error.is_some()
                || !self.items.is_empty()
                || self.config.empty_message.is_some())
    }

    pub(crate) fn is_loading(&self) -> bool {
        self.loading
    }

    pub(crate) fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    pub(crate) fn items(&self) -> &[SuggestionItem] {
        &self.items
    }

    pub(crate) fn highlighted_index(&self) -> Option<usize> {
        self.highlighted
    }

    pub(crate) fn highlighted_item(&self) -> Option<&SuggestionItem> {
        self.highlighted
            .and_then(|idx| self.items.get(idx))
            .filter(|item| !item.disabled)
    }

    pub(crate) fn set_pending_highlight(&mut self, edge: HighlightEdge) {
        self.pending_highlight_on_load = Some(edge);
    }

    /// Closes the popover and cancels any in-flight async fetch.
    pub(crate) fn close(&mut self) {
        self.generation = self.generation.wrapping_add(1);
        self.pending_task = None;
        self.open = false;
        self.loading = false;
        self.error = None;
        self.highlighted = None;
        self.pending_highlight_on_load = None;
        self.items = Rc::new(Vec::new());
        self.rows = Rc::new(Vec::new());
        self.item_to_row.clear();
        self.has_descriptions = false;
    }

    fn apply_items(&mut self, mut items: Vec<SuggestionItem>) {
        if let Some(max) = self.config.max_items {
            items.truncate(max);
        }

        let has_groups = items.iter().any(|item| item.group.is_some());
        let (ordered_items, rows, item_to_row) = if has_groups {
            let mut group_map: HashMap<Option<SharedString>, usize> = HashMap::new();
            let mut grouped: Vec<(Option<SharedString>, Vec<SuggestionItem>)> = Vec::new();
            for item in items {
                let key = item.group.clone();
                if let Some(&pos) = group_map.get(&key) {
                    grouped[pos].1.push(item);
                } else {
                    let pos = grouped.len();
                    group_map.insert(key.clone(), pos);
                    grouped.push((key, vec![item]));
                }
            }

            let mut ordered = Vec::new();
            let mut rows = Vec::new();
            let mut item_to_row = Vec::new();
            for (group_name, group_items) in grouped {
                if let Some(name) = group_name {
                    rows.push(CompletionRow::GroupHeader(name));
                }
                for item in group_items {
                    let item_idx = ordered.len();
                    let row_idx = rows.len();
                    ordered.push(item);
                    item_to_row.push(row_idx);
                    rows.push(CompletionRow::Item(item_idx));
                }
            }
            (ordered, rows, item_to_row)
        } else {
            let len = items.len();
            let rows: Vec<CompletionRow> = (0..len).map(CompletionRow::Item).collect();
            let item_to_row: Vec<usize> = (0..len).collect();
            (items, rows, item_to_row)
        };

        self.has_descriptions = ordered_items.iter().any(|item| item.description.is_some());
        self.items = Rc::new(ordered_items);
        self.rows = Rc::new(rows);
        self.item_to_row = item_to_row;
        self.loading = false;
        self.error = None;

        self.highlighted = match self.pending_highlight_on_load.take() {
            Some(HighlightEdge::First) => self.items.iter().position(|item| !item.disabled),
            Some(HighlightEdge::Last) => self.items.iter().rposition(|item| !item.disabled),
            None if self.config.auto_highlight => self.items.iter().position(|item| !item.disabled),
            None => None,
        };

        if let Some(idx) = self.highlighted
            && let Some(&row_idx) = self.item_to_row.get(idx)
        {
            self.scroll_handle
                .scroll_to_item(row_idx, ScrollStrategy::Nearest);
        } else {
            self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        }
    }

    fn apply_error(&mut self, err: SharedString) {
        self.loading = false;
        self.error = Some(err);
        self.highlighted = None;
        self.pending_highlight_on_load = None;
        self.items = Rc::new(Vec::new());
        self.rows = Rc::new(Vec::new());
        self.item_to_row.clear();
        self.has_descriptions = false;
    }

    /// Handles completion navigation keys (`Down`, `Up`, `PageDown`, `PageUp`, `Escape`, `Tab`,
    /// and fallback `Enter`). Always leaves `Left`/`Right`/`Home`/`End` untouched for the text
    /// input caret.
    pub(crate) fn handle_key_down(&mut self, event: &KeyDownEvent) -> CompletionKeyOutcome {
        let mods = &event.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform || mods.function {
            return CompletionKeyOutcome::Ignored;
        }
        let key = event.keystroke.key.as_str();

        if key == "tab" {
            if self.is_visible_popover() {
                if !mods.shift
                    && self.config.accept_on_tab
                    && let Some(item) = self.highlighted_item().cloned()
                {
                    return CompletionKeyOutcome::Accept(item);
                }
                self.close();
                return CompletionKeyOutcome::ClosedPassThrough;
            }
            if self.open {
                self.close();
            }
            return CompletionKeyOutcome::Ignored;
        }

        if mods.shift {
            return CompletionKeyOutcome::Ignored;
        }

        match key {
            "down" | "up" => {
                let edge = if key == "down" {
                    HighlightEdge::First
                } else {
                    HighlightEdge::Last
                };
                if !self.open {
                    return CompletionKeyOutcome::RequestOpen(edge);
                }
                if self.loading && self.items.is_empty() {
                    self.pending_highlight_on_load = Some(edge);
                    return CompletionKeyOutcome::Consumed;
                }
                if !self.items.is_empty() {
                    let enabled: Vec<bool> = self.items.iter().map(|i| !i.disabled).collect();
                    if let Some(next) = list_step(key, self.highlighted, &enabled, PAGE_STEP_SIZE, false) {
                        self.highlighted = Some(next);
                        if let Some(&row_idx) = self.item_to_row.get(next) {
                            self.scroll_handle
                                .scroll_to_item(row_idx, ScrollStrategy::Nearest);
                        }
                    }
                    return CompletionKeyOutcome::Consumed;
                }
                if self.is_visible_popover() {
                    CompletionKeyOutcome::Consumed
                } else {
                    CompletionKeyOutcome::Ignored
                }
            }
            "pagedown" | "pageup" if self.is_visible_popover() => {
                if !self.items.is_empty() {
                    let enabled: Vec<bool> = self.items.iter().map(|i| !i.disabled).collect();
                    if let Some(next) =
                        list_step(key, self.highlighted, &enabled, PAGE_STEP_SIZE, false)
                    {
                        self.highlighted = Some(next);
                        if let Some(&row_idx) = self.item_to_row.get(next) {
                            self.scroll_handle
                                .scroll_to_item(row_idx, ScrollStrategy::Nearest);
                        }
                    }
                    CompletionKeyOutcome::Consumed
                } else if self.loading {
                    CompletionKeyOutcome::Consumed
                } else {
                    CompletionKeyOutcome::Ignored
                }
            }
            "escape" => {
                if self.is_visible_popover() {
                    self.close();
                    CompletionKeyOutcome::Consumed
                } else {
                    if self.open {
                        self.close();
                    }
                    CompletionKeyOutcome::Ignored
                }
            }
            // Fallback when input keybindings (which bind Enter -> Submit) are not installed.
            "enter" if self.is_visible_popover() => {
                if let Some(item) = self.highlighted_item().cloned() {
                    CompletionKeyOutcome::Accept(item)
                } else {
                    CompletionKeyOutcome::Ignored
                }
            }
            _ => CompletionKeyOutcome::Ignored,
        }
    }

    /// Invisible full-size bounds-measuring overlay to place inside the anchor element.
    pub(crate) fn bounds_canvas(&self) -> impl IntoElement {
        let bounds_cell = self.anchor_bounds.clone();
        canvas(
            move |bounds, _, _| {
                bounds_cell.set(Some(bounds));
            },
            |_, _, _, _| {},
        )
        .absolute()
        .size_full()
    }

    /// Renders the anchored deferred completion popover when visible.
    pub(crate) fn render_popover<V: 'static>(
        &self,
        id_prefix: &'static str,
        entity: Entity<V>,
        cx: &App,
        get_ctrl: fn(&mut V) -> Option<&mut CompletionController>,
        on_accept: fn(&mut V, SuggestionItem, &mut Window, &mut Context<V>),
        on_close: fn(&mut V, &mut Context<V>),
    ) -> Option<AnyElement> {
        if !self.is_visible_popover() {
            return None;
        }

        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let entity_id = entity.entity_id();
        let captured_bounds = self.anchor_bounds.get();
        let menu_min_w = captured_bounds
            .map(|b| b.size.width)
            .unwrap_or(px(220.))
            .max(px(180.));

        let mut panel = div()
            .id((id_prefix, entity_id))
            .flex()
            .flex_col()
            .min_w(menu_min_w)
            .max_w(px(520.))
            .rounded(theme.radius_lg)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .overflow_hidden()
            // Clicking inside the suggestion popover keeps focus in the host text input.
            .on_mouse_down(MouseButton::Left, |_, window, _| window.prevent_default())
            .on_mouse_down_out({
                let anchor_bounds = self.anchor_bounds.clone();
                let entity_out = entity.clone();
                move |ev: &MouseDownEvent, _, cx| {
                    if anchor_bounds
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    entity_out.update(cx, |this, cx| {
                        on_close(this, cx);
                    });
                }
            });

        if let Some(err) = self.error.clone() {
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(12.))
                    .text_size(px(12.5))
                    .text_color(theme.destructive)
                    .child(
                        Icon::new(LucideIcon::TriangleExclamationPoint)
                            .size(IconSize::Sm)
                            .color(theme.destructive),
                    )
                    .child(err),
            );
        } else if self.loading && self.items.is_empty() {
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .px(px(12.))
                    .py(px(12.))
                    .text_size(px(12.5))
                    .text_color(theme.muted_foreground)
                    .child(
                        Spinner::new()
                            .variant(SpinnerVariant::Ring)
                            .size(IconSize::Xs),
                    )
                    .child(self.config.loading_message.clone()),
            );
        } else if self.items.is_empty() {
            if let Some(empty) = self.config.empty_message.clone() {
                panel = panel.child(
                    div()
                        .flex()
                        .items_center()
                        .justify_center()
                        .px(px(12.))
                        .py(px(14.))
                        .text_size(px(12.5))
                        .text_color(theme.muted_foreground)
                        .child(empty),
                );
            }
        } else {
            // Subtle top status strip while refreshing stale results asynchronously.
            if self.loading {
                panel = panel.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .px(px(10.))
                        .py(px(6.))
                        .border_b_1()
                        .border_color(theme.border)
                        .bg(theme.surface_muted.opacity(0.45))
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .child(
                            Spinner::new()
                                .variant(SpinnerVariant::Ring)
                                .size(IconSize::Xs),
                        )
                        .child(self.config.loading_message.clone()),
                );
            }

            let row_height = if self.has_descriptions {
                DETAILED_ROW_HEIGHT
            } else {
                COMPACT_ROW_HEIGHT
            };
            let rows_rc = Rc::clone(&self.rows);
            let items_rc = Rc::clone(&self.items);
            let row_count = rows_rc.len();
            let highlighted = self.highlighted;
            let list_h = px((row_count as f32 * row_height).min(self.config.max_height.0));
            let row_id_prefix: SharedString = format!("{id_prefix}-row").into();

            let list = uniform_list(
                ("gpui-completion-vlist", entity_id),
                row_count,
                move |range: Range<usize>, _window: &mut Window, cx: &mut App| {
                    let theme = cx.theme();
                    let hover_bg = theme.surface_hover;
                    range
                        .map(|row_i| match &rows_rc[row_i] {
                            CompletionRow::GroupHeader(group_name) => div()
                                .flex()
                                .items_end()
                                .w_full()
                                .h(px(row_height))
                                .px(px(10.))
                                .pb(px(4.))
                                .text_size(px(11.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.muted_foreground)
                                .child(group_name.clone())
                                .into_any_element(),
                            CompletionRow::Item(item_idx) => {
                                let item_idx = *item_idx;
                                let item = &items_rc[item_idx];
                                let is_highlighted = highlighted == Some(item_idx);
                                let disabled = item.disabled;
                                let row_bg = if is_highlighted {
                                    theme.primary_soft
                                } else {
                                    transparent_black()
                                };
                                let label_fg = if is_highlighted {
                                    theme.primary_soft_foreground
                                } else {
                                    theme.popover_foreground
                                };
                                let icon_fg = if is_highlighted {
                                    theme.primary_soft_foreground
                                } else {
                                    theme.muted_foreground
                                };
                                let ent_hover = entity.clone();
                                let ent_click = entity.clone();
                                let item_for_click = item.clone();

                                div()
                                    .id((row_id_prefix.clone(), item_idx))
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .justify_between()
                                    .gap(px(10.))
                                    .w_full()
                                    .h(px(row_height))
                                    .px(px(10.))
                                    .rounded(theme.radius_sm)
                                    .bg(row_bg)
                                    .when(!disabled, |el| {
                                        el.cursor_pointer()
                                            .when(!is_highlighted, |el| {
                                                el.hover(move |s| s.bg(hover_bg))
                                            })
                                            .on_mouse_move(move |_, _, cx| {
                                                ent_hover.update(cx, |this, cx| {
                                                    if let Some(ctrl) = get_ctrl(this)
                                                        && ctrl.highlighted != Some(item_idx)
                                                    {
                                                        ctrl.highlighted = Some(item_idx);
                                                        cx.notify();
                                                    }
                                                });
                                            })
                                            .on_click(move |_, window, cx| {
                                                let accepted = item_for_click.clone();
                                                ent_click.update(cx, |this, cx| {
                                                    on_accept(this, accepted, window, cx);
                                                });
                                            })
                                    })
                                    .when(disabled, |el| el.opacity(0.45).cursor_not_allowed())
                                    .child(
                                        div()
                                            .flex()
                                            .flex_row()
                                            .items_center()
                                            .gap(px(8.))
                                            .flex_1()
                                            .min_w_0()
                                            .when_some(item.icon, |el, ic| {
                                                el.child(
                                                    Icon::new(ic)
                                                        .size(IconSize::Sm)
                                                        .color(icon_fg),
                                                )
                                            })
                                            .child(
                                                div()
                                                    .flex()
                                                    .flex_col()
                                                    .flex_1()
                                                    .min_w_0()
                                                    .child(
                                                        div()
                                                            .truncate()
                                                            .text_size(px(13.))
                                                            .font_weight(if is_highlighted {
                                                                FontWeight::MEDIUM
                                                            } else {
                                                                FontWeight::NORMAL
                                                            })
                                                            .text_color(label_fg)
                                                            .child(item.label.clone()),
                                                    )
                                                    .when_some(
                                                        item.description.clone(),
                                                        |el, desc| {
                                                            el.child(
                                                                div()
                                                                    .truncate()
                                                                    .text_size(px(11.5))
                                                                    .text_color(
                                                                        theme.muted_foreground,
                                                                    )
                                                                    .child(desc),
                                                            )
                                                        },
                                                    ),
                                            ),
                                    )
                                    .when_some(item.badge.clone(), |el, badge_text| {
                                        el.child(
                                            div()
                                                .flex_shrink_0()
                                                .px(px(6.))
                                                .py(px(1.5))
                                                .rounded(theme.radius_sm)
                                                .bg(theme.surface_muted)
                                                .text_size(px(11.))
                                                .text_color(theme.muted_foreground)
                                                .child(badge_text),
                                        )
                                    })
                                    .into_any_element()
                            }
                        })
                        .collect()
                },
            )
            .track_scroll(&self.scroll_handle)
            .w_full()
            .h(list_h);

            panel = panel.child(div().w_full().p(px(4.)).child(list));
        }

        let animated_panel = if reduce_motion {
            panel.into_any_element()
        } else {
            panel
                .with_animation(
                    ("gpui-completion-anim", entity_id),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).mt(px(-6.0 * (1.0 - delta))),
                )
                .into_any_element()
        };

        let mut anchor = anchored()
            .anchor(Anchor::TopLeft)
            .snap_to_window_with_margin(px(8.));
        if let Some(bounds) = captured_bounds {
            anchor = anchor
                .attach(bounds, Anchor::BottomLeft)
                .offset(point(px(0.), px(4.)));
        } else {
            anchor = anchor.offset(point(px(0.), px(4.)));
        }

        Some(deferred(anchor.child(animated_panel)).priority(20).into_any_element())
    }
}

/// Runs a completion query for any entity owning a [`CompletionController`], handling both
/// synchronous filtering and debounced, generation-guarded asynchronous loading while applying
/// live exclusion rules (such as already-added tags in [`super::transfer_tags::TagInputState`]).
pub(crate) fn run_completion_query<V: 'static>(
    this: &mut V,
    query: &str,
    open_if_eligible: bool,
    cx: &mut Context<V>,
    get_ctrl: fn(&mut V) -> Option<&mut CompletionController>,
    exclude: fn(&V, &SuggestionItem) -> bool,
) {
    let (source, min_chars, already_open) = {
        let Some(ctrl) = get_ctrl(this) else {
            return;
        };
        (ctrl.source.clone(), ctrl.config.min_chars, ctrl.open)
    };

    if query.trim().chars().count() < min_chars {
        if let Some(ctrl) = get_ctrl(this) {
            ctrl.close();
        }
        return;
    }

    if !open_if_eligible && !already_open {
        return;
    }

    let generation = {
        let Some(ctrl) = get_ctrl(this) else {
            return;
        };
        ctrl.open = true;
        ctrl.query = SharedString::from(query.to_owned());
        ctrl.generation = ctrl.generation.wrapping_add(1);
        ctrl.pending_task = None;
        ctrl.generation
    };

    match &source.kind {
        SuggestionSourceKind::Static(indexed) => {
            let q = query.trim().to_lowercase();
            let filtered: Vec<SuggestionItem> = if q.is_empty() {
                indexed
                    .iter()
                    .filter(|entry| !exclude(this, &entry.item))
                    .map(|entry| entry.item.clone())
                    .collect()
            } else {
                let mut scored: Vec<(i32, usize)> = indexed
                    .iter()
                    .enumerate()
                    .filter_map(|(idx, entry)| {
                        if exclude(this, &entry.item) {
                            return None;
                        }
                        let score = entry.score(&q)?;
                        Some((score, idx))
                    })
                    .collect();
                scored.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
                scored
                    .into_iter()
                    .map(|(_, idx)| indexed[idx].item.clone())
                    .collect()
            };
            if let Some(ctrl) = get_ctrl(this) {
                ctrl.apply_items(filtered);
            }
        }
        SuggestionSourceKind::Dynamic(loader) => {
            let raw = loader(&*cx);
            let mut filtered = filter_suggestions(query, raw);
            filtered.retain(|item| !exclude(this, item));
            if let Some(ctrl) = get_ctrl(this) {
                ctrl.apply_items(filtered);
            }
        }
        SuggestionSourceKind::Sync(loader) => {
            let raw = loader(query, &*cx);
            let mut filtered = if source.filter_locally {
                filter_suggestions(query, raw)
            } else {
                raw
            };
            filtered.retain(|item| !exclude(this, item));
            if let Some(ctrl) = get_ctrl(this) {
                ctrl.apply_items(filtered);
            }
        }
        SuggestionSourceKind::Async(loader) => {
            let loader = Rc::clone(loader);
            let debounce = source.debounce;
            let filter_locally = source.filter_locally;
            let query_owned = query.to_owned();

            if let Some(ctrl) = get_ctrl(this) {
                ctrl.loading = true;
                ctrl.error = None;
                // Clear highlight while fetching fresh results so Enter won't accept a stale row.
                ctrl.highlighted = None;
            }

            let task = cx.spawn(async move |weak: WeakEntity<V>, cx: &mut AsyncApp| {
                if !debounce.is_zero() {
                    cx.background_executor().timer(debounce).await;
                }

                let fetch_task = match weak.update(cx, |this, cx| {
                    let ctrl = get_ctrl(this)?;
                    if ctrl.generation != generation || !ctrl.open {
                        return None;
                    }
                    Some(loader(&query_owned, cx))
                }) {
                    Ok(Some(task)) => task,
                    _ => return,
                };

                let result = fetch_task.await;
                let _ = weak.update(cx, |this, cx| {
                    let is_current = get_ctrl(this)
                        .is_some_and(|ctrl| ctrl.generation == generation && ctrl.open);
                    if !is_current {
                        return;
                    }
                    match result {
                        Ok(raw) => {
                            let mut filtered = if filter_locally {
                                filter_suggestions(&query_owned, raw)
                            } else {
                                raw
                            };
                            filtered.retain(|item| !exclude(this, item));
                            if let Some(ctrl) = get_ctrl(this) {
                                ctrl.pending_task = None;
                                ctrl.apply_items(filtered);
                            }
                        }
                        Err(err) => {
                            if let Some(ctrl) = get_ctrl(this) {
                                ctrl.pending_task = None;
                                ctrl.apply_error(err);
                            }
                        }
                    }
                    cx.notify();
                });
            });

            if let Some(ctrl) = get_ctrl(this) {
                ctrl.pending_task = Some(task);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;
    use std::time::Duration;

    use super::*;
    use crate::components::button::button;
    use crate::components::combobox::{ComboBoxEvent, ComboBoxOption, ComboBoxState, combo_box};
    use crate::components::input::{Input, InputEvent, InputState, input};
    use crate::components::keyboard_tests::keys;
    use crate::components::layout::v_stack;
    use crate::components::select::{SelectOption, SelectState, select};
    use crate::components::theme::init_components;
    use crate::components::transfer_tags::{TagInputEvent, TagInputState, tag_input};
    use crate::{
        ArrowNavigation, EntityInputHandler, Focusable, Modifiers, Render, TestAppContext,
        VisualTestContext,
    };

    #[test]
    fn fuzzy_ranking_prefers_exact_then_prefix_then_substring_then_subsequence() {
        let items = vec![
            SuggestionItem::new("super-rusty-tools").label("Super Rusty Tools"),
            SuggestionItem::new("r-u-s-t").label("Rapid Unified Syntax Tree"),
            SuggestionItem::new("rust-analyzer").label("rust-analyzer"),
            SuggestionItem::new("rust").label("rust"),
        ];
        let ranked = filter_suggestions("rust", items);
        let values: Vec<&str> = ranked.iter().map(|i| i.value.as_ref()).collect();
        assert_eq!(
            values,
            vec!["rust", "rust-analyzer", "super-rusty-tools", "r-u-s-t"]
        );
    }

    #[test]
    fn fuzzy_matches_description_keywords_and_group() {
        let items = vec![
            SuggestionItem::new("gpui")
                .description("GPU UI framework")
                .keywords(["zed", "metal", "vulkan"])
                .group("Graphics"),
            SuggestionItem::new("taffy").description("Flexbox layout"),
        ];
        assert_eq!(filter_suggestions("vulkan", items.clone()).len(), 1);
        assert_eq!(filter_suggestions("graphics", items.clone()).len(), 1);
        assert_eq!(filter_suggestions("flexbox", items)[0].value, "taffy");
    }

    fn focused<T: Focusable>(cx: &mut VisualTestContext, entity: &Entity<T>) -> bool {
        cx.update(|window, cx| entity.read(cx).focus_handle(cx).is_focused(window))
    }

    #[crate::test]
    fn input_sync_completions_filter_navigate_accept_undo_escape_and_tab(cx: &mut TestAppContext) {
        cx.update(init_components);

        struct Host {
            search: Entity<InputState>,
        }

        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                v_stack()
                    .w(px(400.))
                    .child(input(&self.search))
                    .child(button("after").label("After"))
            }
        }

        let events = Rc::new(RefCell::new(Vec::new()));
        let ev_log = events.clone();
        let (view, cx) = cx.add_window_view(move |_, cx| {
            let search = cx.new(|cx| {
                InputState::new(cx)
                    .placeholder("Search...")
                    .completions(SuggestionSource::items([
                        SuggestionItem::new("rust").group("Languages"),
                        SuggestionItem::new("ruby")
                            .group("Languages")
                            .disabled(true),
                        SuggestionItem::new("rust-analyzer").group("Tools"),
                        SuggestionItem::new("runtime").group("Core"),
                        SuggestionItem::new("python").group("Languages"),
                    ]))
            });
            let log = ev_log.clone();
            cx.subscribe(&search, move |_, _, ev: &InputEvent, _| match ev {
                InputEvent::Submit(v) => log.borrow_mut().push(format!("submit:{v}")),
                InputEvent::AcceptSuggestion(item) => {
                    log.borrow_mut().push(format!("accept:{}", item.value))
                }
                _ => {}
            })
            .detach();
            Host { search }
        });
        cx.run_until_parked();

        let search = view.read_with(cx, |h, _| h.search.clone());
        keys(cx, "tab");
        assert!(focused(cx, &search));
        // Default search preset does not auto-open on focus.
        assert!(!search.read_with(cx, |s, _| s.is_completions_open()));

        // Typing filters suggestions on the fly and opens popover without auto-highlighting.
        cx.simulate_input("ru");
        cx.run_until_parked();
        assert!(search.read_with(cx, |s, _| s.is_completions_open()));
        assert_eq!(search.read_with(cx, |s, _| s.highlighted_completion_index()), None);
        let values: Vec<SharedString> = search.read_with(cx, |s, _| {
            s.completion_items().iter().map(|i| i.value.clone()).collect()
        });
        assert!(values.contains(&SharedString::from("rust")));
        assert!(values.contains(&SharedString::from("rust-analyzer")));
        assert!(values.contains(&SharedString::from("runtime")));
        assert!(!values.contains(&SharedString::from("python")));

        // Left/Right/Home/End still move the text caret while suggestions are open.
        keys(cx, "left");
        assert_eq!(search.read_with(cx, |s, _| s.selected_range()), 1..1);
        keys(cx, "end");
        assert_eq!(search.read_with(cx, |s, _| s.selected_range()), 2..2);

        // Down highlights first enabled item ("rust"); next Down skips disabled "ruby"!
        keys(cx, "down");
        assert_eq!(
            search.read_with(cx, |s, _| s
                .highlighted_completion_index()
                .map(|i| s.completion_items()[i].value.clone())),
            Some(SharedString::from("rust"))
        );
        keys(cx, "down");
        let after_second_down = search.read_with(cx, |s, _| {
            s.highlighted_completion_index()
                .map(|i| s.completion_items()[i].value.clone())
        });
        assert_ne!(after_second_down, Some(SharedString::from("ruby")));

        // Escape closes popover while keeping focus on the input.
        keys(cx, "escape");
        assert!(!search.read_with(cx, |s, _| s.is_completions_open()));
        assert!(focused(cx, &search));

        // Pressing Enter with no highlight submits raw text "ru".
        keys(cx, "enter");
        assert_eq!(events.borrow().last().map(String::as_str), Some("submit:ru"));

        // Pressing Down while closed reopens & highlights first item; Enter accepts it.
        keys(cx, "down enter");
        assert_eq!(search.read_with(cx, |s, _| s.value().to_owned()), "rust");
        assert!(!search.read_with(cx, |s, _| s.is_completions_open()));
        assert_eq!(
            events.borrow().last().map(String::as_str),
            Some("accept:rust")
        );

        // Undo restores the pre-completion query "ru".
        cx.dispatch_action(crate::components::input::input_actions::Undo);
        cx.run_until_parked();
        assert_eq!(search.read_with(cx, |s, _| s.value().to_owned()), "ru");

        // Tab closes an open completion popover and moves focus onward (default browser rule).
        assert!(search.read_with(cx, |s, _| s.is_completions_open()));
        keys(cx, "tab");
        assert!(!search.read_with(cx, |s, _| s.is_completions_open()));
        assert!(!focused(cx, &search));
    }

    #[crate::test]
    fn input_accept_on_tab_option_accepts_highlighted_item(cx: &mut TestAppContext) {
        cx.update(init_components);
        let (search, cx) = cx.add_window_view(|_, cx| {
            InputState::new(cx)
                .completions(SuggestionSource::items(["alpha", "apricot"]))
                .completion_config(
                    CompletionConfig::search()
                        .auto_highlight(true)
                        .accept_on_tab(true),
                )
        });
        cx.run_until_parked();

        search.update_in(cx, |s, window, cx| window.focus(&s.focus_handle(cx), cx));
        cx.run_until_parked();
        cx.simulate_input("ap");
        cx.run_until_parked();
        keys(cx, "tab");
        assert_eq!(search.read_with(cx, |s, _| s.value().to_owned()), "apricot");
        assert!(focused(cx, &search));
    }

    #[crate::test]
    fn input_async_completions_debounce_discard_stale_and_handle_errors(cx: &mut TestAppContext) {
        cx.update(init_components);

        let fetch_count = Rc::new(Cell::new(0usize));
        let fetch_count_ref = fetch_count.clone();
        let (search, cx) = cx.add_window_view(move |_, cx| {
            let count = fetch_count_ref.clone();
            InputState::new(cx).completions(
                SuggestionSource::async_loader(move |query, cx| {
                    count.set(count.get() + 1);
                    let q = query.to_owned();
                    let bg = cx.background_executor().clone();
                    cx.background_spawn(async move {
                        // Make query "slow" take 300ms and "fast" take 50ms to verify stale discard.
                        let delay = if q == "slow" {
                            Duration::from_millis(300)
                        } else if q == "err" {
                            Duration::from_millis(40)
                        } else {
                            Duration::from_millis(50)
                        };
                        bg.timer(delay).await;
                        if q == "err" {
                            return Err(SharedString::from("Network timeout"));
                        }
                        Ok(vec![
                            SuggestionItem::new(format!("{q}-one")),
                            SuggestionItem::new(format!("{q}-two")),
                        ])
                    })
                })
                .debounce(Duration::from_millis(120)),
            )
        });
        cx.run_until_parked();

        search.update_in(cx, |s, window, cx| window.focus(&s.focus_handle(cx), cx));
        cx.run_until_parked();

        // Rapid typing within debounce window collapses into a single fetch.
        cx.simulate_input("s");
        cx.background_executor.advance_clock(Duration::from_millis(40));
        cx.simulate_input("l");
        cx.background_executor.advance_clock(Duration::from_millis(40));
        cx.simulate_input("o");
        cx.background_executor.advance_clock(Duration::from_millis(40));
        cx.simulate_input("w");
        cx.run_until_parked();
        assert_eq!(fetch_count.get(), 0);
        assert!(search.read_with(cx, |s, _| s.is_completions_loading()));

        // Debounce fires -> starts 300ms fetch for "slow".
        cx.background_executor.advance_clock(Duration::from_millis(120));
        cx.run_until_parked();
        assert_eq!(fetch_count.get(), 1);
        assert!(search.read_with(cx, |s, _| s.is_completions_loading()));

        // Before "slow" returns (100ms in), user replaces text with "fast" (50ms fetch).
        cx.background_executor.advance_clock(Duration::from_millis(100));
        cx.run_until_parked();
        search.update_in(cx, |s, window, cx| {
            s.replace_text_in_range(Some(0..4), "fast", window, cx);
        });
        cx.run_until_parked();

        // User presses Down while "fast" is loading -> should highlight first item once loaded!
        keys(cx, "down");

        // Advance past "fast" debounce (120ms) + fetch (50ms) -> "fast" results arrive.
        cx.background_executor.advance_clock(Duration::from_millis(170));
        cx.run_until_parked();
        assert_eq!(fetch_count.get(), 2);
        assert!(!search.read_with(cx, |s, _| s.is_completions_loading()));
        assert_eq!(
            search.read_with(cx, |s, _| s
                .completion_items()
                .iter()
                .map(|i| i.value.clone())
                .collect::<Vec<_>>()),
            vec![SharedString::from("fast-one"), SharedString::from("fast-two")]
        );
        assert_eq!(search.read_with(cx, |s, _| s.highlighted_completion_index()), Some(0));

        // Advance further past when the older "slow" task would have completed -> must NOT overwrite!
        cx.background_executor.advance_clock(Duration::from_millis(300));
        cx.run_until_parked();
        assert_eq!(
            search.read_with(cx, |s, _| s.completion_items()[0].value.clone()),
            SharedString::from("fast-one")
        );

        // Error state populates cleanly.
        search.update_in(cx, |s, window, cx| {
            s.replace_text_in_range(Some(0..4), "err", window, cx);
        });
        cx.background_executor.advance_clock(Duration::from_millis(170));
        cx.run_until_parked();
        assert!(!search.read_with(cx, |s, _| s.is_completions_loading()));
        assert!(search.read_with(cx, |s, _| s.completion_items().is_empty()));
        assert_eq!(
            search.read_with(cx, |s, _| s.completion_error().cloned()),
            Some(SharedString::from("Network timeout"))
        );
        assert!(search.read_with(cx, |s, _| s.is_completions_open()));

        // Blurring closes completions and cancels any in-flight request.
        search.update_in(cx, |s, window, cx| {
            s.replace_text_in_range(Some(0..3), "cancelme", window, cx);
        });
        cx.run_until_parked();
        assert!(search.read_with(cx, |s, _| s.is_completions_loading()));
        cx.update(|window, cx| window.blur(cx));
        cx.run_until_parked();
        assert!(!search.read_with(cx, |s, _| s.is_completions_open()));
        let count_before_blur_advance = fetch_count.get();
        cx.background_executor.advance_clock(Duration::from_millis(500));
        cx.run_until_parked();
        assert_eq!(fetch_count.get(), count_before_blur_advance);
    }

    #[crate::test]
    fn tag_input_sync_and_async_suggestions_exclude_added_tags_and_respect_cap(
        cx: &mut TestAppContext,
    ) {
        cx.update(init_components);

        let changes = Rc::new(RefCell::new(Vec::<Vec<SharedString>>::new()));
        let changes_ref = changes.clone();
        let (tags, cx) = cx.add_window_view(move |_, cx| {
            let state = TagInputState::new(cx)
                .default_tags(["rust"])
                .max_tags(3)
                .suggestions(SuggestionSource::items([
                    SuggestionItem::new("rust"),
                    SuggestionItem::new("vulkan"),
                    SuggestionItem::new("wayland"),
                    SuggestionItem::new("wgpu"),
                ]));
            let log = changes_ref.clone();
            let ent = cx.entity();
            cx.subscribe(&ent, move |_, _, ev: &TagInputEvent, _| {
                let TagInputEvent::Change(list) = ev;
                log.borrow_mut().push(list.clone());
            })
            .detach();
            state
        });
        cx.run_until_parked();

        // Focusing TagInput's text input opens suggestions automatically and excludes already-added "rust".
        // (First Tab stop is the "rust" badge's remove button; focusing TagInput directly targets its inner Input.)
        tags.update_in(cx, |t, window, cx| window.focus(&t.focus_handle(cx), cx));
        cx.run_until_parked();
        assert!(tags.read_with(cx, |t, _| t.is_suggestions_open()));
        let initial_suggestions: Vec<SharedString> = tags.read_with(cx, |t, _| {
            t.suggestion_items().iter().map(|i| i.value.clone()).collect()
        });
        assert_eq!(
            initial_suggestions,
            vec![
                SharedString::from("vulkan"),
                SharedString::from("wayland"),
                SharedString::from("wgpu")
            ]
        );
        // Default tags config auto-highlights the first available tag.
        assert_eq!(tags.read_with(cx, |t, _| t.highlighted_suggestion_index()), Some(0));

        // Typing filters suggestions on the fly.
        cx.simulate_input("w");
        cx.run_until_parked();
        let filtered: Vec<SharedString> = tags.read_with(cx, |t, _| {
            t.suggestion_items().iter().map(|i| i.value.clone()).collect()
        });
        assert_eq!(
            filtered,
            vec![SharedString::from("wayland"), SharedString::from("wgpu")]
        );

        // Down selects "wgpu", Enter adds it, clears inner input, keeps suggestions open minus "wgpu".
        keys(cx, "down enter");
        assert_eq!(
            tags.read_with(cx, |t, _| t.tags().to_vec()),
            vec![SharedString::from("rust"), SharedString::from("wgpu")]
        );
        assert!(tags.read_with(cx, |t, _| t.is_suggestions_open()));
        let remaining: Vec<SharedString> = tags.read_with(cx, |t, _| {
            t.suggestion_items().iter().map(|i| i.value.clone()).collect()
        });
        assert_eq!(
            remaining,
            vec![SharedString::from("vulkan"), SharedString::from("wayland")]
        );

        // Accepting one more hits max_tags(3) -> closes suggestions cleanly.
        keys(cx, "enter");
        assert_eq!(
            tags.read_with(cx, |t, _| t.tags().to_vec()),
            vec![
                SharedString::from("rust"),
                SharedString::from("wgpu"),
                SharedString::from("vulkan")
            ]
        );
        assert!(!tags.read_with(cx, |t, _| t.is_suggestions_open()));

        // Backspace removes last tag ("vulkan") and restores focus to inner input, reopening suggestions with "vulkan" back!
        keys(cx, "backspace");
        assert_eq!(
            tags.read_with(cx, |t, _| t.tags().to_vec()),
            vec![SharedString::from("rust"), SharedString::from("wgpu")]
        );
        assert!(tags.read_with(cx, |t, _| t
            .suggestion_items()
            .iter()
            .any(|i| i.value == "vulkan")));

        // Verify allow_custom(false) blocks arbitrary custom tags while still allowing suggestion selection.
        let (strict_tags, cx2_tags) = (tags.clone(), cx);
        strict_tags.update(cx2_tags, |t, cx| {
            t.set_allow_custom(false, cx);
        });
        cx2_tags.simulate_input("not-in-list");
        cx2_tags.run_until_parked();
        keys(cx2_tags, "enter");
        assert_eq!(
            strict_tags.read_with(cx2_tags, |t, _| t.tags().len()),
            2,
            "allow_custom(false) must reject unmatched free-form text"
        );
    }

    #[crate::test]
    fn completions_coexist_with_enclosing_arrow_navigation_container(cx: &mut TestAppContext) {
        cx.update(init_components);

        struct NavHost {
            with_comp: Entity<InputState>,
            plain: Entity<InputState>,
            tags: Entity<TagInputState>,
            after_clicked: Rc<Cell<bool>>,
        }

        impl Render for NavHost {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                let clicked = self.after_clicked.clone();
                div()
                    .flex()
                    .flex_col()
                    .gap(px(8.))
                    .arrow_navigation(ArrowNavigation::Vertical)
                    .child(Input::new(&self.with_comp))
                    .child(Input::new(&self.plain))
                    .child(tag_input(&self.tags))
                    .child(button("after").label("After").on_click(move |_, _, _| {
                        clicked.set(true);
                    }))
            }
        }

        let after_clicked = Rc::new(Cell::new(false));
        let ac = after_clicked.clone();
        let (view, cx) = cx.add_window_view(move |_, cx| NavHost {
            with_comp: cx.new(|cx| {
                InputState::new(cx)
                    .completions(SuggestionSource::items(["alpha", "beta", "gamma"]))
            }),
            plain: cx.new(InputState::new),
            tags: cx.new(|cx| {
                TagInputState::new(cx).suggestions(SuggestionSource::items(["one", "two"]))
            }),
            after_clicked: ac,
        });
        cx.run_until_parked();

        let (with_comp, plain, tags) = view.read_with(cx, |h, _| {
            (h.with_comp.clone(), h.plain.clone(), h.tags.clone())
        });

        // Focus input WITH completions: Down opens & navigates completions instead of jumping to `plain`!
        keys(cx, "tab down down");
        assert!(focused(cx, &with_comp));
        assert!(with_comp.read_with(cx, |s, _| s.is_completions_open()));
        assert_eq!(
            with_comp.read_with(cx, |s, _| s.highlighted_completion_index()),
            Some(1)
        );
        assert!(!focused(cx, &plain));

        // Tab moves from `with_comp` to `plain`.
        keys(cx, "tab");
        assert!(focused(cx, &plain));

        // On `plain` (no completions): Down bubbles to `.arrow_navigation(Vertical)` and moves focus to `tags`!
        keys(cx, "down");
        assert!(focused(cx, &tags));

        // Inside `tags` (with suggestions): Down steps suggestions instead of jumping to `after` button!
        keys(cx, "down");
        assert!(focused(cx, &tags));
        assert_eq!(
            tags.read_with(cx, |t, _| t.highlighted_suggestion_index()),
            Some(1)
        );
        assert!(!after_clicked.get());
    }

    #[crate::test]
    fn tag_input_async_suggestions_exclude_tags_added_mid_flight(cx: &mut TestAppContext) {
        cx.update(init_components);
        let (tags, cx) = cx.add_window_view(|_, cx| {
            TagInputState::new(cx).suggestions(
                SuggestionSource::async_fn(|query, cx| {
                    let q = query.to_owned();
                    let bg = cx.background_executor().clone();
                    cx.background_spawn(async move {
                        bg.timer(Duration::from_millis(80)).await;
                        let all = vec![
                            SuggestionItem::new("rust"),
                            SuggestionItem::new("runtime"),
                            SuggestionItem::new("ruby"),
                        ];
                        filter_suggestions(&q, all)
                    })
                })
                .debounce(Duration::from_millis(50)),
            )
        });
        cx.run_until_parked();

        keys(cx, "tab");
        cx.simulate_input("ru");
        cx.run_until_parked();
        assert!(tags.read_with(cx, |t, _| t.is_suggestions_loading()));

        // While the async fetch is still in flight, programmatically add "rust".
        tags.update(cx, |t, cx| {
            t.add_tag_str("rust", cx);
        });
        cx.background_executor.advance_clock(Duration::from_millis(140));
        cx.run_until_parked();

        assert!(!tags.read_with(cx, |t, _| t.is_suggestions_loading()));
        let values: Vec<SharedString> = tags.read_with(cx, |t, _| {
            t.suggestion_items().iter().map(|i| i.value.clone()).collect()
        });
        assert!(!values.contains(&SharedString::from("rust")));
        assert!(values.contains(&SharedString::from("runtime")));
        assert!(values.contains(&SharedString::from("ruby")));
    }

    #[crate::test]
    fn clicking_completion_suggestion_accepts_and_keeps_focus(cx: &mut TestAppContext) {
        cx.update(init_components);

        struct ClickHost {
            search: Entity<InputState>,
        }

        impl Render for ClickHost {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                div()
                    .size_full()
                    .p(px(16.))
                    .child(
                        div()
                            .w(px(300.))
                            .debug_selector(|| "completion-input".into())
                            .child(input(&self.search)),
                    )
            }
        }

        let (view, cx) = cx.add_window_view(|_, cx| ClickHost {
            search: cx.new(|cx| {
                InputState::new(cx)
                    .completions(SuggestionSource::items(["alpha", "apricot"]))
                    .completion_config(CompletionConfig::search().open_on_focus(true))
            }),
        });
        cx.run_until_parked();

        let search = view.read_with(cx, |h, _| h.search.clone());
        keys(cx, "tab");
        assert!(focused(cx, &search));
        assert!(search.read_with(cx, |s, _| s.is_completions_open()));

        let bounds = cx.debug_bounds("completion-input").expect("input bounds");
        // Popover opens 4px below input with 4px top padding; first compact row is 34px tall.
        let first_row = point(bounds.left() + px(24.), bounds.bottom() + px(20.));
        cx.simulate_click(first_row, Modifiers::none());
        cx.run_until_parked();

        assert_eq!(search.read_with(cx, |s, _| s.value().to_owned()), "alpha");
        assert!(!search.read_with(cx, |s, _| s.is_completions_open()));
        assert!(focused(cx, &search));
    }

    #[crate::test]
    fn select_and_combobox_support_pagedown_and_pageup(cx: &mut TestAppContext) {
        cx.update(init_components);

        struct ListHost {
            select: Entity<SelectState<usize>>,
            combo: Entity<ComboBoxState<usize>>,
        }

        impl Render for ListHost {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                v_stack()
                    .w(px(300.))
                    .child(select(&self.select))
                    .child(combo_box(&self.combo))
            }
        }

        let (view, cx) = cx.add_window_view(|_, cx| {
            let s_opts = (0..12)
                .map(|i| SelectOption::new(i, format!("Item {i}")))
                .collect();
            let c_opts = (0..12)
                .map(|i| ComboBoxOption::new(i, format!("Option {i}")))
                .collect();
            ListHost {
                select: cx.new(|cx| SelectState::new(s_opts, cx)),
                combo: cx.new(|cx| ComboBoxState::new(c_opts, cx)),
            }
        });
        cx.run_until_parked();

        let (sel, cmb) = view.read_with(cx, |h, _| (h.select.clone(), h.combo.clone()));
        // Select: Tab -> Down opens at 0 -> PageDown jumps to 6 -> Enter selects 6.
        keys(cx, "tab down pagedown enter");
        assert_eq!(sel.read_with(cx, |s, _| s.selected_value().copied()), Some(6));

        // ComboBox: Tab -> Down opens at 0 -> PageDown jumps to 6 -> PageUp back to 0 -> PageDown -> Enter selects 6.
        keys(cx, "tab down pagedown pageup pagedown enter");
        assert_eq!(cmb.read_with(cx, |c, _| c.selected_value().copied()), Some(6));
    }

    #[crate::test]
    fn combobox_types_inline_in_original_box_for_single_and_multi_select(cx: &mut TestAppContext) {
        cx.update(init_components);

        let created = Rc::new(RefCell::new(Vec::<SharedString>::new()));
        let created_ref = created.clone();

        struct ComboHost {
            single: Entity<ComboBoxState<&'static str>>,
            multi: Entity<ComboBoxState<String>>,
        }

        impl Render for ComboHost {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                v_stack()
                    .w(px(360.))
                    .child(combo_box(&self.single))
                    .child(combo_box(&self.multi))
            }
        }

        let (view, cx) = cx.add_window_view(move |_, cx| {
            let single = cx.new(|cx| {
                ComboBoxState::new(
                    vec![
                        ComboBoxOption::new("gpui", "GPUI").keywords(["zed", "ui"]),
                        ComboBoxOption::new("wgpu", "wgpu").keywords(["vulkan", "metal"]),
                        ComboBoxOption::new("taffy", "Taffy").keywords(["flexbox", "css"]),
                    ],
                    cx,
                )
                .default_values(vec!["gpui"])
            });
            let multi = cx.new(|cx| {
                ComboBoxState::new(
                    vec![
                        ComboBoxOption::new("rust".to_string(), "Rust"),
                        ComboBoxOption::new("taffy".to_string(), "Taffy"),
                        ComboBoxOption::new("dark".to_string(), "Dark Mode"),
                    ],
                    cx,
                )
                .multiple(true)
                .allow_custom(true)
                .default_values(vec!["rust".to_string()])
            });
            let log = created_ref.clone();
            cx.subscribe(&multi, move |_, _, ev: &ComboBoxEvent<String>, _| {
                if let ComboBoxEvent::Create(label) = ev {
                    log.borrow_mut().push(label.clone());
                }
            })
            .detach();
            ComboHost { single, multi }
        });
        cx.run_until_parked();

        let (single, multi) = view.read_with(cx, |h, _| (h.single.clone(), h.multi.clone()));

        // Tabbing onto single-select focuses its inline input directly; typing opens & filters live!
        keys(cx, "tab");
        assert!(focused(cx, &single));
        cx.simulate_input("vulkan");
        cx.run_until_parked();
        assert!(single.read_with(cx, |c, _| c.is_open()));
        assert_eq!(single.read_with(cx, |c, cx| c.query(cx).to_owned()), "vulkan");
        // Escape closes, clears the inline filter, keeps previous selection & focus.
        keys(cx, "escape");
        assert!(!single.read_with(cx, |c, _| c.is_open()));
        assert_eq!(single.read_with(cx, |c, _| c.selected_value().copied()), Some("gpui"));
        assert_eq!(single.read_with(cx, |c, cx| c.query(cx).to_owned()), "");
        assert!(focused(cx, &single));

        // Typing "css" + Enter selects "taffy" directly from inline typing.
        cx.simulate_input("css");
        cx.run_until_parked();
        keys(cx, "enter");
        assert_eq!(single.read_with(cx, |c, _| c.selected_value().copied()), Some("taffy"));
        assert!(!single.read_with(cx, |c, _| c.is_open()));

        // Tab moves cleanly to multi-select inline input.
        keys(cx, "tab");
        assert!(focused(cx, &multi));
        cx.simulate_input("da");
        cx.run_until_parked();
        assert!(multi.read_with(cx, |c, _| c.is_open()));
        // Enter selects "Dark Mode", clears inline query, keeps popover open.
        keys(cx, "enter");
        assert_eq!(
            multi.read_with(cx, |c, _| c.selected_values().to_vec()),
            vec!["rust".to_string(), "dark".to_string()]
        );
        assert_eq!(multi.read_with(cx, |c, cx| c.query(cx).to_owned()), "");
        assert!(multi.read_with(cx, |c, _| c.is_open()));

        // Backspace on empty inline query pops the last chip ("dark").
        keys(cx, "backspace");
        assert_eq!(
            multi.read_with(cx, |c, _| c.selected_values().to_vec()),
            vec!["rust".to_string()]
        );

        // Typing unmatched text + Enter emits ComboBoxEvent::Create.
        cx.simulate_input("wayland");
        cx.run_until_parked();
        keys(cx, "enter");
        assert_eq!(*created.borrow(), vec![SharedString::from("wayland")]);
        assert!(!multi.read_with(cx, |c, _| c.is_open()));
    }
}

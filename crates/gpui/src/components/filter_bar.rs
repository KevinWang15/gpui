//! Structured faceted [`FilterBar`] with segmented pill chips, single- & multi-value
//! facet popovers with live counts, and full keyboard grammar.

use std::cell::RefCell;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, ParentElement, RenderOnce,
    ScrollStrategy, SharedString, StyleRefinement, Styled, UniformListScrollHandle, Window,
    anchored, deferred, div, ease_out_quint, prelude::*, px, uniform_list,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::cursor_blink::{CursorBlink, is_mounted_and_focused};
use super::theme::{ANIM_FAST, ActiveTheme, Theme, tabular_numerals};
use super::tooltip::Tooltip;

/// Maximum logical width for a single-value chip segment before ellipsizing.
const MAX_SINGLE_VALUE_WIDTH: f32 = 200.0;
/// Maximum logical width for the primary value inside a multi-value chip (`value +N`).
const MAX_MULTI_VALUE_WIDTH: f32 = 148.0;
/// Maximum logical width for a field label inside a chip before ellipsizing.
const MAX_FIELD_LABEL_WIDTH: f32 = 140.0;
/// Combined character length threshold below which a 2-value clause renders inline (`a, b`).
const INLINE_PAIR_MAX_CHARS: usize = 24;
/// Character threshold above which a single value attaches a full-value hover tooltip.
const VALUE_TOOLTIP_MIN_CHARS: usize = 22;
/// Character threshold above which a popover facet option attaches a full-label hover tooltip.
const FACET_TOOLTIP_MIN_CHARS: usize = 36;
/// Maximum values listed individually inside a chip hover tooltip before summarizing `+N more`.
const TOOLTIP_MAX_LISTED_VALUES: usize = 10;

/// Allocation-free ASCII case-insensitive substring check with Unicode fallback.
#[inline]
fn contains_case_insensitive(haystack: &str, needle: &str) -> bool {
    if needle.is_empty() {
        return true;
    }
    if haystack.is_ascii() && needle.is_ascii() {
        let h = haystack.as_bytes();
        let n = needle.as_bytes();
        if n.len() > h.len() {
            return false;
        }
        let first_lo = n[0].to_ascii_lowercase();
        let first_hi = n[0].to_ascii_uppercase();
        let last_start = h.len() - n.len();
        let mut i = 0;
        while i <= last_start {
            let b = h[i];
            if (b == first_lo || b == first_hi) && h[i..i + n.len()].eq_ignore_ascii_case(n) {
                return true;
            }
            i += 1;
        }
        false
    } else {
        haystack.to_lowercase().contains(&needle.to_lowercase())
    }
}

/// Comparison operator inside a [`FilterClause`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FilterOperator {
    /// Exact single-value match (`=`).
    Eq,
    /// Single-value exclusion (`!=`).
    Neq,
    /// Multi-value set membership (`is one of`).
    OneOf,
    /// Multi-value set exclusion (`is not one of`).
    NotOneOf,
    /// Substring match (`contains`).
    Contains,
}

impl FilterOperator {
    /// Compact label displayed inside filter chips and operator menus.
    pub fn label(self) -> &'static str {
        match self {
            Self::Eq => "=",
            Self::Neq => "!=",
            Self::OneOf => "is one of",
            Self::NotOneOf => "is not one of",
            Self::Contains => "contains",
        }
    }

    /// Returns `true` when this operator accepts multiple checkbox-selected values.
    pub fn is_multi(self) -> bool {
        matches!(self, Self::OneOf | Self::NotOneOf)
    }
}

/// Schema definition for one filterable field (e.g. `Provider`, `Model`, `Key`).
#[derive(Clone, Debug)]
pub struct FilterFieldDef {
    /// Stable programmatic field key (e.g. `"provider"`).
    pub key: SharedString,
    /// Human-readable label shown in chips & suggestions (e.g. `"Provider"`).
    pub label: SharedString,
    /// Permitted operators for this field.
    pub operators: Vec<FilterOperator>,
    /// Optional short hint shown in the field picker.
    pub hint: Option<SharedString>,
}

impl FilterFieldDef {
    /// Creates a new field supporting `=`, `!=`, and `is one of` by default.
    pub fn new(key: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self {
            key: key.into(),
            label: label.into(),
            operators: vec![
                FilterOperator::Eq,
                FilterOperator::OneOf,
                FilterOperator::Neq,
            ],
            hint: None,
        }
    }

    /// Overrides the allowed [`FilterOperator`]s for this field.
    pub fn operators(mut self, operators: impl IntoIterator<Item = FilterOperator>) -> Self {
        self.operators = operators.into_iter().collect();
        self
    }

    /// Adds a subtle right-aligned hint shown in the field picker dropdown.
    pub fn hint(mut self, hint: impl Into<SharedString>) -> Self {
        self.hint = Some(hint.into());
        self
    }
}

/// One suggested facet value with an optional right-aligned hit count.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterFacetOption {
    /// Underlying filter value (e.g. `"anthropic"`).
    pub value: SharedString,
    /// Display label (defaults to `value`).
    pub label: SharedString,
    /// Optional facet match count (e.g. `Some(309)`).
    pub count: Option<usize>,
}

impl FilterFacetOption {
    /// Creates a facet option with optional count.
    pub fn new(value: impl Into<SharedString>, count: Option<usize>) -> Self {
        let value = value.into();
        Self {
            label: value.clone(),
            value,
            count,
        }
    }

    /// Overrides the display label independently of `value`.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }
}

/// One committed structured filter predicate (`Provider = anthropic` or `Model is one of ...`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FilterClause {
    /// Field key (matches [`FilterFieldDef::key`]).
    pub field: SharedString,
    /// Field display label.
    pub field_label: SharedString,
    /// Comparison operator.
    pub operator: FilterOperator,
    /// Selected value(s). Single-value operators have 1 entry; multi-value operators have 1+.
    pub values: SmallVec<[SharedString; 4]>,
}

impl FilterClause {
    /// Formats the selected value(s) compactly for plain-text display.
    ///
    /// Two short values join inline (`"OpenAI, Anthropic"`); longer pairs or 3+ values
    /// show the first value followed by `"+N"` so wide tokens (e.g. UUIDs) stay compact.
    pub fn values_summary(&self) -> String {
        match self.values.len() {
            0 => String::new(),
            1 => self.values[0].to_string(),
            2 if self.values[0].chars().count() + self.values[1].chars().count()
                <= INLINE_PAIR_MAX_CHARS =>
            {
                format!("{}, {}", self.values[0], self.values[1])
            }
            n => format!("{}, +{}", self.values[0], n - 1),
        }
    }

    /// Returns `(primary_text, extra_count)` for structured single-line chip rendering,
    /// where `extra_count` is rendered as a dedicated `+N` pill badge when present.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn chip_value_parts(&self) -> (SharedString, Option<usize>) {
        display_value_parts(&self.values)
    }

    /// Returns `true` when the chip value is multi-value or long enough to truncate,
    /// warranting a full-value hover tooltip.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) fn needs_value_tooltip(&self) -> bool {
        self.values.len() > 1
            || self
                .values
                .first()
                .is_some_and(|v| v.chars().count() > VALUE_TOOLTIP_MIN_CHARS)
    }

    /// Evaluates whether a candidate field value satisfies this clause.
    pub fn matches_value(&self, candidate: &str) -> bool {
        match self.operator {
            FilterOperator::Eq => self.values.iter().any(|v| v.as_ref() == candidate),
            FilterOperator::Neq => self.values.iter().all(|v| v.as_ref() != candidate),
            FilterOperator::OneOf => self.values.iter().any(|v| v.as_ref() == candidate),
            FilterOperator::NotOneOf => self.values.iter().all(|v| v.as_ref() != candidate),
            FilterOperator::Contains => self
                .values
                .iter()
                .any(|v| contains_case_insensitive(candidate, v.as_ref())),
        }
    }
}

/// Events emitted by [`FilterBarState`].
#[derive(Clone, Debug)]
pub enum FilterBarEvent {
    /// Emitted whenever the list of committed [`FilterClause`]s changes.
    Change(Vec<FilterClause>),
}

/// Reactive callback computing dynamic facet options & counts given active sibling clauses.
pub type FacetProvider =
    Rc<dyn Fn(&[FilterClause], &str, &str) -> Vec<FilterFacetOption> + 'static>;

/// Optional formatter turning `(field_key, raw_value)` into a human-readable display label
/// for chips, draft previews, and hover tooltips while keeping [`FilterClause::values`] on raw IDs.
pub type ValueLabelFormatter = Rc<dyn Fn(&str, &str) -> SharedString + 'static>;

fn format_value_label(
    formatter: Option<&ValueLabelFormatter>,
    field: &str,
    raw: &SharedString,
) -> SharedString {
    match formatter {
        Some(fmt) => fmt(field, raw.as_ref()),
        None => raw.clone(),
    }
}

fn display_value_parts(labels: &[SharedString]) -> (SharedString, Option<usize>) {
    match labels.len() {
        0 => (SharedString::default(), None),
        1 => (labels[0].clone(), None),
        2 if labels[0].chars().count() + labels[1].chars().count() <= INLINE_PAIR_MAX_CHARS => (
            SharedString::from(format!("{}, {}", labels[0], labels[1])),
            None,
        ),
        n => (labels[0].clone(), Some(n - 1)),
    }
}

#[derive(Clone, Debug)]
enum FilterDraft {
    /// Choosing a field (`Provider`, `Model`, ...).
    Field,
    /// Choosing an operator for `field`.
    Operator {
        field: SharedString,
        field_label: SharedString,
    },
    /// Choosing a single value from facet suggestions.
    SingleValue {
        field: SharedString,
        field_label: SharedString,
        operator: FilterOperator,
    },
    /// Toggling one or more values via checkboxes, then pressing Enter.
    MultiValue {
        field: SharedString,
        field_label: SharedString,
        operator: FilterOperator,
        pending: SmallVec<[SharedString; 4]>,
    },
}

struct CachedFacets {
    field: SharedString,
    query: String,
    options: Rc<[FilterFacetOption]>,
}

/// Interactive state entity backing a [`FilterBar`].
pub struct FilterBarState {
    fields: Vec<FilterFieldDef>,
    clauses: Vec<FilterClause>,
    /// Original `(index, clause)` held aside while editing an existing committed chip so
    /// cancelling (Escape or clicking outside) restores it cleanly without losing the filter.
    editing_original: Option<(usize, FilterClause)>,
    draft: FilterDraft,
    query: String,
    open: bool,
    highlighted: usize,
    placeholder: SharedString,
    facet_provider: FacetProvider,
    value_label: Option<ValueLabelFormatter>,
    cached_facets: RefCell<Option<CachedFacets>>,
    scroll_handle: UniformListScrollHandle,
    focus_handle: FocusHandle,
    blink: CursorBlink,
}

impl EventEmitter<FilterBarEvent> for FilterBarState {}

impl Focusable for FilterBarState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FilterBarState {
    /// Creates a new [`FilterBarState`] with the given schema `fields` and dynamic `facet_provider`.
    pub fn new(
        fields: Vec<FilterFieldDef>,
        facet_provider: impl Fn(&[FilterClause], &str, &str) -> Vec<FilterFacetOption> + 'static,
        cx: &mut Context<Self>,
    ) -> Self {
        Self {
            fields,
            clauses: Vec::new(),
            editing_original: None,
            draft: FilterDraft::Field,
            query: String::new(),
            open: false,
            highlighted: 0,
            placeholder: "Filter by field, operator, value...".into(),
            facet_provider: Rc::new(facet_provider),
            value_label: None,
            cached_facets: RefCell::new(None),
            scroll_handle: UniformListScrollHandle::new(),
            focus_handle: cx.focus_handle().tab_stop(true),
            blink: CursorBlink::new(),
        }
    }

    #[cfg(test)]
    pub(crate) fn blink(&self) -> &CursorBlink {
        &self.blink
    }

    /// Sets the placeholder text shown when no filters are active.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets a display-name formatter mapping `(field_key, raw_value)` to a readable label for
    /// chips, draft previews, and tooltips while keeping [`FilterClause::values`] on raw IDs.
    pub fn value_label(
        mut self,
        formatter: impl Fn(&str, &str) -> SharedString + 'static,
    ) -> Self {
        self.value_label = Some(Rc::new(formatter));
        self
    }

    /// Invalidates cached facet suggestions and re-renders the bar (e.g. after async facet
    /// values or display names arrive).
    pub fn invalidate_facets(&mut self, cx: &mut Context<Self>) {
        self.invalidate_facet_cache();
        cx.notify();
    }

    /// Seeds initial [`FilterClause`]s.
    pub fn initial_clauses(mut self, clauses: Vec<FilterClause>) -> Self {
        self.clauses = clauses;
        self
    }

    /// Returns the currently committed [`FilterClause`]s.
    pub fn clauses(&self) -> &[FilterClause] {
        &self.clauses
    }

    fn invalidate_facet_cache(&self) {
        *self.cached_facets.borrow_mut() = None;
    }

    /// Returns `true` if any clause matches `(field, value)` with `=` or `is one of`.
    pub fn is_narrowed_to(&self, field: &str, value: &str) -> bool {
        self.clauses.iter().any(|c| {
            c.field.as_ref() == field
                && matches!(c.operator, FilterOperator::Eq | FilterOperator::OneOf)
                && c.values.iter().any(|v| v.as_ref() == value)
        })
    }

    /// Programmatically toggles or upserts a single-value equality filter (e.g. from clicking a
    /// bar in [`BreakdownList`](super::charts::BreakdownList) to narrow the view).
    pub fn toggle_narrow(
        &mut self,
        field_key: impl Into<SharedString>,
        value: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let field_key = field_key.into();
        let value = value.into();
        if let Some(pos) = self.clauses.iter().position(|c| {
            c.field == field_key
                && c.operator == FilterOperator::Eq
                && c.values.len() == 1
                && c.values[0] == value
        }) {
            self.clauses.remove(pos);
        } else {
            let field_label = self
                .fields
                .iter()
                .find(|f| f.key == field_key)
                .map(|f| f.label.clone())
                .unwrap_or_else(|| field_key.clone());
            self.clauses.retain(|c| c.field != field_key);
            self.clauses.push(FilterClause {
                field: field_key,
                field_label,
                operator: FilterOperator::Eq,
                values: smallvec::smallvec![value],
            });
        }
        self.invalidate_facet_cache();
        cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        cx.notify();
    }

    /// Replaces or inserts a clause for `field_key`.
    pub fn upsert_clause(
        &mut self,
        field_key: impl Into<SharedString>,
        operator: FilterOperator,
        values: impl IntoIterator<Item = SharedString>,
        cx: &mut Context<Self>,
    ) {
        let field_key = field_key.into();
        let values: SmallVec<[SharedString; 4]> = values.into_iter().collect();
        if values.is_empty() {
            return;
        }
        let field_label = self
            .fields
            .iter()
            .find(|f| f.key == field_key)
            .map(|f| f.label.clone())
            .unwrap_or_else(|| field_key.clone());
        self.clauses.retain(|c| c.field != field_key);
        self.clauses.push(FilterClause {
            field: field_key,
            field_label,
            operator,
            values,
        });
        self.invalidate_facet_cache();
        cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        cx.notify();
    }

    /// Removes the clause at `index`.
    pub fn remove_clause(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.clauses.len() {
            self.clauses.remove(index);
            if let Some((orig_idx, _)) = &mut self.editing_original
                && index < *orig_idx
            {
                *orig_idx = orig_idx.saturating_sub(1);
            }
            self.invalidate_facet_cache();
            cx.emit(FilterBarEvent::Change(self.clauses.clone()));
            cx.notify();
        }
    }

    /// Clears all committed clauses and any in-progress draft.
    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        let changed = !self.clauses.is_empty() || self.editing_original.is_some();
        self.clauses.clear();
        self.editing_original = None;
        self.draft = FilterDraft::Field;
        self.query.clear();
        self.highlighted = 0;
        self.open = false;
        self.invalidate_facet_cache();
        self.blink.reset(cx);
        if changed {
            cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        }
        cx.notify();
    }

    /// Restores any in-progress edit back into `self.clauses` without emitting `Change`.
    fn restore_editing_original(&mut self) -> bool {
        if let Some((orig_idx, orig_clause)) = self.editing_original.take() {
            let insert_at = orig_idx.min(self.clauses.len());
            self.clauses.insert(insert_at, orig_clause);
            true
        } else {
            false
        }
    }

    fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.highlighted = 0;
        self.invalidate_facet_cache();
        self.blink.reset(cx);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        let restored = self.restore_editing_original();
        if restored {
            self.draft = FilterDraft::Field;
            self.query.clear();
            self.highlighted = 0;
        }
        if self.open || restored {
            self.open = false;
            self.invalidate_facet_cache();
            cx.notify();
        }
    }

    fn edit_clause_operator(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_editing_original();
        if index >= self.clauses.len() {
            return;
        }
        let clause = self.clauses.remove(index);
        self.draft = FilterDraft::Operator {
            field: clause.field.clone(),
            field_label: clause.field_label.clone(),
        };
        self.editing_original = Some((index, clause));
        self.query.clear();
        self.highlighted = 0;
        self.open = true;
        self.invalidate_facet_cache();
        self.blink.reset(cx);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn edit_clause_value(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        self.restore_editing_original();
        if index >= self.clauses.len() {
            return;
        }
        let clause = self.clauses.remove(index);
        self.draft = if clause.operator.is_multi() {
            FilterDraft::MultiValue {
                field: clause.field.clone(),
                field_label: clause.field_label.clone(),
                operator: clause.operator,
                pending: clause.values.clone(),
            }
        } else {
            FilterDraft::SingleValue {
                field: clause.field.clone(),
                field_label: clause.field_label.clone(),
                operator: clause.operator,
            }
        };
        self.editing_original = Some((index, clause));
        self.query.clear();
        self.highlighted = 0;
        self.open = true;
        self.invalidate_facet_cache();
        self.blink.reset(cx);
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn matching_fields(&self) -> Vec<FilterFieldDef> {
        let q = self.query.trim();
        self.fields
            .iter()
            .filter(|f| {
                q.is_empty()
                    || contains_case_insensitive(f.label.as_ref(), q)
                    || contains_case_insensitive(f.key.as_ref(), q)
            })
            .cloned()
            .collect()
    }

    fn matching_operators(&self, field_key: &str) -> Vec<FilterOperator> {
        let ops = self
            .fields
            .iter()
            .find(|f| f.key.as_ref() == field_key)
            .map(|f| f.operators.as_slice())
            .unwrap_or(&[FilterOperator::Eq, FilterOperator::OneOf]);
        let q = self.query.trim();
        ops.iter()
            .copied()
            .filter(|op| q.is_empty() || contains_case_insensitive(op.label(), q))
            .collect()
    }

    /// Returns cached facet suggestions for `(field_key, query)`, avoiding repeated
    /// `facet_provider` calls across arrow-key navigation, checkbox toggles, and renders.
    fn matching_facets(&self, field_key: &str) -> Rc<[FilterFacetOption]> {
        let q = self.query.trim();
        if let Some(cached) = self.cached_facets.borrow().as_ref()
            && cached.field.as_ref() == field_key
            && cached.query == q
        {
            return cached.options.clone();
        }
        let options: Rc<[FilterFacetOption]> =
            Rc::from((self.facet_provider)(&self.clauses, field_key, q));
        *self.cached_facets.borrow_mut() = Some(CachedFacets {
            field: SharedString::from(field_key.to_owned()),
            query: q.to_owned(),
            options: options.clone(),
        });
        options
    }

    fn choose_field(&mut self, field: &FilterFieldDef, cx: &mut Context<Self>) {
        self.query.clear();
        self.highlighted = 0;
        self.invalidate_facet_cache();
        if field.operators.len() == 1 {
            let op = field.operators[0];
            self.draft = if op.is_multi() {
                FilterDraft::MultiValue {
                    field: field.key.clone(),
                    field_label: field.label.clone(),
                    operator: op,
                    pending: SmallVec::new(),
                }
            } else {
                FilterDraft::SingleValue {
                    field: field.key.clone(),
                    field_label: field.label.clone(),
                    operator: op,
                }
            };
        } else {
            self.draft = FilterDraft::Operator {
                field: field.key.clone(),
                field_label: field.label.clone(),
            };
        }
        self.open = true;
        self.blink.reset(cx);
        cx.notify();
    }

    fn choose_operator(&mut self, op: FilterOperator, cx: &mut Context<Self>) {
        let (field, field_label) = match &self.draft {
            FilterDraft::Operator { field, field_label } => (field.clone(), field_label.clone()),
            _ => return,
        };
        let carried_values: SmallVec<[SharedString; 4]> = self
            .editing_original
            .as_ref()
            .filter(|(_, orig)| orig.field == field)
            .map(|(_, orig)| orig.values.clone())
            .unwrap_or_default();
        self.query.clear();
        self.highlighted = 0;
        self.invalidate_facet_cache();
        self.draft = if op.is_multi() {
            FilterDraft::MultiValue {
                field,
                field_label,
                operator: op,
                pending: carried_values,
            }
        } else {
            FilterDraft::SingleValue {
                field,
                field_label,
                operator: op,
            }
        };
        self.open = true;
        self.blink.reset(cx);
        cx.notify();
    }

    fn commit_single_value(&mut self, value: SharedString, cx: &mut Context<Self>) {
        let (field, field_label, operator) = match &self.draft {
            FilterDraft::SingleValue {
                field,
                field_label,
                operator,
            } => (field.clone(), field_label.clone(), *operator),
            _ => return,
        };
        let new_clause = FilterClause {
            field,
            field_label,
            operator,
            values: smallvec::smallvec![value],
        };
        let unchanged = self
            .editing_original
            .as_ref()
            .is_some_and(|(_, orig)| orig == &new_clause);
        if let Some((orig_idx, _)) = self.editing_original.take() {
            let insert_at = orig_idx.min(self.clauses.len());
            self.clauses.insert(insert_at, new_clause);
        } else {
            self.clauses.push(new_clause);
        }
        self.draft = FilterDraft::Field;
        self.query.clear();
        self.highlighted = 0;
        self.invalidate_facet_cache();
        self.blink.reset(cx);
        if !unchanged {
            cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        }
        cx.notify();
    }

    fn toggle_multi_value(&mut self, value: SharedString, cx: &mut Context<Self>) {
        if let FilterDraft::MultiValue { pending, .. } = &mut self.draft {
            if let Some(pos) = pending.iter().position(|v| v == &value) {
                pending.remove(pos);
            } else {
                pending.push(value);
            }
            self.blink.reset(cx);
            cx.notify();
        }
    }

    fn clear_multi_pending(&mut self, cx: &mut Context<Self>) {
        if let FilterDraft::MultiValue { pending, .. } = &mut self.draft
            && !pending.is_empty()
        {
            pending.clear();
            self.blink.reset(cx);
            cx.notify();
        }
    }

    fn commit_multi_values(&mut self, cx: &mut Context<Self>) {
        let (field, field_label, operator, mut pending) = match &self.draft {
            FilterDraft::MultiValue {
                field,
                field_label,
                operator,
                pending,
            } => (
                field.clone(),
                field_label.clone(),
                *operator,
                pending.clone(),
            ),
            _ => return,
        };
        if pending.is_empty() {
            let facets = self.matching_facets(field.as_ref());
            if let Some(opt) = facets.get(self.highlighted) {
                pending.push(opt.value.clone());
            } else if !self.query.trim().is_empty() {
                pending.push(self.query.trim().to_string().into());
            } else {
                return;
            }
        }
        let new_clause = FilterClause {
            field,
            field_label,
            operator,
            values: pending,
        };
        let unchanged = self
            .editing_original
            .as_ref()
            .is_some_and(|(_, orig)| orig == &new_clause);
        if let Some((orig_idx, _)) = self.editing_original.take() {
            let insert_at = orig_idx.min(self.clauses.len());
            self.clauses.insert(insert_at, new_clause);
        } else {
            self.clauses.push(new_clause);
        }
        self.draft = FilterDraft::Field;
        self.query.clear();
        self.highlighted = 0;
        self.invalidate_facet_cache();
        self.blink.reset(cx);
        if !unchanged {
            cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        }
        cx.notify();
    }

    fn step_back(&mut self, cx: &mut Context<Self>) {
        let woke = self.blink.reset(cx);
        if !self.query.is_empty() {
            self.query.pop();
            self.highlighted = 0;
            cx.notify();
            return;
        }
        match &mut self.draft {
            FilterDraft::MultiValue {
                field,
                field_label,
                pending,
                ..
            } => {
                if !pending.is_empty() {
                    pending.pop();
                } else {
                    self.draft = FilterDraft::Operator {
                        field: field.clone(),
                        field_label: field_label.clone(),
                    };
                }
                self.highlighted = 0;
                cx.notify();
            }
            FilterDraft::SingleValue {
                field, field_label, ..
            } => {
                self.draft = FilterDraft::Operator {
                    field: field.clone(),
                    field_label: field_label.clone(),
                };
                self.highlighted = 0;
                cx.notify();
            }
            FilterDraft::Operator { .. } => {
                self.draft = FilterDraft::Field;
                self.highlighted = 0;
                if self.editing_original.take().is_some() {
                    self.invalidate_facet_cache();
                    cx.emit(FilterBarEvent::Change(self.clauses.clone()));
                }
                cx.notify();
            }
            FilterDraft::Field => {
                if !self.clauses.is_empty() {
                    self.clauses.pop();
                    self.invalidate_facet_cache();
                    cx.emit(FilterBarEvent::Change(self.clauses.clone()));
                    cx.notify();
                } else if woke {
                    cx.notify();
                }
            }
        }
    }

    fn current_option_count(&self) -> usize {
        match &self.draft {
            FilterDraft::Field => self.matching_fields().len(),
            FilterDraft::Operator { field, .. } => self.matching_operators(field.as_ref()).len(),
            FilterDraft::SingleValue { field, .. } | FilterDraft::MultiValue { field, .. } => {
                self.matching_facets(field.as_ref()).len()
            }
        }
    }

    /// How far the draft clause has got: field, operator, then value.
    fn draft_step(&self) -> u8 {
        match self.draft {
            FilterDraft::Field => 0,
            FilterDraft::Operator { .. } => 1,
            FilterDraft::SingleValue { .. } | FilterDraft::MultiValue { .. } => 2,
        }
    }

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        if key == "tab" {
            self.handle_tab(event, cx);
            return;
        }
        match key {
            "escape" => {
                self.restore_editing_original();
                self.open = false;
                self.draft = FilterDraft::Field;
                self.query.clear();
                self.highlighted = 0;
                self.invalidate_facet_cache();
                self.blink.reset(cx);
                cx.notify();
            }
            "down" => {
                self.open = true;
                let count = self.current_option_count();
                if count > 0 {
                    self.highlighted = (self.highlighted + 1) % count;
                    self.scroll_handle
                        .scroll_to_item(self.highlighted, ScrollStrategy::Nearest);
                }
                self.blink.reset(cx);
                cx.notify();
            }
            "up" => {
                self.open = true;
                let count = self.current_option_count();
                if count > 0 {
                    self.highlighted = (self.highlighted + count - 1) % count;
                    self.scroll_handle
                        .scroll_to_item(self.highlighted, ScrollStrategy::Nearest);
                }
                self.blink.reset(cx);
                cx.notify();
            }
            "backspace" => self.step_back(cx),
            "space" if self.query.is_empty() => {
                if let FilterDraft::MultiValue { field, .. } = &self.draft {
                    let facets = self.matching_facets(field.as_ref());
                    if let Some(opt) = facets.get(self.highlighted) {
                        self.toggle_multi_value(opt.value.clone(), cx);
                    }
                }
            }
            "enter" => self.commit_step(cx),
            _ => {
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                    && let Some(ch) = &event.keystroke.key_char
                    && !ch.chars().any(|c| c.is_control())
                {
                    self.open = true;
                    self.query.push_str(ch);
                    self.highlighted = 0;
                    self.blink.reset(cx);
                    cx.notify();
                }
            }
        }
    }

    /// Enter: completes the draft's current step (picks the highlighted
    /// field, operator or value; commits a multi-value clause).
    fn commit_step(&mut self, cx: &mut Context<Self>) {
        match &self.draft {
            FilterDraft::Field => {
                let fields = self.matching_fields();
                if let Some(field) = fields.get(self.highlighted) {
                    self.choose_field(field, cx);
                }
            }
            FilterDraft::Operator { field, .. } => {
                let ops = self.matching_operators(field.as_ref());
                if let Some(&op) = ops.get(self.highlighted) {
                    self.choose_operator(op, cx);
                }
            }
            FilterDraft::SingleValue { field, .. } => {
                let facets = self.matching_facets(field.as_ref());
                if let Some(opt) = facets.get(self.highlighted) {
                    self.commit_single_value(opt.value.clone(), cx);
                } else if !self.query.trim().is_empty() {
                    let custom: SharedString = self.query.trim().to_string().into();
                    self.commit_single_value(custom, cx);
                }
            }
            FilterDraft::MultiValue { .. } => {
                self.commit_multi_values(cx);
            }
        }
    }

    /// Tab completes the draft's current step as Enter does, and then moves
    /// focus on, except when that step started the next one of a clause in
    /// progress (field -> operator -> value): then focus stays so the user
    /// can finish the clause, as Tab completion does in a shell. A Tab with
    /// nothing typed and no draft just moves on, so tabbing through the bar
    /// doesn't pick a field.
    fn handle_tab(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let mods = event.keystroke.modifiers;
        let writing = self.draft_step() > 0 || !self.query.is_empty();
        if mods.shift || !writing {
            self.close_menu(cx);
            return;
        }
        let before = self.draft_step();
        self.commit_step(cx);
        if self.draft_step() > before {
            cx.stop_propagation();
        } else {
            self.close_menu(cx);
        }
    }
}

/// Structured faceted filter bar supporting single-value (`=`), multi-value (`is one of`),
/// live facet hit counts, and keyboard-first editing.
#[derive(IntoElement)]
pub struct FilterBar {
    id: ElementId,
    state: Entity<FilterBarState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`FilterBar::new`].
pub fn filter_bar(id: impl Into<ElementId>, state: &Entity<FilterBarState>) -> FilterBar {
    FilterBar::new(id, state)
}

#[derive(Clone)]
struct TooltipValue {
    label: SharedString,
    /// Raw wire ID shown muted beneath `label` when it differs meaningfully from `label`.
    raw: Option<SharedString>,
}

/// Rich multi-line hover tooltip showing full untruncated filter clause values.
struct FilterClauseTooltip {
    field_label: SharedString,
    operator_label: &'static str,
    values: SmallVec<[TooltipValue; 4]>,
}

impl crate::Render for FilterClauseTooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let bg = theme.tooltip_bg;
        let fg = theme.tooltip_foreground;
        let border = theme.tooltip_border;
        let multi = self.values.len() > 1;
        let header = if multi {
            format!(
                "{} {} ({})",
                self.field_label,
                self.operator_label,
                self.values.len()
            )
        } else {
            format!("{} {}", self.field_label, self.operator_label)
        };
        let extra = self.values.len().saturating_sub(TOOLTIP_MAX_LISTED_VALUES);

        let mut list = div().flex().flex_col().gap(px(4.));
        for val in self.values.iter().take(TOOLTIP_MAX_LISTED_VALUES) {
            list = list.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(1.))
                    .child(
                        div()
                            .flex()
                            .items_center()
                            .gap(px(6.))
                            .text_size(px(12.))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(fg)
                            .font_features(tabular_numerals())
                            .when(multi, |el| {
                                el.child(
                                    div()
                                        .w(px(4.))
                                        .h(px(4.))
                                        .flex_shrink_0()
                                        .rounded_full()
                                        .bg(fg.opacity(0.55)),
                                )
                            })
                            .child(val.label.clone()),
                    )
                    .when_some(val.raw.clone(), |el, raw| {
                        el.child(
                            div()
                                .when(multi, |d| d.pl(px(10.)))
                                .text_size(px(10.5))
                                .text_color(fg.opacity(0.6))
                                .font_features(tabular_numerals())
                                .child(raw),
                        )
                    }),
            );
        }
        if extra > 0 {
            list = list.child(
                div()
                    .text_size(px(11.))
                    .text_color(fg.opacity(0.7))
                    .child(format!("+{extra} more")),
            );
        }

        div()
            .max_w(px(420.))
            .px(px(10.))
            .py(px(7.))
            .rounded(theme.radius_md)
            .bg(bg)
            .text_color(fg)
            .border_1()
            .border_color(border)
            .when(theme.elevation_shadows, |el| el.shadow_md())
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(
                div()
                    .text_size(px(11.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(fg.opacity(0.7))
                    .child(header),
            )
            .child(list)
            .child(
                div()
                    .pt(px(2.))
                    .text_size(px(10.5))
                    .text_color(fg.opacity(0.55))
                    .child("Click to edit"),
            )
    }
}

impl FilterBar {
    /// Creates a new [`FilterBar`] bound to `state`.
    pub fn new(id: impl Into<ElementId>, state: &Entity<FilterBarState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }

    fn render_committed_chip(
        &self,
        idx: usize,
        clause: &FilterClause,
        value_label: Option<&ValueLabelFormatter>,
        theme: &Theme,
    ) -> impl IntoElement {
        let hover_bg = theme.surface_hover;
        let state_op = self.state.clone();
        let state_val = self.state.clone();
        let state_rm = self.state.clone();
        let labels: SmallVec<[SharedString; 4]> = clause
            .values
            .iter()
            .map(|v| format_value_label(value_label, clause.field.as_ref(), v))
            .collect();
        let (primary_val, extra_count) = display_value_parts(&labels);
        let val_max_w = if extra_count.is_some() {
            MAX_MULTI_VALUE_WIDTH
        } else {
            MAX_SINGLE_VALUE_WIDTH
        };
        let tip_values: SmallVec<[TooltipValue; 4]> = clause
            .values
            .iter()
            .zip(labels.iter())
            .map(|(raw, label)| TooltipValue {
                label: label.clone(),
                raw: (!label.eq_ignore_ascii_case(raw.as_ref())).then(|| raw.clone()),
            })
            .collect();
        let needs_tooltip = labels.len() > 1
            || labels
                .first()
                .is_some_and(|v| v.chars().count() > VALUE_TOOLTIP_MIN_CHARS)
            || tip_values.iter().any(|v| v.raw.is_some());
        let tip_field = clause.field_label.clone();
        let tip_op = clause.operator.label();

        let val_el = div()
            .id(("clause-val", idx))
            .min_w_0()
            .px(px(7.))
            .h_full()
            .flex()
            .items_center()
            .gap(px(5.))
            .overflow_hidden()
            .cursor_pointer()
            .hover(move |s| s.bg(hover_bg))
            .on_click(move |_: &ClickEvent, window, cx| {
                cx.stop_propagation();
                state_val.update(cx, |s, cx| s.edit_clause_value(idx, window, cx));
            })
            .child(
                div()
                    .min_w_0()
                    .max_w(px(val_max_w))
                    .truncate()
                    .whitespace_nowrap()
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(primary_val),
            )
            .when_some(extra_count, |el, extra| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded_full()
                        .bg(theme.primary_soft)
                        .text_size(px(10.5))
                        .line_height(px(13.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.primary_soft_foreground)
                        .font_features(tabular_numerals())
                        .whitespace_nowrap()
                        .child(format!("+{extra}")),
                )
            })
            .when(needs_tooltip, |el| {
                el.tooltip(move |_window, cx| {
                    let field_label = tip_field.clone();
                    let values = tip_values.clone();
                    cx.new(|_| FilterClauseTooltip {
                        field_label,
                        operator_label: tip_op,
                        values,
                    })
                    .into()
                })
            });

        div()
            .id(("clause", idx))
            .max_w_full()
            .min_w_0()
            .flex()
            .items_center()
            .h(px(26.))
            .rounded(theme.radius_md)
            .bg(theme.surface_muted)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .text_size(px(12.))
            .child(
                div()
                    .flex_shrink_0()
                    .max_w(px(MAX_FIELD_LABEL_WIDTH))
                    .px(px(7.))
                    .h_full()
                    .flex()
                    .items_center()
                    .overflow_hidden()
                    .child(
                        div()
                            .min_w_0()
                            .truncate()
                            .whitespace_nowrap()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.muted_foreground)
                            .child(clause.field_label.clone()),
                    ),
            )
            .child(div().w(px(1.)).h_full().flex_shrink_0().bg(theme.border))
            .child(
                div()
                    .id(("clause-op", idx))
                    .flex_shrink_0()
                    .whitespace_nowrap()
                    .px(px(7.))
                    .h_full()
                    .flex()
                    .items_center()
                    .cursor_pointer()
                    .text_color(theme.muted_foreground)
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(move |_: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        state_op.update(cx, |s, cx| s.edit_clause_operator(idx, window, cx));
                    })
                    .child(clause.operator.label()),
            )
            .child(div().w(px(1.)).h_full().flex_shrink_0().bg(theme.border))
            .child(val_el)
            .child(div().w(px(1.)).h_full().flex_shrink_0().bg(theme.border))
            .child(
                div()
                    .id(("clause-rm", idx))
                    .flex_shrink_0()
                    .px(px(5.))
                    .h_full()
                    .flex()
                    .items_center()
                    .justify_center()
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .tooltip(|window, cx| Tooltip::text("Remove filter", window, cx))
                    .on_click(move |_: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        state_rm.update(cx, |s, cx| s.remove_clause(idx, cx));
                    })
                    .child(
                        Icon::new(LucideIcon::X)
                            .size(IconSize::Xs)
                            .color(theme.muted_foreground),
                    ),
            )
    }

    fn render_draft_chip(
        draft: &FilterDraft,
        value_label: Option<&ValueLabelFormatter>,
        theme: &Theme,
    ) -> Option<AnyElement> {
        let (field_label, operator, pending_parts) = match draft {
            FilterDraft::Field => return None,
            FilterDraft::Operator { field_label, .. } => (field_label.clone(), None, None),
            FilterDraft::SingleValue {
                field_label,
                operator,
                ..
            } => (field_label.clone(), Some(*operator), None),
            FilterDraft::MultiValue {
                field,
                field_label,
                operator,
                pending,
            } => {
                let parts = if pending.is_empty() {
                    None
                } else {
                    let labels: SmallVec<[SharedString; 4]> = pending
                        .iter()
                        .map(|v| format_value_label(value_label, field.as_ref(), v))
                        .collect();
                    Some(display_value_parts(&labels))
                };
                (field_label.clone(), Some(*operator), parts)
            }
        };

        let sep = || div().w(px(1.)).h_full().flex_shrink_0().bg(theme.ring.opacity(0.3));
        Some(
            div()
                .max_w_full()
                .min_w_0()
                .flex()
                .items_center()
                .h(px(26.))
                .rounded(theme.radius_md)
                .bg(theme.primary_soft)
                .border_1()
                .border_color(theme.ring.opacity(0.45))
                .overflow_hidden()
                .text_size(px(12.))
                .child(
                    div()
                        .flex_shrink_0()
                        .max_w(px(MAX_FIELD_LABEL_WIDTH))
                        .px(px(8.))
                        .h_full()
                        .flex()
                        .items_center()
                        .overflow_hidden()
                        .child(
                            div()
                                .min_w_0()
                                .truncate()
                                .whitespace_nowrap()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.primary_soft_foreground)
                                .child(field_label),
                        ),
                )
                .when_some(operator, |el, op| {
                    el.child(sep()).child(
                        div()
                            .flex_shrink_0()
                            .whitespace_nowrap()
                            .px(px(8.))
                            .h_full()
                            .flex()
                            .items_center()
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.primary_soft_foreground)
                            .child(op.label()),
                    )
                })
                .when_some(pending_parts, |el, (primary, extra_count)| {
                    let max_w = if extra_count.is_some() {
                        MAX_MULTI_VALUE_WIDTH
                    } else {
                        MAX_SINGLE_VALUE_WIDTH
                    };
                    el.child(sep()).child(
                        div()
                            .min_w_0()
                            .px(px(8.))
                            .h_full()
                            .flex()
                            .items_center()
                            .gap(px(5.))
                            .overflow_hidden()
                            .child(
                                div()
                                    .min_w_0()
                                    .max_w(px(max_w))
                                    .truncate()
                                    .whitespace_nowrap()
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.foreground)
                                    .child(primary),
                            )
                            .when_some(extra_count, |el, extra| {
                                el.child(
                                    div()
                                        .flex_shrink_0()
                                        .px(px(5.))
                                        .py(px(1.))
                                        .rounded_full()
                                        .bg(theme.primary.opacity(0.2))
                                        .text_size(px(10.5))
                                        .line_height(px(13.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.primary_soft_foreground)
                                        .font_features(tabular_numerals())
                                        .whitespace_nowrap()
                                        .child(format!("+{extra}")),
                                )
                            }),
                    )
                })
                .into_any_element(),
        )
    }

    fn render_inline_input(
        query: String,
        focused: bool,
        cursor_visible: bool,
        has_content: bool,
        placeholder: SharedString,
        theme: &Theme,
    ) -> impl IntoElement {
        if !query.is_empty() {
            div()
                .min_h(px(24.))
                .flex()
                .items_center()
                .whitespace_nowrap()
                .text_size(px(13.))
                .text_color(theme.foreground)
                .child(query)
                .when(focused, |el| {
                    el.child(
                        div()
                            .w(px(1.5))
                            .h(px(15.))
                            .ml(px(1.))
                            .when(cursor_visible, |el| el.bg(theme.primary)),
                    )
                })
        } else if focused {
            div()
                .min_h(px(24.))
                .flex()
                .items_center()
                .gap(px(4.))
                .child(
                    div()
                        .w(px(1.5))
                        .h(px(15.))
                        .when(cursor_visible, |el| el.bg(theme.primary)),
                )
                .when(!has_content, |el| {
                    el.child(
                        div()
                            .whitespace_nowrap()
                            .text_size(px(13.))
                            .text_color(theme.placeholder)
                            .child(placeholder),
                    )
                })
        } else if !has_content {
            div()
                .min_h(px(24.))
                .flex()
                .items_center()
                .whitespace_nowrap()
                .text_size(px(13.))
                .text_color(theme.placeholder)
                .child(placeholder)
        } else {
            div().w(px(4.)).h(px(24.))
        }
    }

    fn render_empty_notice(theme: &Theme) -> impl IntoElement {
        div()
            .px(px(10.))
            .py(px(8.))
            .text_size(px(12.5))
            .text_color(theme.muted_foreground)
            .child("No matching values")
    }

    fn render_single_facet_row(
        idx: usize,
        opt: &FilterFacetOption,
        active: bool,
        state: Entity<FilterBarState>,
        theme: &Theme,
    ) -> impl IntoElement + use<> {
        let hover_bg = theme.surface_hover;
        let val = opt.value.clone();
        let long_label = opt.label.chars().count() > FACET_TOOLTIP_MIN_CHARS;
        let tip_text = opt.label.clone();
        div()
            .id(("opt-val", idx))
            .w_full()
            .h(px(32.))
            .px(px(10.))
            .rounded(theme.radius_md)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .cursor_pointer()
            .text_size(px(13.))
            .when(active, |el| el.bg(theme.primary_soft))
            .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
            .when(long_label, |el| {
                el.tooltip(move |window, cx| Tooltip::text(tip_text.clone(), window, cx))
            })
            .on_click(move |_: &ClickEvent, _, cx| {
                state.update(cx, |s, cx| {
                    s.commit_single_value(val.clone(), cx);
                });
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .font_weight(if active {
                        FontWeight::SEMIBOLD
                    } else {
                        FontWeight::MEDIUM
                    })
                    .text_color(if active {
                        theme.primary_soft_foreground
                    } else {
                        theme.foreground
                    })
                    .truncate()
                    .whitespace_nowrap()
                    .child(opt.label.clone()),
            )
            .when_some(opt.count, |el, count| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .font_features(tabular_numerals())
                        .text_color(theme.muted_foreground)
                        .child(format!("{count}")),
                )
            })
    }

    fn render_multi_facet_row(
        idx: usize,
        opt: &FilterFacetOption,
        active: bool,
        checked: bool,
        state: Entity<FilterBarState>,
        theme: &Theme,
    ) -> impl IntoElement + use<> {
        let hover_bg = theme.surface_hover;
        let val = opt.value.clone();
        let long_label = opt.label.chars().count() > FACET_TOOLTIP_MIN_CHARS;
        let tip_text = opt.label.clone();
        let checkbox_box = div()
            .w(px(16.))
            .h(px(16.))
            .flex_shrink_0()
            .rounded(px(4.))
            .flex()
            .items_center()
            .justify_center()
            .border_1()
            .when(checked, |el| {
                el.bg(theme.primary)
                    .border_color(theme.primary)
                    .child(
                        Icon::new(LucideIcon::Check)
                            .size(IconSize::Xs)
                            .color(theme.primary_foreground),
                    )
            })
            .when(!checked, |el| {
                el.bg(theme.surface).border_color(theme.input_border)
            });

        div()
            .id(("opt-mval", idx))
            .w_full()
            .h(px(32.))
            .px(px(10.))
            .rounded(theme.radius_md)
            .flex()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .cursor_pointer()
            .text_size(px(13.))
            .when(active, |el| el.bg(theme.primary_soft))
            .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
            .when(long_label, |el| {
                el.tooltip(move |window, cx| Tooltip::text(tip_text.clone(), window, cx))
            })
            .on_click(move |_: &ClickEvent, _, cx| {
                state.update(cx, |s, cx| {
                    s.toggle_multi_value(val.clone(), cx);
                });
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .flex()
                    .items_center()
                    .gap(px(8.))
                    .child(checkbox_box)
                    .child(
                        div()
                            .flex_1()
                            .min_w_0()
                            .font_weight(if checked || active {
                                FontWeight::SEMIBOLD
                            } else {
                                FontWeight::MEDIUM
                            })
                            .text_color(if active {
                                theme.primary_soft_foreground
                            } else {
                                theme.foreground
                            })
                            .truncate()
                            .whitespace_nowrap()
                            .child(opt.label.clone()),
                    ),
            )
            .when_some(opt.count, |el, count| {
                el.child(
                    div()
                        .flex_shrink_0()
                        .text_size(px(12.))
                        .font_features(tabular_numerals())
                        .text_color(theme.muted_foreground)
                        .child(format!("{count}")),
                )
            })
    }

    #[allow(clippy::too_many_arguments)]
    fn render_popover(
        &self,
        draft: &FilterDraft,
        highlighted: usize,
        field_options: &[FilterFieldDef],
        operator_options: &[FilterOperator],
        facet_options: &Rc<[FilterFacetOption]>,
        scroll_handle: &UniformListScrollHandle,
        theme: &std::sync::Arc<Theme>,
    ) -> AnyElement {
        let hover_bg = theme.surface_hover;
        let state_out = self.state.clone();

        let header_hint: Option<&'static str> = match draft {
            FilterDraft::Field => Some("Filter by field"),
            FilterDraft::Operator { .. } => Some("Choose operator"),
            FilterDraft::SingleValue { .. } => None,
            FilterDraft::MultiValue { .. } => Some("Choose any values, then Enter"),
        };

        let list_body: AnyElement = match draft {
            FilterDraft::Field => {
                let mut list = div().flex().flex_col().gap(px(2.));
                for (idx, field) in field_options.iter().enumerate() {
                    let active = idx == highlighted;
                    let field_clone = field.clone();
                    let state = self.state.clone();
                    list = list.child(
                        div()
                            .id(("opt-field", idx))
                            .w_full()
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(theme.radius_md)
                            .flex()
                            .items_center()
                            .justify_between()
                            .gap(px(12.))
                            .cursor_pointer()
                            .text_size(px(13.))
                            .when(active, |el| el.bg(theme.primary_soft))
                            .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                            .on_click(move |_: &ClickEvent, _, cx| {
                                state.update(cx, |s, cx| s.choose_field(&field_clone, cx));
                            })
                            .child(
                                div()
                                    .flex_1()
                                    .min_w_0()
                                    .truncate()
                                    .whitespace_nowrap()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if active {
                                        theme.primary_soft_foreground
                                    } else {
                                        theme.foreground
                                    })
                                    .child(field.label.clone()),
                            )
                            .when_some(field.hint.clone(), |el, hint| {
                                el.child(
                                    div()
                                        .flex_shrink_0()
                                        .whitespace_nowrap()
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .child(hint),
                                )
                            }),
                    );
                }
                div()
                    .id((self.id.clone(), "filter-field-scroll"))
                    .max_h(px(280.))
                    .overflow_y_scroll()
                    .child(list)
                    .into_any_element()
            }
            FilterDraft::Operator { .. } => {
                let mut list = div().flex().flex_col().gap(px(2.));
                for (idx, &op) in operator_options.iter().enumerate() {
                    let active = idx == highlighted;
                    let state = self.state.clone();
                    list = list.child(
                        div()
                            .id(("opt-op", idx))
                            .px(px(10.))
                            .py(px(7.))
                            .rounded(theme.radius_md)
                            .flex()
                            .items_center()
                            .justify_between()
                            .cursor_pointer()
                            .text_size(px(13.))
                            .when(active, |el| el.bg(theme.primary_soft))
                            .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                            .on_click(move |_: &ClickEvent, _, cx| {
                                state.update(cx, |s, cx| s.choose_operator(op, cx));
                            })
                            .child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(if active {
                                        theme.primary_soft_foreground
                                    } else {
                                        theme.foreground
                                    })
                                    .child(op.label()),
                            )
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .text_color(theme.muted_foreground)
                                    .child(if op.is_multi() { "multi" } else { "single" }),
                            ),
                    );
                }
                list.into_any_element()
            }
            FilterDraft::SingleValue { .. } => {
                if facet_options.is_empty() {
                    Self::render_empty_notice(theme).into_any_element()
                } else if facet_options.len() <= 10 {
                    let mut list = div().flex().flex_col().gap(px(2.));
                    for (idx, opt) in facet_options.iter().enumerate() {
                        list = list.child(Self::render_single_facet_row(
                            idx,
                            opt,
                            idx == highlighted,
                            self.state.clone(),
                            theme,
                        ));
                    }
                    list.into_any_element()
                } else {
                    let options = facet_options.clone();
                    let state = self.state.clone();
                    let theme_cloned = theme.clone();
                    uniform_list("filter-single-vlist", options.len(), move |range, _, _| {
                        range
                            .map(|idx| {
                                Self::render_single_facet_row(
                                    idx,
                                    &options[idx],
                                    idx == highlighted,
                                    state.clone(),
                                    &theme_cloned,
                                )
                            })
                            .collect()
                    })
                    .track_scroll(scroll_handle)
                    .w_full()
                    .h(px(280.))
                    .into_any_element()
                }
            }
            FilterDraft::MultiValue { pending, .. } => {
                if facet_options.is_empty() {
                    Self::render_empty_notice(theme).into_any_element()
                } else if facet_options.len() <= 10 {
                    let mut list = div().flex().flex_col().gap(px(2.));
                    for (idx, opt) in facet_options.iter().enumerate() {
                        let checked = pending.iter().any(|v| v == &opt.value);
                        list = list.child(Self::render_multi_facet_row(
                            idx,
                            opt,
                            idx == highlighted,
                            checked,
                            self.state.clone(),
                            theme,
                        ));
                    }
                    list.into_any_element()
                } else {
                    let options = facet_options.clone();
                    let pending_rc: Rc<[SharedString]> = Rc::from(pending.as_slice());
                    let state = self.state.clone();
                    let theme_cloned = theme.clone();
                    uniform_list("filter-multi-vlist", options.len(), move |range, _, _| {
                        range
                            .map(|idx| {
                                let opt = &options[idx];
                                let checked = pending_rc.iter().any(|v| v == &opt.value);
                                Self::render_multi_facet_row(
                                    idx,
                                    opt,
                                    idx == highlighted,
                                    checked,
                                    state.clone(),
                                    &theme_cloned,
                                )
                            })
                            .collect()
                    })
                    .track_scroll(scroll_handle)
                    .w_full()
                    .h(px(280.))
                    .into_any_element()
                }
            }
        };

        let multi_footer = match draft {
            FilterDraft::MultiValue { pending, .. } if !pending.is_empty() => {
                let state_commit = self.state.clone();
                let state_clear = self.state.clone();
                Some(
                    div()
                        .mt(px(4.))
                        .pt(px(6.))
                        .px(px(6.))
                        .border_t_1()
                        .border_color(theme.border)
                        .flex()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .child(
                            div()
                                .flex()
                                .items_center()
                                .gap(px(8.))
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{} selected", pending.len())),
                                )
                                .child(
                                    div()
                                        .id("multi-clear")
                                        .px(px(6.))
                                        .py(px(2.))
                                        .rounded(theme.radius_sm)
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .cursor_pointer()
                                        .hover(move |s| s.bg(hover_bg))
                                        .on_click(move |_: &ClickEvent, _, cx| {
                                            state_clear
                                                .update(cx, |s, cx| s.clear_multi_pending(cx));
                                        })
                                        .child("Clear"),
                                ),
                        )
                        .child(
                            div()
                                .id("multi-commit")
                                .px(px(10.))
                                .py(px(4.))
                                .rounded(theme.radius_md)
                                .bg(theme.primary)
                                .text_size(px(12.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.primary_foreground)
                                .cursor_pointer()
                                .on_click(move |_: &ClickEvent, _, cx| {
                                    state_commit.update(cx, |s, cx| s.commit_multi_values(cx));
                                })
                                .child("Apply (Enter)"),
                        ),
                )
            }
            _ => None,
        };

        let popover = div()
            .id((self.id.clone(), "filter-popover"))
            .occlude()
            .min_w(px(280.))
            .max_w(px(440.))
            .mt(px(6.))
            .p(px(6.))
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .flex()
            .flex_col()
            // A click on a suggestion keeps focus in the bar.
            .on_mouse_down(crate::MouseButton::Left, |_, window, _| window.prevent_default())
            .on_mouse_down_out(move |_, _, cx| {
                state_out.update(cx, |s, cx| s.close_menu(cx));
            })
            .when_some(header_hint, |el, hint| {
                el.child(
                    div()
                        .px(px(10.))
                        .pt(px(4.))
                        .pb(px(6.))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(hint),
                )
            })
            .child(list_body)
            .when_some(multi_footer, |el, footer| el.child(footer))
            .with_animation(
                (self.id.clone(), "filter-pop-enter"),
                Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                |el, delta| el.opacity(delta).mt(px(2. + (1. - delta) * 6.)),
            );

        deferred(
            anchored()
                .snap_to_window_with_margin(px(8.))
                .child(popover),
        )
        .with_priority(2)
        .into_any_element()
    }
}

impl Styled for FilterBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FilterBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (
            clauses,
            draft,
            query,
            open,
            highlighted,
            placeholder,
            value_label,
            focus_handle,
            scroll_handle,
            focused,
            has_content,
            field_options,
            operator_options,
            facet_options,
            needs_blink_sync,
            cursor_visible,
        ) = {
            let snap = self.state.read(cx);
            let clauses = snap.clauses.clone();
            let draft = snap.draft.clone();
            let query = snap.query.clone();
            let open = snap.open;
            let highlighted = snap.highlighted;
            let placeholder = snap.placeholder.clone();
            let value_label = snap.value_label.clone();
            let focus_handle = snap.focus_handle.clone();
            let scroll_handle = snap.scroll_handle.clone();
            let focused = focus_handle.is_focused(window);
            let has_content = !clauses.is_empty()
                || !matches!(draft, FilterDraft::Field)
                || !query.is_empty();

            // Snapshot popover suggestions up front so we don't re-borrow `state`.
            let field_options = if open && matches!(draft, FilterDraft::Field) {
                snap.matching_fields()
            } else {
                Vec::new()
            };
            let operator_options = if open
                && let FilterDraft::Operator { field, .. } = &draft
            {
                snap.matching_operators(field.as_ref())
            } else {
                Vec::new()
            };
            let facet_options: Rc<[FilterFacetOption]> = if open {
                match &draft {
                    FilterDraft::SingleValue { field, .. }
                    | FilterDraft::MultiValue { field, .. } => snap.matching_facets(field.as_ref()),
                    _ => Rc::from([]),
                }
            } else {
                Rc::from([])
            };
            let should_blink = focused && window.is_window_active() && !cx.reduce_motion();
            let needs_blink_sync = snap.blink.needs_sync(should_blink, window);
            let cursor_visible = window.is_window_active() && snap.blink.is_visible();
            (
                clauses,
                draft,
                query,
                open,
                highlighted,
                placeholder,
                value_label,
                focus_handle,
                scroll_handle,
                focused,
                has_content,
                field_options,
                operator_options,
                facet_options,
                needs_blink_sync,
                cursor_visible,
            )
        };

        let cursor_visible = if needs_blink_sync {
            let should_blink = focused && window.is_window_active() && !cx.reduce_motion();
            self.state.update(cx, |s, cx| {
                s.blink.sync(
                    should_blink,
                    window,
                    cx,
                    |this| &mut this.blink,
                    |this, window, _cx| {
                        is_mounted_and_focused(&this.focus_handle, window)
                            && window.is_window_active()
                    },
                );
                window.is_window_active() && s.blink.is_visible()
            })
        } else {
            cursor_visible
        };

        let hover_bg = theme.surface_hover;
        let border_color = if focused || open {
            theme.ring
        } else {
            theme.input_border
        };

        let mut tokens_row = div()
            .flex_1()
            .min_w_0()
            .flex()
            .flex_wrap()
            .items_center()
            .gap(px(6.));

        for (idx, clause) in clauses.iter().enumerate() {
            tokens_row = tokens_row.child(self.render_committed_chip(
                idx,
                clause,
                value_label.as_ref(),
                &theme,
            ));
        }
        if let Some(draft_chip) = Self::render_draft_chip(&draft, value_label.as_ref(), &theme) {
            tokens_row = tokens_row.child(draft_chip);
        }
        tokens_row = tokens_row.child(Self::render_inline_input(
            query,
            focused,
            cursor_visible,
            has_content,
            placeholder,
            &theme,
        ));

        let mut bar = div()
            .id("bar")
            .track_focus(&focus_handle)
            .on_key_down({
                let state = self.state.clone();
                move |event: &KeyDownEvent, _, cx| {
                    state.update(cx, |s, cx| s.handle_key_down(event, cx));
                }
            })
            .on_click({
                let state = self.state.clone();
                move |_: &ClickEvent, window, cx| {
                    state.update(cx, |s, cx| s.open_menu(window, cx));
                }
            })
            .w_full()
            .min_h(px(38.))
            .px(px(12.))
            .py(px(5.))
            .flex()
            .items_center()
            .gap(px(8.))
            .rounded(theme.radius_xl)
            .bg(theme.input_bg)
            .border_1()
            .border_color(border_color)
            .when(theme.elevation_shadows, |el| el.shadow_xs())
            .cursor_text()
            .child(
                div()
                    .flex_shrink_0()
                    .flex()
                    .items_center()
                    .child(
                        Icon::new(LucideIcon::Funnel)
                            .size(IconSize::Sm)
                            .color(theme.muted_foreground),
                    ),
            )
            .child(tokens_row)
            .when(has_content, |el| {
                let state = self.state.clone();
                el.child(
                    div()
                        .id("clear-all")
                        .flex_shrink_0()
                        .w(px(22.))
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme.radius_sm)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .tooltip(|window, cx| Tooltip::text("Clear all filters", window, cx))
                        .on_click(move |_: &ClickEvent, _, cx| {
                            cx.stop_propagation();
                            state.update(cx, |s, cx| s.clear_all(cx));
                        })
                        .child(
                            Icon::new(LucideIcon::X)
                                .size(IconSize::Xs)
                                .color(theme.muted_foreground),
                        ),
                )
            });

        crate::Refineable::refine(bar.style(), &self.style);

        let popover = open.then(|| {
            self.render_popover(
                &draft,
                highlighted,
                &field_options,
                &operator_options,
                &facet_options,
                &scroll_handle,
                &theme,
            )
        });

        div()
            .id(self.id)
            .relative()
            .w_full()
            .flex_shrink_0()
            .child(bar)
            .when_some(popover, |el, p| el.child(p))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{TestAppContext, size};

    #[test]
    fn chip_value_parts_and_summary_keep_long_values_compact() {
        let short_pair = FilterClause {
            field: "provider".into(),
            field_label: "Provider".into(),
            operator: FilterOperator::OneOf,
            values: smallvec::smallvec!["OpenAI".into(), "Anthropic".into()],
        };
        assert_eq!(short_pair.values_summary(), "OpenAI, Anthropic");
        assert_eq!(
            short_pair.chip_value_parts(),
            ("OpenAI, Anthropic".into(), None)
        );
        assert!(short_pair.needs_value_tooltip());

        let uuid_pair = FilterClause {
            field: "label.virtual_key".into(),
            field_label: "virtual_key".into(),
            operator: FilterOperator::OneOf,
            values: smallvec::smallvec![
                "1d8acbc2-5644-4ade-ad5b-6a7033eee00a".into(),
                "700c0e37-d268-4db8-8ffa-83df9e63fb1f".into(),
            ],
        };
        assert_eq!(
            uuid_pair.values_summary(),
            "1d8acbc2-5644-4ade-ad5b-6a7033eee00a, +1"
        );
        assert_eq!(
            uuid_pair.chip_value_parts(),
            ("1d8acbc2-5644-4ade-ad5b-6a7033eee00a".into(), Some(1))
        );
        assert!(uuid_pair.needs_value_tooltip());

        let short_single = FilterClause {
            field: "service".into(),
            field_label: "Service".into(),
            operator: FilterOperator::Eq,
            values: smallvec::smallvec!["gateway".into()],
        };
        assert!(!short_single.needs_value_tooltip());

        let long_single = FilterClause {
            field: "label.virtual_key".into(),
            field_label: "virtual_key".into(),
            operator: FilterOperator::Eq,
            values: smallvec::smallvec!["1d8acbc2-5644-4ade-ad5b-6a7033eee00a".into()],
        };
        assert!(long_single.needs_value_tooltip());
    }

    #[crate::test]
    fn non_destructive_edit_restores_on_cancel_and_updates_in_place(cx: &mut TestAppContext) {
        cx.update(crate::components::init_components);
        let uuid_a: SharedString = "1d8acbc2-5644-4ade-ad5b-6a7033eee00a".into();
        let uuid_b: SharedString = "700c0e37-d268-4db8-8ffa-83df9e63fb1f".into();
        let uuid_c: SharedString = "9f10b8e4-1200-4c9a-9ab1-112233445566".into();
        let changes = Rc::new(std::cell::Cell::new(0usize));
        let changes_sub = changes.clone();

        let (uuid_a2, uuid_b2, uuid_c2) = (uuid_a.clone(), uuid_b.clone(), uuid_c.clone());
        let window = cx.open_window(size(px(640.), px(420.)), move |_, cx| {
            struct Host(Entity<FilterBarState>);
            impl crate::Render for Host {
                fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                    div().w(px(520.)).child(filter_bar("fb-test", &self.0))
                }
            }
            let filter = cx.new(|cx| {
                FilterBarState::new(
                    vec![
                        FilterFieldDef::new("label.virtual_key", "virtual_key").operators([
                            FilterOperator::Eq,
                            FilterOperator::OneOf,
                            FilterOperator::Neq,
                        ]),
                        FilterFieldDef::new("service", "Service"),
                    ],
                    move |_, _, _| {
                        vec![
                            FilterFacetOption::new(uuid_a2.clone(), Some(120)),
                            FilterFacetOption::new(uuid_b2.clone(), Some(42)),
                            FilterFacetOption::new(uuid_c2.clone(), Some(10)),
                        ]
                    },
                    cx,
                )
                .initial_clauses(vec![
                    FilterClause {
                        field: "label.virtual_key".into(),
                        field_label: "virtual_key".into(),
                        operator: FilterOperator::OneOf,
                        values: smallvec::smallvec![uuid_a.clone(), uuid_b.clone()],
                    },
                    FilterClause {
                        field: "service".into(),
                        field_label: "Service".into(),
                        operator: FilterOperator::Eq,
                        values: smallvec::smallvec!["gateway".into()],
                    },
                ])
            });
            cx.subscribe(&filter, move |_, _, _: &FilterBarEvent, _| {
                changes_sub.set(changes_sub.get() + 1);
            })
            .detach();
            Host(filter)
        });
        cx.run_until_parked();

        // Starting an edit and cancelling (close_menu) restores original clause at index 0 with 0 Change events.
        window
            .update(cx, |host, window, cx| {
                host.0.update(cx, |s, cx| {
                    s.edit_clause_value(0, window, cx);
                    assert_eq!(s.clauses().len(), 1);
                    s.close_menu(cx);
                    assert_eq!(s.clauses().len(), 2);
                    assert_eq!(s.clauses()[0].field.as_ref(), "label.virtual_key");
                    assert_eq!(s.clauses()[0].values.len(), 2);
                });
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(changes.get(), 0);

        // Editing value and committing adds the 3rd value in place at index 0 and emits 1 Change event.
        window
            .update(cx, |host, window, cx| {
                host.0.update(cx, |s, cx| {
                    s.edit_clause_value(0, window, cx);
                    s.toggle_multi_value(uuid_c.clone(), cx);
                    s.commit_multi_values(cx);
                    assert_eq!(s.clauses().len(), 2);
                    assert_eq!(s.clauses()[0].field.as_ref(), "label.virtual_key");
                    assert_eq!(s.clauses()[0].values.len(), 3);
                });
            })
            .unwrap();
        cx.run_until_parked();
        assert_eq!(changes.get(), 1);
    }

    #[crate::test]
    fn value_label_formats_chips_and_drafts_without_mutating_wire_values(cx: &mut TestAppContext) {
        cx.update(crate::components::init_components);
        let uuid_a: SharedString = "1d8acbc2-5644-4ade-ad5b-6a7033eee00a".into();
        let uuid_b: SharedString = "700c0e37-d268-4db8-8ffa-83df9e63fb1f".into();
        let names = Rc::new(RefCell::new(std::collections::HashMap::<String, String>::new()));
        let names_fmt = names.clone();
        let names_facets = names.clone();
        let (uuid_a2, uuid_b2) = (uuid_a.clone(), uuid_b.clone());
        let (uuid_a_init, uuid_b_init) = (uuid_a.clone(), uuid_b.clone());

        let window = cx.open_window(size(px(640.), px(420.)), move |_, cx| {
            struct Host(Entity<FilterBarState>);
            impl crate::Render for Host {
                fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                    div().w(px(520.)).child(filter_bar("fb-labels", &self.0))
                }
            }
            let filter = cx.new(|cx| {
                FilterBarState::new(
                    vec![FilterFieldDef::new("label.virtual_key", "virtual_key").operators([
                        FilterOperator::Eq,
                        FilterOperator::OneOf,
                    ])],
                    move |_, _, _| {
                        let map = names_facets.borrow();
                        let opt = |id: &SharedString, n| {
                            let label = map
                                .get(id.as_ref())
                                .cloned()
                                .unwrap_or_else(|| id.to_string());
                            FilterFacetOption::new(id.clone(), Some(n)).label(label)
                        };
                        vec![opt(&uuid_a2, 80), opt(&uuid_b2, 15)]
                    },
                    cx,
                )
                .value_label(move |field, raw| {
                    if field == "label.virtual_key"
                        && let Some(display) = names_fmt.borrow().get(raw)
                    {
                        return display.clone().into();
                    }
                    raw.to_string().into()
                })
                .initial_clauses(vec![FilterClause {
                    field: "label.virtual_key".into(),
                    field_label: "virtual_key".into(),
                    operator: FilterOperator::OneOf,
                    values: smallvec::smallvec![uuid_a_init, uuid_b_init],
                }])
            });
            Host(filter)
        });
        cx.run_until_parked();

        // Populate display names asynchronously after initial render, invalidate, and re-render.
        names
            .borrow_mut()
            .insert(uuid_a.to_string(), "cursor-workstation · Alex Chen".into());
        names
            .borrow_mut()
            .insert(uuid_b.to_string(), "ci-eval-runner · Alex Chen".into());
        window
            .update(cx, |host, window, cx| {
                host.0.update(cx, |s, cx| {
                    s.invalidate_facets(cx);
                    // Raw wire values stay untouched on FilterClause even when display labels are active.
                    assert_eq!(s.clauses()[0].values.as_slice(), &[uuid_a.clone(), uuid_b.clone()]);
                    // Multi-value draft keeps raw values too while rendering display labels.
                    s.edit_clause_value(0, window, cx);
                    let facets = s.matching_facets("label.virtual_key");
                    assert_eq!(facets[0].value, uuid_a);
                    assert_eq!(facets[0].label.as_ref(), "cursor-workstation · Alex Chen");
                    s.close_menu(cx);
                    assert_eq!(s.clauses()[0].values.as_slice(), &[uuid_a.clone(), uuid_b.clone()]);
                });
            })
            .unwrap();
        cx.run_until_parked();
    }
}

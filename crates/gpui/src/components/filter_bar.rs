//! Structured faceted [`FilterBar`] with segmented pill chips, single- & multi-value
//! facet popovers with live counts, and full keyboard grammar.

use std::rc::Rc;
use std::sync::Arc;

use smallvec::SmallVec;

use crate::{
    Animation, AnimationExt, App, ClickEvent, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, FontFeatures, FontWeight, IntoElement, KeyDownEvent, ParentElement,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, anchored, deferred, div,
    ease_out_quint, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ANIM_FAST, ActiveTheme};

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
    /// Formats the selected value(s) compactly for chip display.
    pub fn values_summary(&self) -> String {
        match self.values.len() {
            0 => String::new(),
            1 => self.values[0].to_string(),
            2 => format!("{}, {}", self.values[0], self.values[1]),
            n => format!("{}, +{}", self.values[0], n - 1),
        }
    }

    /// Evaluates whether a candidate field value satisfies this clause.
    pub fn matches_value(&self, candidate: &str) -> bool {
        match self.operator {
            FilterOperator::Eq => self.values.iter().any(|v| v.as_ref() == candidate),
            FilterOperator::Neq => self.values.iter().all(|v| v.as_ref() != candidate),
            FilterOperator::OneOf => self.values.iter().any(|v| v.as_ref() == candidate),
            FilterOperator::NotOneOf => self.values.iter().all(|v| v.as_ref() != candidate),
            FilterOperator::Contains => {
                let lower = candidate.to_lowercase();
                self.values
                    .iter()
                    .any(|v| lower.contains(&v.to_lowercase()))
            }
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

/// Interactive state entity backing a [`FilterBar`].
pub struct FilterBarState {
    fields: Vec<FilterFieldDef>,
    clauses: Vec<FilterClause>,
    draft: FilterDraft,
    query: String,
    open: bool,
    highlighted: usize,
    placeholder: SharedString,
    facet_provider: FacetProvider,
    focus_handle: FocusHandle,
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
            draft: FilterDraft::Field,
            query: String::new(),
            open: false,
            highlighted: 0,
            placeholder: "Filter by field, operator, value...".into(),
            facet_provider: Rc::new(facet_provider),
            focus_handle: cx.focus_handle(),
        }
    }

    /// Sets the placeholder text shown when no filters are active.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
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
        cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        cx.notify();
    }

    /// Removes the clause at `index`.
    pub fn remove_clause(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.clauses.len() {
            self.clauses.remove(index);
            cx.emit(FilterBarEvent::Change(self.clauses.clone()));
            cx.notify();
        }
    }

    /// Clears all committed clauses and any in-progress draft.
    pub fn clear_all(&mut self, cx: &mut Context<Self>) {
        let changed = !self.clauses.is_empty();
        self.clauses.clear();
        self.draft = FilterDraft::Field;
        self.query.clear();
        self.highlighted = 0;
        if changed {
            cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        }
        cx.notify();
    }

    fn open_menu(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        self.open = true;
        self.highlighted = 0;
        window.focus(&self.focus_handle, cx);
        cx.notify();
    }

    fn close_menu(&mut self, cx: &mut Context<Self>) {
        if self.open {
            self.open = false;
            cx.notify();
        }
    }

    fn edit_clause_operator(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.clauses.len() {
            return;
        }
        let clause = self.clauses.remove(index);
        self.draft = FilterDraft::Operator {
            field: clause.field,
            field_label: clause.field_label,
        };
        self.query.clear();
        self.highlighted = 0;
        self.open = true;
        window.focus(&self.focus_handle, cx);
        cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        cx.notify();
    }

    fn edit_clause_value(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index >= self.clauses.len() {
            return;
        }
        let clause = self.clauses.remove(index);
        self.draft = if clause.operator.is_multi() {
            FilterDraft::MultiValue {
                field: clause.field,
                field_label: clause.field_label,
                operator: clause.operator,
                pending: clause.values,
            }
        } else {
            FilterDraft::SingleValue {
                field: clause.field,
                field_label: clause.field_label,
                operator: clause.operator,
            }
        };
        self.query.clear();
        self.highlighted = 0;
        self.open = true;
        window.focus(&self.focus_handle, cx);
        cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        cx.notify();
    }

    fn matching_fields(&self) -> Vec<FilterFieldDef> {
        let q = self.query.trim().to_lowercase();
        self.fields
            .iter()
            .filter(|f| {
                q.is_empty()
                    || f.label.to_lowercase().contains(&q)
                    || f.key.to_lowercase().contains(&q)
            })
            .cloned()
            .collect()
    }

    fn matching_operators(&self, field_key: &str) -> Vec<FilterOperator> {
        let ops = self
            .fields
            .iter()
            .find(|f| f.key.as_ref() == field_key)
            .map(|f| f.operators.clone())
            .unwrap_or_else(|| vec![FilterOperator::Eq, FilterOperator::OneOf]);
        let q = self.query.trim().to_lowercase();
        ops.into_iter()
            .filter(|op| q.is_empty() || op.label().to_lowercase().contains(&q))
            .collect()
    }

    fn matching_facets(&self, field_key: &str) -> Vec<FilterFacetOption> {
        (self.facet_provider)(&self.clauses, field_key, self.query.trim())
    }

    fn choose_field(&mut self, field: &FilterFieldDef, cx: &mut Context<Self>) {
        self.query.clear();
        self.highlighted = 0;
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
        cx.notify();
    }

    fn choose_operator(&mut self, op: FilterOperator, cx: &mut Context<Self>) {
        let (field, field_label) = match &self.draft {
            FilterDraft::Operator { field, field_label } => (field.clone(), field_label.clone()),
            _ => return,
        };
        self.query.clear();
        self.highlighted = 0;
        self.draft = if op.is_multi() {
            FilterDraft::MultiValue {
                field,
                field_label,
                operator: op,
                pending: SmallVec::new(),
            }
        } else {
            FilterDraft::SingleValue {
                field,
                field_label,
                operator: op,
            }
        };
        self.open = true;
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
        self.clauses.push(FilterClause {
            field,
            field_label,
            operator,
            values: smallvec::smallvec![value],
        });
        self.draft = FilterDraft::Field;
        self.query.clear();
        self.highlighted = 0;
        cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        cx.notify();
    }

    fn toggle_multi_value(&mut self, value: SharedString, cx: &mut Context<Self>) {
        if let FilterDraft::MultiValue { pending, .. } = &mut self.draft {
            if let Some(pos) = pending.iter().position(|v| v == &value) {
                pending.remove(pos);
            } else {
                pending.push(value);
            }
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
        self.clauses.push(FilterClause {
            field,
            field_label,
            operator,
            values: pending,
        });
        self.draft = FilterDraft::Field;
        self.query.clear();
        self.highlighted = 0;
        cx.emit(FilterBarEvent::Change(self.clauses.clone()));
        cx.notify();
    }

    fn step_back(&mut self, cx: &mut Context<Self>) {
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
                cx.notify();
            }
            FilterDraft::Field => {
                if !self.clauses.is_empty() {
                    self.clauses.pop();
                    cx.emit(FilterBarEvent::Change(self.clauses.clone()));
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

    fn handle_key_down(&mut self, event: &KeyDownEvent, cx: &mut Context<Self>) {
        let key = event.keystroke.key.as_str();
        match key {
            "escape" => {
                self.open = false;
                self.draft = FilterDraft::Field;
                self.query.clear();
                self.highlighted = 0;
                cx.notify();
            }
            "down" => {
                self.open = true;
                let count = self.current_option_count();
                if count > 0 {
                    self.highlighted = (self.highlighted + 1) % count;
                }
                cx.notify();
            }
            "up" => {
                self.open = true;
                let count = self.current_option_count();
                if count > 0 {
                    self.highlighted = (self.highlighted + count - 1) % count;
                }
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
            "enter" | "tab" => match &self.draft {
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
            },
            _ => {
                if !event.keystroke.modifiers.control
                    && !event.keystroke.modifiers.platform
                    && let Some(ch) = &event.keystroke.key_char
                    && !ch.chars().any(|c| c.is_control())
                {
                    self.open = true;
                    self.query.push_str(ch);
                    self.highlighted = 0;
                    cx.notify();
                }
            }
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

impl FilterBar {
    /// Creates a new [`FilterBar`] bound to `state`.
    pub fn new(id: impl Into<ElementId>, state: &Entity<FilterBarState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            style: StyleRefinement::default(),
        }
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
        let tnum = FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]));

        let snap = self.state.read(cx);
        let clauses = snap.clauses.clone();
        let draft = snap.draft.clone();
        let query = snap.query.clone();
        let open = snap.open;
        let highlighted = snap.highlighted;
        let placeholder = snap.placeholder.clone();
        let focus_handle = snap.focus_handle.clone();
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
        let facet_options = if open {
            match &draft {
                FilterDraft::SingleValue { field, .. } | FilterDraft::MultiValue { field, .. } => {
                    snap.matching_facets(field.as_ref())
                }
                _ => Vec::new(),
            }
        } else {
            Vec::new()
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

        // 1. Committed filter chips: [ Provider | = | anthropic | × ]
        for (idx, clause) in clauses.iter().enumerate() {
            let state_op = self.state.clone();
            let state_val = self.state.clone();
            let state_rm = self.state.clone();

            let chip = div()
                .id((self.id.clone(), format!("clause-{idx}")))
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
                        .px(px(7.))
                        .h_full()
                        .flex()
                        .items_center()
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(clause.field_label.clone()),
                )
                .child(div().w(px(1.)).h_full().bg(theme.border))
                .child(
                    div()
                        .id((self.id.clone(), format!("clause-op-{idx}")))
                        .px(px(7.))
                        .h_full()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .text_color(theme.muted_foreground)
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_: &ClickEvent, window, cx| {
                            state_op.update(cx, |s, cx| s.edit_clause_operator(idx, window, cx));
                        })
                        .child(clause.operator.label()),
                )
                .child(div().w(px(1.)).h_full().bg(theme.border))
                .child(
                    div()
                        .id((self.id.clone(), format!("clause-val-{idx}")))
                        .px(px(7.))
                        .h_full()
                        .flex()
                        .items_center()
                        .cursor_pointer()
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.foreground)
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_: &ClickEvent, window, cx| {
                            state_val.update(cx, |s, cx| s.edit_clause_value(idx, window, cx));
                        })
                        .child(clause.values_summary()),
                )
                .child(div().w(px(1.)).h_full().bg(theme.border))
                .child(
                    div()
                        .id((self.id.clone(), format!("clause-rm-{idx}")))
                        .px(px(5.))
                        .h_full()
                        .flex()
                        .items_center()
                        .justify_center()
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_: &ClickEvent, _, cx| {
                            state_rm.update(cx, |s, cx| s.remove_clause(idx, cx));
                        })
                        .child(
                            Icon::new(LucideIcon::X)
                                .size(IconSize::Xs)
                                .color(theme.muted_foreground),
                        ),
                );
            tokens_row = tokens_row.child(chip);
        }

        // 2. Active draft chip (when mid-construction)
        match &draft {
            FilterDraft::Field => {}
            FilterDraft::Operator { field_label, .. } => {
                tokens_row = tokens_row.child(
                    div()
                        .flex()
                        .items_center()
                        .h(px(26.))
                        .rounded(theme.radius_md)
                        .bg(theme.primary_soft)
                        .border_1()
                        .border_color(theme.ring.opacity(0.45))
                        .text_size(px(12.))
                        .child(
                            div()
                                .px(px(8.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.primary_soft_foreground)
                                .child(field_label.clone()),
                        ),
                );
            }
            FilterDraft::SingleValue {
                field_label,
                operator,
                ..
            } => {
                tokens_row = tokens_row.child(
                    div()
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
                                .px(px(8.))
                                .h_full()
                                .flex()
                                .items_center()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.primary_soft_foreground)
                                .child(field_label.clone()),
                        )
                        .child(div().w(px(1.)).h_full().bg(theme.ring.opacity(0.3)))
                        .child(
                            div()
                                .px(px(8.))
                                .h_full()
                                .flex()
                                .items_center()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.primary_soft_foreground)
                                .child(operator.label()),
                        ),
                );
            }
            FilterDraft::MultiValue {
                field_label,
                operator,
                pending,
                ..
            } => {
                let pending_text = if pending.is_empty() {
                    None
                } else if pending.len() == 1 {
                    Some(pending[0].to_string())
                } else {
                    Some(format!("{}, +{}", pending[0], pending.len() - 1))
                };
                tokens_row = tokens_row.child(
                    div()
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
                                .px(px(8.))
                                .h_full()
                                .flex()
                                .items_center()
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.primary_soft_foreground)
                                .child(field_label.clone()),
                        )
                        .child(div().w(px(1.)).h_full().bg(theme.ring.opacity(0.3)))
                        .child(
                            div()
                                .px(px(8.))
                                .h_full()
                                .flex()
                                .items_center()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.primary_soft_foreground)
                                .child(operator.label()),
                        )
                        .when_some(pending_text, |el, summary| {
                            el.child(div().w(px(1.)).h_full().bg(theme.ring.opacity(0.3)))
                                .child(
                                    div()
                                        .px(px(8.))
                                        .h_full()
                                        .flex()
                                        .items_center()
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(theme.foreground)
                                        .child(summary),
                                )
                        }),
                );
            }
        }

        // 3. Inline query text + caret / placeholder
        let inline_input = if !query.is_empty() {
            div()
                .flex()
                .items_center()
                .text_size(px(13.))
                .text_color(theme.foreground)
                .child(query)
                .when(focused, |el| {
                    el.child(div().w(px(1.5)).h(px(15.)).ml(px(1.)).bg(theme.primary))
                })
        } else if focused {
            div()
                .flex()
                .items_center()
                .gap(px(4.))
                .child(div().w(px(1.5)).h(px(15.)).bg(theme.primary))
                .when(!has_content, |el| {
                    el.child(
                        div()
                            .text_size(px(13.))
                            .text_color(theme.placeholder)
                            .child(placeholder),
                    )
                })
        } else if !has_content {
            div()
                .text_size(px(13.))
                .text_color(theme.placeholder)
                .child(placeholder)
        } else {
            div().w(px(4.))
        };
        tokens_row = tokens_row.child(inline_input);

        let bar = div()
            .id((self.id.clone(), "bar"))
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
            .bg(theme.surface)
            .border_1()
            .border_color(border_color)
            .shadow_xs()
            .cursor_text()
            .child(
                Icon::new(LucideIcon::Funnel)
                    .size(IconSize::Sm)
                    .color(theme.muted_foreground),
            )
            .child(tokens_row)
            .when(has_content, |el| {
                let state = self.state.clone();
                el.child(
                    div()
                        .id((self.id.clone(), "clear-all"))
                        .w(px(22.))
                        .h(px(22.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .rounded(theme.radius_sm)
                        .cursor_pointer()
                        .hover(move |s| s.bg(hover_bg))
                        .on_click(move |_: &ClickEvent, _, cx| {
                            state.update(cx, |s, cx| s.clear_all(cx));
                        })
                        .child(
                            Icon::new(LucideIcon::X)
                                .size(IconSize::Xs)
                                .color(theme.muted_foreground),
                        ),
                )
            });

        let mut root = div().id(self.id.clone()).relative().w_full().flex_shrink_0().child(bar);

        // 4. Anchored Suggestion Popover
        if open {
            let state_out = self.state.clone();
            let mut list = div().flex().flex_col().gap(px(2.));

            let header_hint: Option<&'static str> = match &draft {
                FilterDraft::Field => Some("Filter by field"),
                FilterDraft::Operator { .. } => Some("Choose operator"),
                FilterDraft::SingleValue { .. } => None,
                FilterDraft::MultiValue { .. } => Some("Choose any values, then Enter"),
            };

            if let Some(hint) = header_hint {
                list = list.child(
                    div()
                        .px(px(10.))
                        .pt(px(4.))
                        .pb(px(6.))
                        .text_size(px(11.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(hint),
                );
            }

            match &draft {
                FilterDraft::Field => {
                    for (idx, field) in field_options.iter().enumerate() {
                        let active = idx == highlighted;
                        let field_clone = field.clone();
                        let state = self.state.clone();
                        list = list.child(
                            div()
                                .id((self.id.clone(), format!("opt-field-{idx}")))
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
                                            .text_size(px(11.5))
                                            .text_color(theme.muted_foreground)
                                            .child(hint),
                                    )
                                }),
                        );
                    }
                }
                FilterDraft::Operator { .. } => {
                    for (idx, &op) in operator_options.iter().enumerate() {
                        let active = idx == highlighted;
                        let state = self.state.clone();
                        list = list.child(
                            div()
                                .id((self.id.clone(), format!("opt-op-{idx}")))
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
                }
                FilterDraft::SingleValue { .. } => {
                    if facet_options.is_empty() {
                        list = list.child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child("No matching values"),
                        );
                    }
                    for (idx, opt) in facet_options.iter().enumerate() {
                        let active = idx == highlighted;
                        let val = opt.value.clone();
                        let state = self.state.clone();
                        list = list.child(
                            div()
                                .id((self.id.clone(), format!("opt-val-{idx}")))
                                .px(px(10.))
                                .py(px(7.))
                                .rounded(theme.radius_md)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(16.))
                                .cursor_pointer()
                                .text_size(px(13.))
                                .when(active, |el| el.bg(theme.primary_soft))
                                .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                                .on_click(move |_: &ClickEvent, _, cx| {
                                    state.update(cx, |s, cx| {
                                        s.commit_single_value(val.clone(), cx);
                                    });
                                })
                                .child(
                                    div()
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
                                        .child(opt.label.clone()),
                                )
                                .when_some(opt.count, |el, count| {
                                    el.child(
                                        div()
                                            .text_size(px(12.))
                                            .font_features(tnum.clone())
                                            .text_color(theme.muted_foreground)
                                            .child(format!("{count}")),
                                    )
                                }),
                        );
                    }
                }
                FilterDraft::MultiValue { pending, .. } => {
                    if facet_options.is_empty() {
                        list = list.child(
                            div()
                                .px(px(10.))
                                .py(px(8.))
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child("No matching values"),
                        );
                    }
                    for (idx, opt) in facet_options.iter().enumerate() {
                        let active = idx == highlighted;
                        let checked = pending.iter().any(|v| v == &opt.value);
                        let val = opt.value.clone();
                        let state = self.state.clone();
                        let checkbox_box = div()
                            .w(px(16.))
                            .h(px(16.))
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

                        list = list.child(
                            div()
                                .id((self.id.clone(), format!("opt-mval-{idx}")))
                                .px(px(10.))
                                .py(px(7.))
                                .rounded(theme.radius_md)
                                .flex()
                                .items_center()
                                .justify_between()
                                .gap(px(16.))
                                .cursor_pointer()
                                .text_size(px(13.))
                                .when(active, |el| el.bg(theme.primary_soft))
                                .when(!active, |el| el.hover(move |s| s.bg(hover_bg)))
                                .on_click(move |_: &ClickEvent, _, cx| {
                                    state.update(cx, |s, cx| {
                                        s.toggle_multi_value(val.clone(), cx);
                                    });
                                })
                                .child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .gap(px(8.))
                                        .child(checkbox_box)
                                        .child(
                                            div()
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
                                                .child(opt.label.clone()),
                                        ),
                                )
                                .when_some(opt.count, |el, count| {
                                    el.child(
                                        div()
                                            .text_size(px(12.))
                                            .font_features(tnum.clone())
                                            .text_color(theme.muted_foreground)
                                            .child(format!("{count}")),
                                    )
                                }),
                        );
                    }
                    if !pending.is_empty() {
                        let state_commit = self.state.clone();
                        list = list.child(
                            div()
                                .mt(px(4.))
                                .pt(px(6.))
                                .px(px(6.))
                                .border_t_1()
                                .border_color(theme.border)
                                .flex()
                                .items_center()
                                .justify_between()
                                .child(
                                    div()
                                        .text_size(px(11.5))
                                        .text_color(theme.muted_foreground)
                                        .child(format!("{} selected", pending.len())),
                                )
                                .child(
                                    div()
                                        .id((self.id.clone(), "multi-commit"))
                                        .px(px(10.))
                                        .py(px(4.))
                                        .rounded(theme.radius_md)
                                        .bg(theme.primary)
                                        .text_size(px(12.))
                                        .font_weight(FontWeight::MEDIUM)
                                        .text_color(theme.primary_foreground)
                                        .cursor_pointer()
                                        .on_click(move |_: &ClickEvent, _, cx| {
                                            state_commit
                                                .update(cx, |s, cx| s.commit_multi_values(cx));
                                        })
                                        .child("Apply (Enter)"),
                                ),
                        );
                    }
                }
            }

            let popover = div()
                .id((self.id.clone(), "filter-popover"))
                .min_w(px(260.))
                .max_w(px(340.))
                .mt(px(6.))
                .p(px(6.))
                .rounded(theme.radius_xl)
                .bg(theme.popover)
                .border_1()
                .border_color(theme.border)
                .shadow_lg()
                .on_mouse_down_out(move |_, _, cx| {
                    state_out.update(cx, |s, cx| s.close_menu(cx));
                })
                .child(list)
                .with_animation(
                    (self.id.clone(), "filter-pop-enter"),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta).mt(px(2. + (1. - delta) * 6.)),
                );

            root = root.child(
                deferred(
                    anchored()
                        .snap_to_window_with_margin(px(8.))
                        .child(popover),
                )
                .with_priority(2),
            );
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

//! Lightweight declarative [`FormState`] & [`FormRule`] validation controller.
//!
//! Tracks field values, validation rules (`Required`, `MinLength`, `MaxLength`, `Email`,
//! `NumericRange`, `Matches`, `Custom`, `CustomWithValues`), per-field errors, touched/dirty
//! state, server errors, and reset — pairing seamlessly with [`super::Field`], [`super::Input`],
//! and [`super::Button`].

use std::collections::{HashMap, HashSet};
use std::fmt;
use std::rc::Rc;

use crate::SharedString;

/// Validates an email address with strict hostname/local-part checks (no whitespace, single `@`,
/// non-empty labels, no leading/trailing/consecutive dots, TLD >= 2 ASCII letters).
fn is_valid_email(v: &str) -> bool {
    if v.is_empty() || v.chars().any(|c| c.is_whitespace() || c.is_control()) {
        return false;
    }
    let Some((local, domain)) = v.split_once('@') else {
        return false;
    };
    if local.is_empty()
        || domain.is_empty()
        || domain.contains('@')
        || local.starts_with('.')
        || local.ends_with('.')
        || local.contains("..")
    {
        return false;
    }
    let labels: Vec<&str> = domain.split('.').collect();
    if labels.len() < 2 {
        return false;
    }
    for label in &labels {
        if label.is_empty()
            || label.starts_with('-')
            || label.ends_with('-')
            || !label.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        {
            return false;
        }
    }
    let tld = labels[labels.len() - 1];
    tld.len() >= 2 && tld.bytes().all(|b| b.is_ascii_alphabetic())
}

/// Declarative validation rule evaluated against a field's string value.
#[derive(Clone)]
pub enum FormRule {
    /// Rejects empty or whitespace-only input.
    Required(SharedString),
    /// Rejects non-empty values shorter than `min` Unicode scalar values.
    /// (Pair with [`FormRule::Required`] to also reject empty input.)
    MinLength(usize, SharedString),
    /// Rejects values longer than `max` Unicode scalar values.
    MaxLength(usize, SharedString),
    /// Validates non-empty email shape (`local@domain.tld`).
    /// (Pair with [`FormRule::Required`] to also reject empty input.)
    Email(SharedString),
    /// Parses a finite `f64` from non-empty input and verifies it falls within `min..=max`.
    /// (Pair with [`FormRule::Required`] to also reject empty input.)
    NumericRange(f64, f64, SharedString),
    /// Verifies this field equals another field's current value (e.g. password confirmation).
    Matches(SharedString, SharedString),
    /// Arbitrary custom validator returning `Some(error_message)` on failure.
    Custom(Rc<dyn Fn(&str) -> Option<SharedString>>),
    /// Cross-field custom validator receiving `(field_value, all_form_values)`.
    CustomWithValues(Rc<dyn Fn(&str, &HashMap<SharedString, SharedString>) -> Option<SharedString>>),
}

impl fmt::Debug for FormRule {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Required(m) => f.debug_tuple("Required").field(m).finish(),
            Self::MinLength(n, m) => f.debug_tuple("MinLength").field(n).field(m).finish(),
            Self::MaxLength(n, m) => f.debug_tuple("MaxLength").field(n).field(m).finish(),
            Self::Email(m) => f.debug_tuple("Email").field(m).finish(),
            Self::NumericRange(a, b, m) => f
                .debug_tuple("NumericRange")
                .field(a)
                .field(b)
                .field(m)
                .finish(),
            Self::Matches(other, m) => f.debug_tuple("Matches").field(other).field(m).finish(),
            Self::Custom(_) => f.write_str("Custom(<fn>)"),
            Self::CustomWithValues(_) => f.write_str("CustomWithValues(<fn>)"),
        }
    }
}

impl FormRule {
    /// Required rule with a default or custom message.
    pub fn required(message: impl Into<SharedString>) -> Self {
        Self::Required(message.into())
    }

    /// Minimum character length rule.
    pub fn min_length(min: usize, message: impl Into<SharedString>) -> Self {
        Self::MinLength(min, message.into())
    }

    /// Maximum character length rule.
    pub fn max_length(max: usize, message: impl Into<SharedString>) -> Self {
        Self::MaxLength(max, message.into())
    }

    /// Strict email format rule.
    pub fn email(message: impl Into<SharedString>) -> Self {
        Self::Email(message.into())
    }

    /// Inclusive numeric range rule (automatically normalizes reversed `min > max` bounds).
    pub fn numeric_range(min: f64, max: f64, message: impl Into<SharedString>) -> Self {
        let (lo, hi) = if min.is_finite() && max.is_finite() {
            (min.min(max), min.max(max))
        } else {
            (f64::NEG_INFINITY, f64::INFINITY)
        };
        Self::NumericRange(lo, hi, message.into())
    }

    /// Cross-field equality rule (e.g. `"password"`).
    pub fn matches(other_field: impl Into<SharedString>, message: impl Into<SharedString>) -> Self {
        Self::Matches(other_field.into(), message.into())
    }

    /// Custom closure validator.
    pub fn custom(f: impl Fn(&str) -> Option<SharedString> + 'static) -> Self {
        Self::Custom(Rc::new(f))
    }

    /// Cross-field custom closure validator receiving `(value, all_values)`.
    pub fn custom_with_values(
        f: impl Fn(&str, &HashMap<SharedString, SharedString>) -> Option<SharedString> + 'static,
    ) -> Self {
        Self::CustomWithValues(Rc::new(f))
    }

    fn check(
        &self,
        value: &str,
        all_values: &HashMap<SharedString, SharedString>,
    ) -> Option<SharedString> {
        match self {
            Self::Required(msg) => value.trim().is_empty().then(|| msg.clone()),
            Self::MinLength(min, msg) => {
                let trimmed = value.trim();
                (!trimmed.is_empty() && trimmed.chars().count() < *min).then(|| msg.clone())
            }
            Self::MaxLength(max, msg) => (value.chars().count() > *max).then(|| msg.clone()),
            Self::Email(msg) => {
                let v = value.trim();
                if v.is_empty() {
                    return None;
                }
                (!is_valid_email(v)).then(|| msg.clone())
            }
            Self::NumericRange(min, max, msg) => {
                let v = value.trim();
                if v.is_empty() {
                    return None;
                }
                let (lo, hi) = if min.is_finite() && max.is_finite() {
                    (min.min(*max), min.max(*max))
                } else {
                    return Some(msg.clone());
                };
                let valid = v
                    .parse::<f64>()
                    .is_ok_and(|n| n.is_finite() && n >= lo && n <= hi);
                (!valid).then(|| msg.clone())
            }
            Self::Matches(other, msg) => {
                let other_val = all_values.get(other).map(|s| s.as_ref()).unwrap_or("");
                (value != other_val).then(|| msg.clone())
            }
            Self::Custom(f) => f(value),
            Self::CustomWithValues(f) => f(value, all_values),
        }
    }
}

/// Stateful form validation controller tracking field values, rules, dirty/touched flags,
/// and per-field error messages.
#[derive(Clone, Default, Debug)]
pub struct FormState {
    initial_values: HashMap<SharedString, SharedString>,
    values: HashMap<SharedString, SharedString>,
    rules: HashMap<SharedString, Vec<FormRule>>,
    errors: HashMap<SharedString, SharedString>,
    touched: HashSet<SharedString>,
    submitted: bool,
}

impl FormState {
    /// Creates an empty [`FormState`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Registers a field with an initial value and ordered list of [`FormRule`]s.
    pub fn field(
        mut self,
        name: impl Into<SharedString>,
        initial_value: impl Into<SharedString>,
        rules: Vec<FormRule>,
    ) -> Self {
        let name: SharedString = name.into();
        let val: SharedString = initial_value.into();
        self.initial_values.insert(name.clone(), val.clone());
        self.values.insert(name.clone(), val);
        self.rules.insert(name, rules);
        self
    }

    /// Updates a field's value, marks it touched, and re-validates it plus any transitive dependents.
    pub fn set_value(
        &mut self,
        name: impl Into<SharedString>,
        value: impl Into<SharedString>,
    ) {
        let name: SharedString = name.into();
        let val: SharedString = value.into();
        if self.values.get(&name) == Some(&val) && self.touched.contains(&name) {
            return;
        }
        self.values.insert(name.clone(), val);
        self.touched.insert(name.clone());
        self.validate_field(&name);

        // Transitively re-validate any sibling fields with `Matches` or `CustomWithValues` rules
        // when those dependents are touched, submitted, or already non-empty.
        let mut queue = vec![name.clone()];
        let mut visited = HashSet::new();
        visited.insert(name);
        while let Some(changed_key) = queue.pop() {
            let deps: Vec<SharedString> = self
                .rules
                .iter()
                .filter(|(k, rules)| {
                    !visited.contains(*k)
                        && (self.submitted
                            || self.touched.contains(*k)
                            || !self.value(k).is_empty())
                        && rules.iter().any(|r| {
                            matches!(r, FormRule::Matches(other, _) if other == &changed_key)
                                || matches!(r, FormRule::CustomWithValues(_))
                        })
                })
                .map(|(k, _)| k.clone())
                .collect();
            for dep in deps {
                visited.insert(dep.clone());
                self.validate_field(&dep);
                queue.push(dep);
            }
        }
    }

    /// Marks a field as touched (e.g. on blur) and validates it immediately.
    pub fn touch(&mut self, name: impl Into<SharedString>) -> bool {
        let name: SharedString = name.into();
        self.touched.insert(name.clone());
        self.validate_field(&name)
    }

    /// Returns whether `name` has been touched by user input or submission.
    pub fn is_touched(&self, name: &str) -> bool {
        self.touched.contains(name)
    }

    /// Sets an explicit field error (useful for server-side validation responses).
    pub fn set_error(
        &mut self,
        name: impl Into<SharedString>,
        message: impl Into<SharedString>,
    ) {
        self.errors.insert(name.into(), message.into());
    }

    /// Clears a field's active error message.
    pub fn clear_error(&mut self, name: &str) {
        self.errors.remove(name);
    }

    /// Returns the current value for `name` (or `""` if unregistered).
    pub fn value(&self, name: &str) -> &str {
        self.values.get(name).map(|s| s.as_ref()).unwrap_or("")
    }

    /// Returns the active validation error message for `name`, if any.
    pub fn error(&self, name: &str) -> Option<SharedString> {
        self.errors.get(name).cloned()
    }

    /// Iterates over all currently recorded `(field_name, error_message)` pairs.
    pub fn errors(&self) -> impl Iterator<Item = (&SharedString, &SharedString)> {
        self.errors.iter()
    }

    /// Returns whether there are any currently displayed validation errors.
    pub fn has_errors(&self) -> bool {
        !self.errors.is_empty()
    }

    /// Returns whether `name` has diverged from its initial value.
    pub fn is_field_dirty(&self, name: &str) -> bool {
        let init = self
            .initial_values
            .get(name)
            .map(|s| s.as_ref())
            .unwrap_or("");
        self.value(name) != init
    }

    /// Returns whether any field has diverged from its initial value.
    pub fn is_dirty(&self) -> bool {
        self.initial_values
            .iter()
            .any(|(k, init)| self.values.get(k).map(|s| s.as_ref()) != Some(init.as_ref()))
            || self.values.iter().any(|(k, v)| {
                !self.initial_values.contains_key(k) && !v.is_empty()
            })
    }

    /// Evaluates all registered rules live (without mutating `touched`/`submitted`/`errors`)
    /// and returns `true` iff every rule passes and no manual error is active.
    pub fn is_valid(&self) -> bool {
        if !self.errors.is_empty() {
            return false;
        }
        self.rules.iter().all(|(name, rules)| {
            let val = self.values.get(name).map(|s| s.as_ref()).unwrap_or("");
            rules
                .iter()
                .all(|rule| rule.check(val, &self.values).is_none())
        })
    }

    /// Returns whether `validate_all()` has been invoked at least once since last reset.
    pub fn is_submitted(&self) -> bool {
        self.submitted
    }

    /// Validates a single field and updates its error entry. Returns `true` if valid.
    pub fn validate_field(&mut self, name: &str) -> bool {
        let Some((key_ref, rules)) = self.rules.get_key_value(name) else {
            self.errors.remove(name);
            return true;
        };
        let key_shared = key_ref.clone();
        let val = self.values.get(name).map(|s| s.as_ref()).unwrap_or("");
        let err = rules
            .iter()
            .find_map(|rule| rule.check(val, &self.values));
        if let Some(msg) = err {
            self.errors.insert(key_shared, msg);
            false
        } else {
            self.errors.remove(name);
            true
        }
    }

    /// Validates every registered field, marks the form submitted, and returns `true` iff all pass.
    pub fn validate_all(&mut self) -> bool {
        self.submitted = true;
        let keys: Vec<SharedString> = self.rules.keys().cloned().collect();
        let mut ok = true;
        for key in keys {
            self.touched.insert(key.clone());
            if !self.validate_field(&key) {
                ok = false;
            }
        }
        ok
    }

    /// Promotes current values to be the new clean baseline (e.g. right after a successful save).
    pub fn mark_pristine(&mut self) {
        self.initial_values = self.values.clone();
        self.touched.clear();
        self.submitted = false;
    }

    /// Resets all values to their initial defaults and clears touched/error/submitted flags.
    pub fn reset(&mut self) {
        self.values = self.initial_values.clone();
        self.errors.clear();
        self.touched.clear();
        self.submitted = false;
    }
}

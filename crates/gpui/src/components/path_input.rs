//! Filesystem & remote path autocomplete [`PathInput`] / [`PathInputState`] component.
//!
//! Built for local or remote directory and file path fields with:
//! - Built-in async local filesystem lookup ([`PathSource::local_directories`], [`PathSource::local_fs`])
//! - Custom synchronous or debounced asynchronous providers ([`PathSource::sync`], [`PathSource::async_loader`])
//! - Automatic cancellation of stale in-flight lookups and inline retry on errors
//! - Shell-style `Tab` completion (highlighted match, unique match, or longest common path prefix)
//! - Directory drill-down (accepting a directory keeps focus and immediately suggests its children)
//! - Monospace path rendering, truncation hints, and full keyboard navigation (`Up`/`Down`/`PageUp`/`PageDown`/`Enter`/`Tab`/`Escape`)

use std::cell::Cell;
use std::io;
use std::ops::Range;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::time::Duration;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, AsyncApp, Bounds, Context, Entity,
    EventEmitter, FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton,
    MouseDownEvent, Pixels, Render, RenderOnce, ScrollStrategy, SharedString, StyleRefinement,
    Styled, Subscription, Task, UniformListScrollHandle, WeakEntity, Window, anchored, canvas,
    deferred, div, ease_out_quint, point, prelude::*, px, transparent_black, uniform_list,
};

use super::feedback::{Spinner, SpinnerVariant};
use super::focus::list_step;
use super::input::{Input, InputEvent, InputState};
use super::layout::focus_ring_shadow;
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize};

const DEFAULT_PATH_DEBOUNCE: Duration = Duration::from_millis(200);
const DEFAULT_LOCAL_LIMIT: usize = 50;
const COMPACT_ROW_HEIGHT: f32 = 34.0;
const DETAILED_ROW_HEIGHT: f32 = 46.0;
const PAGE_STEP_SIZE: usize = 6;

/// A single filesystem or remote path suggestion entry inside a [`PathInput`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PathEntry {
    /// Canonical path inserted into the input when accepted (e.g. `"~/projects/app/"`).
    pub path: SharedString,
    /// Primary display label shown in the suggestion row (defaults to `path`).
    pub label: SharedString,
    /// Whether this entry represents a directory (`true`) or a file (`false`).
    pub is_dir: bool,
    /// Optional muted secondary description shown below the label.
    pub description: Option<SharedString>,
    /// Optional leading [`LucideIcon`] override (defaults to `Folder` for dirs, `File` for files).
    pub icon: Option<LucideIcon>,
    /// Optional trailing badge / metadata pill (e.g. `"symlink"`, `"git"`).
    pub badge: Option<SharedString>,
    /// Prevents highlighting or accepting this entry.
    pub disabled: bool,
}

impl PathEntry {
    /// Creates a new [`PathEntry`] with explicit `is_dir` flag.
    pub fn new(path: impl Into<SharedString>, is_dir: bool) -> Self {
        let path: SharedString = path.into();
        Self {
            label: path.clone(),
            path,
            is_dir,
            description: None,
            icon: None,
            badge: None,
            disabled: false,
        }
    }

    /// Creates a directory suggestion entry.
    pub fn dir(path: impl Into<SharedString>) -> Self {
        Self::new(path, true)
    }

    /// Creates a file suggestion entry.
    pub fn file(path: impl Into<SharedString>) -> Self {
        Self::new(path, false)
    }

    /// Overrides the displayed label while keeping the canonical `path`.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = label.into();
        self
    }

    /// Sets a muted description line shown below the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Overrides the leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets a trailing metadata badge pill.
    pub fn badge(mut self, badge: impl Into<SharedString>) -> Self {
        self.badge = Some(badge.into());
        self
    }

    /// Marks this entry disabled so it cannot be selected.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }
}

/// Result batch returned by a [`PathSource`] lookup.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct PathSuggestions {
    /// Matching path entries in display order.
    pub entries: Vec<PathEntry>,
    /// Set to `true` when additional matches were omitted due to a result cap.
    pub truncated: bool,
}

impl PathSuggestions {
    /// Creates a non-truncated batch of path entries.
    pub fn new(entries: Vec<PathEntry>) -> Self {
        Self {
            entries,
            truncated: false,
        }
    }

    /// Creates an empty suggestion batch.
    pub fn empty() -> Self {
        Self::default()
    }

    /// Marks whether results were truncated.
    pub fn truncated(mut self, truncated: bool) -> Self {
        self.truncated = truncated;
        self
    }
}

impl From<Vec<PathEntry>> for PathSuggestions {
    fn from(entries: Vec<PathEntry>) -> Self {
        Self::new(entries)
    }
}

type SyncPathLoader = Rc<dyn Fn(&str, &App) -> PathSuggestions>;
type AsyncPathLoader = Rc<dyn Fn(&str, &App) -> Task<Result<PathSuggestions, SharedString>>>;
type PathQueryFilter = Rc<dyn Fn(&str) -> bool>;

#[derive(Clone)]
enum PathSourceKind {
    Sync(SyncPathLoader),
    Async(AsyncPathLoader),
}

/// Data source powering path suggestions for [`PathInputState`].
#[derive(Clone)]
pub struct PathSource {
    kind: PathSourceKind,
    debounce: Duration,
}

impl PathSource {
    /// Built-in async local filesystem provider returning directories only (capped at 50 entries).
    pub fn local_directories() -> Self {
        Self::local_with_limit(true, DEFAULT_LOCAL_LIMIT)
    }

    /// Built-in async local filesystem provider returning directories and files (capped at 50 entries).
    pub fn local_fs() -> Self {
        Self::local_with_limit(false, DEFAULT_LOCAL_LIMIT)
    }

    /// Built-in async local filesystem provider with configurable directory-only filter and limit.
    pub fn local_with_limit(dirs_only: bool, max_results: usize) -> Self {
        let cap = max_results.max(1);
        Self::async_loader(move |prefix, cx| {
            let owned = prefix.to_owned();
            cx.background_spawn(async move { local_fs_lookup(&owned, dirs_only, cap) })
        })
        .debounce(Duration::from_millis(80))
    }

    /// Synchronous path provider evaluated immediately on every edit.
    pub fn sync(loader: impl Fn(&str, &App) -> PathSuggestions + 'static) -> Self {
        Self {
            kind: PathSourceKind::Sync(Rc::new(loader)),
            debounce: Duration::ZERO,
        }
    }

    /// Asynchronous fallible path provider returning a GPUI [`Task`].
    ///
    /// Defaults to a `200ms` debounce window; in-flight lookups are automatically cancelled and
    /// stale responses discarded whenever the path changes or the popover closes.
    pub fn async_loader(
        loader: impl Fn(&str, &App) -> Task<Result<PathSuggestions, SharedString>> + 'static,
    ) -> Self {
        Self {
            kind: PathSourceKind::Async(Rc::new(loader)),
            debounce: DEFAULT_PATH_DEBOUNCE,
        }
    }

    /// Asynchronous infallible convenience provider returning `Task<PathSuggestions>`.
    pub fn async_fn(loader: impl Fn(&str, &App) -> Task<PathSuggestions> + 'static) -> Self {
        Self::async_loader(move |prefix, cx| {
            let task = loader(prefix, cx);
            cx.foreground_executor()
                .spawn(async move { Ok(task.await) })
        })
    }

    /// Sets the debounce duration before invoking an async loader (default `200ms`).
    pub fn debounce(mut self, duration: Duration) -> Self {
        self.debounce = duration;
        self
    }
}

/// Computes the longest shared UTF-8 prefix across all `paths` (used for shell-style `Tab` completion).
pub fn longest_common_path_prefix(paths: &[&str]) -> String {
    let Some((first, rest)) = paths.split_first() else {
        return String::new();
    };
    let mut len = first.len();
    for other in rest {
        let common: usize = first
            .chars()
            .zip(other.chars())
            .take_while(|(a, b)| a == b)
            .map(|(a, _)| a.len_utf8())
            .sum();
        len = len.min(common);
        if len == 0 {
            break;
        }
    }
    first[..len].to_owned()
}

fn user_home_dir() -> Option<PathBuf> {
    #[cfg(windows)]
    {
        std::env::var_os("USERPROFILE")
            .map(PathBuf::from)
            .or_else(|| {
                let drive = std::env::var_os("HOMEDRIVE")?;
                let path = std::env::var_os("HOMEPATH")?;
                let mut full = PathBuf::from(drive);
                full.push(path);
                Some(full)
            })
            .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
    }
    #[cfg(not(windows))]
    {
        std::env::var_os("HOME").map(PathBuf::from)
    }
}

fn local_fs_lookup(
    prefix: &str,
    dirs_only: bool,
    max_results: usize,
) -> Result<PathSuggestions, SharedString> {
    let sep = if cfg!(windows) && prefix.contains('\\') && !prefix.contains('/') {
        '\\'
    } else {
        '/'
    };

    let (display_parent, fs_parent, name_prefix) = resolve_local_prefix(prefix, sep)?;
    let read_dir = match std::fs::read_dir(&fs_parent) {
        Ok(iter) => iter,
        Err(err) if matches!(err.kind(), io::ErrorKind::NotFound | io::ErrorKind::NotADirectory) => {
            return Ok(PathSuggestions::empty());
        }
        Err(err) if err.kind() == io::ErrorKind::PermissionDenied => {
            return Err(SharedString::new_static(
                "Permission denied while reading this directory.",
            ));
        }
        Err(err) => {
            return Err(SharedString::from(format!(
                "Could not read directory: {err}"
            )));
        }
    };

    let show_hidden = name_prefix.starts_with('.');
    let name_prefix_lower = if cfg!(windows) {
        Some(name_prefix.to_lowercase())
    } else {
        None
    };

    let mut matches: Vec<(bool, String, PathEntry)> = Vec::new();
    for entry in read_dir.flatten() {
        let Ok(file_name) = entry.file_name().into_string() else {
            continue;
        };
        if !show_hidden && file_name.starts_with('.') {
            continue;
        }
        let matches_prefix = if let Some(ref lower_prefix) = name_prefix_lower {
            file_name.to_lowercase().starts_with(lower_prefix)
        } else {
            file_name.starts_with(name_prefix.as_str())
        };
        if !matches_prefix {
            continue;
        }
        let is_dir = entry
            .file_type()
            .map(|ft| {
                if ft.is_symlink() {
                    std::fs::metadata(entry.path()).is_ok_and(|m| m.is_dir())
                } else {
                    ft.is_dir()
                }
            })
            .unwrap_or(false);
        if dirs_only && !is_dir {
            continue;
        }
        let full = if is_dir {
            format!("{display_parent}{file_name}{sep}")
        } else {
            format!("{display_parent}{file_name}")
        };
        let sort_key = file_name.to_lowercase();
        matches.push((is_dir, sort_key, PathEntry::new(full, is_dir)));
    }

    matches.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.cmp(&b.1))
            .then_with(|| a.2.path.cmp(&b.2.path))
    });

    let truncated = matches.len() > max_results;
    let entries = matches
        .into_iter()
        .take(max_results)
        .map(|(_, _, entry)| entry)
        .collect();
    Ok(PathSuggestions { entries, truncated })
}

fn resolve_local_prefix(prefix: &str, sep: char) -> Result<(String, PathBuf, String), SharedString> {
    let trimmed = prefix.trim();
    if trimmed.is_empty() || trimmed == "~" {
        let home = user_home_dir().ok_or_else(|| {
            SharedString::new_static("Could not determine the home directory.")
        })?;
        return Ok((format!("~{sep}"), home, String::new()));
    }

    if let Some(rest) = trimmed
        .strip_prefix("~/")
        .or_else(|| trimmed.strip_prefix("~\\"))
    {
        let home = user_home_dir().ok_or_else(|| {
            SharedString::new_static("Could not determine the home directory.")
        })?;
        let split_at = rest.rfind(['/', '\\']).map(|idx| idx + 1).unwrap_or(0);
        let rel_parent = &rest[..split_at];
        let name = &rest[split_at..];
        let fs_parent = if rel_parent.is_empty() {
            home
        } else {
            home.join(rel_parent)
        };
        let display_parent = format!("~{sep}{rel_parent}");
        return Ok((display_parent, fs_parent, name.to_owned()));
    }

    if let Some(idx) = trimmed.rfind(['/', '\\']) {
        let parent_str = &trimmed[..=idx];
        let name = &trimmed[idx + 1..];
        return Ok((parent_str.to_owned(), PathBuf::from(parent_str), name.to_owned()));
    }

    // Relative prefix (e.g. "src"): search relative to current working directory when available.
    if let Ok(cwd) = std::env::current_dir() {
        return Ok((String::new(), cwd, trimmed.to_owned()));
    }
    let fallback = Path::new(".");
    Ok((String::new(), fallback.to_path_buf(), trimmed.to_owned()))
}

/// Events emitted by [`PathInputState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum PathInputEvent {
    /// Emitted whenever the path text changes (from typing, clearing, or accepting a suggestion).
    Change(SharedString),
    /// Emitted when the user accepts a path suggestion (via click, `Enter`, or `Tab`).
    Accept(PathEntry),
    /// Emitted when the user presses `Enter` with no highlighted suggestion.
    Submit(SharedString),
    /// Emitted when the path input gains focus.
    Focus,
    /// Emitted when the path input loses focus.
    Blur,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum HighlightEdge {
    First,
    Last,
}

/// Entity state backing a path-autocompleting [`PathInput`].
pub struct PathInputState {
    focus_handle: FocusHandle,
    input: Entity<InputState>,
    _subscriptions: Vec<Subscription>,
    source: PathSource,
    query_filter: Option<PathQueryFilter>,
    open: bool,
    loading: bool,
    error: Option<SharedString>,
    entries: Rc<Vec<PathEntry>>,
    truncated: bool,
    has_descriptions: bool,
    highlighted: Option<usize>,
    pending_highlight_on_load: Option<HighlightEdge>,
    scroll_handle: UniformListScrollHandle,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    generation: u64,
    pending_task: Option<Task<()>>,
    placeholder: SharedString,
    leading_icon: Option<LucideIcon>,
    size: ComponentSize,
    clearable: bool,
    disabled: bool,
    read_only: bool,
    invalid: bool,
    monospace: bool,
    open_on_focus: bool,
    accept_on_tab: bool,
    continue_on_dir: bool,
    max_menu_height: Pixels,
    loading_message: SharedString,
    empty_message: Option<SharedString>,
    truncated_message: SharedString,
    hint_message: Option<SharedString>,
    frame_style: StyleRefinement,
}

impl EventEmitter<PathInputEvent> for PathInputState {}

impl Focusable for PathInputState {
    fn focus_handle(&self, cx: &App) -> FocusHandle {
        if !self.disabled {
            self.input.read(cx).focus_handle(cx)
        } else {
            self.focus_handle.clone()
        }
    }
}

impl PathInputState {
    /// Creates a new [`PathInputState`] powered by `source`.
    pub fn new(source: PathSource, cx: &mut Context<Self>) -> Self {
        let input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder("~/projects/my-app")
                .size(ComponentSize::Md)
                .content_px(px(0.))
        });
        let sub = cx.subscribe(&input, |this, _input, event: &InputEvent, cx| {
            if this.disabled {
                return;
            }
            match event {
                InputEvent::Change(val) => {
                    let val_clone = val.clone();
                    cx.emit(PathInputEvent::Change(val_clone));
                    if this.can_edit() {
                        this.refresh_suggestions(true, cx);
                    }
                    cx.notify();
                }
                InputEvent::Submit(val) => {
                    if this.is_visible_popover()
                        && let Some(entry) = this.highlighted_entry().cloned()
                    {
                        this.accept_entry(entry, cx);
                        return;
                    }
                    this.close_suggestions(cx);
                    cx.emit(PathInputEvent::Submit(val.clone()));
                }
                InputEvent::Focus => {
                    if this.can_edit() && this.open_on_focus {
                        this.refresh_suggestions(true, cx);
                    }
                    cx.emit(PathInputEvent::Focus);
                    cx.notify();
                }
                InputEvent::Blur => {
                    this.close_suggestions(cx);
                    cx.emit(PathInputEvent::Blur);
                    cx.notify();
                }
                InputEvent::AcceptSuggestion(_) | InputEvent::CommittedChange(_) => {}
            }
        });

        Self {
            focus_handle: cx.focus_handle(),
            input,
            _subscriptions: vec![sub],
            source,
            query_filter: None,
            open: false,
            loading: false,
            error: None,
            entries: Rc::new(Vec::new()),
            truncated: false,
            has_descriptions: false,
            highlighted: None,
            pending_highlight_on_load: None,
            scroll_handle: UniformListScrollHandle::new(),
            trigger_bounds: Rc::new(Cell::new(None)),
            generation: 0,
            pending_task: None,
            placeholder: SharedString::new_static("~/projects/my-app"),
            leading_icon: Some(LucideIcon::Folder),
            size: ComponentSize::Md,
            clearable: false,
            disabled: false,
            read_only: false,
            invalid: false,
            monospace: true,
            open_on_focus: true,
            accept_on_tab: true,
            continue_on_dir: true,
            max_menu_height: px(260.),
            loading_message: SharedString::new_static("Looking up paths…"),
            empty_message: Some(SharedString::new_static("No matching paths.")),
            truncated_message: SharedString::new_static(
                "Showing first matches — keep typing to narrow.",
            ),
            hint_message: Some(SharedString::new_static(
                "↑/↓ navigate · Enter or Tab to complete",
            )),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets the initial path value without emitting [`PathInputEvent::Change`].
    pub fn default_value(self, value: impl Into<String>, cx: &mut App) -> Self {
        let value = value.into();
        self.input
            .update(cx, |inp, cx| inp.set_value_silent(value, cx));
        self
    }

    /// Sets placeholder text when empty.
    pub fn placeholder(mut self, placeholder: impl Into<SharedString>) -> Self {
        self.placeholder = placeholder.into();
        self
    }

    /// Sets the leading [`LucideIcon`] inside the frame (defaults to `Some(LucideIcon::Folder)`).
    pub fn leading_icon(mut self, icon: Option<LucideIcon>) -> Self {
        self.leading_icon = icon;
        self
    }

    /// Sets control size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Enables an inline clear (`X`) button when non-empty.
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Prevents editing while still allowing focus and copying.
    pub fn read_only(mut self, read_only: bool) -> Self {
        self.read_only = read_only;
        self
    }

    /// Highlights the input with invalid error styling.
    pub fn invalid(mut self, invalid: bool) -> Self {
        self.invalid = invalid;
        self
    }

    /// Uses the active theme's monospace font for paths (default `true`).
    pub fn monospace(mut self, monospace: bool) -> Self {
        self.monospace = monospace;
        self
    }

    /// Opens suggestions automatically on focus/click (default `true`).
    pub fn open_on_focus(mut self, open: bool) -> Self {
        self.open_on_focus = open;
        self
    }

    /// Enables shell-style `Tab` completion for highlighted, unique, or common-prefix matches
    /// (default `true`).
    pub fn accept_on_tab(mut self, accept: bool) -> Self {
        self.accept_on_tab = accept;
        self
    }

    /// Keeps the popover open and queries child entries after accepting a directory (default `true`).
    pub fn continue_on_dir(mut self, cont: bool) -> Self {
        self.continue_on_dir = cont;
        self
    }

    /// Sets the maximum scrollable height of the suggestion popover.
    pub fn max_menu_height(mut self, height: Pixels) -> Self {
        self.max_menu_height = if height.0.is_finite() {
            height.max(px(80.))
        } else {
            px(260.)
        };
        self
    }

    /// Sets the label shown beside the spinner while an async lookup is in flight.
    pub fn loading_message(mut self, message: impl Into<SharedString>) -> Self {
        self.loading_message = message.into();
        self
    }

    /// Sets the message shown when zero paths match (pass `None` to hide the popover when empty).
    pub fn empty_message(mut self, message: impl Into<Option<SharedString>>) -> Self {
        self.empty_message = message.into();
        self
    }

    /// Sets the footer notice shown when the source sets `PathSuggestions::truncated = true`.
    pub fn truncated_message(mut self, message: impl Into<SharedString>) -> Self {
        self.truncated_message = message.into();
        self
    }

    /// Sets an optional footer hint shown below non-truncated suggestions.
    pub fn hint_message(mut self, message: impl Into<Option<SharedString>>) -> Self {
        self.hint_message = message.into();
        self
    }

    /// Sets an optional predicate deciding whether a typed path should trigger a lookup.
    ///
    /// When the predicate returns `false`, the popover stays closed cleanly without invoking
    /// the [`PathSource`]. Useful for remote POSIX inputs that only query absolute or `~/` paths.
    pub fn query_filter(mut self, filter: impl Fn(&str) -> bool + 'static) -> Self {
        self.query_filter = Some(Rc::new(filter));
        self
    }

    /// Returns the current path text.
    pub fn value<'a>(&self, cx: &'a App) -> &'a str {
        self.input.read(cx).value()
    }

    /// Returns the underlying embedded [`InputState`] entity.
    pub fn input(&self) -> &Entity<InputState> {
        &self.input
    }

    /// Returns `true` if suggestions are currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Returns `true` when the suggestion popover has visible content (items, loading, error,
    /// or an explicit empty message).
    pub fn is_visible_popover(&self) -> bool {
        self.open
            && (self.loading
                || self.error.is_some()
                || !self.entries.is_empty()
                || self.empty_message.is_some())
    }

    /// Returns `true` while an asynchronous lookup is in flight.
    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// Returns `true` if user interaction is disabled.
    pub fn is_disabled(&self) -> bool {
        self.disabled
    }

    /// Returns the last lookup error message, if any.
    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Returns the currently displayed path suggestions.
    pub fn entries(&self) -> &[PathEntry] {
        &self.entries
    }

    /// Returns whether the latest batch reported truncation.
    pub fn truncated(&self) -> bool {
        self.truncated
    }

    /// Returns the currently highlighted entry index, if any.
    pub fn highlighted_index(&self) -> Option<usize> {
        self.highlighted
    }

    fn highlighted_entry(&self) -> Option<&PathEntry> {
        self.highlighted
            .and_then(|idx| self.entries.get(idx))
            .filter(|e| !e.disabled)
    }

    fn can_edit(&self) -> bool {
        !self.disabled && !self.read_only
    }

    /// Programmatically replaces the path value and emits [`PathInputEvent::Change`].
    pub fn set_value(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        let next = value.into();
        self.input
            .update(cx, |inp, cx| inp.set_value_silent(next.clone(), cx));
        cx.emit(PathInputEvent::Change(SharedString::from(next)));
        if self.open && self.can_edit() {
            self.refresh_suggestions(false, cx);
        }
        cx.notify();
    }

    /// Programmatically replaces the path value without emitting [`PathInputEvent::Change`].
    pub fn set_value_silent(&mut self, value: impl Into<String>, cx: &mut Context<Self>) {
        self.input
            .update(cx, |inp, cx| inp.set_value_silent(value, cx));
        if self.open {
            self.close_suggestions(cx);
        } else {
            cx.notify();
        }
    }

    /// Replaces the [`PathSource`] dynamically (for example when switching remote hosts).
    pub fn set_source(&mut self, source: PathSource, cx: &mut Context<Self>) {
        let was_open = self.open;
        self.source = source;
        self.generation = self.generation.wrapping_add(1);
        self.pending_task = None;
        self.loading = false;
        self.error = None;
        self.entries = Rc::new(Vec::new());
        self.truncated = false;
        self.has_descriptions = false;
        self.highlighted = None;
        if was_open && self.can_edit() {
            self.refresh_suggestions(true, cx);
        }
        cx.notify();
    }

    /// Updates the disabled state dynamically, closing suggestions if disabling.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled == disabled {
            return;
        }
        self.disabled = disabled;
        if disabled {
            self.close_suggestions(cx);
        }
        self.input
            .update(cx, |inp, cx| inp.set_disabled(disabled, cx));
        cx.notify();
    }

    /// Updates the invalid error state dynamically.
    pub fn set_invalid(&mut self, invalid: bool, cx: &mut Context<Self>) {
        if self.invalid != invalid {
            self.invalid = invalid;
            cx.notify();
        }
    }

    /// Updates the placeholder text dynamically.
    pub fn set_placeholder(
        &mut self,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) {
        let placeholder = placeholder.into();
        if self.placeholder != placeholder {
            self.placeholder = placeholder;
            cx.notify();
        }
    }

    /// Programmatically opens suggestions and triggers a lookup for the current path.
    pub fn open_suggestions(&mut self, cx: &mut Context<Self>) {
        if self.can_edit() {
            self.refresh_suggestions(true, cx);
            cx.notify();
        }
    }

    /// Closes the suggestion popover and cancels any in-flight lookup.
    pub fn close_suggestions(&mut self, cx: &mut Context<Self>) {
        self.generation = self.generation.wrapping_add(1);
        self.pending_task = None;
        let had_state = self.open
            || self.loading
            || self.error.is_some()
            || !self.entries.is_empty()
            || self.highlighted.is_some();
        self.open = false;
        self.loading = false;
        self.error = None;
        self.highlighted = None;
        self.pending_highlight_on_load = None;
        self.entries = Rc::new(Vec::new());
        self.truncated = false;
        self.has_descriptions = false;
        if had_state {
            cx.notify();
        }
    }

    /// Retries the lookup for the current path (useful after transient remote/IO failures).
    pub fn retry(&mut self, cx: &mut Context<Self>) {
        if self.can_edit() {
            self.error = None;
            self.refresh_suggestions(true, cx);
            cx.notify();
        }
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.can_edit() || self.input.read(cx).value().is_empty() {
            return;
        }
        self.input
            .update(cx, |inp, cx| inp.set_value_silent(String::new(), cx));
        cx.emit(PathInputEvent::Change(SharedString::new_static("")));
        if self.open || self.open_on_focus {
            self.refresh_suggestions(true, cx);
        }
        cx.notify();
    }

    fn apply_suggestions(&mut self, result: PathSuggestions) {
        self.has_descriptions = result.entries.iter().any(|e| e.description.is_some());
        self.entries = Rc::new(result.entries);
        self.truncated = result.truncated;
        self.loading = false;
        self.error = None;

        self.highlighted = match self.pending_highlight_on_load.take() {
            Some(HighlightEdge::First) => self.entries.iter().position(|e| !e.disabled),
            Some(HighlightEdge::Last) => self.entries.iter().rposition(|e| !e.disabled),
            None => None,
        };

        if let Some(idx) = self.highlighted {
            self.scroll_handle
                .scroll_to_item(idx, ScrollStrategy::Nearest);
        } else {
            self.scroll_handle.scroll_to_item(0, ScrollStrategy::Top);
        }
    }

    fn apply_error(&mut self, err: SharedString) {
        self.loading = false;
        self.error = Some(err);
        self.highlighted = None;
        self.pending_highlight_on_load = None;
        self.entries = Rc::new(Vec::new());
        self.truncated = false;
        self.has_descriptions = false;
    }

    fn refresh_suggestions(&mut self, open_if_eligible: bool, cx: &mut Context<Self>) {
        if !self.can_edit() {
            return;
        }
        let query = self.input.read(cx).value().to_owned();
        if let Some(ref filter) = self.query_filter
            && !filter(&query)
        {
            self.close_suggestions(cx);
            return;
        }
        if !open_if_eligible && !self.open {
            return;
        }

        self.open = true;
        self.generation = self.generation.wrapping_add(1);
        self.pending_task = None;
        let generation = self.generation;

        match &self.source.kind {
            PathSourceKind::Sync(loader) => {
                let loader = Rc::clone(loader);
                let result = loader(&query, &*cx);
                self.apply_suggestions(result);
            }
            PathSourceKind::Async(loader) => {
                let loader = Rc::clone(loader);
                let debounce = self.source.debounce;
                self.loading = true;
                self.error = None;
                self.highlighted = None;

                let task = cx.spawn(async move |weak: WeakEntity<Self>, cx: &mut AsyncApp| {
                    if !debounce.is_zero() {
                        cx.background_executor().timer(debounce).await;
                    }
                    let fetch_task = match weak.update(cx, |this, cx| {
                        if this.generation != generation || !this.open {
                            return None;
                        }
                        Some(loader(&query, cx))
                    }) {
                        Ok(Some(task)) => task,
                        _ => return,
                    };

                    let result = fetch_task.await;
                    let _ = weak.update(cx, |this, cx| {
                        if this.generation != generation || !this.open {
                            return;
                        }
                        this.pending_task = None;
                        match result {
                            Ok(batch) => this.apply_suggestions(batch),
                            Err(err) => this.apply_error(err),
                        }
                        cx.notify();
                    });
                });
                self.pending_task = Some(task);
            }
        }
    }

    fn accept_entry(&mut self, entry: PathEntry, cx: &mut Context<Self>) {
        if !self.can_edit() || entry.disabled {
            return;
        }
        let should_continue = self.continue_on_dir && entry.is_dir;
        let next = entry.path.to_string();
        self.input
            .update(cx, |inp, cx| inp.set_value_silent(next, cx));
        cx.emit(PathInputEvent::Change(entry.path.clone()));
        cx.emit(PathInputEvent::Accept(entry));
        if should_continue {
            self.highlighted = None;
            self.refresh_suggestions(true, cx);
        } else {
            self.close_suggestions(cx);
        }
        cx.notify();
    }

    fn step_highlight(&mut self, key: &str) {
        let enabled: Vec<bool> = self.entries.iter().map(|e| !e.disabled).collect();
        if let Some(next) = list_step(key, self.highlighted, &enabled, PAGE_STEP_SIZE, false) {
            self.highlighted = Some(next);
            self.scroll_handle
                .scroll_to_item(next, ScrollStrategy::Nearest);
        }
    }

    fn try_tab_complete(&mut self, cx: &mut Context<Self>) -> bool {
        if let Some(entry) = self.highlighted_entry().cloned() {
            self.accept_entry(entry, cx);
            return true;
        }
        let enabled: Vec<PathEntry> = self
            .entries
            .iter()
            .filter(|e| !e.disabled)
            .cloned()
            .collect();
        if enabled.len() == 1 {
            self.accept_entry(enabled.into_iter().next().unwrap(), cx);
            return true;
        }
        if enabled.len() > 1 {
            let paths: Vec<&str> = enabled.iter().map(|e| e.path.as_ref()).collect();
            let lcp = longest_common_path_prefix(&paths);
            let current = self.input.read(cx).value();
            let extends = lcp.len() > current.len()
                && (lcp.starts_with(current)
                    || (cfg!(windows) && lcp.to_lowercase().starts_with(&current.to_lowercase())));
            if extends {
                let shared = SharedString::from(lcp.clone());
                self.input
                    .update(cx, |inp, cx| inp.set_value_silent(lcp, cx));
                cx.emit(PathInputEvent::Change(shared));
                self.refresh_suggestions(true, cx);
                cx.notify();
                return true;
            }
        }
        false
    }

    fn on_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.can_edit() {
            return;
        }
        let mods = &event.keystroke.modifiers;
        if mods.control || mods.alt || mods.platform || mods.function {
            return;
        }
        let key = event.keystroke.key.as_str();

        if key == "tab" {
            if self.is_visible_popover() {
                if !mods.shift && self.accept_on_tab && self.try_tab_complete(cx) {
                    window.prevent_default();
                    cx.stop_propagation();
                    return;
                }
                self.close_suggestions(cx);
                return;
            }
            if self.open {
                self.close_suggestions(cx);
            }
            return;
        }

        if mods.shift {
            return;
        }

        match key {
            "down" | "up" => {
                let edge = if key == "down" {
                    HighlightEdge::First
                } else {
                    HighlightEdge::Last
                };
                if !self.open {
                    self.pending_highlight_on_load = Some(edge);
                    self.refresh_suggestions(true, cx);
                    cx.stop_propagation();
                    cx.notify();
                } else if self.loading && self.entries.is_empty() {
                    self.pending_highlight_on_load = Some(edge);
                    cx.stop_propagation();
                    cx.notify();
                } else if !self.entries.is_empty() {
                    self.step_highlight(key);
                    cx.stop_propagation();
                    cx.notify();
                } else if self.is_visible_popover() {
                    cx.stop_propagation();
                }
            }
            "pagedown" | "pageup" if self.is_visible_popover() => {
                if !self.entries.is_empty() {
                    self.step_highlight(key);
                    cx.stop_propagation();
                    cx.notify();
                } else if self.loading {
                    cx.stop_propagation();
                }
            }
            "escape" if self.open => {
                self.close_suggestions(cx);
                window.prevent_default();
                cx.stop_propagation();
            }
            // Fallback when input keybindings aren't installed.
            "enter" if self.is_visible_popover() => {
                if let Some(entry) = self.highlighted_entry().cloned() {
                    self.accept_entry(entry, cx);
                    cx.stop_propagation();
                }
            }
            _ => {}
        }
    }

    fn render_popover(&self, cx: &Context<Self>) -> Option<AnyElement> {
        if !self.is_visible_popover() {
            return None;
        }

        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let entity = cx.entity();
        let entity_id = cx.entity_id();
        let captured_bounds = self.trigger_bounds.get();
        let menu_min_w = captured_bounds
            .map(|b| b.size.width)
            .unwrap_or(px(260.))
            .max(px(220.));
        let mono_font = self
            .monospace
            .then(|| theme.mono_font_family.clone())
            .flatten();

        let mut panel = div()
            .id(("gpui-path-input-popover", entity_id))
            .occlude()
            .flex()
            .flex_col()
            .min_w(menu_min_w)
            .max_w(menu_min_w.max(px(560.)))
            .rounded(theme.radius_lg)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .shadow_lg()
            .overflow_hidden()
            // Keep focus in the embedded Input while clicking suggestions or Retry
            // and never leak mouse-down events to elements underneath.
            .on_any_mouse_down(|_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_mouse_down_out({
                let bounds_cell = self.trigger_bounds.clone();
                let entity_out = entity.clone();
                move |ev: &MouseDownEvent, _, cx| {
                    if bounds_cell
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    entity_out.update(cx, |this, cx| this.close_suggestions(cx));
                }
            });

        if let Some(err) = self.error.clone() {
            let entity_retry = entity.clone();
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(10.))
                    .px(px(12.))
                    .py(px(10.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .flex_1()
                            .min_w_0()
                            .text_size(px(12.5))
                            .text_color(theme.destructive)
                            .child(
                                Icon::new(LucideIcon::TriangleExclamationPoint)
                                    .size(IconSize::Sm)
                                    .color(theme.destructive),
                            )
                            .child(div().flex_1().min_w_0().child(err)),
                    )
                    .child(
                        div()
                            .id(("gpui-path-input-retry", entity_id))
                            .flex_none()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(5.))
                            .px(px(8.))
                            .py(px(4.))
                            .rounded(theme.radius_sm)
                            .border_1()
                            .border_color(theme.input_border)
                            .bg(theme.secondary)
                            .text_size(px(11.5))
                            .font_weight(FontWeight::MEDIUM)
                            .text_color(theme.secondary_foreground)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.secondary_hover))
                            .on_click(move |_, _, cx| {
                                entity_retry.update(cx, |this, cx| this.retry(cx));
                            })
                            .child(
                                Icon::new(LucideIcon::RefreshCw)
                                    .size(IconSize::Xs)
                                    .color(theme.muted_foreground),
                            )
                            .child("Retry"),
                    ),
            );
        } else if self.loading && self.entries.is_empty() {
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
                    .child(self.loading_message.clone()),
            );
        } else if self.entries.is_empty() {
            if let Some(empty) = self.empty_message.clone() {
                panel = panel.child(
                    div()
                        .flex()
                        .items_center()
                        .px(px(12.))
                        .py(px(12.))
                        .text_size(px(12.5))
                        .text_color(theme.muted_foreground)
                        .child(empty),
                );
            }
        } else {
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
                        .child(self.loading_message.clone()),
                );
            }

            let row_height = if self.has_descriptions {
                DETAILED_ROW_HEIGHT
            } else {
                COMPACT_ROW_HEIGHT
            };
            let entries_rc = Rc::clone(&self.entries);
            let count = entries_rc.len();
            let highlighted = self.highlighted;
            let list_h = px((count as f32 * row_height).min(self.max_menu_height.0));
            let entity_list = entity.clone();

            let list = uniform_list(
                ("gpui-path-input-list", entity_id),
                count,
                move |range: Range<usize>, _window: &mut Window, cx: &mut App| {
                    let theme = cx.theme();
                    let hover_bg = theme.surface_hover;
                    range
                        .map(|idx| {
                            let entry = &entries_rc[idx];
                            let is_highlighted = highlighted == Some(idx);
                            let disabled = entry.disabled;
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
                            let icon = entry.icon.unwrap_or(if entry.is_dir {
                                LucideIcon::Folder
                            } else {
                                LucideIcon::File
                            });
                            let ent_hover = entity_list.clone();
                            let ent_click = entity_list.clone();
                            let entry_click = entry.clone();
                            let mono = mono_font.clone();

                            div()
                                .id(("gpui-path-input-row", idx))
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
                                                if this.highlighted != Some(idx) {
                                                    this.highlighted = Some(idx);
                                                    cx.notify();
                                                }
                                            });
                                        })
                                        .on_click(move |_, _, cx| {
                                            let chosen = entry_click.clone();
                                            ent_click.update(cx, |this, cx| {
                                                this.accept_entry(chosen, cx);
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
                                        .child(
                                            Icon::new(icon)
                                                .size(IconSize::Sm)
                                                .color(icon_fg),
                                        )
                                        .child(
                                            div()
                                                .flex()
                                                .flex_col()
                                                .flex_1()
                                                .min_w_0()
                                                .child(
                                                    div()
                                                        .truncate()
                                                        .when_some(mono, |el, f| el.font_family(f))
                                                        .text_size(px(12.5))
                                                        .font_weight(if is_highlighted {
                                                            FontWeight::MEDIUM
                                                        } else {
                                                            FontWeight::NORMAL
                                                        })
                                                        .text_color(label_fg)
                                                        .child(entry.label.clone()),
                                                )
                                                .when_some(
                                                    entry.description.clone(),
                                                    |el, desc| {
                                                        el.child(
                                                            div()
                                                                .truncate()
                                                                .text_size(px(11.5))
                                                                .text_color(theme.muted_foreground)
                                                                .child(desc),
                                                        )
                                                    },
                                                ),
                                        ),
                                )
                                .when_some(entry.badge.clone(), |el, badge_text| {
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
                        })
                        .collect()
                },
            )
            .track_scroll(&self.scroll_handle)
            .w_full()
            .h(list_h);

            panel = panel.child(div().w_full().p(px(4.)).child(list));

            let footer_text = if self.truncated {
                Some(self.truncated_message.clone())
            } else {
                self.hint_message.clone()
            };
            if let Some(footer_msg) = footer_text {
                panel = panel.child(
                    div()
                        .w_full()
                        .px(px(10.))
                        .py(px(6.))
                        .border_t_1()
                        .border_color(theme.border)
                        .bg(theme.surface_muted.opacity(0.35))
                        .text_size(px(11.))
                        .text_color(theme.muted_foreground)
                        .child(footer_msg),
                );
            }
        }

        let animated_panel = if reduce_motion {
            panel.into_any_element()
        } else {
            panel
                .with_animation(
                    ("gpui-path-input-anim", entity_id),
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

impl Render for PathInputState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity = cx.entity();
        let entity_id = cx.entity_id();
        let input_focused = !self.disabled && self.input.focus_handle(cx).is_focused(window);
        let focused = !self.disabled && (self.focus_handle.is_focused(window) || input_focused);
        let is_empty = self.input.read(cx).value().is_empty();

        // Keep embedded input synchronized silently without extra notify loops.
        {
            let inp = self.input.read(cx);
            let needs_size = inp.size_value() != self.size;
            let needs_ph = *inp.placeholder_text() != self.placeholder;
            let needs_dis = inp.is_disabled() != self.disabled;
            if needs_size || needs_ph || needs_dis {
                let size = self.size;
                let ph = self.placeholder.clone();
                let dis = self.disabled;
                self.input.update(cx, |inp, cx| {
                    if needs_size {
                        inp.set_size_silent(size);
                    }
                    if needs_ph {
                        inp.set_placeholder_silent(ph);
                    }
                    if needs_dis {
                        inp.set_disabled(dis, cx);
                    }
                });
            }
        }

        let border_color = if self.invalid {
            theme.destructive
        } else if focused || self.open {
            theme.ring
        } else {
            theme.input_border
        };
        let bg = if self.disabled {
            theme.disabled_bg
        } else {
            theme.input_bg
        };
        let input_h = px((self.size.height().0 - 2.0).max(20.0));
        let show_clear = self.clearable && !is_empty && self.can_edit();
        let bounds_cell = self.trigger_bounds.clone();
        let mono_font = self
            .monospace
            .then(|| theme.mono_font_family.clone())
            .flatten();

        let mut frame = div()
            .id(("gpui-path-input-frame", entity_id))
            .relative()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .w_full()
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_md)
            .bg(bg)
            .border_1()
            .border_color(border_color)
            .text_size(self.size.font_size())
            .when_some(mono_font, |el, f| el.font_family(f))
            .when(!self.disabled, |el| {
                el.cursor_text()
                    .when(theme.elevation_shadows, |el| el.shadow_2xs())
            })
            .when((focused || self.open) && !self.disabled, |el| {
                let ring = focus_ring_shadow(&theme, self.invalid);
                el.when(!ring.is_empty(), |el| el.shadow(ring))
            })
            .when(!focused && !self.open && !self.disabled, |el| {
                let hover_border = if self.invalid {
                    theme.destructive
                } else {
                    theme.input_border_hover
                };
                el.hover(move |s| s.border_color(hover_border))
            })
            .when(self.disabled, |el| el.opacity(0.6).cursor_not_allowed())
            .when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle.clone().tab_stop(false))
                    .on_key_down(cx.listener(Self::on_key_down))
                    .on_mouse_down(
                        MouseButton::Left,
                        cx.listener(|this, _, window, cx| {
                            let input_focus = this.input.focus_handle(cx);
                            window.focus(&input_focus, cx);
                            if !this.open && this.open_on_focus && this.can_edit() {
                                this.refresh_suggestions(true, cx);
                                cx.notify();
                            }
                        }),
                    )
            })
            .child(
                canvas(
                    move |bounds, _, _| {
                        bounds_cell.set(Some(bounds));
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .when_some(self.leading_icon, |el, icon| {
                el.child(
                    Icon::new(icon)
                        .size(self.size.icon_size())
                        .color(theme.muted_foreground),
                )
            })
            .child(
                div()
                    .flex_1()
                    .min_w_0()
                    .child(
                        Input::new(&self.input)
                            .border_0()
                            .shadow_none()
                            .bg(transparent_black())
                            .h(input_h),
                    ),
            )
            .when(self.loading && self.open, |el| {
                el.child(
                    Spinner::new()
                        .variant(SpinnerVariant::Ring)
                        .size(IconSize::Xs),
                )
            })
            .when(show_clear, |el| {
                let entity_clear = entity.clone();
                el.child(
                    div()
                        .id(("gpui-path-input-clear", entity_id))
                        .flex()
                        .items_center()
                        .justify_center()
                        .p(px(2.))
                        .rounded_full()
                        .cursor_pointer()
                        .hover(|s| s.bg(theme.surface_hover))
                        .on_mouse_down(MouseButton::Left, |_, window, cx| {
                            window.prevent_default();
                            cx.stop_propagation();
                        })
                        .on_click(move |_, _, cx| {
                            cx.stop_propagation();
                            entity_clear.update(cx, |state, cx| state.clear(cx));
                        })
                        .child(
                            Icon::new(LucideIcon::X)
                                .size(IconSize::Xs)
                                .color(theme.muted_foreground),
                        ),
                )
            });

        let popover = self.render_popover(cx);
        crate::Refineable::refine(frame.style(), &self.frame_style);
        let has_explicit_width = self.frame_style.size.width.is_some();

        div()
            .flex()
            .flex_col()
            .when(!has_explicit_width, |el| el.w_full())
            .child(frame)
            .when_some(popover, |el, pop| el.child(div().h_0().w_full().child(pop)))
    }
}

/// Renderable path autocomplete input wrapping an [`Entity<PathInputState>`].
#[derive(IntoElement)]
pub struct PathInput {
    state: Entity<PathInputState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`PathInput::new`].
pub fn path_input(state: &Entity<PathInputState>) -> PathInput {
    PathInput::new(state)
}

impl PathInput {
    /// Creates a [`PathInput`] element backed by `state`.
    pub fn new(state: &Entity<PathInputState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for PathInput {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for PathInput {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            self.state.update(cx, |state, cx| {
                state.frame_style = self.style;
                cx.notify();
            });
        }
        self.state
    }
}

#[cfg(test)]
mod tests {
    use std::cell::{Cell, RefCell};
    use std::rc::Rc;

    use super::*;
    use crate::components::button::button;
    use crate::components::keyboard_tests::keys;
    use crate::components::layout::v_stack;
    use crate::components::theme::init_components;
    use crate::{TestAppContext, VisualTestContext};

    fn focused<T: Focusable>(cx: &mut VisualTestContext, entity: &Entity<T>) -> bool {
        cx.update(|window, cx| entity.read(cx).focus_handle(cx).is_focused(window))
    }

    #[test]
    fn longest_common_path_prefix_handles_ascii_and_unicode() {
        assert_eq!(
            longest_common_path_prefix(&["/home/dev/outpost/", "/home/dev/output/"]),
            "/home/dev/outp"
        );
        assert_eq!(
            longest_common_path_prefix(&["~/项目/alpha/", "~/项目/beta/"]),
            "~/项目/"
        );
        assert_eq!(longest_common_path_prefix(&[]), "");
    }

    #[crate::test]
    fn path_input_tab_completion_dir_drilldown_and_escape(cx: &mut TestAppContext) {
        cx.update(init_components);

        struct Host {
            path: Entity<PathInputState>,
        }

        impl Render for Host {
            fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
                v_stack()
                    .w(px(420.))
                    .child(path_input(&self.path))
                    .child(button("after").label("After"))
            }
        }

        let events: Rc<RefCell<Vec<String>>> = Rc::new(RefCell::new(Vec::new()));
        let ev_log = events.clone();

        let (view, cx) = cx.add_window_view(move |_, cx| {
            let source = PathSource::sync(|prefix, _| match prefix {
                "" | "~" | "~/" => PathSuggestions::new(vec![
                    PathEntry::dir("~/projects/"),
                    PathEntry::dir("~/private/"),
                ]),
                "~/pr" => PathSuggestions::new(vec![
                    PathEntry::dir("~/projects/"),
                    PathEntry::dir("~/private/"),
                ]),
                "~/pro" => PathSuggestions::new(vec![PathEntry::dir("~/projects/")]),
                "~/projects/" => PathSuggestions::new(vec![
                    PathEntry::dir("~/projects/app/"),
                    PathEntry::file("~/projects/README.md"),
                ]),
                _ => PathSuggestions::empty(),
            });
            let path = cx.new(|cx| PathInputState::new(source, cx));
            let log = ev_log.clone();
            cx.subscribe(&path, move |_, _, ev: &PathInputEvent, _| match ev {
                PathInputEvent::Accept(e) => log.borrow_mut().push(format!("accept:{}", e.path)),
                PathInputEvent::Submit(v) => log.borrow_mut().push(format!("submit:{v}")),
                _ => {}
            })
            .detach();
            Host { path }
        });
        cx.run_until_parked();

        let path = view.read_with(cx, |h, _| h.path.clone());
        keys(cx, "tab");
        assert!(focused(cx, &path));
        assert!(path.read_with(cx, |p, _| p.is_open()));
        assert_eq!(path.read_with(cx, |p, _| p.entries().len()), 2);

        // Tab with multiple matches completes the longest common prefix ("~/pr") and keeps focus.
        keys(cx, "tab");
        assert_eq!(path.read_with(cx, |p, cx| p.value(cx).to_owned()), "~/pr");
        assert!(focused(cx, &path));

        // Typing "o" narrows to a single match; Tab accepts "~/projects/" and drills down into its children.
        cx.simulate_input("o");
        cx.run_until_parked();
        assert_eq!(path.read_with(cx, |p, _| p.entries().len()), 1);
        keys(cx, "tab");
        assert_eq!(
            path.read_with(cx, |p, cx| p.value(cx).to_owned()),
            "~/projects/"
        );
        assert!(focused(cx, &path));
        assert!(path.read_with(cx, |p, _| p.is_open()));
        assert_eq!(path.read_with(cx, |p, _| p.entries().len()), 2);

        // Down -> Down highlights the file ("~/projects/README.md"); Enter accepts it and closes popover.
        keys(cx, "down down enter");
        assert_eq!(
            path.read_with(cx, |p, cx| p.value(cx).to_owned()),
            "~/projects/README.md"
        );
        assert!(!path.read_with(cx, |p, _| p.is_open()));
        assert!(events
            .borrow()
            .contains(&"accept:~/projects/README.md".to_string()));

        // Pressing Enter with popover closed emits Submit.
        keys(cx, "enter");
        assert_eq!(
            events.borrow().last().map(String::as_str),
            Some("submit:~/projects/README.md")
        );

        // Escape closes an open popover while keeping focus on the input.
        path.update(cx, |p, cx| p.open_suggestions(cx));
        cx.run_until_parked();
        assert!(path.read_with(cx, |p, _| p.is_open()));
        keys(cx, "escape");
        assert!(!path.read_with(cx, |p, _| p.is_open()));
        assert!(focused(cx, &path));
    }

    #[crate::test]
    fn path_input_async_error_retry_and_query_filter(cx: &mut TestAppContext) {
        cx.update(init_components);
        let attempts = Rc::new(Cell::new(0usize));
        let attempts_cb = attempts.clone();

        let (path, cx) = cx.add_window_view(move |_, cx| {
            let source = PathSource::async_loader(move |prefix, cx| {
                let attempt = attempts_cb.get() + 1;
                attempts_cb.set(attempt);
                let prefix = prefix.to_owned();
                cx.foreground_executor().spawn(async move {
                    if attempt == 1 {
                        Err(SharedString::from("Connection timed out"))
                    } else {
                        Ok(PathSuggestions::new(vec![PathEntry::dir(format!(
                            "{prefix}app/"
                        ))])
                        .truncated(true))
                    }
                })
            })
            .debounce(Duration::ZERO);
            PathInputState::new(source, cx)
                .query_filter(|q| q.is_empty() || q.starts_with('/') || q.starts_with("~/"))
        });
        cx.run_until_parked();

        // Invalid prefix is skipped by query_filter and does not invoke the loader.
        path.update(cx, |p, cx| {
            p.set_value("relative-invalid", cx);
            p.open_suggestions(cx);
        });
        cx.run_until_parked();
        assert_eq!(attempts.get(), 0);
        assert!(!path.read_with(cx, |p, _| p.is_open()));

        // Valid prefix triggers lookup -> first attempt errors -> retry succeeds with truncated=true.
        path.update(cx, |p, cx| {
            p.set_value("~/", cx);
            p.open_suggestions(cx);
        });
        cx.run_until_parked();
        assert_eq!(attempts.get(), 1);
        assert_eq!(
            path.read_with(cx, |p, _| p.error().cloned()),
            Some(SharedString::from("Connection timed out"))
        );

        path.update(cx, |p, cx| p.retry(cx));
        cx.run_until_parked();
        assert_eq!(attempts.get(), 2);
        assert!(path.read_with(cx, |p, _| p.error().is_none()));
        assert!(path.read_with(cx, |p, _| p.truncated()));
        assert_eq!(
            path.read_with(cx, |p, _| p.entries()[0].path.clone()),
            SharedString::from("~/app/")
        );
    }
}

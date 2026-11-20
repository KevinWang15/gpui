//! Native desktop [`FileDropzone`] / [`FileDropzoneState`] supporting OS drag-and-drop
//! ([`ExternalPaths`]), native file dialog browsing ([`App::prompt_for_paths`]), extension
//! filtering, per-file upload progress bars, status badges, keyboard activation, and file removal.

use std::collections::HashSet;
use std::path::{Path, PathBuf};

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    App, Context, Entity, EventEmitter, ExternalPaths, FocusHandle, Focusable, FontWeight,
    IntoElement, KeyDownEvent, PathPromptOptions, Pixels, Render, RenderOnce, SharedString,
    StyleRefinement, Styled, Task, Window, div, prelude::*, px,
};

use super::badge::{BadgeColor, badge};
use super::feedback::ProgressBar;
use super::theme::{ActiveTheme, ComponentSize, focus_ring_shadow, tabular_numerals};

/// Processing / upload status for a [`FileEntry`].
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub enum FileStatus {
    /// Added and waiting to start.
    #[default]
    Ready,
    /// Actively uploading / processing (paired with optional `progress`).
    Uploading,
    /// Finished cleanly.
    Done,
    /// Failed with an error reason.
    Error(SharedString),
}

/// Single file entry tracked inside [`FileDropzoneState`].
#[derive(Clone, Debug)]
pub struct FileEntry {
    /// Stable monotonic ID within the dropzone.
    pub id: u64,
    /// Local filesystem path.
    pub path: PathBuf,
    /// Display filename.
    pub name: SharedString,
    /// File size in bytes, if available.
    pub size_bytes: Option<u64>,
    /// Optional completion percentage (`0.0..=100.0`).
    pub progress: Option<f32>,
    /// Current status.
    pub status: FileStatus,
}

impl FileEntry {
    /// Creates a [`FileEntry`] from a local path, reading file size if accessible on disk.
    pub fn from_path(id: u64, path: PathBuf) -> Self {
        let name: SharedString = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.to_string_lossy().into_owned())
            .into();
        let size_bytes = std::fs::metadata(&path)
            .ok()
            .filter(|m| m.is_file())
            .map(|m| m.len());
        Self {
            id,
            path,
            name,
            size_bytes,
            progress: None,
            status: FileStatus::Ready,
        }
    }

    /// Creates a synthetic [`FileEntry`] with explicit size and status (useful for demos & tests).
    pub fn synthetic(
        id: u64,
        name: impl Into<SharedString>,
        size_bytes: u64,
        status: FileStatus,
        progress: Option<f32>,
    ) -> Self {
        let name: SharedString = name.into();
        let clamped_progress = progress.map(|p| if p.is_finite() { p.clamp(0.0, 100.0) } else { 0.0 });
        Self {
            id,
            path: PathBuf::from(name.as_ref()),
            name,
            size_bytes: Some(size_bytes),
            progress: clamped_progress,
            status,
        }
    }
}

/// Events emitted by [`FileDropzoneState`].
#[derive(Clone, Debug)]
pub enum FileDropzoneEvent {
    /// Emitted when one or more accepted files are added via drop, OS dialog, or `add_paths`.
    FilesAdded(Vec<FileEntry>),
    /// Emitted when a file is removed by ID.
    FileRemoved(u64),
    /// Emitted when files are rejected by extension filter, directory policy, or max file count.
    Rejected(SharedString),
}

/// Formats a byte count into a clean human-readable string (`"1.4 MB"`).
pub fn format_byte_size(bytes: u64) -> String {
    const KB: f64 = 1024.0;
    const MB: f64 = 1024.0 * 1024.0;
    const GB: f64 = 1024.0 * 1024.0 * 1024.0;
    let b = bytes as f64;
    if b >= GB {
        format!("{:.2} GB", b / GB)
    } else if b >= MB {
        format!("{:.1} MB", b / MB)
    } else if b >= KB {
        format!("{:.1} KB", b / KB)
    } else {
        format!("{bytes} B")
    }
}

fn icon_for_filename(name: &str) -> LucideIcon {
    let ext = Path::new(name)
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    match ext.as_deref() {
        Some("png" | "jpg" | "jpeg" | "gif" | "webp" | "svg") => LucideIcon::Image,
        Some("rs" | "ts" | "tsx" | "js" | "py" | "json" | "toml" | "yaml" | "yml") => {
            LucideIcon::FileCode
        }
        Some("zip" | "tar" | "gz" | "7z" | "rar") => LucideIcon::FileArchive,
        Some("csv" | "xlsx" | "parquet") => LucideIcon::FileSpreadsheet,
        Some("md" | "txt" | "pdf" | "log") => LucideIcon::FileText,
        _ => LucideIcon::File,
    }
}

/// Entity state backing a desktop [`FileDropzone`].
pub struct FileDropzoneState {
    focus_handle: FocusHandle,
    files: Vec<FileEntry>,
    next_id: u64,
    accept_extensions: Vec<String>,
    cached_accept_hint: Option<SharedString>,
    multiple: bool,
    allow_directories: bool,
    max_files: Option<usize>,
    max_list_height: Option<Pixels>,
    disabled: bool,
    picker_task: Option<Task<()>>,
    title: SharedString,
    description: SharedString,
    frame_style: StyleRefinement,
}

impl EventEmitter<FileDropzoneEvent> for FileDropzoneState {}

impl Focusable for FileDropzoneState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl FileDropzoneState {
    /// Creates a new [`FileDropzoneState`] accepting multiple files.
    pub fn new(cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            files: Vec::new(),
            next_id: 1,
            accept_extensions: Vec::new(),
            cached_accept_hint: None,
            multiple: true,
            allow_directories: false,
            max_files: None,
            max_list_height: None,
            disabled: false,
            picker_task: None,
            title: "Drop files here or click to browse".into(),
            description: "Supports native desktop drag-and-drop and OS file picker".into(),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Restricts accepted file extensions (case-insensitive, with or without leading `.`).
    pub fn accept(mut self, exts: &[&str]) -> Self {
        self.accept_extensions = exts
            .iter()
            .map(|e| e.trim().trim_start_matches('.').to_ascii_lowercase())
            .filter(|e| !e.is_empty())
            .collect();
        self.cached_accept_hint = (!self.accept_extensions.is_empty()).then(|| {
            SharedString::from(format!(
                "Accepted: {}",
                self.accept_extensions
                    .iter()
                    .map(|e| format!(".{e}"))
                    .collect::<Vec<_>>()
                    .join(", ")
            ))
        });
        self
    }

    /// Controls whether multiple files can be attached (default `true`).
    pub fn multiple(mut self, multiple: bool) -> Self {
        self.multiple = multiple;
        self
    }

    /// Allows choosing or dropping folders as well as files.
    pub fn allow_directories(mut self, allow: bool) -> Self {
        self.allow_directories = allow;
        self
    }

    /// Caps the maximum number of files stored simultaneously.
    pub fn max_files(mut self, max: usize) -> Self {
        self.max_files = Some(max.max(1));
        self
    }

    /// Sets an optional scrollable max height for the attached file list.
    pub fn max_list_height(mut self, height: Pixels) -> Self {
        self.max_list_height = height.0.is_finite().then(|| height.max(px(64.)));
        self
    }

    /// Sets the dropzone headline prompt.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = title.into();
        self
    }

    /// Sets the dropzone secondary helper description.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = description.into();
        self
    }

    /// Disables browsing, dropping, and removing attached files.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Dynamically updates disabled state.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            cx.notify();
        }
    }

    /// Pre-populates files (e.g. existing attachments or demo entries), ensuring unique IDs.
    pub fn default_files(mut self, mut entries: Vec<FileEntry>) -> Self {
        let mut seen = HashSet::with_capacity(entries.len());
        let mut next = entries
            .iter()
            .map(|e| e.id)
            .max()
            .unwrap_or(0)
            .saturating_add(1)
            .max(1);
        for entry in &mut entries {
            if !seen.insert(entry.id) {
                while !seen.insert(next) {
                    next = next.saturating_add(1);
                }
                entry.id = next;
                next = next.saturating_add(1);
            }
        }
        self.next_id = next;
        self.files = entries;
        self
    }

    /// Returns the currently attached file list.
    pub fn files(&self) -> &[FileEntry] {
        &self.files
    }

    fn alloc_id(&mut self) -> u64 {
        let id = self.next_id;
        self.next_id = self.next_id.saturating_add(1);
        id
    }

    /// Adds local file paths subject to directory, extension, multiplicity, and max-file rules.
    pub fn add_paths(&mut self, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        if self.disabled || paths.is_empty() {
            return;
        }
        let mut added = Vec::new();
        for path in paths {
            let is_dir = path.is_dir();
            if is_dir && !self.allow_directories {
                cx.emit(FileDropzoneEvent::Rejected(
                    format!("Directories are not allowed: {}", path.display()).into(),
                ));
                continue;
            }
            if !is_dir && !self.accept_extensions.is_empty() {
                let ext = path
                    .extension()
                    .and_then(|e| e.to_str())
                    .map(|e| e.to_ascii_lowercase())
                    .unwrap_or_default();
                if !self.accept_extensions.iter().any(|allowed| allowed == &ext) {
                    cx.emit(FileDropzoneEvent::Rejected(
                        format!("Unsupported file type: {}", path.display()).into(),
                    ));
                    continue;
                }
            }
            if !self.multiple {
                if !added.is_empty() {
                    cx.emit(FileDropzoneEvent::Rejected(
                        "Only a single file can be attached.".into(),
                    ));
                    break;
                }
                let removed_ids: Vec<u64> = self.files.drain(..).map(|f| f.id).collect();
                for rid in removed_ids {
                    cx.emit(FileDropzoneEvent::FileRemoved(rid));
                }
            }
            if let Some(max) = self.max_files
                && self.files.len() >= max
            {
                cx.emit(FileDropzoneEvent::Rejected(
                    format!("Maximum of {max} files reached.").into(),
                ));
                break;
            }
            let id = self.alloc_id();
            let entry = FileEntry::from_path(id, path);
            self.files.push(entry.clone());
            added.push(entry);
        }
        if !added.is_empty() {
            cx.emit(FileDropzoneEvent::FilesAdded(added));
            cx.notify();
        }
    }

    /// Adds a custom/synthetic entry directly (respecting `multiple` and `max_files`).
    pub fn push_synthetic(
        &mut self,
        name: impl Into<SharedString>,
        size_bytes: u64,
        status: FileStatus,
        progress: Option<f32>,
        cx: &mut Context<Self>,
    ) -> u64 {
        if !self.multiple {
            let removed_ids: Vec<u64> = self.files.drain(..).map(|f| f.id).collect();
            for rid in removed_ids {
                cx.emit(FileDropzoneEvent::FileRemoved(rid));
            }
        } else if let Some(max) = self.max_files
            && self.files.len() >= max
        {
            cx.emit(FileDropzoneEvent::Rejected(
                format!("Maximum of {max} files reached.").into(),
            ));
            return 0;
        }
        let id = self.alloc_id();
        let entry = FileEntry::synthetic(id, name, size_bytes, status, progress);
        self.files.push(entry.clone());
        cx.emit(FileDropzoneEvent::FilesAdded(vec![entry]));
        cx.notify();
        id
    }

    /// Updates progress (`0.0..=100.0`) and status for a file by ID.
    pub fn set_file_progress(
        &mut self,
        id: u64,
        progress: f32,
        status: FileStatus,
        cx: &mut Context<Self>,
    ) {
        if let Some(file) = self.files.iter_mut().find(|f| f.id == id) {
            let safe_pct = if progress.is_finite() {
                progress.clamp(0.0, 100.0)
            } else {
                0.0
            };
            file.progress = Some(safe_pct);
            file.status = status;
            cx.notify();
        }
    }

    /// Removes a file by ID and emits [`FileDropzoneEvent::FileRemoved`].
    pub fn remove_file(&mut self, id: u64, cx: &mut Context<Self>) {
        let before = self.files.len();
        self.files.retain(|f| f.id != id);
        if self.files.len() != before {
            cx.emit(FileDropzoneEvent::FileRemoved(id));
            cx.notify();
        }
    }

    /// Clears all attached files.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if !self.files.is_empty() {
            self.files.clear();
            cx.notify();
        }
    }

    /// Opens the OS native file picker and appends any chosen paths (guarded against double-open).
    pub fn open_picker(&mut self, cx: &mut Context<Self>) {
        if self.disabled || self.picker_task.is_some() {
            return;
        }
        let receiver = cx.prompt_for_paths(PathPromptOptions {
            files: true,
            directories: self.allow_directories,
            multiple: self.multiple,
            prompt: Some("Select files".into()),
        });
        let task = cx.spawn(async move |this, cx| {
            let res = receiver.await;
            let _ = this.update(cx, |state, cx| {
                state.picker_task = None;
                if let Ok(Ok(Some(paths))) = res {
                    state.add_paths(paths, cx);
                }
            });
        });
        self.picker_task = Some(task);
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, _window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        let mods = event.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        if matches!(event.keystroke.key.as_str(), "enter" | "space") {
            self.open_picker(cx);
            cx.stop_propagation();
        }
    }
}

impl Render for FileDropzoneState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let hover_bg = theme.primary_soft.opacity(0.45);
        let drag_border = theme.primary;
        let drag_bg = theme.primary_soft.opacity(0.65);
        let entity_id = cx.entity_id();
        let is_focused = self.focus_handle.is_focused(window);
        let disabled = self.disabled;

        let zone = div()
            .id(("file-dropzone-target", entity_id))
            .when(!disabled, |el| {
                el.track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
            })
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap(px(8.))
            .w_full()
            .px(px(20.))
            .py(px(26.))
            .rounded(theme.radius_xl)
            .border_2()
            .border_dashed()
            .border_color(if is_focused && !disabled {
                theme.ring
            } else {
                theme.input_border
            })
            .when(is_focused && !disabled, |el| {
                el.shadow(focus_ring_shadow(&theme))
            })
            .bg(theme.surface_muted.opacity(0.35))
            .when(!disabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.border_color(drag_border).bg(hover_bg))
                    .drag_over::<ExternalPaths>(move |style, _, _, _| {
                        style.border_color(drag_border).bg(drag_bg)
                    })
                    .on_drop::<ExternalPaths>(cx.listener(|this, paths: &ExternalPaths, _, cx| {
                        this.add_paths(paths.paths().to_vec(), cx);
                    }))
                    .on_click(cx.listener(|this, _, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.open_picker(cx);
                    }))
            })
            .when(disabled, |el| el.opacity(0.5).cursor_not_allowed())
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(42.))
                    .h(px(42.))
                    .rounded_full()
                    .bg(theme.primary_soft)
                    .child(
                        Icon::new(LucideIcon::Upload)
                            .size(IconSize::Lg)
                            .color(theme.primary_soft_foreground),
                    ),
            )
            .child(
                div()
                    .text_size(px(14.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .child(self.title.clone()),
            )
            .child(
                div()
                    .text_size(px(12.5))
                    .text_color(theme.muted_foreground)
                    .child(self.description.clone()),
            )
            .when_some(self.cached_accept_hint.clone(), |el, hint| {
                el.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.muted_foreground)
                        .child(hint),
                )
            });

        let file_rows = (!self.files.is_empty()).then(|| {
            let remove_hover = theme.destructive_soft;
            let mut container = div()
                .id(("dz-file-list", entity_id))
                .flex()
                .flex_col()
                .gap(px(6.))
                .w_full();
            if let Some(max_h) = self.max_list_height {
                container = container.max_h(max_h).overflow_y_scroll();
            }
            container.children(self.files.iter().map(|file| {
                let id = file.id;
                let icon = icon_for_filename(&file.name);
                let status_badge = match &file.status {
                    FileStatus::Ready => badge("Ready").color(BadgeColor::Gray).pill(true),
                    FileStatus::Uploading => badge("Uploading").color(BadgeColor::Indigo).pill(true),
                    FileStatus::Done => badge("Uploaded").color(BadgeColor::Green).pill(true),
                    FileStatus::Error(msg) => badge(msg.clone())
                        .color(BadgeColor::Red)
                        .pill(true)
                        .max_w(px(180.))
                        .truncate(),
                };
                let is_error = matches!(file.status, FileStatus::Error(_));
                div()
                    .id(("dz-file", id))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .w_full()
                    .px(px(12.))
                    .py(px(9.))
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .bg(theme.surface)
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .justify_between()
                            .gap(px(10.))
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
                                            .size(IconSize::Md)
                                            .color(theme.primary),
                                    )
                                    .child(
                                        div()
                                            .text_size(px(13.))
                                            .font_weight(FontWeight::MEDIUM)
                                            .text_color(theme.foreground)
                                            .truncate()
                                            .child(file.name.clone()),
                                    )
                                    .when_some(file.size_bytes, |el, bytes| {
                                        el.child(
                                            div()
                                                .flex_shrink_0()
                                                .text_size(px(11.5))
                                                .font_features(tabular_numerals())
                                                .text_color(theme.muted_foreground)
                                                .child(format_byte_size(bytes)),
                                        )
                                    }),
                            )
                            .child(
                                div()
                                    .flex()
                                    .flex_row()
                                    .items_center()
                                    .flex_shrink_0()
                                    .gap(px(6.))
                                    .child(status_badge)
                                    .child(
                                        div()
                                            .id(("dz-rm", id))
                                            .flex()
                                            .items_center()
                                            .justify_center()
                                            .w(px(22.))
                                            .h(px(22.))
                                            .rounded(theme.radius_sm)
                                            .when(!disabled, |el| {
                                                el.cursor_pointer()
                                                    .hover(move |s| s.bg(remove_hover))
                                                    .on_click(cx.listener(move |this, _, _, cx| {
                                                        this.remove_file(id, cx);
                                                    }))
                                            })
                                            .when(disabled, |el| {
                                                el.opacity(0.4).cursor_not_allowed()
                                            })
                                            .child(
                                                Icon::new(LucideIcon::X)
                                                    .size(IconSize::Xs)
                                                    .color(theme.muted_foreground),
                                            ),
                                    ),
                            ),
                    )
                    .when_some(file.progress, |el, pct| {
                        if !is_error && (file.status == FileStatus::Uploading || pct < 100.0) {
                            el.child(ProgressBar::new(pct).size(ComponentSize::Xs))
                        } else {
                            el
                        }
                    })
            }))
        });

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w_full()
            .child(zone)
            .children(file_rows);
        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable desktop file dropzone element wrapping [`Entity<FileDropzoneState>`].
#[derive(IntoElement)]
pub struct FileDropzone {
    state: Entity<FileDropzoneState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`FileDropzone::new`].
pub fn file_dropzone(state: &Entity<FileDropzoneState>) -> FileDropzone {
    FileDropzone::new(state)
}

impl FileDropzone {
    /// Creates a [`FileDropzone`] bound to `state`.
    pub fn new(state: &Entity<FileDropzoneState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for FileDropzone {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FileDropzone {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.frame_style = style;
            });
        }
        self.state
    }
}

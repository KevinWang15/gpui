//! Cross-platform desktop & Windows application builder primitives:
//!
//! - [`WindowCornerPreference`] — OS rounded corner preference (`DWMWA_WINDOW_CORNER_PREFERENCE`).
//! - [`ExternalWindowHandle`] — opaque handle to an external foreground OS window for focus restore.
//! - [`FileFilter`], [`OpenPromptOptions`], [`SavePromptOptions`] — filtered native open/save dialogs.
//! - [`TrayIconData`], [`TrayMenuItem`], [`TrayIconOptions`], [`TrayEvent`] — system tray & native context menus.
//! - [`GlobalHotKey`], [`GlobalHotKeyId`], [`GlobalHotKeyManager`] — global OS shortcuts with transactional apply & suspend/resume.
//! - [`AutostartOptions`] — per-user launch-at-login registration (Windows Registry `Run`, macOS LaunchAgent, Linux XDG autostart).

use std::collections::HashMap;
use std::path::PathBuf;

use anyhow::{Result, bail};

use crate::{App, Keystroke, Modifiers, SharedString};

/// Platform rounded-corner preference for top-level and popup windows (e.g. Windows 11
/// `DWMWA_WINDOW_CORNER_PREFERENCE`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum WindowCornerPreference {
    /// Let the operating system choose its default corner rounding.
    #[default]
    Default,
    /// Disable OS-applied corner rounding so custom-painted or WinRT-clipped corners stay exact.
    DoNotRound,
    /// Standard rounded corners (8 px on Windows 11).
    Round,
    /// Subtle small rounded corners (4 px on Windows 11).
    RoundSmall,
}

/// Opaque handle to an external operating-system window (such as the previously active window
/// captured before showing a Spotlight/Alfred launcher so focus can be returned cleanly on close).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct ExternalWindowHandle(pub isize);

/// A single file-type filter for native open/save file dialogs (e.g. `"PNG Image"`, `["png"]`).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct FileFilter {
    /// Human-readable description shown in the dialog's format dropdown.
    pub name: SharedString,
    /// Lowercase file extensions without leading dots (e.g. `["png", "jpg"]` or `["*"]`).
    pub extensions: Vec<SharedString>,
}

impl FileFilter {
    /// Creates a new [`FileFilter`] with a display name and extension list.
    pub fn new(
        name: impl Into<SharedString>,
        extensions: impl IntoIterator<Item = impl Into<SharedString>>,
    ) -> Self {
        Self {
            name: name.into(),
            extensions: extensions.into_iter().map(Into::into).collect(),
        }
    }

    /// Formats the extension list into a semicolon-separated Win32 wildcard spec (e.g. `"*.png;*.jpg"`).
    pub fn windows_spec(&self) -> String {
        if self.extensions.is_empty() {
            return "*.*".to_owned();
        }
        self.extensions
            .iter()
            .map(|ext| {
                let trimmed = ext.trim().trim_start_matches('.');
                if trimmed.is_empty() || trimmed == "*" {
                    "*.*".to_owned()
                } else {
                    format!("*.{trimmed}")
                }
            })
            .collect::<Vec<_>>()
            .join(";")
    }
}

/// Options for [`App::prompt_for_open_paths`], extending [`crate::PathPromptOptions`] with
/// file-type filters and an optional starting directory.
#[derive(Clone, Debug)]
pub struct OpenPromptOptions {
    /// Whether files may be selected.
    pub files: bool,
    /// Whether directories may be selected.
    pub directories: bool,
    /// Whether multiple paths may be selected.
    pub multiple: bool,
    /// Optional confirm button label / prompt title.
    pub prompt: Option<SharedString>,
    /// Optional initial directory opened by the file picker.
    pub initial_directory: Option<PathBuf>,
    /// Optional list of allowed file-type filters.
    pub filters: Vec<FileFilter>,
}

impl Default for OpenPromptOptions {
    fn default() -> Self {
        Self {
            files: true,
            directories: false,
            multiple: false,
            prompt: None,
            initial_directory: None,
            filters: Vec::new(),
        }
    }
}

/// Options for [`App::prompt_for_save_path`], supporting file-type filters, default extension,
/// suggested filename, and initial directory.
#[derive(Clone, Debug, Default)]
pub struct SavePromptOptions {
    /// Optional initial directory.
    pub directory: Option<PathBuf>,
    /// Optional prefilled file name.
    pub suggested_name: Option<SharedString>,
    /// Optional list of format filters shown in the "Save as type" dropdown.
    pub filters: Vec<FileFilter>,
    /// Optional default extension appended when the user omits one (without leading dot).
    pub default_extension: Option<SharedString>,
    /// Optional confirm button label / prompt title.
    pub prompt: Option<SharedString>,
}

/// Decoded RGBA8 pixel buffer representing a system tray icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TrayIconData {
    /// Icon width in pixels.
    pub width: u32,
    /// Icon height in pixels.
    pub height: u32,
    /// Row-major straight-alpha RGBA bytes (`width * height * 4`).
    pub rgba: Vec<u8>,
    /// Hint for macOS menu bar monochrome template rendering.
    pub is_template: bool,
}

impl TrayIconData {
    /// Creates [`TrayIconData`] from raw row-major RGBA8 bytes.
    pub fn from_rgba(width: u32, height: u32, rgba: Vec<u8>) -> Result<Self> {
        let expected = (width as usize)
            .checked_mul(height as usize)
            .and_then(|px| px.checked_mul(4))
            .ok_or_else(|| anyhow::anyhow!("tray icon dimensions overflow"))?;
        anyhow::ensure!(
            width > 0 && height > 0 && rgba.len() == expected,
            "invalid tray icon buffer: expected {expected} bytes for {width}x{height}, got {}",
            rgba.len()
        );
        Ok(Self {
            width,
            height,
            rgba,
            is_template: false,
        })
    }

    /// Decodes an encoded PNG (or other supported image format) into [`TrayIconData`].
    pub fn from_image_bytes(bytes: &[u8]) -> Result<Self> {
        let decoded = image::load_from_memory(bytes)?.to_rgba8();
        let (width, height) = decoded.dimensions();
        Self::from_rgba(width, height, decoded.into_raw())
    }

    /// Generates a crisp anti-aliased solid circular badge icon in RGBA8 — handy for demos,
    /// dynamic status dots, and apps without an embedded `.png`.
    pub fn solid_badge(size: u32, r: u8, g: u8, b: u8) -> Self {
        let size = size.max(16);
        let mut rgba = vec![0u8; (size * size * 4) as usize];
        let center = (size as f32 - 1.0) * 0.5;
        let radius = size as f32 * 0.42;
        for y in 0..size {
            for x in 0..size {
                let dx = x as f32 - center;
                let dy = y as f32 - center;
                let dist = (dx * dx + dy * dy).sqrt();
                let cov = (radius + 0.75 - dist).clamp(0.0, 1.0);
                if cov > 0.0 {
                    let idx = ((y * size + x) * 4) as usize;
                    rgba[idx] = r;
                    rgba[idx + 1] = g;
                    rgba[idx + 2] = b;
                    rgba[idx + 3] = (cov * 255.0).round() as u8;
                }
            }
        }
        Self {
            width: size,
            height: size,
            rgba,
            is_template: false,
        }
    }

    /// Marks this icon as a monochrome template image (used on macOS menu bars).
    pub fn template(mut self, is_template: bool) -> Self {
        self.is_template = is_template;
        self
    }
}

/// Entry in a native OS tray menu or native window context menu.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayMenuItem {
    /// Standard clickable command item.
    Action {
        /// Identifier emitted in [`TrayEvent::MenuItem`].
        id: SharedString,
        /// Visible label text.
        label: SharedString,
        /// Whether the item is enabled.
        enabled: bool,
    },
    /// Toggleable checkable menu item.
    Check {
        /// Identifier emitted in [`TrayEvent::MenuItem`].
        id: SharedString,
        /// Visible label text.
        label: SharedString,
        /// Current checkmark state.
        checked: bool,
        /// Whether the item is enabled.
        enabled: bool,
    },
    /// Horizontal divider line.
    Separator,
    /// Nested submenu.
    Submenu {
        /// Visible submenu title.
        label: SharedString,
        /// Whether the submenu is enabled.
        enabled: bool,
        /// Child menu entries.
        items: Vec<TrayMenuItem>,
    },
}

impl TrayMenuItem {
    /// Creates an enabled action item.
    pub fn action(id: impl Into<SharedString>, label: impl Into<SharedString>) -> Self {
        Self::Action {
            id: id.into(),
            label: label.into(),
            enabled: true,
        }
    }

    /// Creates a disabled status/header row.
    pub fn status(label: impl Into<SharedString>) -> Self {
        Self::Action {
            id: SharedString::default(),
            label: label.into(),
            enabled: false,
        }
    }

    /// Creates an enabled checkable menu item.
    pub fn check(
        id: impl Into<SharedString>,
        label: impl Into<SharedString>,
        checked: bool,
    ) -> Self {
        Self::Check {
            id: id.into(),
            label: label.into(),
            checked,
            enabled: true,
        }
    }

    /// Creates a separator divider.
    pub const fn separator() -> Self {
        Self::Separator
    }

    /// Creates a nested submenu.
    pub fn submenu(label: impl Into<SharedString>, items: Vec<TrayMenuItem>) -> Self {
        Self::Submenu {
            label: label.into(),
            enabled: true,
            items,
        }
    }
}

/// Configuration for creating a system tray / menu bar icon via [`App::set_tray_icon`].
#[derive(Clone, Debug)]
pub struct TrayIconOptions {
    /// Hover tooltip text.
    pub tooltip: SharedString,
    /// Icon image pixels.
    pub icon: TrayIconData,
    /// Context menu entries.
    pub menu: Vec<TrayMenuItem>,
    /// Whether left-clicking opens the menu directly (`false` by default on Windows so left-click
    /// triggers the app's primary action while right-click opens the menu).
    pub menu_on_left_click: bool,
}

impl TrayIconOptions {
    /// Creates [`TrayIconOptions`] with the given tooltip and icon.
    pub fn new(tooltip: impl Into<SharedString>, icon: TrayIconData) -> Self {
        Self {
            tooltip: tooltip.into(),
            icon,
            menu: Vec::new(),
            menu_on_left_click: cfg!(target_os = "macos"),
        }
    }

    /// Sets the context menu items.
    pub fn menu(mut self, items: Vec<TrayMenuItem>) -> Self {
        self.menu = items;
        self
    }

    /// Configures whether left-clicking shows the menu directly.
    pub fn menu_on_left_click(mut self, open_on_left: bool) -> Self {
        self.menu_on_left_click = open_on_left;
        self
    }
}

/// User interaction delivered from the OS system tray / menu bar icon.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TrayEvent {
    /// Primary (left) click on the tray icon.
    LeftClick,
    /// Double-click on the tray icon.
    DoubleClick,
    /// Secondary (right) click on the tray icon.
    RightClick,
    /// A tray menu item with the given `id` was activated.
    MenuItem(SharedString),
}

/// Identifier returned when registering a global system hotkey.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct GlobalHotKeyId(pub u32);

/// Portable keyboard shortcut specification shared by [`App::register_global_hotkey`] and
/// [`crate::components::ShortcutRecorder`].
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub struct GlobalHotKey {
    /// Modifier keys required for this shortcut.
    pub modifiers: Modifiers,
    /// Canonical key name (e.g. `"A"`, `"Space"`, `"F12"`, `"ArrowUp"`, `"Backquote"`).
    pub key: SharedString,
}

impl GlobalHotKey {
    /// Creates a new [`GlobalHotKey`] with canonicalized key naming.
    pub fn new(modifiers: Modifiers, key: impl Into<SharedString>) -> Self {
        let raw: SharedString = key.into();
        let canonical = canonicalize_shortcut_key(raw.as_ref()).unwrap_or_else(|| raw.to_string());
        Self {
            modifiers,
            key: SharedString::from(canonical),
        }
    }

    /// Parses a canonical shortcut string such as `"Ctrl+Alt+A"`, `"Super+Space"`, or `"F9"`.
    pub fn parse(spec: &str) -> Result<Self> {
        let mut mods = Modifiers::none();
        let mut key_token: Option<&str> = None;

        for raw in spec.split('+') {
            let token = raw.trim();
            if token.is_empty() {
                bail!("empty token in shortcut specification {spec:?}");
            }
            match token.to_ascii_lowercase().as_str() {
                "ctrl" | "control" => mods.control = true,
                "alt" | "option" => mods.alt = true,
                "shift" => mods.shift = true,
                "super" | "cmd" | "command" | "meta" | "win" | "windows" => mods.platform = true,
                _ => {
                    if key_token.is_some() {
                        bail!("multiple non-modifier keys in shortcut {spec:?}");
                    }
                    key_token = Some(token);
                }
            }
        }

        let key = key_token
            .and_then(canonicalize_shortcut_key)
            .ok_or_else(|| anyhow::anyhow!("missing or unsupported key in shortcut {spec:?}"))?;

        if !mods.modified() && !is_function_key(&key) {
            bail!("shortcut {spec:?} requires at least one modifier key (or a function key F1-F24)");
        }

        Ok(Self {
            modifiers: mods,
            key: SharedString::from(key),
        })
    }

    /// Attempts to convert a GPUI [`Keystroke`] into a valid [`GlobalHotKey`].
    ///
    /// Rejects bare modifier presses and plain unmodified typing keys (while carefully
    /// allowing function keys `F1`..`F24` without modifiers, and distinguishing letter `"f"`).
    pub fn from_keystroke(keystroke: &Keystroke) -> Option<Self> {
        let canonical = canonicalize_shortcut_key(keystroke.key.as_str())?;
        let mods = Modifiers {
            control: keystroke.modifiers.control,
            alt: keystroke.modifiers.alt,
            shift: keystroke.modifiers.shift,
            platform: keystroke.modifiers.platform,
            function: false,
        };
        if !mods.modified() && !is_function_key(&canonical) {
            return None;
        }
        Some(Self {
            modifiers: mods,
            key: SharedString::from(canonical),
        })
    }

    /// Formats into the portable canonical spec string (e.g. `"Ctrl+Alt+Space"`).
    pub fn to_spec(&self) -> String {
        let mut parts = Vec::with_capacity(5);
        if self.modifiers.control {
            parts.push("Ctrl");
        }
        if self.modifiers.alt {
            parts.push("Alt");
        }
        if self.modifiers.shift {
            parts.push("Shift");
        }
        if self.modifiers.platform {
            parts.push("Super");
        }
        parts.push(self.key.as_ref());
        parts.join("+")
    }

    /// Formats using native OS display conventions (`⌃⌥⇧⌘C` on macOS, `Ctrl + Alt + C` on Windows/Linux).
    pub fn display_platform(&self) -> String {
        self.display_for_macos(cfg!(target_os = "macos"))
    }

    /// Formats using macOS glyphs (`macos = true`) or Windows/Linux labels (`macos = false`).
    pub fn display_for_macos(&self, macos: bool) -> String {
        if macos {
            let mut out = String::new();
            if self.modifiers.control {
                out.push('⌃');
            }
            if self.modifiers.alt {
                out.push('⌥');
            }
            if self.modifiers.shift {
                out.push('⇧');
            }
            if self.modifiers.platform {
                out.push('⌘');
            }
            out.push_str(display_key_label(self.key.as_ref(), true));
            out
        } else {
            let mut parts: Vec<&str> = Vec::with_capacity(5);
            if self.modifiers.control {
                parts.push("Ctrl");
            }
            if self.modifiers.alt {
                parts.push("Alt");
            }
            if self.modifiers.shift {
                parts.push("Shift");
            }
            if self.modifiers.platform {
                parts.push(if cfg!(target_os = "windows") {
                    "Win"
                } else {
                    "Super"
                });
            }
            parts.push(display_key_label(self.key.as_ref(), false));
            parts.join(" + ")
        }
    }
}

/// Canonicalizes a GPUI or user key token into a portable shortcut key name.
pub fn canonicalize_shortcut_key(raw: &str) -> Option<String> {
    let trimmed = raw.trim();
    if trimmed.is_empty() {
        return None;
    }
    let lower = trimmed.to_ascii_lowercase();
    // Reject bare modifier keys.
    if matches!(
        lower.as_str(),
        "control"
            | "ctrl"
            | "alt"
            | "option"
            | "shift"
            | "super"
            | "meta"
            | "cmd"
            | "command"
            | "win"
            | "windows"
            | "fn"
            | "function"
            | "capslock"
            | "numlock"
            | "scrolllock"
    ) {
        return None;
    }

    if lower.len() == 1 {
        let ch = lower.as_bytes()[0] as char;
        if ch.is_ascii_alphabetic() {
            return Some(ch.to_ascii_uppercase().to_string());
        }
        if ch.is_ascii_digit() {
            return Some(ch.to_string());
        }
    }

    if let Some(num) = lower.strip_prefix('f')
        && !num.is_empty()
        && let Ok(n) = num.parse::<u8>()
        && (1..=24).contains(&n)
    {
        return Some(format!("F{n}"));
    }

    let mapped = match lower.as_str() {
        " " | "space" => "Space",
        "enter" | "return" => "Enter",
        "tab" => "Tab",
        "escape" | "esc" => "Escape",
        "backspace" => "Backspace",
        "delete" | "del" => "Delete",
        "insert" | "ins" => "Insert",
        "home" => "Home",
        "end" => "End",
        "pageup" | "page_up" => "PageUp",
        "pagedown" | "page_down" => "PageDown",
        "up" | "arrowup" => "ArrowUp",
        "down" | "arrowdown" => "ArrowDown",
        "left" | "arrowleft" => "ArrowLeft",
        "right" | "arrowright" => "ArrowRight",
        "`" | "~" | "backquote" | "grave" => "Backquote",
        "-" | "_" | "minus" => "Minus",
        "=" | "+" | "equal" | "equals" => "Equal",
        "[" | "{" | "bracketleft" => "BracketLeft",
        "]" | "}" | "bracketright" => "BracketRight",
        "\\" | "|" | "backslash" => "Backslash",
        ";" | ":" | "semicolon" => "Semicolon",
        "'" | "\"" | "quote" => "Quote",
        "," | "<" | "comma" => "Comma",
        "." | ">" | "period" => "Period",
        "/" | "?" | "slash" => "Slash",
        _ => return None,
    };
    Some(mapped.to_owned())
}

fn is_function_key(canonical: &str) -> bool {
    canonical
        .strip_prefix('F')
        .and_then(|n| n.parse::<u8>().ok())
        .is_some_and(|n| (1..=24).contains(&n))
}

fn display_key_label<'a>(canonical: &'a str, macos: bool) -> &'a str {
    match canonical {
        "Space" => "Space",
        "Enter" => if macos { "↩" } else { "Enter" },
        "Tab" => if macos { "⇥" } else { "Tab" },
        "Escape" => if macos { "⎋" } else { "Esc" },
        "Backspace" => if macos { "⌫" } else { "Backspace" },
        "Delete" => if macos { "⌦" } else { "Del" },
        "ArrowUp" => "↑",
        "ArrowDown" => "↓",
        "ArrowLeft" => "←",
        "ArrowRight" => "→",
        "Backquote" => "`",
        "Minus" => "-",
        "Equal" => "=",
        "BracketLeft" => "[",
        "BracketRight" => "]",
        "Backslash" => "\\",
        "Semicolon" => ";",
        "Quote" => "'",
        "Comma" => ",",
        "Period" => ".",
        "Slash" => "/",
        other => other,
    }
}

/// Transactional manager for named global hotkeys with `suspend` / `resume` support (e.g. while
/// a [`crate::components::ShortcutRecorder`] is capturing a replacement shortcut).
#[derive(Default)]
pub struct GlobalHotKeyManager {
    specs: HashMap<SharedString, GlobalHotKey>,
    registered: HashMap<SharedString, GlobalHotKeyId>,
    id_to_name: HashMap<GlobalHotKeyId, SharedString>,
    suspended: bool,
}

impl GlobalHotKeyManager {
    /// Creates an empty [`GlobalHotKeyManager`].
    pub fn new() -> Self {
        Self::default()
    }

    /// Resolves a fired [`GlobalHotKeyId`] back to its logical action name.
    pub fn name_for_id(&self, id: GlobalHotKeyId) -> Option<&SharedString> {
        self.id_to_name.get(&id)
    }

    /// Returns whether global hotkeys are currently suspended.
    pub fn is_suspended(&self) -> bool {
        self.suspended
    }

    /// Returns the currently configured shortcut for `name`.
    pub fn get(&self, name: &str) -> Option<&GlobalHotKey> {
        self.specs.get(name)
    }

    /// Temporarily unregisters all active OS hotkeys so a shortcut recorder can capture keys
    /// without firing the app's own shortcuts.
    pub fn suspend(&mut self, cx: &App) {
        if self.suspended {
            return;
        }
        self.suspended = true;
        for (_, id) in self.registered.drain() {
            cx.unregister_global_hotkey(id);
        }
        self.id_to_name.clear();
    }

    /// Re-registers all configured shortcuts after [`Self::suspend`]. Returns any registration
    /// failures keyed by action name.
    pub fn resume(&mut self, cx: &App) -> Vec<(SharedString, anyhow::Error)> {
        if !self.suspended {
            return Vec::new();
        }
        self.suspended = false;
        let mut errors = Vec::new();
        for (name, hotkey) in &self.specs {
            match cx.register_global_hotkey(hotkey.clone()) {
                Ok(id) => {
                    self.registered.insert(name.clone(), id);
                    self.id_to_name.insert(id, name.clone());
                }
                Err(err) => errors.push((name.clone(), err)),
            }
        }
        errors
    }

    /// Atomically applies a complete set of named shortcuts.
    ///
    /// Rejects duplicate chords across names. When active (not suspended), registers all new
    /// shortcuts before unregistering replaced ones and rolls back newly registered hotkeys if
    /// any registration fails so the app is never stranded without shortcuts.
    pub fn apply(
        &mut self,
        next_specs: HashMap<SharedString, GlobalHotKey>,
        cx: &App,
    ) -> Result<()> {
        // Reject duplicate key chords within the requested map.
        let mut seen = HashMap::new();
        for (name, hk) in &next_specs {
            if let Some(existing) = seen.insert(hk.clone(), name.clone()) {
                bail!(
                    "shortcut {} is assigned to both '{}' and '{}'",
                    hk.to_spec(),
                    existing,
                    name
                );
            }
        }

        if self.suspended {
            self.specs = next_specs;
            return Ok(());
        }

        // Only register shortcuts that actually changed or are brand new.
        let mut newly_registered: Vec<(SharedString, GlobalHotKeyId)> = Vec::new();
        for (name, hk) in &next_specs {
            if self.specs.get(name) == Some(hk) && self.registered.contains_key(name) {
                continue;
            }
            // If this chord is already owned by a *different* name in our own map, defer until
            // old registrations are swapped cleanly.
            let owned_by_other = self
                .specs
                .iter()
                .any(|(old_name, old_hk)| old_name != name && old_hk == hk);
            if owned_by_other {
                continue;
            }
            match cx.register_global_hotkey(hk.clone()) {
                Ok(id) => newly_registered.push((name.clone(), id)),
                Err(err) => {
                    for (_, id) in newly_registered {
                        cx.unregister_global_hotkey(id);
                    }
                    return Err(err);
                }
            }
        }

        // Unregister old shortcuts that were removed or changed.
        let mut next_registered = HashMap::new();
        let mut next_id_to_name = HashMap::new();

        for (name, old_id) in self.registered.drain() {
            let keep = next_specs
                .get(&name)
                .is_some_and(|hk| self.specs.get(&name) == Some(hk));
            if keep {
                next_registered.insert(name.clone(), old_id);
                next_id_to_name.insert(old_id, name);
            } else {
                cx.unregister_global_hotkey(old_id);
            }
        }
        self.id_to_name.clear();

        for (name, id) in newly_registered {
            next_registered.insert(name.clone(), id);
            next_id_to_name.insert(id, name);
        }

        // Now register any chord that swapped names inside our own map.
        for (name, hk) in &next_specs {
            if !next_registered.contains_key(name) {
                let id = cx.register_global_hotkey(hk.clone())?;
                next_registered.insert(name.clone(), id);
                next_id_to_name.insert(id, name.clone());
            }
        }

        self.specs = next_specs;
        self.registered = next_registered;
        self.id_to_name = next_id_to_name;
        Ok(())
    }

    /// Unregisters all shortcuts managed by this instance.
    pub fn clear(&mut self, cx: &App) {
        for (_, id) in self.registered.drain() {
            cx.unregister_global_hotkey(id);
        }
        self.id_to_name.clear();
        self.specs.clear();
    }
}

/// Configuration for per-user Launch-at-Login / Start-with-Windows registration.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct AutostartOptions {
    /// Registry value name (Windows), LaunchAgent label (macOS), or `.desktop` stem (Linux).
    pub app_name: SharedString,
    /// Optional explicit executable path; defaults to `std::env::current_exe()`.
    pub executable: Option<PathBuf>,
    /// Optional command-line arguments passed on login launch (e.g. `["--background"]`).
    pub args: Vec<String>,
}

impl AutostartOptions {
    /// Creates [`AutostartOptions`] for `app_name` using the current executable.
    pub fn new(app_name: impl Into<SharedString>) -> Self {
        Self {
            app_name: app_name.into(),
            executable: None,
            args: Vec::new(),
        }
    }

    /// Overrides the executable path written into the autostart entry.
    pub fn executable(mut self, path: impl Into<PathBuf>) -> Self {
        self.executable = Some(path.into());
        self
    }

    /// Sets CLI arguments passed when launched at login.
    pub fn args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Resolves the executable path (`self.executable` or `std::env::current_exe()`).
    pub fn resolved_executable(&self) -> Result<PathBuf> {
        match &self.executable {
            Some(path) => Ok(path.clone()),
            None => std::env::current_exe().map_err(Into::into),
        }
    }

    /// Formats the quoted Windows `HKCU\...\Run` command line.
    pub fn windows_command_line(&self) -> Result<String> {
        let exe = self.resolved_executable()?;
        let mut cmd = format!("\"{}\"", exe.display());
        for arg in &self.args {
            cmd.push(' ');
            if arg.contains([' ', '\t', '"']) {
                cmd.push('"');
                cmd.push_str(&arg.replace('"', "\\\""));
                cmd.push('"');
            } else {
                cmd.push_str(arg);
            }
        }
        Ok(cmd)
    }
}

/// Default Unix / macOS implementation of launch-at-login query.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
pub(crate) fn unix_is_launch_at_login(app_name: &str) -> Result<bool> {
    let path = unix_autostart_path(app_name)?;
    Ok(path.exists())
}

/// Default Unix / macOS implementation of launch-at-login toggle.
#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd"))]
pub(crate) fn unix_set_launch_at_login(options: &AutostartOptions, enabled: bool) -> Result<()> {
    let path = unix_autostart_path(options.app_name.as_ref())?;
    if !enabled {
        match std::fs::remove_file(&path) {
            Ok(()) => return Ok(()),
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(err) => return Err(err.into()),
        }
    }

    let exe = options.resolved_executable()?;
    let contents = format_unix_autostart_file(options.app_name.as_ref(), &exe, &options.args);
    gpui_util::atomic_file::atomic_write_str(&path, &contents)?;
    Ok(())
}

#[cfg(target_os = "macos")]
fn unix_autostart_path(app_name: &str) -> Result<PathBuf> {
    let home = std::env::var_os("HOME").ok_or_else(|| anyhow::anyhow!("HOME is not set"))?;
    Ok(PathBuf::from(home)
        .join("Library/LaunchAgents")
        .join(format!("{app_name}.plist")))
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn unix_autostart_path(app_name: &str) -> Result<PathBuf> {
    let base = std::env::var_os("XDG_CONFIG_HOME")
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .ok_or_else(|| anyhow::anyhow!("neither XDG_CONFIG_HOME nor HOME is set"))?;
    Ok(base.join("autostart").join(format!("{app_name}.desktop")))
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd", test))]
fn escape_xml(s: &str) -> String {
    s.replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;")
}

#[cfg(any(target_os = "macos", target_os = "linux", target_os = "freebsd", test))]
fn escape_xdg_exec_arg(arg: &str) -> String {
    let needs_quote = arg.is_empty()
        || arg
            .chars()
            .any(|c| c.is_whitespace() || matches!(c, '"' | '\\' | '$' | '`' | '\'' | ';'));
    if !needs_quote {
        return arg.to_owned();
    }
    let mut out = String::with_capacity(arg.len() + 2);
    out.push('"');
    for ch in arg.chars() {
        if matches!(ch, '"' | '\\' | '$' | '`') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('"');
    out
}

#[cfg(target_os = "macos")]
fn format_unix_autostart_file(app_name: &str, exe: &std::path::Path, args: &[String]) -> String {
    let mut prog_args = String::new();
    prog_args.push_str(&format!(
        "        <string>{}</string>\n",
        escape_xml(&exe.display().to_string())
    ));
    for arg in args {
        prog_args.push_str(&format!(
            "        <string>{}</string>\n",
            escape_xml(arg)
        ));
    }
    format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>Label</key>
    <string>{}</string>
    <key>ProgramArguments</key>
    <array>
{}    </array>
    <key>RunAtLoad</key>
    <true/>
    <key>ProcessType</key>
    <string>Interactive</string>
    <key>LimitLoadToSessionType</key>
    <string>Aqua</string>
</dict>
</plist>
"#,
        escape_xml(app_name),
        prog_args
    )
}

#[cfg(any(target_os = "linux", target_os = "freebsd"))]
fn format_unix_autostart_file(app_name: &str, exe: &std::path::Path, args: &[String]) -> String {
    let mut exec_parts = vec![escape_xdg_exec_arg(&exe.display().to_string())];
    for arg in args {
        exec_parts.push(escape_xdg_exec_arg(arg));
    }
    let _ = escape_xml;
    format!(
        "[Desktop Entry]\nType=Application\nName={app_name}\nExec={}\nX-GNOME-Autostart-enabled=true\nNoDisplay=true\n",
        exec_parts.join(" ")
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn global_hotkey_parse_and_display() {
        let hk = GlobalHotKey::parse("Ctrl+Alt+Space").expect("valid hotkey");
        assert!(hk.modifiers.control && hk.modifiers.alt && !hk.modifiers.shift);
        assert_eq!(hk.key.as_ref(), "Space");
        assert_eq!(hk.to_spec(), "Ctrl+Alt+Space");
        assert_eq!(hk.display_for_macos(true), "⌃⌥Space");
        assert_eq!(hk.display_for_macos(false), "Ctrl + Alt + Space");

        // Function keys are valid standalone; plain letter 'f' is NOT.
        assert!(GlobalHotKey::parse("F12").is_ok());
        assert!(GlobalHotKey::parse("f").is_err());
        assert!(GlobalHotKey::parse("Ctrl").is_err());
    }

    #[test]
    fn global_hotkey_from_keystroke_filters_bare_and_plain_keys() {
        let bare = Keystroke::parse("a").unwrap();
        assert!(GlobalHotKey::from_keystroke(&bare).is_none());

        let bare_f = Keystroke::parse("f").unwrap();
        assert!(GlobalHotKey::from_keystroke(&bare_f).is_none());

        let f5 = Keystroke::parse("f5").unwrap();
        assert_eq!(GlobalHotKey::from_keystroke(&f5).unwrap().to_spec(), "F5");

        let combo = Keystroke::parse("ctrl-alt-a").unwrap();
        assert_eq!(
            GlobalHotKey::from_keystroke(&combo).unwrap().to_spec(),
            "Ctrl+Alt+A"
        );
    }

    #[test]
    fn file_filter_windows_spec() {
        let filter = FileFilter::new("Images", ["png", ".jpg", "webp"]);
        assert_eq!(filter.windows_spec(), "*.png;*.jpg;*.webp");
    }

    #[test]
    fn autostart_windows_command_line_quotes_paths_with_spaces() {
        let opts = AutostartOptions::new("MyApp")
            .executable(r"C:\Program Files\My App\my-app.exe")
            .args(["--background", "--profile", "my profile"]);
        assert_eq!(
            opts.windows_command_line().unwrap(),
            r#""C:\Program Files\My App\my-app.exe" --background --profile "my profile""#
        );
    }

    #[test]
    fn xdg_and_xml_escaping() {
        assert_eq!(escape_xml("A & <B>"), "A &amp; &lt;B&gt;");
        assert_eq!(
            escape_xdg_exec_arg("/opt/My App/$bin"),
            "\"/opt/My App/\\$bin\""
        );
    }
}

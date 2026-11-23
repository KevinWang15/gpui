//! Interactive keyboard shortcut / global hotkey recorder ([`ShortcutRecorder`]).
//!
//! Displays the currently bound [`GlobalHotKey`] in OS-native notation (`⌃⌥⌘A` on macOS,
//! `Ctrl + Alt + A` on Windows/Linux), enters capture mode on click or `Enter`/`Space`,
//! cancels cleanly on `Escape` or focus loss, validates chords (rejecting bare modifiers and
//! unmodified typing keys while allowing `F1`–`F24`), and surfaces inline conflict errors.

use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    App, Context, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, GlobalHotKey,
    IntoElement, KeyDownEvent, Render, RenderOnce, SharedString, StyleRefinement, Styled, Window,
    div, prelude::*, px,
};

use super::focus::tab_stop;
use super::theme::{ActiveTheme, ComponentSize};

/// Events emitted by [`ShortcutRecorderState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum ShortcutRecorderEvent {
    /// Recording started — apps using [`crate::GlobalHotKeyManager`] should call `suspend(cx)`
    /// so pressing the currently active shortcut records it instead of triggering its action.
    StartRecording,
    /// Recording ended without changing the shortcut (`Escape`, blur, or window deactivation).
    CancelRecording,
    /// A new shortcut was recorded (`Some`) or cleared (`None`).
    Change(Option<GlobalHotKey>),
}

/// Entity state backing a [`ShortcutRecorder`].
pub struct ShortcutRecorderState {
    focus_handle: FocusHandle,
    shortcut: Option<GlobalHotKey>,
    recording: bool,
    clearable: bool,
    disabled: bool,
    placeholder: SharedString,
    recording_prompt: SharedString,
    error: Option<SharedString>,
    size: ComponentSize,
    validator: Option<Rc<dyn Fn(&GlobalHotKey, &App) -> Result<(), SharedString> + 'static>>,
}

impl EventEmitter<ShortcutRecorderEvent> for ShortcutRecorderState {}

impl Focusable for ShortcutRecorderState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ShortcutRecorderState {
    /// Creates a new [`ShortcutRecorderState`] initialized with `shortcut`.
    pub fn new(shortcut: Option<GlobalHotKey>, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        Self {
            focus_handle,
            shortcut,
            recording: false,
            clearable: true,
            disabled: false,
            placeholder: "Click to record shortcut".into(),
            recording_prompt: "Press shortcut… (Esc to cancel)".into(),
            error: None,
            size: ComponentSize::Md,
            validator: None,
        }
    }

    /// Sets whether the user can clear the shortcut back to `None`.
    pub fn clearable(mut self, clearable: bool) -> Self {
        self.clearable = clearable;
        self
    }

    /// Disables recording and clearing.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Sets the placeholder shown when no shortcut is bound.
    pub fn placeholder(mut self, text: impl Into<SharedString>) -> Self {
        self.placeholder = text.into();
        self
    }

    /// Sets the prompt displayed while actively capturing a key chord.
    pub fn recording_prompt(mut self, text: impl Into<SharedString>) -> Self {
        self.recording_prompt = text.into();
        self
    }

    /// Sets the component size preset.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Registers a synchronous validator (for example probing [`App::is_global_hotkey_available`])
    /// returning `Err(reason)` to reject a shortcut and display an inline error message.
    pub fn validator(
        mut self,
        f: impl Fn(&GlobalHotKey, &App) -> Result<(), SharedString> + 'static,
    ) -> Self {
        self.validator = Some(Rc::new(f));
        self
    }

    /// Returns the currently bound shortcut, if any.
    pub fn shortcut(&self) -> Option<&GlobalHotKey> {
        self.shortcut.as_ref()
    }

    /// Returns whether the recorder is actively listening for a keystroke.
    pub fn is_recording(&self) -> bool {
        self.recording
    }

    /// Returns the current validation/conflict error message, if any.
    pub fn error(&self) -> Option<&SharedString> {
        self.error.as_ref()
    }

    /// Updates the bound shortcut programmatically without emitting [`ShortcutRecorderEvent::Change`].
    pub fn set_shortcut(&mut self, shortcut: Option<GlobalHotKey>, cx: &mut Context<Self>) {
        self.shortcut = shortcut;
        self.error = None;
        cx.notify();
    }

    /// Sets or clears an external error message (e.g. from an async OS hotkey registration conflict).
    pub fn set_error(&mut self, error: Option<SharedString>, cx: &mut Context<Self>) {
        self.error = error;
        cx.notify();
    }

    /// Enters recording mode and focuses the recorder pill.
    pub fn start_recording(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled || self.recording {
            return;
        }
        self.recording = true;
        self.error = None;
        window.focus(&self.focus_handle, cx);
        cx.emit(ShortcutRecorderEvent::StartRecording);
        cx.notify();
    }

    /// Cancels an active recording session without altering the bound shortcut.
    pub fn cancel_recording(&mut self, cx: &mut Context<Self>) {
        if !self.recording {
            return;
        }
        self.recording = false;
        cx.emit(ShortcutRecorderEvent::CancelRecording);
        cx.notify();
    }

    /// Clears the bound shortcut to `None`.
    pub fn clear(&mut self, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if self.recording {
            self.recording = false;
            cx.emit(ShortcutRecorderEvent::CancelRecording);
        }
        if self.shortcut.is_some() {
            self.shortcut = None;
            self.error = None;
            cx.emit(ShortcutRecorderEvent::Change(None));
            cx.notify();
        }
    }

    fn handle_key_down(
        &mut self,
        event: &KeyDownEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.disabled || event.is_held {
            return;
        }
        let ks = &event.keystroke;
        if !self.recording {
            if !ks.modifiers.modified() && matches!(ks.key.as_str(), "enter" | "space") {
                self.start_recording(window, cx);
                cx.stop_propagation();
            } else if !ks.modifiers.modified()
                && self.clearable
                && matches!(ks.key.as_str(), "backspace" | "delete")
            {
                self.clear(cx);
                cx.stop_propagation();
            }
            return;
        }

        // Stop propagation for every key while recording so ancestor window bindings never steal keys.
        cx.stop_propagation();

        if !ks.modifiers.modified() && ks.key.as_str() == "escape" {
            self.cancel_recording(cx);
            return;
        }

        let Some(candidate) = GlobalHotKey::from_keystroke(ks) else {
            // Bare modifiers or plain typing keys keep recording active until a valid chord arrives.
            if !matches!(
                ks.key.as_str(),
                "control" | "alt" | "shift" | "platform" | "super" | "meta" | "function"
            ) {
                self.error = Some("Include a modifier key (Ctrl, Alt, Shift, Win/Cmd) or use F1–F24.".into());
                cx.notify();
            }
            return;
        };

        if let Some(validator) = &self.validator {
            if let Err(reason) = validator(&candidate, cx) {
                self.error = Some(reason);
                cx.notify();
                return;
            }
        }

        self.recording = false;
        self.error = None;
        self.shortcut = Some(candidate.clone());
        cx.emit(ShortcutRecorderEvent::Change(Some(candidate)));
        cx.notify();
    }
}

impl Render for ShortcutRecorderState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_focused = self.focus_handle.is_focused(window);
        // If focus left this recorder while recording, cancel cleanly.
        if self.recording && !is_focused {
            self.recording = false;
            cx.emit(ShortcutRecorderEvent::CancelRecording);
        }

        let (height, px_x, text_sz) = match self.size {
            ComponentSize::Xs => (px(28.), px(8.), px(12.)),
            ComponentSize::Sm => (px(32.), px(10.), px(12.5)),
            ComponentSize::Md => (px(36.), px(12.), px(13.)),
            ComponentSize::Lg => (px(40.), px(14.), px(14.)),
            ComponentSize::Xl => (px(44.), px(16.), px(15.)),
        };

        let border_color = if self.error.is_some() {
            theme.destructive
        } else if self.recording {
            theme.primary
        } else if is_focused {
            theme.ring
        } else {
            theme.input_border
        };

        let bg_color = if self.disabled {
            theme.disabled_bg
        } else if self.recording {
            theme.primary_soft.opacity(0.65)
        } else {
            theme.input_bg
        };

        let label_text: SharedString = if self.recording {
            self.recording_prompt.clone()
        } else if let Some(hk) = &self.shortcut {
            SharedString::from(hk.display_platform())
        } else {
            self.placeholder.clone()
        };

        let label_color = if self.disabled {
            theme.disabled_foreground
        } else if self.recording {
            theme.primary_soft_foreground
        } else if self.shortcut.is_some() {
            theme.foreground
        } else {
            theme.placeholder
        };

        let show_clear = self.clearable && !self.disabled && self.shortcut.is_some() && !self.recording;
        let entity_id = cx.entity_id();

        let pill = tab_stop(
            div()
                .id(("shortcut-recorder", entity_id))
                .track_focus(&self.focus_handle),
            !self.disabled,
            &theme,
        )
        .flex()
        .flex_row()
        .items_center()
        .justify_between()
        .gap(px(8.))
        .h(height)
        .min_w(px(210.))
        .px(px_x)
        .rounded(theme.radius_md)
        .bg(bg_color)
        .border_1()
        .border_color(border_color)
        .when(!self.disabled, |el| {
            let hover_border = if self.recording {
                theme.primary
            } else {
                theme.input_border_hover
            };
            el.cursor_pointer()
                .hover(move |s| s.border_color(hover_border))
                .on_click(cx.listener(|this, _, window, cx| {
                    if this.recording {
                        this.cancel_recording(cx);
                    } else {
                        this.start_recording(window, cx);
                    }
                }))
                .on_key_down(cx.listener(Self::handle_key_down))
        })
        .when(self.disabled, |el| el.cursor_not_allowed().opacity(0.6))
        .child(
            div()
                .flex()
                .flex_row()
                .items_center()
                .gap(px(8.))
                .min_w_0()
                .child(
                    Icon::new(if self.recording {
                        LucideIcon::Radio
                    } else {
                        LucideIcon::Keyboard
                    })
                    .size(IconSize::Sm)
                    .color(if self.recording {
                        theme.primary
                    } else {
                        theme.muted_foreground
                    }),
                )
                .child(
                    div()
                        .text_size(text_sz)
                        .font_weight(if self.shortcut.is_some() || self.recording {
                            FontWeight::MEDIUM
                        } else {
                            FontWeight::NORMAL
                        })
                        .when_some(theme.mono_font_family.clone(), |el, mono| {
                            if self.shortcut.is_some() && !self.recording {
                                el.font_family(mono)
                            } else {
                                el
                            }
                        })
                        .text_color(label_color)
                        .truncate()
                        .child(label_text),
                ),
        )
        .when(show_clear, |el| {
            let hover_bg = theme.surface_hover;
            el.child(
                div()
                    .id(("shortcut-clear", entity_id))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(20.))
                    .h(px(20.))
                    .rounded(theme.radius_sm)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_mouse_down(crate::MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(|this, _, _, cx| {
                        cx.stop_propagation();
                        this.clear(cx);
                    }))
                    .child(
                        Icon::new(LucideIcon::X)
                            .size(IconSize::Xs)
                            .color(theme.muted_foreground),
                    ),
            )
        });

        div()
            .flex()
            .flex_col()
            .gap(px(4.))
            .child(pill)
            .when_some(self.error.clone(), |el, err| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(4.))
                        .text_size(px(11.5))
                        .text_color(theme.destructive)
                        .child(
                            Icon::new(LucideIcon::CircleExclamationPoint)
                                .size(IconSize::Xs)
                                .color(theme.destructive),
                        )
                        .child(err),
                )
            })
    }
}

/// Interactive keyboard shortcut recorder element bound to a [`ShortcutRecorderState`].
#[derive(IntoElement)]
pub struct ShortcutRecorder {
    state: Entity<ShortcutRecorderState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ShortcutRecorder::new`].
pub fn shortcut_recorder(state: &Entity<ShortcutRecorderState>) -> ShortcutRecorder {
    ShortcutRecorder::new(state)
}

impl ShortcutRecorder {
    /// Creates a [`ShortcutRecorder`] bound to `state`.
    pub fn new(state: &Entity<ShortcutRecorderState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for ShortcutRecorder {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ShortcutRecorder {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut wrapper = div().child(self.state);
        crate::Refineable::refine(wrapper.style(), &self.style);
        wrapper
    }
}

//! Modal dialogs supporting both **In-Window Overlay** ([`Dialog`]) and **Native OS Window**
//! ([`DialogWindow`], backed by [`WindowKind::Dialog`]) presentation modes.
//!
//! Both modes share the same visual design (header icon badge, title, muted description,
//! body slot, and footer actions) and read the active global [`Theme`], making it
//! easy to switch or compare in-window modal sheets against true OS-level modal dialog windows.

use std::cell::Cell;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    Animation, AnimationExt, AnyElement, App, Bounds, ClickEvent, Context, DisplayId, ElementId,
    FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton, ParentElement,
    Pixels, Render, RenderOnce, SharedString, StyleRefinement, Styled, TitlebarOptions, Window,
    WindowBackgroundAppearance, WindowBounds, WindowControlArea, WindowDecorations, WindowHandle,
    WindowKind, WindowOptions, deferred, div, ease_out_quint, prelude::*, px, size,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::button::{Button, ButtonVariant};
use super::theme::{ANIM_NORMAL, ActiveTheme, Theme};

/// Controls whether an overlay component (such as a dialog or toast notification) renders inside
/// the parent window's deferred layer or in its own dedicated OS-level window.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum OverlayPresentation {
    /// Renders inside the parent window using GPUI's `deferred(...)` overlay layer.
    #[default]
    InWindow,
    /// Renders as a dedicated native OS window (`WindowKind::Dialog` for modal dialogs,
    /// `WindowKind::PopUp` for floating desktop toast notifications).
    NativeWindow,
}

impl OverlayPresentation {
    /// Human-readable label for display in settings and UI controls.
    pub const fn label(self) -> &'static str {
        match self {
            Self::InWindow => "In-Window Overlay",
            Self::NativeWindow => "Native OS Window",
        }
    }
}

/// Width presets for [`Dialog`] and [`DialogWindow`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum DialogSize {
    /// `384px` (`max-w-sm`).
    Sm,
    /// `480px` (`max-w-md`).
    #[default]
    Md,
    /// `576px` (`max-w-lg`).
    Lg,
    /// `720px` (`max-w-2xl`).
    Xl,
}

impl DialogSize {
    /// Returns the dialog width in logical pixels.
    pub const fn width(self) -> Pixels {
        match self {
            Self::Sm => px(384.),
            Self::Md => px(480.),
            Self::Lg => px(576.),
            Self::Xl => px(720.),
        }
    }
}

/// Renders the shared dialog header (icon badge, title, muted description, close button).
#[allow(clippy::too_many_arguments)]
fn render_dialog_header(
    close_id: ElementId,
    title: Option<SharedString>,
    description: Option<SharedString>,
    icon: Option<LucideIcon>,
    destructive: bool,
    show_close_button: bool,
    draggable_window_header: bool,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    theme: &Theme,
) -> impl IntoElement {
    let icon_bg = if destructive {
        theme.destructive_soft
    } else {
        theme.primary_soft
    };
    let icon_fg = if destructive {
        theme.destructive
    } else {
        theme.primary
    };
    let hover_bg = theme.surface_hover;

    div()
        .flex()
        .flex_row()
        .items_start()
        .justify_between()
        .gap(px(12.))
        .px(px(20.))
        .pt(px(20.))
        .pb(px(12.))
        .when(draggable_window_header, |el| {
            el.window_control_area(WindowControlArea::Drag)
                .on_mouse_down(MouseButton::Left, |_, window, _| {
                    window.start_window_move();
                })
        })
        .child(
            div()
                .flex()
                .flex_row()
                .items_start()
                .gap(px(12.))
                .flex_1()
                .min_w_0()
                .when_some(icon, |el, icon| {
                    el.child(
                        div()
                            .flex()
                            .items_center()
                            .justify_center()
                            .flex_shrink_0()
                            .w(px(38.))
                            .h(px(38.))
                            .rounded_full()
                            .bg(icon_bg)
                            .child(Icon::new(icon).size(IconSize::Lg).color(icon_fg)),
                    )
                })
                .child(
                    div()
                        .flex()
                        .flex_col()
                        .gap(px(4.))
                        .flex_1()
                        .min_w_0()
                        .when_some(title, |el, title| {
                            el.child(
                                div()
                                    .text_size(px(17.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.popover_foreground)
                                    .child(title),
                            )
                        })
                        .when_some(description, |el, desc| {
                            el.child(
                                div()
                                    .text_size(px(13.5))
                                    .text_color(theme.muted_foreground)
                                    .child(desc),
                            )
                        }),
                ),
        )
        .when(show_close_button, |el| {
            el.child(
                div()
                    .id(close_id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .p(px(6.))
                    .rounded(theme.radius_md)
                    .cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .when_some(on_close, |el, cb| {
                        el.on_click(move |_, window, cx| cb(&(), window, cx))
                    })
                    .child(
                        Icon::new(LucideIcon::X)
                            .size(IconSize::Md)
                            .color(theme.muted_foreground),
                    ),
            )
        })
}

/// Modal dialog rendered inside the parent window's top-priority deferred overlay layer.
///
/// To present a dialog as a separate OS-level modal window instead, see [`DialogWindow`].
#[derive(IntoElement)]
pub struct Dialog {
    id: ElementId,
    open: bool,
    title: Option<SharedString>,
    description: Option<SharedString>,
    icon: Option<LucideIcon>,
    destructive: bool,
    size: DialogSize,
    dismiss_on_backdrop: bool,
    show_close_button: bool,
    backdrop_blur: Pixels,
    children: SmallVec<[AnyElement; 4]>,
    footer: SmallVec<[AnyElement; 3]>,
    on_close: Option<Rc<dyn Fn(&(), &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Dialog::new`] (in-window deferred modal overlay).
pub fn dialog(id: impl Into<ElementId>, open: bool) -> Dialog {
    Dialog::new(id, open)
}

impl Dialog {
    /// Creates a new in-window modal dialog; renders nothing when `open` is `false`.
    pub fn new(id: impl Into<ElementId>, open: bool) -> Self {
        Self {
            id: id.into(),
            open,
            title: None,
            description: None,
            icon: None,
            destructive: false,
            size: DialogSize::Md,
            dismiss_on_backdrop: true,
            show_close_button: true,
            backdrop_blur: px(10.),
            children: SmallVec::new(),
            footer: SmallVec::new(),
            on_close: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the dialog heading.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets muted description text beneath the title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a leading circular accent icon badge in the dialog header.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Styles the header icon badge in destructive red tones (an alert dialog).
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// Sets the dialog width preset.
    pub fn size(mut self, size: DialogSize) -> Self {
        self.size = size;
        self
    }

    /// Controls whether clicking the dark backdrop invokes `on_close` (default `true`).
    pub fn dismiss_on_backdrop(mut self, dismiss: bool) -> Self {
        self.dismiss_on_backdrop = dismiss;
        self
    }

    /// Controls visibility of the top-right `X` close button (default `true`).
    pub fn show_close_button(mut self, show: bool) -> Self {
        self.show_close_button = show;
        self
    }

    /// Sets the GPU frosted-glass backdrop blur radius behind the modal overlay (default `10px`).
    ///
    /// Pass `px(0.)` to disable backdrop blur and render only the flat dimmed scrim.
    pub fn backdrop_blur(mut self, radius: impl Into<Pixels>) -> Self {
        self.backdrop_blur = radius.into();
        self
    }

    /// Appends a footer action element (e.g. Cancel / Confirm buttons).
    pub fn footer_action(mut self, action: impl IntoElement) -> Self {
        self.footer.push(action.into_any_element());
        self
    }

    /// Registers the close callback invoked by backdrop clicks, close button, or Escape key.
    ///
    /// Accepts `cx.listener(|this, _, window, cx| ...)` directly.
    pub fn on_close(mut self, handler: impl Fn(&(), &mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for Dialog {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for Dialog {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Dialog {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        // Takes focus when it opens, keeps Tab inside, gives focus back when
        // it closes (`focus::modal_focus`).
        let focus = super::focus::modal_focus((self.id.clone(), "focus"), self.open, window, cx);
        if !self.open {
            return div().into_any_element();
        }
        let theme = cx.theme();
        let on_close_backdrop = self.on_close.clone();
        let on_close_btn = self.on_close.clone();
        let on_close_key = self.on_close;

        let mut panel = div()
            .id((self.id.clone(), "panel"))
            .track_focus(&focus)
            .focus_trap()
            .flex()
            .flex_col()
            .w(self.size.width())
            .max_w_full()
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .shadow_2xl()
            .overflow_hidden()
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .on_click(|_: &ClickEvent, _, cx| cx.stop_propagation());

        if self.title.is_some() || self.description.is_some() || self.show_close_button {
            panel = panel.child(render_dialog_header(
                (self.id.clone(), "close").into(),
                self.title,
                self.description,
                self.icon,
                self.destructive,
                self.show_close_button,
                false,
                on_close_btn,
                &theme,
            ));
        }

        if !self.children.is_empty() {
            panel = panel.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .px(px(20.))
                    .py(px(12.))
                    .children(self.children),
            );
        }

        if !self.footer.is_empty() {
            panel = panel.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.surface_muted.opacity(0.45))
                    .children(self.footer),
            );
        }

        crate::Refineable::refine(panel.style(), &self.style);

        let panel_anim_id = (self.id.clone(), "panel-anim");
        let backdrop_anim_id = (self.id.clone(), "backdrop-anim");
        let backdrop_color = theme.backdrop;
        let backdrop_blur = self.backdrop_blur;

        let animated_panel = panel.with_animation(
            panel_anim_id,
            Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
            |el, delta| el.opacity(delta).mt(px(14.0 * (1.0 - delta))),
        );

        let overlay = div()
            .id(self.id)
            .absolute()
            .inset_0()
            .occlude()
            .flex()
            .items_center()
            .justify_center()
            .p(px(20.))
            .when(self.dismiss_on_backdrop, |el| {
                el.when_some(on_close_backdrop, |el, cb| {
                    el.on_click(move |_, window, cx| cb(&(), window, cx))
                })
            })
            .when_some(on_close_key, |el, cb| {
                el.on_key_down(move |ev: &KeyDownEvent, window, cx| {
                    if ev.keystroke.key == "escape" {
                        cb(&(), window, cx);
                    }
                })
            })
            .child(animated_panel)
            .with_animation(
                backdrop_anim_id,
                Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
                move |el, delta| {
                    el.bg(backdrop_color.opacity(delta))
                        .when(backdrop_blur > px(0.), |el| {
                            el.backdrop_blur(px(backdrop_blur.0 * delta))
                        })
                },
            );

        deferred(overlay).priority(50).into_any_element()
    }
}

// ============================================================================
// Native OS Window Dialog (`WindowKind::Dialog`)
// ============================================================================

/// Footer button specification rendered inside a [`DialogWindow`].
#[derive(Clone)]
pub struct DialogWindowAction {
    /// Button text label.
    pub label: SharedString,
    /// Visual [`ButtonVariant`] (e.g. `Secondary`, `Primary`, `Destructive`).
    pub variant: ButtonVariant,
    /// Whether clicking this action closes the native dialog window automatically (default `true`).
    pub closes_window: bool,
    /// Optional click handler invoked with the dialog's `Window` and `App`.
    pub on_click: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
}

impl DialogWindowAction {
    /// Creates a new [`DialogWindowAction`] that closes the dialog window when clicked.
    pub fn new(label: impl Into<SharedString>, variant: ButtonVariant) -> Self {
        Self {
            label: label.into(),
            variant,
            closes_window: true,
            on_click: None,
        }
    }

    /// Sets whether clicking this action automatically closes the dialog window (default `true`).
    pub fn closes_window(mut self, closes: bool) -> Self {
        self.closes_window = closes;
        self
    }

    /// Attaches a click callback executed before closing the dialog window.
    pub fn on_click(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_click = Some(Rc::new(handler));
        self
    }
}

/// Builder for opening a modal dialog in a dedicated native OS window ([`WindowKind::Dialog`]).
///
/// Compared with the in-window [`Dialog`] overlay:
/// - Receives native OS modality (`_NET_WM_STATE_MODAL` on X11, `xdg_dialog` on Wayland,
///   sheet/panel on macOS, `WS_EX_DLGMODALFRAME` + disabled parent HWND on Windows).
/// - Can be dragged freely anywhere on the desktop, even outside a small parent window.
/// - Synchronizes live with global [`Theme`] changes via `cx.refresh_windows()`.
pub struct DialogWindow {
    title: Option<SharedString>,
    window_title: Option<SharedString>,
    description: Option<SharedString>,
    icon: Option<LucideIcon>,
    destructive: bool,
    size: DialogSize,
    height: Option<Pixels>,
    show_close_button: bool,
    native_titlebar: bool,
    display_id: Option<DisplayId>,
    parent_bounds: Option<Bounds<Pixels>>,
    actions: Vec<DialogWindowAction>,
    body: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>>,
    footer: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`DialogWindow::new`].
pub fn dialog_window() -> DialogWindow {
    DialogWindow::new()
}

impl Default for DialogWindow {
    fn default() -> Self {
        Self::new()
    }
}

impl DialogWindow {
    /// Creates a new [`DialogWindow`] builder.
    pub fn new() -> Self {
        Self {
            title: None,
            window_title: None,
            description: None,
            icon: None,
            destructive: false,
            size: DialogSize::Md,
            height: None,
            show_close_button: true,
            native_titlebar: false,
            display_id: None,
            parent_bounds: None,
            actions: Vec::new(),
            body: None,
            footer: None,
            on_close: None,
        }
    }

    /// Sets the dialog heading (also used as the OS window title unless [`Self::window_title`] is set).
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets an explicit OS window titlebar caption.
    pub fn window_title(mut self, title: impl Into<SharedString>) -> Self {
        self.window_title = Some(title.into());
        self
    }

    /// Sets muted description text beneath the title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Adds a leading circular accent icon badge in the dialog header.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Styles the header icon badge in destructive red tones.
    pub fn destructive(mut self, destructive: bool) -> Self {
        self.destructive = destructive;
        self
    }

    /// Sets the dialog width preset (`Sm`, `Md`, `Lg`, `Xl`).
    pub fn size(mut self, size: DialogSize) -> Self {
        self.size = size;
        self
    }

    /// Sets an explicit OS window height in logical pixels.
    ///
    /// When omitted, a compact height is calculated automatically from the title, description,
    /// optional body, and footer actions.
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = Some(height);
        self
    }

    /// Controls visibility of the top-right `X` close button inside the dialog header (default `true`).
    pub fn show_close_button(mut self, show: bool) -> Self {
        self.show_close_button = show;
        self
    }

    /// Controls whether the OS renders a standard native window titlebar (`true`) or whether
    /// the dialog renders as borderless custom chrome draggable by its header (`false`, default).
    pub fn native_titlebar(mut self, enabled: bool) -> Self {
        self.native_titlebar = enabled;
        self
    }

    /// Centers the native dialog window over `parent_window` and inherits its display.
    pub fn center_over(mut self, parent_window: &Window, cx: &App) -> Self {
        self.parent_bounds = Some(parent_window.bounds());
        self.display_id = parent_window.display(cx).map(|d| d.id());
        self
    }

    /// Explicitly selects the display on which to open the dialog window.
    pub fn display(mut self, display_id: DisplayId) -> Self {
        self.display_id = Some(display_id);
        self
    }

    /// Sets a dynamic body content renderer inside the dialog window.
    pub fn body(mut self, render_body: impl Fn(&mut Window, &mut App) -> AnyElement + 'static) -> Self {
        self.body = Some(Rc::new(render_body));
        self
    }

    /// Sets a custom footer renderer (replaces declarative [`Self::action`] buttons when provided).
    pub fn footer(
        mut self,
        render_footer: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        self.footer = Some(Rc::new(render_footer));
        self
    }

    /// Appends a declarative footer [`DialogWindowAction`] button.
    pub fn action(mut self, action: DialogWindowAction) -> Self {
        self.actions.push(action);
        self
    }

    /// Convenience helper appending a secondary Cancel button that dismisses the dialog window.
    pub fn cancel_button(mut self, label: impl Into<SharedString>) -> Self {
        self.actions
            .push(DialogWindowAction::new(label, ButtonVariant::Secondary));
        self
    }

    /// Convenience helper appending a Primary (or Destructive if [`Self::destructive`] is set)
    /// confirmation button that runs `on_confirm` and closes the dialog window.
    pub fn confirm_button(
        mut self,
        label: impl Into<SharedString>,
        on_confirm: impl Fn(&mut Window, &mut App) + 'static,
    ) -> Self {
        let variant = if self.destructive {
            ButtonVariant::Destructive
        } else {
            ButtonVariant::Primary
        };
        self.actions
            .push(DialogWindowAction::new(label, variant).on_click(on_confirm));
        self
    }

    /// Registers a callback invoked once whenever the dialog window is dismissed (via Escape,
    /// header close button, Cancel button, or OS window close).
    pub fn on_close(mut self, handler: impl Fn(&mut Window, &mut App) + 'static) -> Self {
        self.on_close = Some(Rc::new(handler));
        self
    }

    fn resolved_size(&self) -> crate::Size<Pixels> {
        let width = self.size.width();
        let height = self.height.unwrap_or_else(|| {
            let mut h = 92.0_f32;
            if let Some(desc) = &self.description {
                let chars_per_line = ((width.0 - 104.0) / 7.2).max(24.0);
                let lines = ((desc.len() as f32) / chars_per_line).ceil().max(1.0);
                h += lines * 20.0 + 10.0;
            }
            if self.body.is_some() {
                h += 88.0;
            }
            if !self.actions.is_empty() || self.footer.is_some() {
                h += 64.0;
            }
            px(h.clamp(168.0, 560.0))
        });
        size(width, height)
    }

    /// Opens the configured dialog as a dedicated native OS [`WindowKind::Dialog`] window.
    pub fn open(self, cx: &mut App) -> anyhow::Result<WindowHandle<DialogWindowView>> {
        let win_size = self.resolved_size();
        let window_bounds = if let Some(parent) = self.parent_bounds {
            let mut b = Bounds::centered_at(parent.center(), win_size);
            if let Some(display) = self
                .display_id
                .and_then(|id| cx.find_display(id))
                .or_else(|| cx.primary_display())
            {
                let vis = display.visible_bounds();
                let max_x = (vis.origin.x + vis.size.width - b.size.width).max(vis.origin.x);
                let max_y = (vis.origin.y + vis.size.height - b.size.height).max(vis.origin.y);
                b.origin.x = b.origin.x.clamp(vis.origin.x, max_x);
                b.origin.y = b.origin.y.clamp(vis.origin.y, max_y);
            }
            WindowBounds::Windowed(b)
        } else {
            WindowBounds::Windowed(Bounds::centered(self.display_id, win_size, cx))
        };

        let os_title = self
            .window_title
            .clone()
            .or_else(|| self.title.clone())
            .unwrap_or_else(|| SharedString::new_static("Dialog"));

        let (titlebar, decorations) = if self.native_titlebar {
            (
                Some(TitlebarOptions {
                    title: Some(os_title),
                    appears_transparent: false,
                    traffic_light_position: None,
                }),
                Some(WindowDecorations::Server),
            )
        } else {
            (None, Some(WindowDecorations::Client))
        };

        let options = WindowOptions {
            window_bounds: Some(window_bounds),
            titlebar,
            focus: true,
            show: true,
            kind: WindowKind::Dialog,
            is_movable: true,
            is_resizable: false,
            is_minimizable: false,
            display_id: self.display_id,
            window_background: WindowBackgroundAppearance::Opaque,
            window_decorations: decorations,
            ..Default::default()
        };

        let on_close_fired = Rc::new(Cell::new(false));
        let on_close_should_close = self.on_close.clone();
        let fired_should_close = on_close_fired.clone();

        cx.open_window(options, move |window, cx| {
            window.on_window_should_close(cx, move |window, cx| {
                if !fired_should_close.replace(true)
                    && let Some(cb) = &on_close_should_close
                {
                    cb(window, cx);
                }
                true
            });

            let view = cx.new(|cx| {
                let focus_handle = cx.focus_handle();
                DialogWindowView {
                    focus_handle,
                    title: self.title,
                    description: self.description,
                    icon: self.icon,
                    destructive: self.destructive,
                    show_close_button: self.show_close_button,
                    native_titlebar: self.native_titlebar,
                    actions: self.actions,
                    body: self.body,
                    footer: self.footer,
                    on_close: self.on_close,
                    on_close_fired,
                }
            });
            let focus_handle = view.read(cx).focus_handle.clone();
            window.focus(&focus_handle, cx);
            view
        })
    }
}

/// Root GPUI view hosted inside a native OS [`DialogWindow`].
pub struct DialogWindowView {
    focus_handle: FocusHandle,
    title: Option<SharedString>,
    description: Option<SharedString>,
    icon: Option<LucideIcon>,
    destructive: bool,
    show_close_button: bool,
    native_titlebar: bool,
    actions: Vec<DialogWindowAction>,
    body: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>>,
    footer: Option<Rc<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>>,
    on_close: Option<Rc<dyn Fn(&mut Window, &mut App) + 'static>>,
    on_close_fired: Rc<Cell<bool>>,
}

impl Focusable for DialogWindowView {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl DialogWindowView {
    fn dismiss(&self, fire_on_close: bool, window: &mut Window, cx: &mut App) {
        if fire_on_close {
            if !self.on_close_fired.replace(true)
                && let Some(cb) = &self.on_close
            {
                cb(window, cx);
            }
        } else {
            self.on_close_fired.set(true);
        }
        window.remove_window();
    }
}

impl Render for DialogWindowView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let on_close_cb = self.on_close.clone();
        let on_close_fired = self.on_close_fired.clone();

        let dismiss_cb: Rc<dyn Fn(&(), &mut Window, &mut App)> = Rc::new({
            let on_close_cb = on_close_cb.clone();
            let on_close_fired = on_close_fired.clone();
            move |_, window, cx| {
                if !on_close_fired.replace(true)
                    && let Some(cb) = &on_close_cb
                {
                    cb(window, cx);
                }
                window.remove_window();
            }
        });

        let mut root = div()
            .id("gpui-native-dialog-window")
            .track_focus(&self.focus_handle)
            .size_full()
            .flex()
            .flex_col()
            .justify_between()
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .when(!self.native_titlebar, |el| {
                el.border_1().border_color(theme.input_border)
            })
            .on_key_down(cx.listener(|this, ev: &KeyDownEvent, window, cx| {
                if ev.keystroke.key == "escape" {
                    this.dismiss(true, window, cx);
                }
            }));

        let mut top_section = div().flex().flex_col().flex_1().min_h_0();
        if self.title.is_some() || self.description.is_some() || self.show_close_button {
            top_section = top_section.child(render_dialog_header(
                "native-dlg-close".into(),
                self.title.clone(),
                self.description.clone(),
                self.icon,
                self.destructive,
                self.show_close_button,
                !self.native_titlebar,
                Some(dismiss_cb),
                &theme,
            ));
        }
        if let Some(render_body) = &self.body {
            let body_el = render_body(window, cx);
            top_section = top_section.child(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(12.))
                    .px(px(20.))
                    .py(px(10.))
                    .child(body_el),
            );
        }
        root = root.child(top_section);

        if let Some(render_footer) = &self.footer {
            let footer_el = render_footer(window, cx);
            root = root.child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_end()
                    .gap(px(8.))
                    .px(px(20.))
                    .py(px(14.))
                    .border_t_1()
                    .border_color(theme.border)
                    .bg(theme.surface_muted.opacity(0.45))
                    .child(footer_el),
            );
        } else if !self.actions.is_empty() {
            let mut footer = div()
                .flex()
                .flex_row()
                .items_center()
                .justify_end()
                .gap(px(8.))
                .px(px(20.))
                .py(px(14.))
                .border_t_1()
                .border_color(theme.border)
                .bg(theme.surface_muted.opacity(0.45));

            for (idx, action) in self.actions.iter().enumerate() {
                let handler = action.on_click.clone();
                let closes = action.closes_window;
                let has_custom_handler = handler.is_some();
                let on_close_fired = on_close_fired.clone();
                let on_close_cb = on_close_cb.clone();
                footer = footer.child(
                    Button::new(("native-dlg-action", idx))
                        .variant(action.variant)
                        .label(action.label.clone())
                        .on_click(move |_, window, cx| {
                            if let Some(cb) = &handler {
                                cb(window, cx);
                            }
                            if closes {
                                if has_custom_handler {
                                    // Custom action (e.g. Confirm) handled completion; mark closed
                                    // without also firing the cancellation `on_close` hook.
                                    on_close_fired.set(true);
                                } else if !on_close_fired.replace(true)
                                    && let Some(cb) = &on_close_cb
                                {
                                    cb(window, cx);
                                }
                                window.remove_window();
                            }
                        }),
                );
            }
            root = root.child(footer);
        }

        root
    }
}

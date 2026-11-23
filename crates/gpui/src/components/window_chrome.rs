//! Cross-platform frameless [`WindowTitleBar`] and pinned [`StatusBar`] window chrome.
//!
//! - On **Windows**, attaches native [`WindowControlArea::Drag`], `Min`, `Max`, and `Close`
//!   hit-test regions so Windows 11 Aero Snap, Snap Layouts on maximize-button hover, double-click
//!   maximize, and right-click system menu work natively with the standard 46 px caption controls
//!   and `#E81123` close hover fill.
//! - On **macOS**, leaves an 80 px left inset for native traffic-light buttons and hides the
//!   right-side Win32 caption cluster.
//! - On **Linux**, wires manual `window.start_window_move()`, double-click `window.zoom_window()`,
//!   and minimize/maximize/close button click handlers.

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, FontWeight, IntoElement, MouseButton, Pixels, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, WindowControlArea, div, prelude::*, px, rgb,
};

use super::field::kbd;
use super::theme::ActiveTheme;

/// Cross-platform custom-painted frameless title bar for desktop apps using
/// `TitlebarOptions { appears_transparent: true, .. }`.
#[derive(IntoElement)]
pub struct WindowTitleBar {
    title: Option<SharedString>,
    icon: Option<LucideIcon>,
    leading: Option<AnyElement>,
    center: Option<AnyElement>,
    trailing: Option<AnyElement>,
    height: Pixels,
    show_controls: bool,
    preview_mode: bool,
    bordered: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`WindowTitleBar::new`].
pub fn window_title_bar() -> WindowTitleBar {
    WindowTitleBar::new()
}

impl WindowTitleBar {
    /// Creates a new [`WindowTitleBar`] with default `38px` height.
    pub fn new() -> Self {
        Self {
            title: None,
            icon: None,
            leading: None,
            center: None,
            trailing: None,
            height: px(38.),
            show_controls: true,
            preview_mode: false,
            bordered: true,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the title text shown on the leading side.
    pub fn title(mut self, title: impl Into<SharedString>) -> Self {
        self.title = Some(title.into());
        self
    }

    /// Sets an optional leading [`LucideIcon`] beside the title.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets a custom leading element (brand mark, menu trigger, etc.).
    pub fn leading(mut self, el: impl IntoElement) -> Self {
        self.leading = Some(el.into_any_element());
        self
    }

    /// Sets a custom center element (segmented navigation, search bar, etc.).
    pub fn center(mut self, el: impl IntoElement) -> Self {
        self.center = Some(el.into_any_element());
        self
    }

    /// Sets a custom trailing element rendered just left of the window caption buttons.
    pub fn trailing(mut self, el: impl IntoElement) -> Self {
        self.trailing = Some(el.into_any_element());
        self
    }

    /// Sets the bar height (default `38px`).
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Toggles the bottom hairline border (default `true`).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }

    /// Toggles the OS window control buttons (default `true`).
    pub fn show_controls(mut self, show: bool) -> Self {
        self.show_controls = show;
        self
    }

    /// Renders in non-destructive preview mode (for embedding inside playground cards without
    /// actually closing/minimizing the host window or attaching top-level hit-test areas).
    pub fn preview_mode(mut self, preview: bool) -> Self {
        self.preview_mode = preview;
        self
    }
}

impl Default for WindowTitleBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for WindowTitleBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for WindowTitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let is_macos = cfg!(target_os = "macos") && !self.preview_mode;
        let os_handles = cfg!(target_os = "windows") && !self.preview_mode;
        let is_maximized = !self.preview_mode && window.is_maximized();
        let preview = self.preview_mode;

        let hover_bg = theme.surface_hover;
        let fg = theme.foreground;
        let muted = theme.muted_foreground;

        let mut drag_region = div()
            .flex_1()
            .h_full()
            .min_w(px(16.))
            .flex()
            .items_center()
            .justify_center()
            .when_some(self.center, |el, center| el.child(center));

        if !preview {
            drag_region = drag_region.window_control_area(WindowControlArea::Drag);
            if !os_handles && !is_macos {
                drag_region = drag_region.on_mouse_down(MouseButton::Left, |ev, window, _| {
                    if ev.click_count >= 2 {
                        window.zoom_window();
                    } else {
                        window.start_window_move();
                    }
                });
            }
        }

        let controls = if self.show_controls && !is_macos {
            let mut min_btn = div()
                .id("titlebar-min")
                .flex()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h_full()
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .child(Icon::new(LucideIcon::Minus).size(IconSize::Sm).color(muted));
            let mut max_btn = div()
                .id("titlebar-max")
                .flex()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h_full()
                .cursor_pointer()
                .hover(move |s| s.bg(hover_bg))
                .child(
                    Icon::new(if is_maximized {
                        LucideIcon::Copy
                    } else {
                        LucideIcon::Square
                    })
                    .size(IconSize::Xs)
                    .color(muted),
                );
            let mut close_btn = div()
                .id("titlebar-close")
                .flex()
                .items_center()
                .justify_center()
                .w(px(46.))
                .h_full()
                .cursor_pointer()
                .hover(|s| s.bg(rgb(0xE81123)).text_color(rgb(0xFFFFFF)))
                .child(Icon::new(LucideIcon::X).size(IconSize::Sm).color(fg));

            if os_handles {
                min_btn = min_btn.window_control_area(WindowControlArea::Min);
                max_btn = max_btn.window_control_area(WindowControlArea::Max);
                close_btn = close_btn.window_control_area(WindowControlArea::Close);
            } else if !preview {
                min_btn = min_btn.on_click(|_, window, _| window.minimize_window());
                max_btn = max_btn.on_click(|_, window, _| window.zoom_window());
                close_btn = close_btn.on_click(|_, window, _| window.remove_window());
            }

            Some(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .h_full()
                    .flex_shrink_0()
                    .child(min_btn)
                    .child(max_btn)
                    .child(close_btn),
            )
        } else {
            None
        };

        let mut bar = div()
            .w_full()
            .h(self.height)
            .flex()
            .flex_row()
            .items_center()
            .flex_shrink_0()
            .bg(theme.surface)
            .text_color(theme.foreground)
            .when(self.bordered, |el| el.border_b_1().border_color(theme.border))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .pl(if is_macos { px(80.) } else { px(12.) })
                    .pr(px(8.))
                    .flex_shrink_0()
                    .when_some(self.leading, |el, leading| el.child(leading))
                    .when_some(self.icon, |el, icon| {
                        el.child(Icon::new(icon).size(IconSize::Sm).color(theme.primary))
                    })
                    .when_some(self.title, |el, title| {
                        el.child(
                            div()
                                .text_size(px(13.))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(title),
                        )
                    }),
            )
            .child(drag_region)
            .when_some(self.trailing, |el, trailing| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(6.))
                        .px(px(8.))
                        .flex_shrink_0()
                        .child(trailing),
                )
            })
            .when_some(controls, |el, c| el.child(c));

        crate::Refineable::refine(bar.style(), &self.style);
        bar
    }
}

/// Compact keycap + action label hint pair (e.g. `↑↓ Navigate`, `↵ Open`, `Ctrl+K Commands`).
#[derive(IntoElement)]
pub struct KeyHint {
    keys: Vec<SharedString>,
    label: SharedString,
}

/// Convenience constructor for [`KeyHint::new`].
pub fn key_hint(keys: &[&str], label: impl Into<SharedString>) -> KeyHint {
    KeyHint::new(keys, label)
}

impl KeyHint {
    /// Creates a [`KeyHint`] displaying one or more keycaps beside `label`.
    pub fn new(keys: &[&str], label: impl Into<SharedString>) -> Self {
        Self {
            keys: keys.iter().map(|k| SharedString::from(*k)).collect(),
            label: label.into(),
        }
    }
}

impl RenderOnce for KeyHint {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .text_size(px(11.5))
            .text_color(theme.muted_foreground)
            .children(self.keys.into_iter().map(kbd))
            .child(self.label)
    }
}

/// Pinned desktop footer [`StatusBar`] showing left status/beacons, center context badges,
/// and right keyboard shortcut hints.
#[derive(IntoElement)]
pub struct StatusBar {
    left: Vec<AnyElement>,
    center: Vec<AnyElement>,
    right: Vec<AnyElement>,
    height: Pixels,
    bordered: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`StatusBar::new`].
pub fn status_bar() -> StatusBar {
    StatusBar::new()
}

impl StatusBar {
    /// Creates a new [`StatusBar`] with default `30px` height.
    pub fn new() -> Self {
        Self {
            left: Vec::new(),
            center: Vec::new(),
            right: Vec::new(),
            height: px(30.),
            bordered: true,
            style: StyleRefinement::default(),
        }
    }

    /// Adds a child to the left status region.
    pub fn left(mut self, el: impl IntoElement) -> Self {
        self.left.push(el.into_any_element());
        self
    }

    /// Adds a child to the center status region.
    pub fn center(mut self, el: impl IntoElement) -> Self {
        self.center.push(el.into_any_element());
        self
    }

    /// Adds a child to the right status region (e.g. [`KeyHint`]).
    pub fn right(mut self, el: impl IntoElement) -> Self {
        self.right.push(el.into_any_element());
        self
    }

    /// Sets the status bar height (default `30px`).
    pub fn height(mut self, height: Pixels) -> Self {
        self.height = height;
        self
    }

    /// Toggles the top hairline divider (default `true`).
    pub fn bordered(mut self, bordered: bool) -> Self {
        self.bordered = bordered;
        self
    }
}

impl Default for StatusBar {
    fn default() -> Self {
        Self::new()
    }
}

impl Styled for StatusBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for StatusBar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut bar = div()
            .w_full()
            .h(self.height)
            .px(px(12.))
            .flex()
            .flex_row()
            .items_center()
            .justify_between()
            .gap(px(12.))
            .flex_shrink_0()
            .bg(theme.surface_muted.opacity(0.65))
            .text_size(px(11.5))
            .text_color(theme.muted_foreground)
            .when(self.bordered, |el| el.border_t_1().border_color(theme.border))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .min_w_0()
                    .children(self.left),
            )
            .when(!self.center.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .children(self.center),
                )
            })
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(12.))
                    .flex_shrink_0()
                    .children(self.right),
            );

        crate::Refineable::refine(bar.style(), &self.style);
        bar
    }
}

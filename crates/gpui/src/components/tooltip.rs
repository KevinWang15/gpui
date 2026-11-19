//! Styled Tailwind [`Tooltip`] view for use with GPUI's `.tooltip(...)` API.

use crate::{
    AnyView, App, Context, IntoElement, Render, SharedString, Window, div, prelude::*, px,
};

use super::palette as tw;
use super::theme::{ActiveTheme, ThemeMode};

/// Styled dark/themed Tailwind tooltip optionally displaying a keyboard shortcut hint.
pub struct Tooltip {
    text: SharedString,
    shortcut: Option<SharedString>,
}

impl Tooltip {
    /// Builds an [`AnyView`] tooltip showing `text`, ready to return from `.tooltip(...)`.
    pub fn text(text: impl Into<SharedString>, _window: &mut Window, cx: &mut App) -> AnyView {
        let text = text.into();
        cx.new(|_| Self {
            text,
            shortcut: None,
        })
        .into()
    }

    /// Builds an [`AnyView`] tooltip showing `text` alongside a keycap shortcut (e.g. `"⌘K"`).
    pub fn with_shortcut(
        text: impl Into<SharedString>,
        shortcut: impl Into<SharedString>,
        _window: &mut Window,
        cx: &mut App,
    ) -> AnyView {
        let text = text.into();
        let shortcut = shortcut.into();
        cx.new(|_| Self {
            text,
            shortcut: Some(shortcut),
        })
        .into()
    }
}

impl Render for Tooltip {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border) = match theme.mode {
            ThemeMode::Light => (tw::SLATE_900, tw::WHITE, tw::SLATE_800),
            ThemeMode::Dark => (tw::SLATE_800, tw::SLATE_50, tw::SLATE_700),
        };

        div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .px(px(10.))
            .py(px(6.))
            .rounded(theme.radius_md)
            .bg(bg)
            .text_color(fg)
            .border_1()
            .border_color(border)
            .shadow_md()
            .text_size(px(12.))
            .child(self.text.clone())
            .when_some(self.shortcut.clone(), |el, sc| {
                el.child(
                    div()
                        .px(px(5.))
                        .py(px(1.))
                        .rounded(px(3.))
                        .bg(crate::Hsla::from(tw::WHITE).opacity(0.14))
                        .text_size(px(11.))
                        .text_color(tw::SLATE_200)
                        .child(sc),
                )
            })
    }
}

//! Three-way Light / System / Dark [`ThemeSwitcher`] with click-origin circular GPU transitions.

use std::rc::Rc;

use crate::{
    App, ElementId, IntoElement, Pixels, Point, RenderOnce, SharedString, StyleRefinement, Styled,
    Window,
};
use crate::icons::LucideIcon;

use super::segmented::{SegmentedControl, SegmentedItem};
use super::theme::{ActiveTheme, ComponentSize, Theme, ThemePreference, ThemeTransitionStyle};

/// Compact 3-way Light / System / Dark icon switcher backed by [`SegmentedControl`] and
/// GPU-composited [`ViewTransition`](crate::ViewTransition)s.
///
/// Clicking any segment switches the theme with the configured [`ThemeTransitionStyle`]
/// (defaulting to [`ThemeTransitionStyle::RadialReveal`]) whenever the resolved
/// [`ThemeMode`](super::ThemeMode) changes.
#[derive(IntoElement)]
pub struct ThemeSwitcher {
    id: ElementId,
    preference: Option<ThemePreference>,
    transition_style: ThemeTransitionStyle,
    size: ComponentSize,
    style: StyleRefinement,
    on_change: Option<Rc<dyn Fn(ThemePreference, Point<Pixels>, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`ThemeSwitcher::new`].
pub fn theme_switcher(id: impl Into<ElementId>) -> ThemeSwitcher {
    ThemeSwitcher::new(id)
}

impl ThemeSwitcher {
    /// Creates a new [`ThemeSwitcher`] that reflects the active global [`Theme::preference`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            preference: None,
            transition_style: ThemeTransitionStyle::RadialReveal,
            size: ComponentSize::Sm,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Explicitly overrides the displayed [`ThemePreference`] instead of reading `cx.theme().preference`.
    pub fn preference(mut self, preference: ThemePreference) -> Self {
        self.preference = Some(preference);
        self
    }

    /// Sets the GPU shader [`ThemeTransitionStyle`] (`ChromaticWarp`, `Shockwave`, `PageCurl`,
    /// `LiquidDissolve`, `RadialReveal`, or `CrossFade`) used when switching themes without a custom `on_change`.
    pub fn transition_style(mut self, style: ThemeTransitionStyle) -> Self {
        self.transition_style = style;
        self
    }

    /// Sets the control size (`Xs`, `Sm`, or `Md`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Registers a custom handler receiving `(preference, click_origin, window, cx)`.
    ///
    /// When omitted, [`ThemeSwitcher`] automatically calls
    /// [`Theme::set_preference_with_transition`].
    pub fn on_change(
        mut self,
        handler: impl Fn(ThemePreference, Point<Pixels>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for ThemeSwitcher {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn pref_to_key(pref: ThemePreference) -> SharedString {
    match pref {
        ThemePreference::Light => "light".into(),
        ThemePreference::System => "system".into(),
        ThemePreference::Dark => "dark".into(),
    }
}

fn key_to_pref(key: &str) -> ThemePreference {
    match key {
        "dark" => ThemePreference::Dark,
        "system" => ThemePreference::System,
        _ => ThemePreference::Light,
    }
}

impl RenderOnce for ThemeSwitcher {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let active_pref = self.preference.unwrap_or_else(|| cx.theme().preference);
        let transition_style = self.transition_style;
        let on_change = self.on_change;

        let mut control = SegmentedControl::new(self.id, pref_to_key(active_pref))
            .size(self.size)
            .items([
                SegmentedItem::icon_only("light", LucideIcon::Sun),
                SegmentedItem::icon_only("system", LucideIcon::Monitor),
                SegmentedItem::icon_only("dark", LucideIcon::Moon),
            ])
            .on_change_with_position(move |key, click_pos, window, cx| {
                let next_pref = key_to_pref(key.as_ref());
                if let Some(custom) = &on_change {
                    custom(next_pref, click_pos, window, cx);
                } else {
                    Theme::set_preference_with_transition_style(
                        next_pref,
                        transition_style,
                        Some(click_pos),
                        window,
                        cx,
                    );
                }
            });

        crate::Refineable::refine(control.style(), &self.style);
        control
    }
}

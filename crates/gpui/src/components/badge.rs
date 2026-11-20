//! [`Badge`] / pill tag component with semantic colors, variants,
//! status dot, leading icon, and optional remove button.

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    App, ClickEvent, ElementId, FontWeight, Hsla, IntoElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};

use super::palette as tw;
use super::theme::{ActiveTheme, Theme, ThemeMode};

/// Color themes for [`Badge`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeColor {
    /// Neutral slate/gray badge.
    #[default]
    Gray,
    /// Brand indigo badge.
    Indigo,
    /// Blue informational badge.
    Blue,
    /// Emerald/green success badge.
    Green,
    /// Amber/yellow warning badge.
    Amber,
    /// Red error/danger badge.
    Red,
    /// Purple accent badge.
    Purple,
    /// Pink accent badge.
    Pink,
}

/// Visual treatment for [`Badge`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum BadgeVariant {
    /// Soft tinted background with matching ring border (the default badge).
    #[default]
    Soft,
    /// Solid saturated background with white ink.
    Solid,
    /// Transparent background with colored border and ink.
    Outline,
}

/// Badge / pill tag with optional dot, icon, and removable `X` button.
#[derive(IntoElement)]
pub struct Badge {
    label: SharedString,
    color: BadgeColor,
    variant: BadgeVariant,
    pill: bool,
    dot: bool,
    icon: Option<LucideIcon>,
    remove_id: Option<ElementId>,
    on_remove: Option<Box<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Badge::new`].
pub fn badge(label: impl Into<SharedString>) -> Badge {
    Badge::new(label)
}

impl Badge {
    /// Creates a new [`Badge`] with `label`.
    pub fn new(label: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            color: BadgeColor::Gray,
            variant: BadgeVariant::Soft,
            pill: false,
            dot: false,
            icon: None,
            remove_id: None,
            on_remove: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets the semantic color.
    pub fn color(mut self, color: BadgeColor) -> Self {
        self.color = color;
        self
    }

    /// Sets the visual variant (`Soft`, `Solid`, `Outline`).
    pub fn variant(mut self, variant: BadgeVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Uses fully rounded pill corners (`rounded-full`).
    pub fn pill(mut self, pill: bool) -> Self {
        self.pill = pill;
        self
    }

    /// Displays a small colored status dot before the label.
    pub fn dot(mut self, dot: bool) -> Self {
        self.dot = dot;
        self
    }

    /// Adds a leading [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Renders a removable `X` icon button that triggers `handler` when clicked.
    pub fn on_remove(
        mut self,
        id: impl Into<ElementId>,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.remove_id = Some(id.into());
        self.on_remove = Some(Box::new(handler));
        self
    }

    fn resolve_colors(&self, theme: &Theme) -> (Hsla, Hsla, Hsla, Hsla) {
        let dark = theme.mode == ThemeMode::Dark;
        // (soft_bg, soft_fg, solid_bg, dot_color)
        let (soft_bg, soft_fg, solid_bg, dot_color): (Hsla, Hsla, Hsla, Hsla) = match self.color {
            BadgeColor::Gray => {
                if dark {
                    (
                        Hsla::from(tw::SLATE_400).opacity(0.15),
                        tw::SLATE_300.into(),
                        tw::SLATE_600.into(),
                        tw::SLATE_400.into(),
                    )
                } else {
                    (
                        tw::SLATE_100.into(),
                        tw::SLATE_700.into(),
                        tw::SLATE_700.into(),
                        tw::SLATE_500.into(),
                    )
                }
            }
            BadgeColor::Indigo => {
                if dark {
                    (
                        Hsla::from(tw::INDIGO_400).opacity(0.16),
                        tw::INDIGO_300.into(),
                        tw::INDIGO_600.into(),
                        tw::INDIGO_400.into(),
                    )
                } else {
                    (
                        tw::INDIGO_50.into(),
                        tw::INDIGO_700.into(),
                        tw::INDIGO_600.into(),
                        tw::INDIGO_500.into(),
                    )
                }
            }
            BadgeColor::Blue => {
                if dark {
                    (
                        Hsla::from(tw::BLUE_400).opacity(0.16),
                        tw::BLUE_300.into(),
                        tw::BLUE_600.into(),
                        tw::BLUE_400.into(),
                    )
                } else {
                    (
                        tw::BLUE_50.into(),
                        tw::BLUE_700.into(),
                        tw::BLUE_600.into(),
                        tw::BLUE_500.into(),
                    )
                }
            }
            BadgeColor::Green => {
                if dark {
                    (
                        Hsla::from(tw::EMERALD_400).opacity(0.16),
                        tw::EMERALD_300.into(),
                        tw::EMERALD_600.into(),
                        tw::EMERALD_400.into(),
                    )
                } else {
                    (
                        tw::EMERALD_50.into(),
                        tw::EMERALD_700.into(),
                        tw::EMERALD_600.into(),
                        tw::EMERALD_500.into(),
                    )
                }
            }
            BadgeColor::Amber => {
                if dark {
                    (
                        Hsla::from(tw::AMBER_400).opacity(0.16),
                        tw::AMBER_300.into(),
                        tw::AMBER_600.into(),
                        tw::AMBER_400.into(),
                    )
                } else {
                    (
                        tw::AMBER_50.into(),
                        tw::AMBER_800.into(),
                        tw::AMBER_500.into(),
                        tw::AMBER_500.into(),
                    )
                }
            }
            BadgeColor::Red => {
                if dark {
                    (
                        Hsla::from(tw::RED_400).opacity(0.16),
                        tw::RED_300.into(),
                        tw::RED_600.into(),
                        tw::RED_400.into(),
                    )
                } else {
                    (
                        tw::RED_50.into(),
                        tw::RED_700.into(),
                        tw::RED_600.into(),
                        tw::RED_500.into(),
                    )
                }
            }
            BadgeColor::Purple => {
                if dark {
                    (
                        Hsla::from(tw::PURPLE_400).opacity(0.16),
                        tw::PURPLE_300.into(),
                        tw::PURPLE_600.into(),
                        tw::PURPLE_400.into(),
                    )
                } else {
                    (
                        tw::PURPLE_50.into(),
                        tw::PURPLE_700.into(),
                        tw::PURPLE_600.into(),
                        tw::PURPLE_500.into(),
                    )
                }
            }
            BadgeColor::Pink => {
                if dark {
                    (
                        Hsla::from(tw::PINK_400).opacity(0.16),
                        tw::PINK_300.into(),
                        tw::PINK_600.into(),
                        tw::PINK_400.into(),
                    )
                } else {
                    (
                        tw::PINK_50.into(),
                        tw::PINK_700.into(),
                        tw::PINK_600.into(),
                        tw::PINK_500.into(),
                    )
                }
            }
        };

        match self.variant {
            BadgeVariant::Soft => (soft_bg, soft_fg, dot_color.opacity(0.25), dot_color),
            BadgeVariant::Solid => (solid_bg, tw::WHITE.into(), solid_bg, tw::WHITE.into()),
            BadgeVariant::Outline => (
                crate::transparent_black(),
                soft_fg,
                dot_color.opacity(0.55),
                dot_color,
            ),
        }
    }
}

impl Styled for Badge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Badge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (bg, fg, border, dot_color) = self.resolve_colors(&theme);

        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(5.))
            .px(px(8.))
            .py(px(2.))
            .when(self.pill, |el| el.rounded_full())
            .when(!self.pill, |el| el.rounded(theme.radius_md))
            .bg(bg)
            .text_color(fg)
            .border_1()
            .border_color(border)
            .text_size(px(12.))
            .font_weight(FontWeight::MEDIUM)
            .whitespace_nowrap()
            .when(self.dot, |el| {
                el.child(div().w(px(6.)).h(px(6.)).rounded_full().bg(dot_color))
            })
            .when_some(self.icon, |el, icon| {
                el.child(Icon::new(icon).size(IconSize::Xs).color(fg))
            })
            .child(self.label);

        if let (Some(remove_id), Some(on_remove)) = (self.remove_id, self.on_remove) {
            el = el.child(
                div()
                    .id(remove_id)
                    .flex()
                    .items_center()
                    .justify_center()
                    .rounded_full()
                    .p(px(1.))
                    .cursor_pointer()
                    .hover(|s| s.bg(fg.opacity(0.15)))
                    .on_click(move |ev, window, cx| on_remove(ev, window, cx))
                    .child(Icon::new(LucideIcon::X).size(IconSize::Xs).color(fg)),
            );
        }

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

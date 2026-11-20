//! [`Badge`] / pill tag component with semantic colors, variants,
//! status dot, leading icon, and optional remove button.

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    App, ClickEvent, ElementId, FontWeight, Hsla, IntoElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};

use super::palette;
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
                        Hsla::from(palette::SLATE_400).opacity(0.15),
                        palette::SLATE_300.into(),
                        palette::SLATE_600.into(),
                        palette::SLATE_400.into(),
                    )
                } else {
                    (
                        palette::SLATE_100.into(),
                        palette::SLATE_700.into(),
                        palette::SLATE_700.into(),
                        palette::SLATE_500.into(),
                    )
                }
            }
            BadgeColor::Indigo => {
                if dark {
                    (
                        Hsla::from(palette::INDIGO_400).opacity(0.16),
                        palette::INDIGO_300.into(),
                        palette::INDIGO_600.into(),
                        palette::INDIGO_400.into(),
                    )
                } else {
                    (
                        palette::INDIGO_50.into(),
                        palette::INDIGO_700.into(),
                        palette::INDIGO_600.into(),
                        palette::INDIGO_500.into(),
                    )
                }
            }
            BadgeColor::Blue => {
                if dark {
                    (
                        Hsla::from(palette::BLUE_400).opacity(0.16),
                        palette::BLUE_300.into(),
                        palette::BLUE_600.into(),
                        palette::BLUE_400.into(),
                    )
                } else {
                    (
                        palette::BLUE_50.into(),
                        palette::BLUE_700.into(),
                        palette::BLUE_600.into(),
                        palette::BLUE_500.into(),
                    )
                }
            }
            BadgeColor::Green => {
                if dark {
                    (
                        Hsla::from(palette::EMERALD_400).opacity(0.16),
                        palette::EMERALD_300.into(),
                        palette::EMERALD_600.into(),
                        palette::EMERALD_400.into(),
                    )
                } else {
                    (
                        palette::EMERALD_50.into(),
                        palette::EMERALD_700.into(),
                        palette::EMERALD_600.into(),
                        palette::EMERALD_500.into(),
                    )
                }
            }
            BadgeColor::Amber => {
                if dark {
                    (
                        Hsla::from(palette::AMBER_400).opacity(0.16),
                        palette::AMBER_300.into(),
                        palette::AMBER_600.into(),
                        palette::AMBER_400.into(),
                    )
                } else {
                    (
                        palette::AMBER_50.into(),
                        palette::AMBER_800.into(),
                        palette::AMBER_500.into(),
                        palette::AMBER_500.into(),
                    )
                }
            }
            BadgeColor::Red => {
                if dark {
                    (
                        Hsla::from(palette::RED_400).opacity(0.16),
                        palette::RED_300.into(),
                        palette::RED_600.into(),
                        palette::RED_400.into(),
                    )
                } else {
                    (
                        palette::RED_50.into(),
                        palette::RED_700.into(),
                        palette::RED_600.into(),
                        palette::RED_500.into(),
                    )
                }
            }
            BadgeColor::Purple => {
                if dark {
                    (
                        Hsla::from(palette::PURPLE_400).opacity(0.16),
                        palette::PURPLE_300.into(),
                        palette::PURPLE_600.into(),
                        palette::PURPLE_400.into(),
                    )
                } else {
                    (
                        palette::PURPLE_50.into(),
                        palette::PURPLE_700.into(),
                        palette::PURPLE_600.into(),
                        palette::PURPLE_500.into(),
                    )
                }
            }
            BadgeColor::Pink => {
                if dark {
                    (
                        Hsla::from(palette::PINK_400).opacity(0.16),
                        palette::PINK_300.into(),
                        palette::PINK_600.into(),
                        palette::PINK_400.into(),
                    )
                } else {
                    (
                        palette::PINK_50.into(),
                        palette::PINK_700.into(),
                        palette::PINK_600.into(),
                        palette::PINK_500.into(),
                    )
                }
            }
        };

        match self.variant {
            BadgeVariant::Soft => (soft_bg, soft_fg, dot_color.opacity(0.25), dot_color),
            BadgeVariant::Solid => (
                solid_bg,
                palette::WHITE.into(),
                solid_bg,
                palette::WHITE.into(),
            ),
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
            let remove = div().id(remove_id);
            el = el.child(
                super::focus::tab_stop(remove, true, &theme)
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

//! Brand accent color presets for live playground theme switching.

use gpui::components::{Theme, ThemeMode, palette as tw};
use gpui::{Hsla, Rgba};

/// Brand accent swatches selectable in the playground navbar.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AccentPreset {
    #[default]
    Indigo,
    Blue,
    Emerald,
    Violet,
    Rose,
    Amber,
    Slate,
}

impl AccentPreset {
    pub const ALL: &'static [Self] = &[
        Self::Indigo,
        Self::Blue,
        Self::Emerald,
        Self::Violet,
        Self::Rose,
        Self::Amber,
        Self::Slate,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::Indigo => "Indigo",
            Self::Blue => "Blue",
            Self::Emerald => "Emerald",
            Self::Violet => "Violet",
            Self::Rose => "Rose",
            Self::Amber => "Amber",
            Self::Slate => "Slate",
        }
    }

    pub const fn swatch(self) -> Rgba {
        match self {
            Self::Indigo => tw::INDIGO_600,
            Self::Blue => tw::BLUE_600,
            Self::Emerald => tw::EMERALD_600,
            Self::Violet => tw::VIOLET_600,
            Self::Rose => tw::ROSE_600,
            Self::Amber => tw::AMBER_500,
            Self::Slate => tw::SLATE_800,
        }
    }

    /// Builds a complete light or dark [`Theme`] tinted with this brand accent.
    pub fn build_theme(self, mode: ThemeMode) -> Theme {
        let mut theme = match mode {
            ThemeMode::Light => Theme::light(),
            ThemeMode::Dark => Theme::dark(),
        };

        let (p_light, p_light_h, p_soft_l, p_soft_fg_l, p_dark, p_dark_h, p_soft_fg_d) = match self
        {
            Self::Indigo => (
                tw::INDIGO_600,
                tw::INDIGO_700,
                tw::INDIGO_50,
                tw::INDIGO_700,
                tw::INDIGO_500,
                tw::INDIGO_400,
                tw::INDIGO_300,
            ),
            Self::Blue => (
                tw::BLUE_600,
                tw::BLUE_700,
                tw::BLUE_50,
                tw::BLUE_700,
                tw::BLUE_500,
                tw::BLUE_400,
                tw::BLUE_300,
            ),
            Self::Emerald => (
                tw::EMERALD_600,
                tw::EMERALD_700,
                tw::EMERALD_50,
                tw::EMERALD_700,
                tw::EMERALD_500,
                tw::EMERALD_400,
                tw::EMERALD_300,
            ),
            Self::Violet => (
                tw::VIOLET_600,
                tw::VIOLET_700,
                tw::VIOLET_50,
                tw::VIOLET_700,
                tw::VIOLET_500,
                tw::VIOLET_400,
                tw::VIOLET_300,
            ),
            Self::Rose => (
                tw::ROSE_600,
                tw::ROSE_700,
                tw::ROSE_50,
                tw::ROSE_700,
                tw::ROSE_500,
                tw::ROSE_400,
                tw::ROSE_300,
            ),
            Self::Amber => (
                tw::AMBER_600,
                tw::AMBER_700,
                tw::AMBER_50,
                tw::AMBER_800,
                tw::AMBER_500,
                tw::AMBER_400,
                tw::AMBER_300,
            ),
            Self::Slate => (
                tw::SLATE_900,
                tw::SLATE_800,
                tw::SLATE_100,
                tw::SLATE_800,
                tw::SLATE_200,
                tw::SLATE_300,
                tw::SLATE_200,
            ),
        };

        match mode {
            ThemeMode::Light => {
                theme.primary = p_light.into();
                theme.primary_hover = p_light_h.into();
                theme.primary_soft = p_soft_l.into();
                theme.primary_soft_foreground = p_soft_fg_l.into();
                theme.ring = p_light.into();
                theme.selection = Hsla::from(p_light).opacity(0.22);
                theme.sidebar_active_bg = p_soft_l.into();
                theme.sidebar_active_foreground = p_soft_fg_l.into();
                theme.sidebar_active_icon = p_light.into();
                theme.toggle_track_on = p_light.into();
                theme.slider_fill = p_light.into();
                if let Some(first) = theme.chart_colors.first_mut() {
                    *first = p_light.into();
                }
            }
            ThemeMode::Dark => {
                theme.primary = p_dark.into();
                theme.primary_hover = p_dark_h.into();
                if self == Self::Slate {
                    theme.primary_foreground = tw::SLATE_950.into();
                }
                theme.primary_soft = Hsla::from(p_dark).opacity(0.18);
                theme.primary_soft_foreground = p_soft_fg_d.into();
                theme.ring = p_dark_h.into();
                theme.selection = Hsla::from(p_dark_h).opacity(0.32);
                theme.sidebar_active_bg = Hsla::from(p_dark).opacity(0.18);
                theme.sidebar_active_foreground = p_soft_fg_d.into();
                theme.sidebar_active_icon = p_dark_h.into();
                theme.toggle_track_on = p_dark.into();
                theme.slider_fill = p_dark.into();
                if let Some(first) = theme.chart_colors.first_mut() {
                    *first = p_dark_h.into();
                }
            }
        }

        theme
    }
}

//! Brand accent color presets for live playground theme switching.

use gpui::components::{Theme, ThemeMode, palette};
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
            Self::Indigo => palette::INDIGO_600,
            Self::Blue => palette::BLUE_600,
            Self::Emerald => palette::EMERALD_600,
            Self::Violet => palette::VIOLET_600,
            Self::Rose => palette::ROSE_600,
            Self::Amber => palette::AMBER_500,
            Self::Slate => palette::SLATE_800,
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
                palette::INDIGO_600,
                palette::INDIGO_700,
                palette::INDIGO_50,
                palette::INDIGO_700,
                palette::INDIGO_500,
                palette::INDIGO_400,
                palette::INDIGO_300,
            ),
            Self::Blue => (
                palette::BLUE_600,
                palette::BLUE_700,
                palette::BLUE_50,
                palette::BLUE_700,
                palette::BLUE_500,
                palette::BLUE_400,
                palette::BLUE_300,
            ),
            Self::Emerald => (
                palette::EMERALD_600,
                palette::EMERALD_700,
                palette::EMERALD_50,
                palette::EMERALD_700,
                palette::EMERALD_500,
                palette::EMERALD_400,
                palette::EMERALD_300,
            ),
            Self::Violet => (
                palette::VIOLET_600,
                palette::VIOLET_700,
                palette::VIOLET_50,
                palette::VIOLET_700,
                palette::VIOLET_500,
                palette::VIOLET_400,
                palette::VIOLET_300,
            ),
            Self::Rose => (
                palette::ROSE_600,
                palette::ROSE_700,
                palette::ROSE_50,
                palette::ROSE_700,
                palette::ROSE_500,
                palette::ROSE_400,
                palette::ROSE_300,
            ),
            Self::Amber => (
                palette::AMBER_600,
                palette::AMBER_700,
                palette::AMBER_50,
                palette::AMBER_800,
                palette::AMBER_500,
                palette::AMBER_400,
                palette::AMBER_300,
            ),
            Self::Slate => (
                palette::SLATE_900,
                palette::SLATE_800,
                palette::SLATE_100,
                palette::SLATE_800,
                palette::SLATE_200,
                palette::SLATE_300,
                palette::SLATE_200,
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
                    theme.primary_foreground = palette::SLATE_950.into();
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

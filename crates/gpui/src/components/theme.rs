//! Semantic design tokens, light/dark Tailwind themes, sizing tokens, and component initialization.

use std::sync::Arc;
use std::time::Duration;

use crate::{App, Global, Hsla, Pixels, SpringConfig, Window, WindowAppearance, px};

use super::palette as tw;

/// Snappy, tactile spring preset for micro-interactions (toggle thumb slide, checkbox/radio pop, chevron rotation).
///
/// Damping ratio ~0.75 gives a crisp, quick feel (~160ms) with barely perceptible organic settle.
pub const SPRING_SNAPPY: SpringConfig = SpringConfig::new(560.0, 35.0, 1.0);

/// Smooth critically-damped spring preset for color fades, progress bars, and selection highlights.
pub const SPRING_SMOOTH: SpringConfig = SpringConfig::new(360.0, 34.0, 1.0);

/// Fast duration (140ms) for popover/dropdown/disclosure entrance animations.
pub const ANIM_FAST: Duration = Duration::from_millis(140);

/// Standard duration (175ms) for modal dialog and toast entrance animations.
pub const ANIM_NORMAL: Duration = Duration::from_millis(175);

/// Whether the active UI theme is light or dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ThemeMode {
    /// Crisp light Tailwind aesthetic (white surfaces, slate-900 ink, indigo-600 primary).
    #[default]
    Light,
    /// Deep dark Tailwind aesthetic (slate-950 canvas, slate-900 surfaces, indigo-500 primary).
    Dark,
}

impl From<WindowAppearance> for ThemeMode {
    fn from(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
        }
    }
}

/// Shared size scale across Tailwind-style interactive components.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ComponentSize {
    /// Extra small (`24px` control height).
    Xs,
    /// Small (`32px` control height).
    Sm,
    /// Medium / default (`36px` control height).
    #[default]
    Md,
    /// Large (`42px` control height).
    Lg,
    /// Extra large (`48px` control height).
    Xl,
}

impl ComponentSize {
    /// Recommended control height for buttons, inputs, selects, and comboboxes.
    pub const fn height(self) -> Pixels {
        match self {
            Self::Xs => px(26.),
            Self::Sm => px(32.),
            Self::Md => px(38.),
            Self::Lg => px(44.),
            Self::Xl => px(50.),
        }
    }

    /// Horizontal padding for controls of this size.
    pub const fn px_x(self) -> Pixels {
        match self {
            Self::Xs => px(8.),
            Self::Sm => px(10.),
            Self::Md => px(12.),
            Self::Lg => px(16.),
            Self::Xl => px(20.),
        }
    }

    /// Font size in pixels for controls of this size.
    pub const fn font_size(self) -> Pixels {
        match self {
            Self::Xs => px(12.),
            Self::Sm => px(13.),
            Self::Md => px(14.),
            Self::Lg => px(15.),
            Self::Xl => px(16.),
        }
    }

    /// Default Lucide icon size matching this control size.
    pub const fn icon_size(self) -> crate::icons::IconSize {
        match self {
            Self::Xs => crate::icons::IconSize::Xs,
            Self::Sm => crate::icons::IconSize::Sm,
            Self::Md => crate::icons::IconSize::Md,
            Self::Lg => crate::icons::IconSize::Lg,
            Self::Xl => crate::icons::IconSize::Lg,
        }
    }
}

/// Semantic Tailwind-inspired color & radius tokens used by all `gpui::components`.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Active light or dark mode.
    pub mode: ThemeMode,
    /// Page / window canvas background.
    pub background: Hsla,
    /// Elevated card / panel background.
    pub surface: Hsla,
    /// Subtle muted surface background (headers, input addons, secondary hover).
    pub surface_muted: Hsla,
    /// Surface hover fill.
    pub surface_hover: Hsla,
    /// Surface active / pressed fill.
    pub surface_active: Hsla,
    /// Primary text color.
    pub foreground: Hsla,
    /// Secondary / muted text color.
    pub muted_foreground: Hsla,
    /// Placeholder text color.
    pub placeholder: Hsla,
    /// Primary brand accent (Tailwind Indigo 600 / 500).
    pub primary: Hsla,
    /// Primary hover fill.
    pub primary_hover: Hsla,
    /// Text color on top of `primary`.
    pub primary_foreground: Hsla,
    /// Soft primary tint background (for soft buttons, selected chips, active items).
    pub primary_soft: Hsla,
    /// Text color on top of `primary_soft`.
    pub primary_soft_foreground: Hsla,
    /// Secondary button / surface fill.
    pub secondary: Hsla,
    /// Secondary hover fill.
    pub secondary_hover: Hsla,
    /// Text on secondary fill.
    pub secondary_foreground: Hsla,
    /// Destructive / danger solid fill (Tailwind Red 600).
    pub destructive: Hsla,
    /// Destructive hover fill.
    pub destructive_hover: Hsla,
    /// Text on destructive fill.
    pub destructive_foreground: Hsla,
    /// Soft destructive tint background.
    pub destructive_soft: Hsla,
    /// Text on soft destructive tint.
    pub destructive_soft_foreground: Hsla,
    /// Success accent color.
    pub success: Hsla,
    /// Soft success background.
    pub success_soft: Hsla,
    /// Text on soft success background.
    pub success_soft_foreground: Hsla,
    /// Warning accent color.
    pub warning: Hsla,
    /// Soft warning background.
    pub warning_soft: Hsla,
    /// Text on soft warning background.
    pub warning_soft_foreground: Hsla,
    /// Info accent color.
    pub info: Hsla,
    /// Soft info background.
    pub info_soft: Hsla,
    /// Text on soft info background.
    pub info_soft_foreground: Hsla,
    /// Default divider / card border color.
    pub border: Hsla,
    /// Border color for form inputs & controls.
    pub input_border: Hsla,
    /// Border color on hover for controls.
    pub input_border_hover: Hsla,
    /// Focus ring color.
    pub ring: Hsla,
    /// Text selection highlight color.
    pub selection: Hsla,
    /// Floating popover / dropdown / modal surface.
    pub popover: Hsla,
    /// Text color inside floating popovers.
    pub popover_foreground: Hsla,
    /// Modal backdrop scrim.
    pub backdrop: Hsla,
    /// Disabled background tint.
    pub disabled_bg: Hsla,
    /// Disabled text / icon foreground.
    pub disabled_foreground: Hsla,
    /// Small border radius (`4px`).
    pub radius_sm: Pixels,
    /// Medium border radius (`6px`, Tailwind `rounded-md`).
    pub radius_md: Pixels,
    /// Large border radius (`8px`, Tailwind `rounded-lg`).
    pub radius_lg: Pixels,
    /// Extra large border radius (`12px`, Tailwind `rounded-xl`).
    pub radius_xl: Pixels,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

impl Theme {
    /// Tailwind-inspired light theme.
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            background: tw::SLATE_50.into(),
            surface: tw::WHITE.into(),
            surface_muted: tw::SLATE_100.into(),
            surface_hover: tw::SLATE_100.into(),
            surface_active: tw::SLATE_200.into(),
            foreground: tw::SLATE_900.into(),
            muted_foreground: tw::SLATE_500.into(),
            placeholder: tw::SLATE_400.into(),
            primary: tw::INDIGO_600.into(),
            primary_hover: tw::INDIGO_700.into(),
            primary_foreground: tw::WHITE.into(),
            primary_soft: tw::INDIGO_50.into(),
            primary_soft_foreground: tw::INDIGO_700.into(),
            secondary: tw::WHITE.into(),
            secondary_hover: tw::SLATE_50.into(),
            secondary_foreground: tw::SLATE_900.into(),
            destructive: tw::RED_600.into(),
            destructive_hover: tw::RED_700.into(),
            destructive_foreground: tw::WHITE.into(),
            destructive_soft: tw::RED_50.into(),
            destructive_soft_foreground: tw::RED_700.into(),
            success: tw::EMERALD_600.into(),
            success_soft: tw::EMERALD_50.into(),
            success_soft_foreground: tw::EMERALD_700.into(),
            warning: tw::AMBER_500.into(),
            warning_soft: tw::AMBER_50.into(),
            warning_soft_foreground: tw::AMBER_800.into(),
            info: tw::BLUE_600.into(),
            info_soft: tw::BLUE_50.into(),
            info_soft_foreground: tw::BLUE_700.into(),
            border: tw::SLATE_200.into(),
            input_border: tw::SLATE_300.into(),
            input_border_hover: tw::SLATE_400.into(),
            ring: tw::INDIGO_600.into(),
            selection: Hsla::from(tw::INDIGO_500).opacity(0.22),
            popover: tw::WHITE.into(),
            popover_foreground: tw::SLATE_900.into(),
            backdrop: Hsla::from(tw::SLATE_950).opacity(0.45),
            disabled_bg: tw::SLATE_100.into(),
            disabled_foreground: tw::SLATE_400.into(),
            radius_sm: px(4.),
            radius_md: px(6.),
            radius_lg: px(8.),
            radius_xl: px(12.),
        }
    }

    /// Tailwind-inspired dark theme.
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            background: tw::SLATE_950.into(),
            surface: tw::SLATE_900.into(),
            surface_muted: tw::SLATE_800.into(),
            surface_hover: tw::SLATE_800.into(),
            surface_active: tw::SLATE_700.into(),
            foreground: tw::SLATE_50.into(),
            muted_foreground: tw::SLATE_400.into(),
            placeholder: tw::SLATE_500.into(),
            primary: tw::INDIGO_500.into(),
            primary_hover: tw::INDIGO_400.into(),
            primary_foreground: tw::WHITE.into(),
            primary_soft: Hsla::from(tw::INDIGO_500).opacity(0.18),
            primary_soft_foreground: tw::INDIGO_300.into(),
            secondary: tw::SLATE_800.into(),
            secondary_hover: tw::SLATE_700.into(),
            secondary_foreground: tw::SLATE_100.into(),
            destructive: tw::RED_600.into(),
            destructive_hover: tw::RED_500.into(),
            destructive_foreground: tw::WHITE.into(),
            destructive_soft: Hsla::from(tw::RED_500).opacity(0.16),
            destructive_soft_foreground: tw::RED_300.into(),
            success: tw::EMERALD_500.into(),
            success_soft: Hsla::from(tw::EMERALD_500).opacity(0.16),
            success_soft_foreground: tw::EMERALD_300.into(),
            warning: tw::AMBER_400.into(),
            warning_soft: Hsla::from(tw::AMBER_500).opacity(0.16),
            warning_soft_foreground: tw::AMBER_300.into(),
            info: tw::BLUE_400.into(),
            info_soft: Hsla::from(tw::BLUE_500).opacity(0.16),
            info_soft_foreground: tw::BLUE_300.into(),
            border: tw::SLATE_800.into(),
            input_border: tw::SLATE_700.into(),
            input_border_hover: tw::SLATE_600.into(),
            ring: tw::INDIGO_400.into(),
            selection: Hsla::from(tw::INDIGO_400).opacity(0.32),
            popover: tw::SLATE_900.into(),
            popover_foreground: tw::SLATE_50.into(),
            backdrop: Hsla::from(tw::BLACK).opacity(0.65),
            disabled_bg: tw::SLATE_900.into(),
            disabled_foreground: tw::SLATE_600.into(),
            radius_sm: px(4.),
            radius_md: px(6.),
            radius_lg: px(8.),
            radius_xl: px(12.),
        }
    }

    /// Creates a theme matching the window's light/dark appearance.
    pub fn for_window(window: &Window) -> Self {
        match ThemeMode::from(window.appearance()) {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
    }

    /// Returns a cloned active theme from `cx`, or the default light theme if none is installed.
    pub fn active(cx: &App) -> Arc<Self> {
        cx.try_global::<GlobalTheme>()
            .map(|global| global.0.clone())
            .unwrap_or_else(|| Arc::new(Self::light()))
    }

    /// Installs `theme` as the global active theme.
    pub fn set_global(theme: Self, cx: &mut App) {
        cx.set_global(GlobalTheme(Arc::new(theme)));
    }

    /// Switches the active global theme to `mode`.
    pub fn set_mode(mode: ThemeMode, cx: &mut App) {
        let theme = match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        };
        Self::set_global(theme, cx);
    }

    /// Toggles the active global theme between [`ThemeMode::Light`] and [`ThemeMode::Dark`].
    pub fn toggle_mode(cx: &mut App) {
        let next = match Self::active(cx).mode {
            ThemeMode::Light => ThemeMode::Dark,
            ThemeMode::Dark => ThemeMode::Light,
        };
        Self::set_mode(next, cx);
    }
}

/// Global wrapper storing the active [`Theme`].
#[derive(Clone, Debug)]
pub struct GlobalTheme(pub Arc<Theme>);

impl Global for GlobalTheme {}

/// Convenience extension trait for reading the active [`Theme`] from [`App`].
pub trait ActiveTheme {
    /// Returns the active [`Theme`] (falling back to [`Theme::light`] if unset).
    fn theme(&self) -> Arc<Theme>;
}

impl ActiveTheme for App {
    fn theme(&self) -> Arc<Theme> {
        Theme::active(self)
    }
}

/// Initializes the default [`Theme`] global and registers standard keyboard bindings
/// for all interactive GPUI components (`Input`, `TextBox`, `Select`, `ComboBox`, etc.).
pub fn init_components(cx: &mut App) {
    if !cx.has_global::<GlobalTheme>() {
        Theme::set_global(Theme::light(), cx);
    }
    super::input::bind_input_keys(cx);
    super::textbox::bind_textbox_keys(cx);
}

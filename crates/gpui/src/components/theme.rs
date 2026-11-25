//! Semantic design tokens, light/dark themes, sizing tokens, and component initialization.

use std::sync::{Arc, OnceLock};
use std::time::Duration;

use crate::{
    App, Bounds, FontFeatures, Global, Hsla, Pixels, Point, Rgba, SharedString, SpringConfig,
    Subscription, ViewTransition, Window, WindowAppearance, WindowBackgroundAppearance, px, rgb,
};

use super::palette;
use super::syntax::SyntaxTheme;

/// Returns a shared [`FontFeatures`] instance enabling OpenType tabular numerals (`tnum`).
///
/// Backed by a [`OnceLock`] so every call performs a cheap [`Arc`] refcount increment with
/// zero heap allocations, avoiding per-tick and per-cell allocations in charts, KPI strips,
/// and facet counters.
pub fn tabular_numerals() -> FontFeatures {
    static TNUM: OnceLock<FontFeatures> = OnceLock::new();
    TNUM.get_or_init(|| FontFeatures(Arc::new(vec![("tnum".to_string(), 1)])))
        .clone()
}

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

/// User-facing theme selection preference (`Light`, `System`, or `Dark`).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemePreference {
    /// Always use the light theme.
    #[default]
    Light,
    /// Follow the operating system's light/dark window appearance dynamically.
    System,
    /// Always use the dark theme.
    Dark,
}

impl ThemePreference {
    /// Resolves this preference into a concrete [`ThemeMode`] given the current `appearance`.
    pub fn resolve(self, appearance: WindowAppearance) -> ThemeMode {
        match self {
            Self::Light => ThemeMode::Light,
            Self::Dark => ThemeMode::Dark,
            Self::System => ThemeMode::from(appearance),
        }
    }
}

/// Whether the active UI theme is light or dark.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ThemeMode {
    /// Crisp light aesthetic (white surfaces, slate-900 ink, indigo-600 primary).
    #[default]
    Light,
    /// Deep dark aesthetic (slate-950 canvas, slate-900 surfaces, indigo-500 primary).
    Dark,
}

impl ThemeMode {
    /// Returns the approximate native OS backdrop color behind translucent window surfaces for
    /// this theme mode (e.g. WinUI 3 Dark Mica `#202020` / Light Mica `#f3f3f3`), or `None` for
    /// opaque or purely transparent windows where the OS does not flip a backdrop tint.
    pub fn native_backdrop_color(
        self,
        background_appearance: WindowBackgroundAppearance,
    ) -> Option<Rgba> {
        match background_appearance {
            WindowBackgroundAppearance::MicaBackdrop
            | WindowBackgroundAppearance::MicaAltBackdrop => Some(match self {
                Self::Dark => rgb(0x202020),
                Self::Light => rgb(0xf3f3f3),
            }),
            WindowBackgroundAppearance::Blurred => Some(match self {
                Self::Dark => rgb(0x1c1c1c),
                Self::Light => rgb(0xf3f3f3),
            }),
            WindowBackgroundAppearance::Opaque | WindowBackgroundAppearance::Transparent => None,
        }
    }
}

impl From<WindowAppearance> for ThemeMode {
    fn from(appearance: WindowAppearance) -> Self {
        match appearance {
            WindowAppearance::Light | WindowAppearance::VibrantLight => Self::Light,
            WindowAppearance::Dark | WindowAppearance::VibrantDark => Self::Dark,
        }
    }
}

/// Shared size scale across interactive components.
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

/// Semantic color & radius tokens used by all `gpui::components`.
#[derive(Clone, Debug)]
pub struct Theme {
    /// Active light or dark mode.
    pub mode: ThemeMode,
    /// User preference (`Light`, `System`, or `Dark`) that produced `mode`.
    pub preference: ThemePreference,
    /// Page / window canvas background.
    pub background: Hsla,
    /// Left sidebar panel background (defaults to `surface`; customizable independently of cards).
    pub sidebar_bg: Hsla,
    /// Sidebar divider border color.
    pub sidebar_border: Hsla,
    /// Active [`SidebarItem`](super::SidebarItem) pill background.
    pub sidebar_active_bg: Hsla,
    /// Active [`SidebarItem`](super::SidebarItem) label color.
    pub sidebar_active_foreground: Hsla,
    /// Active [`SidebarItem`](super::SidebarItem) icon color.
    pub sidebar_active_icon: Hsla,
    /// Elevated card / panel background.
    pub surface: Hsla,
    /// Recessed background for inputs, selects, comboboxes, filter bars, and pickers.
    pub input_bg: Hsla,
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
    /// Primary brand accent (Indigo 600 / 500).
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
    /// Destructive / danger solid fill (Red 600).
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
    /// Unchecked [`Toggle`](super::Toggle) track background.
    pub toggle_track_off: Hsla,
    /// Checked [`Toggle`](super::Toggle) track background.
    pub toggle_track_on: Hsla,
    /// Circular [`Toggle`](super::Toggle) thumb fill.
    pub toggle_thumb: Hsla,
    /// Unfilled [`Slider`](super::Slider) rail background.
    pub slider_track: Hsla,
    /// Filled [`Slider`](super::Slider) range highlight.
    pub slider_fill: Hsla,
    /// Circular [`Slider`](super::Slider) thumb fill.
    pub slider_thumb: Hsla,
    /// [`Tooltip`](super::Tooltip) container background.
    pub tooltip_bg: Hsla,
    /// [`Tooltip`](super::Tooltip) text color.
    pub tooltip_foreground: Hsla,
    /// [`Tooltip`](super::Tooltip) border color.
    pub tooltip_border: Hsla,
    /// Ordered categorical color palette used by [`charts`](super::charts).
    pub chart_colors: Vec<Hsla>,
    /// Muted baseline/comparison line color used by [`charts`](super::charts).
    pub chart_baseline: Hsla,
    /// Hairline chart gridline color.
    pub chart_gridline: Hsla,
    /// Optional default UI font family applied at root shells (`AppShell`).
    pub font_family: Option<SharedString>,
    /// Optional monospace font family applied on [`Kbd`](super::Kbd) and code surfaces.
    pub mono_font_family: Option<SharedString>,
    /// Syntax highlighting color palette shared by [`Markdown`](super::Markdown) and
    /// [`CodeBlock`](super::CodeBlock).
    pub syntax: SyntaxTheme,
    /// Spread width for focus ring shadows (`px(0.)` produces a crisp border-only focus state).
    pub focus_ring_width: Pixels,
    /// Whether cards, inputs, buttons, and strips apply subtle drop shadows (`false` for flat chrome).
    pub elevation_shadows: bool,
    /// Small border radius (`4px`).
    pub radius_sm: Pixels,
    /// Medium border radius (`6px`, `rounded-md`).
    pub radius_md: Pixels,
    /// Large border radius (`8px`, `rounded-lg`).
    pub radius_lg: Pixels,
    /// Extra large border radius (`12px`, `rounded-xl`).
    pub radius_xl: Pixels,
}

impl Default for Theme {
    fn default() -> Self {
        Self::light()
    }
}

/// Visual GPU shader preset shared by both full-window theme transitions and component-scoped
/// transitions ([`TransitionView`](super::TransitionView)).
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TransitionStyle {
    /// Crisp expanding circular reveal from the click point.
    #[default]
    RadialReveal,
    /// Expanding refractive optical shockwave with RGB chromatic dispersion and caustic rim.
    Shockwave,
    /// 3D cylindrical page-curl with specular ridge, frosted underside, and cast shadow.
    PageCurl,
    /// Organic domain-warped liquid dissolve with edge refraction and iridescent meniscus.
    LiquidDissolve,
    /// Cinema hyper-zoom & whip-spin warp with mirror-tiled radial streaks and heavy chromatic aberration.
    ChromaticWarp,
    /// Staged blur-out (0–0.2s), opacity cross-fade (0.1–0.2s), and blur-in (0.1–0.3s).
    BlurFade,
    /// Asynchronous digital glitch blocks & RGB tear strips flipping over 0.3s.
    Glitch,
    /// Organic double-blob radial reveal with prismatic chromatic aberration between inner & outer blobs.
    PrismaticBlob,
    /// Gentle cross-fade.
    CrossFade,
}

/// Backward-compatible alias for [`TransitionStyle`] used by [`ThemeSwitcher`](super::ThemeSwitcher).
pub type ThemeTransitionStyle = TransitionStyle;

impl TransitionStyle {
    /// All available transition styles in display order.
    pub const ALL: [Self; 9] = [
        Self::ChromaticWarp,
        Self::PrismaticBlob,
        Self::Glitch,
        Self::BlurFade,
        Self::Shockwave,
        Self::PageCurl,
        Self::LiquidDissolve,
        Self::RadialReveal,
        Self::CrossFade,
    ];

    /// Short human-readable label for UI pickers.
    pub const fn label(self) -> &'static str {
        match self {
            Self::RadialReveal => "Radial",
            Self::Shockwave => "Shockwave",
            Self::PageCurl => "Page Curl",
            Self::LiquidDissolve => "Liquid",
            Self::ChromaticWarp => "Chromatic Warp",
            Self::BlurFade => "Blur Fade",
            Self::Glitch => "Glitch",
            Self::PrismaticBlob => "Prismatic Blob",
            Self::CrossFade => "Cross-Fade",
        }
    }

    /// Converts this style into a full-window GPU [`ViewTransition`] for `window`.
    pub fn to_view_transition(
        self,
        origin: Option<Point<Pixels>>,
        window: &Window,
    ) -> ViewTransition {
        let viewport_bounds = Bounds {
            origin: crate::point(px(0.), px(0.)),
            size: window.viewport_size(),
        };
        self.build_transition(origin, viewport_bounds, true)
    }

    /// Converts this style into a component-scoped GPU [`ViewTransition`] bounded to `bounds`.
    pub fn to_component_transition(
        self,
        origin: Option<Point<Pixels>>,
        bounds: Bounds<Pixels>,
    ) -> ViewTransition {
        self.build_transition(origin, bounds, false).in_bounds(bounds)
    }

    fn build_transition(
        self,
        origin: Option<Point<Pixels>>,
        target_bounds: Bounds<Pixels>,
        crossfade_radial_without_origin: bool,
    ) -> ViewTransition {
        let fallback_center = || target_bounds.center();
        let fallback_corner = || crate::point(target_bounds.right(), target_bounds.top());
        match self {
            Self::RadialReveal => match origin {
                Some(pt) => ViewTransition::radial_reveal(pt),
                None if crossfade_radial_without_origin => ViewTransition::cross_fade(),
                None => ViewTransition::radial_reveal(fallback_center()),
            },
            Self::Shockwave => ViewTransition::shockwave(origin.unwrap_or_else(fallback_center)),
            Self::PageCurl => ViewTransition::page_curl(origin.unwrap_or_else(fallback_corner)),
            Self::LiquidDissolve => {
                ViewTransition::liquid_dissolve(origin.unwrap_or_else(fallback_center))
            }
            Self::ChromaticWarp => {
                ViewTransition::chromatic_warp(origin.unwrap_or_else(fallback_center))
            }
            Self::BlurFade => ViewTransition::blur_fade(),
            Self::Glitch => ViewTransition::glitch(),
            Self::PrismaticBlob => {
                ViewTransition::prismatic_blob(origin.unwrap_or_else(fallback_center))
            }
            Self::CrossFade => ViewTransition::cross_fade(),
        }
    }
}

impl Theme {
    /// Light theme.
    pub fn light() -> Self {
        Self {
            mode: ThemeMode::Light,
            preference: ThemePreference::Light,
            background: palette::SLATE_50.into(),
            sidebar_bg: palette::WHITE.into(),
            sidebar_border: palette::SLATE_200.into(),
            sidebar_active_bg: palette::INDIGO_50.into(),
            sidebar_active_foreground: palette::INDIGO_700.into(),
            sidebar_active_icon: palette::INDIGO_600.into(),
            surface: palette::WHITE.into(),
            input_bg: palette::WHITE.into(),
            surface_muted: palette::SLATE_100.into(),
            surface_hover: palette::SLATE_100.into(),
            surface_active: palette::SLATE_200.into(),
            foreground: palette::SLATE_900.into(),
            muted_foreground: palette::SLATE_500.into(),
            placeholder: palette::SLATE_400.into(),
            primary: palette::INDIGO_600.into(),
            primary_hover: palette::INDIGO_700.into(),
            primary_foreground: palette::WHITE.into(),
            primary_soft: palette::INDIGO_50.into(),
            primary_soft_foreground: palette::INDIGO_700.into(),
            secondary: palette::WHITE.into(),
            secondary_hover: palette::SLATE_50.into(),
            secondary_foreground: palette::SLATE_900.into(),
            destructive: palette::RED_600.into(),
            destructive_hover: palette::RED_700.into(),
            destructive_foreground: palette::WHITE.into(),
            destructive_soft: palette::RED_50.into(),
            destructive_soft_foreground: palette::RED_700.into(),
            success: palette::EMERALD_600.into(),
            success_soft: palette::EMERALD_50.into(),
            success_soft_foreground: palette::EMERALD_700.into(),
            warning: palette::AMBER_500.into(),
            warning_soft: palette::AMBER_50.into(),
            warning_soft_foreground: palette::AMBER_800.into(),
            info: palette::BLUE_600.into(),
            info_soft: palette::BLUE_50.into(),
            info_soft_foreground: palette::BLUE_700.into(),
            border: palette::SLATE_200.into(),
            input_border: palette::SLATE_300.into(),
            input_border_hover: palette::SLATE_400.into(),
            ring: palette::INDIGO_600.into(),
            selection: Hsla::from(palette::INDIGO_500).opacity(0.22),
            popover: palette::WHITE.into(),
            popover_foreground: palette::SLATE_900.into(),
            backdrop: Hsla::from(palette::SLATE_950).opacity(0.45),
            disabled_bg: palette::SLATE_100.into(),
            disabled_foreground: palette::SLATE_400.into(),
            toggle_track_off: palette::SLATE_200.into(),
            toggle_track_on: palette::INDIGO_600.into(),
            toggle_thumb: palette::WHITE.into(),
            slider_track: palette::SLATE_200.into(),
            slider_fill: palette::INDIGO_600.into(),
            slider_thumb: palette::WHITE.into(),
            tooltip_bg: palette::SLATE_900.into(),
            tooltip_foreground: palette::WHITE.into(),
            tooltip_border: palette::SLATE_800.into(),
            chart_colors: vec![
                palette::INDIGO_500.into(),
                palette::EMERALD_500.into(),
                palette::AMBER_500.into(),
                palette::SKY_500.into(),
                palette::ROSE_500.into(),
                palette::VIOLET_500.into(),
            ],
            chart_baseline: palette::SLATE_400.into(),
            chart_gridline: Hsla::from(palette::SLATE_200).opacity(0.85),
            font_family: None,
            mono_font_family: None,
            syntax: SyntaxTheme::light(),
            focus_ring_width: px(3.),
            elevation_shadows: true,
            radius_sm: px(4.),
            radius_md: px(6.),
            radius_lg: px(8.),
            radius_xl: px(12.),
        }
    }

    /// Dark theme.
    pub fn dark() -> Self {
        Self {
            mode: ThemeMode::Dark,
            preference: ThemePreference::Dark,
            background: palette::SLATE_950.into(),
            sidebar_bg: palette::SLATE_900.into(),
            sidebar_border: palette::SLATE_800.into(),
            sidebar_active_bg: Hsla::from(palette::INDIGO_500).opacity(0.18),
            sidebar_active_foreground: palette::INDIGO_300.into(),
            sidebar_active_icon: palette::INDIGO_400.into(),
            surface: palette::SLATE_900.into(),
            input_bg: palette::SLATE_950.into(),
            surface_muted: palette::SLATE_800.into(),
            surface_hover: palette::SLATE_800.into(),
            surface_active: palette::SLATE_700.into(),
            foreground: palette::SLATE_50.into(),
            muted_foreground: palette::SLATE_400.into(),
            placeholder: palette::SLATE_500.into(),
            primary: palette::INDIGO_500.into(),
            primary_hover: palette::INDIGO_400.into(),
            primary_foreground: palette::WHITE.into(),
            primary_soft: Hsla::from(palette::INDIGO_500).opacity(0.18),
            primary_soft_foreground: palette::INDIGO_300.into(),
            secondary: palette::SLATE_800.into(),
            secondary_hover: palette::SLATE_700.into(),
            secondary_foreground: palette::SLATE_100.into(),
            destructive: palette::RED_600.into(),
            destructive_hover: palette::RED_500.into(),
            destructive_foreground: palette::WHITE.into(),
            destructive_soft: Hsla::from(palette::RED_500).opacity(0.16),
            destructive_soft_foreground: palette::RED_300.into(),
            success: palette::EMERALD_500.into(),
            success_soft: Hsla::from(palette::EMERALD_500).opacity(0.16),
            success_soft_foreground: palette::EMERALD_300.into(),
            warning: palette::AMBER_400.into(),
            warning_soft: Hsla::from(palette::AMBER_500).opacity(0.16),
            warning_soft_foreground: palette::AMBER_300.into(),
            info: palette::BLUE_400.into(),
            info_soft: Hsla::from(palette::BLUE_500).opacity(0.16),
            info_soft_foreground: palette::BLUE_300.into(),
            border: palette::SLATE_800.into(),
            input_border: palette::SLATE_700.into(),
            input_border_hover: palette::SLATE_600.into(),
            ring: palette::INDIGO_400.into(),
            selection: Hsla::from(palette::INDIGO_400).opacity(0.32),
            popover: palette::SLATE_900.into(),
            popover_foreground: palette::SLATE_50.into(),
            backdrop: Hsla::from(palette::BLACK).opacity(0.65),
            disabled_bg: palette::SLATE_900.into(),
            disabled_foreground: palette::SLATE_600.into(),
            toggle_track_off: palette::SLATE_700.into(),
            toggle_track_on: palette::INDIGO_500.into(),
            toggle_thumb: palette::WHITE.into(),
            slider_track: palette::SLATE_800.into(),
            slider_fill: palette::INDIGO_500.into(),
            slider_thumb: palette::WHITE.into(),
            tooltip_bg: palette::SLATE_800.into(),
            tooltip_foreground: palette::SLATE_50.into(),
            tooltip_border: palette::SLATE_700.into(),
            chart_colors: vec![
                palette::INDIGO_400.into(),
                palette::EMERALD_400.into(),
                palette::AMBER_400.into(),
                palette::SKY_400.into(),
                palette::ROSE_400.into(),
                palette::VIOLET_400.into(),
            ],
            chart_baseline: palette::SLATE_500.into(),
            chart_gridline: Hsla::from(palette::SLATE_700).opacity(0.65),
            font_family: None,
            mono_font_family: None,
            syntax: SyntaxTheme::dark(),
            focus_ring_width: px(3.),
            elevation_shadows: true,
            radius_sm: px(4.),
            radius_md: px(6.),
            radius_lg: px(8.),
            radius_xl: px(12.),
        }
    }

    /// Creates a theme matching the window's light/dark appearance with [`ThemePreference::System`].
    pub fn for_window(window: &Window) -> Self {
        let mut theme = match ThemeMode::from(window.appearance()) {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        };
        theme.preference = ThemePreference::System;
        theme
    }

    /// Returns a copy of this theme tagged with the given [`ThemePreference`].
    pub fn with_preference(mut self, preference: ThemePreference) -> Self {
        self.preference = preference;
        self
    }

    /// Re-tints the entire theme around a single brand `accent` color, automatically deriving
    /// harmonious hover, soft tint, readable foreground, focus ring, selection, active sidebar,
    /// toggle, slider, and primary chart tokens.
    pub fn with_accent(mut self, accent: Hsla) -> Self {
        let is_dark = self.mode == ThemeMode::Dark;
        let primary_fg: Hsla = if accent.l > 0.68 {
            palette::SLATE_950.into()
        } else {
            palette::WHITE.into()
        };
        let hover = Hsla {
            l: if is_dark {
                (accent.l + 0.06).min(0.92)
            } else {
                (accent.l - 0.06).max(0.08)
            },
            ..accent
        };
        let soft = if is_dark {
            accent.opacity(0.18)
        } else {
            Hsla {
                l: 0.96,
                s: (accent.s * 0.85).min(1.0),
                a: 1.0,
                ..accent
            }
        };
        let soft_fg = if is_dark {
            Hsla {
                l: (accent.l + 0.16).min(0.88),
                ..accent
            }
        } else {
            Hsla {
                l: (accent.l - 0.10).max(0.18),
                ..accent
            }
        };

        self.primary = accent;
        self.primary_hover = hover;
        self.primary_foreground = primary_fg;
        self.primary_soft = soft;
        self.primary_soft_foreground = soft_fg;
        self.ring = accent;
        self.selection = accent.opacity(if is_dark { 0.30 } else { 0.22 });
        self.sidebar_active_bg = soft;
        self.sidebar_active_foreground = soft_fg;
        self.sidebar_active_icon = accent;
        self.toggle_track_on = accent;
        self.slider_fill = accent;
        if let Some(first) = self.chart_colors.first_mut() {
            *first = accent;
        }
        self
    }

    /// Customizes the four core surface elevation tiers: canvas `background`, `sidebar`,
    /// elevated card `surface`, and recessed `input_bg`.
    pub fn with_surfaces(
        mut self,
        background: Hsla,
        sidebar: Hsla,
        surface: Hsla,
        input_bg: Hsla,
    ) -> Self {
        self.background = background;
        self.sidebar_bg = sidebar;
        self.surface = surface;
        self.input_bg = input_bg;
        self.popover = surface;
        self.secondary = input_bg;
        self
    }

    /// Customizes subtle muted, hover, and active interactive surface fills.
    pub fn with_surface_states(mut self, muted: Hsla, hover: Hsla, active: Hsla) -> Self {
        self.surface_muted = muted;
        self.surface_hover = hover;
        self.surface_active = active;
        self.secondary_hover = hover;
        self
    }

    /// Customizes card/divider `border`, default `input_border`, and `input_border_hover`.
    pub fn with_borders(
        mut self,
        border: Hsla,
        input_border: Hsla,
        input_border_hover: Hsla,
    ) -> Self {
        self.border = border;
        self.sidebar_border = border;
        self.input_border = input_border;
        self.input_border_hover = input_border_hover;
        self
    }

    /// Customizes primary, muted, and placeholder text foreground colors.
    pub fn with_text_colors(
        mut self,
        foreground: Hsla,
        muted_foreground: Hsla,
        placeholder: Hsla,
    ) -> Self {
        self.foreground = foreground;
        self.popover_foreground = foreground;
        self.secondary_foreground = foreground;
        self.muted_foreground = muted_foreground;
        self.placeholder = placeholder;
        self
    }

    /// Customizes sidebar surface, divider border, and active navigation pill styling.
    pub fn with_sidebar_colors(
        mut self,
        bg: Hsla,
        border: Hsla,
        active_bg: Hsla,
        active_foreground: Hsla,
        active_icon: Hsla,
    ) -> Self {
        self.sidebar_bg = bg;
        self.sidebar_border = border;
        self.sidebar_active_bg = active_bg;
        self.sidebar_active_foreground = active_foreground;
        self.sidebar_active_icon = active_icon;
        self
    }

    /// Customizes [`Toggle`](super::Toggle) track and thumb colors.
    pub fn with_toggle_colors(mut self, track_off: Hsla, track_on: Hsla, thumb: Hsla) -> Self {
        self.toggle_track_off = track_off;
        self.toggle_track_on = track_on;
        self.toggle_thumb = thumb;
        self
    }

    /// Customizes the four corner radius tokens (`sm`, `md`, `lg`, `xl`).
    pub fn with_radii(mut self, sm: Pixels, md: Pixels, lg: Pixels, xl: Pixels) -> Self {
        self.radius_sm = sm;
        self.radius_md = md;
        self.radius_lg = lg;
        self.radius_xl = xl;
        self
    }

    /// Sets the focus ring shadow spread width (`px(0.)` gives a crisp border-only focus state).
    pub fn with_focus_ring_width(mut self, width: Pixels) -> Self {
        self.focus_ring_width = width.max(px(0.));
        self
    }

    /// Toggles elevation drop shadows on cards, inputs, buttons, and panels.
    pub fn with_elevation_shadows(mut self, enabled: bool) -> Self {
        self.elevation_shadows = enabled;
        self
    }

    /// Sets the default UI font family applied on root shells.
    pub fn with_font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.font_family = Some(family.into());
        self
    }

    /// Sets the default monospace font family applied on [`Kbd`](super::Kbd) and code surfaces.
    pub fn with_mono_font_family(mut self, family: impl Into<SharedString>) -> Self {
        self.mono_font_family = Some(family.into());
        self
    }

    /// Overrides the [`SyntaxTheme`] palette used by [`Markdown`](super::Markdown) and
    /// [`CodeBlock`](super::CodeBlock).
    pub fn with_syntax_theme(mut self, syntax: SyntaxTheme) -> Self {
        self.syntax = syntax;
        self
    }

    /// Overrides the ordered categorical series color palette for [`charts`](super::charts).
    pub fn with_chart_colors(mut self, colors: impl IntoIterator<Item = Hsla>) -> Self {
        let collected: Vec<Hsla> = colors.into_iter().collect();
        if !collected.is_empty() {
            self.chart_colors = collected;
        }
        self
    }

    /// Crisp neutral light theme using achromatic zinc/neutral grays (free of blue-slate tint)
    /// for calm, paper-clean daytime readability.
    pub fn light_neutral() -> Self {
        let mut t = Self::light();
        t.background = palette::ZINC_50.into();
        t.sidebar_bg = palette::WHITE.into();
        t.sidebar_border = palette::ZINC_200.into();
        t.surface = palette::WHITE.into();
        t.input_bg = palette::WHITE.into();
        t.surface_muted = palette::ZINC_100.into();
        t.surface_hover = palette::ZINC_100.into();
        t.surface_active = palette::ZINC_200.into();
        t.foreground = palette::ZINC_900.into();
        t.muted_foreground = palette::ZINC_600.into();
        t.placeholder = palette::ZINC_500.into();
        t.secondary = palette::WHITE.into();
        t.secondary_hover = palette::ZINC_50.into();
        t.secondary_foreground = palette::ZINC_900.into();
        t.border = palette::ZINC_200.into();
        t.input_border = palette::ZINC_300.into();
        t.input_border_hover = palette::ZINC_400.into();
        t.popover_foreground = palette::ZINC_900.into();
        t.backdrop = Hsla::from(palette::ZINC_950).opacity(0.42);
        t.disabled_bg = palette::ZINC_100.into();
        t.disabled_foreground = palette::ZINC_400.into();
        t.toggle_track_off = palette::ZINC_200.into();
        t.slider_track = palette::ZINC_200.into();
        t.tooltip_bg = palette::ZINC_900.into();
        t.tooltip_border = palette::ZINC_800.into();
        t.chart_baseline = palette::ZINC_400.into();
        t.chart_gridline = Hsla::from(palette::ZINC_200).opacity(0.85);
        t
    }

    /// Translucent matte-glass dark theme designed for Windows 11 `MicaBackdrop` or `Blurred`
    /// frosted floating panes and Spotlight launchers (keeps inputs and popovers highly legible).
    pub fn dark_glass() -> Self {
        Self::dark().with_vibrancy(0.68)
    }

    /// Translucent frosted-porcelain light theme designed for Windows 11 `MicaBackdrop` or
    /// `Blurred` floating panes and Spotlight launchers.
    pub fn light_glass() -> Self {
        Self::light().with_vibrancy(0.76)
    }

    /// Applies layered surface translucency onto `self` so OS backdrop blur/Mica shines through
    /// while keeping text inputs, dropdowns, and popovers crisp and legible.
    pub fn with_vibrancy(mut self, surface_alpha: f32) -> Self {
        let a = surface_alpha.clamp(0.15, 0.98);
        self.background.a = (a * 0.82).clamp(0.15, 0.95);
        self.sidebar_bg.a = (a * 0.65).clamp(0.15, 0.92);
        self.surface.a = a;
        self.surface_muted.a = (a * 0.85).clamp(0.20, 0.92);
        self.input_bg.a = 0.92;
        self.popover.a = 0.95;
        self.border = Self::edge_for_opacity(self.border, a, self.mode == ThemeMode::Dark);
        self
    }

    /// Returns a translucent surface color with absolute alpha `opacity` clamped to `[0.05, 1.0]`.
    pub fn surface_with_opacity(&self, opacity: f32) -> Hsla {
        Hsla {
            a: opacity.clamp(0.05, 1.0),
            ..self.surface
        }
    }

    /// Computes an opacity-adaptive border color that subtly boosts rim contrast as glass opacity
    /// decreases so sheer floating panes stay crisp over both bright and dark wallpapers.
    pub fn edge_for_opacity(base_border: Hsla, glass_opacity: f32, dark_mode: bool) -> Hsla {
        let sheer = (1.0 - glass_opacity.clamp(0.1, 1.0)).clamp(0.0, 0.9);
        let target_alpha = if dark_mode {
            (0.14 + sheer * 0.14).clamp(0.10, 0.34)
        } else {
            (0.16 + sheer * 0.12).clamp(0.12, 0.34)
        };
        Hsla {
            a: target_alpha,
            ..base_border
        }
    }

    /// Resolves `preference` into a concrete [`ThemeMode`] for the current `window`.
    pub fn resolve_mode(preference: ThemePreference, window: &Window) -> ThemeMode {
        preference.resolve(window.appearance())
    }

    /// Synchronizes native platform window chrome and backdrop materials (such as Windows 11
    /// Mica / `DWMWA_USE_IMMERSIVE_DARK_MODE`) with this theme's [`ThemePreference`].
    pub fn sync_window_appearance(&self, window: &Window) {
        let override_app = match self.preference {
            ThemePreference::System => None,
            ThemePreference::Light => Some(WindowAppearance::Light),
            ThemePreference::Dark => Some(WindowAppearance::Dark),
        };
        window.set_appearance_override(override_app);
    }

    /// Switches to `next_theme`, running a GPU-composited circular reveal originating from `origin`
    /// (or a cross-fade when `origin` is `None`) whenever the resolved [`ThemeMode`] changes.
    pub fn switch_with_transition(
        next_theme: Self,
        origin: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        Self::switch_with_transition_style(
            next_theme,
            ThemeTransitionStyle::RadialReveal,
            origin,
            window,
            cx,
        );
    }

    /// Switches to `next_theme` using an explicit GPU [`ThemeTransitionStyle`]
    /// (`Shockwave`, `PageCurl`, `LiquidDissolve`, `RadialReveal`, or `CrossFade`).
    pub fn switch_with_transition_style(
        next_theme: Self,
        style: ThemeTransitionStyle,
        origin: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let current_mode = Self::active(cx).mode;
        if current_mode == next_theme.mode {
            next_theme.sync_window_appearance(window);
            Self::set_global(next_theme, cx);
            cx.refresh_windows();
            return;
        }
        let mut transition = style.to_view_transition(origin, window);
        if let Some(old_backdrop) = current_mode.native_backdrop_color(window.background_appearance())
        {
            transition = transition.with_old_backdrop(old_backdrop);
        }
        window.start_view_transition(transition, cx, move |window, cx| {
            next_theme.sync_window_appearance(window);
            Self::set_global(next_theme, cx);
            cx.refresh_windows();
        });
    }

    /// Applies `preference` (`Light`, `System`, or `Dark`) with a GPU-composited radial transition
    /// expanding from `origin` (or a cross-fade when `origin` is `None`).
    pub fn set_preference_with_transition(
        preference: ThemePreference,
        origin: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        Self::set_preference_with_transition_style(
            preference,
            ThemeTransitionStyle::RadialReveal,
            origin,
            window,
            cx,
        );
    }

    /// Applies `preference` (`Light`, `System`, or `Dark`) with an explicit GPU
    /// [`ThemeTransitionStyle`].
    pub fn set_preference_with_transition_style(
        preference: ThemePreference,
        style: ThemeTransitionStyle,
        origin: Option<Point<Pixels>>,
        window: &mut Window,
        cx: &mut App,
    ) {
        let mode = Self::resolve_mode(preference, window);
        let next = match mode {
            ThemeMode::Light => Self::light(),
            ThemeMode::Dark => Self::dark(),
        }
        .with_preference(preference);
        Self::switch_with_transition_style(next, style, origin, window, cx);
    }

    /// Subscribes to OS appearance changes on `window` so [`ThemePreference::System`] updates live.
    pub fn observe_system_appearance(
        window: &mut Window,
        mut build_theme: impl FnMut(ThemeMode, ThemePreference, &App) -> Self + 'static,
    ) -> Subscription {
        window.observe_window_appearance(move |window, cx| {
            let current = Self::active(cx);
            if current.preference != ThemePreference::System {
                return;
            }
            let target_mode = ThemeMode::from(window.appearance());
            if target_mode != current.mode {
                let next = build_theme(target_mode, ThemePreference::System, cx);
                Self::switch_with_transition(next, None, window, cx);
            }
        })
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
    super::selectable_text::bind_selectable_text_keys(cx);
    super::markdown::bind_markdown_keys(cx);
}

/// Convenience wrapper for the standard non-invalid focus ring shadow.
pub(crate) fn focus_ring_shadow(theme: &Theme) -> Vec<crate::BoxShadow> {
    super::layout::focus_ring_shadow(theme, false)
}

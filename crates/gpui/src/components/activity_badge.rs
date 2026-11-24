//! Compact, GPU-animated iconic status badge ([`ActivityBadge`]) for tabs,
//! pane headers, lists, and status bars.
//!
//! Each [`ActivityState`] maps to a clear outline icon, semantic theme color,
//! and optional zero-relayout GPU animation (`Spin` for [`ActivityState::Pending`] and
//! [`ActivityState::Working`], layered fine-grained bell ring with damped shell,
//! inertial clapper, and radiating sound waves for [`ActivityState::Attention`]).
//! Animations are phase-locked to GPUI's shared scene clock so metadata updates never
//! restart a running spinner or ringing bell, and [`App::reduce_motion`] is respected
//! automatically.

use std::time::Duration;

use crate::icons::{Icon, IconAnimation, IconSize, LucideIcon};
use crate::{
    AnyElement, App, FontWeight, Hsla, IntoElement, Pixels, RenderOnce, SharedString,
    StyleRefinement, Styled, Svg, TimedTransformation, Transformation, Window, div, prelude::*, px,
    radians, svg,
};

use super::theme::{ActiveTheme, Theme};

const BELL_STATIC_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9Z"/><path d="M10.3 21a2 2 0 0 0 3.4 0"/><path d="M2 8c0-2 1-4 3-5m17 5c0-2-1-4-3-5"/></svg>"#;
const BELL_BODY_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M18 8a6 6 0 0 0-12 0c0 7-3 7-3 9h18c0-2-3-2-3-9Z"/></svg>"#;
const BELL_CLAPPER_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M10.3 21a2 2 0 0 0 3.4 0"/></svg>"#;
const BELL_WAVES_SVG: &str = r#"<svg xmlns="http://www.w3.org/2000/svg" width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round"><path d="M2 8c0-2 1-4 3-5m17 5c0-2-1-4-3-5"/></svg>"#;

fn bell_layer_svg(
    cache_key: &'static str,
    raw: &'static str,
    stroke_width: Option<f32>,
    px_size: Pixels,
    color: Hsla,
) -> Svg {
    let element = match stroke_width {
        Some(width) if (width - 2.0).abs() > f32::EPSILON => {
            let customized = raw.replace("stroke-width=\"2\"", &format!("stroke-width=\"{width}\""));
            svg().data(customized.as_bytes())
        }
        _ => svg().static_data(SharedString::new_static(cache_key), raw.as_bytes()),
    };
    element
        .flex_shrink_0()
        .w(px_size)
        .h(px_size)
        .text_color(color)
}

/// Semantic lifecycle / activity state displayed by [`ActivityBadge`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum ActivityState {
    /// Connected, healthy, or ready (`CircleCheck`, success tint, static).
    Ready,
    /// Connecting, loading, or pending (`LoaderCircle`, muted tint, clockwise GPU spin).
    Pending,
    /// Actively working or running (`LoaderCircle`, bright foreground tint, clockwise GPU spin).
    Working,
    /// Waiting for input, completed unread work, or alert (`BellRing`, warning tint, GPU bell ring).
    Attention,
    /// Idle or standby (`Clock`, muted tint, static).
    #[default]
    Idle,
    /// Error or failure (`CircleExclamationPoint`, destructive tint, static).
    Error,
    /// Disconnected, stopped, or closed (`CircleX`, muted tint, static).
    Disconnected,
    /// Unknown or unavailable status (`CircleQuestionMark`, muted tint, static).
    Unknown,
}

impl ActivityState {
    /// All [`ActivityState`] variants in a logical showcase order.
    pub const ALL: &'static [Self] = &[
        Self::Ready,
        Self::Pending,
        Self::Working,
        Self::Attention,
        Self::Idle,
        Self::Error,
        Self::Disconnected,
        Self::Unknown,
    ];

    /// Default [`LucideIcon`] representing this state.
    pub const fn icon(self) -> LucideIcon {
        match self {
            Self::Ready => LucideIcon::CircleCheck,
            Self::Pending | Self::Working => LucideIcon::LoaderCircle,
            Self::Attention => LucideIcon::BellRing,
            Self::Idle => LucideIcon::Clock,
            Self::Error => LucideIcon::CircleExclamationPoint,
            Self::Disconnected => LucideIcon::CircleX,
            Self::Unknown => LucideIcon::CircleQuestionMark,
        }
    }

    /// Human-readable default label for this state.
    pub const fn default_label(self) -> &'static str {
        match self {
            Self::Ready => "Ready",
            Self::Pending => "Connecting",
            Self::Working => "Working",
            Self::Attention => "Attention",
            Self::Idle => "Idle",
            Self::Error => "Error",
            Self::Disconnected => "Disconnected",
            Self::Unknown => "Unknown",
        }
    }

    /// Default GPU animation for this state, if any.
    pub fn default_animation(self) -> Option<IconAnimation> {
        match self {
            Self::Pending | Self::Working => Some(IconAnimation::Spin(Duration::from_secs(1))),
            Self::Attention => Some(IconAnimation::Ring {
                period: Duration::from_millis(2400),
                amplitude: radians(16.0_f32.to_radians()),
            }),
            Self::Ready | Self::Idle | Self::Error | Self::Disconnected | Self::Unknown => None,
        }
    }

    /// Default semantic foreground color resolved from `theme`.
    pub fn default_color(self, theme: &Theme) -> Hsla {
        match self {
            Self::Ready => theme.success,
            Self::Pending | Self::Idle | Self::Disconnected | Self::Unknown => {
                theme.muted_foreground
            }
            Self::Working => theme.foreground,
            Self::Attention => theme.warning,
            Self::Error => theme.destructive,
        }
    }
}

/// Compact, GPU-animated iconic status indicator for tabs, headers, and rows.
///
/// Renders as a fixed square icon box by default (ideal to the left of a tab or
/// pane title), and optionally displays a label and/or soft tinted pill chrome.
#[derive(IntoElement)]
pub struct ActivityBadge {
    state: ActivityState,
    icon_override: Option<LucideIcon>,
    size: IconSize,
    color: Option<Hsla>,
    stroke_width: Option<f32>,
    animated: bool,
    animation_override: Option<Option<IconAnimation>>,
    label: Option<SharedString>,
    pill: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`ActivityBadge::new`].
pub fn activity_badge(state: ActivityState) -> ActivityBadge {
    ActivityBadge::new(state)
}

impl ActivityBadge {
    /// Creates a new [`ActivityBadge`] for `state`.
    pub fn new(state: ActivityState) -> Self {
        Self {
            state,
            icon_override: None,
            size: IconSize::Md,
            color: None,
            stroke_width: None,
            animated: true,
            animation_override: None,
            label: None,
            pill: false,
            style: StyleRefinement::default(),
        }
    }

    /// Returns the configured [`ActivityState`].
    pub const fn state(&self) -> ActivityState {
        self.state
    }

    /// Overrides the icon size (default [`IconSize::Md`] = `16px`).
    pub fn size(mut self, size: impl Into<IconSize>) -> Self {
        self.size = size.into();
        self
    }

    /// Overrides the semantic icon color.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Overrides the underlying [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon_override = Some(icon);
        self
    }

    /// Overrides the SVG `stroke-width` (Lucide default is `2.0`).
    pub fn stroke_width(mut self, width: f32) -> Self {
        self.stroke_width = Some(width);
        self
    }

    /// Enables or disables GPU animations (default `true`; [`App::reduce_motion`] always disables motion).
    pub fn animated(mut self, animated: bool) -> Self {
        self.animated = animated;
        self
    }

    /// Overrides the animation applied to the icon (`None` forces static rendering).
    pub fn animation(mut self, animation: Option<IconAnimation>) -> Self {
        self.animation_override = Some(animation);
        self
    }

    /// Overrides the spin revolution period when this badge spins.
    pub fn spin_period(mut self, period: Duration) -> Self {
        self.animation_override = Some(Some(IconAnimation::Spin(period)));
        self
    }

    /// Overrides the bell-ring cycle period when this badge rings.
    pub fn ring_period(mut self, period: Duration) -> Self {
        self.animation_override = Some(Some(IconAnimation::Ring {
            period,
            amplitude: radians(16.0_f32.to_radians()),
        }));
        self
    }

    /// Displays an optional text label to the right of the status icon.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Displays the state's [`ActivityState::default_label`] beside the icon.
    pub fn with_default_label(mut self) -> Self {
        self.label = Some(SharedString::new_static(self.state.default_label()));
        self
    }

    /// Wraps the icon (and optional label) in a soft tinted pill surface.
    pub fn pill(mut self, pill: bool) -> Self {
        self.pill = pill;
        self
    }
}

impl Styled for ActivityBadge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ActivityBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let color = self
            .color
            .unwrap_or_else(|| self.state.default_color(&theme));
        let glyph = self.icon_override.unwrap_or_else(|| self.state.icon());
        let resolved_animation = if !self.animated {
            None
        } else {
            self.animation_override
                .unwrap_or_else(|| self.state.default_animation())
        };

        let px_size = self.size.pixels();
        let glyph_element: AnyElement = if self.state == ActivityState::Attention
            && self.icon_override.is_none()
        {
            match resolved_animation {
                Some(IconAnimation::Ring { period, amplitude }) if !cx.reduce_motion() => {
                    let body = bell_layer_svg(
                        "__gpui_activity_bell_body__",
                        BELL_BODY_SVG,
                        self.stroke_width,
                        px_size,
                        color,
                    )
                    .absolute()
                    .inset_0()
                    .with_timed_transformation(TimedTransformation::bell_ring(
                        period,
                        amplitude,
                        Transformation::default(),
                    ));
                    let clapper = bell_layer_svg(
                        "__gpui_activity_bell_clapper__",
                        BELL_CLAPPER_SVG,
                        self.stroke_width,
                        px_size,
                        color,
                    )
                    .absolute()
                    .inset_0()
                    .with_timed_transformation(TimedTransformation::bell_clapper(
                        period,
                        radians(amplitude.0 * 1.22),
                        Transformation::default(),
                    ));
                    let waves = bell_layer_svg(
                        "__gpui_activity_bell_waves__",
                        BELL_WAVES_SVG,
                        self.stroke_width,
                        px_size,
                        color,
                    )
                    .absolute()
                    .inset_0()
                    .with_timed_transformation(TimedTransformation::bell_wave(
                        period,
                        Transformation::default(),
                    ));
                    div()
                        .relative()
                        .flex_shrink_0()
                        .w(px_size)
                        .h(px_size)
                        .child(waves)
                        .child(clapper)
                        .child(body)
                        .into_any_element()
                }
                None | Some(IconAnimation::Ring { .. }) => bell_layer_svg(
                    "__gpui_activity_bell_static__",
                    BELL_STATIC_SVG,
                    self.stroke_width,
                    px_size,
                    color,
                )
                .into_any_element(),
                Some(other_animation) => {
                    let mut icon = Icon::new(glyph)
                        .size(self.size)
                        .color(color)
                        .animation(other_animation);
                    if let Some(width) = self.stroke_width {
                        icon = icon.stroke_width(width);
                    }
                    icon.into_any_element()
                }
            }
        } else {
            let mut icon = Icon::new(glyph).size(self.size).color(color);
            if let Some(width) = self.stroke_width {
                icon = icon.stroke_width(width);
            }
            if let Some(animation) = resolved_animation {
                icon = icon.animation(animation);
            }
            icon.into_any_element()
        };

        let mut el = if self.label.is_some() || self.pill {
            let mut row = div()
                .flex()
                .flex_row()
                .flex_shrink_0()
                .items_center()
                .gap(px(5.))
                .text_color(color)
                .text_size(px(12.))
                .font_weight(FontWeight::MEDIUM)
                .whitespace_nowrap()
                .child(glyph_element);
            if self.pill {
                row = row
                    .px(px(8.))
                    .py(px(2.))
                    .rounded_full()
                    .bg(color.opacity(0.14))
                    .border_1()
                    .border_color(color.opacity(0.28));
            }
            if let Some(label) = self.label {
                row = row.child(label);
            }
            row
        } else {
            div()
                .flex()
                .flex_shrink_0()
                .items_center()
                .justify_center()
                .w(px_size)
                .h(px_size)
                .child(glyph_element)
        };

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

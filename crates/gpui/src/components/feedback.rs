//! Feedback & loading components: [`ProgressBar`], [`ProgressRing`],
//! [`Spinner`], [`Skeleton`], and [`EmptyState`].

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use smallvec::SmallVec;

use crate::{
    AnyElement, App, BorderStyle, Bounds, Corners, Edges, ElementId, FontWeight, Hsla,
    IntoElement, PathBuilder, Pixels, Point, QuadAnimationCurve, QuadAnimationMode, RenderOnce,
    SharedString, Size, StyleRefinement, Styled, Window, canvas, div, point, prelude::*, px, quad,
    relative, transparent_black,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, ComponentSize, SPRING_SMOOTH, tabular_numerals};

struct ProgressAnimState {
    last_ratio: f32,
    from_ratio: f32,
    started_at: Option<Instant>,
}

const PROGRESS_ANIM_MAX_MS: u64 = 650;

/// Determinate progress bar with optional label, percentage readout, custom color,
/// and optional smooth spring width animation via [`ProgressBar::animated`].
#[derive(IntoElement)]
pub struct ProgressBar {
    id: Option<ElementId>,
    value: f32,
    max: f32,
    size: ComponentSize,
    color: Option<Hsla>,
    label: Option<SharedString>,
    show_percentage: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`ProgressBar::new`].
pub fn progress_bar(value: f32) -> ProgressBar {
    ProgressBar::new(value)
}

impl ProgressBar {
    /// Creates a progress bar where `value` is in `0.0..=100.0` by default.
    pub fn new(value: f32) -> Self {
        Self {
            id: None,
            value,
            max: 100.0,
            size: ComponentSize::Md,
            color: None,
            label: None,
            show_percentage: false,
            style: StyleRefinement::default(),
        }
    }

    /// Enables smooth spring interpolation between value changes keyed by `id`.
    pub fn animated(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the maximum value (default `100.0`).
    pub fn max(mut self, max: f32) -> Self {
        self.max = max.max(f32::EPSILON);
        self
    }

    /// Sets bar thickness (`Xs`..`Xl`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Overrides the fill bar color.
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Sets a left-aligned label above the progress track.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Shows the rounded percentage above the right side of the track.
    pub fn show_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }
}

impl Styled for ProgressBar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ProgressBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let ratio = (self.value / self.max).clamp(0.0, 1.0);
        let pct_int = (ratio * 100.0).round() as u32;
        let height = match self.size {
            ComponentSize::Xs => px(4.),
            ComponentSize::Sm => px(6.),
            ComponentSize::Md => px(8.),
            ComponentSize::Lg => px(12.),
            ComponentSize::Xl => px(16.),
        };
        let fill_color = self.color.unwrap_or(theme.primary);
        let reduce_motion = cx.reduce_motion();

        let fill_element = if let Some(anim_id) = self.id {
            let state_entity = window.use_keyed_state(
                (anim_id, "gpu-progress-anim"),
                cx,
                |_, _| {
                    Rc::new(RefCell::new(ProgressAnimState {
                        last_ratio: ratio,
                        from_ratio: ratio,
                        started_at: None,
                    }))
                },
            );
            let state_rc = state_entity.read(cx).clone();
            let now = cx.background_executor().now();
            let max_dur = Duration::from_millis(PROGRESS_ANIM_MAX_MS);
            let curve = QuadAnimationCurve::from(SPRING_SMOOTH);

            let (from_ratio, active_start) = {
                let mut st = state_rc.borrow_mut();
                if (st.last_ratio - ratio).abs() > 0.0005 {
                    let current = if let Some(started) = st.started_at {
                        let elapsed = now.saturating_duration_since(started).as_secs_f32();
                        let (pos, _) = curve.evaluate(elapsed, max_dur.as_secs_f32());
                        (st.from_ratio + (st.last_ratio - st.from_ratio) * pos).clamp(0.0, 1.0)
                    } else {
                        st.last_ratio
                    };
                    st.from_ratio = current;
                    st.last_ratio = ratio;
                    st.started_at = (!reduce_motion).then_some(now);
                }
                let active = st.started_at.filter(|started| {
                    !reduce_motion
                        && now.saturating_duration_since(*started) < max_dur
                        && (st.from_ratio - ratio).abs() > 0.0005
                });
                (st.from_ratio, active)
            };

            canvas(
                |_, _, _| {},
                move |track_bounds, _, window, _| {
                    let full_w = track_bounds.size.width;
                    let radius = track_bounds.size.height * 0.5;
                    let make_bounds = |r: f32| Bounds {
                        origin: track_bounds.origin,
                        size: Size::new(px(full_w.0 * r.clamp(0.0, 1.0)), track_bounds.size.height),
                    };
                    let to_bounds = make_bounds(ratio);
                    let fill_quad = quad(
                        to_bounds,
                        Corners::all(radius),
                        fill_color,
                        Edges::default(),
                        transparent_black(),
                        BorderStyle::Solid,
                    );
                    if let Some(started_at) = active_start {
                        window.paint_animated_quad(
                            fill_quad,
                            QuadAnimationMode::Oneshot {
                                started_at,
                                duration: max_dur,
                                curve,
                            },
                            make_bounds(from_ratio),
                            fill_color,
                            transparent_black(),
                        );
                    } else if to_bounds.size.width > px(0.) {
                        window.paint_quad(fill_quad);
                    }
                },
            )
            .size_full()
            .into_any_element()
        } else {
            div()
                .h_full()
                .w(relative(ratio))
                .rounded_full()
                .bg(fill_color)
                .into_any_element()
        };

        let mut root = div()
            .flex()
            .flex_col()
            .gap(px(6.))
            .w_full()
            .when(self.label.is_some() || self.show_percentage, |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .text_size(px(13.))
                        .child(
                            div()
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .children(self.label),
                        )
                        .when(self.show_percentage, |el| {
                            el.child(
                                div()
                                    .font_weight(FontWeight::MEDIUM)
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{pct_int}%")),
                            )
                        }),
                )
            })
            .child(
                div()
                    .w_full()
                    .h(height)
                    .rounded_full()
                    .bg(theme.surface_muted)
                    .overflow_hidden()
                    .child(fill_element),
            );

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Visual animation style for [`Spinner`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SpinnerVariant {
    /// Clean rotating ring arc (`LucideIcon::LoaderCircle`, default).
    #[default]
    Ring,
    /// Classic 8-spoke radial spinner (`LucideIcon::Loader`).
    Loader,
    /// Three staggered zero-relayout GPU bouncing/pulsing dots.
    Dots,
    /// Four rhythmic zero-relayout GPU equalizer bars.
    Bars,
    /// Smooth concentric zero-relayout GPU breathing pulse beacon.
    Pulse,
}

/// GPU-animated spinning/pulsing loading indicator with multiple visual [`SpinnerVariant`] styles.
///
/// - `Ring` and `Loader` use GPU-transformed SVG rotation.
/// - `Dots`, `Bars`, and `Pulse` use zero-relayout scene-level [`QuadAnimationMode::SyncedWave`]
///   animations so loading states stay silky smooth without triggering Taffy layout.
/// - Automatically respects [`App::reduce_motion`].
#[derive(IntoElement)]
pub struct Spinner {
    variant: SpinnerVariant,
    size: IconSize,
    color: Option<Hsla>,
    label: Option<SharedString>,
    description: Option<SharedString>,
    vertical: bool,
    speed: Option<Duration>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Spinner::new`].
pub fn spinner() -> Spinner {
    Spinner::new()
}

impl Default for Spinner {
    fn default() -> Self {
        Self::new()
    }
}

impl Spinner {
    /// Creates a medium spinning indicator (`SpinnerVariant::Ring`).
    pub fn new() -> Self {
        Self {
            variant: SpinnerVariant::Ring,
            size: IconSize::Md,
            color: None,
            label: None,
            description: None,
            vertical: false,
            speed: None,
            style: StyleRefinement::default(),
        }
    }

    /// Selects the visual animation style (`Ring`, `Loader`, `Dots`, `Bars`, `Pulse`).
    pub fn variant(mut self, variant: SpinnerVariant) -> Self {
        self.variant = variant;
        self
    }

    /// Sets spinner size.
    pub fn size(mut self, size: impl Into<IconSize>) -> Self {
        self.size = size.into();
        self
    }

    /// Sets spinner tint color (defaults to `theme.primary`).
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Adds a status label next to (or below, when [`Spinner::vertical`]) the spinner.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Adds a secondary muted description line under the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Stacks the indicator vertically above its label/description (centered).
    pub fn vertical(mut self) -> Self {
        self.vertical = true;
        self
    }

    /// Overrides the animation cycle period (e.g. `Duration::from_millis(650)` for faster spin).
    pub fn speed(mut self, period: Duration) -> Self {
        self.speed = Some(period);
        self
    }
}

impl Styled for Spinner {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let color = self.color.unwrap_or(theme.primary);
        let reduce_motion = cx.reduce_motion();
        let size_px = self.size.pixels().0.max(10.0);

        let indicator: AnyElement = match self.variant {
            SpinnerVariant::Ring | SpinnerVariant::Loader => {
                let icon_kind = if self.variant == SpinnerVariant::Ring {
                    LucideIcon::LoaderCircle
                } else {
                    LucideIcon::Loader
                };
                let mut ic = Icon::new(icon_kind).size(self.size).color(color);
                if !reduce_motion {
                    ic = match self.speed {
                        Some(period) => ic.spin_duration(period),
                        None => ic.spin(),
                    };
                }
                ic.into_any_element()
            }
            SpinnerVariant::Dots => {
                let dot_d = (size_px * 0.28).clamp(4.0, 12.0);
                let gap = (dot_d * 0.65).max(3.0);
                let total_w = dot_d * 3.0 + gap * 2.0;
                let total_h = (dot_d * 2.2).max(size_px * 0.85);
                let period = self.speed.unwrap_or(Duration::from_millis(950));
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let base_y = bounds.origin.y.0 + (bounds.size.height.0 - dot_d) * 0.5;
                        let lift = (bounds.size.height.0 - dot_d) * 0.42;
                        for i in 0..3 {
                            let x = bounds.origin.x.0 + (i as f32) * (dot_d + gap);
                            let low_bounds = Bounds {
                                origin: Point::new(px(x), px(base_y + lift * 0.35)),
                                size: Size::new(px(dot_d), px(dot_d)),
                            };
                            let high_bounds = Bounds {
                                origin: Point::new(px(x), px(base_y - lift * 0.65)),
                                size: Size::new(px(dot_d), px(dot_d)),
                            };
                            let dot_quad = quad(
                                high_bounds,
                                Corners::all(px(dot_d * 0.5)),
                                color,
                                Edges::default(),
                                transparent_black(),
                                BorderStyle::Solid,
                            );
                            if reduce_motion {
                                window.paint_quad(quad(
                                    Bounds {
                                        origin: Point::new(px(x), px(base_y)),
                                        size: Size::new(px(dot_d), px(dot_d)),
                                    },
                                    Corners::all(px(dot_d * 0.5)),
                                    color.opacity(0.6 + 0.15 * i as f32),
                                    Edges::default(),
                                    transparent_black(),
                                    BorderStyle::Solid,
                                ));
                            } else {
                                window.paint_animated_quad(
                                    dot_quad,
                                    QuadAnimationMode::SyncedWave {
                                        period,
                                        phase_offset: i as f32 * 0.18,
                                        min_alpha: 0.32,
                                        max_alpha: 1.0,
                                    },
                                    low_bounds,
                                    color,
                                    transparent_black(),
                                );
                            }
                        }
                    },
                )
                .flex_shrink_0()
                .w(px(total_w))
                .h(px(total_h))
                .into_any_element()
            }
            SpinnerVariant::Bars => {
                let bar_w = (size_px * 0.16).clamp(2.5, 6.0);
                let gap = (bar_w * 0.85).max(2.5);
                let total_w = bar_w * 4.0 + gap * 3.0;
                let total_h = size_px;
                let period = self.speed.unwrap_or(Duration::from_millis(900));
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let full_h = bounds.size.height.0;
                        let min_h = (full_h * 0.28).max(3.0);
                        let radius = px(bar_w * 0.5);
                        for i in 0..4 {
                            let x = bounds.origin.x.0 + (i as f32) * (bar_w + gap);
                            let from_bounds = Bounds {
                                origin: Point::new(
                                    px(x),
                                    px(bounds.origin.y.0 + (full_h - min_h) * 0.5),
                                ),
                                size: Size::new(px(bar_w), px(min_h)),
                            };
                            let to_bounds = Bounds {
                                origin: Point::new(px(x), bounds.origin.y),
                                size: Size::new(px(bar_w), px(full_h)),
                            };
                            if reduce_motion {
                                let h = full_h * (0.45 + 0.15 * ((i % 2) as f32));
                                window.paint_quad(quad(
                                    Bounds {
                                        origin: Point::new(
                                            px(x),
                                            px(bounds.origin.y.0 + (full_h - h) * 0.5),
                                        ),
                                        size: Size::new(px(bar_w), px(h)),
                                    },
                                    Corners::all(radius),
                                    color,
                                    Edges::default(),
                                    transparent_black(),
                                    BorderStyle::Solid,
                                ));
                            } else {
                                window.paint_animated_quad(
                                    quad(
                                        to_bounds,
                                        Corners::all(radius),
                                        color,
                                        Edges::default(),
                                        transparent_black(),
                                        BorderStyle::Solid,
                                    ),
                                    QuadAnimationMode::SyncedWave {
                                        period,
                                        phase_offset: i as f32 * 0.16,
                                        min_alpha: 0.35,
                                        max_alpha: 1.0,
                                    },
                                    from_bounds,
                                    color,
                                    transparent_black(),
                                );
                            }
                        }
                    },
                )
                .flex_shrink_0()
                .w(px(total_w))
                .h(px(total_h))
                .into_any_element()
            }
            SpinnerVariant::Pulse => {
                let period = self.speed.unwrap_or(Duration::from_millis(1300));
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let d = bounds.size.width.0.min(bounds.size.height.0);
                        let cx_pos = bounds.origin.x.0 + bounds.size.width.0 * 0.5;
                        let cy_pos = bounds.origin.y.0 + bounds.size.height.0 * 0.5;
                        let core_d = d * 0.36;
                        let core_bounds = Bounds {
                            origin: Point::new(px(cx_pos - core_d * 0.5), px(cy_pos - core_d * 0.5)),
                            size: Size::new(px(core_d), px(core_d)),
                        };
                        let ring_min = d * 0.38;
                        let ring_from = Bounds {
                            origin: Point::new(
                                px(cx_pos - ring_min * 0.5),
                                px(cy_pos - ring_min * 0.5),
                            ),
                            size: Size::new(px(ring_min), px(ring_min)),
                        };
                        let ring_to = Bounds {
                            origin: Point::new(px(cx_pos - d * 0.5), px(cy_pos - d * 0.5)),
                            size: Size::new(px(d), px(d)),
                        };
                        if !reduce_motion {
                            window.paint_animated_quad(
                                quad(
                                    ring_to,
                                    Corners::all(px(d * 0.5)),
                                    color.opacity(0.28),
                                    Edges::all(px(1.5)),
                                    color.opacity(0.55),
                                    BorderStyle::Solid,
                                ),
                                QuadAnimationMode::SyncedWave {
                                    period,
                                    phase_offset: 0.0,
                                    min_alpha: 0.95,
                                    max_alpha: 0.12,
                                },
                                ring_from,
                                color.opacity(0.32),
                                color.opacity(0.65),
                            );
                        } else {
                            window.paint_quad(quad(
                                ring_to,
                                Corners::all(px(d * 0.5)),
                                color.opacity(0.18),
                                Edges::all(px(1.5)),
                                color.opacity(0.45),
                                BorderStyle::Solid,
                            ));
                        }
                        window.paint_quad(quad(
                            core_bounds,
                            Corners::all(px(core_d * 0.5)),
                            color,
                            Edges::default(),
                            transparent_black(),
                            BorderStyle::Solid,
                        ));
                    },
                )
                .flex_shrink_0()
                .w(px(size_px))
                .h(px(size_px))
                .into_any_element()
            }
        };

        let has_text = self.label.is_some() || self.description.is_some();
        let text_block = has_text.then(|| {
            div()
                .flex()
                .flex_col()
                .when(self.vertical, |el| el.items_center())
                .gap(px(2.))
                .when_some(self.label, |el, label| {
                    el.child(
                        div()
                            .text_size(px(13.5))
                            .font_weight(if self.description.is_some() || self.vertical {
                                FontWeight::MEDIUM
                            } else {
                                FontWeight::NORMAL
                            })
                            .text_color(if self.description.is_some() {
                                theme.foreground
                            } else {
                                theme.muted_foreground
                            })
                            .when(self.vertical, |el| el.text_center())
                            .child(label),
                    )
                })
                .when_some(self.description, |el, desc| {
                    el.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .when(self.vertical, |el| el.text_center())
                            .child(desc),
                    )
                })
        });

        let mut el = div()
            .flex()
            .when(self.vertical, |el| {
                el.flex_col().items_center().justify_center().gap(px(10.))
            })
            .when(!self.vertical, |el| el.flex_row().items_center().gap(px(8.)))
            .child(indicator)
            .children(text_block);

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Circular progress ring / gauge built on GPUI vector paths ([`PathBuilder`]).
///
/// Supports determinate percentages with optional center readout, custom label,
/// configurable diameter & stroke thickness, and optional smooth spring animation via
/// [`ProgressRing::animated`].
#[derive(IntoElement)]
pub struct ProgressRing {
    id: Option<ElementId>,
    value: f32,
    max: f32,
    diameter: Pixels,
    thickness: Pixels,
    color: Option<Hsla>,
    track_color: Option<Hsla>,
    show_percentage: bool,
    center_label: Option<SharedString>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ProgressRing::new`].
pub fn progress_ring(value: f32) -> ProgressRing {
    ProgressRing::new(value)
}

impl ProgressRing {
    /// Creates a determinate circular progress ring where `value` is in `0.0..=100.0` by default.
    pub fn new(value: f32) -> Self {
        Self {
            id: None,
            value,
            max: 100.0,
            diameter: px(56.),
            thickness: px(5.),
            color: None,
            track_color: None,
            show_percentage: true,
            center_label: None,
            style: StyleRefinement::default(),
        }
    }

    /// Enables smooth spring interpolation between value changes keyed by `id`.
    pub fn animated(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the maximum value (default `100.0`).
    pub fn max(mut self, max: f32) -> Self {
        self.max = max.max(f32::EPSILON);
        self
    }

    /// Sets ring diameter & proportional stroke thickness using [`ComponentSize`].
    pub fn size(mut self, size: ComponentSize) -> Self {
        let (d, t) = match size {
            ComponentSize::Xs => (px(28.), px(3.)),
            ComponentSize::Sm => (px(40.), px(4.)),
            ComponentSize::Md => (px(56.), px(5.)),
            ComponentSize::Lg => (px(76.), px(6.5)),
            ComponentSize::Xl => (px(96.), px(8.)),
        };
        self.diameter = d;
        self.thickness = t;
        self
    }

    /// Sets an explicit outer diameter in logical pixels.
    pub fn diameter(mut self, diameter: Pixels) -> Self {
        self.diameter = diameter;
        self
    }

    /// Sets the stroke thickness in logical pixels.
    pub fn thickness(mut self, thickness: Pixels) -> Self {
        self.thickness = thickness;
        self
    }

    /// Overrides the progress arc stroke color (defaults to `theme.primary`).
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Overrides the background ring track color (defaults to `theme.surface_muted`).
    pub fn track_color(mut self, color: impl Into<Hsla>) -> Self {
        self.track_color = Some(color.into());
        self
    }

    /// Shows or hides the centered percentage text (default `true`).
    pub fn show_percentage(mut self, show: bool) -> Self {
        self.show_percentage = show;
        self
    }

    /// Sets a custom centered label (overrides the numeric percentage when set).
    pub fn center_label(mut self, label: impl Into<SharedString>) -> Self {
        self.center_label = Some(label.into());
        self
    }
}

impl Styled for ProgressRing {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

fn paint_ring_arc(
    window: &mut Window,
    center: Point<Pixels>,
    radius: f32,
    thickness: f32,
    start_angle: f32,
    sweep_angle: f32,
    color: Hsla,
) {
    if radius <= 1.0 || sweep_angle.abs() < 0.002 {
        return;
    }
    let segments = ((sweep_angle.abs() / std::f32::consts::TAU) * 64.0)
        .ceil()
        .clamp(4.0, 72.0) as usize;
    let mut pb = PathBuilder::stroke(px(thickness));
    for i in 0..=segments {
        let frac = i as f32 / segments as f32;
        let angle = start_angle + sweep_angle * frac;
        let x = center.x.0 + radius * angle.cos();
        let y = center.y.0 + radius * angle.sin();
        if i == 0 {
            pb.move_to(point(px(x), px(y)));
        } else {
            pb.line_to(point(px(x), px(y)));
        }
    }
    if let Ok(path) = pb.build() {
        window.paint_path(path, color);
    }

    // Rounded circular end-caps when arc is partial (< full loop)
    if sweep_angle.abs() < std::f32::consts::TAU - 0.02 {
        let cap_r = thickness * 0.5;
        for &angle in &[start_angle, start_angle + sweep_angle] {
            let cx = center.x.0 + radius * angle.cos();
            let cy = center.y.0 + radius * angle.sin();
            window.paint_quad(quad(
                Bounds {
                    origin: point(px(cx - cap_r), px(cy - cap_r)),
                    size: Size::new(px(thickness), px(thickness)),
                },
                Corners::all(px(cap_r)),
                color,
                Edges::default(),
                transparent_black(),
                BorderStyle::Solid,
            ));
        }
    }
}

impl RenderOnce for ProgressRing {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let target_ratio = (self.value / self.max).clamp(0.0, 1.0);
        let reduce_motion = cx.reduce_motion();

        let display_ratio = if let Some(anim_id) = self.id {
            let state_entity = window.use_keyed_state(
                (anim_id, "progress-ring-anim"),
                cx,
                |_, _| {
                    Rc::new(RefCell::new(ProgressAnimState {
                        last_ratio: target_ratio,
                        from_ratio: target_ratio,
                        started_at: None,
                    }))
                },
            );
            let state_rc = state_entity.read(cx).clone();
            let now = cx.background_executor().now();
            let max_dur = Duration::from_millis(PROGRESS_ANIM_MAX_MS);
            let curve = QuadAnimationCurve::from(SPRING_SMOOTH);
            let mut st = state_rc.borrow_mut();
            if (st.last_ratio - target_ratio).abs() > 0.0005 {
                let current = if let Some(started) = st.started_at {
                    let elapsed = now.saturating_duration_since(started).as_secs_f32();
                    let (pos, _) = curve.evaluate(elapsed, max_dur.as_secs_f32());
                    (st.from_ratio + (st.last_ratio - st.from_ratio) * pos).clamp(0.0, 1.0)
                } else {
                    st.last_ratio
                };
                st.from_ratio = current;
                st.last_ratio = target_ratio;
                st.started_at = (!reduce_motion).then_some(now);
            }
            if let Some(started) = st.started_at
                && !reduce_motion
            {
                let elapsed = now.saturating_duration_since(started).as_secs_f32();
                let (pos, settled) = curve.evaluate(elapsed, max_dur.as_secs_f32());
                if settled || elapsed >= max_dur.as_secs_f32() {
                    st.started_at = None;
                    st.from_ratio = target_ratio;
                    target_ratio
                } else {
                    window.request_animation_frame();
                    (st.from_ratio + (target_ratio - st.from_ratio) * pos).clamp(0.0, 1.0)
                }
            } else {
                target_ratio
            }
        } else {
            target_ratio
        };

        let fill_color = self.color.unwrap_or(theme.primary);
        let track_color = self.track_color.unwrap_or(theme.surface_muted);
        let thickness = self.thickness.0.max(2.0);
        let pct_int = (display_ratio * 100.0).round() as u32;
        let center_text: Option<SharedString> = self
            .center_label
            .or_else(|| self.show_percentage.then(|| format!("{pct_int}%").into()));
        let font_px = (self.diameter.0 * 0.22).clamp(10.0, 22.0);

        let mut root = div()
            .relative()
            .flex()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .w(self.diameter)
            .h(self.diameter)
            .child(
                canvas(
                    |_, _, _| {},
                    move |bounds, _, window, _| {
                        let cx_pos = bounds.origin.x.0 + bounds.size.width.0 * 0.5;
                        let cy_pos = bounds.origin.y.0 + bounds.size.height.0 * 0.5;
                        let min_dim = bounds.size.width.0.min(bounds.size.height.0);
                        let radius = ((min_dim - thickness) * 0.5).max(1.0);
                        let center = point(px(cx_pos), px(cy_pos));
                        // Full background track
                        paint_ring_arc(
                            window,
                            center,
                            radius,
                            thickness,
                            -std::f32::consts::FRAC_PI_2,
                            std::f32::consts::TAU,
                            track_color,
                        );
                        // Foreground determinate arc starting at 12 o'clock
                        if display_ratio > 0.001 {
                            paint_ring_arc(
                                window,
                                center,
                                radius,
                                thickness,
                                -std::f32::consts::FRAC_PI_2,
                                std::f32::consts::TAU * display_ratio,
                                fill_color,
                            );
                        }
                    },
                )
                .absolute()
                .inset_0()
                .size_full(),
            )
            .when_some(center_text, |el, txt| {
                el.child(
                    div()
                        .text_size(px(font_px))
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_features(tabular_numerals())
                        .text_color(theme.foreground)
                        .child(txt),
                )
            });

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Placeholder shimmer-style loading block (rectangle, circle, or text line).
///
/// By default, animates smoothly at display refresh rate using a zero-relayout GPU scene
/// [`QuadAnimationMode::SyncedPulse`] (skipping view `render()` and Taffy layout on every frame)
/// and automatically respects [`App::reduce_motion`].
#[derive(IntoElement)]
pub struct Skeleton {
    rounded_full: bool,
    shimmer: bool,
    style: StyleRefinement,
}

/// Creates a rectangular [`Skeleton`] block.
pub fn skeleton() -> Skeleton {
    Skeleton::new()
}

impl Default for Skeleton {
    fn default() -> Self {
        Self::new()
    }
}

impl Skeleton {
    /// Creates a rounded placeholder skeleton block.
    pub fn new() -> Self {
        Self {
            rounded_full: false,
            shimmer: true,
            style: StyleRefinement::default(),
        }
    }

    /// Creates a circular skeleton placeholder (e.g. for an avatar).
    pub fn circle(diameter: Pixels) -> Self {
        let mut s = Self {
            rounded_full: true,
            shimmer: true,
            style: StyleRefinement::default(),
        };
        s.style.size.width = Some(diameter.into());
        s.style.size.height = Some(diameter.into());
        s
    }

    /// Creates a single text line skeleton placeholder.
    pub fn line(height: Pixels) -> Self {
        let mut s = Self::new();
        s.style.size.width = Some(relative(1.).into());
        s.style.size.height = Some(height.into());
        s
    }

    /// Enables or disables the zero-relayout GPU loading pulse animation (default `true`).
    pub fn shimmer(mut self, enabled: bool) -> Self {
        self.shimmer = enabled;
        self
    }
}

impl Styled for Skeleton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Skeleton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let base_color = theme.surface_muted;
        let rounded_full = self.rounded_full;
        let default_radius = theme.radius_md;

        if self.shimmer && !cx.reduce_motion() {
            let mut el = div()
                .h(px(16.))
                .w_full()
                .when(rounded_full, |el| el.rounded_full())
                .when(!rounded_full, |el| el.rounded(default_radius))
                .overflow_hidden()
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let radius = if rounded_full {
                                px(bounds.size.width.0.min(bounds.size.height.0) * 0.5)
                            } else {
                                default_radius
                            };
                            let fill_quad = quad(
                                bounds,
                                Corners::all(radius),
                                base_color,
                                Edges::default(),
                                transparent_black(),
                                BorderStyle::Solid,
                            );
                            window.paint_animated_quad(
                                fill_quad,
                                QuadAnimationMode::SyncedPulse {
                                    period: Duration::from_millis(1600),
                                    min_alpha: 0.42,
                                    max_alpha: 1.0,
                                },
                                bounds,
                                base_color,
                                transparent_black(),
                            );
                        },
                    )
                    .size_full(),
                );
            crate::Refineable::refine(el.style(), &self.style);
            el
        } else {
            let mut el = div()
                .h(px(16.))
                .w_full()
                .bg(base_color)
                .when(rounded_full, |el| el.rounded_full())
                .when(!rounded_full, |el| el.rounded(default_radius));
            crate::Refineable::refine(el.style(), &self.style);
            el
        }
    }
}

/// Empty State panel with dashed border, centered Lucide icon, title, description,
/// and primary call-to-action slot.
#[derive(IntoElement)]
pub struct EmptyState {
    icon: LucideIcon,
    title: SharedString,
    description: Option<SharedString>,
    dashed: bool,
    actions: SmallVec<[AnyElement; 2]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`EmptyState::new`].
pub fn empty_state(icon: LucideIcon, title: impl Into<SharedString>) -> EmptyState {
    EmptyState::new(icon, title)
}

impl EmptyState {
    /// Creates a new [`EmptyState`].
    pub fn new(icon: LucideIcon, title: impl Into<SharedString>) -> Self {
        Self {
            icon,
            title: title.into(),
            description: None,
            dashed: true,
            actions: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Sets descriptive text below the heading.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Uses a solid border instead of dashed.
    pub fn solid_border(mut self) -> Self {
        self.dashed = false;
        self
    }

    /// Appends a call-to-action button.
    pub fn action(mut self, action: impl IntoElement) -> Self {
        self.actions.push(action.into_any_element());
        self
    }
}

impl Styled for EmptyState {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for EmptyState {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_col()
            .flex_shrink_0()
            .items_center()
            .justify_center()
            .gap(px(10.))
            .w_full()
            .px(px(24.))
            .py(px(36.))
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.input_border)
            .when(self.dashed, |el| el.border_dashed())
            .bg(theme.surface.opacity(0.6))
            .child(
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(44.))
                    .h(px(44.))
                    .rounded_full()
                    .bg(theme.surface_muted)
                    .child(
                        Icon::new(self.icon)
                            .size(IconSize::Xl)
                            .color(theme.muted_foreground),
                    ),
            )
            .child(
                div()
                    .text_size(px(15.))
                    .font_weight(FontWeight::SEMIBOLD)
                    .text_color(theme.foreground)
                    .text_center()
                    .child(self.title),
            )
            .when_some(self.description, |el, desc| {
                el.child(
                    div()
                        .max_w(px(380.))
                        .text_size(px(13.))
                        .text_color(theme.muted_foreground)
                        .text_center()
                        .child(desc),
                )
            })
            .when(!self.actions.is_empty(), |el| {
                el.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .gap(px(8.))
                        .pt(px(6.))
                        .children(self.actions),
                )
            });

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

//! Tailwind-inspired feedback & loading components: [`ProgressBar`], [`Spinner`],
//! [`Skeleton`], and [`EmptyState`].

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use smallvec::SmallVec;

use crate::{
    AnyElement, App, BorderStyle, Bounds, Corners, Edges, ElementId, FontWeight, Hsla,
    IntoElement, Pixels, QuadAnimationCurve, QuadAnimationMode, RenderOnce, SharedString, Size,
    StyleRefinement, Styled, Window, canvas, div, prelude::*, px, quad, relative,
    transparent_black,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, ComponentSize, SPRING_SMOOTH};

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

/// GPU-animated spinning loading indicator powered by [`LucideIcon::LoaderCircle`].
#[derive(IntoElement)]
pub struct Spinner {
    size: IconSize,
    color: Option<Hsla>,
    label: Option<SharedString>,
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
    /// Creates a medium spinning indicator.
    pub fn new() -> Self {
        Self {
            size: IconSize::Md,
            color: None,
            label: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets spinner icon size.
    pub fn size(mut self, size: impl Into<IconSize>) -> Self {
        self.size = size.into();
        self
    }

    /// Sets spinner tint color (defaults to `theme.primary`).
    pub fn color(mut self, color: impl Into<Hsla>) -> Self {
        self.color = Some(color.into());
        self
    }

    /// Adds an inline status label next to the spinner (e.g. `"Loading..."`).
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
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
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .child(
                Icon::new(LucideIcon::LoaderCircle)
                    .size(self.size)
                    .color(color)
                    .spin(),
            )
            .when_some(self.label, |el, label| {
                el.child(
                    div()
                        .text_size(px(13.5))
                        .text_color(theme.muted_foreground)
                        .child(label),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
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

/// Tailwind UI Empty State panel with dashed border, centered Lucide icon, title, description,
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

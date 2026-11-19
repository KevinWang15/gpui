//! Tailwind-inspired feedback & loading components: [`ProgressBar`], [`Spinner`],
//! [`Skeleton`], and [`EmptyState`].

use smallvec::SmallVec;

use crate::{
    AnimationExt, AnyElement, App, ElementId, FontWeight, Hsla, IntoElement, Pixels, RenderOnce,
    SharedString, SpringAnimation, StyleRefinement, Styled, Window, div, prelude::*, px, relative,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, ComponentSize, SPRING_SMOOTH};

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
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
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
                    .child(if let Some(anim_id) = self.id {
                        div()
                            .h_full()
                            .rounded_full()
                            .bg(fill_color)
                            .with_spring(
                                anim_id,
                                SpringAnimation::new(SPRING_SMOOTH).to(ratio),
                                |el, val| el.w(relative(val.clamp(0.0, 1.0))),
                            )
                            .into_any_element()
                    } else {
                        div()
                            .h_full()
                            .w(relative(ratio))
                            .rounded_full()
                            .bg(fill_color)
                            .into_any_element()
                    }),
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

impl RenderOnce for Spinner {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let color = self.color.unwrap_or(theme.primary);
        div()
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
            })
    }
}

/// Placeholder shimmer-style loading block (rectangle, circle, or text line).
#[derive(IntoElement)]
pub struct Skeleton {
    rounded_full: bool,
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
            style: StyleRefinement::default(),
        }
    }

    /// Creates a circular skeleton placeholder (e.g. for an avatar).
    pub fn circle(diameter: Pixels) -> Self {
        let mut s = Self {
            rounded_full: true,
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
}

impl Styled for Skeleton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Skeleton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .h(px(16.))
            .w_full()
            .bg(theme.surface_muted)
            .when(self.rounded_full, |el| el.rounded_full())
            .when(!self.rounded_full, |el| el.rounded(theme.radius_md));
        crate::Refineable::refine(el.style(), &self.style);
        el
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

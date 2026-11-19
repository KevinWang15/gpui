//! Interactive multi-step wizard & workflow [`Stepper`] (`Horizontal` or `Vertical`)
//! with completed checkmarks, active highlight, error status, custom icons, connector lines,
//! and optional clickable step navigation.

use std::rc::Rc;

use crate::{
    App, ElementId, FontWeight, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, tabular_numerals};

/// Visual status for an individual [`StepItem`].
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum StepStatus {
    /// Step already completed (renders a solid checkmark badge).
    Completed,
    /// Step currently in progress (highlighted border & primary accent).
    Active,
    /// Future step not yet reached.
    Pending,
    /// Step encountered a validation or execution error.
    Error,
}

/// Layout axis for [`Stepper`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StepperOrientation {
    /// Steps laid out left-to-right with horizontal connectors (default).
    #[default]
    Horizontal,
    /// Steps stacked top-to-bottom with vertical connectors.
    Vertical,
}

/// Single step descriptor inside a [`Stepper`].
#[derive(Clone, Debug)]
pub struct StepItem {
    /// Primary step title.
    pub title: SharedString,
    /// Optional muted helper description.
    pub description: Option<SharedString>,
    /// Optional custom [`LucideIcon`] shown in place of the step number when pending/active.
    pub icon: Option<LucideIcon>,
    /// Optional explicit status override (defaults to deriving from `active_step`).
    pub status: Option<StepStatus>,
}

impl StepItem {
    /// Creates a new step with `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            icon: None,
            status: None,
        }
    }

    /// Sets a muted description below the step title.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets a custom [`LucideIcon`] for the step circle.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Explicitly overrides this step's [`StepStatus`] (e.g. [`StepStatus::Error`]).
    pub fn status(mut self, status: StepStatus) -> Self {
        self.status = Some(status);
        self
    }
}

/// Multi-step progress and clickable wizard indicator.
#[derive(IntoElement)]
pub struct Stepper {
    id: ElementId,
    steps: Vec<StepItem>,
    active_step: usize,
    orientation: StepperOrientation,
    clickable: bool,
    on_step_click: Option<Rc<dyn Fn(&usize, &mut Window, &mut App) + 'static>>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Stepper::new`].
pub fn stepper(id: impl Into<ElementId>, steps: Vec<StepItem>, active_step: usize) -> Stepper {
    Stepper::new(id, steps, active_step)
}

impl Stepper {
    /// Creates a horizontal [`Stepper`] where `active_step` is a 0-based index.
    ///
    /// Steps `< active_step` default to [`StepStatus::Completed`], `== active_step` to
    /// [`StepStatus::Active`], and `> active_step` to [`StepStatus::Pending`].
    pub fn new(id: impl Into<ElementId>, steps: Vec<StepItem>, active_step: usize) -> Self {
        Self {
            id: id.into(),
            steps,
            active_step,
            orientation: StepperOrientation::Horizontal,
            clickable: false,
            on_step_click: None,
            style: StyleRefinement::default(),
        }
    }

    /// Switches between [`StepperOrientation::Horizontal`] and [`StepperOrientation::Vertical`].
    pub fn orientation(mut self, orientation: StepperOrientation) -> Self {
        self.orientation = orientation;
        self
    }

    /// Convenience shortcut for vertical stepper layout.
    pub fn vertical(mut self) -> Self {
        self.orientation = StepperOrientation::Vertical;
        self
    }

    /// Enables clicking any step to navigate directly.
    pub fn clickable(mut self, clickable: bool) -> Self {
        self.clickable = clickable;
        self
    }

    /// Registers a click callback receiving the clicked 0-based step index.
    /// Automatically enables clickability.
    pub fn on_step_click(
        mut self,
        handler: impl Fn(&usize, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.clickable = true;
        self.on_step_click = Some(Rc::new(handler));
        self
    }
}

impl Styled for Stepper {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Stepper {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let count = self.steps.len();
        let active_step = self.active_step;

        let resolve_status = |idx: usize, step: &StepItem| -> StepStatus {
            step.status.unwrap_or_else(|| {
                if idx < active_step {
                    StepStatus::Completed
                } else if idx == active_step {
                    StepStatus::Active
                } else {
                    StepStatus::Pending
                }
            })
        };

        let mut root = div()
            .id(self.id.clone())
            .flex()
            .w_full()
            .when(self.orientation == StepperOrientation::Horizontal, |el| {
                el.flex_row().items_start()
            })
            .when(self.orientation == StepperOrientation::Vertical, |el| {
                el.flex_col()
            });

        for (idx, step) in self.steps.into_iter().enumerate() {
            let status = resolve_status(idx, &step);
            let is_last = idx + 1 == count;
            let on_click = self.on_step_click.clone();
            let clickable = self.clickable && on_click.is_some();

            let (circle_bg, circle_border, circle_fg) = match status {
                StepStatus::Completed => (theme.primary, theme.primary, theme.primary_foreground),
                StepStatus::Active => (theme.primary_soft, theme.primary, theme.primary),
                StepStatus::Pending => (theme.surface, theme.input_border, theme.muted_foreground),
                StepStatus::Error => (
                    theme.destructive_soft,
                    theme.destructive,
                    theme.destructive,
                ),
            };

            let title_color = match status {
                StepStatus::Pending => theme.muted_foreground,
                StepStatus::Error => theme.destructive,
                StepStatus::Completed | StepStatus::Active => theme.foreground,
            };

            let connector_color = if status == StepStatus::Completed {
                theme.primary
            } else {
                theme.border
            };

            let circle_content = match status {
                StepStatus::Completed => Icon::new(LucideIcon::Check)
                    .size(IconSize::Sm)
                    .color(circle_fg)
                    .into_any_element(),
                StepStatus::Error => Icon::new(LucideIcon::X)
                    .size(IconSize::Sm)
                    .color(circle_fg)
                    .into_any_element(),
                StepStatus::Active | StepStatus::Pending => {
                    if let Some(ic) = step.icon {
                        Icon::new(ic)
                            .size(IconSize::Sm)
                            .color(circle_fg)
                            .into_any_element()
                    } else {
                        div()
                            .text_size(px(12.5))
                            .font_weight(FontWeight::SEMIBOLD)
                            .font_features(tabular_numerals())
                            .text_color(circle_fg)
                            .child((idx + 1).to_string())
                            .into_any_element()
                    }
                }
            };

            let circle = div()
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .w(px(30.))
                .h(px(30.))
                .rounded_full()
                .border_2()
                .border_color(circle_border)
                .bg(circle_bg)
                .child(circle_content);

            let label_block = div()
                .flex()
                .flex_col()
                .gap(px(2.))
                .child(
                    div()
                        .text_size(px(13.))
                        .font_weight(if status == StepStatus::Active {
                            FontWeight::SEMIBOLD
                        } else {
                            FontWeight::MEDIUM
                        })
                        .text_color(title_color)
                        .child(step.title),
                )
                .when_some(step.description, |el, desc| {
                    el.child(
                        div()
                            .text_size(px(12.))
                            .text_color(theme.muted_foreground)
                            .child(desc),
                    )
                });

            if self.orientation == StepperOrientation::Horizontal {
                let step_btn = div()
                    .id((self.id.clone(), format!("step-{idx}")))
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(10.))
                    .px(px(6.))
                    .py(px(4.))
                    .rounded(theme.radius_md)
                    .when(clickable, |el| {
                        let hover_bg = theme.surface_hover;
                        el.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .when_some(on_click, |el, cb| {
                                el.on_click(move |_, window, cx| cb(&idx, window, cx))
                            })
                    })
                    .child(circle)
                    .child(label_block);

                root = root.child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .when(!is_last, |el| el.flex_1())
                        .child(step_btn)
                        .when(!is_last, |el| {
                            el.child(
                                div()
                                    .flex_1()
                                    .min_w(px(20.))
                                    .h(px(2.))
                                    .mx(px(8.))
                                    .rounded_full()
                                    .bg(connector_color),
                            )
                        }),
                );
            } else {
                let row = div()
                    .id((self.id.clone(), format!("step-v-{idx}")))
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(12.))
                    .when(clickable, |el| {
                        el.cursor_pointer().when_some(on_click, |el, cb| {
                            el.on_click(move |_, window, cx| cb(&idx, window, cx))
                        })
                    })
                    .child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .child(circle)
                            .when(!is_last, |el| {
                                el.child(
                                    div()
                                        .w(px(2.))
                                        .h(px(28.))
                                        .my(px(4.))
                                        .rounded_full()
                                        .bg(connector_color),
                                )
                            }),
                    )
                    .child(
                        div()
                            .pt(px(4.))
                            .pb(if is_last { px(0.) } else { px(12.) })
                            .child(label_block),
                    );
                root = root.child(row);
            }
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

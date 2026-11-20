//! Guided interactive onboarding [`Tour`] with a 4-quad spotlight cutout highlighting live UI
//! elements registered via [`tour_target`], paired with a floating step card (`Back`, `Next`,
//! `Skip`, clickable progress dots, and `Escape`/`Arrow`/`Enter` keyboard navigation).

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, AnyElement, App, Bounds, Context, Entity, EventEmitter,
    FocusHandle, Focusable, FontWeight, IntoElement, KeyDownEvent, MouseButton, Pixels, Render,
    RenderOnce, SharedString, StyleRefinement, Styled, Window, anchored, canvas, deferred, div,
    ease_out_quint, point, prelude::*, px, size,
};

use super::button::button;
use super::palette;
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize, tabular_numerals};

/// Descriptor for a single step in an interactive [`Tour`].
#[derive(Clone, Debug)]
pub struct TourStep {
    /// Key matching a [`tour_target`] wrapper (`None` centers the step card without a cutout).
    pub target_key: Option<SharedString>,
    /// Step heading.
    pub title: SharedString,
    /// Explanatory body copy.
    pub description: SharedString,
    /// Optional leading [`LucideIcon`].
    pub icon: Option<LucideIcon>,
}

impl TourStep {
    /// Creates a centered tour step with no specific spotlight target.
    pub fn centered(title: impl Into<SharedString>, description: impl Into<SharedString>) -> Self {
        Self {
            target_key: None,
            title: title.into(),
            description: description.into(),
            icon: None,
        }
    }

    /// Creates a tour step spotlighting the live element wrapped by `tour_target(..., key, el)`.
    pub fn target(
        key: impl Into<SharedString>,
        title: impl Into<SharedString>,
        description: impl Into<SharedString>,
    ) -> Self {
        Self {
            target_key: Some(key.into()),
            title: title.into(),
            description: description.into(),
            icon: None,
        }
    }

    /// Sets an icon shown beside the step heading.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }
}

/// Events emitted by [`TourState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TourEvent {
    /// Emitted when advancing or stepping to `step_index`.
    StepChange(usize),
    /// Emitted when the user completes the final step.
    Finish,
    /// Emitted when the user dismisses/skips the tour early.
    Close,
}

/// Shared entity state managing tour steps, active step index, and live frame-stamped target bounds.
pub struct TourState {
    focus_handle: FocusHandle,
    previous_focus: Option<FocusHandle>,
    steps: Vec<TourStep>,
    active_step: Option<usize>,
    frame_gen: Rc<Cell<u64>>,
    targets: Rc<RefCell<HashMap<SharedString, (u64, Bounds<Pixels>)>>>,
    spotlight_padding: Pixels,
    backdrop_blur: Pixels,
    frame_style: StyleRefinement,
}

impl EventEmitter<TourEvent> for TourState {}

impl Focusable for TourState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl TourState {
    /// Creates a closed [`TourState`] with the given `steps`.
    pub fn new(steps: Vec<TourStep>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            steps,
            active_step: None,
            frame_gen: Rc::new(Cell::new(1)),
            targets: Rc::new(RefCell::new(HashMap::new())),
            spotlight_padding: px(6.),
            backdrop_blur: px(4.),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets breathing padding around the spotlight cutout (default `6px`).
    pub fn spotlight_padding(mut self, pad: Pixels) -> Self {
        self.spotlight_padding = if pad.0.is_finite() {
            pad.max(px(0.))
        } else {
            px(6.)
        };
        self
    }

    /// Sets the GPU backdrop blur radius for centered steps (default `4px`).
    pub fn backdrop_blur(mut self, radius: Pixels) -> Self {
        self.backdrop_blur = if radius.0.is_finite() {
            radius.max(px(0.))
        } else {
            px(4.)
        };
        self
    }

    /// Replaces the tour steps dynamically, clamping or closing the active step if needed.
    pub fn set_steps(&mut self, steps: Vec<TourStep>, cx: &mut Context<Self>) {
        self.steps = steps;
        if let Some(idx) = self.active_step {
            if self.steps.is_empty() {
                self.active_step = None;
            } else if idx >= self.steps.len() {
                self.active_step = Some(self.steps.len() - 1);
            }
        }
        cx.notify();
    }

    /// Starts the tour from step `0`.
    pub fn start(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if !self.steps.is_empty() {
            if self.active_step.is_none() {
                self.previous_focus = window.focused(cx);
            }
            self.active_step = Some(0);
            window.focus(&self.focus_handle, cx);
            cx.emit(TourEvent::StepChange(0));
            cx.notify();
        }
    }

    /// Jumps directly to 0-based step `index` while open.
    pub fn go_to(&mut self, index: usize, cx: &mut Context<Self>) {
        if self.active_step.is_some() && index < self.steps.len() && self.active_step != Some(index)
        {
            self.active_step = Some(index);
            cx.emit(TourEvent::StepChange(index));
            cx.notify();
        }
    }

    /// Returns whether the tour overlay is currently active.
    pub fn is_open(&self) -> bool {
        self.active_step.is_some()
    }

    /// Returns the active 0-based step index, if open.
    pub fn current_step(&self) -> Option<usize> {
        self.active_step
    }

    /// Advances to the next step, or finishes if on the last step.
    pub fn next(&mut self, cx: &mut Context<Self>) {
        let Some(idx) = self.active_step else {
            return;
        };
        if idx + 1 >= self.steps.len() {
            self.active_step = None;
            self.previous_focus = None;
            cx.emit(TourEvent::Finish);
        } else {
            self.active_step = Some(idx + 1);
            cx.emit(TourEvent::StepChange(idx + 1));
        }
        cx.notify();
    }

    /// Advances to the next step, restoring focus if finishing.
    pub fn next_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(idx) = self.active_step else {
            return;
        };
        if idx + 1 >= self.steps.len() {
            self.active_step = None;
            self.restore_focus(window, cx);
            cx.emit(TourEvent::Finish);
            cx.notify();
        } else {
            self.next(cx);
        }
    }

    /// Moves back to the previous step.
    pub fn prev(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.active_step
            && idx > 0
        {
            self.active_step = Some(idx - 1);
            cx.emit(TourEvent::StepChange(idx - 1));
            cx.notify();
        }
    }

    /// Dismisses the tour early.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.active_step.take().is_some() {
            self.previous_focus = None;
            cx.emit(TourEvent::Close);
            cx.notify();
        }
    }

    /// Dismisses the tour early and restores previous window focus.
    pub fn close_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_step.take().is_some() {
            self.restore_focus(window, cx);
            cx.emit(TourEvent::Close);
            cx.notify();
        }
    }

    fn restore_focus(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if let Some(prev) = self.previous_focus.take() {
            window.focus(&prev, cx);
        } else if self.focus_handle.is_focused(window) {
            window.blur(cx);
        }
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.active_step.is_none() {
            return;
        }
        let mods = ev.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        match ev.keystroke.key.as_str() {
            "escape" => {
                self.close_in(window, cx);
                cx.stop_propagation();
            }
            // Enter on one of the card's buttons presses that button instead.
            "enter" if !self.focus_handle.is_focused(window) => {}
            "right" | "enter" => {
                self.next_in(window, cx);
                cx.stop_propagation();
            }
            "left" => {
                self.prev(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

/// Transparent layout-tracking wrapper that registers `child`'s live screen bounds under `key`
/// so a [`Tour`] step with `TourStep::target(key, ...)` spotlights it automatically.
#[derive(IntoElement)]
pub struct TourTarget {
    state: Entity<TourState>,
    key: SharedString,
    child: AnyElement,
    style: StyleRefinement,
}

/// Wraps `child` so it can be spotlighted by a [`Tour`] step matching `key`.
pub fn tour_target(
    state: &Entity<TourState>,
    key: impl Into<SharedString>,
    child: impl IntoElement,
) -> TourTarget {
    TourTarget {
        state: state.clone(),
        key: key.into(),
        child: child.into_any_element(),
        style: StyleRefinement::default(),
    }
}

impl Styled for TourTarget {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TourTarget {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let (targets, frame_gen, is_open) = {
            let st = self.state.read(cx);
            (st.targets.clone(), st.frame_gen.clone(), st.is_open())
        };
        let key = self.key;
        let mut wrapper = div().relative().child(self.child).child(
            canvas(
                move |bounds, window, _| {
                    let generation = frame_gen.get();
                    let mut map = targets.borrow_mut();
                    let changed = map
                        .get(&key)
                        .map(|(_, prev)| *prev != bounds)
                        .unwrap_or(true);
                    map.insert(key.clone(), (generation, bounds));
                    // If a live target moves (resize/scroll/first mount) while tour is open,
                    // refresh immediately so the spotlight tracks with zero stale offset.
                    if is_open && changed {
                        window.refresh();
                    }
                },
                |_, _, _, _| {},
            )
            .absolute()
            .inset_0()
            .size_full(),
        );
        crate::Refineable::refine(wrapper.style(), &self.style);
        wrapper
    }
}

impl Render for TourState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let current_gen = self.frame_gen.get().wrapping_add(1).max(1);
        self.frame_gen.set(current_gen);

        let Some(idx) = self.active_step else {
            return div().into_any_element();
        };
        let Some(step) = self.steps.get(idx).cloned() else {
            return div().into_any_element();
        };

        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let entity_id = cx.entity_id();
        let total = self.steps.len();
        let is_first = idx == 0;
        let is_last = idx + 1 == total;
        let vp = window.viewport_size();
        let scrim = crate::Hsla::from(palette::SLATE_950).opacity(0.58);
        let pad = self.spotlight_padding.0;

        // Only honor targets actually mounted in the current/previous frame and intersecting the viewport.
        let target_bounds = step.target_key.as_ref().and_then(|k| {
            let (seen_gen, b) = *self.targets.borrow().get(k)?;
            if current_gen.saturating_sub(seen_gen) > 2 {
                return None;
            }
            let x1 = b.origin.x.0 - pad;
            let y1 = b.origin.y.0 - pad;
            let x2 = b.origin.x.0 + b.size.width.0 + pad;
            let y2 = b.origin.y.0 + b.size.height.0 + pad;
            if x2 <= 0.0 || y2 <= 0.0 || x1 >= vp.width.0 || y1 >= vp.height.0 {
                return None;
            }
            Some(b)
        });

        let card_max_w = (vp.width.0 - 32.0).max(200.0);
        let card_w = 328.0_f32.min(card_max_w);

        let mut step_card = div()
            .id(("tour-step-card", entity_id))
            .flex()
            .flex_col()
            .gap(px(12.))
            .w(px(card_w))
            .max_w(px(card_max_w))
            .p(px(16.))
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_2xl())
            .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                window.focus(&this.focus_handle, cx);
                cx.stop_propagation();
            }))
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .justify_between()
                    .gap(px(8.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(8.))
                            .flex_1()
                            .min_w_0()
                            .when_some(step.icon, |el, ic| {
                                el.child(
                                    div()
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .flex_shrink_0()
                                        .w(px(28.))
                                        .h(px(28.))
                                        .rounded(theme.radius_md)
                                        .bg(theme.primary_soft)
                                        .child(
                                            Icon::new(ic)
                                                .size(IconSize::Sm)
                                                .color(theme.primary_soft_foreground),
                                        ),
                                )
                            })
                            .child(
                                div()
                                    .min_w_0()
                                    .text_size(px(15.))
                                    .font_weight(FontWeight::SEMIBOLD)
                                    .text_color(theme.popover_foreground)
                                    .child(step.title),
                            ),
                    )
                    .child(
                        div()
                            .id(("tour-close-x", entity_id))
                            .flex_shrink_0()
                            .p(px(4.))
                            .rounded(theme.radius_sm)
                            .cursor_pointer()
                            .hover(|s| s.bg(theme.surface_hover))
                            .on_click(cx.listener(|this, _, window, cx| this.close_in(window, cx)))
                            .child(
                                Icon::new(LucideIcon::X)
                                    .size(IconSize::Sm)
                                    .color(theme.muted_foreground),
                            ),
                    ),
            )
            .child(
                div()
                    .text_size(px(13.))
                    .text_color(theme.muted_foreground)
                    .child(step.description),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .gap(px(8.))
                    .pt(px(4.))
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(5.))
                            .min_w_0()
                            .when(total <= 8, |el| {
                                el.children((0..total).map(|dot_i| {
                                    div()
                                        .id(("tour-dot", dot_i))
                                        .h(px(6.))
                                        .w(if dot_i == idx { px(16.) } else { px(6.) })
                                        .rounded_full()
                                        .bg(if dot_i == idx {
                                            theme.primary
                                        } else {
                                            theme.border
                                        })
                                        .cursor_pointer()
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.go_to(dot_i, cx);
                                        }))
                                }))
                            })
                            .child(
                                div()
                                    .ml(px(4.))
                                    .text_size(px(11.5))
                                    .font_features(tabular_numerals())
                                    .text_color(theme.muted_foreground)
                                    .child(format!("{} / {total}", idx + 1)),
                            ),
                    )
                    .child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .flex_shrink_0()
                            .gap(px(6.))
                            .when(!is_first, |el| {
                                el.child(
                                    button(("tour-prev", entity_id))
                                        .secondary()
                                        .size(ComponentSize::Xs)
                                        .label("Back")
                                        .on_click(cx.listener(|this, _, _, cx| this.prev(cx))),
                                )
                            })
                            .child(
                                button(("tour-next", entity_id))
                                    .primary()
                                    .size(ComponentSize::Xs)
                                    .label(if is_last { "Finish" } else { "Next" })
                                    .on_click(
                                        cx.listener(|this, _, window, cx| this.next_in(window, cx)),
                                    ),
                            ),
                    ),
            );
        crate::Refineable::refine(step_card.style(), &self.frame_style);

        let card_el = if reduce_motion {
            step_card.into_any_element()
        } else {
            step_card
                .with_animation(
                    ("tour-card-anim", entity_id),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
        };

        let scrim_slab = |id: &'static str| {
            div()
                .id((id, entity_id))
                .absolute()
                .bg(scrim)
                .occlude()
                // Dismiss only on pointer mouse-down on the darkened scrim, NEVER on keyboard Enter/Space
                // and NEVER when clicking inside the spotlighted target cutout!
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_in(window, cx)),
                )
        };

        let overlay = if let Some(b) = target_bounds {
            let x1 = (b.origin.x.0 - pad).clamp(0.0, vp.width.0);
            let y1 = (b.origin.y.0 - pad).clamp(0.0, vp.height.0);
            let x2 = (b.origin.x.0 + b.size.width.0 + pad).clamp(x1, vp.width.0);
            let y2 = (b.origin.y.0 + b.size.height.0 + pad).clamp(y1, vp.height.0);
            let hole_w = (x2 - x1).max(0.0);
            let hole_h = (y2 - y1).max(0.0);
            let spotlight_bounds = Bounds::new(point(px(x1), px(y1)), size(px(hole_w), px(hole_h)));

            div()
                .id(("tour-overlay-spotlight", entity_id))
                .track_focus(&self.focus_handle)
                // Modal: Tab stays on the step card's buttons.
                .focus_trap()
                .on_key_down(cx.listener(Self::on_key_down))
                .relative()
                .w(vp.width)
                .h(vp.height)
                // 4 surrounding darkened scrim slabs forming a crisp cutout window
                .child(
                    scrim_slab("tour-scrim-t")
                        .top_0()
                        .left_0()
                        .w_full()
                        .h(px(y1)),
                )
                .child(
                    scrim_slab("tour-scrim-b")
                        .top(px(y2))
                        .left_0()
                        .w_full()
                        .h(px((vp.height.0 - y2).max(0.0))),
                )
                .child(
                    scrim_slab("tour-scrim-l")
                        .top(px(y1))
                        .left_0()
                        .w(px(x1))
                        .h(px(hole_h)),
                )
                .child(
                    scrim_slab("tour-scrim-r")
                        .top(px(y1))
                        .left(px(x2))
                        .w(px((vp.width.0 - x2).max(0.0)))
                        .h(px(hole_h)),
                )
                // Crisp spotlight ring around the target
                .child(
                    div()
                        .absolute()
                        .left(px(x1))
                        .top(px(y1))
                        .w(px(hole_w))
                        .h(px(hole_h))
                        .rounded(theme.radius_lg)
                        .border_2()
                        .border_color(theme.primary),
                )
                // Auto-flipping anchored step card attached directly to spotlight bounds!
                .child(
                    anchored()
                        .anchor(Anchor::TopLeft)
                        .attach(spotlight_bounds, Anchor::BottomLeft)
                        .offset(point(px(0.), px(12.)))
                        .snap_to_window_with_margin(px(12.))
                        .child(card_el),
                )
        } else {
            let blur_r = self.backdrop_blur;
            div()
                .id(("tour-overlay-center", entity_id))
                .track_focus(&self.focus_handle)
                // Modal: Tab stays on the step card's buttons.
                .focus_trap()
                .on_key_down(cx.listener(Self::on_key_down))
                .w(vp.width)
                .h(vp.height)
                .occlude()
                .flex()
                .items_center()
                .justify_center()
                .bg(scrim)
                .when(blur_r > px(0.), |el| el.backdrop_blur(blur_r))
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_in(window, cx)),
                )
                .child(card_el)
        };

        deferred(anchored().position(point(px(0.), px(0.))).child(overlay))
            .priority(58)
            .into_any_element()
    }
}

/// Renderable [`Tour`] overlay bound to [`Entity<TourState>`].
#[derive(IntoElement)]
pub struct Tour {
    state: Entity<TourState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Tour::new`].
pub fn tour(state: &Entity<TourState>) -> Tour {
    Tour::new(state)
}

impl Tour {
    /// Creates a [`Tour`] overlay bound to `state`.
    pub fn new(state: &Entity<TourState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for Tour {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Tour {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        if self.state.read(cx).frame_style != self.style {
            let style = self.style;
            self.state.update(cx, |s, _| {
                s.frame_style = style;
            });
        }
        self.state
    }
}

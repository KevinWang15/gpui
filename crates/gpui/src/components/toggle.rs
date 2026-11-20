//! [`Toggle`] / [`Switch`], [`ToggleButton`], and [`ToggleGroup`] components.

use std::cell::RefCell;
use std::rc::Rc;
use std::time::{Duration, Instant};

use smallvec::SmallVec;

use crate::{
    AnimationExt, AnyElement, App, BorderStyle, Bounds, ClickEvent, Corners, Edges, ElementId,
    FontWeight, Hsla, Interpolate, IntoElement, ParentElement, Point,
    QuadAnimationCurve, QuadAnimationMode, RenderOnce, SharedString, Size, SpringAnimation,
    StyleRefinement, Styled, Window, canvas, div, prelude::*, px, quad, transparent_black,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::theme::{ActiveTheme, ComponentSize, SPRING_SNAPPY};

struct ToggleAnimState {
    last_checked: bool,
    from_phase: f32,
    started_at: Option<Instant>,
}

const TOGGLE_ANIM_MAX_MS: u64 = 520;

/// Alias for [`Toggle`].
pub type Switch = Toggle;

/// Toggle / Headless UI Switch with pill track, sliding thumb, optional
/// on/off icons inside the thumb, and optional label & description.
#[derive(IntoElement)]
pub struct Toggle {
    id: ElementId,
    checked: bool,
    disabled: bool,
    size: ComponentSize,
    with_icons: bool,
    label: Option<SharedString>,
    description: Option<SharedString>,
    label_left: bool,
    track_on_color: Option<Hsla>,
    track_off_color: Option<Hsla>,
    thumb_color: Option<Hsla>,
    style: StyleRefinement,
    on_change: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`Toggle::new`].
pub fn toggle(id: impl Into<ElementId>, checked: bool) -> Toggle {
    Toggle::new(id, checked)
}

/// Convenience constructor alias for [`Toggle::new`].
pub fn switch(id: impl Into<ElementId>, checked: bool) -> Switch {
    Toggle::new(id, checked)
}

impl Toggle {
    /// Creates a new [`Toggle`] in the given `checked` state.
    pub fn new(id: impl Into<ElementId>, checked: bool) -> Self {
        Self {
            id: id.into(),
            checked,
            disabled: false,
            size: ComponentSize::Md,
            with_icons: false,
            label: None,
            description: None,
            label_left: false,
            track_on_color: None,
            track_off_color: None,
            thumb_color: None,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Sets the toggle size (`Sm`, `Md`, or `Lg`).
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Renders subtle check/x Lucide icons inside the circular thumb.
    pub fn with_icons(mut self, with_icons: bool) -> Self {
        self.with_icons = with_icons;
        self
    }

    /// Attaches a label next to the toggle switch.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Attaches a muted description under the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Places label/description on the left with the switch aligned to the right.
    pub fn label_left(mut self, label_left: bool) -> Self {
        self.label_left = label_left;
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Overrides the checked track color (defaults to [`Theme::toggle_track_on`](super::Theme::toggle_track_on)).
    pub fn track_on_color(mut self, color: Hsla) -> Self {
        self.track_on_color = Some(color);
        self
    }

    /// Overrides the unchecked track color (defaults to [`Theme::toggle_track_off`](super::Theme::toggle_track_off)).
    pub fn track_off_color(mut self, color: Hsla) -> Self {
        self.track_off_color = Some(color);
        self
    }

    /// Overrides the circular thumb color (defaults to [`Theme::toggle_thumb`](super::Theme::toggle_thumb)).
    pub fn thumb_color(mut self, color: Hsla) -> Self {
        self.thumb_color = Some(color);
        self
    }

    /// Registers a callback invoked with the new boolean state whenever toggled.
    pub fn on_change(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_change = Some(Box::new(handler));
        self
    }
}

impl Styled for Toggle {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Toggle {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let (track_w, track_h, thumb_size, pad) = match self.size {
            ComponentSize::Xs | ComponentSize::Sm => (px(34.), px(20.), px(16.), px(2.)),
            ComponentSize::Md => (px(44.), px(24.), px(20.), px(2.)),
            ComponentSize::Lg | ComponentSize::Xl => (px(52.), px(28.), px(24.), px(2.)),
        };

        let off_bg: Hsla = self.track_off_color.unwrap_or(theme.toggle_track_off);
        let on_bg: Hsla = self.track_on_color.unwrap_or(theme.toggle_track_on);
        let thumb_bg: Hsla = self.thumb_color.unwrap_or(theme.toggle_thumb);
        let travel_end = track_w - thumb_size - pad;
        let checked = self.checked;
        let with_icons = self.with_icons;
        // The row is the tab stop, but the ring goes on the track, as a
        // browser draws it; so the row tracks a handle the track can ask about.
        let focus = (!self.disabled).then(|| {
            window
                .use_keyed_state((self.id.clone(), "focus"), cx, |_, cx| cx.focus_handle())
                .read(cx)
                .clone()
        });
        let focus_outline = focus
            .as_ref()
            .and_then(|handle| super::focus::focus_visible_outline(handle, window, &theme));

        let track = if !with_icons {
            // Zero-relayout GPU scene-animated track color + sliding thumb.
            let target_phase = if checked { 1.0f32 } else { 0.0f32 };
            let state_entity = window.use_keyed_state(
                (self.id.clone(), "gpu-toggle-anim"),
                cx,
                |_, _| {
                    Rc::new(RefCell::new(ToggleAnimState {
                        last_checked: checked,
                        from_phase: target_phase,
                        started_at: None,
                    }))
                },
            );
            let state_rc = state_entity.read(cx).clone();
            let now = cx.background_executor().now();
            let reduce_motion = cx.reduce_motion();
            let max_dur = Duration::from_millis(TOGGLE_ANIM_MAX_MS);
            let curve = QuadAnimationCurve::from(SPRING_SNAPPY);

            let (from_phase, active_start) = {
                let mut st = state_rc.borrow_mut();
                if st.last_checked != checked {
                    let prev_target = if st.last_checked { 1.0f32 } else { 0.0f32 };
                    let current = if let Some(started) = st.started_at {
                        let elapsed = now.saturating_duration_since(started).as_secs_f32();
                        let (pos, _) = curve.evaluate(elapsed, max_dur.as_secs_f32());
                        st.from_phase + (prev_target - st.from_phase) * pos
                    } else {
                        prev_target
                    };
                    st.from_phase = current;
                    st.last_checked = checked;
                    st.started_at = (!reduce_motion).then_some(now);
                }
                let active = st.started_at.filter(|started| {
                    !reduce_motion && now.saturating_duration_since(*started) < max_dur
                });
                (st.from_phase, active)
            };

            let from_bg = Hsla::interpolate(off_bg, on_bg, from_phase.clamp(0.0, 1.0));
            let to_bg = if checked { on_bg } else { off_bg };
            let from_thumb_x = px(pad.0 + (travel_end.0 - pad.0) * from_phase);
            let to_thumb_x = if checked { travel_end } else { pad };

            div()
                .relative()
                .flex_shrink_0()
                .w(track_w)
                .h(track_h)
                // The canvas paints the pill; the rounding shapes the ring.
                .rounded_full()
                .when_some(focus_outline, |el, outline| el.outline(outline))
                .child(
                    canvas(
                        |_, _, _| {},
                        move |bounds, _, window, _| {
                            let track_radius = bounds.size.height * 0.5;
                            let thumb_radius = thumb_size * 0.5;
                            let track_quad = quad(
                                bounds,
                                Corners::all(track_radius),
                                to_bg,
                                Edges::default(),
                                transparent_black(),
                                BorderStyle::Solid,
                            );
                            let to_thumb = Bounds {
                                origin: Point::new(bounds.origin.x + to_thumb_x, bounds.origin.y + pad),
                                size: Size::new(thumb_size, thumb_size),
                            };
                            let thumb_quad = quad(
                                to_thumb,
                                Corners::all(thumb_radius),
                                thumb_bg,
                                Edges::default(),
                                transparent_black(),
                                BorderStyle::Solid,
                            );

                            if let Some(started_at) = active_start {
                                let mode = QuadAnimationMode::Oneshot {
                                    started_at,
                                    duration: max_dur,
                                    curve,
                                };
                                window.paint_animated_quad(
                                    track_quad,
                                    mode,
                                    bounds,
                                    from_bg,
                                    transparent_black(),
                                );
                                let from_thumb = Bounds {
                                    origin: Point::new(
                                        bounds.origin.x + from_thumb_x,
                                        bounds.origin.y + pad,
                                    ),
                                    size: Size::new(thumb_size, thumb_size),
                                };
                                window.paint_animated_quad(
                                    thumb_quad,
                                    mode,
                                    from_thumb,
                                    thumb_bg,
                                    transparent_black(),
                                );
                            } else {
                                window.paint_quad(track_quad);
                                window.paint_quad(thumb_quad);
                            }
                        },
                    )
                    .size_full(),
                )
                .into_any_element()
        } else {
            let track_id = (self.id.clone(), "track");
            let thumb_id = (self.id.clone(), "thumb");

            let thumb = div()
                .absolute()
                .top(pad)
                .flex()
                .items_center()
                .justify_center()
                .w(thumb_size)
                .h(thumb_size)
                .rounded_full()
                .bg(thumb_bg)
                .when(theme.elevation_shadows, |el| el.shadow_xs())
                .child({
                    let icon = if checked {
                        LucideIcon::Check
                    } else {
                        LucideIcon::X
                    };
                    let color = if checked {
                        on_bg
                    } else {
                        theme.muted_foreground
                    };
                    Icon::new(icon).size(IconSize::Xs).color(color)
                })
                .with_spring(
                    thumb_id,
                    SpringAnimation::new(SPRING_SNAPPY).to(checked),
                    move |el, phase| el.left(phase.interpolate(pad, travel_end)),
                );

            div()
                .relative()
                .flex_shrink_0()
                .w(track_w)
                .h(track_h)
                .rounded_full()
                .when_some(focus_outline, |el, outline| el.outline(outline))
                .child(thumb)
                .with_spring(
                    track_id,
                    SpringAnimation::new(SPRING_SNAPPY).to(checked),
                    move |el, phase| el.bg(phase.interpolate_clamped(off_bg, on_bg)),
                )
                .into_any_element()
        };

        let has_text = self.label.is_some() || self.description.is_some();
        let text_block = if has_text {
            Some(
                div()
                    .flex()
                    .flex_col()
                    .gap(px(2.))
                    .when_some(self.label, |el, label| {
                        el.child(
                            div()
                                .text_size(px(14.))
                                .font_weight(FontWeight::MEDIUM)
                                .text_color(theme.foreground)
                                .child(label),
                        )
                    })
                    .when_some(self.description, |el, desc| {
                        el.child(
                            div()
                                .text_size(px(12.5))
                                .text_color(theme.muted_foreground)
                                .child(desc),
                        )
                    }),
            )
        } else {
            None
        };

        let next_checked = !self.checked;
        let on_change = self.on_change;
        let mut root = div()
            .id(self.id)
            .when_some(focus, |el, handle| el.track_focus(&handle).tab_stop(true))
            .flex()
            .flex_row()
            .items_center()
            .gap(px(12.))
            .when(self.label_left && has_text, |el| el.justify_between().w_full())
            .when(!self.disabled, |el| el.cursor_pointer())
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed());

        if self.label_left {
            if let Some(text) = text_block {
                root = root.child(text);
            }
            root = root.child(track);
        } else {
            root = root.child(track);
            if let Some(text) = text_block {
                root = root.child(text);
            }
        }

        if !self.disabled
            && let Some(handler) = on_change
        {
            // Enter and Space come here too, as a keyboard click.
            root = root.on_click(move |_: &ClickEvent, window, cx| {
                handler(&next_checked, window, cx);
            });
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Pressable toolbar toggle button (e.g. Bold / Italic / Grid view switcher).
#[derive(IntoElement)]
pub struct ToggleButton {
    id: ElementId,
    label: Option<SharedString>,
    icon: Option<LucideIcon>,
    pressed: bool,
    disabled: bool,
    size: ComponentSize,
    style: StyleRefinement,
    on_toggle: Option<Box<dyn Fn(&bool, &mut Window, &mut App) + 'static>>,
}

impl ToggleButton {
    /// Creates a new [`ToggleButton`].
    pub fn new(id: impl Into<ElementId>, pressed: bool) -> Self {
        Self {
            id: id.into(),
            label: None,
            icon: None,
            pressed,
            disabled: false,
            size: ComponentSize::Sm,
            style: StyleRefinement::default(),
            on_toggle: None,
        }
    }

    /// Sets the button label.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets a [`LucideIcon`].
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets button size.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Disables the button.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a toggle callback invoked with the toggled boolean state.
    pub fn on_toggle(
        mut self,
        handler: impl Fn(&bool, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_toggle = Some(Box::new(handler));
        self
    }
}

impl Styled for ToggleButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ToggleButton {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let next = !self.pressed;
        let (bg, default_fg) = if self.pressed {
            (theme.primary_soft, theme.primary_soft_foreground)
        } else {
            (crate::transparent_black(), theme.muted_foreground)
        };
        let fg = self.style.text.color.unwrap_or(default_fg);

        let el = div()
            .id(self.id)
            .flex()
            .flex_row()
            .items_center()
            .justify_center()
            .gap(px(6.))
            .h(self.size.height())
            .px(self.size.px_x())
            .rounded(theme.radius_sm)
            .bg(bg)
            .text_color(fg)
            .text_size(self.size.font_size())
            .font_weight(FontWeight::MEDIUM)
            .when(!self.disabled, |el| {
                el.cursor_pointer().hover(|s| {
                    if self.pressed {
                        s.bg(theme.primary_soft)
                    } else {
                        s.bg(theme.surface_hover).text_color(theme.foreground)
                    }
                })
            })
            .when(self.disabled, |el| el.opacity(0.5).cursor_not_allowed())
            .when_some(self.icon, |el, icon| {
                el.child(Icon::new(icon).size(self.size.icon_size()).color(fg))
            })
            .when_some(self.label, |el, label| el.child(label));
        let mut el = super::focus::tab_stop(el, !self.disabled, &theme)
            .when(!self.disabled, |el| {
                el.when_some(self.on_toggle, |el, handler| {
                    el.on_click(move |_, window, cx| handler(&next, window, cx))
                })
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

/// Framed horizontal group of [`ToggleButton`] controls.
#[derive(IntoElement)]
pub struct ToggleGroup {
    style: StyleRefinement,
    children: SmallVec<[AnyElement; 4]>,
}

/// Creates a new [`ToggleGroup`].
pub fn toggle_group() -> ToggleGroup {
    ToggleGroup {
        style: StyleRefinement::default(),
        children: SmallVec::new(),
    }
}

impl ParentElement for ToggleGroup {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for ToggleGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ToggleGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(2.))
            .p(px(3.))
            .rounded(theme.radius_md)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .children(self.children);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

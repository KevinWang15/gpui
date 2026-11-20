//! Interactive [`Carousel`] slide deck with prev/next arrow controls, clickable dot/pill
//! indicators, smooth entrance transitions, keyboard `Left`/`Right`/`Home`/`End` navigation,
//! and hover/reduce-motion-aware optional autoplay timer.

use std::time::Duration;

use smallvec::SmallVec;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Animation, AnimationExt, AnyElement, App, Context, ElementId, Entity, EventEmitter,
    FocusHandle, Focusable, IntoElement, KeyDownEvent, RenderOnce, StyleRefinement, Styled, Task,
    Window, div, ease_out_quint, prelude::*, px,
};

use super::palette;
use super::theme::{ANIM_NORMAL, ActiveTheme, focus_ring_shadow};

const MIN_AUTOPLAY_INTERVAL: Duration = Duration::from_millis(100);

/// Events emitted by [`CarouselState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CarouselEvent {
    /// Emitted when the active 0-based slide index changes.
    Change(usize),
}

/// Entity state tracking active slide index, looping, and optional autoplay for [`Carousel`].
pub struct CarouselState {
    focus_handle: FocusHandle,
    active_index: usize,
    slide_count: usize,
    looping: bool,
    hovered: bool,
    autoplay_interval: Option<Duration>,
    _autoplay_task: Option<Task<()>>,
}

impl EventEmitter<CarouselEvent> for CarouselState {}

impl Focusable for CarouselState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl CarouselState {
    /// Creates a new [`CarouselState`] for `slide_count` slides starting at slide `0`.
    pub fn new(slide_count: usize, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            active_index: 0,
            slide_count,
            looping: true,
            hovered: false,
            autoplay_interval: None,
            _autoplay_task: None,
        }
    }

    /// Enables or disables wrap-around looping (default `true`).
    pub fn looping(mut self, looping: bool) -> Self {
        self.looping = looping;
        self
    }

    /// Enables automatic slide cycling every `interval` (`Duration::ZERO` disables autoplay;
    /// sub-100ms intervals are clamped to `100ms` to prevent tight-loop executor spinning).
    pub fn autoplay(mut self, interval: Duration, cx: &mut Context<Self>) -> Self {
        self.autoplay_interval = if interval.is_zero() {
            None
        } else {
            Some(interval.max(MIN_AUTOPLAY_INTERVAL))
        };
        self.restart_autoplay(cx);
        self
    }

    /// Toggles autoplay on/off dynamically.
    pub fn set_autoplay(&mut self, interval: Option<Duration>, cx: &mut Context<Self>) {
        self.autoplay_interval = interval.and_then(|d| {
            if d.is_zero() {
                None
            } else {
                Some(d.max(MIN_AUTOPLAY_INTERVAL))
            }
        });
        self.restart_autoplay(cx);
        cx.notify();
    }

    /// Returns whether autoplay is currently active.
    pub fn is_autoplaying(&self) -> bool {
        self.autoplay_interval.is_some()
    }

    /// Returns the active 0-based slide index.
    pub fn active_index(&self) -> usize {
        self.active_index
    }

    /// Jumps directly to `index` (and restarts autoplay countdown if enabled).
    pub fn go_to(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.slide_count && index != self.active_index {
            self.active_index = index;
            if self.autoplay_interval.is_some() {
                self.restart_autoplay(cx);
            }
            cx.emit(CarouselEvent::Change(index));
            cx.notify();
        }
    }

    /// Advances to the next slide.
    pub fn next(&mut self, cx: &mut Context<Self>) {
        if self.slide_count <= 1 {
            return;
        }
        let next = if self.active_index + 1 < self.slide_count {
            self.active_index + 1
        } else if self.looping {
            0
        } else {
            return;
        };
        self.go_to(next, cx);
    }

    /// Steps back to the previous slide.
    pub fn prev(&mut self, cx: &mut Context<Self>) {
        if self.slide_count <= 1 {
            return;
        }
        let prev = if self.active_index > 0 {
            self.active_index - 1
        } else if self.looping {
            self.slide_count - 1
        } else {
            return;
        };
        self.go_to(prev, cx);
    }

    fn advance_from_timer(&mut self, cx: &mut Context<Self>) -> bool {
        if self.autoplay_interval.is_none() || self.slide_count <= 1 {
            return false;
        }
        // Pause politely while hovered or when OS reduce-motion preference is active.
        if self.hovered || cx.reduce_motion() {
            return true;
        }
        if self.active_index + 1 < self.slide_count {
            self.active_index += 1;
            cx.emit(CarouselEvent::Change(self.active_index));
            cx.notify();
            true
        } else if self.looping {
            self.active_index = 0;
            cx.emit(CarouselEvent::Change(0));
            cx.notify();
            true
        } else {
            // Reached end of non-looping deck — stop spinning the background timer.
            self._autoplay_task = None;
            false
        }
    }

    fn restart_autoplay(&mut self, cx: &mut Context<Self>) {
        self._autoplay_task = self.autoplay_interval.map(|interval| {
            let safe_interval = interval.max(MIN_AUTOPLAY_INTERVAL);
            cx.spawn(async move |weak, cx| {
                loop {
                    cx.background_executor().timer(safe_interval).await;
                    let Ok(keep_going) = weak.update(cx, |this, cx| this.advance_from_timer(cx))
                    else {
                        break;
                    };
                    if !keep_going {
                        break;
                    }
                }
            })
        });
    }
}

/// Interactive slide deck container rendering one active slide at a time with arrow buttons
/// and clickable indicator pills.
#[derive(IntoElement)]
pub struct Carousel {
    id: ElementId,
    state: Entity<CarouselState>,
    slides: SmallVec<[AnyElement; 6]>,
    show_arrows: bool,
    show_dots: bool,
    style: StyleRefinement,
}

/// Convenience constructor for [`Carousel::new`].
pub fn carousel(id: impl Into<ElementId>, state: &Entity<CarouselState>) -> Carousel {
    Carousel::new(id, state)
}

impl Carousel {
    /// Creates a [`Carousel`] bound to `state`.
    pub fn new(id: impl Into<ElementId>, state: &Entity<CarouselState>) -> Self {
        Self {
            id: id.into(),
            state: state.clone(),
            slides: SmallVec::new(),
            show_arrows: true,
            show_dots: true,
            style: StyleRefinement::default(),
        }
    }

    /// Appends a single slide element.
    pub fn slide(mut self, slide: impl IntoElement) -> Self {
        self.slides.push(slide.into_any_element());
        self
    }

    /// Appends multiple slide elements.
    pub fn slides(mut self, slides: impl IntoIterator<Item = AnyElement>) -> Self {
        self.slides.extend(slides);
        self
    }

    /// Shows or hides floating prev/next arrow buttons (default `true`).
    pub fn show_arrows(mut self, show: bool) -> Self {
        self.show_arrows = show;
        self
    }

    /// Shows or hides bottom indicator pills (default `true`).
    pub fn show_dots(mut self, show: bool) -> Self {
        self.show_dots = show;
        self
    }
}

impl Styled for Carousel {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Carousel {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let slide_count = self.slides.len();

        if self.state.read(cx).slide_count != slide_count {
            self.state.update(cx, |s, cx| {
                s.slide_count = slide_count;
                if slide_count == 0 {
                    s.active_index = 0;
                } else if s.active_index >= slide_count {
                    s.active_index = 0;
                    cx.emit(CarouselEvent::Change(0));
                }
            });
        }

        let (active_idx, looping, focus_handle) = {
            let st = self.state.read(cx);
            (
                st.active_index.min(slide_count.saturating_sub(1)),
                st.looping,
                st.focus_handle.clone(),
            )
        };
        let focused = focus_handle.is_focused(window);
        let can_prev = slide_count > 1 && (looping || active_idx > 0);
        let can_next = slide_count > 1 && (looping || active_idx + 1 < slide_count);

        let active_slide = self.slides.into_iter().nth(active_idx);
        let state_prev = self.state.clone();
        let state_next = self.state.clone();
        let state_key = self.state.clone();
        let state_hover = self.state.clone();

        let slide_stage = div().w_full().h_full().children(active_slide);
        let animated_slide = if reduce_motion {
            slide_stage.into_any_element()
        } else {
            slide_stage
                .with_animation(
                    ("car-slide", active_idx),
                    Animation::new(ANIM_NORMAL).with_easing(ease_out_quint()),
                    |el, delta| el.opacity(delta),
                )
                .into_any_element()
        };

        let arrow_bg = crate::Hsla::from(palette::SLATE_900).opacity(0.62);
        let arrow_hover = crate::Hsla::from(palette::SLATE_900).opacity(0.84);

        let mut root = div()
            .id(self.id)
            .track_focus(&focus_handle)
            .tab_stop(true)
            .on_hover(move |is_hovered, _, cx| {
                state_hover.update(cx, |s, _| {
                    s.hovered = *is_hovered;
                });
            })
            .on_key_down(move |ev: &KeyDownEvent, _, cx| {
                let mods = ev.keystroke.modifiers;
                if mods.control || mods.platform || mods.alt {
                    return;
                }
                match ev.keystroke.key.as_str() {
                    "left" => {
                        state_key.update(cx, |s, cx| s.prev(cx));
                        cx.stop_propagation();
                    }
                    "right" => {
                        state_key.update(cx, |s, cx| s.next(cx));
                        cx.stop_propagation();
                    }
                    "home" => {
                        state_key.update(cx, |s, cx| s.go_to(0, cx));
                        cx.stop_propagation();
                    }
                    "end" => {
                        state_key.update(cx, |s, cx| {
                            if s.slide_count > 0 {
                                s.go_to(s.slide_count - 1, cx);
                            }
                        });
                        cx.stop_propagation();
                    }
                    _ => {}
                }
            })
            .relative()
            .w_full()
            .min_h(px(180.))
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(if focused { theme.ring } else { theme.border })
            .when(focused, |el| el.shadow(focus_ring_shadow(&theme)))
            .bg(theme.surface)
            .overflow_hidden()
            .child(animated_slide)
            .when(self.show_arrows && slide_count > 1, |el| {
                el.child(
                    div()
                        .id("car-prev")
                        .absolute()
                        .left(px(12.))
                        .top(crate::relative(0.5))
                        .mt(px(-16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(32.))
                        .h(px(32.))
                        .rounded_full()
                        .bg(arrow_bg)
                        .text_color(palette::WHITE)
                        .when(can_prev, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.bg(arrow_hover))
                                .on_click(move |_, _, cx| {
                                    state_prev.update(cx, |s, cx| s.prev(cx));
                                })
                        })
                        .when(!can_prev, |el| el.opacity(0.35).cursor_not_allowed())
                        .child(Icon::new(LucideIcon::ChevronLeft).size(IconSize::Sm)),
                )
                .child(
                    div()
                        .id("car-next")
                        .absolute()
                        .right(px(12.))
                        .top(crate::relative(0.5))
                        .mt(px(-16.))
                        .flex()
                        .items_center()
                        .justify_center()
                        .w(px(32.))
                        .h(px(32.))
                        .rounded_full()
                        .bg(arrow_bg)
                        .text_color(palette::WHITE)
                        .when(can_next, |el| {
                            el.cursor_pointer()
                                .hover(move |s| s.bg(arrow_hover))
                                .on_click(move |_, _, cx| {
                                    state_next.update(cx, |s, cx| s.next(cx));
                                })
                        })
                        .when(!can_next, |el| el.opacity(0.35).cursor_not_allowed())
                        .child(Icon::new(LucideIcon::ChevronRight).size(IconSize::Sm)),
                )
            })
            .when(self.show_dots && slide_count > 1, |el| {
                let st_dots = self.state.clone();
                el.child(
                    div()
                        .absolute()
                        .bottom(px(10.))
                        .left_0()
                        .right_0()
                        .flex()
                        .justify_center()
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(2.))
                                .px(px(6.))
                                .py(px(2.))
                                .rounded_full()
                                .bg(crate::Hsla::from(palette::SLATE_950).opacity(0.38))
                                .children((0..slide_count).map(move |i| {
                                    let is_active = i == active_idx;
                                    let st_i = st_dots.clone();
                                    div()
                                        .id(("car-dot", i))
                                        .flex()
                                        .items_center()
                                        .justify_center()
                                        .h(px(20.))
                                        .px(px(3.))
                                        .cursor_pointer()
                                        .on_click(move |_, _, cx| {
                                            st_i.update(cx, |s, cx| s.go_to(i, cx));
                                        })
                                        .child(
                                            div()
                                                .h(px(6.))
                                                .w(if is_active { px(20.) } else { px(6.) })
                                                .rounded_full()
                                                .bg(if is_active {
                                                    crate::Hsla::from(palette::WHITE)
                                                } else {
                                                    crate::Hsla::from(palette::WHITE).opacity(0.45)
                                                }),
                                        )
                                })),
                        ),
                )
            });

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

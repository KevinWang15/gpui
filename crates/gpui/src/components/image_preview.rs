//! Interactive [`ImagePreview`] & [`ImagePreviewState`] thumbnail gallery and full-window
//! GPU-blurred lightbox viewer with zoom (`50%`–`300%`), 90° rotation state, prev/next navigation,
//! and keyboard controls (`Left`/`Right`/`+`/`-`/`R`/`Shift+R`/`0`/`Escape`).

use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Animation, AnimationExt, AnyElement, App, Context, Entity, FocusHandle, Focusable, FontWeight,
    ImageSource, IntoElement, KeyDownEvent, MouseButton, Pixels, Render, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, anchored, deferred, div, ease_out_quint, img, point,
    prelude::*, px,
};

use super::palette as tw;
use super::theme::{ANIM_FAST, ActiveTheme, tabular_numerals};

/// Visual source rendered inside an [`ImagePreviewItem`].
#[derive(Clone)]
pub enum PreviewMedia {
    /// Standard GPUI [`ImageSource`] (file path, URI, embedded bytes, or `RenderImage`).
    Image(ImageSource),
    /// Custom vector/canvas preview builder (great for live diagrams, icons, or synthetic previews).
    Custom(Rc<dyn Fn(&mut Window, &mut App) -> AnyElement + 'static>),
}

/// Single media item displayed in an [`ImagePreview`] gallery.
#[derive(Clone)]
pub struct ImagePreviewItem {
    /// Display caption shown in the lightbox header.
    pub title: SharedString,
    /// Optional muted subtitle / metadata (e.g. dimensions or date).
    pub subtitle: Option<SharedString>,
    /// Underlying media source.
    pub media: PreviewMedia,
}

impl ImagePreviewItem {
    /// Creates an item backed by a GPUI [`ImageSource`].
    pub fn image(title: impl Into<SharedString>, source: impl Into<ImageSource>) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            media: PreviewMedia::Image(source.into()),
        }
    }

    /// Creates an item backed by a custom element builder closure.
    pub fn custom(
        title: impl Into<SharedString>,
        builder: impl Fn(&mut Window, &mut App) -> AnyElement + 'static,
    ) -> Self {
        Self {
            title: title.into(),
            subtitle: None,
            media: PreviewMedia::Custom(Rc::new(builder)),
        }
    }

    /// Sets optional caption metadata.
    pub fn subtitle(mut self, subtitle: impl Into<SharedString>) -> Self {
        self.subtitle = Some(subtitle.into());
        self
    }
}

/// Entity state backing an [`ImagePreview`] gallery & lightbox overlay.
pub struct ImagePreviewState {
    focus_handle: FocusHandle,
    previous_focus: Option<FocusHandle>,
    items: Vec<ImagePreviewItem>,
    open_index: Option<usize>,
    zoom: f32,
    rotation_deg: i32,
    thumb_width: Pixels,
    thumb_height: Pixels,
    backdrop_blur: Pixels,
    frame_style: StyleRefinement,
}

impl Focusable for ImagePreviewState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ImagePreviewState {
    /// Creates a new [`ImagePreviewState`] with gallery `items`.
    pub fn new(items: Vec<ImagePreviewItem>, cx: &mut Context<Self>) -> Self {
        Self {
            focus_handle: cx.focus_handle(),
            previous_focus: None,
            items,
            open_index: None,
            zoom: 1.0,
            rotation_deg: 0,
            thumb_width: px(136.),
            thumb_height: px(96.),
            backdrop_blur: px(10.),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets thumbnail card dimensions (default `136×96px`).
    pub fn thumbnail_size(mut self, width: Pixels, height: Pixels) -> Self {
        self.thumb_width = if width.0.is_finite() {
            width.max(px(32.))
        } else {
            px(136.)
        };
        self.thumb_height = if height.0.is_finite() {
            height.max(px(32.))
        } else {
            px(96.)
        };
        self
    }

    /// Sets the GPU backdrop blur radius on the lightbox scrim (default `10px`).
    pub fn backdrop_blur(mut self, radius: Pixels) -> Self {
        self.backdrop_blur = if radius.0.is_finite() {
            radius.max(px(0.))
        } else {
            px(10.)
        };
        self
    }

    /// Replaces gallery items dynamically, closing or clamping the open lightbox index if needed.
    pub fn set_items(&mut self, items: Vec<ImagePreviewItem>, cx: &mut Context<Self>) {
        self.items = items;
        if let Some(idx) = self.open_index {
            if self.items.is_empty() {
                self.open_index = None;
            } else if idx >= self.items.len() {
                self.open_index = Some(self.items.len() - 1);
            }
        }
        cx.notify();
    }

    /// Returns whether the lightbox is currently open.
    pub fn is_open(&self) -> bool {
        self.open_index.is_some()
    }

    /// Returns current zoom factor (`0.5..=3.0`).
    pub fn zoom(&self) -> f32 {
        self.zoom
    }

    /// Returns current rotation in degrees (`0`, `90`, `180`, `270`).
    pub fn rotation_deg(&self) -> i32 {
        self.rotation_deg
    }

    /// Opens the lightbox on item `index` and resets zoom/rotation.
    pub fn open(&mut self, index: usize, window: &mut Window, cx: &mut Context<Self>) {
        if index < self.items.len() {
            if self.open_index.is_none() {
                self.previous_focus = window.focused(cx);
            }
            self.open_index = Some(index);
            self.zoom = 1.0;
            self.rotation_deg = 0;
            window.focus(&self.focus_handle, cx);
            cx.notify();
        }
    }

    /// Closes the lightbox.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open_index.take().is_some() {
            self.previous_focus = None;
            cx.notify();
        }
    }

    /// Closes the lightbox and restores previous window focus.
    pub fn close_in(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_index.take().is_some() {
            if let Some(prev) = self.previous_focus.take() {
                window.focus(&prev, cx);
            } else if self.focus_handle.is_focused(window) {
                window.blur(cx);
            }
            cx.notify();
        }
    }

    /// Steps to the next item (wrapping around).
    pub fn next(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.open_index
            && self.items.len() > 1
        {
            self.open_index = Some((idx + 1) % self.items.len());
            self.zoom = 1.0;
            self.rotation_deg = 0;
            cx.notify();
        }
    }

    /// Steps to the previous item (wrapping around).
    pub fn prev(&mut self, cx: &mut Context<Self>) {
        if let Some(idx) = self.open_index
            && self.items.len() > 1
        {
            self.open_index = Some((idx + self.items.len() - 1) % self.items.len());
            self.zoom = 1.0;
            self.rotation_deg = 0;
            cx.notify();
        }
    }

    /// Adjusts zoom by `delta` clamped to `0.5..=3.0`.
    pub fn adjust_zoom(&mut self, delta: f32, cx: &mut Context<Self>) {
        if !delta.is_finite() {
            return;
        }
        let next = (self.zoom + delta).clamp(0.5, 3.0);
        if (next - self.zoom).abs() > f32::EPSILON {
            self.zoom = next;
            cx.notify();
        }
    }

    /// Rotates by `delta_deg` (e.g. `90` or `-90`).
    pub fn rotate(&mut self, delta_deg: i32, cx: &mut Context<Self>) {
        self.rotation_deg = (self.rotation_deg + delta_deg).rem_euclid(360);
        cx.notify();
    }

    /// Resets zoom to `100%` and rotation to `0°`.
    pub fn reset_view(&mut self, cx: &mut Context<Self>) {
        self.zoom = 1.0;
        self.rotation_deg = 0;
        cx.notify();
    }

    fn on_key_down(&mut self, ev: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.open_index.is_none() {
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
            "right" => {
                self.next(cx);
                cx.stop_propagation();
            }
            "left" => {
                self.prev(cx);
                cx.stop_propagation();
            }
            "+" | "=" => {
                self.adjust_zoom(0.25, cx);
                cx.stop_propagation();
            }
            "-" => {
                self.adjust_zoom(-0.25, cx);
                cx.stop_propagation();
            }
            "r" => {
                self.rotate(if mods.shift { -90 } else { 90 }, cx);
                cx.stop_propagation();
            }
            "0" => {
                self.reset_view(cx);
                cx.stop_propagation();
            }
            _ => {}
        }
    }
}

fn render_media(media: &PreviewMedia, window: &mut Window, cx: &mut App) -> AnyElement {
    match media {
        PreviewMedia::Image(src) => img(src.clone())
            .size_full()
            .object_fit(crate::ObjectFit::Contain)
            .into_any_element(),
        PreviewMedia::Custom(f) => f(window, cx),
    }
}

impl Render for ImagePreviewState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();
        let entity_id = cx.entity_id();
        let tw_w = self.thumb_width;
        let tw_h = self.thumb_height;

        let mut thumbs = div()
            .flex()
            .flex_row()
            .flex_wrap()
            .gap(px(10.))
            .w_full();

        if self.items.is_empty() {
            thumbs = thumbs.child(
                div()
                    .px(px(14.))
                    .py(px(18.))
                    .rounded(theme.radius_lg)
                    .border_1()
                    .border_color(theme.border)
                    .text_size(px(12.5))
                    .text_color(theme.muted_foreground)
                    .child("No images to preview."),
            );
        } else {
            for (idx, item) in self.items.iter().enumerate() {
                let content = render_media(&item.media, window, cx);
                thumbs = thumbs.child(
                    div()
                        .id(("img-thumb", idx))
                        .group("img-prev-card")
                        .relative()
                        .w(tw_w)
                        .h(tw_h)
                        .rounded(theme.radius_lg)
                        .border_1()
                        .border_color(theme.border)
                        .bg(theme.surface_muted)
                        .overflow_hidden()
                        .cursor_pointer()
                        .hover(|s| s.border_color(theme.primary))
                        .on_click(cx.listener(move |this, _, window, cx| {
                            this.open(idx, window, cx);
                        }))
                        .child(div().size_full().child(content))
                        .child(
                            div()
                                .absolute()
                                .inset_0()
                                .flex()
                                .items_center()
                                .justify_center()
                                .bg(crate::Hsla::from(tw::SLATE_950).opacity(0.0))
                                .group_hover("img-prev-card", |s| {
                                    s.bg(crate::Hsla::from(tw::SLATE_950).opacity(0.42))
                                })
                                .child(
                                    div()
                                        .opacity(0.0)
                                        .group_hover("img-prev-card", |s| s.opacity(1.0))
                                        .flex()
                                        .items_center()
                                        .gap(px(4.))
                                        .px(px(8.))
                                        .py(px(4.))
                                        .rounded_full()
                                        .bg(crate::Hsla::from(tw::SLATE_900).opacity(0.85))
                                        .text_size(px(11.5))
                                        .text_color(tw::WHITE)
                                        .child(Icon::new(LucideIcon::ZoomIn).size(IconSize::Xs))
                                        .child("Preview"),
                                ),
                        ),
                );
            }
        }

        let lightbox = self.open_index.and_then(|idx| {
            let item = self.items.get(idx)?;
            let total = self.items.len();
            let title = item.title.clone();
            let subtitle = item.subtitle.clone();
            let media_el = render_media(&item.media, window, cx);
            let vp = window.viewport_size();
            let zoom_pct = (self.zoom * 100.0).round() as i32;
            let at_min_zoom = self.zoom <= 0.501;
            let at_max_zoom = self.zoom >= 2.999;

            // Size viewport clip box dynamically from window size while letting the zoomed canvas
            // expand smoothly all the way up to 300% inside a scrollable stage.
            let max_stage_w = (vp.width.0 - 156.0).max(200.0);
            let max_stage_h = (vp.height.0 - 176.0).max(160.0);
            let fit_w = 520.0_f32.min(max_stage_w);
            let fit_h = 340.0_f32.min(max_stage_h);
            let zoomed_w = fit_w * self.zoom;
            let zoomed_h = fit_h * self.zoom;
            let (stage_w, stage_h) = if self.rotation_deg % 180 == 90 {
                (px(zoomed_h), px(zoomed_w))
            } else {
                (px(zoomed_w), px(zoomed_h))
            };
            let blur_r = self.backdrop_blur;

            let toolbar_btn = |id: &'static str, icon: LucideIcon, disabled: bool| {
                div()
                    .id((id, entity_id))
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .w(px(32.))
                    .h(px(32.))
                    .rounded_full()
                    .text_color(tw::SLATE_100)
                    .when(!disabled, |el| {
                        el.cursor_pointer()
                            .hover(|s| s.bg(crate::Hsla::from(tw::WHITE).opacity(0.16)))
                    })
                    .when(disabled, |el| el.opacity(0.35).cursor_not_allowed())
                    .child(Icon::new(icon).size(IconSize::Sm))
            };

            let scrim = crate::Hsla::from(tw::SLATE_950).opacity(0.72);
            let overlay = div()
                .id(("img-lightbox-backdrop", entity_id))
                .track_focus(&self.focus_handle)
                .on_key_down(cx.listener(Self::on_key_down))
                .w(vp.width)
                .h(vp.height)
                .bg(scrim)
                .occlude()
                .flex()
                .flex_col()
                .justify_between()
                .p(px(20.))
                // Use mouse-down rather than on_click so Space/Enter keyboard events never dismiss!
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, window, cx| this.close_in(window, cx)),
                )
                // Top caption bar
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(12.))
                        .px(px(14.))
                        .py(px(10.))
                        .rounded(theme.radius_xl)
                        .bg(crate::Hsla::from(tw::SLATE_900).opacity(0.78))
                        .border_1()
                        .border_color(crate::Hsla::from(tw::WHITE).opacity(0.12))
                        .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                            window.focus(&this.focus_handle, cx);
                            cx.stop_propagation();
                        }))
                        .child(
                            div()
                                .flex()
                                .flex_row()
                                .items_center()
                                .gap(px(10.))
                                .flex_1()
                                .min_w_0()
                                .child(
                                    div()
                                        .flex_shrink_0()
                                        .px(px(8.))
                                        .py(px(2.))
                                        .rounded_full()
                                        .bg(crate::Hsla::from(tw::WHITE).opacity(0.14))
                                        .text_size(px(12.))
                                        .font_features(tabular_numerals())
                                        .text_color(tw::SLATE_100)
                                        .child(format!("{} / {total}", idx + 1)),
                                )
                                .child(
                                    div()
                                        .min_w_0()
                                        .truncate()
                                        .text_size(px(14.))
                                        .font_weight(FontWeight::SEMIBOLD)
                                        .text_color(tw::WHITE)
                                        .child(title),
                                )
                                .when_some(subtitle, |el, sub| {
                                    el.child(
                                        div()
                                            .min_w_0()
                                            .truncate()
                                            .text_size(px(12.5))
                                            .text_color(tw::SLATE_300)
                                            .child(sub),
                                    )
                                }),
                        )
                        .child(toolbar_btn("lb-close", LucideIcon::X, false).on_click(
                            cx.listener(|this, _, window, cx| this.close_in(window, cx)),
                        )),
                )
                // Center stage
                .child(
                    div()
                        .flex_1()
                        .min_h_0()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(16.))
                        .child(div().flex_shrink_0().w(px(40.)).when(total > 1, |el| {
                            el.on_any_mouse_down(|_, _, cx| cx.stop_propagation()).child(
                                toolbar_btn("lb-prev", LucideIcon::ChevronLeft, false)
                                    .w(px(40.))
                                    .h(px(40.))
                                    .bg(crate::Hsla::from(tw::SLATE_900).opacity(0.75))
                                    .on_click(cx.listener(|this, _, _, cx| this.prev(cx))),
                            )
                        }))
                        .child(
                            div()
                                .id(("lb-stage-scroll", entity_id))
                                .max_w(px(max_stage_w))
                                .max_h(px(max_stage_h))
                                .overflow_scroll()
                                .rounded(theme.radius_xl)
                                .border_1()
                                .border_color(crate::Hsla::from(tw::WHITE).opacity(0.18))
                                .shadow_2xl()
                                .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                                    window.focus(&this.focus_handle, cx);
                                    cx.stop_propagation();
                                }))
                                .child(
                                    div()
                                        .w(stage_w)
                                        .h(stage_h)
                                        .flex_shrink_0()
                                        .child(media_el),
                                ),
                        )
                        .child(div().flex_shrink_0().w(px(40.)).when(total > 1, |el| {
                            el.on_any_mouse_down(|_, _, cx| cx.stop_propagation()).child(
                                toolbar_btn("lb-next", LucideIcon::ChevronRight, false)
                                    .w(px(40.))
                                    .h(px(40.))
                                    .bg(crate::Hsla::from(tw::SLATE_900).opacity(0.75))
                                    .on_click(cx.listener(|this, _, _, cx| this.next(cx))),
                            )
                        })),
                )
                // Bottom frosted toolbar
                .child(
                    div().flex().justify_center().child(
                        div()
                            .flex()
                            .flex_row()
                            .items_center()
                            .gap(px(6.))
                            .px(px(12.))
                            .py(px(6.))
                            .rounded_full()
                            .bg(crate::Hsla::from(tw::SLATE_900).opacity(0.84))
                            .border_1()
                            .border_color(crate::Hsla::from(tw::WHITE).opacity(0.14))
                            .on_any_mouse_down(cx.listener(|this, _, window, cx| {
                                window.focus(&this.focus_handle, cx);
                                cx.stop_propagation();
                            }))
                            .child(
                                toolbar_btn("lb-zoom-out", LucideIcon::ZoomOut, at_min_zoom).when(
                                    !at_min_zoom,
                                    |el| {
                                        el.on_click(cx.listener(|this, _, _, cx| {
                                            this.adjust_zoom(-0.25, cx)
                                        }))
                                    },
                                ),
                            )
                            .child(
                                div()
                                    .w(px(48.))
                                    .text_center()
                                    .text_size(px(12.))
                                    .font_features(tabular_numerals())
                                    .text_color(tw::SLATE_100)
                                    .child(format!("{zoom_pct}%")),
                            )
                            .child(
                                toolbar_btn("lb-zoom-in", LucideIcon::ZoomIn, at_max_zoom).when(
                                    !at_max_zoom,
                                    |el| {
                                        el.on_click(cx.listener(|this, _, _, cx| {
                                            this.adjust_zoom(0.25, cx)
                                        }))
                                    },
                                ),
                            )
                            .child(
                                div()
                                    .w(px(1.))
                                    .h(px(18.))
                                    .mx(px(4.))
                                    .bg(crate::Hsla::from(tw::WHITE).opacity(0.18)),
                            )
                            .child(toolbar_btn("lb-rot-l", LucideIcon::RotateCcw, false).on_click(
                                cx.listener(|this, _, _, cx| this.rotate(-90, cx)),
                            ))
                            .child(toolbar_btn("lb-rot-r", LucideIcon::RotateCw, false).on_click(
                                cx.listener(|this, _, _, cx| this.rotate(90, cx)),
                            ))
                            .child(
                                div()
                                    .text_size(px(11.5))
                                    .font_features(tabular_numerals())
                                    .text_color(tw::SLATE_300)
                                    .px(px(4.))
                                    .child(format!("{}°", self.rotation_deg)),
                            )
                            .child(
                                toolbar_btn("lb-reset", LucideIcon::RefreshCw, false).on_click(
                                    cx.listener(|this, _, _, cx| this.reset_view(cx)),
                                ),
                            ),
                    ),
                );

            let final_lb = if reduce_motion {
                overlay
                    .when(blur_r > px(0.), |el| el.backdrop_blur(blur_r))
                    .into_any_element()
            } else {
                overlay
                    .with_animation(
                        ("lb-enter", entity_id),
                        Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                        move |el, delta| {
                            el.opacity(delta).when(blur_r > px(0.), |el| {
                                el.backdrop_blur(px(blur_r.0 * delta))
                            })
                        },
                    )
                    .into_any_element()
            };

            Some(
                deferred(
                    anchored()
                        .position(point(px(0.), px(0.)))
                        .child(final_lb),
                )
                .priority(60),
            )
        });

        let mut root = div()
            .flex()
            .flex_col()
            .w_full()
            .child(thumbs)
            .when_some(lightbox, |el, lb| el.child(div().h_0().child(lb)));
        crate::Refineable::refine(root.style(), &self.frame_style);
        root
    }
}

/// Renderable [`ImagePreview`] gallery element bound to [`Entity<ImagePreviewState>`].
#[derive(IntoElement)]
pub struct ImagePreview {
    state: Entity<ImagePreviewState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ImagePreview::new`].
pub fn image_preview(state: &Entity<ImagePreviewState>) -> ImagePreview {
    ImagePreview::new(state)
}

impl ImagePreview {
    /// Creates an [`ImagePreview`] bound to `state`.
    pub fn new(state: &Entity<ImagePreviewState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for ImagePreview {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ImagePreview {
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

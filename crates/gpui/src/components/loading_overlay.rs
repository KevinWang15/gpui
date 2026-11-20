//! GPU-accelerated [`BlurContainer`] and [`LoadingOverlay`] components.
//!
//! [`BlurContainer`] wraps arbitrary child UI and, when active, blurs everything inside it
//! using GPUI's GPU Dual-Kawase [`Styled::backdrop_blur`] pipeline (supported on both WGPU
//! and Windows DirectX 11), blocks pointer interaction underneath, and renders an optional
//! razor-sharp unblurred foreground overlay on top.
//!
//! [`LoadingOverlay`] builds directly on [`BlurContainer`] to provide a one-liner loading
//! wrapper with configurable [`SpinnerVariant`], label, description, optional determinate
//! [`ProgressBar`], optional frosted pill card, and cancel action.

use std::rc::Rc;
use std::time::Duration;

use smallvec::SmallVec;

use crate::{
    Animation, AnimationExt, AnyElement, App, ClickEvent, ElementId, IntoElement, MouseButton,
    ParentElement, Pixels, RenderOnce, SharedString, StyleRefinement, Styled, Window, div,
    ease_out_quint, prelude::*, px,
};
use crate::icons::IconSize;

use super::button::button;
use super::feedback::{ProgressBar, Spinner, SpinnerVariant};
use super::theme::{ANIM_FAST, ActiveTheme, ComponentSize, ThemeMode};

/// Wrapper container that blurs its wrapped children via GPU Dual-Kawase [`Styled::backdrop_blur`]
/// and optionally overlays crisp, unblurred foreground content on top.
///
/// Ideal for loading states ([`LoadingOverlay`]), permission/paywall gates, spoiler/privacy
/// shields ("Click to reveal"), and frosted-glass focus guards.
#[derive(IntoElement)]
pub struct BlurContainer {
    id: Option<ElementId>,
    active: bool,
    blur_radius: Pixels,
    tint: Option<crate::Hsla>,
    scrim_opacity: Option<f32>,
    block_interaction: bool,
    overlay: Option<AnyElement>,
    overlay_padding: Pixels,
    anim_id: Option<ElementId>,
    on_overlay_click: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    children: SmallVec<[AnyElement; 4]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`BlurContainer::new`].
pub fn blur_container() -> BlurContainer {
    BlurContainer::new()
}

impl Default for BlurContainer {
    fn default() -> Self {
        Self::new()
    }
}

impl BlurContainer {
    /// Creates an active [`BlurContainer`] with a default `8px` GPU backdrop blur.
    pub fn new() -> Self {
        Self {
            id: None,
            active: true,
            blur_radius: px(8.),
            tint: None,
            scrim_opacity: None,
            block_interaction: true,
            overlay: None,
            overlay_padding: px(16.),
            anim_id: None,
            on_overlay_click: None,
            children: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Assigns an explicit [`ElementId`] to disambiguate sibling [`BlurContainer`] instances.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Enables or disables the blur and overlay layer (default `true`).
    pub fn active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    /// Sets the GPU backdrop blur radius in logical pixels (default `8px`; `0px` for dim-only).
    pub fn blur(mut self, radius: impl Into<Pixels>) -> Self {
        let r: Pixels = radius.into();
        self.blur_radius = if r.0.is_finite() { r.max(px(0.)) } else { px(8.) };
        self
    }

    /// Overrides the translucent scrim tint painted over the blurred backdrop.
    pub fn tint(mut self, color: impl Into<crate::Hsla>) -> Self {
        self.tint = Some(color.into());
        self
    }

    /// Adjusts the default theme-aware frosted scrim opacity (`0.0..=1.0`).
    pub fn scrim_opacity(mut self, opacity: f32) -> Self {
        if opacity.is_finite() {
            self.scrim_opacity = Some(opacity.clamp(0.0, 1.0));
        }
        self
    }

    /// Controls whether pointer clicks and scroll wheel events are intercepted while blurred
    /// (default `true`).
    pub fn block_interaction(mut self, block: bool) -> Self {
        self.block_interaction = block;
        self
    }

    /// Sets a crisp, unblurred foreground element centered on top of the blurred content.
    pub fn overlay(mut self, element: impl IntoElement) -> Self {
        self.overlay = Some(element.into_any_element());
        self
    }

    /// Sets inner padding around the centered overlay content (default `16px`).
    pub fn overlay_padding(mut self, padding: Pixels) -> Self {
        self.overlay_padding = padding;
        self
    }

    /// Enables a smooth entrance fade & blur ramp keyed by `id`.
    pub fn animated(mut self, id: impl Into<ElementId>) -> Self {
        self.anim_id = Some(id.into());
        self
    }

    /// Registers a click handler on the blur scrim (e.g. click-to-reveal privacy shield).
    pub fn on_overlay_click(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_overlay_click = Some(Rc::new(handler));
        self
    }
}

impl ParentElement for BlurContainer {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for BlurContainer {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for BlurContainer {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();

        let default_tint = match theme.mode {
            ThemeMode::Light => theme.surface.opacity(self.scrim_opacity.unwrap_or(0.42)),
            ThemeMode::Dark => theme.background.opacity(self.scrim_opacity.unwrap_or(0.46)),
        };
        let scrim_color = self.tint.unwrap_or(default_tint);
        let blur_radius = self.blur_radius;
        let corner_radii = self.style.corner_radii.clone();

        let mut root = div()
            .relative()
            .overflow_hidden()
            .children(self.children);

        if self.active {
            let has_overlay_click = self.on_overlay_click.is_some();
            let click_id = self
                .id
                .clone()
                .or_else(|| self.anim_id.clone())
                .unwrap_or_else(|| ElementId::Name("blur-scrim-click".into()));
            let mut layer = div()
                .id((click_id, "scrim"))
                .absolute()
                .inset_0()
                .bg(scrim_color)
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .p(self.overlay_padding)
                .when(self.block_interaction, |el| {
                    // Clicks and hover under the scrim are blocked. The wheel
                    // isn't: what's covered isn't scrolled (it's not under the
                    // pointer), and the containers around the overlay still
                    // are (Window::with_scroll_container).
                    el.occlude()
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .on_mouse_down(MouseButton::Right, |_, _, cx| cx.stop_propagation())
                })
                .when_some(self.on_overlay_click, |el, cb| {
                    el.cursor_pointer()
                        .on_click(move |ev, window, cx| cb(ev, window, cx))
                })
                .when_some(self.overlay, |el, overlay| {
                    el.child(
                        div()
                            .flex()
                            .flex_col()
                            .items_center()
                            .justify_center()
                            // Allow click-to-reveal overlays to receive clicks on centered prompt too.
                            .when(!has_overlay_click, |el| {
                                el.on_mouse_down(MouseButton::Left, |_, _, cx| {
                                    cx.stop_propagation()
                                })
                            })
                            .child(overlay),
                    )
                });

            // Inherit container corner radii so the GPU blur SDF clips cleanly to rounded cards.
            layer.style().corner_radii = corner_radii;

            if let Some(anim_id) = self.anim_id
                && !reduce_motion
            {
                let animated_layer = layer.with_animation(
                    (anim_id, "blur-enter"),
                    Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                    move |el, delta| {
                        el.opacity(delta).when(blur_radius > px(0.), |el| {
                            el.backdrop_blur(px(blur_radius.0 * delta))
                        })
                    },
                );
                root = root.child(animated_layer);
            } else {
                layer = layer.when(blur_radius > px(0.), |el| el.backdrop_blur(blur_radius));
                root = root.child(layer);
            }
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Async loading overlay that blurs its wrapped content and centers a crisp [`Spinner`]
/// (plus optional status label, description, determinate [`ProgressBar`], pill card, and
/// Cancel button) on top.
///
/// Built on [`BlurContainer`] so it automatically uses GPU Dual-Kawase backdrop blur on
/// both WGPU and Windows DirectX 11 and blocks clicks underneath while `loading == true`.
#[derive(IntoElement)]
pub struct LoadingOverlay {
    id: Option<ElementId>,
    loading: bool,
    blur_radius: Pixels,
    tint: Option<crate::Hsla>,
    scrim_opacity: Option<f32>,
    block_interaction: bool,
    spinner_variant: SpinnerVariant,
    spinner_size: IconSize,
    spinner_color: Option<crate::Hsla>,
    spinner_speed: Option<Duration>,
    label: Option<SharedString>,
    description: Option<SharedString>,
    progress: Option<(f32, f32)>,
    card: bool,
    cancel_label: SharedString,
    on_cancel: Option<Rc<dyn Fn(&ClickEvent, &mut Window, &mut App) + 'static>>,
    custom_indicator: Option<AnyElement>,
    anim_id: Option<ElementId>,
    children: SmallVec<[AnyElement; 4]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`LoadingOverlay::new`].
pub fn loading_overlay(loading: bool) -> LoadingOverlay {
    LoadingOverlay::new(loading)
}

impl LoadingOverlay {
    /// Creates a [`LoadingOverlay`] wrapping children; active only when `loading` is `true`.
    pub fn new(loading: bool) -> Self {
        Self {
            id: None,
            loading,
            blur_radius: px(6.),
            tint: None,
            scrim_opacity: None,
            block_interaction: true,
            spinner_variant: SpinnerVariant::Ring,
            spinner_size: IconSize::Xl,
            spinner_color: None,
            spinner_speed: None,
            label: None,
            description: None,
            progress: None,
            card: false,
            cancel_label: "Cancel".into(),
            on_cancel: None,
            custom_indicator: None,
            anim_id: None,
            children: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Assigns an explicit [`ElementId`] to disambiguate sibling [`LoadingOverlay`] instances.
    pub fn id(mut self, id: impl Into<ElementId>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Sets the GPU backdrop blur radius in pixels (default `6px`).
    pub fn blur(mut self, radius: impl Into<Pixels>) -> Self {
        let r: Pixels = radius.into();
        self.blur_radius = if r.0.is_finite() { r.max(px(0.)) } else { px(6.) };
        self
    }

    /// Overrides the backdrop scrim tint color.
    pub fn tint(mut self, color: impl Into<crate::Hsla>) -> Self {
        self.tint = Some(color.into());
        self
    }

    /// Sets default frosted scrim opacity (`0.0..=1.0`).
    pub fn scrim_opacity(mut self, opacity: f32) -> Self {
        if opacity.is_finite() {
            self.scrim_opacity = Some(opacity.clamp(0.0, 1.0));
        }
        self
    }

    /// Controls whether clicks & scroll are blocked while loading (default `true`).
    pub fn block_interaction(mut self, block: bool) -> Self {
        self.block_interaction = block;
        self
    }

    /// Selects the [`SpinnerVariant`] displayed in the overlay.
    pub fn spinner_variant(mut self, variant: SpinnerVariant) -> Self {
        self.spinner_variant = variant;
        self
    }

    /// Sets the spinner icon/graphic size (default `IconSize::Xl`).
    pub fn spinner_size(mut self, size: impl Into<IconSize>) -> Self {
        self.spinner_size = size.into();
        self
    }

    /// Overrides the spinner color (defaults to `theme.primary`).
    pub fn spinner_color(mut self, color: impl Into<crate::Hsla>) -> Self {
        self.spinner_color = Some(color.into());
        self
    }

    /// Overrides the spinner cycle period.
    pub fn spinner_speed(mut self, period: Duration) -> Self {
        self.spinner_speed = Some(period);
        self
    }

    /// Sets the primary loading status text (e.g. `"Loading analytics..."`).
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets a muted secondary line below the label.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Displays a determinate [`ProgressBar`] (`value` in `0.0..=100.0`) below the spinner.
    pub fn progress(mut self, value: f32) -> Self {
        let safe = if value.is_finite() {
            value.clamp(0.0, 100.0)
        } else {
            0.0
        };
        self.progress = Some((safe, 100.0));
        self
    }

    /// Displays a determinate [`ProgressBar`] with an explicit maximum value.
    pub fn progress_of(mut self, value: f32, max: f32) -> Self {
        let safe_max = if max.is_finite() && max > 0.0 {
            max
        } else {
            100.0
        };
        let safe_val = if value.is_finite() {
            value.clamp(0.0, safe_max)
        } else {
            0.0
        };
        self.progress = Some((safe_val, safe_max));
        self
    }

    /// Wraps the spinner and text in an elevated surface pill card for extra legibility
    /// over high-contrast charts or imagery.
    pub fn card(mut self, enabled: bool) -> Self {
        self.card = enabled;
        self
    }

    /// Adds a Cancel button inside the overlay and registers its click handler.
    pub fn on_cancel(
        mut self,
        handler: impl Fn(&ClickEvent, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_cancel = Some(Rc::new(handler));
        self
    }

    /// Customizes the Cancel button label (default `"Cancel"`).
    pub fn cancel_label(mut self, label: impl Into<SharedString>) -> Self {
        self.cancel_label = label.into();
        self
    }

    /// Replaces the default [`Spinner`] with a custom foreground indicator element
    /// (for example a [`super::ProgressRing`]).
    pub fn custom_indicator(mut self, indicator: impl IntoElement) -> Self {
        self.custom_indicator = Some(indicator.into_any_element());
        self
    }

    /// Enables smooth entrance animation keyed by `id`.
    pub fn animated(mut self, id: impl Into<ElementId>) -> Self {
        self.anim_id = Some(id.into());
        self
    }
}

impl ParentElement for LoadingOverlay {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for LoadingOverlay {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LoadingOverlay {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();

        let mut container = BlurContainer::new()
            .active(self.loading)
            .blur(self.blur_radius)
            .block_interaction(self.block_interaction);

        if let Some(tint) = self.tint {
            container = container.tint(tint);
        }
        if let Some(opacity) = self.scrim_opacity {
            container = container.scrim_opacity(opacity);
        }
        if let Some(ref id) = self.id {
            container = container.id(id.clone());
        }
        if let Some(ref anim_id) = self.anim_id {
            container = container.animated(anim_id.clone());
        }

        if self.loading {
            let label = self.label;
            let description = self.description;
            let indicator: AnyElement = if let Some(custom) = self.custom_indicator {
                let has_text = label.is_some() || description.is_some();
                div()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .gap(px(10.))
                    .child(custom)
                    .when(has_text, |el| {
                        el.child(
                            div()
                                .flex()
                                .flex_col()
                                .items_center()
                                .gap(px(2.))
                                .when_some(label, |el, text| {
                                    el.child(
                                        div()
                                            .text_size(px(13.5))
                                            .font_weight(crate::FontWeight::MEDIUM)
                                            .text_color(theme.foreground)
                                            .text_center()
                                            .child(text),
                                    )
                                })
                                .when_some(description, |el, desc| {
                                    el.child(
                                        div()
                                            .text_size(px(12.))
                                            .text_color(theme.muted_foreground)
                                            .text_center()
                                            .child(desc),
                                    )
                                }),
                        )
                    })
                    .into_any_element()
            } else {
                let mut sp = Spinner::new()
                    .variant(self.spinner_variant)
                    .size(self.spinner_size)
                    .vertical();
                if let Some(color) = self.spinner_color {
                    sp = sp.color(color);
                }
                if let Some(speed) = self.spinner_speed {
                    sp = sp.speed(speed);
                }
                if let Some(label) = label {
                    sp = sp.label(label);
                }
                if let Some(desc) = description {
                    sp = sp.description(desc);
                }
                sp.into_any_element()
            };

            let cancel_btn = self.on_cancel.map(|cb| {
                let btn_id = self
                    .id
                    .clone()
                    .or_else(|| self.anim_id.clone())
                    .unwrap_or_else(|| ElementId::Name("loading-overlay".into()));
                button((btn_id, "cancel"))
                    .secondary()
                    .size(ComponentSize::Xs)
                    .label(self.cancel_label)
                    .on_click(move |ev, window, cx| cb(ev, window, cx))
            });

            let mut stack = div()
                .flex()
                .flex_col()
                .items_center()
                .justify_center()
                .gap(px(10.))
                .child(indicator)
                .when_some(self.progress, |el, (val, max)| {
                    el.child(
                        div()
                            .w(px(180.))
                            .child(
                                ProgressBar::new(val)
                                    .max(max)
                                    .size(ComponentSize::Sm)
                                    .show_percentage(true),
                            ),
                    )
                })
                .children(cancel_btn);

            if self.card {
                stack = stack
                    .px(px(22.))
                    .py(px(18.))
                    .rounded(theme.radius_xl)
                    .bg(theme.popover.opacity(0.92))
                    .border_1()
                    .border_color(theme.border)
                    .when(theme.elevation_shadows, |el| el.shadow_lg());
            }

            container = container.overlay(stack);
        }

        crate::Refineable::refine(container.style(), &self.style);
        container.children(self.children)
    }
}

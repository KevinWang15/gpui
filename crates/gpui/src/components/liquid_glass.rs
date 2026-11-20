//! Composable optical [`LiquidGlass`] container (`ParentElement + Styled`) with real GPU
//! Snell refraction, RGB chromatic edge dispersion, analytical 3D Fresnel specular rim &
//! live cursor sheen, continuous [`ProgressiveBlur`], and optional smooth pointer dragging
//! via [`LiquidGlassDragState`] + [`LiquidGlassStage`].
//!
//! Because [`LiquidGlass`] is a pure composable container, you can place **any** GPUI elements
//! inside it — buttons, segmented controls, theme switchers, search inputs, badges, icons, or
//! just a clean magnifying lens overlay — and everything inside remains 100% interactive and
//! razor-sharp while live text, charts, and cards behind it bend organically through the glass.

use std::cell::Cell;
use std::rc::Rc;

use smallvec::SmallVec;

use crate::{
    AbsoluteLength, AnyElement, App, Bounds, Context, Corners, CursorStyle, DispatchPhase,
    ElementId, Entity, EventEmitter, Hsla, IntoElement, LiquidGlassOptions, MouseButton,
    MouseDownEvent, MouseMoveEvent, MouseUpEvent, ParentElement, Pixels, Point, ProgressiveBlur,
    RenderOnce, Size, Style, StyleRefinement, Styled, Window, canvas, div, point, prelude::*, px,
};

use super::theme::{ActiveTheme, ThemeMode};

/// Events emitted by [`LiquidGlassDragState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum LiquidGlassDragEvent {
    /// Emitted whenever the floating glass element moves to a new `(x, y)` offset within its stage.
    Move(Point<Pixels>),
}

/// Composable drag-position entity that makes any [`LiquidGlass`] container smoothly draggable
/// across a [`LiquidGlassStage`] (or any `relative()` parent container).
pub struct LiquidGlassDragState {
    position: Point<Pixels>,
    dragging: bool,
    drag_start_mouse: Point<Pixels>,
    drag_start_pos: Point<Pixels>,
    has_moved: bool,
    disabled: bool,
    stage_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    element_size: Rc<Cell<Size<Pixels>>>,
}

impl EventEmitter<LiquidGlassDragEvent> for LiquidGlassDragState {}

impl LiquidGlassDragState {
    /// Creates a new [`LiquidGlassDragState`] initialized at `initial_position` (relative to its
    /// parent container's top-left corner).
    pub fn new(initial_position: Point<Pixels>, _cx: &mut Context<Self>) -> Self {
        let safe_pos = point(
            if initial_position.x.0.is_finite() {
                initial_position.x
            } else {
                px(0.)
            },
            if initial_position.y.0.is_finite() {
                initial_position.y
            } else {
                px(0.)
            },
        );
        Self {
            position: safe_pos,
            dragging: false,
            drag_start_mouse: point(px(0.), px(0.)),
            drag_start_pos: safe_pos,
            has_moved: false,
            disabled: false,
            stage_bounds: Rc::new(Cell::new(None)),
            element_size: Rc::new(Cell::new(Size::default())),
        }
    }

    /// Disables or enables dragging.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        if disabled {
            self.dragging = false;
        }
        self
    }

    /// Dynamically updates whether dragging is disabled.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.dragging = false;
            }
            cx.notify();
        }
    }

    /// Returns the current `(x, y)` offset relative to the parent container.
    pub fn position(&self) -> Point<Pixels> {
        self.position
    }

    /// Returns whether the user is currently dragging this floating glass container.
    pub fn is_dragging(&self) -> bool {
        self.dragging
    }

    /// Programmatically moves the floating glass container (clamped inside stage bounds if known).
    pub fn set_position(&mut self, pos: Point<Pixels>, cx: &mut Context<Self>) {
        if !pos.x.0.is_finite() || !pos.y.0.is_finite() {
            return;
        }
        let clamped = self.clamp_position(pos);
        if (clamped.x.0 - self.position.x.0).abs() > 0.25
            || (clamped.y.0 - self.position.y.0).abs() > 0.25
        {
            self.position = clamped;
            cx.emit(LiquidGlassDragEvent::Move(clamped));
            cx.notify();
        }
    }

    fn clamp_position(&self, pos: Point<Pixels>) -> Point<Pixels> {
        let Some(stage) = self.stage_bounds.get() else {
            return point(pos.x.max(px(0.)), pos.y.max(px(0.)));
        };
        let elem = self.element_size.get();
        let max_x = (stage.size.width - elem.width).max(px(0.));
        let max_y = (stage.size.height - elem.height).max(px(0.));
        point(pos.x.clamp(px(0.), max_x), pos.y.clamp(px(0.), max_y))
    }

    fn start_drag(&mut self, mouse: Point<Pixels>, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        self.dragging = true;
        self.has_moved = false;
        self.drag_start_mouse = mouse;
        self.drag_start_pos = self.position;
        cx.notify();
    }

    fn move_drag(&mut self, mouse: Point<Pixels>, cx: &mut Context<Self>) {
        if !self.dragging || self.disabled {
            return;
        }
        let dx = mouse.x - self.drag_start_mouse.x;
        let dy = mouse.y - self.drag_start_mouse.y;
        if !self.has_moved && dx.0.hypot(dy.0) < 2.5 {
            return;
        }
        self.has_moved = true;
        let next = point(self.drag_start_pos.x + dx, self.drag_start_pos.y + dy);
        self.set_position(next, cx);
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.dragging {
            self.dragging = false;
            cx.notify();
        }
    }

    pub(crate) fn stage_bounds_cell(&self) -> Rc<Cell<Option<Bounds<Pixels>>>> {
        self.stage_bounds.clone()
    }
}

/// Composable `relative().overflow_hidden()` stage container that automatically measures its
/// live bounds and supplies clamping geometry to any attached [`LiquidGlassDragState`]s.
///
/// Put any normal background UI (text, charts, tables, cards) and one or more draggable
/// [`LiquidGlass`] overlays inside as regular children.
#[derive(IntoElement)]
pub struct LiquidGlassStage {
    id: ElementId,
    tracked: SmallVec<[Entity<LiquidGlassDragState>; 3]>,
    children: SmallVec<[AnyElement; 4]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`LiquidGlassStage::new`].
pub fn liquid_glass_stage(id: impl Into<ElementId>) -> LiquidGlassStage {
    LiquidGlassStage::new(id)
}

impl LiquidGlassStage {
    /// Creates a new [`LiquidGlassStage`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            tracked: SmallVec::new(),
            children: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Registers a [`LiquidGlassDragState`] so its drag bounds clamp responsively to this stage.
    pub fn track(mut self, state: &Entity<LiquidGlassDragState>) -> Self {
        self.tracked.push(state.clone());
        self
    }
}

impl ParentElement for LiquidGlassStage {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for LiquidGlassStage {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LiquidGlassStage {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let cells: SmallVec<[Rc<Cell<Option<Bounds<Pixels>>>>; 3]> = self
            .tracked
            .iter()
            .map(|st| st.read(cx).stage_bounds_cell())
            .collect();

        let mut root = div()
            .id(self.id)
            .relative()
            .overflow_hidden()
            .child(
                canvas(
                    move |bounds, _, _| {
                        for cell in &cells {
                            cell.set(Some(bounds));
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .inset_0(),
            )
            .children(self.children);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

/// Composable optical liquid-glass container (`ParentElement + Styled`).
///
/// Wraps any child components in a real GPU-refracted optical glass surface featuring:
/// - **Snell's-law refraction** driven by an analytical 3D rounded-box bevel & lens dome
/// - **RGB chromatic edge dispersion** along the curved glass rim
/// - **Fresnel specular rim & live cursor sheen** that slides around curved corners as the pointer moves
/// - **Continuous [`ProgressiveBlur`]** blending seamlessly from crisp full-resolution scene to Dual-Kawase frost
/// - **Optional `.draggable(&state)`** turning any glass container (lens, floating toolbar, inspector, badge) into a smoothly draggable floating layer
#[derive(IntoElement)]
pub struct LiquidGlass {
    id: ElementId,
    blur_radius: Pixels,
    refraction: f32,
    dispersion: f32,
    bevel_width: Pixels,
    specular: f32,
    progressive_blur: ProgressiveBlur,
    tint: Option<Hsla>,
    interactive_sheen: bool,
    elevation_shadow: bool,
    drag_state: Option<Entity<LiquidGlassDragState>>,
    children: SmallVec<[AnyElement; 4]>,
    style: StyleRefinement,
}

/// Convenience constructor for [`LiquidGlass::new`].
pub fn liquid_glass(id: impl Into<ElementId>) -> LiquidGlass {
    LiquidGlass::new(id)
}

impl LiquidGlass {
    /// Creates a new composable [`LiquidGlass`] container with balanced optical defaults.
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            blur_radius: px(4.0),
            refraction: 0.90,
            dispersion: 0.55,
            bevel_width: px(16.0),
            specular: 0.85,
            progressive_blur: ProgressiveBlur::None,
            tint: None,
            interactive_sheen: true,
            elevation_shadow: true,
            drag_state: None,
            children: SmallVec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Configures a crystal-clear optical magnifying lens preset (`0px` frost, strong Snell bending
    /// and rainbow prism edge dispersion). Still accepts any children and style overrides.
    pub fn lens(mut self) -> Self {
        self.blur_radius = px(0.0);
        self.refraction = 1.20;
        self.dispersion = 0.78;
        self.bevel_width = px(22.0);
        self.specular = 1.0;
        self.tint = Some(Hsla::transparent_black());
        self
    }

    /// Configures a floating pill-shaped Liquid Glass toolbar / dock preset with soft frost,
    /// tactile optical rim bending, and room for buttons or controls inside.
    pub fn floating_bar(mut self) -> Self {
        self.blur_radius = px(6.0);
        self.refraction = 0.95;
        self.dispersion = 0.58;
        self.bevel_width = px(16.0);
        self.specular = 0.90;
        self.style.corner_radii = crate::CornersRefinement {
            top_left: Some(AbsoluteLength::Pixels(px(9999.))),
            top_right: Some(AbsoluteLength::Pixels(px(9999.))),
            bottom_right: Some(AbsoluteLength::Pixels(px(9999.))),
            bottom_left: Some(AbsoluteLength::Pixels(px(9999.))),
        };
        self
    }

    /// Configures a continuous progressive-blur header/panel preset that smoothly transitions
    /// from frosted glass to crystal-clear content with zero banding seams.
    pub fn progressive_panel(mut self, direction: ProgressiveBlur) -> Self {
        self.blur_radius = px(18.0);
        self.refraction = 0.35;
        self.dispersion = 0.22;
        self.bevel_width = px(12.0);
        self.specular = 0.45;
        self.progressive_blur = direction;
        self.elevation_shadow = false;
        self
    }

    /// Sets the Dual-Kawase frost radius in logical pixels (`px(0.)` = crystal-clear optical lens).
    pub fn blur(mut self, radius: impl Into<Pixels>) -> Self {
        let r = radius.into();
        self.blur_radius = if r.0.is_finite() { r.max(px(0.)) } else { px(0.) };
        self
    }

    /// Sets Snell's-law optical refraction strength (`0.0` disables bending; `0.8..=1.4` is glassy).
    pub fn refraction(mut self, strength: f32) -> Self {
        self.refraction = if strength.is_finite() {
            strength.clamp(0.0, 2.0)
        } else {
            0.0
        };
        self
    }

    /// Sets chromatic RGB prism dispersion along the curved bevel (`0.0..=1.5`).
    pub fn dispersion(mut self, amount: f32) -> Self {
        self.dispersion = if amount.is_finite() {
            amount.clamp(0.0, 2.0)
        } else {
            0.0
        };
        self
    }

    /// Sets the analytical 3D curved glass bevel thickness in logical pixels (default `16px`).
    pub fn bevel(mut self, width: impl Into<Pixels>) -> Self {
        let w = width.into();
        self.bevel_width = if w.0.is_finite() {
            w.max(px(0.))
        } else {
            px(16.)
        };
        self
    }

    /// Sets Fresnel specular rim & live cursor sheen intensity (`0.0..=1.5`).
    pub fn specular(mut self, intensity: f32) -> Self {
        self.specular = if intensity.is_finite() {
            intensity.clamp(0.0, 2.0)
        } else {
            0.0
        };
        self
    }

    /// Enables a continuous variable-radius [`ProgressiveBlur`] gradient across this glass surface.
    pub fn progressive_blur(mut self, mode: ProgressiveBlur) -> Self {
        self.progressive_blur = mode;
        self
    }

    /// Overrides the subtle glass body tint (defaults to a subtle theme-adaptive veil).
    pub fn tint(mut self, color: impl Into<Hsla>) -> Self {
        self.tint = Some(color.into());
        self
    }

    /// Enables or disables live cursor-tracking specular sheen around the 3D glass bevel (default `true`).
    pub fn interactive_sheen(mut self, enabled: bool) -> Self {
        self.interactive_sheen = enabled;
        self
    }

    /// Enables or disables soft ambient elevation shadow behind the glass slab (default `true`).
    pub fn elevation_shadow(mut self, enabled: bool) -> Self {
        self.elevation_shadow = enabled;
        self
    }

    /// Makes this [`LiquidGlass`] container smoothly draggable at `state.position()`, clamping
    /// inside an enclosing [`LiquidGlassStage`] when tracked.
    pub fn draggable(mut self, state: &Entity<LiquidGlassDragState>) -> Self {
        self.drag_state = Some(state.clone());
        self
    }
}

impl ParentElement for LiquidGlass {
    fn extend(&mut self, elements: impl IntoIterator<Item = AnyElement>) {
        self.children.extend(elements);
    }
}

impl Styled for LiquidGlass {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for LiquidGlass {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let reduce_motion = cx.reduce_motion();

        let default_tint = match theme.mode {
            ThemeMode::Light => Hsla {
                h: 215.0 / 360.0,
                s: 0.35,
                l: 0.99,
                a: 0.14,
            },
            ThemeMode::Dark => Hsla {
                h: 222.0 / 360.0,
                s: 0.25,
                l: 0.16,
                a: 0.20,
            },
        };
        let tint = self.tint.unwrap_or(default_tint);

        // Resolve final corner radii from style refinement (so `.rounded(...)` and `.rounded_full()`
        // automatically drive the GPU optical SDF bevel!).
        let mut resolved_style = Style::default();
        let default_r = AbsoluteLength::Pixels(theme.radius_xl);
        resolved_style.corner_radii = Corners {
            top_left: default_r,
            top_right: default_r,
            bottom_right: default_r,
            bottom_left: default_r,
        };
        crate::Refineable::refine(&mut resolved_style, &self.style);
        let corner_radii_lengths = resolved_style.corner_radii;

        let drag_info = self.drag_state.as_ref().map(|st| {
            let s = st.read(cx);
            (
                st.clone(),
                s.position,
                s.dragging,
                s.disabled,
                s.element_size.clone(),
            )
        });

        let blur_radius = self.blur_radius;
        let refraction = self.refraction;
        let dispersion = self.dispersion;
        let bevel_width = self.bevel_width;
        let specular = self.specular;
        let progressive_blur = self.progressive_blur;
        let interactive_sheen = self.interactive_sheen && !reduce_motion;
        let shadow_opacity = if self.elevation_shadow && theme.elevation_shadows {
            match theme.mode {
                ThemeMode::Light => 0.20,
                ThemeMode::Dark => 0.48,
            }
        } else {
            0.0
        };

        let drag_paint_entity = drag_info.as_ref().map(|(e, _, _, _, _)| e.clone());
        let is_dragging = drag_info.as_ref().is_some_and(|(_, _, d, _, _)| *d);
        let elem_size_cell = drag_info.as_ref().map(|(_, _, _, _, c)| c.clone());

        let glass_canvas = canvas(
            move |bounds, _, _| {
                if let Some(cell) = &elem_size_cell {
                    cell.set(bounds.size);
                }
            },
            move |bounds, _, window, _| {
                let corner_radii = corner_radii_lengths
                    .to_pixels(window.rem_size())
                    .clamp_radii_for_quad_size(bounds.size);
                let pointer_position = interactive_sheen.then(|| window.mouse_position());
                window.paint_liquid_glass(
                    bounds,
                    corner_radii,
                    blur_radius,
                    LiquidGlassOptions {
                        refraction,
                        dispersion,
                        bevel_width,
                        specular,
                        progressive_blur,
                        pointer_position,
                        tint,
                        shadow_opacity,
                    },
                );

                // Refresh live cursor sheen only when pointer moves within falloff halo (~240px)
                // of this glass slab — zero idle repaints when cursor is elsewhere in the window.
                if interactive_sheen && !is_dragging {
                    let halo = px(240.);
                    let expanded = Bounds {
                        origin: point(bounds.origin.x - halo, bounds.origin.y - halo),
                        size: Size {
                            width: bounds.size.width + halo * 2.0,
                            height: bounds.size.height + halo * 2.0,
                        },
                    };
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, window, _| {
                        if phase == DispatchPhase::Bubble && expanded.contains(&ev.position) {
                            window.refresh();
                        }
                    });
                }

                // Window-level Capture drag tracking so fast pointer drags never drop or stick.
                if let Some(drag_ent) = drag_paint_entity
                    && is_dragging
                {
                    window.set_window_cursor_style(CursorStyle::ClosedHand);
                    let ent_move = drag_ent.clone();
                    let ent_up = drag_ent;
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture {
                            if ev.pressed_button != Some(MouseButton::Left) {
                                ent_move.update(cx, |s, cx| s.end_drag(cx));
                            } else {
                                ent_move.update(cx, |s, cx| s.move_drag(ev.position, cx));
                            }
                        }
                    });
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && ev.button == MouseButton::Left {
                            ent_up.update(cx, |s, cx| s.end_drag(cx));
                        }
                    });
                }
            },
        )
        .absolute()
        .inset_0();

        let mut root = div()
            .id(self.id)
            .relative()
            .rounded(theme.radius_xl)
            .when_some(drag_info, |el, (st, pos, dragging, disabled, _)| {
                el.absolute()
                    .left(pos.x)
                    .top(pos.y)
                    .when(!disabled, |el| {
                        el.cursor(if dragging {
                            CursorStyle::ClosedHand
                        } else {
                            CursorStyle::OpenHand
                        })
                        .on_mouse_down(
                            MouseButton::Left,
                            move |ev: &MouseDownEvent, _, cx| {
                                st.update(cx, |s, cx| s.start_drag(ev.position, cx));
                                cx.stop_propagation();
                            },
                        )
                    })
            })
            .child(glass_canvas)
            .children(self.children);

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

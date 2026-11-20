//! Interactive HSVA [`ColorPicker`] / [`ColorPickerState`] with a GPU-gradient 2D Saturation/Value
//! spectrum canvas, Hue rainbow slider, Alpha opacity slider with checkerboard preview, live
//! Hex input (`#RRGGBB` / `#RRGGBBAA`), preset palette swatches, keyboard navigation, and inline
//! or popover modes.

use std::cell::Cell;
use std::rc::Rc;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    Anchor, Animation, AnimationExt, App, BorderStyle, Bounds, Context, Corners, DispatchPhase,
    Edges, Entity, EventEmitter, FocusHandle, Focusable, FontWeight, Hsla, IntoElement,
    KeyDownEvent, MouseButton, MouseDownEvent, MouseMoveEvent, MouseUpEvent, Pixels, Point, Render,
    RenderOnce, Rgba, Size, StyleRefinement, Styled, Window, anchored, canvas, deferred, div,
    ease_out_quint, fill, linear_color_stop, linear_gradient, point, prelude::*, px, quad,
    relative, transparent_black,
};

use super::input::{Input, InputEvent, InputState};
use super::interactive::copy_button;
use super::palette;
use super::theme::{
    ANIM_FAST, ActiveTheme, ComponentSize, ThemeMode, tabular_numerals,
};

/// Events emitted by [`ColorPickerState`].
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ColorPickerEvent {
    /// Emitted whenever the selected [`Hsla`] color changes.
    Change(Hsla),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DragTarget {
    Spectrum,
    Hue,
    Alpha,
}

/// Converts HSVA (`h` in `0..=360`, `s`, `v`, `a` in `0..=1`) into RGB floats + alpha.
pub(crate) fn hsva_to_rgba(h: f32, s: f32, v: f32, a: f32) -> Rgba {
    let h = if h.is_finite() {
        h.rem_euclid(360.0)
    } else {
        0.0
    };
    let s = if s.is_finite() {
        s.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let v = if v.is_finite() {
        v.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let a = if a.is_finite() {
        a.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let c = v * s;
    let x = c * (1.0 - ((h / 60.0) % 2.0 - 1.0).abs());
    let m = v - c;
    let (r1, g1, b1) = if h < 60.0 {
        (c, x, 0.0)
    } else if h < 120.0 {
        (x, c, 0.0)
    } else if h < 180.0 {
        (0.0, c, x)
    } else if h < 240.0 {
        (0.0, x, c)
    } else if h < 300.0 {
        (x, 0.0, c)
    } else {
        (c, 0.0, x)
    };
    Rgba {
        r: (r1 + m).clamp(0.0, 1.0),
        g: (g1 + m).clamp(0.0, 1.0),
        b: (b1 + m).clamp(0.0, 1.0),
        a,
    }
}

/// Converts [`Rgba`] into HSVA (`h` in `0..360`, `s`, `v`, `a` in `0..=1`).
pub(crate) fn rgba_to_hsva(rgba: Rgba) -> (f32, f32, f32, f32) {
    let r = if rgba.r.is_finite() {
        rgba.r.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let g = if rgba.g.is_finite() {
        rgba.g.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let b = if rgba.b.is_finite() {
        rgba.b.clamp(0.0, 1.0)
    } else {
        0.0
    };
    let a = if rgba.a.is_finite() {
        rgba.a.clamp(0.0, 1.0)
    } else {
        1.0
    };
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let h = if delta < 1e-6 {
        0.0
    } else if (max - r).abs() < 1e-6 {
        60.0 * (((g - b) / delta).rem_euclid(6.0))
    } else if (max - g).abs() < 1e-6 {
        60.0 * (((b - r) / delta) + 2.0)
    } else {
        60.0 * (((r - g) / delta) + 4.0)
    };
    let s = if max < 1e-6 { 0.0 } else { delta / max };
    (
        h.rem_euclid(360.0),
        s.clamp(0.0, 1.0),
        max.clamp(0.0, 1.0),
        a,
    )
}

/// Formats an [`Rgba`] color as `#RRGGBB` (when opaque) or `#RRGGBBAA`.
pub fn format_hex(rgba: Rgba, include_alpha: bool) -> String {
    let r = (rgba.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (rgba.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (rgba.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    let a = (rgba.a.clamp(0.0, 1.0) * 255.0).round() as u8;
    if include_alpha && a < 255 {
        format!("#{r:02X}{g:02X}{b:02X}{a:02X}")
    } else {
        format!("#{r:02X}{g:02X}{b:02X}")
    }
}

/// Parses `#RGB`, `#RGBA`, `#RRGGBB`, or `#RRGGBBAA` (with or without leading `#`).
pub fn parse_hex_color(input: &str) -> Option<Rgba> {
    let s = input.trim().trim_start_matches('#');
    if !s.is_ascii() {
        return None;
    }
    match s.len() {
        3 | 4 => {
            let r = u8::from_str_radix(&s[0..1], 16).ok()? * 17;
            let g = u8::from_str_radix(&s[1..2], 16).ok()? * 17;
            let b = u8::from_str_radix(&s[2..3], 16).ok()? * 17;
            let a = if s.len() == 4 {
                u8::from_str_radix(&s[3..4], 16).ok()? * 17
            } else {
                255
            };
            Some(Rgba {
                r: r as f32 / 255.0,
                g: g as f32 / 255.0,
                b: b as f32 / 255.0,
                a: a as f32 / 255.0,
            })
        }
        6 | 8 => {
            let r = u8::from_str_radix(&s[0..2], 16).ok()?;
            let g = u8::from_str_radix(&s[2..4], 16).ok()?;
            let b = u8::from_str_radix(&s[4..6], 16).ok()?;
            let a = if s.len() == 8 {
                u8::from_str_radix(&s[6..8], 16).ok()?
            } else {
                255
            };
            Some(Rgba {
                r: r as f32 / 255.0,
                g: g as f32 / 255.0,
                b: b as f32 / 255.0,
                a: a as f32 / 255.0,
            })
        }
        _ => None,
    }
}

fn default_presets() -> Vec<Hsla> {
    vec![
        palette::SLATE_900.into(),
        palette::SLATE_500.into(),
        palette::RED_500.into(),
        palette::ORANGE_500.into(),
        palette::AMBER_500.into(),
        palette::EMERALD_500.into(),
        palette::TEAL_500.into(),
        palette::SKY_500.into(),
        palette::BLUE_600.into(),
        palette::INDIGO_600.into(),
        palette::VIOLET_600.into(),
        palette::ROSE_500.into(),
    ]
}

fn paint_checkerboard(bounds: Bounds<Pixels>, sq: f32, is_dark: bool, window: &mut Window) {
    let (light, dark): (Hsla, Hsla) = if is_dark {
        (palette::SLATE_700.into(), palette::SLATE_800.into())
    } else {
        (palette::SLATE_100.into(), palette::SLATE_300.into())
    };
    window.paint_quad(fill(bounds, light));
    let cols = (bounds.size.width.0 / sq).ceil().max(0.0) as usize;
    let rows = (bounds.size.height.0 / sq).ceil().max(0.0) as usize;
    for r in 0..rows {
        for c in 0..cols {
            if (r + c) % 2 == 1 {
                let x = bounds.origin.x.0 + c as f32 * sq;
                let y = bounds.origin.y.0 + r as f32 * sq;
                let w = (bounds.origin.x.0 + bounds.size.width.0 - x).min(sq);
                let h = (bounds.origin.y.0 + bounds.size.height.0 - y).min(sq);
                if w > 0.0 && h > 0.0 {
                    window.paint_quad(fill(
                        Bounds {
                            origin: point(px(x), px(y)),
                            size: Size::new(px(w), px(h)),
                        },
                        dark,
                    ));
                }
            }
        }
    }
}

/// Entity state backing an interactive [`ColorPicker`].
pub struct ColorPickerState {
    focus_handle: FocusHandle,
    hue: f32,
    sat: f32,
    val: f32,
    alpha: f32,
    show_alpha: bool,
    inline: bool,
    open: bool,
    disabled: bool,
    presets: Vec<Hsla>,
    hex_input: Entity<InputState>,
    dragging: Option<DragTarget>,
    spectrum_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    hue_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    alpha_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    trigger_bounds: Rc<Cell<Option<Bounds<Pixels>>>>,
    frame_style: StyleRefinement,
}

impl EventEmitter<ColorPickerEvent> for ColorPickerState {}

impl Focusable for ColorPickerState {
    fn focus_handle(&self, _cx: &App) -> FocusHandle {
        self.focus_handle.clone()
    }
}

impl ColorPickerState {
    /// Creates a new [`ColorPickerState`] defaulting to Indigo 600 (`#4F46E5`).
    pub fn new(cx: &mut Context<Self>) -> Self {
        let initial: Hsla = palette::INDIGO_600.into();
        let initial_rgba: Rgba = initial.into();
        let (h, s, v, a) = rgba_to_hsva(initial_rgba);
        let initial_hex = format_hex(initial_rgba, true);

        let hex_input = cx.new(|cx| {
            InputState::new(cx)
                .default_value(initial_hex)
                .placeholder("#4F46E5")
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&hex_input, |this, _, ev: &InputEvent, cx| {
            if this.disabled {
                return;
            }
            match ev {
                InputEvent::Change(txt) => {
                    let clean = txt.trim().trim_start_matches('#');
                    if (clean.len() == 6 || clean.len() == 8)
                        && let Some(rgba) = parse_hex_color(txt)
                    {
                        let (h, s, v, a) = rgba_to_hsva(rgba);
                        this.hue = h;
                        this.sat = s;
                        this.val = v;
                        if this.show_alpha {
                            this.alpha = a;
                        }
                        cx.emit(ColorPickerEvent::Change(this.color()));
                        cx.notify();
                    }
                }
                InputEvent::Submit(_) | InputEvent::Blur => {
                    let raw = this.hex_input.read(cx).value().to_string();
                    if let Some(rgba) = parse_hex_color(&raw) {
                        let normalized = format_hex(rgba, this.show_alpha);
                        if normalized != this.hex() {
                            this.set_color(rgba.into(), cx);
                        } else {
                            this.sync_hex_input(cx);
                        }
                    } else {
                        this.sync_hex_input(cx);
                    }
                }
                InputEvent::Focus | InputEvent::AcceptSuggestion(_) => {}
            }
        })
        .detach();

        Self {
            // The trigger (or the inline panel) is the tab stop.
            focus_handle: cx.focus_handle().tab_stop(true),
            hue: h,
            sat: s,
            val: v,
            alpha: a,
            show_alpha: true,
            inline: false,
            open: false,
            disabled: false,
            presets: default_presets(),
            hex_input,
            dragging: None,
            spectrum_bounds: Rc::new(Cell::new(None)),
            hue_bounds: Rc::new(Cell::new(None)),
            alpha_bounds: Rc::new(Cell::new(None)),
            trigger_bounds: Rc::new(Cell::new(None)),
            frame_style: StyleRefinement::default(),
        }
    }

    /// Sets the initial color without emitting a change event.
    pub fn default_color(mut self, color: impl Into<Hsla>, cx: &mut Context<Self>) -> Self {
        let hsla: Hsla = color.into();
        let rgba: Rgba = hsla.into();
        let (h, s, v, a) = rgba_to_hsva(rgba);
        self.hue = h;
        self.sat = s;
        self.val = v;
        self.alpha = if self.show_alpha { a } else { 1.0 };
        self.sync_hex_input(cx);
        self
    }

    /// Shows or hides the Alpha opacity slider (default `true`).
    pub fn show_alpha(mut self, show: bool) -> Self {
        self.show_alpha = show;
        if !show {
            self.alpha = 1.0;
        }
        self
    }

    /// Renders the full picker inline instead of behind a trigger swatch popover.
    pub fn inline(mut self, inline: bool) -> Self {
        self.inline = inline;
        self
    }

    /// Overrides the quick-pick swatch presets.
    pub fn presets(mut self, presets: impl IntoIterator<Item = impl Into<Hsla>>) -> Self {
        self.presets = presets.into_iter().map(Into::into).collect();
        self
    }

    /// Disables user interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Updates the disabled state dynamically.
    pub fn set_disabled(&mut self, disabled: bool, cx: &mut Context<Self>) {
        if self.disabled != disabled {
            self.disabled = disabled;
            if disabled {
                self.open = false;
                self.dragging = None;
            }
            self.hex_input
                .update(cx, |inp, cx| inp.set_disabled(disabled, cx));
            cx.notify();
        }
    }

    /// Returns whether the popover is currently open.
    pub fn is_open(&self) -> bool {
        self.open
    }

    /// Closes the color picker popover if open.
    pub fn close(&mut self, cx: &mut Context<Self>) {
        if self.open || self.dragging.is_some() {
            self.open = false;
            self.dragging = None;
            cx.notify();
        }
    }

    /// Returns the currently selected color as [`Hsla`].
    pub fn color(&self) -> Hsla {
        self.rgba().into()
    }

    /// Returns the currently selected color as [`Rgba`].
    pub fn rgba(&self) -> Rgba {
        hsva_to_rgba(
            self.hue,
            self.sat,
            self.val,
            if self.show_alpha { self.alpha } else { 1.0 },
        )
    }

    /// Returns the current hex string (`#RRGGBB` or `#RRGGBBAA`).
    pub fn hex(&self) -> String {
        format_hex(self.rgba(), self.show_alpha)
    }

    /// Programmatically sets the color and emits [`ColorPickerEvent::Change`].
    pub fn set_color(&mut self, color: Hsla, cx: &mut Context<Self>) {
        let rgba: Rgba = color.into();
        let (h, s, v, a) = rgba_to_hsva(rgba);
        self.hue = h;
        self.sat = s;
        self.val = v;
        self.alpha = if self.show_alpha { a } else { 1.0 };
        self.sync_hex_input(cx);
        cx.emit(ColorPickerEvent::Change(self.color()));
        cx.notify();
    }

    fn sync_hex_input(&mut self, cx: &mut Context<Self>) {
        let hex = self.hex();
        self.hex_input.update(cx, |inp, cx| {
            if inp.value() != hex {
                inp.set_value_silent(hex, cx);
            }
        });
    }

    fn apply_drag(&mut self, target: DragTarget, pos: Point<Pixels>, cx: &mut Context<Self>) {
        match target {
            DragTarget::Spectrum => {
                if let Some(b) = self.spectrum_bounds.get() {
                    let s = ((pos.x.0 - b.origin.x.0) / b.size.width.0.max(1.0)).clamp(0.0, 1.0);
                    let v =
                        1.0 - ((pos.y.0 - b.origin.y.0) / b.size.height.0.max(1.0)).clamp(0.0, 1.0);
                    self.sat = s;
                    self.val = v;
                }
            }
            DragTarget::Hue => {
                if let Some(b) = self.hue_bounds.get() {
                    let t = ((pos.x.0 - b.origin.x.0) / b.size.width.0.max(1.0)).clamp(0.0, 1.0);
                    self.hue = (t * 360.0).min(359.99);
                }
            }
            DragTarget::Alpha => {
                if let Some(b) = self.alpha_bounds.get() {
                    let t = ((pos.x.0 - b.origin.x.0) / b.size.width.0.max(1.0)).clamp(0.0, 1.0);
                    self.alpha = t;
                }
            }
        }
        self.sync_hex_input(cx);
        cx.emit(ColorPickerEvent::Change(self.color()));
        cx.notify();
    }

    fn on_key_down(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        // As a browser's select: Tab closes the popup and moves on from the
        // trigger (the popup comes last in the tab order).
        if event.keystroke.key == "tab" && !self.inline && self.open {
            self.close(cx);
            window.focus(&self.focus_handle, cx);
            return;
        }
        if self.hex_input.read(cx).focus_handle(cx).is_focused(window) {
            if event.keystroke.key == "escape" && !self.inline && self.open {
                self.close(cx);
                window.focus(&self.focus_handle, cx);
                cx.stop_propagation();
            }
            return;
        }
        let mods = event.keystroke.modifiers;
        if mods.control || mods.platform || mods.alt {
            return;
        }
        match event.keystroke.key.as_str() {
            "escape" if !self.inline && self.open => {
                self.close(cx);
                // Focus may be on a button in the popup.
                window.focus(&self.focus_handle, cx);
                cx.stop_propagation();
            }
            "enter" | "space" if !self.inline => {
                self.open = !self.open;
                if !self.open {
                    self.dragging = None;
                }
                cx.notify();
                cx.stop_propagation();
            }
            "left" | "right" if self.inline || self.open => {
                let delta = if event.keystroke.key == "right" {
                    6.0
                } else {
                    -6.0
                };
                self.hue = (self.hue + delta).rem_euclid(360.0);
                self.sync_hex_input(cx);
                cx.emit(ColorPickerEvent::Change(self.color()));
                cx.notify();
                cx.stop_propagation();
            }
            _ => {}
        }
    }

    fn render_panel(&self, window: &mut Window, cx: &mut Context<Self>) -> crate::Stateful<crate::Div> {
        let theme = cx.theme();
        let is_dark = theme.mode == ThemeMode::Dark;
        let current_rgba = self.rgba();
        let current_hsla: Hsla = current_rgba.into();
        let hue = self.hue;
        let sat = self.sat;
        let val = self.val;
        let alpha = self.alpha;
        let hex = self.hex();
        let disabled = self.disabled;
        let dragging = self.dragging;
        let entity_id = cx.entity_id();
        let entity_for_drag = cx.entity();

        let spec_cell = self.spectrum_bounds.clone();
        let hue_cell = self.hue_bounds.clone();
        let alpha_cell = self.alpha_bounds.clone();

        // 2D Saturation / Value spectrum canvas (2 pixel-smooth GPU gradient quads!)
        let pure_hue: Hsla = hsva_to_rgba(hue, 1.0, 1.0, 1.0).into();
        let spectrum = div()
            .id(("cp-spectrum", entity_id))
            .relative()
            .w_full()
            .h(px(152.))
            .rounded(theme.radius_md)
            .overflow_hidden()
            .border_1()
            .border_color(theme.border)
            .when(!disabled, |el| el.cursor_crosshair())
            .when(disabled, |el| el.cursor_not_allowed())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    if !this.disabled {
                        window.focus(&this.focus_handle, cx);
                        this.dragging = Some(DragTarget::Spectrum);
                        this.apply_drag(DragTarget::Spectrum, ev.position, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(
                canvas(
                    move |bounds, _, _| spec_cell.set(Some(bounds)),
                    move |bounds, _, window, _| {
                        window.paint_quad(quad(
                            bounds,
                            Corners::default(),
                            linear_gradient(
                                90.0,
                                linear_color_stop(Hsla::white(), 0.0),
                                linear_color_stop(pure_hue, 1.0),
                            ),
                            Edges::default(),
                            transparent_black(),
                            BorderStyle::Solid,
                        ));
                        window.paint_quad(quad(
                            bounds,
                            Corners::default(),
                            linear_gradient(
                                180.0,
                                linear_color_stop(transparent_black(), 0.0),
                                linear_color_stop(Hsla::black(), 1.0),
                            ),
                            Edges::default(),
                            transparent_black(),
                            BorderStyle::Solid,
                        ));
                    },
                )
                .size_full(),
            )
            // Dual-contrast draggable crosshair ring (visible on white AND black!)
            .child(
                div()
                    .absolute()
                    .left(relative(sat))
                    .top(relative(1.0 - val))
                    .ml(px(-7.))
                    .mt(px(-7.))
                    .w(px(14.))
                    .h(px(14.))
                    .rounded_full()
                    .border_2()
                    .border_color(palette::WHITE)
                    .bg(current_hsla.opacity(1.0))
                    .shadow_md()
                    .child(
                        div()
                            .size_full()
                            .rounded_full()
                            .border_1()
                            .border_color(Hsla::black().opacity(0.45)),
                    ),
            );

        // Rainbow Hue slider bar (6 pixel-smooth GPU gradient quads + unclipped outer thumb)
        let hue_slider = div()
            .id(("cp-hue", entity_id))
            .relative()
            .w_full()
            .h(px(14.))
            .flex()
            .items_center()
            .when(!disabled, |el| el.cursor_pointer())
            .when(disabled, |el| el.cursor_not_allowed())
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                    if !this.disabled {
                        window.focus(&this.focus_handle, cx);
                        this.dragging = Some(DragTarget::Hue);
                        this.apply_drag(DragTarget::Hue, ev.position, cx);
                        cx.stop_propagation();
                    }
                }),
            )
            .child(
                div()
                    .w_full()
                    .h(px(12.))
                    .rounded_full()
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        canvas(
                            move |bounds, _, _| hue_cell.set(Some(bounds)),
                            move |bounds, _, window, _| {
                                let stops: [Hsla; 7] = [
                                    hsva_to_rgba(0.0, 1.0, 1.0, 1.0).into(),
                                    hsva_to_rgba(60.0, 1.0, 1.0, 1.0).into(),
                                    hsva_to_rgba(120.0, 1.0, 1.0, 1.0).into(),
                                    hsva_to_rgba(180.0, 1.0, 1.0, 1.0).into(),
                                    hsva_to_rgba(240.0, 1.0, 1.0, 1.0).into(),
                                    hsva_to_rgba(300.0, 1.0, 1.0, 1.0).into(),
                                    hsva_to_rgba(0.0, 1.0, 1.0, 1.0).into(),
                                ];
                                let sw = bounds.size.width.0 / 6.0;
                                for i in 0..6usize {
                                    window.paint_quad(quad(
                                        Bounds {
                                            origin: point(
                                                px(bounds.origin.x.0 + i as f32 * sw),
                                                bounds.origin.y,
                                            ),
                                            size: Size::new(px(sw + 0.5), bounds.size.height),
                                        },
                                        Corners::default(),
                                        linear_gradient(
                                            90.0,
                                            linear_color_stop(stops[i], 0.0),
                                            linear_color_stop(stops[i + 1], 1.0),
                                        ),
                                        Edges::default(),
                                        transparent_black(),
                                        BorderStyle::Solid,
                                    ));
                                }
                            },
                        )
                        .size_full(),
                    ),
            )
            .child(
                div()
                    .absolute()
                    .left(relative(hue / 360.0))
                    .top(px(0.))
                    .ml(px(-7.))
                    .w(px(14.))
                    .h(px(14.))
                    .rounded_full()
                    .bg(pure_hue)
                    .border_2()
                    .border_color(palette::WHITE)
                    .shadow_sm()
                    .child(
                        div()
                            .size_full()
                            .rounded_full()
                            .border_1()
                            .border_color(Hsla::black().opacity(0.35)),
                    ),
            );

        // Alpha slider bar with checkerboard backdrop + 1 smooth gradient quad
        let solid_hsla: Hsla = hsva_to_rgba(hue, sat, val, 1.0).into();
        let alpha_slider = self.show_alpha.then(|| {
            div()
                .id(("cp-alpha", entity_id))
                .relative()
                .w_full()
                .h(px(14.))
                .flex()
                .items_center()
                .when(!disabled, |el| el.cursor_pointer())
                .when(disabled, |el| el.cursor_not_allowed())
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, ev: &MouseDownEvent, window, cx| {
                        if !this.disabled {
                            window.focus(&this.focus_handle, cx);
                            this.dragging = Some(DragTarget::Alpha);
                            this.apply_drag(DragTarget::Alpha, ev.position, cx);
                            cx.stop_propagation();
                        }
                    }),
                )
                .child(
                    div()
                        .w_full()
                        .h(px(12.))
                        .rounded_full()
                        .overflow_hidden()
                        .border_1()
                        .border_color(theme.border)
                        .child(
                            canvas(
                                move |bounds, _, _| alpha_cell.set(Some(bounds)),
                                move |bounds, _, window, _| {
                                    paint_checkerboard(bounds, 6.0, is_dark, window);
                                    window.paint_quad(quad(
                                        bounds,
                                        Corners::default(),
                                        linear_gradient(
                                            90.0,
                                            linear_color_stop(solid_hsla.opacity(0.0), 0.0),
                                            linear_color_stop(solid_hsla, 1.0),
                                        ),
                                        Edges::default(),
                                        transparent_black(),
                                        BorderStyle::Solid,
                                    ));
                                },
                            )
                            .size_full(),
                        ),
                )
                .child(
                    div()
                        .absolute()
                        .left(relative(alpha))
                        .top(px(0.))
                        .ml(px(-7.))
                        .w(px(14.))
                        .h(px(14.))
                        .rounded_full()
                        .bg(current_hsla)
                        .border_2()
                        .border_color(palette::WHITE)
                        .shadow_sm()
                        .child(
                            div()
                                .size_full()
                                .rounded_full()
                                .border_1()
                                .border_color(Hsla::black().opacity(0.35)),
                        ),
                )
        });

        let r8 = (current_rgba.r * 255.0).round() as u8;
        let g8 = (current_rgba.g * 255.0).round() as u8;
        let b8 = (current_rgba.b * 255.0).round() as u8;
        let a_pct = (current_rgba.a * 100.0).round() as u8;

        let current_hex = format_hex(self.color().into(), true);
        // The presets are one tab stop (the current color's, else the
        // first); the arrows move along them and Enter or Space picks one.
        let preset_keys = super::focus::choice_focus(
            ("cp-presets", cx.entity_id()),
            self.presets.len(),
            window,
            cx,
        );
        let preset_tab = self
            .presets
            .iter()
            .position(|p| format_hex((*p).into(), true) == current_hex)
            .or(Some(0))
            .filter(|_| !disabled);
        let preset_outlines: Vec<_> = preset_keys
            .iter()
            .map(|h| super::focus::focus_visible_outline(h, window, &theme))
            .collect();
        let presets_row = (!self.presets.is_empty()).then(|| {
            div()
                .on_key_down(super::focus::roving_keys(preset_keys.clone(), None))
                .flex()
                .flex_row()
                .flex_wrap()
                .gap(px(6.))
                .pt(px(2.))
                .children(self.presets.iter().enumerate().map(|(idx, &preset)| {
                    // The stronger border keeps swatches near the surface's
                    // color visible; the current color's swatch gets the ring.
                    let selected = format_hex(preset.into(), true) == current_hex;
                    div()
                        .id(("cp-preset", idx))
                        .track_focus(&super::focus::choice_item_handle(&preset_keys, idx, preset_tab))
                        .when_some(preset_outlines[idx], |el, outline| el.outline(outline))
                        .w(px(20.))
                        .h(px(20.))
                        .rounded(theme.radius_sm)
                        .bg(preset)
                        .border_1()
                        .border_color(theme.input_border_hover)
                        .when(selected, |el| el.border_2().border_color(theme.ring))
                        .when(!disabled, |el| {
                            el.cursor_pointer()
                                .hover(|s| s.border_color(theme.foreground))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.set_color(preset, cx);
                                }))
                        })
                        .when(disabled, |el| el.cursor_not_allowed())
                }))
        });

        // Window-level capture drag tracker active only while `dragging.is_some()` so dragging
        // outside the 268px panel tracks cleanly to 0%/100% and never sticks if released outside.
        let drag_tracker = dragging.map(|target| {
            canvas(
                |_, _, _| {},
                move |_, _, window, _| {
                    let ent_move = entity_for_drag.clone();
                    window.on_mouse_event(move |ev: &MouseMoveEvent, phase, _, cx| {
                        if phase != DispatchPhase::Capture {
                            return;
                        }
                        if ev.pressed_button != Some(MouseButton::Left) {
                            ent_move.update(cx, |this, cx| {
                                this.dragging = None;
                                cx.notify();
                            });
                            return;
                        }
                        ent_move.update(cx, |this, cx| {
                            if this.dragging == Some(target) {
                                this.apply_drag(target, ev.position, cx);
                            }
                        });
                    });
                    let ent_up = entity_for_drag.clone();
                    window.on_mouse_event(move |ev: &MouseUpEvent, phase, _, cx| {
                        if phase == DispatchPhase::Capture && ev.button == MouseButton::Left {
                            ent_up.update(cx, |this, cx| {
                                if this.dragging.take().is_some() {
                                    cx.notify();
                                }
                            });
                        }
                    });
                },
            )
            .absolute()
            .size_0()
        });

        div()
            .id(("cp-panel", entity_id))
            .relative()
            .flex()
            .flex_col()
            .gap(px(10.))
            .w(px(268.))
            .p(px(12.))
            .rounded(theme.radius_xl)
            .bg(theme.popover)
            .text_color(theme.popover_foreground)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_lg())
            .when(disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .on_any_mouse_down(|_, _, cx| cx.stop_propagation())
            .children(drag_tracker)
            .child(spectrum)
            .child(hue_slider)
            .children(alpha_slider)
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(
                        div()
                            .relative()
                            .w(px(30.))
                            .h(px(30.))
                            .flex_shrink_0()
                            .rounded(theme.radius_md)
                            .overflow_hidden()
                            .border_1()
                            .border_color(theme.border)
                            .child(
                                canvas(
                                    |_, _, _| {},
                                    move |bounds, _, window, _| {
                                        paint_checkerboard(bounds, 5.0, is_dark, window);
                                        window.paint_quad(fill(bounds, current_hsla));
                                    },
                                )
                                .size_full(),
                            ),
                    )
                    .child(div().flex_1().min_w_0().child(Input::new(&self.hex_input)))
                    .child(
                        copy_button(("cp-copy-hex", entity_id), hex)
                            .icon_only()
                            .size(ComponentSize::Sm),
                    ),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.5))
                    .font_features(tabular_numerals())
                    .text_color(theme.muted_foreground)
                    .child(format!("RGB({r8}, {g8}, {b8})"))
                    .when(self.show_alpha, |el| el.child(format!("Alpha {a_pct}%"))),
            )
            .children(presets_row)
    }
}

impl Render for ColorPickerState {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let is_dark = theme.mode == ThemeMode::Dark;
        let reduce_motion = cx.reduce_motion();
        let entity_id = cx.entity_id();

        // Keep hex input disabled/value state synchronized cleanly if toggled via builder.
        let desired_hex = self.hex();
        let input_disabled_mismatch = self.hex_input.read(cx).is_disabled() != self.disabled;
        let input_alpha_mismatch = !self.show_alpha && self.hex_input.read(cx).value().len() > 7;
        if input_disabled_mismatch || input_alpha_mismatch {
            let disabled = self.disabled;
            self.hex_input.update(cx, |inp, cx| {
                inp.set_disabled(disabled, cx);
                if input_alpha_mismatch {
                    inp.set_value_silent(desired_hex, cx);
                }
            });
        }

        if self.inline {
            let mut panel = self.render_panel(window, cx).when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
            });
            crate::Refineable::refine(panel.style(), &self.frame_style);
            return panel.into_any_element();
        }

        let current_hsla = self.color();
        let hex = self.hex();
        let hover_bg = theme.surface_hover;
        let is_focused = self.focus_handle.is_focused(window);
        let focus_visible = self.focus_handle.is_focus_visible(window);
        let bounds_cell = self.trigger_bounds.clone();
        let bounds_for_out = self.trigger_bounds.clone();
        let captured_bounds = self.trigger_bounds.get();

        let popover = (self.open && !self.disabled).then(|| {
            let panel = self
                .render_panel(window, cx)
                .on_key_down(cx.listener(Self::on_key_down))
                // A click on the popup's background keeps focus where it was.
                .on_any_mouse_down(|_, window, _| window.prevent_default())
                .on_mouse_down_out(cx.listener(move |this, ev: &MouseDownEvent, _, cx| {
                    if bounds_for_out
                        .get()
                        .is_some_and(|b| b.contains(&ev.position))
                    {
                        return;
                    }
                    if this.open {
                        this.open = false;
                        this.dragging = None;
                        cx.notify();
                    }
                }));
            let animated = if reduce_motion {
                panel.into_any_element()
            } else {
                panel
                    .with_animation(
                        ("cp-pop-anim", entity_id),
                        Animation::new(ANIM_FAST).with_easing(ease_out_quint()),
                        |el, delta| el.opacity(delta),
                    )
                    .into_any_element()
            };

            let mut anchor_el = anchored()
                .anchor(Anchor::TopLeft)
                .snap_to_window_with_margin(px(8.));
            if let Some(bounds) = captured_bounds {
                anchor_el = anchor_el
                    .attach(bounds, Anchor::BottomLeft)
                    .offset(point(px(0.), px(6.)));
            } else {
                anchor_el = anchor_el.offset(point(px(0.), px(6.)));
            }
            deferred(anchor_el.child(animated)).priority(35)
        });

        let mut trigger = div()
            .id(("cp-trigger", entity_id))
            .relative()
            .when(!self.disabled, |el| {
                el.track_focus(&self.focus_handle)
                    .on_key_down(cx.listener(Self::on_key_down))
            })
            .flex()
            .flex_row()
            .items_center()
            .gap(px(8.))
            .h(ComponentSize::Md.height())
            .px(px(10.))
            .rounded(theme.radius_md)
            .border_1()
            .border_color(if self.open || is_focused {
                theme.ring
            } else {
                theme.input_border
            })
            // Dashed outline for keyboard focus only (`focus` module docs).
            .when(focus_visible && !self.disabled, |el| {
                el.when_some(super::focus::focus_outline(&theme), |el, outline| {
                    el.outline(outline)
                })
            })
            .bg(theme.input_bg)
            .text_size(px(13.))
            .font_weight(FontWeight::MEDIUM)
            .font_features(tabular_numerals())
            .text_color(theme.foreground)
            .when(!self.disabled, |el| {
                el.cursor_pointer()
                    .hover(move |s| s.bg(hover_bg))
                    .on_click(cx.listener(|this, _: &crate::ClickEvent, window, cx| {
                        window.focus(&this.focus_handle, cx);
                        this.open = !this.open;
                        if !this.open {
                            this.dragging = None;
                        }
                        cx.notify();
                    }))
            })
            .when(self.disabled, |el| el.opacity(0.55).cursor_not_allowed())
            .child(
                canvas(
                    move |bounds, _, _| bounds_cell.set(Some(bounds)),
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full(),
            )
            .child(
                div()
                    .relative()
                    .w(px(20.))
                    .h(px(20.))
                    .flex_shrink_0()
                    .rounded(theme.radius_sm)
                    .overflow_hidden()
                    .border_1()
                    .border_color(theme.border)
                    .child(
                        canvas(
                            |_, _, _| {},
                            move |bounds, _, window, _| {
                                paint_checkerboard(bounds, 4.0, is_dark, window);
                                window.paint_quad(fill(bounds, current_hsla));
                            },
                        )
                        .size_full(),
                    ),
            )
            .child(hex)
            .child(
                Icon::new(LucideIcon::ChevronDown)
                    .size(IconSize::Sm)
                    .color(theme.muted_foreground),
            );
        crate::Refineable::refine(trigger.style(), &self.frame_style);

        div()
            .flex()
            .flex_col()
            .items_start()
            .child(trigger)
            .when_some(popover, |el, pop| el.child(div().h_0().child(pop)))
            .into_any_element()
    }
}

/// Renderable color picker element wrapping [`Entity<ColorPickerState>`].
#[derive(IntoElement)]
pub struct ColorPicker {
    state: Entity<ColorPickerState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ColorPicker::new`].
pub fn color_picker(state: &Entity<ColorPickerState>) -> ColorPicker {
    ColorPicker::new(state)
}

impl ColorPicker {
    /// Creates a [`ColorPicker`] bound to `state`.
    pub fn new(state: &Entity<ColorPickerState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for ColorPicker {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ColorPicker {
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

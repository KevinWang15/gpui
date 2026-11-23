//! Essential native desktop & studio application controls ported from real-world GPUI apps:
//!
//! - [`StatusIndicator`] — live static or GPU-pulsing status beacon with semantic tone.
//! - [`TrendBadge`] — standalone polarity-aware `%` / `pp` directional trend pill.
//! - [`ConfirmButton`] — inline two-click armed destructive confirmation button with auto-disarm.
//! - [`CompactStepper`] — zero-entity stateless `[Label | − 244 +]` inspector/toolbar stepper.
//! - [`LazyElement`] / [`lazy`] — deferred subtree builder (only constructs children on mount).
//! - [`StringList`] / [`StringListState`] — editable settings string list with inline add & dedup.
//! - [`ColorSwatchPopover`] — fast preset swatch grid with transparent checkerboard chip.
//! - [`FloatingToolbar`] — compact elevated horizontal floating bar with separators.
//! - [`SimulatedBackdrop`] — studio desktop backdrop (`Wallpaper`, `Code`, `Plain`) for previewing
//!   frosted glass opacity and blur.

use std::rc::Rc;
use std::time::Duration;

use crate::icons::{Icon, IconSize, LucideIcon};
use crate::{
    AnyElement, App, ClickEvent, Context, ElementId, Entity, EventEmitter, FontWeight, Hsla,
    IntoElement, MouseButton, Pixels, Render, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, div, linear_color_stop, linear_gradient, prelude::*, px, rgb, rgba,
};

use super::button::{Button, ButtonVariant};
use super::feedback::{Spinner, SpinnerVariant};
use super::input::{Input, InputEvent, InputState};
use super::stat::TrendPolarity;
use super::theme::{ActiveTheme, ComponentSize, tabular_numerals};

// ---------------------------------------------------------------------------
// 1. StatusIndicator (static or pulsing GPU beacon)
// ---------------------------------------------------------------------------

/// Semantic status tone for [`StatusIndicator`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum StatusTone {
    /// Neutral muted indicator.
    #[default]
    Neutral,
    /// Healthy / online (green).
    Success,
    /// Active / syncing (primary / blue).
    Info,
    /// Degraded / caution (amber).
    Warning,
    /// Offline / error (red).
    Destructive,
}

/// Compact status beacon (static dot or GPU-animated pulsing ring) with optional label.
#[derive(IntoElement)]
pub struct StatusIndicator {
    tone: StatusTone,
    label: Option<SharedString>,
    pulsing: bool,
    dot_size: Pixels,
    style: StyleRefinement,
}

/// Convenience constructor for [`StatusIndicator::new`].
pub fn status_indicator(tone: StatusTone) -> StatusIndicator {
    StatusIndicator::new(tone)
}

impl StatusIndicator {
    /// Creates a [`StatusIndicator`] with semantic `tone`.
    pub fn new(tone: StatusTone) -> Self {
        Self {
            tone,
            label: None,
            pulsing: false,
            dot_size: px(8.),
            style: StyleRefinement::default(),
        }
    }

    /// Sets the text label beside the indicator dot.
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Switches between a crisp static dot and a GPU-animated pulsing sync beacon.
    pub fn pulsing(mut self, pulsing: bool) -> Self {
        self.pulsing = pulsing;
        self
    }

    /// Overrides the indicator dot diameter (default `8px`).
    pub fn dot_size(mut self, size: Pixels) -> Self {
        self.dot_size = size;
        self
    }
}

impl Styled for StatusIndicator {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for StatusIndicator {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let color = match self.tone {
            StatusTone::Neutral => theme.muted_foreground,
            StatusTone::Success => theme.success,
            StatusTone::Info => theme.info,
            StatusTone::Warning => theme.warning,
            StatusTone::Destructive => theme.destructive,
        };

        let dot = if self.pulsing {
            Spinner::new()
                .variant(SpinnerVariant::Pulse)
                .size(IconSize::Xs)
                .color(color)
                .into_any_element()
        } else {
            div()
                .w(self.dot_size)
                .h(self.dot_size)
                .flex_shrink_0()
                .rounded_full()
                .bg(color)
                .into_any_element()
        };

        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(6.))
            .child(dot)
            .when_some(self.label, |el, label| {
                el.child(
                    div()
                        .text_size(px(12.))
                        .text_color(theme.muted_foreground)
                        .truncate()
                        .child(label),
                )
            });
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

// ---------------------------------------------------------------------------
// 2. TrendBadge (standalone polarity-aware % / pp directional badge)
// ---------------------------------------------------------------------------

/// Standalone directional trend badge with Lucide arrow, polarity coloring, and tabular figures.
#[derive(IntoElement)]
pub struct TrendBadge {
    delta: f64,
    is_percentage_points: bool,
    polarity: TrendPolarity,
    pill: bool,
    empty_label: Option<SharedString>,
    style: StyleRefinement,
}

/// Creates a percentage-change [`TrendBadge`] (e.g. `delta = 12.4` -> `↑ 12.4%`).
pub fn trend_badge(delta_percent: f64) -> TrendBadge {
    TrendBadge::percent(delta_percent)
}

impl TrendBadge {
    /// Creates a relative percentage change badge (`+12.4%` / `-3.1%`).
    pub fn percent(delta_percent: f64) -> Self {
        Self {
            delta: delta_percent,
            is_percentage_points: false,
            polarity: TrendPolarity::HigherIsBetter,
            pill: true,
            empty_label: None,
            style: StyleRefinement::default(),
        }
    }

    /// Creates a percentage-point (`pp`) change badge (ideal for comparing rates such as cache hit
    /// rate or success rate: `+1.4 pp` / `-0.6 pp`).
    pub fn percentage_points(delta_pp: f64) -> Self {
        Self {
            delta: delta_pp,
            is_percentage_points: true,
            polarity: TrendPolarity::HigherIsBetter,
            pill: true,
            empty_label: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets whether increases are positive (`HigherIsBetter`) or negative (`LowerIsBetter`, e.g.
    /// cost or latency).
    pub fn polarity(mut self, polarity: TrendPolarity) -> Self {
        self.polarity = polarity;
        self
    }

    /// Toggles tinted pill chrome (`true`) vs plain inline trend text (`false`).
    pub fn pill(mut self, pill: bool) -> Self {
        self.pill = pill;
        self
    }

    /// Optional fallback text when `delta` is non-finite (e.g. `"no prior"`).
    pub fn empty_label(mut self, label: impl Into<SharedString>) -> Self {
        self.empty_label = Some(label.into());
        self
    }
}

impl Styled for TrendBadge {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for TrendBadge {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let tnum = tabular_numerals();

        if !self.delta.is_finite() {
            let text = self.empty_label.unwrap_or_else(|| "—".into());
            let mut el = div()
                .flex()
                .items_center()
                .px(px(6.))
                .py(px(2.))
                .rounded_full()
                .when(self.pill, |d| d.bg(theme.surface_muted))
                .text_size(px(11.5))
                .text_color(theme.muted_foreground)
                .child(text);
            crate::Refineable::refine(el.style(), &self.style);
            return el;
        }

        let is_zero = self.delta.abs() < 0.05;
        let is_up = self.delta > 0.0;
        let is_good = match self.polarity {
            TrendPolarity::HigherIsBetter => is_up,
            TrendPolarity::LowerIsBetter => !is_up,
            TrendPolarity::Neutral => true,
        };

        let (fg, bg) = if is_zero || self.polarity == TrendPolarity::Neutral {
            (theme.muted_foreground, theme.surface_muted)
        } else if is_good {
            (theme.success_soft_foreground, theme.success_soft)
        } else {
            (theme.destructive_soft_foreground, theme.destructive_soft)
        };

        let formatted = if self.is_percentage_points {
            format!("{:.1} pp", self.delta.abs())
        } else {
            format!("{:.1}%", self.delta.abs())
        };

        let mut el = div()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(3.))
            .font_features(tnum)
            .text_size(px(11.5))
            .font_weight(FontWeight::SEMIBOLD)
            .text_color(fg)
            .when(self.pill, |d| d.px(px(7.)).py(px(2.)).rounded_full().bg(bg))
            .when(!is_zero, |d| {
                d.child(
                    Icon::new(if is_up {
                        LucideIcon::ArrowUp
                    } else {
                        LucideIcon::ArrowDown
                    })
                    .size(IconSize::Xs)
                    .color(fg),
                )
            })
            .child(formatted);
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

// ---------------------------------------------------------------------------
// 3. ConfirmButton (inline two-click armed destructive button)
// ---------------------------------------------------------------------------

/// Events emitted by [`ConfirmButtonState`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConfirmButtonEvent {
    /// First click armed the confirmation state.
    Armed,
    /// Second click confirmed the destructive action.
    Confirmed,
    /// Timed out or manually disarmed before confirmation.
    Disarmed,
}

/// Entity state backing an inline two-click [`ConfirmButton`].
pub struct ConfirmButtonState {
    label: SharedString,
    confirm_label: SharedString,
    icon: Option<LucideIcon>,
    armed: bool,
    generation: u64,
    disarm_after: Duration,
    size: ComponentSize,
    idle_variant: ButtonVariant,
    disabled: bool,
}

impl EventEmitter<ConfirmButtonEvent> for ConfirmButtonState {}

impl ConfirmButtonState {
    /// Creates a new [`ConfirmButtonState`].
    pub fn new(
        label: impl Into<SharedString>,
        confirm_label: impl Into<SharedString>,
        _cx: &mut Context<Self>,
    ) -> Self {
        Self {
            label: label.into(),
            confirm_label: confirm_label.into(),
            icon: None,
            armed: false,
            generation: 0,
            disarm_after: Duration::from_secs(4),
            size: ComponentSize::Md,
            idle_variant: ButtonVariant::Secondary,
            disabled: false,
        }
    }

    /// Sets an optional leading icon.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Sets the button variant while idle (default [`ButtonVariant::Secondary`]).
    pub fn idle_variant(mut self, variant: ButtonVariant) -> Self {
        self.idle_variant = variant;
        self
    }

    /// Sets the component size preset.
    pub fn size(mut self, size: ComponentSize) -> Self {
        self.size = size;
        self
    }

    /// Sets how long the button stays armed before auto-disarming (default `4s`).
    pub fn disarm_after(mut self, duration: Duration) -> Self {
        self.disarm_after = duration;
        self
    }

    /// Disables interaction.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Returns whether the button is currently armed waiting for the confirming second click.
    pub fn is_armed(&self) -> bool {
        self.armed
    }

    /// Manually resets an armed button back to idle.
    pub fn disarm(&mut self, cx: &mut Context<Self>) {
        if self.armed {
            self.armed = false;
            self.generation = self.generation.wrapping_add(1);
            cx.emit(ConfirmButtonEvent::Disarmed);
            cx.notify();
        }
    }

    fn handle_click(&mut self, cx: &mut Context<Self>) {
        if self.disabled {
            return;
        }
        if !self.armed {
            self.armed = true;
            self.generation = self.generation.wrapping_add(1);
            let gen_id = self.generation;
            let delay = self.disarm_after;
            cx.emit(ConfirmButtonEvent::Armed);
            cx.notify();

            cx.spawn(async move |this, cx| {
                cx.background_executor().timer(delay).await;
                let _ = this.update(cx, |this, cx| {
                    if this.armed && this.generation == gen_id {
                        this.armed = false;
                        cx.emit(ConfirmButtonEvent::Disarmed);
                        cx.notify();
                    }
                });
            })
            .detach();
        } else {
            self.armed = false;
            self.generation = self.generation.wrapping_add(1);
            cx.emit(ConfirmButtonEvent::Confirmed);
            cx.notify();
        }
    }
}

impl Render for ConfirmButtonState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let mut btn = Button::new(("confirm-btn", cx.entity_id()))
            .variant(if self.armed {
                ButtonVariant::Destructive
            } else {
                self.idle_variant
            })
            .size(self.size)
            .disabled(self.disabled)
            .label(if self.armed {
                self.confirm_label.clone()
            } else {
                self.label.clone()
            })
            .on_click(cx.listener(|this, _, _, cx| this.handle_click(cx)));
        if let Some(icon) = if self.armed {
            Some(LucideIcon::TriangleExclamationPoint)
        } else {
            self.icon
        } {
            btn = btn.leading_icon(icon);
        }
        btn
    }
}

/// Inline two-click destructive confirmation button bound to [`ConfirmButtonState`].
#[derive(IntoElement)]
pub struct ConfirmButton {
    state: Entity<ConfirmButtonState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`ConfirmButton::new`].
pub fn confirm_button(state: &Entity<ConfirmButtonState>) -> ConfirmButton {
    ConfirmButton::new(state)
}

impl ConfirmButton {
    /// Creates a [`ConfirmButton`] bound to `state`.
    pub fn new(state: &Entity<ConfirmButtonState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for ConfirmButton {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ConfirmButton {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut wrapper = div().child(self.state);
        crate::Refineable::refine(wrapper.style(), &self.style);
        wrapper
    }
}

// ---------------------------------------------------------------------------
// 4. CompactStepper (zero-entity stateless toolbar/inspector numeric control)
// ---------------------------------------------------------------------------

/// Stateless compact numeric stepper (`[Label | − 244 +]`) with tabular numerals and optional
/// `Shift`-key 10× coarse stepping — ideal for toolbars and property inspectors without creating
/// a dedicated entity per field.
#[derive(IntoElement)]
pub struct CompactStepper {
    id: ElementId,
    label: Option<SharedString>,
    value: f64,
    step: f64,
    shift_multiplier: f64,
    min: f64,
    max: f64,
    suffix: Option<SharedString>,
    precision: usize,
    disabled: bool,
    style: StyleRefinement,
    on_change: Option<Rc<dyn Fn(f64, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`CompactStepper::new`].
pub fn compact_stepper(id: impl Into<ElementId>, value: f64) -> CompactStepper {
    CompactStepper::new(id, value)
}

impl CompactStepper {
    /// Creates a new [`CompactStepper`] displaying `value`.
    pub fn new(id: impl Into<ElementId>, value: f64) -> Self {
        Self {
            id: id.into(),
            label: None,
            value,
            step: 1.0,
            shift_multiplier: 10.0,
            min: f64::NEG_INFINITY,
            max: f64::INFINITY,
            suffix: None,
            precision: 0,
            disabled: false,
            style: StyleRefinement::default(),
            on_change: None,
        }
    }

    /// Sets a compact prefix label (e.g. `"X"`, `"W"`, `"R"`, `"Blur"`).
    pub fn label(mut self, label: impl Into<SharedString>) -> Self {
        self.label = Some(label.into());
        self
    }

    /// Sets the step increment (default `1.0`).
    pub fn step(mut self, step: f64) -> Self {
        self.step = step.max(1e-6);
        self
    }

    /// Sets the `Shift`-click multiplier (default `10.0`).
    pub fn shift_multiplier(mut self, mult: f64) -> Self {
        self.shift_multiplier = mult.max(1.0);
        self
    }

    /// Sets the inclusive `[min, max]` bounds.
    pub fn range(mut self, min: f64, max: f64) -> Self {
        self.min = min;
        self.max = max.max(min);
        self
    }

    /// Sets an optional unit suffix (e.g. `"px"`, `"%"`).
    pub fn suffix(mut self, suffix: impl Into<SharedString>) -> Self {
        self.suffix = Some(suffix.into());
        self
    }

    /// Sets decimal places displayed (default `0`).
    pub fn precision(mut self, precision: usize) -> Self {
        self.precision = precision.min(6);
        self
    }

    /// Disables decrement/increment actions.
    pub fn disabled(mut self, disabled: bool) -> Self {
        self.disabled = disabled;
        self
    }

    /// Registers a change callback invoked with the newly clamped value.
    pub fn on_change(mut self, handler: impl Fn(f64, &mut Window, &mut App) + 'static) -> Self {
        self.on_change = Some(Rc::new(handler));
        self
    }
}

impl Styled for CompactStepper {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for CompactStepper {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let tnum = tabular_numerals();
        let hover_bg = theme.surface_hover;

        let formatted = match &self.suffix {
            Some(suf) => format!("{:.prec$}{suf}", self.value, prec = self.precision),
            None => format!("{:.prec$}", self.value, prec = self.precision),
        };

        let dec_disabled = self.disabled || self.value <= self.min + 1e-9;
        let inc_disabled = self.disabled || self.value >= self.max - 1e-9;

        let on_dec = self.on_change.clone();
        let on_inc = self.on_change;
        let (val, step, mult, min_v, max_v) = (
            self.value,
            self.step,
            self.shift_multiplier,
            self.min,
            self.max,
        );

        let mut el = div()
            .id(self.id.clone())
            .flex()
            .flex_row()
            .items_center()
            .h(px(28.))
            .rounded(theme.radius_md)
            .bg(theme.input_bg)
            .border_1()
            .border_color(theme.input_border)
            .overflow_hidden()
            .when(self.disabled, |d| d.opacity(0.55))
            .when_some(self.label, |d, lbl| {
                d.child(
                    div()
                        .px(px(7.))
                        .h_full()
                        .flex()
                        .items_center()
                        .bg(theme.surface_muted.opacity(0.7))
                        .border_r_1()
                        .border_color(theme.border)
                        .text_size(px(11.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .text_color(theme.muted_foreground)
                        .child(lbl),
                )
            })
            .child(
                div()
                    .id((self.id.clone(), "dec"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(22.))
                    .h_full()
                    .when(!dec_disabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(move |ev: &ClickEvent, window, cx| {
                                if let Some(ref cb) = on_dec {
                                    let delta = if ev.modifiers().shift { step * mult } else { step };
                                    cb((val - delta).clamp(min_v, max_v), window, cx);
                                }
                            })
                    })
                    .when(dec_disabled, |d| d.opacity(0.35).cursor_not_allowed())
                    .child(
                        Icon::new(LucideIcon::Minus)
                            .size(IconSize::Xs)
                            .color(theme.foreground),
                    ),
            )
            .child(
                div()
                    .px(px(6.))
                    .min_w(px(36.))
                    .text_center()
                    .font_features(tnum)
                    .text_size(px(12.))
                    .font_weight(FontWeight::MEDIUM)
                    .text_color(theme.foreground)
                    .child(formatted),
            )
            .child(
                div()
                    .id((self.id, "inc"))
                    .flex()
                    .items_center()
                    .justify_center()
                    .w(px(22.))
                    .h_full()
                    .when(!inc_disabled, |d| {
                        d.cursor_pointer()
                            .hover(move |s| s.bg(hover_bg))
                            .on_click(move |ev: &ClickEvent, window, cx| {
                                if let Some(ref cb) = on_inc {
                                    let delta = if ev.modifiers().shift { step * mult } else { step };
                                    cb((val + delta).clamp(min_v, max_v), window, cx);
                                }
                            })
                    })
                    .when(inc_disabled, |d| d.opacity(0.35).cursor_not_allowed())
                    .child(
                        Icon::new(LucideIcon::Plus)
                            .size(IconSize::Xs)
                            .color(theme.foreground),
                    ),
            );

        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

// ---------------------------------------------------------------------------
// 5. LazyElement (deferred subtree construction on mount)
// ---------------------------------------------------------------------------

/// Defers constructing an expensive element subtree (such as a large collapsed data table inside
/// `ChartCard`, `Accordion`, or `Tabs`) until GPUI actually renders it.
#[derive(IntoElement)]
pub struct LazyElement {
    builder: Box<dyn FnOnce(&mut Window, &mut App) -> AnyElement + 'static>,
}

/// Convenience constructor for [`LazyElement::new`].
pub fn lazy<E: IntoElement + 'static>(
    builder: impl FnOnce(&mut Window, &mut App) -> E + 'static,
) -> LazyElement {
    LazyElement::new(builder)
}

impl LazyElement {
    /// Wraps `builder` so it runs only when this element is rendered.
    pub fn new<E: IntoElement + 'static>(
        builder: impl FnOnce(&mut Window, &mut App) -> E + 'static,
    ) -> Self {
        Self {
            builder: Box::new(move |window, cx| builder(window, cx).into_any_element()),
        }
    }
}

impl RenderOnce for LazyElement {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        (self.builder)(window, cx)
    }
}

// ---------------------------------------------------------------------------
// 6. StringList (editable settings list with inline draft input + dedup)
// ---------------------------------------------------------------------------

/// Events emitted by [`StringListState`].
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StringListEvent {
    /// Emitted whenever items are added or removed.
    Change(Vec<SharedString>),
}

/// Entity state backing an editable [`StringList`] widget (excluded apps, watch folders, globs).
pub struct StringListState {
    items: Vec<SharedString>,
    draft_input: Entity<InputState>,
    add_label: SharedString,
    empty_text: SharedString,
    deduplicate: bool,
    case_insensitive_dedup: bool,
    error: Option<SharedString>,
}

impl EventEmitter<StringListEvent> for StringListState {}

impl StringListState {
    /// Creates a [`StringListState`] with initial `items` and placeholder for the add input.
    pub fn new(
        items: impl IntoIterator<Item = impl Into<SharedString>>,
        placeholder: impl Into<SharedString>,
        cx: &mut Context<Self>,
    ) -> Self {
        let placeholder: SharedString = placeholder.into();
        let draft_input = cx.new(|cx| {
            InputState::new(cx)
                .placeholder(placeholder)
                .size(ComponentSize::Sm)
        });
        cx.subscribe(&draft_input, |this, _, ev: &InputEvent, cx| match ev {
            InputEvent::Submit(_) => this.commit_draft(cx),
            InputEvent::Change(_) => {
                if this.error.is_some() {
                    this.error = None;
                    cx.notify();
                }
            }
            _ => {}
        })
        .detach();

        Self {
            items: items.into_iter().map(Into::into).collect(),
            draft_input,
            add_label: "Add".into(),
            empty_text: "No items configured.".into(),
            deduplicate: true,
            case_insensitive_dedup: true,
            error: None,
        }
    }

    /// Sets the add button label (default `"Add"`).
    pub fn add_label(mut self, label: impl Into<SharedString>) -> Self {
        self.add_label = label.into();
        self
    }

    /// Sets the empty state copy.
    pub fn empty_text(mut self, text: impl Into<SharedString>) -> Self {
        self.empty_text = text.into();
        self
    }

    /// Controls duplicate prevention (default `true`, case-insensitive).
    pub fn deduplicate(mut self, deduplicate: bool, case_insensitive: bool) -> Self {
        self.deduplicate = deduplicate;
        self.case_insensitive_dedup = case_insensitive;
        self
    }

    /// Returns the current items slice.
    pub fn items(&self) -> &[SharedString] {
        &self.items
    }

    /// Replaces the items programmatically.
    pub fn set_items(
        &mut self,
        items: impl IntoIterator<Item = impl Into<SharedString>>,
        cx: &mut Context<Self>,
    ) {
        self.items = items.into_iter().map(Into::into).collect();
        self.error = None;
        cx.notify();
    }

    /// Attempts to add the trimmed draft input to the list.
    pub fn commit_draft(&mut self, cx: &mut Context<Self>) {
        let raw = self.draft_input.read(cx).value().trim().to_owned();
        if raw.is_empty() {
            return;
        }
        if self.deduplicate {
            let exists = if self.case_insensitive_dedup {
                self.items.iter().any(|it| it.eq_ignore_ascii_case(&raw))
            } else {
                self.items.iter().any(|it| it.as_ref() == raw)
            };
            if exists {
                self.error = Some(SharedString::from(format!("'{raw}' is already in the list.")));
                cx.notify();
                return;
            }
        }
        self.items.push(SharedString::from(raw));
        self.error = None;
        self.draft_input
            .update(cx, |inp, cx| inp.set_value_silent(String::new(), cx));
        cx.emit(StringListEvent::Change(self.items.clone()));
        cx.notify();
    }

    /// Removes the item at `index`.
    pub fn remove_at(&mut self, index: usize, cx: &mut Context<Self>) {
        if index < self.items.len() {
            self.items.remove(index);
            self.error = None;
            cx.emit(StringListEvent::Change(self.items.clone()));
            cx.notify();
        }
    }
}

impl Render for StringListState {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let theme = cx.theme();
        let entity_id = cx.entity_id();

        let rows = if self.items.is_empty() {
            div()
                .px(px(12.))
                .py(px(10.))
                .text_size(px(12.5))
                .text_color(theme.muted_foreground)
                .child(self.empty_text.clone())
                .into_any_element()
        } else {
            div()
                .flex()
                .flex_col()
                .children(self.items.iter().enumerate().map(|(idx, item)| {
                    let hover_bg = theme.surface_hover;
                    div()
                        .flex()
                        .flex_row()
                        .items_center()
                        .justify_between()
                        .gap(px(8.))
                        .px(px(12.))
                        .py(px(7.))
                        .when(idx > 0, |d| d.border_t_1().border_color(theme.border))
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(theme.foreground)
                                .truncate()
                                .child(item.clone()),
                        )
                        .child(
                            div()
                                .id(("strlist-rm", idx))
                                .flex()
                                .items_center()
                                .justify_center()
                                .p(px(4.))
                                .rounded(theme.radius_sm)
                                .cursor_pointer()
                                .hover(move |s| s.bg(hover_bg))
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    this.remove_at(idx, cx);
                                }))
                                .child(
                                    Icon::new(LucideIcon::Trash)
                                        .size(IconSize::Xs)
                                        .color(theme.muted_foreground),
                                ),
                        )
                }))
                .into_any_element()
        };

        div()
            .id(("string-list", entity_id))
            .flex()
            .flex_col()
            .gap(px(8.))
            .child(
                div()
                    .w_full()
                    .rounded(theme.radius_md)
                    .bg(theme.surface)
                    .border_1()
                    .border_color(theme.border)
                    .overflow_hidden()
                    .child(rows),
            )
            .child(
                div()
                    .flex()
                    .flex_row()
                    .items_center()
                    .gap(px(8.))
                    .child(div().flex_1().min_w_0().child(Input::new(&self.draft_input)))
                    .child(
                        Button::new(("strlist-add", entity_id))
                            .secondary()
                            .size(ComponentSize::Sm)
                            .leading_icon(LucideIcon::Plus)
                            .label(self.add_label.clone())
                            .on_click(cx.listener(|this, _, _, cx| this.commit_draft(cx))),
                    ),
            )
            .when_some(self.error.clone(), |el, err| {
                el.child(
                    div()
                        .text_size(px(11.5))
                        .text_color(theme.destructive)
                        .child(err),
                )
            })
    }
}

/// Editable settings string list element bound to [`StringListState`].
#[derive(IntoElement)]
pub struct StringList {
    state: Entity<StringListState>,
    style: StyleRefinement,
}

/// Convenience constructor for [`StringList::new`].
pub fn string_list(state: &Entity<StringListState>) -> StringList {
    StringList::new(state)
}

impl StringList {
    /// Creates a [`StringList`] element bound to `state`.
    pub fn new(state: &Entity<StringListState>) -> Self {
        Self {
            state: state.clone(),
            style: StyleRefinement::default(),
        }
    }
}

impl Styled for StringList {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for StringList {
    fn render(self, _window: &mut Window, _cx: &mut App) -> impl IntoElement {
        let mut wrapper = div().child(self.state);
        crate::Refineable::refine(wrapper.style(), &self.style);
        wrapper
    }
}

// ---------------------------------------------------------------------------
// 7. ColorSwatchPopover (fast preset swatch strip + transparent checkerboard chip)
// ---------------------------------------------------------------------------

/// Compact preset color swatch picker with an optional transparent checkerboard chip (`None`).
#[derive(IntoElement)]
pub struct ColorSwatchPopover {
    id: ElementId,
    selected: Option<Hsla>,
    swatches: Vec<Hsla>,
    allow_transparent: bool,
    columns: usize,
    style: StyleRefinement,
    on_select: Option<Rc<dyn Fn(Option<Hsla>, &mut Window, &mut App) + 'static>>,
}

/// Convenience constructor for [`ColorSwatchPopover::new`].
pub fn color_swatch_popover(id: impl Into<ElementId>, selected: Option<Hsla>) -> ColorSwatchPopover {
    ColorSwatchPopover::new(id, selected)
}

impl ColorSwatchPopover {
    /// Creates a [`ColorSwatchPopover`] with a curated default palette.
    pub fn new(id: impl Into<ElementId>, selected: Option<Hsla>) -> Self {
        Self {
            id: id.into(),
            selected,
            swatches: vec![
                rgb(0xEF4444).into(),
                rgb(0xF97316).into(),
                rgb(0xEAB308).into(),
                rgb(0x22C55E).into(),
                rgb(0x06B6D4).into(),
                rgb(0x3B82F6).into(),
                rgb(0x6366F1).into(),
                rgb(0xA855F7).into(),
                rgb(0xEC4899).into(),
                rgb(0x18181B).into(),
                rgb(0xFFFFFF).into(),
            ],
            allow_transparent: true,
            columns: 6,
            style: StyleRefinement::default(),
            on_select: None,
        }
    }

    /// Overrides the preset swatch colors.
    pub fn swatches(mut self, swatches: impl IntoIterator<Item = Hsla>) -> Self {
        self.swatches = swatches.into_iter().collect();
        self
    }

    /// Toggles the transparent checkerboard swatch chip (default `true`).
    pub fn allow_transparent(mut self, allow: bool) -> Self {
        self.allow_transparent = allow;
        self
    }

    /// Sets swatches per row (default `6`).
    pub fn columns(mut self, cols: usize) -> Self {
        self.columns = cols.max(1);
        self
    }

    /// Callback invoked with `Some(color)` or `None` (transparent).
    pub fn on_select(
        mut self,
        handler: impl Fn(Option<Hsla>, &mut Window, &mut App) + 'static,
    ) -> Self {
        self.on_select = Some(Rc::new(handler));
        self
    }
}

impl Styled for ColorSwatchPopover {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for ColorSwatchPopover {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let on_select = self.on_select;
        let selected = self.selected;

        let mut chips: Vec<AnyElement> = Vec::new();
        if self.allow_transparent {
            let is_sel = selected.is_none();
            let cb = on_select.clone();
            chips.push(
                div()
                    .id((self.id.clone(), "swatch-transparent"))
                    .w(px(22.))
                    .h(px(22.))
                    .rounded(theme.radius_sm)
                    .overflow_hidden()
                    .border_2()
                    .border_color(if is_sel { theme.primary } else { theme.border })
                    .cursor_pointer()
                    .flex()
                    .flex_col()
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_row()
                            .child(div().flex_1().bg(rgb(0xFFFFFF)))
                            .child(div().flex_1().bg(rgb(0xCBD5E1))),
                    )
                    .child(
                        div()
                            .flex_1()
                            .flex()
                            .flex_row()
                            .child(div().flex_1().bg(rgb(0xCBD5E1)))
                            .child(div().flex_1().bg(rgb(0xFFFFFF))),
                    )
                    .on_click(move |_, window, cx| {
                        if let Some(ref cb) = cb {
                            cb(None, window, cx);
                        }
                    })
                    .into_any_element(),
            );
        }

        for (i, swatch) in self.swatches.into_iter().enumerate() {
            let is_sel = selected.is_some_and(|s| {
                (s.h - swatch.h).abs() < 0.01
                    && (s.s - swatch.s).abs() < 0.01
                    && (s.l - swatch.l).abs() < 0.01
            });
            let cb = on_select.clone();
            chips.push(
                div()
                    .id((self.id.clone(), format!("swatch-{i}")))
                    .w(px(22.))
                    .h(px(22.))
                    .rounded(theme.radius_sm)
                    .bg(swatch)
                    .border_2()
                    .border_color(if is_sel { theme.primary } else { theme.border })
                    .cursor_pointer()
                    .on_click(move |_, window, cx| {
                        if let Some(ref cb) = cb {
                            cb(Some(swatch), window, cx);
                        }
                    })
                    .into_any_element(),
            );
        }

        let mut rows_box = div().flex().flex_col().gap(px(6.));
        let mut iter = chips.into_iter();
        loop {
            let row_items: Vec<AnyElement> = iter.by_ref().take(self.columns).collect();
            if row_items.is_empty() {
                break;
            }
            rows_box = rows_box.child(div().flex().flex_row().gap(px(6.)).children(row_items));
        }

        let status_label = match selected {
            None => "Transparent".to_owned(),
            Some(c) => {
                let r: crate::Rgba = c.into();
                format!(
                    "#{:02X}{:02X}{:02X}",
                    (r.r * 255.0).round() as u8,
                    (r.g * 255.0).round() as u8,
                    (r.b * 255.0).round() as u8
                )
            }
        };

        let mut el = div()
            .flex()
            .flex_col()
            .gap(px(8.))
            .p(px(10.))
            .rounded(theme.radius_lg)
            .bg(theme.popover)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |d| d.shadow_lg())
            .child(rows_box)
            .child(
                div()
                    .pt(px(6.))
                    .border_t_1()
                    .border_color(theme.border)
                    .flex()
                    .flex_row()
                    .items_center()
                    .justify_between()
                    .text_size(px(11.5))
                    .text_color(theme.muted_foreground)
                    .child("Selected")
                    .child(
                        div()
                            .font_weight(FontWeight::SEMIBOLD)
                            .text_color(theme.foreground)
                            .child(status_label),
                    ),
            );
        crate::Refineable::refine(el.style(), &self.style);
        el
    }
}

// ---------------------------------------------------------------------------
// 8. FloatingToolbar (compact elevated contextual horizontal toolbar)
// ---------------------------------------------------------------------------

/// Elevated compact horizontal floating toolbar (for screenshot editors, canvas overlays, and
/// contextual selection actions) that stops drag/click propagation to the canvas beneath it.
#[derive(IntoElement)]
pub struct FloatingToolbar {
    id: ElementId,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

/// Convenience constructor for [`FloatingToolbar::new`].
pub fn floating_toolbar(id: impl Into<ElementId>) -> FloatingToolbar {
    FloatingToolbar::new(id)
}

/// Hairline vertical separator for [`FloatingToolbar`].
pub fn toolbar_separator(cx: &App) -> AnyElement {
    let theme = cx.theme();
    div()
        .w(px(1.))
        .h(px(18.))
        .mx(px(2.))
        .bg(theme.border)
        .into_any_element()
}

impl FloatingToolbar {
    /// Creates a new [`FloatingToolbar`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Appends a control or separator into the toolbar.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }

    /// Appends multiple controls into the toolbar.
    pub fn children(mut self, children: impl IntoIterator<Item = impl IntoElement>) -> Self {
        self.children
            .extend(children.into_iter().map(IntoElement::into_any_element));
        self
    }
}

impl Styled for FloatingToolbar {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for FloatingToolbar {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let mut bar = div()
            .id(self.id)
            .occlude()
            .flex()
            .flex_row()
            .items_center()
            .gap(px(4.))
            .px(px(6.))
            .py(px(5.))
            .rounded(theme.radius_lg)
            .bg(theme.popover.opacity(0.95))
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |d| d.shadow_lg())
            .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
            .children(self.children);

        crate::Refineable::refine(bar.style(), &self.style);
        bar
    }
}

// ---------------------------------------------------------------------------
// 9. SimulatedBackdrop (Wallpaper / Code / Plain desktop preview stage)
// ---------------------------------------------------------------------------

/// Simulated desktop surface behind a translucent floating pane or frosted glass widget.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum SimulatedBackdropKind {
    /// Rich colorful aurora/gradient wallpaper ideal for tuning frosted glass opacity & blur.
    #[default]
    Wallpaper,
    /// Simulated syntax-highlighted IDE editor window for checking HUD legibility over code.
    Code,
    /// Clean architectural neutral surface.
    Plain,
}

impl SimulatedBackdropKind {
    /// All backdrop presets in display order.
    pub const ALL: [Self; 3] = [Self::Wallpaper, Self::Code, Self::Plain];

    /// Short UI label.
    pub const fn label(self) -> &'static str {
        match self {
            Self::Wallpaper => "Wallpaper",
            Self::Code => "Code IDE",
            Self::Plain => "Plain",
        }
    }
}

/// Studio preview container painting a realistic desktop backdrop (`Wallpaper`, `Code`, or `Plain`)
/// behind arbitrary centered children (such as translucent glass floating panes).
#[derive(IntoElement)]
pub struct SimulatedBackdrop {
    kind: SimulatedBackdropKind,
    children: Vec<AnyElement>,
    style: StyleRefinement,
}

/// Convenience constructor for [`SimulatedBackdrop::new`].
pub fn simulated_backdrop(kind: SimulatedBackdropKind) -> SimulatedBackdrop {
    SimulatedBackdrop::new(kind)
}

impl SimulatedBackdrop {
    /// Creates a [`SimulatedBackdrop`] with `kind`.
    pub fn new(kind: SimulatedBackdropKind) -> Self {
        Self {
            kind,
            children: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Adds foreground content rendered over the simulated backdrop.
    pub fn child(mut self, child: impl IntoElement) -> Self {
        self.children.push(child.into_any_element());
        self
    }
}

impl Styled for SimulatedBackdrop {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for SimulatedBackdrop {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();

        let bg_layer = match self.kind {
            SimulatedBackdropKind::Wallpaper => div()
                .absolute()
                .inset_0()
                .bg(linear_gradient(
                    135.0,
                    linear_color_stop(rgb(0x1E1B4B), 0.0),
                    linear_color_stop(rgb(0x0F766E), 1.0),
                ))
                .child(
                    div()
                        .absolute()
                        .top(px(-40.))
                        .left(px(40.))
                        .w(px(220.))
                        .h(px(220.))
                        .rounded_full()
                        .bg(rgba(0xEC489988)),
                )
                .child(
                    div()
                        .absolute()
                        .bottom(px(-50.))
                        .right(px(50.))
                        .w(px(260.))
                        .h(px(260.))
                        .rounded_full()
                        .bg(rgba(0x38BDF877)),
                )
                .child(
                    div()
                        .absolute()
                        .top(px(40.))
                        .right(px(140.))
                        .w(px(150.))
                        .h(px(150.))
                        .rounded_full()
                        .bg(rgba(0xF59E0B66)),
                )
                .into_any_element(),
            SimulatedBackdropKind::Code => {
                let code_lines: &[(&str, &str, u32)] = &[
                    ("01", "use gpui::{WindowOptions, WindowBackgroundAppearance, px};", 0x93C5FD),
                    ("02", "let opts = WindowOptions {", 0xE2E8F0),
                    ("03", "    always_on_top: true, non_activating: true,", 0x86EFAC),
                    ("04", "    window_background: WindowBackgroundAppearance::Blurred,", 0xFCD34D),
                    ("05", "    backdrop_corner_radius: Some(px(14.)),", 0xF9A8D4),
                    ("06", "    hide_window_border: true, ..Default::default()", 0xC4B5FD),
                    ("07", "};", 0xE2E8F0),
                    ("08", "cx.set_launch_at_login(AutostartOptions::new(\"MyApp\"), true)?;", 0x67E8F9),
                ];
                div()
                    .absolute()
                    .inset_0()
                    .bg(rgb(0x0F172A))
                    .p(px(16.))
                    .flex()
                    .flex_col()
                    .gap(px(6.))
                    .children(code_lines.iter().map(|(ln, code, col)| {
                        div()
                            .flex()
                            .flex_row()
                            .gap(px(12.))
                            .text_size(px(12.))
                            .when_some(theme.mono_font_family.clone(), |d, m| d.font_family(m))
                            .child(div().text_color(rgb(0x475569)).child(*ln))
                            .child(div().text_color(rgb(*col)).child(*code))
                    }))
                    .into_any_element()
            }
            SimulatedBackdropKind::Plain => div()
                .absolute()
                .inset_0()
                .bg(theme.surface_muted)
                .into_any_element(),
        };

        let mut container = div()
            .relative()
            .w_full()
            .min_h(px(210.))
            .rounded(theme.radius_xl)
            .border_1()
            .border_color(theme.border)
            .overflow_hidden()
            .flex()
            .items_center()
            .justify_center()
            .p(px(24.))
            .child(bg_layer)
            .children(self.children);

        crate::Refineable::refine(container.style(), &self.style);
        container
    }
}

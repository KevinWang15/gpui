//! Divided KPI metric strip ([`StatGroup`]) with OpenType tabular numerals and polarity-aware deltas.

use crate::{
    App, ElementId, FontWeight, Hsla, IntoElement, ParentElement, RenderOnce, SharedString,
    StyleRefinement, Styled, Window, div, prelude::*, px,
};

use super::theme::{ActiveTheme, Theme, tabular_numerals};

/// Whether an increase in a KPI metric is favorable, unfavorable, or neutral.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum TrendPolarity {
    /// Positive deltas are good (green), negative deltas are bad (amber/red). E.g. revenue, cache hit rate.
    #[default]
    HigherIsBetter,
    /// Negative deltas are good (green), positive deltas are unfavorable (amber). E.g. cost, latency, error rate.
    LowerIsBetter,
    /// Delta is informational and uses muted neutral styling.
    Neutral,
}

/// Visual tone for a [`StatDelta`] caption.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum StatDeltaTone {
    /// Muted neutral caption (`was 58.0% before`).
    #[default]
    Neutral,
    /// Favorable emerald tone (`-11.9% vs workdays`).
    Positive,
    /// Caution / unfavorable amber tone (`+10.9% vs previous 30 days`).
    Warning,
    /// Severe negative / destructive red tone.
    Negative,
}

/// Subtitle delta line rendered beneath a KPI value inside a [`StatItem`].
#[derive(Clone, Debug)]
pub struct StatDelta {
    /// Formatted delta string (e.g. `"+10.9% vs previous 30 days"`).
    pub text: SharedString,
    /// Semantic color tone.
    pub tone: StatDeltaTone,
}

impl StatDelta {
    /// Creates a neutral muted subtitle (e.g. `"was 58.0% before"`).
    pub fn neutral(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: StatDeltaTone::Neutral,
        }
    }

    /// Creates a favorable green delta subtitle.
    pub fn positive(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: StatDeltaTone::Positive,
        }
    }

    /// Creates a caution / unfavorable amber delta subtitle.
    pub fn warning(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: StatDeltaTone::Warning,
        }
    }

    /// Creates a negative / destructive red delta subtitle.
    pub fn negative(text: impl Into<SharedString>) -> Self {
        Self {
            text: text.into(),
            tone: StatDeltaTone::Negative,
        }
    }

    /// Formats a percentage change (`+10.9%`, `-11.9%`) with `suffix` and selects the semantic
    /// tone automatically according to `polarity`.
    pub fn percent(delta_pct: f64, suffix: &str, polarity: TrendPolarity) -> Self {
        let sign = if delta_pct > 0.0 { "+" } else { "" };
        let text = if suffix.is_empty() {
            format!("{sign}{delta_pct:.1}%")
        } else {
            format!("{sign}{delta_pct:.1}% {suffix}")
        };
        let tone = if delta_pct.abs() < 0.05 || polarity == TrendPolarity::Neutral {
            StatDeltaTone::Neutral
        } else {
            match (delta_pct > 0.0, polarity) {
                (true, TrendPolarity::HigherIsBetter) | (false, TrendPolarity::LowerIsBetter) => {
                    StatDeltaTone::Positive
                }
                (true, TrendPolarity::LowerIsBetter) => StatDeltaTone::Warning,
                (false, TrendPolarity::HigherIsBetter) => StatDeltaTone::Negative,
                (_, TrendPolarity::Neutral) => StatDeltaTone::Neutral,
            }
        };
        Self {
            text: text.into(),
            tone,
        }
    }

    fn color(&self, theme: &Theme) -> Hsla {
        match self.tone {
            StatDeltaTone::Neutral => theme.muted_foreground,
            StatDeltaTone::Positive => theme.success,
            StatDeltaTone::Warning => theme.warning,
            StatDeltaTone::Negative => theme.destructive,
        }
    }
}

/// One KPI metric cell inside a [`StatGroup`].
#[derive(Clone, Debug)]
pub struct StatItem {
    /// Short top metric label (e.g. `"30-day cost"`).
    pub label: SharedString,
    /// Prominent primary figure rendered with tabular numerals (e.g. `"$896.90"`).
    pub value: SharedString,
    /// Optional comparison / delta subtitle.
    pub delta: Option<StatDelta>,
}

impl StatItem {
    /// Creates a new KPI [`StatItem`].
    pub fn new(label: impl Into<SharedString>, value: impl Into<SharedString>) -> Self {
        Self {
            label: label.into(),
            value: value.into(),
            delta: None,
        }
    }

    /// Attaches a comparison delta subtitle.
    pub fn delta(mut self, delta: StatDelta) -> Self {
        self.delta = Some(delta);
        self
    }
}

/// Horizontal KPI strip card divided by 1px vertical hairlines.
#[derive(IntoElement)]
pub struct StatGroup {
    id: ElementId,
    items: Vec<StatItem>,
    style: StyleRefinement,
}

/// Convenience constructor for [`StatGroup::new`].
pub fn stat_group(id: impl Into<ElementId>) -> StatGroup {
    StatGroup::new(id)
}

impl StatGroup {
    /// Creates a new [`StatGroup`].
    pub fn new(id: impl Into<ElementId>) -> Self {
        Self {
            id: id.into(),
            items: Vec::new(),
            style: StyleRefinement::default(),
        }
    }

    /// Appends one [`StatItem`] cell.
    pub fn item(mut self, item: StatItem) -> Self {
        self.items.push(item);
        self
    }

    /// Appends multiple [`StatItem`] cells.
    pub fn items(mut self, items: impl IntoIterator<Item = StatItem>) -> Self {
        self.items.extend(items);
        self
    }
}

impl Styled for StatGroup {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for StatGroup {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let tnum = tabular_numerals();

        let mut row = div()
            .id(self.id)
            .w_full()
            .flex()
            .flex_row()
            .flex_shrink_0()
            .rounded(theme.radius_xl)
            .bg(theme.surface)
            .border_1()
            .border_color(theme.border)
            .when(theme.elevation_shadows, |el| el.shadow_xs())
            .overflow_hidden();

        for (index, item) in self.items.into_iter().enumerate() {
            let mut cell = div()
                .flex_1()
                .min_w(px(140.))
                .flex()
                .flex_col()
                .gap(px(4.))
                .px(px(20.))
                .py(px(16.))
                .when(index > 0, |el| el.border_l_1().border_color(theme.border))
                .child(
                    div()
                        .text_size(px(12.5))
                        .font_weight(FontWeight::MEDIUM)
                        .text_color(theme.muted_foreground)
                        .child(item.label),
                )
                .child(
                    div()
                        .text_size(px(24.))
                        .line_height(px(30.))
                        .font_weight(FontWeight::SEMIBOLD)
                        .font_features(tnum.clone())
                        .text_color(theme.foreground)
                        .child(item.value),
                );

            if let Some(delta) = item.delta {
                let color = delta.color(&theme);
                cell = cell.child(
                    div()
                        .text_size(px(12.))
                        .font_weight(FontWeight::MEDIUM)
                        .font_features(tnum.clone())
                        .text_color(color)
                        .child(delta.text),
                );
            }

            row = row.child(cell);
        }

        crate::Refineable::refine(row.style(), &self.style);
        row
    }
}

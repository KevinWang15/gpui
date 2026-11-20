//! Chronological [`Timeline`] component for audit logs, deploy streams, and activity feeds.
//!
//! Supports `Left` and `Alternate` layouts, semantic status tones, custom [`LucideIcon`] nodes,
//! timestamps, rich body slots, and an optional live spinning pending indicator.

use smallvec::SmallVec;

use crate::{
    AnyElement, App, FontWeight, IntoElement, RenderOnce, SharedString, StyleRefinement, Styled,
    Window, div, prelude::*, px,
};
use crate::icons::{Icon, IconSize, LucideIcon};

use super::feedback::{Spinner, SpinnerVariant};
use super::theme::{ActiveTheme, tabular_numerals};

/// Semantic node accent color inside a [`Timeline`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TimelineTone {
    /// Primary brand accent (default).
    #[default]
    Primary,
    /// Success green.
    Success,
    /// Warning amber.
    Warning,
    /// Destructive red.
    Destructive,
    /// Muted neutral gray.
    Muted,
}

/// Layout alignment for [`Timeline`] entries.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum TimelineMode {
    /// Standard left rail with content aligned to the right (default).
    #[default]
    Left,
    /// Centered rail alternating entries left and right.
    Alternate,
}

/// Single chronological event inside a [`Timeline`].
pub struct TimelineItem {
    /// Primary event heading.
    pub title: SharedString,
    /// Optional secondary description line.
    pub description: Option<SharedString>,
    /// Optional timestamp / metadata text (e.g. `"2 mins ago"` or `"14:32:08 UTC"`).
    pub timestamp: Option<SharedString>,
    /// Status dot / icon color tone.
    pub tone: TimelineTone,
    /// Optional custom [`LucideIcon`] rendered inside a circular badge instead of a dot.
    pub icon: Option<LucideIcon>,
    /// Optional rich custom content slot rendered below description.
    pub content: Option<AnyElement>,
}

impl TimelineItem {
    /// Creates a new [`TimelineItem`] with `title`.
    pub fn new(title: impl Into<SharedString>) -> Self {
        Self {
            title: title.into(),
            description: None,
            timestamp: None,
            tone: TimelineTone::Primary,
            icon: None,
            content: None,
        }
    }

    /// Sets muted descriptive text.
    pub fn description(mut self, description: impl Into<SharedString>) -> Self {
        self.description = Some(description.into());
        self
    }

    /// Sets the event timestamp string.
    pub fn timestamp(mut self, timestamp: impl Into<SharedString>) -> Self {
        self.timestamp = Some(timestamp.into());
        self
    }

    /// Sets the status color tone.
    pub fn tone(mut self, tone: TimelineTone) -> Self {
        self.tone = tone;
        self
    }

    /// Replaces the status dot with a themed icon badge.
    pub fn icon(mut self, icon: LucideIcon) -> Self {
        self.icon = Some(icon);
        self
    }

    /// Appends a rich custom child element below the text block.
    pub fn content(mut self, element: impl IntoElement) -> Self {
        self.content = Some(element.into_any_element());
        self
    }
}

/// Vertical or alternating chronological event [`Timeline`].
#[derive(IntoElement)]
pub struct Timeline {
    items: SmallVec<[TimelineItem; 8]>,
    mode: TimelineMode,
    pending: Option<SharedString>,
    style: StyleRefinement,
}

/// Convenience constructor for [`Timeline::new`].
pub fn timeline() -> Timeline {
    Timeline::new()
}

impl Default for Timeline {
    fn default() -> Self {
        Self::new()
    }
}

impl Timeline {
    /// Creates a new left-aligned [`Timeline`].
    pub fn new() -> Self {
        Self {
            items: SmallVec::new(),
            mode: TimelineMode::Left,
            pending: None,
            style: StyleRefinement::default(),
        }
    }

    /// Sets layout mode (`Left` or `Alternate`).
    pub fn mode(mut self, mode: TimelineMode) -> Self {
        self.mode = mode;
        self
    }

    /// Appends a single [`TimelineItem`].
    pub fn item(mut self, item: TimelineItem) -> Self {
        self.items.push(item);
        self
    }

    /// Appends multiple [`TimelineItem`]s.
    pub fn items(mut self, items: impl IntoIterator<Item = TimelineItem>) -> Self {
        self.items.extend(items);
        self
    }

    /// Appends a live spinning pending ghost node at the bottom (e.g. `"Deploying canary..."`).
    pub fn pending(mut self, label: impl Into<SharedString>) -> Self {
        self.pending = Some(label.into());
        self
    }
}

impl Styled for Timeline {
    fn style(&mut self) -> &mut StyleRefinement {
        &mut self.style
    }
}

impl RenderOnce for Timeline {
    fn render(self, _window: &mut Window, cx: &mut App) -> impl IntoElement {
        let theme = cx.theme();
        let has_pending = self.pending.is_some();
        let count = self.items.len();

        let mut root = div().flex().flex_col().w_full();

        for (idx, item) in self.items.into_iter().enumerate() {
            let show_connector = idx + 1 < count || has_pending;
            let (accent, soft) = match item.tone {
                TimelineTone::Primary => (theme.primary, theme.primary_soft),
                TimelineTone::Success => (theme.success, theme.success_soft),
                TimelineTone::Warning => (theme.warning, theme.warning_soft),
                TimelineTone::Destructive => (theme.destructive, theme.destructive_soft),
                TimelineTone::Muted => (theme.muted_foreground, theme.surface_muted),
            };

            let marker = if let Some(ic) = item.icon {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .w(px(26.))
                    .h(px(26.))
                    .rounded_full()
                    .bg(soft)
                    .border_1()
                    .border_color(accent.opacity(0.45))
                    .child(Icon::new(ic).size(IconSize::Xs).color(accent))
                    .into_any_element()
            } else {
                div()
                    .flex()
                    .items_center()
                    .justify_center()
                    .flex_shrink_0()
                    .w(px(26.))
                    .h(px(22.))
                    .child(
                        div()
                            .w(px(10.))
                            .h(px(10.))
                            .rounded_full()
                            .bg(theme.surface)
                            .border_2()
                            .border_color(accent),
                    )
                    .into_any_element()
            };

            let rail = div()
                .flex()
                .flex_col()
                .items_center()
                .flex_shrink_0()
                .w(px(26.))
                .child(marker)
                .when(show_connector, |el| {
                    el.child(
                        div()
                            .w(px(2.))
                            .flex_1()
                            .min_h(px(24.))
                            .my(px(2.))
                            .rounded_full()
                            .bg(theme.border),
                    )
                });

            let align_right = self.mode == TimelineMode::Alternate && idx % 2 == 1;
            let body = div()
                .flex()
                .flex_col()
                .gap(px(3.))
                .w_full()
                .min_w_0()
                .when(align_right, |el| el.items_end().text_right())
                .pb(if show_connector { px(18.) } else { px(4.) })
                .child(
                    div()
                        .flex()
                        .flex_row()
                        .flex_wrap()
                        .items_center()
                        .gap(px(8.))
                        .w_full()
                        .min_w_0()
                        .when(align_right, |el| el.justify_end())
                        .child(
                            div()
                                .min_w_0()
                                .text_size(px(13.5))
                                .font_weight(FontWeight::SEMIBOLD)
                                .text_color(theme.foreground)
                                .child(item.title),
                        )
                        .when_some(item.timestamp, |el, ts| {
                            el.child(
                                div()
                                    .flex_shrink_0()
                                    .text_size(px(11.5))
                                    .font_features(tabular_numerals())
                                    .text_color(theme.muted_foreground)
                                    .child(ts),
                            )
                        }),
                )
                .when_some(item.description, |el, desc| {
                    el.child(
                        div()
                            .w_full()
                            .min_w_0()
                            .text_size(px(12.5))
                            .text_color(theme.muted_foreground)
                            .child(desc),
                    )
                })
                .when_some(item.content, |el, content| {
                    el.child(div().pt(px(6.)).child(content))
                });

            let row = match self.mode {
                TimelineMode::Left => div()
                    .flex()
                    .flex_row()
                    .items_stretch()
                    .gap(px(12.))
                    .w_full()
                    .child(rail)
                    .child(div().flex_1().min_w_0().pt(px(2.)).child(body)),
                TimelineMode::Alternate => {
                    let (left_slot, right_slot) = if align_right {
                        (
                            div().flex_1().min_w_0().pt(px(2.)).child(body),
                            div().flex_1().min_w_0(),
                        )
                    } else {
                        (
                            div().flex_1().min_w_0(),
                            div().flex_1().min_w_0().pt(px(2.)).child(body),
                        )
                    };
                    div()
                        .flex()
                        .flex_row()
                        .items_stretch()
                        .gap(px(12.))
                        .w_full()
                        .child(left_slot)
                        .child(rail)
                        .child(right_slot)
                }
            };

            root = root.child(row);
        }

        if let Some(pending_label) = self.pending {
            let pending_align_right = self.mode == TimelineMode::Alternate && count % 2 == 1;
            let pending_marker = div()
                .flex()
                .items_center()
                .justify_center()
                .flex_shrink_0()
                .w(px(26.))
                .h(px(24.))
                .child(Spinner::new().variant(SpinnerVariant::Ring).size(IconSize::Sm));
            let pending_text = div()
                .flex_1()
                .min_w_0()
                .pt(px(3.))
                .text_size(px(13.))
                .text_color(theme.muted_foreground)
                .when(pending_align_right, |el| el.text_right())
                .child(pending_label);

            let pending_row = match self.mode {
                TimelineMode::Left => div()
                    .flex()
                    .flex_row()
                    .items_start()
                    .gap(px(12.))
                    .w_full()
                    .child(pending_marker)
                    .child(pending_text),
                TimelineMode::Alternate => {
                    let (left_slot, right_slot) = if pending_align_right {
                        (
                            div().flex_1().min_w_0().flex().child(pending_text),
                            div().flex_1().min_w_0(),
                        )
                    } else {
                        (
                            div().flex_1().min_w_0(),
                            div().flex_1().min_w_0().flex().child(pending_text),
                        )
                    };
                    div()
                        .flex()
                        .flex_row()
                        .items_start()
                        .gap(px(12.))
                        .w_full()
                        .child(left_slot)
                        .child(pending_marker)
                        .child(right_slot)
                }
            };
            root = root.child(pending_row);
        }

        crate::Refineable::refine(root.style(), &self.style);
        root
    }
}

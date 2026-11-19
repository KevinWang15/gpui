//! Interactive Analytics & Charts showcase demonstrating [`TimeRangePicker`],
//! faceted [`FilterBar`], [`StatGroup`], [`SegmentedControl`], [`StackedBarChart`],
//! [`LineChart`], [`BreakdownList`], [`ChartCard`] data tables, and click-origin
//! radial [`ThemeSwitcher`] transitions.

use gpui::components::*;
use gpui::icons::LucideIcon;
use gpui::{Context, FontWeight, SharedString, div, prelude::*, px};

use crate::state::PlaygroundApp;

/// Synthetic model telemetry descriptor driving realistic, reactive filtered charts.
struct ModelSpec {
    name: &'static str,
    provider: &'static str,
    key_scope: &'static str,
    base_daily_cost: f64,
    tokens_per_dollar: f64,
    requests_per_dollar: f64,
    phase: f64,
    facet_hits: usize,
}

const MODELS: [ModelSpec; 4] = [
    ModelSpec {
        name: "claude-sonnet-5-5",
        provider: "anthropic",
        key_scope: "prod-api",
        base_daily_cost: 17.34,
        tokens_per_dollar: 39_500.0,
        requests_per_dollar: 21.0,
        phase: 0.7,
        facet_hits: 309,
    },
    ModelSpec {
        name: "gpt-6.2",
        provider: "openai",
        key_scope: "prod-api",
        base_daily_cost: 8.97,
        tokens_per_dollar: 42_000.0,
        requests_per_dollar: 24.5,
        phase: 2.1,
        facet_hits: 476,
    },
    ModelSpec {
        name: "deepseek-v4.1-flash",
        provider: "local",
        key_scope: "dev-local",
        base_daily_cost: 3.59,
        tokens_per_dollar: 68_000.0,
        requests_per_dollar: 34.0,
        phase: 3.6,
        facet_hits: 309,
    },
    ModelSpec {
        name: "cursor-composer-2",
        provider: "cursor",
        key_scope: "ide-agent",
        base_daily_cost: 1.45,
        tokens_per_dollar: 45_000.0,
        requests_per_dollar: 28.0,
        phase: 4.9,
        facet_hits: 78,
    },
];

/// Builds the reactive facet provider for [`FilterBarState`], narrowing counts dynamically
/// based on active sibling clauses.
pub fn analytics_facet_provider(
    active_clauses: &[FilterClause],
    field_key: &str,
    query: &str,
) -> Vec<FilterFacetOption> {
    let q = query.trim().to_lowercase();
    let matching_models: Vec<&ModelSpec> = MODELS
        .iter()
        .filter(|m| {
            active_clauses
                .iter()
                .filter(|c| c.field.as_ref() != field_key)
                .all(|c| match c.field.as_ref() {
                    "provider" => c.matches_value(m.provider),
                    "model" => c.matches_value(m.name),
                    "key" => c.matches_value(m.key_scope),
                    _ => true,
                })
        })
        .collect();

    let mut counts: Vec<(&str, usize)> = Vec::new();
    for m in matching_models {
        let val = match field_key {
            "provider" => m.provider,
            "model" => m.name,
            "key" => m.key_scope,
            _ => continue,
        };
        if let Some((_, count)) = counts.iter_mut().find(|(k, _)| *k == val) {
            *count += m.facet_hits;
        } else {
            counts.push((val, m.facet_hits));
        }
    }
    counts.sort_by(|a, b| b.1.cmp(&a.1).then_with(|| a.0.cmp(b.0)));

    counts
        .into_iter()
        .filter(|(val, _)| q.is_empty() || val.to_lowercase().contains(&q))
        .map(|(val, count)| FilterFacetOption::new(val, Some(count)))
        .collect()
}

fn day_multiplier(day_epoch: i64, phase: f64, seed: u64) -> f64 {
    let d = (day_epoch as f64) + (seed as f64) * 1.37;
    let wave = 0.65 + 0.35 * ((d * 0.42 + phase).sin() * 0.65 + (d * 1.13 + phase * 1.7).cos() * 0.35);
    let spike = if ((day_epoch + seed as i64).rem_euclid(11)) == 4 {
        1.42
    } else if ((day_epoch + seed as i64).rem_euclid(7)) == 5 {
        0.58
    } else {
        1.0
    };
    (wave * spike).clamp(0.18, 2.15)
}

pub fn render(app: &PlaygroundApp, cx: &mut Context<PlaygroundApp>) -> impl IntoElement {
    let theme = cx.theme();
    let palette = ChartPalette::from_theme(&theme);
    let range = app.analytics_time_range.read(cx).range();
    let active_preset = app.analytics_time_range.read(cx).active_preset();
    let today = app.analytics_time_range.read(cx).today();
    let span_days = range.span_days();
    let clauses = app.analytics_filter.read(cx).clauses().to_vec();
    let metric_key = app.analytics_metric.clone();
    let seed = app.analytics_refresh_seed;
    let drag_zoom_enabled = app.analytics_drag_zoom_enabled;
    let line_selection = app.analytics_line_selection;
    let hidden_models = &app.analytics_hidden_models;
    let scope_mult = match app.analytics_scope_select.read(cx).selected_value() {
        Some(&"team") => 2.35,
        Some(&"all") => 3.80,
        _ => 1.0,
    };

    let (metric_noun, value_format) = match metric_key.as_ref() {
        "tokens" => ("tokens", ChartValueFormat::Compact),
        "requests" => ("requests", ChartValueFormat::Compact),
        _ => ("cost", ChartValueFormat::Currency),
    };
    let metric_title = match metric_key.as_ref() {
        "tokens" => "Tokens",
        "requests" => "Requests",
        _ => "Cost",
    };

    // Determine which models match active FilterBar clauses.
    let active_models: Vec<(usize, &ModelSpec)> = MODELS
        .iter()
        .enumerate()
        .filter(|(_, m)| {
            clauses.iter().all(|c| match c.field.as_ref() {
                "provider" => c.matches_value(m.provider),
                "model" => c.matches_value(m.name),
                "key" => c.matches_value(m.key_scope),
                _ => true,
            })
        })
        .collect();

    // Compute daily series across the selected DateRange.
    let mut bar_points = Vec::with_capacity(span_days as usize);
    let mut model_totals_cost = vec![0.0_f64; active_models.len()];
    let mut model_totals_metric = vec![0.0_f64; active_models.len()];
    let mut total_cost = 0.0_f64;
    let mut total_tokens = 0.0_f64;
    let mut total_requests = 0.0_f64;

    for offset in 0..i64::from(span_days) {
        let day = range.start.add_days(offset);
        let day_epoch = day.to_days_since_epoch();
        let label: SharedString = if day == today {
            "Today".into()
        } else {
            day.format_short().into()
        };

        let mut col_values = Vec::with_capacity(active_models.len());
        for (idx, (_, m)) in active_models.iter().enumerate() {
            let mult = day_multiplier(day_epoch, m.phase, seed) * scope_mult;
            let day_cost = m.base_daily_cost * mult;
            let day_tokens = day_cost * m.tokens_per_dollar;
            let day_reqs = day_cost * m.requests_per_dollar;

            total_cost += day_cost;
            total_tokens += day_tokens;
            total_requests += day_reqs;
            model_totals_cost[idx] += day_cost;

            let metric_val = match metric_key.as_ref() {
                "tokens" => day_tokens,
                "requests" => day_reqs,
                _ => day_cost,
            };
            model_totals_metric[idx] += metric_val;
            let is_hidden = hidden_models.iter().any(|h| h.as_ref() == m.name);
            col_values.push(if is_hidden { 0.0 } else { metric_val });
        }
        bar_points.push(StackedBarPoint::new(label, col_values));
    }

    let avg_daily_cost = if span_days > 0 {
        total_cost / f64::from(span_days)
    } else {
        0.0
    };
    let avg_daily_metric = if span_days > 0 {
        model_totals_metric.iter().sum::<f64>() / f64::from(span_days)
    } else {
        0.0
    };

    // Build cumulative intraday series ("Today" up to 17:36 vs "Previous 5 workdays, average" 00:00..24:00).
    let baseline_target = (avg_daily_metric * 1.38).max(0.01);
    let today_scale = 0.881;
    let mut today_points = Vec::new();
    let mut baseline_points = Vec::new();
    for step in 0..=48 {
        let hour = (step as f64) * 0.5;
        let frac = hour / 24.0;
        let curve = 0.08 * frac + 0.92 * (1.0 - (-(frac * 2.45)).exp()) / (1.0 - (-2.45_f64).exp());
        let base_y = baseline_target * curve;
        baseline_points.push((hour, base_y));
        if hour <= 17.6 {
            let jitter = 1.0 + 0.04 * (hour * 0.85 + (seed as f64)).sin();
            today_points.push((hour, base_y * today_scale * jitter));
        }
    }
    let today_cost_now = (avg_daily_cost * 1.18 * today_scale).max(0.0);

    // Stacked bar series metadata & interactive legend (clicking toggles visibility)
    let bar_series: Vec<StackedBarSeries> = active_models
        .iter()
        .map(|&(orig_idx, m)| StackedBarSeries::new(m.name).color(palette.color(orig_idx)))
        .collect();
    let bar_legend: Vec<ChartLegendItem> = active_models
        .iter()
        .map(|&(orig_idx, m)| {
            let muted = hidden_models.iter().any(|h| h.as_ref() == m.name);
            ChartLegendItem::dot(m.name, palette.color(orig_idx)).muted(muted)
        })
        .collect();
    let legend_model_names: Vec<SharedString> = active_models
        .iter()
        .map(|(_, m)| SharedString::from(m.name))
        .collect();

    // Line chart series & legend
    let line_series = vec![
        LineSeries::new("Today", today_points)
            .color(palette.color(0))
            .fill_area(true)
            .endpoint_callout("Today"),
        LineSeries::new("Previous 5 workdays, average", baseline_points)
            .line_style(LineStyle::Dashed)
            .color(palette.baseline),
    ];
    let line_legend = LineChart::legend_items(&line_series, &palette);

    // Breakdown list rows (sorted descending by value)
    let mut breakdown_items: Vec<BreakdownItem> = active_models
        .iter()
        .enumerate()
        .map(|(i, &(orig_idx, m))| {
            BreakdownItem::new(m.name, model_totals_metric[i])
                .key(m.name)
                .color(palette.color(orig_idx))
        })
        .collect();
    breakdown_items.sort_by(|a, b| b.value.total_cmp(&a.value));

    let selected_model_key: Option<SharedString> = clauses
        .iter()
        .find(|c| c.field.as_ref() == "model" && c.values.len() == 1)
        .map(|c| c.values[0].clone());

    // Data table backing the StackedBarChart "Show data table" disclosure
    let daily_table_rows: Vec<TableRow> = bar_points
        .iter()
        .rev()
        .take(8)
        .map(|pt| {
            let top_share = active_models
                .first()
                .map(|(_, m)| m.name.to_string())
                .unwrap_or_else(|| "—".to_string());
            TableRow::new()
                .text(pt.label.clone())
                .text(value_format.format_detail(pt.total()))
                .text(format!("{}", pt.values.len()))
                .text(top_share)
        })
        .collect();
    let daily_data_table = Table::new(
        "analytics-daily-table",
        vec![
            TableColumn::new("Day"),
            TableColumn::new(format!("Total {metric_noun}")).align(TableAlign::Right),
            TableColumn::new("Active models").align(TableAlign::Right),
            TableColumn::new("Top model"),
        ],
    )
    .compact(true)
    .striped(true)
    .rows(daily_table_rows);

    // Data table backing the LineChart "Show data table" disclosure
    let hourly_table_rows: Vec<TableRow> = [0.0, 6.0, 12.0, 17.5, 24.0]
        .into_iter()
        .map(|hr| {
            let td = line_series[0]
                .sample_at(hr)
                .map(|y| value_format.format_detail(y))
                .unwrap_or_else(|| "—".into());
            let base = line_series[1]
                .sample_at(hr)
                .map(|y| value_format.format_detail(y))
                .unwrap_or_else(|| "—".into());
            TableRow::new()
                .text(format!(
                    "{:02}:{:02}",
                    hr.floor() as u32,
                    ((hr.fract() * 60.0).round()) as u32
                ))
                .text(td)
                .text(base)
        })
        .collect();
    let hourly_data_table = Table::new(
        "analytics-hourly-table",
        vec![
            TableColumn::new("Time"),
            TableColumn::new("Today").align(TableAlign::Right),
            TableColumn::new("5-workday avg").align(TableAlign::Right),
        ],
    )
    .compact(true)
    .striped(true)
    .rows(hourly_table_rows);

    let filter_entity = app.analytics_filter.clone();
    let accent = app.accent;
    let entity_handle = cx.entity().downgrade();

    // Header actions: TimeRangePicker + Refresh + inline radial ThemeSwitcher
    let header_actions = div()
        .flex()
        .flex_wrap()
        .items_center()
        .gap(px(8.))
        .child(time_range_picker("analytics-time-range", &app.analytics_time_range))
        .child(
            button("analytics-refresh")
                .primary()
                .size(ComponentSize::Sm)
                .leading_icon(LucideIcon::RefreshCw)
                .label("Refresh")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.analytics_refresh_seed = this.analytics_refresh_seed.wrapping_add(1);
                    this.log_event("Analytics", "Refreshed telemetry snapshot", cx);
                    this.push_toast(
                        ToastVariant::Info,
                        "Analytics refreshed",
                        Some("Updated charts & KPIs for the active range and filters."),
                        cx,
                    );
                })),
        )
        .child(
            theme_switcher("analytics-theme-switcher").on_change(move |pref, origin, window, cx| {
                let mode = Theme::resolve_mode(pref, window);
                let next_theme = accent.build_theme(mode).with_preference(pref);
                Theme::switch_with_transition(next_theme, Some(origin), window, cx);
                if let Some(app_entity) = entity_handle.upgrade() {
                    app_entity.update(cx, |this, cx| {
                        this.log_event(
                            "ThemeSwitcher",
                            format!(
                                "preference -> {pref:?} (radial from {:.0},{:.0})",
                                f32::from(origin.x),
                                f32::from(origin.y)
                            ),
                            cx,
                        );
                    });
                }
            }),
        );

    let stats_strip = stat_group("analytics-kpis").items([
        StatItem::new(format!("{span_days}-day cost"), format!("${total_cost:.2}"))
            .delta(StatDelta::percent(
                10.9,
                &format!("vs previous {span_days} days"),
                TrendPolarity::LowerIsBetter,
            )),
        StatItem::new("Today", format!("${today_cost_now:.2}")).delta(StatDelta::percent(
            -11.9,
            "vs workdays by 17:36",
            TrendPolarity::LowerIsBetter,
        )),
        StatItem::new("Tokens", format_compact(total_tokens)).delta(StatDelta::percent(
            7.4,
            &format!("vs previous {span_days} days"),
            TrendPolarity::Neutral,
        )),
        StatItem::new("Requests", format_compact(total_requests)).delta(StatDelta::percent(
            4.2,
            &format!("vs previous {span_days} days"),
            TrendPolarity::Neutral,
        )),
        StatItem::new("Cache read", "58.0%").delta(StatDelta::neutral("was 58.0% before")),
    ]);

    let toolbar_row = div()
        .w_full()
        .flex()
        .flex_wrap()
        .items_center()
        .justify_between()
        .gap(px(10.))
        .child(
            div()
                .flex()
                .flex_wrap()
                .items_center()
                .gap(px(10.))
                .child(div().w(px(148.)).child(select(&app.analytics_scope_select)))
                .child(
                    segmented_control("analytics-metric-seg", metric_key)
                        .size(ComponentSize::Sm)
                        .items([
                            SegmentedItem::new("cost", "Cost"),
                            SegmentedItem::new("tokens", "Tokens"),
                            SegmentedItem::new("requests", "Requests"),
                        ])
                        .on_change(cx.listener(|this, val: &SharedString, _, cx| {
                            this.analytics_metric = val.clone();
                            this.log_event("SegmentedControl", format!("metric -> {val}"), cx);
                        })),
                )
                .child(
                    toggle("analytics-drag-select-toggle", drag_zoom_enabled)
                        .size(ComponentSize::Sm)
                        .label("Drag to zoom")
                        .on_change(cx.listener(|this, enabled: &bool, _, cx| {
                            this.analytics_drag_zoom_enabled = *enabled;
                            if !*enabled {
                                this.analytics_line_selection = None;
                            }
                            this.log_event(
                                "Charts",
                                format!("selectable -> {}", *enabled),
                                cx,
                            );
                        })),
                ),
        )
        .child(
            div()
                .text_size(px(12.))
                .text_color(theme.muted_foreground)
                .child(if clauses.is_empty() {
                    if drag_zoom_enabled {
                        format!(
                            "Showing all {} models · drag on bars to zoom TimeRangePicker",
                            MODELS.len()
                        )
                    } else {
                        format!(
                            "Showing all {} models · click breakdown rows to narrow",
                            MODELS.len()
                        )
                    }
                } else {
                    format!(
                        "{} active filter{} · {} matching model{}",
                        clauses.len(),
                        if clauses.len() == 1 { "" } else { "s" },
                        active_models.len(),
                        if active_models.len() == 1 { "" } else { "s" }
                    )
                }),
        );

    let time_range_for_brush = app.analytics_time_range.clone();
    let time_range_for_reset = app.analytics_time_range.clone();
    let range_start_for_brush = range.start;
    let is_custom_zoom = active_preset == TimeRangePreset::Custom;

    let mut stacked_card = chart_card("card-stacked", format!("Daily {metric_noun} by model"))
        .subtitle(if drag_zoom_enabled {
            format!("{span_days} days · drag across bars to zoom range")
        } else {
            format!("{span_days} days · local time")
        })
        .legend(bar_legend)
        .on_legend_click(cx.listener(move |this, idx: &usize, _, cx| {
            if let Some(name) = legend_model_names.get(*idx) {
                if let Some(pos) = this
                    .analytics_hidden_models
                    .iter()
                    .position(|m| m == name)
                {
                    this.analytics_hidden_models.remove(pos);
                } else if this.analytics_hidden_models.len() + 1 < legend_model_names.len() {
                    this.analytics_hidden_models.push(name.clone());
                }
                this.log_event("ChartLegend", format!("toggled {name}"), cx);
            }
        }))
        .content(
            stacked_bar_chart("analytics-stacked-bar")
                .series(bar_series)
                .points(bar_points)
                .height(px(230.))
                .value_format(value_format.clone())
                .on_bar_click(cx.listener(|this, idx, _, cx| {
                    this.log_event("StackedBarChart", format!("clicked column #{idx}"), cx);
                }))
                .on_brush_select(cx.listener(move |this, sel: &std::ops::RangeInclusive<usize>, _, cx| {
                    let start_day = range_start_for_brush.add_days(*sel.start() as i64);
                    let end_day = range_start_for_brush.add_days(*sel.end() as i64);
                    time_range_for_brush.update(cx, |s, cx| {
                        s.apply_custom_range(start_day, end_day, cx);
                    });
                    let zoomed = DateRange::new(start_day, end_day);
                    this.log_event(
                        "Chart -> TimeRange",
                        format!("brushed {} ({}d)", zoomed.format_compact(), zoomed.span_days()),
                        cx,
                    );
                }))
                .selectable(drag_zoom_enabled),
        )
        .data_table(daily_data_table);

    if is_custom_zoom {
        stacked_card = stacked_card.header_action(
            button("analytics-reset-zoom")
                .soft()
                .size(ComponentSize::Xs)
                .leading_icon(LucideIcon::ZoomOut)
                .label("Reset 30d")
                .on_click(move |_, _, cx| {
                    time_range_for_reset.update(cx, |s, cx| {
                        s.apply_preset(TimeRangePreset::Last30Days, cx);
                    });
                }),
        );
    }

    let fmt_hour = |hr: f64| {
        let h = hr.floor().clamp(0.0, 24.0) as u32;
        let m = ((hr.fract() * 60.0).round() as u32).min(59);
        format!("{h:02}:{m:02}")
    };

    let line_subtitle = if let Some((a, b)) = line_selection {
        let delta_str = line_series
            .first()
            .and_then(|today| Some((today.sample_at(a)?, today.sample_at(b)?)))
            .map(|(y0, y1)| format!(" · Δ {}", value_format.format_detail((y1 - y0).max(0.0))))
            .unwrap_or_default();
        format!("Window {} – {}{delta_str}", fmt_hour(a), fmt_hour(b))
    } else if drag_zoom_enabled {
        format!("{metric_title} · cumulative · drag to measure window")
    } else {
        format!("{metric_title} · cumulative · 5-workday average")
    };

    let mut line_card = chart_card("card-line", "Today vs previous workdays")
        .subtitle(line_subtitle)
        .legend(line_legend)
        .content(
            line_chart("analytics-line-chart")
                .series(line_series)
                .x_domain(0.0, 24.0)
                .x_ticks([
                    (0.0, "00:00".into()),
                    (6.0, "06:00".into()),
                    (12.0, "12:00".into()),
                    (18.0, "18:00".into()),
                    (24.0, "24:00".into()),
                ])
                .x_formatter(fmt_hour)
                .height(px(225.))
                .value_format(value_format.clone())
                .selected_range(line_selection)
                .on_brush_select(cx.listener(|this, &(a, b): &(f64, f64), _, cx| {
                    this.analytics_line_selection = Some((a, b));
                    this.log_event(
                        "LineChart",
                        format!("selected {:.1}h – {:.1}h", a, b),
                        cx,
                    );
                }))
                .on_selection_clear(cx.listener(|this, _, _, cx| {
                    this.analytics_line_selection = None;
                    this.log_event("LineChart", "cleared selection", cx);
                }))
                .selectable(drag_zoom_enabled),
        )
        .data_table(hourly_data_table);

    if line_selection.is_some() {
        line_card = line_card.header_action(
            button("analytics-clear-line-sel")
                .ghost()
                .size(ComponentSize::Xs)
                .leading_icon(LucideIcon::X)
                .label("Clear")
                .on_click(cx.listener(|this, _, _, cx| {
                    this.analytics_line_selection = None;
                    cx.notify();
                })),
        );
    }

    let breakdown_card = chart_card("card-breakdown", format!("{metric_title} by model"))
        .subtitle(format!("{span_days} days · click to narrow"))
        .content(
            breakdown_list("analytics-breakdown")
                .items(breakdown_items)
                .value_format(value_format)
                .selected_key(selected_model_key)
                .hint(if clauses.is_empty() {
                    "Click any model to toggle a FilterBar chip."
                } else {
                    "Click the highlighted model again to clear the filter."
                })
                .on_select(move |key, _, cx| {
                    filter_entity.update(cx, |s, cx| s.toggle_narrow("model", key.clone(), cx));
                }),
        );

    let split_charts = div()
        .w_full()
        .flex_shrink_0()
        .flex()
        .flex_wrap()
        .gap(px(16.))
        .child(div().flex_1().min_w(px(460.)).child(line_card))
        .child(div().w(px(360.)).flex_grow(1.0).min_w(px(300.)).child(breakdown_card));

    v_stack()
        .w_full()
        .spacing(px(16.))
        .child(
            div()
                .w_full()
                .flex()
                .flex_wrap()
                .items_center()
                .justify_between()
                .gap(px(12.))
                .child(
                    v_stack()
                        .spacing(px(2.))
                        .child(
                            div()
                                .text_size(px(22.))
                                .font_weight(FontWeight::BOLD)
                                .text_color(theme.foreground)
                                .child("Analytics & Charts"),
                        )
                        .child(
                            div()
                                .text_size(px(13.))
                                .text_color(theme.muted_foreground)
                                .child("Faceted FilterBar, compound TimeRangePicker, GPU charts & click-origin radial ThemeSwitcher."),
                        ),
                )
                .child(header_actions),
        )
        .child(filter_bar("analytics-filter-bar", &app.analytics_filter))
        .child(toolbar_row)
        .child(stats_strip)
        .child(stacked_card)
        .child(split_charts)
}

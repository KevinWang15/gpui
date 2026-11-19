//! Reproducible release-mode performance benchmarks for `gpui::components`.
//!
//! Run with:
//! ```bash
//! cargo test --release -p gpui --lib --features components,lucide -- components::perf_tests --ignored --nocapture
//! ```

use std::cell::Cell;
use std::hint::black_box;
use std::rc::Rc;
use std::time::{Duration, Instant};

use crate::components::*;
use crate::{
    Context, Entity, Focusable, IntoElement, Modifiers, MouseButton, Render, SharedString,
    TestAppContext, Window, div, point, prelude::*, px,
};

struct TimingStats {
    median_us: f64,
    p95_us: f64,
    mean_us: f64,
    total_ms: f64,
}

fn summarize(samples: &mut [Duration]) -> TimingStats {
    samples.sort_unstable();
    let n = samples.len().max(1);
    let total_ns: u128 = samples.iter().map(Duration::as_nanos).sum();
    let median_us = samples[n / 2].as_nanos() as f64 / 1_000.0;
    let p95_idx = ((n as f64 * 0.95).ceil() as usize).saturating_sub(1).min(n - 1);
    let p95_us = samples[p95_idx].as_nanos() as f64 / 1_000.0;
    let mean_us = (total_ns as f64 / n as f64) / 1_000.0;
    let total_ms = total_ns as f64 / 1_000_000.0;
    TimingStats {
        median_us,
        p95_us,
        mean_us,
        total_ms,
    }
}

fn print_stats(label: &str, iters: usize, stats: &TimingStats) {
    println!(
        "[PERF] {label:<46} | n={iters:<4} | median={:>8.1} µs | p95={:>8.1} µs | mean={:>8.1} µs | total={:>7.2} ms",
        stats.median_us, stats.p95_us, stats.mean_us, stats.total_ms
    );
}

// ============================================================================
// 1. Dense LineChart Hover Scrub (4 series × 2,500 points = 10,000 points)
// ============================================================================

struct DenseLineChartView {
    series: Vec<LineSeries>,
}

impl Render for DenseLineChartView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().w(px(1000.)).h(px(300.)).p(px(12.)).child(
            line_chart("perf-dense-line")
                .series(self.series.clone())
                .x_domain(0.0, 24.0)
                .x_ticks([
                    (0.0, "00:00".into()),
                    (6.0, "06:00".into()),
                    (12.0, "12:00".into()),
                    (18.0, "18:00".into()),
                    (24.0, "24:00".into()),
                ])
                .height(px(260.))
                .selectable(true),
        )
    }
}

#[gpui::test]
#[ignore]
fn perf_line_chart_dense_hover_scrub(cx: &mut TestAppContext) {
    cx.update(init_components);
    let make_series = |label: &'static str, phase: f64, dashed: bool, fill_area: bool| {
        let pts: Vec<(f64, f64)> = (0..2_500)
            .map(|i| {
                let x = (i as f64 / 2_499.0) * 24.0;
                let y = 20.0 + x * 2.5 + (x * 1.3 + phase).sin() * 6.0;
                (x, y)
            })
            .collect();
        let mut s = LineSeries::new(label, pts).fill_area(fill_area);
        if dashed {
            s = s.line_style(LineStyle::Dashed);
        }
        s
    };

    let series = vec![
        make_series("Today", 0.2, false, true),
        make_series("5-workday avg", 1.1, true, false),
        make_series("P95 ceiling", 2.0, false, false),
        make_series("Budget pace", 3.1, true, false),
    ];

    let (_view, cx) = cx.add_window_view(|_, _| DenseLineChartView { series });
    cx.run_until_parked();

    // Warm up
    for i in 0..5 {
        let x = 120.0 + (i as f32) * 30.0;
        cx.simulate_mouse_move(point(px(x), px(140.)), None, Modifiers::default());
        cx.run_until_parked();
    }

    let iters = 60;
    let mut samples = Vec::with_capacity(iters);
    for i in 0..iters {
        let x = 100.0 + ((i as f32) / (iters as f32)) * 820.0;
        let t0 = Instant::now();
        cx.simulate_mouse_move(point(px(x), px(140.)), None, Modifiers::default());
        cx.run_until_parked();
        samples.push(t0.elapsed());
    }
    print_stats(
        "LineChart hover scrub (4x2500 pts)",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 2. Dense StackedBarChart Hover + Sub-Column Drag Brush (90 days × 6 series)
// ============================================================================

struct DenseStackedBarView {
    series: Vec<StackedBarSeries>,
    points: Vec<StackedBarPoint>,
    render_count: Rc<Cell<usize>>,
}

impl Render for DenseStackedBarView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_count.set(self.render_count.get() + 1);
        div().w(px(1000.)).h(px(300.)).p(px(12.)).child(
            stacked_bar_chart("perf-dense-bars")
                .series(self.series.clone())
                .points(self.points.clone())
                .height(px(260.))
                .selectable(true),
        )
    }
}

#[gpui::test]
#[ignore]
fn perf_stacked_bar_hover_and_brush(cx: &mut TestAppContext) {
    cx.update(init_components);
    let series: Vec<StackedBarSeries> = (0..6)
        .map(|i| StackedBarSeries::new(format!("model-tier-{i}")))
        .collect();
    let points: Vec<StackedBarPoint> = (0..90)
        .map(|d| {
            let vals: Vec<f64> = (0..6)
                .map(|s| 4.0 + ((d * 7 + s * 13) % 19) as f64)
                .collect();
            StackedBarPoint::new(format!("Day {}", d + 1), vals)
        })
        .collect();

    let render_count = Rc::new(Cell::new(0));
    let (_view, cx) = cx.add_window_view(|_, _| DenseStackedBarView {
        series,
        points,
        render_count: render_count.clone(),
    });
    cx.run_until_parked();

    // Hover across 60 distinct positions
    let hover_iters = 60;
    let mut hover_samples = Vec::with_capacity(hover_iters);
    for i in 0..hover_iters {
        let x = 80.0 + ((i as f32) / (hover_iters as f32)) * 880.0;
        let t0 = Instant::now();
        cx.simulate_mouse_move(point(px(x), px(140.)), None, Modifiers::default());
        cx.run_until_parked();
        hover_samples.push(t0.elapsed());
    }
    print_stats(
        "StackedBarChart hover (90d x 6 series)",
        hover_iters,
        &summarize(&mut hover_samples),
    );

    // Sub-column drag brush (2px increments across 120px = 60 pointer moves over ~12 bars)
    let renders_before = render_count.get();
    cx.simulate_mouse_down(point(px(200.), px(140.)), MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let brush_iters = 60;
    let mut brush_samples = Vec::with_capacity(brush_iters);
    for i in 1..=brush_iters {
        let x = 200.0 + (i as f32) * 2.0;
        let t0 = Instant::now();
        cx.simulate_mouse_move(
            point(px(x), px(140.)),
            Some(MouseButton::Left),
            Modifiers::default(),
        );
        cx.run_until_parked();
        brush_samples.push(t0.elapsed());
    }
    cx.simulate_mouse_up(point(px(320.), px(140.)), MouseButton::Left, Modifiers::default());
    cx.run_until_parked();
    let renders_during_brush = render_count.get() - renders_before;
    print_stats(
        &format!("StackedBarChart 2px brush (renders={renders_during_brush})"),
        brush_iters,
        &summarize(&mut brush_samples),
    );
}

// ============================================================================
// 3. FilterBar Clause Matching Throughput + Large Open Popover Render
// ============================================================================

struct DenseFilterBarView {
    filter: Entity<FilterBarState>,
}

impl Render for DenseFilterBarView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(900.))
            .h(px(500.))
            .p(px(16.))
            .child(filter_bar("perf-filter-bar", &self.filter))
    }
}

#[gpui::test]
#[ignore]
fn perf_filter_bar_matching_and_popover(cx: &mut TestAppContext) {
    cx.update(init_components);

    // Part A: Microbenchmark FilterClause::matches_value with Contains (100,000 checks)
    let clause = FilterClause {
        field: "model".into(),
        field_label: "Model".into(),
        operator: FilterOperator::Contains,
        values: smallvec::smallvec!["Sonnet".into(), "Opus".into(), "Flash".into()],
    };
    let candidates = [
        "claude-sonnet-5-5",
        "gpt-6.2-turbo",
        "deepseek-v4.1-flash",
        "cursor-composer-2",
        "gemini-3-pro",
        "claude-opus-4-6",
    ];
    let t0 = Instant::now();
    let mut matched = 0usize;
    for i in 0..100_000 {
        if clause.matches_value(black_box(candidates[i % candidates.len()])) {
            matched += 1;
        }
    }
    let match_elapsed = t0.elapsed();
    black_box(matched);
    println!(
        "[PERF] {:<46} | n=100k | mean={:>8.3} µs | total={:>7.2} ms",
        "FilterClause::matches_value (Contains)",
        match_elapsed.as_nanos() as f64 / 100_000.0 / 1_000.0,
        match_elapsed.as_secs_f64() * 1_000.0
    );

    // Part B: Render FilterBar with 10 clauses and 120 facet options while navigating
    let all_facets: Vec<FilterFacetOption> = (0..120)
        .map(|i| FilterFacetOption::new(format!("model-service-endpoint-{i:03}"), Some(500 - i)))
        .collect();
    let initial_clauses: Vec<FilterClause> = (0..10)
        .map(|i| FilterClause {
            field: format!("field_{i}").into(),
            field_label: format!("Field {i}").into(),
            operator: FilterOperator::Eq,
            values: smallvec::smallvec![format!("val-{i}").into()],
        })
        .collect();

    let (view, cx) = cx.add_window_view(|_, cx| {
        let facets = all_facets.clone();
        let filter = cx.new(|cx| {
            FilterBarState::new(
                vec![FilterFieldDef::new("model", "Model").operators([FilterOperator::OneOf])],
                move |_, _, q| {
                    if q.is_empty() {
                        facets.clone()
                    } else {
                        let q_low = q.to_lowercase();
                        facets
                            .iter()
                            .filter(|f| f.label.to_lowercase().contains(&q_low))
                            .cloned()
                            .collect()
                    }
                },
                cx,
            )
            .initial_clauses(initial_clauses)
        });
        DenseFilterBarView { filter }
    });
    cx.run_until_parked();

    // Open popover into MultiValue phase (120 checkbox rows!)
    view.update_in(cx, |v, window, cx| {
        let fh = v.filter.read(cx).focus_handle(cx);
        window.focus(&fh, cx);
    });
    cx.run_until_parked();
    cx.simulate_keystrokes("down enter");
    cx.run_until_parked();

    let iters = 40;
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        samples.push(t0.elapsed());
    }
    print_stats(
        "FilterBar open popover nav (10 chips, 120 opts)",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 4. Settings List with 120 Styled-Passthrough Inputs (Parent Re-render)
// ============================================================================

struct DenseSettingsView {
    inputs: Vec<Entity<InputState>>,
    toggle_flag: bool,
}

impl Render for DenseSettingsView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut list = v_stack().w(px(800.)).divided().spacing(px(0.));
        for (i, inp) in self.inputs.iter().enumerate() {
            list = list.child(
                setting_row(format!("Setting option {i}"))
                    .description("Configure shortcut or threshold value")
                    .control(input(inp).w(px(84.)).h(px(30.)).rounded(px(6.))),
            );
        }
        v_stack()
            .w_full()
            .p(px(16.))
            .child(toggle("perf-parent-flag", self.toggle_flag).label("Enabled"))
            .child(card().flat().child(list))
    }
}

#[gpui::test]
#[ignore]
fn perf_settings_list_120_inputs_rerender(cx: &mut TestAppContext) {
    cx.update(init_components);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let inputs = (0..120)
            .map(|i| cx.new(|cx| InputState::new(cx).default_value(format!("{i}")).size(ComponentSize::Sm)))
            .collect();
        DenseSettingsView {
            inputs,
            toggle_flag: false,
        }
    });
    cx.run_until_parked();

    // Warm up
    for _ in 0..3 {
        view.update(cx, |v, cx| {
            v.toggle_flag = !v.toggle_flag;
            cx.notify();
        });
        cx.run_until_parked();
    }

    let iters = 50;
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.toggle_flag = !v.toggle_flag;
            cx.notify();
        });
        cx.run_until_parked();
        samples.push(t0.elapsed());
    }
    print_stats(
        "Settings parent rerender (120 Styled Inputs)",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 5. CalendarDate Math + TimeRangePicker Calendar Month Navigation
// ============================================================================

struct CalendarPerfView {
    month: CalendarDate,
    selected: CalendarDate,
    range: DateRange,
}

impl Render for CalendarPerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div().p(px(16.)).child(
            calendar_grid("perf-cal", self.month)
                .selected(Some(self.selected))
                .highlighted_range(Some(self.range)),
        )
    }
}

#[gpui::test]
#[ignore]
fn perf_time_range_date_math_and_calendar(cx: &mut TestAppContext) {
    cx.update(init_components);
    let anchor = CalendarDate::new(2026, 11, 19);
    let t0 = Instant::now();
    let mut acc = 0i64;
    for i in 0..200_000i64 {
        let d = anchor.add_days(i % 3650 - 1825);
        acc = acc.wrapping_add(i64::from(d.weekday_mon0()) + d.to_days_since_epoch());
    }
    let math_elapsed = t0.elapsed();
    black_box(acc);
    println!(
        "[PERF] {:<46} | n=200k | mean={:>8.3} µs | total={:>7.2} ms",
        "CalendarDate epoch+add_days+weekday",
        math_elapsed.as_nanos() as f64 / 200_000.0 / 1_000.0,
        math_elapsed.as_secs_f64() * 1_000.0
    );

    let (view, cx) = cx.add_window_view(|_, _| CalendarPerfView {
        month: anchor.first_of_month(),
        selected: anchor,
        range: DateRange::last_days(anchor, 30),
    });
    cx.run_until_parked();

    let iters = 60;
    let mut samples = Vec::with_capacity(iters);
    for i in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.month = if i % 2 == 0 {
                v.month.prev_month()
            } else {
                v.month.next_month()
            };
            cx.notify();
        });
        cx.run_until_parked();
        samples.push(t0.elapsed());
    }
    print_stats(
        "CalendarGrid month navigation (42 cells)",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 6. Full Analytics Dashboard Composite Re-render
// ============================================================================

struct FullAnalyticsPerfView {
    filter: Entity<FilterBarState>,
    time_range: Entity<TimeRangePickerState>,
    tick: usize,
}

impl Render for FullAnalyticsPerfView {
    fn render(&mut self, _window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let range = self.time_range.read(cx).range();
        let shift = (self.tick % 5) as f64;
        let bar_points: Vec<StackedBarPoint> = (0..30)
            .map(|d| {
                let day = range.start.add_days(d);
                StackedBarPoint::new(
                    day.format_short(),
                    vec![14.0 + shift, 8.0 + (d % 5) as f64, 4.0, 1.5],
                )
            })
            .collect();
        let line_pts_a: Vec<(f64, f64)> = (0..200)
            .map(|i| {
                let x = i as f64 * 0.12;
                (x, x * 1.8 + shift)
            })
            .collect();
        let line_pts_b: Vec<(f64, f64)> = (0..200)
            .map(|i| {
                let x = i as f64 * 0.12;
                (x, x * 2.05)
            })
            .collect();

        v_stack()
            .w(px(1200.))
            .p(px(16.))
            .spacing(px(12.))
            .child(
                h_stack()
                    .spacing(px(8.))
                    .child(time_range_picker("p-tr", &self.time_range))
                    .child(theme_switcher("p-ts"))
                    .child(
                        segmented_control("p-seg", SharedString::from("cost")).items([
                            SegmentedItem::new("cost", "Cost"),
                            SegmentedItem::new("tokens", "Tokens"),
                            SegmentedItem::new("requests", "Requests"),
                        ]),
                    ),
            )
            .child(filter_bar("p-fb", &self.filter))
            .child(stat_group("p-stats").items([
                StatItem::new("30-day cost", "$896.90").delta(StatDelta::percent(
                    10.9,
                    "vs prev 30d",
                    TrendPolarity::LowerIsBetter,
                )),
                StatItem::new("Today", "$36.21").delta(StatDelta::percent(
                    -11.9,
                    "vs workdays",
                    TrendPolarity::LowerIsBetter,
                )),
                StatItem::new("Tokens", "36.8M").delta(StatDelta::neutral("steady")),
                StatItem::new("Requests", "19.7K").delta(StatDelta::neutral("+4.2%")),
                StatItem::new("Cache read", "58.0%").delta(StatDelta::neutral("was 58.0%")),
            ]))
            .child(
                chart_card("p-bar-card", "Daily cost by model")
                    .subtitle("30 days")
                    .legend([
                        ChartLegendItem::dot("sonnet", palette::INDIGO_500.into()),
                        ChartLegendItem::dot("gpt", palette::EMERALD_500.into()),
                        ChartLegendItem::dot("deepseek", palette::AMBER_500.into()),
                        ChartLegendItem::dot("cursor", palette::SKY_500.into()),
                    ])
                    .content(
                        stacked_bar_chart("p-bars")
                            .series([
                                StackedBarSeries::new("sonnet"),
                                StackedBarSeries::new("gpt"),
                                StackedBarSeries::new("deepseek"),
                                StackedBarSeries::new("cursor"),
                            ])
                            .points(bar_points)
                            .selectable(true),
                    ),
            )
            .child(
                h_stack()
                    .w_full()
                    .spacing(px(12.))
                    .child(
                        div().flex_1().child(
                            chart_card("p-line-card", "Today vs workdays").content(
                                line_chart("p-lines")
                                    .x_domain(0.0, 24.0)
                                    .x_ticks([
                                        (0.0, "00:00".into()),
                                        (12.0, "12:00".into()),
                                        (24.0, "24:00".into()),
                                    ])
                                    .add_series(
                                        LineSeries::new("Today", line_pts_a)
                                            .fill_area(true)
                                            .endpoint_callout("Today"),
                                    )
                                    .add_series(
                                        LineSeries::new("Baseline", line_pts_b)
                                            .line_style(LineStyle::Dashed),
                                    ),
                            ),
                        ),
                    )
                    .child(
                        div().w(px(340.)).child(
                            chart_card("p-bd-card", "Cost by model").content(
                                breakdown_list("p-bd").items([
                                    BreakdownItem::new("claude-sonnet-5-5", 520.2),
                                    BreakdownItem::new("gpt-6.2", 269.1),
                                    BreakdownItem::new("deepseek-v4.1-flash", 107.7),
                                    BreakdownItem::new("cursor-composer-2", 43.5),
                                ]),
                            ),
                        ),
                    ),
            )
    }
}

#[gpui::test]
#[ignore]
fn perf_full_analytics_dashboard_rerender(cx: &mut TestAppContext) {
    cx.update(init_components);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let filter = cx.new(|cx| {
            FilterBarState::new(
                vec![FilterFieldDef::new("model", "Model")],
                |_, _, _| vec![FilterFacetOption::new("claude-sonnet-5-5", Some(309))],
                cx,
            )
        });
        let time_range =
            cx.new(|cx| TimeRangePickerState::new(CalendarDate::new(2026, 11, 19), cx));
        FullAnalyticsPerfView {
            filter,
            time_range,
            tick: 0,
        }
    });
    cx.run_until_parked();

    // Warm up
    for _ in 0..5 {
        view.update(cx, |v, cx| {
            v.tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
    }

    let iters = 60;
    let mut samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
        samples.push(t0.elapsed());
    }
    print_stats(
        "Full Analytics dashboard composite rerender",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 8. Zero-Relayout GPU Scene QuadAnimation vs. Full View Relayout (60 frames)
// ============================================================================

#[gpui::test]
#[ignore]
fn perf_zero_relayout_quad_animation_vs_full_rerender(cx: &mut TestAppContext) {
    cx.update(init_components);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let inputs = (0..120)
            .map(|i| {
                cx.new(|cx| {
                    InputState::new(cx)
                        .default_value(format!("{i}"))
                        .size(ComponentSize::Sm)
                })
            })
            .collect();
        DenseSettingsView {
            inputs,
            toggle_flag: false,
        }
    });
    cx.run_until_parked();

    // Flip toggle once (single layout frame arms zero-relayout GPU QuadAnimation!)
    view.update(cx, |v, cx| {
        v.toggle_flag = true;
        cx.notify();
    });
    cx.run_until_parked();

    // Measure 60 zero-relayout scene animation frames (window.advance_scene_animations)
    let frames = 60usize;
    let mut anim_samples = Vec::with_capacity(frames);
    for _ in 0..frames {
        let t0 = Instant::now();
        cx.update(|window, cx| {
            window.advance_scene_animations(cx);
        });
        anim_samples.push(t0.elapsed());
    }
    print_stats(
        "GPU Scene QuadAnimation advance (120-row page)",
        frames,
        &summarize(&mut anim_samples),
    );
}

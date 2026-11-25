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
use crate::icons::LucideIcon;
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
// 1. Dense Line CartesianPlot Hover Scrub (4 series × 2,500 points = 10,000 points)
// ============================================================================

struct DenseLineChartView {
    layers: Vec<LineLayer>,
}

impl Render for DenseLineChartView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let mut plot = cartesian_plot("perf-dense-line")
            .x_axis(
                XAxis::continuous()
                    .domain(0.0, 24.0)
                    .ticks([
                        (0.0, "00:00".into()),
                        (6.0, "06:00".into()),
                        (12.0, "12:00".into()),
                        (18.0, "18:00".into()),
                        (24.0, "24:00".into()),
                    ]),
            )
            .height(px(260.))
            .selectable(true);
        for layer in &self.layers {
            plot = plot.layer(layer.clone());
        }
        div().w(px(1000.)).h(px(300.)).p(px(12.)).child(plot)
    }
}

#[gpui::test]
#[ignore]
fn perf_line_chart_dense_hover_scrub(cx: &mut TestAppContext) {
    cx.update(init_components);
    let make_layer = |label: &'static str, phase: f64, dashed: bool, fill_area: bool| {
        let pts: Vec<(f64, f64)> = (0..2_500)
            .map(|i| {
                let x = (i as f64 / 2_499.0) * 24.0;
                let y = 20.0 + x * 2.5 + (x * 1.3 + phase).sin() * 6.0;
                (x, y)
            })
            .collect();
        let mut s = LineLayer::new(label, pts).fill_area(fill_area);
        if dashed {
            s = s.dashed();
        }
        s
    };

    let layers = vec![
        make_layer("Today", 0.2, false, true),
        make_layer("5-workday avg", 1.1, true, false),
        make_layer("P95 ceiling", 2.0, false, false),
        make_layer("Budget pace", 3.1, true, false),
    ];

    let (_view, cx) = cx.add_window_view(|_, _| DenseLineChartView { layers });
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
        "CartesianPlot line hover (4x2500 pts)",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 2. Dense StackedBar CartesianPlot Hover + Sub-Column Drag Brush (90 days × 6 series)
// ============================================================================

struct DenseStackedBarView {
    labels: Vec<SharedString>,
    series: Vec<StackedBarSeries>,
    columns: Vec<Vec<f64>>,
    render_count: Rc<Cell<usize>>,
}

impl Render for DenseStackedBarView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        self.render_count.set(self.render_count.get() + 1);
        div().w(px(1000.)).h(px(300.)).p(px(12.)).child(
            cartesian_plot("perf-dense-bars")
                .x_axis(XAxis::category(self.labels.clone()))
                .layer(StackedBarLayer::new(
                    self.series.clone(),
                    self.columns.clone(),
                ))
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
    let labels: Vec<SharedString> = (0..90).map(|d| format!("Day {}", d + 1).into()).collect();
    let columns: Vec<Vec<f64>> = (0..90)
        .map(|d| {
            (0..6)
                .map(|s| 4.0 + ((d * 7 + s * 13) % 19) as f64)
                .collect()
        })
        .collect();

    let render_count = Rc::new(Cell::new(0));
    let (_view, cx) = cx.add_window_view(|_, _| DenseStackedBarView {
        labels,
        series,
        columns,
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
        let bar_labels: Vec<SharedString> = (0..30)
            .map(|d| range.start.add_days(d).format_short().into())
            .collect();
        let bar_columns: Vec<Vec<f64>> = (0..30)
            .map(|d| vec![14.0 + shift, 8.0 + (d % 5) as f64, 4.0, 1.5])
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
                        cartesian_plot("p-bars")
                            .x_axis(XAxis::category(bar_labels))
                            .layer(StackedBarLayer::new(
                                [
                                    StackedBarSeries::new("sonnet"),
                                    StackedBarSeries::new("gpt"),
                                    StackedBarSeries::new("deepseek"),
                                    StackedBarSeries::new("cursor"),
                                ],
                                bar_columns,
                            ))
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
                                cartesian_plot("p-lines")
                                    .x_axis(
                                        XAxis::continuous()
                                            .domain(0.0, 24.0)
                                            .ticks([
                                                (0.0, "00:00".into()),
                                                (12.0, "12:00".into()),
                                                (24.0, "24:00".into()),
                                            ]),
                                    )
                                    .layer(LineLayer::new("Baseline", line_pts_b).dashed())
                                    .layer(
                                        LineLayer::new("Today", line_pts_a)
                                            .fill_area(true)
                                            .endpoint_callout("Today"),
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

// ============================================================================
// 9. Tree (1,200-node 4-level hierarchy: render, filter, check cascade)
// ============================================================================

struct TreePerfView {
    tree: Entity<TreeState>,
    tick: usize,
}

impl Render for TreePerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(460.))
            .h(px(640.))
            .p(px(12.))
            .child(tree(&self.tree))
    }
}

fn build_perf_tree_nodes(roots: usize, mid: usize, leaves: usize) -> Vec<TreeNode> {
    (0..roots)
        .map(|r| {
            let children: Vec<TreeNode> = (0..mid)
                .map(|m| {
                    let grand: Vec<TreeNode> = (0..leaves)
                        .map(|l| {
                            TreeNode::new(
                                format!("n-{r}-{m}-{l}"),
                                format!("module_{r}_{m}_service_{l}.rs"),
                            )
                        })
                        .collect();
                    TreeNode::new(format!("b-{r}-{m}"), format!("subsystem_{r}_{m}"))
                        .children(grand)
                })
                .collect();
            TreeNode::new(format!("r-{r}"), format!("workspace_{r}")).children(children)
        })
        .collect()
}

#[gpui::test]
#[ignore]
fn perf_tree_large_hierarchy(cx: &mut TestAppContext) {
    cx.update(init_components);
    // 6 * (1 + 8 * (1 + 12)) = 630 nodes, all expanded
    let nodes = build_perf_tree_nodes(6, 8, 12);
    let mut expanded_ids: Vec<SharedString> = Vec::new();
    for r in 0..6 {
        expanded_ids.push(format!("r-{r}").into());
        for m in 0..8 {
            expanded_ids.push(format!("b-{r}-{m}").into());
        }
    }

    let (view, cx) = cx.add_window_view(|_, cx| {
        let t = cx.new(|cx| {
            TreeState::new(nodes, cx)
                .checkable(true)
                .searchable(true)
                .default_expanded(expanded_ids)
        });
        TreePerfView { tree: t, tick: 0 }
    });
    cx.run_until_parked();

    let iters = 30;
    let mut render_samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
        render_samples.push(t0.elapsed());
    }
    print_stats(
        "Tree full render (630 expanded checkable)",
        iters,
        &summarize(&mut render_samples),
    );

    let mut toggle_samples = Vec::with_capacity(iters);
    for i in 0..iters {
        let key: SharedString = format!("r-{}", i % 6).into();
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.tree.update(cx, |t, cx| t.toggle_check(&key, cx));
        });
        cx.run_until_parked();
        toggle_samples.push(t0.elapsed());
    }
    print_stats(
        "Tree root cascade toggle_check + render",
        iters,
        &summarize(&mut toggle_samples),
    );
}

// ============================================================================
// 10. ColorPicker inline render & rapid HSV updates
// ============================================================================

struct ColorPickerPerfView {
    picker: Entity<ColorPickerState>,
}

impl Render for ColorPickerPerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(360.))
            .h(px(460.))
            .p(px(12.))
            .child(color_picker(&self.picker))
    }
}

#[gpui::test]
#[ignore]
fn perf_color_picker_drag_and_render(cx: &mut TestAppContext) {
    cx.update(init_components);
    let (view, cx) = cx.add_window_view(|_, cx| {
        let picker = cx.new(|cx| ColorPickerState::new(cx).inline(true));
        ColorPickerPerfView { picker }
    });
    cx.run_until_parked();

    let iters = 60;
    let mut samples = Vec::with_capacity(iters);
    for i in 0..iters {
        let h = (i as f32 * 6.0) % 360.0;
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.picker.update(cx, |p, cx| {
                p.set_color(crate::hsla(h / 360.0, 0.8, 0.5, 0.85), cx);
            });
        });
        cx.run_until_parked();
        samples.push(t0.elapsed());
    }
    print_stats(
        "ColorPicker inline set_color + render",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 11. QRCode (V6 100-byte URL) & Watermark (1400x900) render
// ============================================================================

struct QrWatermarkPerfView {
    payload: SharedString,
    tick: usize,
    mode_watermark: bool,
}

impl Render for QrWatermarkPerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        if self.mode_watermark {
            div().w(px(1400.)).h(px(900.)).child(
                watermark("CONFIDENTIAL — INTERNAL")
                    .subtext("2026-11-20 • AUDIT")
                    .child(div().size_full().p(px(24.)).child(format!("Frame {}", self.tick))),
            )
        } else {
            div()
                .w(px(320.))
                .h(px(320.))
                .p(px(16.))
                .child(qr_code(self.payload.clone()).size(px(220.)))
        }
    }
}

#[gpui::test]
#[ignore]
fn perf_qrcode_and_watermark_render(cx: &mut TestAppContext) {
    cx.update(init_components);
    let url: SharedString =
        "https://example.org/auth/verify?token=9f8e7d6c5b4a3210fedcba9876543210&tenant=gpui-desktop"
            .into();

    // Pure QR encode microbenchmark
    let t0 = Instant::now();
    let mut modules = 0usize;
    for _ in 0..500 {
        let m = QrMatrix::encode(black_box(url.as_bytes()));
        modules += m.size;
    }
    let enc_elapsed = t0.elapsed();
    black_box(modules);
    println!(
        "[PERF] {:<46} | n=500  | mean={:>8.1} µs | total={:>7.2} ms",
        "QrMatrix::encode (V6 ~90B payload)",
        enc_elapsed.as_nanos() as f64 / 500.0 / 1_000.0,
        enc_elapsed.as_secs_f64() * 1_000.0
    );

    // QRCode frame render
    let (view, cx) = cx.add_window_view(|_, _| QrWatermarkPerfView {
        payload: url,
        tick: 0,
        mode_watermark: false,
    });
    cx.run_until_parked();

    let iters = 60;
    let mut qr_samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
        qr_samples.push(t0.elapsed());
    }
    print_stats(
        "QRCode V6 full frame render",
        iters,
        &summarize(&mut qr_samples),
    );

    // Watermark frame render (1400x900)
    view.update(cx, |v, cx| {
        v.mode_watermark = true;
        cx.notify();
    });
    cx.run_until_parked();

    let mut wm_samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
        wm_samples.push(t0.elapsed());
    }
    print_stats(
        "Watermark 1400x900 full frame render",
        iters,
        &summarize(&mut wm_samples),
    );
}

// ============================================================================
// 12. VirtualTable & VirtualList (100,000 rows/items)
// ============================================================================

struct VirtualDataPerfView {
    tick: usize,
    use_list: bool,
}

impl Render for VirtualDataPerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let tick = self.tick;
        if self.use_list {
            div().w(px(640.)).h(px(420.)).p(px(12.)).child(
                virtual_list("perf-vlist", 100_000, move |i, _window, _cx| {
                    div()
                        .px(px(12.))
                        .h(px(34.))
                        .flex()
                        .items_center()
                        .child(format!("Virtual item #{i} (tick {tick})"))
                        .into_any_element()
                })
                .viewport_height(px(360.)),
            )
        } else {
            div().w(px(760.)).h(px(420.)).p(px(12.)).child(
                virtual_table(
                    "perf-vtable",
                    vec![
                        TableColumn::new("ID").width(px(100.)),
                        TableColumn::new("Service"),
                        TableColumn::new("Latency")
                            .width(px(100.))
                            .align(TableAlign::Right),
                    ],
                    100_000,
                    move |i, _window, _cx| {
                        vec![
                            format!("row-{i}").into_any_element(),
                            format!("Service {i} ({tick})").into_any_element(),
                            format!("{} ms", (i * 17) % 400).into_any_element(),
                        ]
                    },
                )
                .viewport_height(px(340.)),
            )
        }
    }
}

#[gpui::test]
#[ignore]
fn perf_virtual_table_and_list_100k(cx: &mut TestAppContext) {
    cx.update(init_components);
    let (view, cx) = cx.add_window_view(|_, _| VirtualDataPerfView {
        tick: 0,
        use_list: false,
    });
    cx.run_until_parked();

    let iters = 60;
    let mut vt_samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
        vt_samples.push(t0.elapsed());
    }
    print_stats(
        "VirtualTable render (100,000 rows)",
        iters,
        &summarize(&mut vt_samples),
    );

    view.update(cx, |v, cx| {
        v.use_list = true;
        cx.notify();
    });
    cx.run_until_parked();

    let mut vl_samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        view.update(cx, |v, cx| {
            v.tick += 1;
            cx.notify();
        });
        cx.run_until_parked();
        vl_samples.push(t0.elapsed());
    }
    print_stats(
        "VirtualList render (100,000 items)",
        iters,
        &summarize(&mut vl_samples),
    );
}

// ============================================================================
// 13. Transfer (600 items), CommandPalette (1,000 cmds), Menu (300 items)
// ============================================================================

struct TransferPerfView {
    transfer: Entity<TransferState>,
    tick: usize,
}

impl Render for TransferPerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(760.))
            .h(px(420.))
            .p(px(12.))
            .child(transfer(&self.transfer))
    }
}

#[gpui::test]
#[ignore]
fn perf_transfer_600_items(cx: &mut TestAppContext) {
    cx.update(init_components);
    let items: Vec<TransferItem> = (0..600)
        .map(|i| {
            TransferItem::new(format!("perm-{i}"), format!("Permission Scope {i}"))
                .description(format!("Allows access to resource group {}", i % 24))
        })
        .collect();
    let targets: Vec<SharedString> = (0..300).map(|i| format!("perm-{}", i * 2).into()).collect();

    let (view, cx) = cx.add_window_view(|_, cx| {
        let tr = cx.new(|cx| TransferState::new(items, targets, cx));
        TransferPerfView {
            transfer: tr,
            tick: 0,
        }
    });
    cx.run_until_parked();

    let iters = 30;
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
        "Transfer render (600 items, 300/300)",
        iters,
        &summarize(&mut samples),
    );
}

struct CommandPalettePerfView {
    palette: Entity<CommandPaletteState>,
}

impl Render for CommandPalettePerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(1000.))
            .h(px(700.))
            .child(command_palette(&self.palette))
    }
}

#[gpui::test]
#[ignore]
fn perf_command_palette_1000_commands(cx: &mut TestAppContext) {
    cx.update(init_components);
    let groups = [
        "Navigation",
        "Workspace",
        "Git",
        "Debug",
        "Terminal",
        "Settings",
        "View",
        "Search",
        "Refactor",
        "Help",
    ];
    let items: Vec<CommandItem> = (0..1_000)
        .map(|i| {
            let g = groups[i % groups.len()];
            CommandItem::new(format!("cmd-{i}"), format!("{g}: Execute Action #{i}"))
                .subtitle(format!("Runs workflow pipeline task {i} in {g}"))
                .group(g)
                .keywords(format!("run exec {g} action task {i}"))
        })
        .collect();

    let (view, cx) = cx.add_window_view(|window, cx| {
        let p = cx.new(|cx| CommandPaletteState::new(items, cx));
        p.update(cx, |p, cx| p.open(window, cx));
        CommandPalettePerfView { palette: p }
    });
    cx.run_until_parked();

    // Filter with a query then measure 40 Arrow-Down navigation renders
    cx.simulate_keystrokes("exec 4");
    cx.run_until_parked();

    let iters = 40;
    let mut nav_samples = Vec::with_capacity(iters);
    for _ in 0..iters {
        let t0 = Instant::now();
        cx.simulate_keystrokes("down");
        cx.run_until_parked();
        nav_samples.push(t0.elapsed());
    }
    let _ = view;
    print_stats(
        "CommandPalette nav (1000 cmds, filtered)",
        iters,
        &summarize(&mut nav_samples),
    );
}

struct MenuPerfView {
    menu: Entity<MenuState>,
    tick: usize,
}

impl Render for MenuPerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        div()
            .w(px(300.))
            .h(px(700.))
            .p(px(8.))
            .child(nav_menu(&self.menu))
    }
}

#[gpui::test]
#[ignore]
fn perf_menu_deep_nested(cx: &mut TestAppContext) {
    cx.update(init_components);
    let mut open_subs: Vec<SharedString> = Vec::new();
    let nodes: Vec<MenuNode> = (0..8)
        .map(|g| {
            open_subs.push(format!("g-{g}").into());
            let subs: Vec<MenuNode> = (0..5)
                .map(|s| {
                    open_subs.push(format!("s-{g}-{s}").into());
                    let items: Vec<MenuNode> = (0..6)
                        .map(|i| MenuNode::item(format!("i-{g}-{s}-{i}"), format!("Endpoint {g}.{s}.{i}")))
                        .collect();
                    MenuNode::submenu(format!("s-{g}-{s}"), format!("Group {g}.{s}"), items)
                })
                .collect();
            MenuNode::submenu(format!("g-{g}"), format!("Domain {g}"), subs)
        })
        .collect();

    let (view, cx) = cx.add_window_view(|_, cx| {
        let m = cx.new(|cx| {
            MenuState::new(nodes, cx)
                .default_open(open_subs)
                .default_active("i-7-4-5")
        });
        MenuPerfView { menu: m, tick: 0 }
    });
    cx.run_until_parked();

    let iters = 40;
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
        "Menu deep nested render (288 nodes)",
        iters,
        &summarize(&mut samples),
    );
}

// ============================================================================
// 14. CodeBlock (400 lines) & TruncatedText (50KB) + FormState (100 fields)
// ============================================================================

struct TypoPerfView {
    code: SharedString,
    big_text: SharedString,
    tick: usize,
}

impl Render for TypoPerfView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        v_stack()
            .w(px(720.))
            .p(px(12.))
            .spacing(px(12.))
            .child(truncated_text("perf-trunc", self.big_text.clone(), 120))
            .child(
                code_block("perf-code", self.code.clone())
                    .language(format!("rust (rev {})", self.tick)),
            )
    }
}

#[gpui::test]
#[ignore]
fn perf_typography_and_form_validation(cx: &mut TestAppContext) {
    cx.update(init_components);
    let code: SharedString = (0..400)
        .map(|i| format!("pub fn compute_metric_{i}(input: f64) -> f64 {{ input * 1.{i:03} }}"))
        .collect::<Vec<_>>()
        .join("\n")
        .into();
    let big_text: SharedString = "GPUI native desktop UI architecture with hardware acceleration. "
        .repeat(800)
        .into();

    let (view, cx) = cx.add_window_view(|_, _| TypoPerfView {
        code,
        big_text,
        tick: 0,
    });
    cx.run_until_parked();

    let iters = 30;
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
        "CodeBlock(400L) + TruncatedText(50KB)",
        iters,
        &summarize(&mut samples),
    );

    // FormState 100-field validation microbenchmark
    let mut form = FormState::new();
    for i in 0..100 {
        let name = format!("field_{i}");
        let mut rules = vec![FormRule::Required("Required".into())];
        let initial = if i % 3 == 0 {
            rules.push(FormRule::Email("Bad email".into()));
            format!("user{i}@example.com")
        } else if i % 3 == 1 {
            rules.push(FormRule::MinLength(4, "Short".into()));
            format!("value_{i}")
        } else {
            rules.push(FormRule::NumericRange(0.0, 500.0, "Range".into()));
            format!("{}", i * 2)
        };
        form = form.field(name, initial, rules);
    }
    let t0 = Instant::now();
    let mut ok_count = 0usize;
    for _ in 0..2_000 {
        if black_box(form.validate_all()) {
            ok_count += 1;
        }
    }
    let elapsed = t0.elapsed();
    black_box(ok_count);
    println!(
        "[PERF] {:<46} | n=2000 | mean={:>8.1} µs | total={:>7.2} ms",
        "FormState::validate_all (100 fields)",
        elapsed.as_nanos() as f64 / 2_000.0 / 1_000.0,
        elapsed.as_secs_f64() * 1_000.0
    );
}

// ============================================================================
// 15. Deep Multi-Component Release Soak Stress Test (1,000 deterministic steps)
// ============================================================================

struct DeepSoakView {
    tree_st: Entity<TreeState>,
    picker: Entity<ColorPickerState>,
    slider_st: Entity<SliderState>,
    range_st: Entity<RangeSliderState>,
    date_st: Entity<DatePickerState>,
    time_st: Entity<TimePickerState>,
    xfer: Entity<TransferState>,
    tags: Entity<TagInputState>,
    img_prev: Entity<ImagePreviewState>,
    tree_sel: Entity<TreeSelectState>,
    casc: Entity<CascaderState>,
    tour_st: Entity<TourState>,
    vmenu: Entity<MenuState>,
    hmenu: Entity<MenuState>,
    car: Entity<CarouselState>,
    palette: Entity<CommandPaletteState>,
    split_st: Entity<SplitPaneState>,
    num_st: Entity<NumberInputState>,
    pin_st: Entity<PinInputState>,
    viewport_w: f32,
    viewport_h: f32,
    drawer_open: bool,
    loading: bool,
    qr_payload: SharedString,
    code_text: SharedString,
    tick: usize,
}

impl Render for DeepSoakView {
    fn render(&mut self, _window: &mut Window, _cx: &mut Context<Self>) -> impl IntoElement {
        let rows: Vec<TableRow> = (0..10)
            .map(|r| {
                TableRow::new()
                    .cell(format!("row-{}-{}", self.tick, r))
                    .cell(format!("{:.2}", r as f32 * 2.5))
            })
            .collect();

        crate::div()
            .relative()
            .w(px(self.viewport_w))
            .h(px(self.viewport_h))
            .overflow_hidden()
            .child(
                loading_overlay(self.loading)
                    .id("soak-loader")
                    .label("Soak Loading")
                    .child(
                        watermark("SOAK TEST")
                            .subtext("GPUI")
                            .child(
                                v_stack()
                                    .w_full()
                                    .p(px(8.))
                                    .spacing(px(6.))
                                    .child(nav_menu(&self.hmenu))
                                    .child(nav_menu(&self.vmenu))
                                    .child(tree(&self.tree_st))
                                    .child(color_picker(&self.picker))
                                    .child(slider(&self.slider_st))
                                    .child(range_slider(&self.range_st))
                                    .child(date_picker(&self.date_st))
                                    .child(time_picker(&self.time_st))
                                    .child(transfer(&self.xfer))
                                    .child(tag_input(&self.tags))
                                    .child(tree_select(&self.tree_sel))
                                    .child(cascader(&self.casc))
                                    .child(number_input(&self.num_st))
                                    .child(pin_input(&self.pin_st))
                                    .child(
                                        crate::div().w_full().h(px(90.)).child(
                                            split_pane("soak-split", &self.split_st)
                                                .first(crate::div().child("Left"))
                                                .second(crate::div().child("Right")),
                                        ),
                                    )
                                    .child(
                                        carousel("soak-car", &self.car)
                                            .slide(crate::div().child("S0"))
                                            .slide(crate::div().child("S1"))
                                            .slide(crate::div().child("S2")),
                                    )
                                    .child(
                                        stepper(
                                            "soak-stepper",
                                            vec![
                                                StepItem::new("Init"),
                                                StepItem::new("Run").description("Soak cycle"),
                                                StepItem::new("Verify"),
                                            ],
                                            self.tick % 3,
                                        )
                                        .orientation(if self.tick.is_multiple_of(2) {
                                            StepperOrientation::Horizontal
                                        } else {
                                            StepperOrientation::Vertical
                                        }),
                                    )
                                    .child(
                                        descriptions("soak-desc")
                                            .columns(3)
                                            .bordered(true)
                                            .item(DescriptionItem::new("Tick", format!("{}", self.tick)))
                                            .item(DescriptionItem::new("Status", "Healthy")),
                                    )
                                    .child(
                                        timeline().item(
                                            TimelineItem::new("Event")
                                                .timestamp("12:00:00"),
                                        ),
                                    )
                                    .child(
                                        table(
                                            "soak-tbl",
                                            vec![
                                                TableColumn::new("Name"),
                                                TableColumn::new("Score").align(TableAlign::Right),
                                            ],
                                        )
                                        .rows(rows),
                                    )
                                    .child(
                                        code_block("soak-code", self.code_text.clone())
                                            .language("rust"),
                                    )
                                    .child(truncated_text("soak-trunc", self.code_text.clone(), 48))
                                    .child(
                                        qr_code(self.qr_payload.clone())
                                            .ecc_level(QrEccLevel::High)
                                            .icon(LucideIcon::ShieldCheck),
                                    )
                                    .child(
                                        tour_target(
                                            &self.tour_st,
                                            "soak-target",
                                            context_menu(
                                                "soak-ctx",
                                                popconfirm(
                                                    "soak-pop",
                                                    "Proceed?",
                                                    button("soak-pop-btn").label("Confirm"),
                                                ),
                                            )
                                            .entry(MenuEntry::item("Inspect", |_, _, _| {})),
                                        ),
                                    )
                                    .child(image_preview(&self.img_prev)),
                            ),
                    ),
            )
            .child(
                float_button_group("soak-fab", LucideIcon::Plus)
                    .action(float_button("soak-fab-1", LucideIcon::Bell)),
            )
            .child(command_palette(&self.palette))
            .child(tour(&self.tour_st))
            .child(
                drawer("soak-drawer", self.drawer_open)
                    .title("Soak Drawer")
                    .on_close(|_, _, _| {}),
            )
    }
}

#[gpui::test]
#[ignore]
fn soak_all_components_deep_release_stress(cx: &mut TestAppContext) {
    cx.update(init_components);

    let make_tree_nodes = |count: usize| -> Vec<TreeNode> {
        (0..count)
            .map(|i| {
                TreeNode::new(format!("g-{i}"), format!("Group {i}")).children(
                    (0..4).map(|j| TreeNode::new(format!("n-{i}-{j}"), format!("Item {i}.{j}"))),
                )
            })
            .collect()
    };

    let (view, cx) = cx.add_window_view(|_, cx| {
        let tree_st = cx.new(|cx| {
            TreeState::new(make_tree_nodes(10), cx)
                .checkable(true)
                .searchable(true)
                .default_expanded(["g-0", "g-1"])
        });
        let picker = cx.new(|cx| {
            ColorPickerState::new(cx).default_color(crate::rgba(0x3b82f6ff), cx)
        });
        let slider_st = cx.new(|cx| SliderState::new(cx).range(0.0, 100.0).step(1.0));
        let range_st = cx.new(|cx| {
            RangeSliderState::new(cx)
                .range(0.0, 100.0)
                .default_range(20.0, 80.0)
        });
        let date_st = cx.new(DatePickerState::new);
        let time_st = cx.new(|cx| TimePickerState::new(cx).show_seconds(true));
        let xfer = cx.new(|cx| {
            let items: Vec<TransferItem> = (0..40)
                .map(|i| TransferItem::new(format!("x-{i}"), format!("Permission {i}")))
                .collect();
            TransferState::new(items, vec![SharedString::from("x-0")], cx)
        });
        let tags = cx.new(|cx| {
            TagInputState::new(cx)
                .max_tags(8)
                .default_tags(["rust", "gpui", "wgpu"])
        });
        let img_prev = cx.new(|cx| {
            ImagePreviewState::new(
                vec![
                    ImagePreviewItem::custom("A", |_, _| crate::div().into_any_element()),
                    ImagePreviewItem::custom("B", |_, _| crate::div().into_any_element()),
                ],
                cx,
            )
        });
        let tree_sel = cx.new(|cx| TreeSelectState::new(make_tree_nodes(4), cx));
        let casc = cx.new(|cx| {
            CascaderState::new(
                vec![CascaderOption::new("us", "US").children([
                    CascaderOption::new("ca", "CA").children([CascaderOption::new("sf", "SF")]),
                ])],
                cx,
            )
        });
        let tour_st = cx.new(|cx| {
            TourState::new(
                vec![
                    TourStep::target("soak-target", "Target", "Inspect target"),
                    TourStep::centered("Done", "Complete"),
                ],
                cx,
            )
        });
        let menu_nodes = vec![MenuNode::submenu(
            "m-root",
            "Section",
            [
                MenuNode::item("m-1", "Dashboard"),
                MenuNode::item("m-2", "Metrics"),
            ],
        )];
        let vmenu = cx.new(|cx| MenuState::new(menu_nodes.clone(), cx).default_open(["m-root"]));
        let hmenu = cx.new(|cx| MenuState::new(menu_nodes, cx).mode(MenuMode::Horizontal));
        let car = cx.new(|cx| CarouselState::new(3, cx));
        let palette = cx.new(|cx| {
            let cmds: Vec<CommandItem> = (0..60)
                .map(|i| {
                    CommandItem::new(format!("cmd-{i}"), format!("Action {i}"))
                        .group(format!("Group {}", i % 4))
                })
                .collect();
            CommandPaletteState::new(cmds, cx)
        });
        let split_st = cx.new(|cx| SplitPaneState::new(cx).clamp_range(0.1, 0.9));
        let num_st = cx.new(|cx| NumberInputState::new(cx).range(-50.0, 50.0, cx).step(0.25));
        let pin_st = cx.new(|cx| PinInputState::new(6, cx));

        DeepSoakView {
            tree_st,
            picker,
            slider_st,
            range_st,
            date_st,
            time_st,
            xfer,
            tags,
            img_prev,
            tree_sel,
            casc,
            tour_st,
            vmenu,
            hmenu,
            car,
            palette,
            split_st,
            num_st,
            pin_st,
            viewport_w: 1280.0,
            viewport_h: 800.0,
            drawer_open: false,
            loading: false,
            qr_payload: "https://github.com/KevinWang15/gpui".into(),
            code_text: (0..80)
                .map(|i| format!("let metric_{i} = compute({i});"))
                .collect::<Vec<_>>()
                .join("\n")
                .into(),
            tick: 0,
        }
    });
    cx.run_until_parked();

    let total_steps = 1_000usize;
    let mut seed: u64 = 0x517c_c1b7_2722_0a95;
    let mut next_u64 = || {
        seed ^= seed << 13;
        seed ^= seed >> 7;
        seed ^= seed << 17;
        seed
    };

    let t0 = Instant::now();
    for step in 0..total_steps {
        let r = next_u64();
        cx.update(|window, cx| {
            // Periodic Light/Dark theme flipping
            if step.is_multiple_of(50) {
                let theme = if step.is_multiple_of(100) {
                    Theme::dark()
                } else {
                    Theme::light()
                };
                Theme::set_global(theme, cx);
            }

            let action = r % 16;
            view.update(cx, |v, cx| {
                v.tick = step;
                match action {
                    0 => {
                        let sizes = [(320.0, 240.0), (640.0, 480.0), (1024.0, 768.0), (1440.0, 900.0)];
                        let (w, h) = sizes[(r as usize / 16) % sizes.len()];
                        v.viewport_w = w;
                        v.viewport_h = h;
                    }
                    1 => v.drawer_open = !v.drawer_open,
                    2 => v.loading = !v.loading,
                    3 => {
                        let count = if r.is_multiple_of(5) { 0 } else { (r as usize % 12) + 1 };
                        v.tree_st.update(cx, |t, cx| {
                            t.set_nodes(make_tree_nodes(count), cx);
                            if count > 0 {
                                t.toggle_check("g-0", cx);
                            }
                        });
                    }
                    4 => {
                        v.picker.update(cx, |p, cx| {
                            let rgba_u32 = ((r & 0x00ff_ffff) << 8) as u32 | 0xff;
                            p.set_color(crate::rgba(rgba_u32).into(), cx);
                        });
                    }
                    5 => {
                        let val = if r.is_multiple_of(10) {
                            f32::NAN
                        } else {
                            (r % 120) as f32 - 10.0
                        };
                        v.slider_st.update(cx, |s, cx| s.set_value(val, cx));
                        v.range_st
                            .update(cx, |s, cx| s.set_values(val, val + 25.0, cx));
                    }
                    6 => {
                        v.split_st.update(cx, |s, cx| {
                            let ratio = if r.is_multiple_of(10) {
                                f32::NAN
                            } else {
                                (r % 100) as f32 / 100.0
                            };
                            s.set_ratio(ratio, cx);
                        });
                    }
                    7 => {
                        v.num_st.update(cx, |n, cx| {
                            if r.is_multiple_of(9) {
                                n.set_value(f64::NAN, cx);
                            } else if r.is_multiple_of(2) {
                                n.increment(1.0, cx);
                            } else {
                                n.decrement(1.0, cx);
                            }
                        });
                    }
                    8 => {
                        v.pin_st.update(cx, |p, cx| {
                            if r.is_multiple_of(3) {
                                p.clear(cx);
                            } else {
                                let code = format!("{:06}", r % 1_000_000);
                                p.set_value(&code, cx);
                            }
                        });
                    }
                    9 => {
                        v.tags.update(cx, |t, cx| {
                            if r.is_multiple_of(4) {
                                t.clear(cx);
                            } else {
                                t.add_tag_str(&format!("tag-{}", r % 12), cx);
                            }
                        });
                    }
                    10 => {
                        v.palette.update(cx, |p, cx| {
                            if p.is_open() {
                                p.close_in(window, cx);
                            } else {
                                p.open(window, cx);
                            }
                        });
                    }
                    11 => {
                        v.tour_st.update(cx, |t, cx| {
                            if t.is_open() {
                                t.next(cx);
                            } else {
                                t.start(window, cx);
                            }
                        });
                    }
                    12 => {
                        v.img_prev.update(cx, |p, cx| {
                            if p.is_open() {
                                p.adjust_zoom(0.5, cx);
                                p.rotate(90, cx);
                                p.close_in(window, cx);
                            } else {
                                p.open(0, window, cx);
                            }
                        });
                    }
                    13 => {
                        v.car.update(cx, |c, cx| {
                            if r.is_multiple_of(2) {
                                c.next(cx);
                            } else {
                                c.prev(cx);
                            }
                        });
                    }
                    14 => {
                        v.qr_payload = if r.is_multiple_of(4) {
                            format!("中文🚀-{}", step).into()
                        } else {
                            format!("https://gpui.rs/soak/{step}").into()
                        };
                    }
                    _ => {
                        v.tree_sel.update(cx, |t, cx| {
                            let next = if t.selected().is_some() {
                                None
                            } else {
                                Some(SharedString::from("n-0-0"))
                            };
                            t.set_selected(next, cx);
                        });
                        v.casc.update(cx, |c, cx| {
                            if c.selected_path().is_empty() {
                                c.set_path(vec!["us".into(), "ca".into(), "sf".into()], cx);
                            } else {
                                c.set_path(Vec::new(), cx);
                            }
                        });
                    }
                }
                cx.notify();
            });
        });
        cx.run_until_parked();
    }
    let elapsed = t0.elapsed();

    // Post-soak invariant checks: every numeric & stateful invariant must remain finite and valid
    view.read_with(cx, |v, cx| {
        assert!(v.slider_st.read(cx).value().is_finite());
        assert!(v.slider_st.read(cx).fraction().is_finite());
        let (rs, re) = v.range_st.read(cx).values();
        assert!(rs.is_finite() && re.is_finite() && rs <= re);
        assert!(v.split_st.read(cx).ratio().is_finite());
        assert!(v.num_st.read(cx).value().is_finite());
        assert!(v.tags.read(cx).tags().len() <= 8);
    });

    println!(
        "[SOAK] All 28 components stress | steps={total_steps} | total={:.2} ms | mean={:.1} µs/step | {:.0} steps/sec",
        elapsed.as_secs_f64() * 1_000.0,
        elapsed.as_nanos() as f64 / total_steps as f64 / 1_000.0,
        total_steps as f64 / elapsed.as_secs_f64(),
    );
}

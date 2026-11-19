# Analytics, Charts, Time Range Picker & Faceted Filtering

This guide covers the data visualization, time-range selection, KPI display, and structured filtering modules built under `gpui::components`.

---

## Module Layout

| Module | Path | Key Public Types |
| --- | --- | --- |
| **Charts** | `crates/gpui/src/components/charts/` | `ChartCard`, `StackedBarChart`, `StackedBarSeries`, `StackedBarPoint`, `LineChart`, `LineSeries`, `LineStyle`, `BreakdownList`, `BreakdownItem`, `ChartPalette`, `ChartValueFormat`, `ChartLegendItem`, `LegendSwatch`, `LinearScale`, `BandScale`, `nice_ticks`, `format_compact` |
| **Time Range** | `crates/gpui/src/components/time_range/` | `TimeRangePicker`, `TimeRangePickerState`, `TimeRangeEvent`, `CalendarGrid`, `CalendarDate`, `DateRange`, `TimeRangePreset`, `TimeRangeUnit` |
| **Faceted Filter Bar** | `crates/gpui/src/components/filter_bar.rs` | `FilterBar`, `FilterBarState`, `FilterBarEvent`, `FilterFieldDef`, `FilterOperator`, `FilterClause`, `FilterFacetOption`, `FacetProvider` |
| **KPI Strip** | `crates/gpui/src/components/stat.rs` | `StatGroup`, `StatItem`, `StatDelta`, `StatDeltaTone`, `TrendPolarity` |
| **Segmented Control** | `crates/gpui/src/components/segmented.rs` | `SegmentedControl`, `SegmentedItem` |

---

## 1. Charts (`gpui::components::charts`)

All charts render natively on the GPU using GPUI quads, Lyon-tessellated `PathBuilder` strokes/gradients, and OpenType tabular numerals (`tabular_numerals()`).

### `ChartCard`

Wraps any chart in a consistent analytics surface with:
- Title + right-aligned muted `.subtitle(...)`
- Optional top-right `.header_action(...)` (e.g. `"Reset 30d"` or `"Clear"` button)
- Optional `.legend(...)` with `.on_legend_click(...)` for interactive series muting (`ChartLegendItem::muted(true)`)
- Optional collapsible `.data_table(...)` disclosure (`"Show data table"` / `"Hide data table"`)

```rust
use gpui::components::*;

let card = chart_card("daily-spend", "Daily cost by model")
    .subtitle("30 days · drag across bars to zoom range")
    .legend(legend_items)
    .on_legend_click(cx.listener(|this, idx: &usize, _, cx| {
        this.toggle_series_visibility(*idx, cx);
    }))
    .content(bar_chart)
    .data_table(summary_table);
```

### `StackedBarChart`

GPU-batched vertical stacked bar chart with automatic nice Y ticks, top-segment-only corner rounding, auto-thinned X-axis labels, hover column highlight + floating tooltip, click callbacks, and horizontal drag-brush selection.

```rust
stacked_bar_chart("cost-bars")
    .series([
        StackedBarSeries::new("sonnet"),
        StackedBarSeries::new("gpt"),
    ])
    .points([
        StackedBarPoint::new("Oct 21", vec![18.4, 9.2]),
        StackedBarPoint::new("Today", vec![22.1, 14.0]),
    ])
    .height(px(230.))
    .value_format(ChartValueFormat::Currency)
    .selectable(true)
    .on_bar_click(cx.listener(|this, idx: &usize, _, cx| {
        this.inspect_day(*idx, cx);
    }))
    .on_brush_select(cx.listener(|this, range: &std::ops::RangeInclusive<usize>, _, cx| {
        this.zoom_to_bar_columns(range.clone(), cx);
    }))
```

- **Drag-brush interaction**: Enabled via `.selectable(true)` (or automatically when `.on_brush_select(...)` is attached). Dragging horizontally >= `4 px` highlights the brushed columns with an accent band, crisp edge guides, dimmed non-selected bars, and a floating summary pill (`"Oct 24 – Nov 2 (10d) · $312.40"`).
- **Controlled highlight**: Pass `.selected_range(Some(start..=end))` to keep a column range highlighted outside active drags.

### `LineChart`

Multi-series continuous line chart supporting solid & dashed strokes, soft vertical area gradient fills, clamped inline endpoint callouts (`"Today: $36.21"`), interactive vertical crosshair scrubber, and horizontal drag-to-select window measurement.

```rust
line_chart("intraday-line")
    .x_domain(0.0, 24.0)
    .x_ticks([
        (0.0, "00:00".into()),
        (6.0, "06:00".into()),
        (12.0, "12:00".into()),
        (18.0, "18:00".into()),
        (24.0, "24:00".into()),
    ])
    .x_formatter(|hr| format!("{:02}:{:02}", hr.floor() as u32, ((hr.fract() * 60.0).round() as u32).min(59)))
    .add_series(
        LineSeries::new("Today", today_pts)
            .fill_area(true)
            .endpoint_callout("Today"),
    )
    .add_series(
        LineSeries::new("Previous 5 workdays, average", baseline_pts)
            .line_style(LineStyle::Dashed),
    )
    .value_format(ChartValueFormat::Currency)
    .selected_range(self.window_selection)
    .on_brush_select(cx.listener(|this, &(a, b): &(f64, f64), _, cx| {
        this.window_selection = Some((a, b));
        cx.notify();
    }))
    .on_selection_clear(cx.listener(|this, _, _, cx| {
        this.window_selection = None;
        cx.notify();
    }))
```

- **Fast sampling**: `LineSeries::sample_at(&self, x: f64) -> Option<f64>` uses `O(log N)` binary search to interpolate any coordinate along a sorted series—reusable both inside tooltips and in parent subtitles/summaries.
- **Dense series performance**: `LineChart` automatically caches tessellated `Path` geometry across hover/brush frames and applies per-pixel-column min/max envelope decimation for dense monotonic series (`> 2× plot_width`), keeping 10,000-point hover scrubs near `~0.10 ms/frame`.

### `BreakdownList`

Ranked proportional horizontal bars with spring-animated widths (`SPRING_SMOOTH`), tabular value + share percentage alignment, and click-to-narrow cross-filtering:

```rust
breakdown_list("model-breakdown")
    .items([
        BreakdownItem::new("claude-sonnet-5-5", 520.20),
        BreakdownItem::new("gpt-6.2", 269.07),
    ])
    .value_format(ChartValueFormat::Currency)
    .selected_key(selected_model)
    .hint("Click any model to toggle a FilterBar chip.")
    .on_select(move |key, _, cx| {
        filter_bar_state.update(cx, |s, cx| s.toggle_narrow("model", key.clone(), cx));
    })
```

---

## 2. Time Range Picker & Calendar (`gpui::components::time_range`)

Built on zero-dependency civil calendar arithmetic (`CalendarDate`, Howard Hinnant's algorithm) with full leap-year support.

### `TimeRangePickerState` & `TimeRangePicker`

Renders the compound analytics toolbar control:

`[ 📅 ⌄ ] [ Last 30 days  Oct 21 – Nov 19 ] | [ < ] [ > ] [ 🔍- ]`

- **Quick Select popover** (`[ 📅 ⌄ ]`): `"Last [N] [days|weeks|months] [Apply]"` with keyboard digit entry, Up/Down stepping, Enter to apply, plus `"COMMONLY USED"` presets (`Last 7 days`, `Last 30 days`, `Last 90 days`, `Month to date`, `Last month`).
- **Start and End popover** (`[ Last 30 days ... ]`): First day / Last day selectors with an embedded interactive `CalendarGrid` showing range highlighting and `max_date` clamping.
- **Steppers & zoom out**: `step_backward()`, `step_forward()` (clamped to `today`), and `zoom_out()` (doubles span up to `max_span_days`, snapping back to named presets when landing on 7/30/90 days ending today).

```rust
let time_range = cx.new(|cx| {
    TimeRangePickerState::new(CalendarDate::today_utc(), cx)
        .max_span_days(92)
        .preset(TimeRangePreset::Last30Days)
});

// Subscribe to committed range changes
cx.subscribe(&time_range, |this, _, ev: &TimeRangeEvent, cx| {
    let TimeRangeEvent::Change { range, preset } = ev;
    this.reload_telemetry(*range, *preset, cx);
}).detach();

// Render in toolbar
time_range_picker("toolbar-range", &time_range)
```

### Standalone `CalendarGrid`

```rust
calendar_grid("standalone-cal", visible_month)
    .selected(Some(selected_day))
    .highlighted_range(Some(date_range))
    .max_date(Some(today))
    .on_select(cx.listener(|this, date, _, cx| {
        this.selected_day = date;
        cx.notify();
    }))
    .on_month_change(cx.listener(|this, next_month, _, cx| {
        this.visible_month = next_month;
        cx.notify();
    }))
```

---

## 3. Faceted `FilterBar` (`gpui::components::filter_bar`)

Structured search & filter bar with segmented chips (`[ Field | op | value | × ]`), reactive facet counts, single- and multi-value popovers, and a keyboard-first grammar (`Up`/`Down`, `Enter`/`Tab`, `Space` to toggle checkboxes, `Backspace` to step back through draft stages or pop chips, `Escape` to close).

```rust
let filter_state = cx.new(|cx| {
    FilterBarState::new(
        vec![
            FilterFieldDef::new("provider", "Provider").hint("API vendor"),
            FilterFieldDef::new("model", "Model")
                .operators([FilterOperator::Eq, FilterOperator::OneOf, FilterOperator::Neq, FilterOperator::Contains]),
            FilterFieldDef::new("key", "Key").hint("Credential scope"),
        ],
        |active_clauses, field_key, query| {
            // Compute facet options & hit counts dynamically given sibling clauses
            compute_facets(active_clauses, field_key, query)
        },
        cx,
    )
    .placeholder("Filter by provider, model, key...")
});
```

- **Large facet lists**: `FilterBarState` caches facet results for the active `(field, query)` across arrow-key navigation and checkbox toggles, and automatically virtualizes suggestion lists with more than 10 items using GPUI's `uniform_list` with scroll-to-highlighted tracking.
- **Zero-allocation matching**: `FilterClause::matches_value` evaluates `FilterOperator::Contains` with an allocation-free ASCII case-insensitive scan (with Unicode fallback for non-ASCII strings).

---

## 4. `StatGroup` & `SegmentedControl`

### `StatGroup`

Divided horizontal KPI card with OpenType tabular figures (`tnum`) and polarity-aware deltas (`TrendPolarity::HigherIsBetter`, `LowerIsBetter`, `Neutral`):

```rust
stat_group("overview-kpis").items([
    StatItem::new("30-day cost", "$896.90").delta(StatDelta::percent(
        10.9,
        "vs previous 30 days",
        TrendPolarity::LowerIsBetter, // positive cost delta renders as Warning amber
    )),
    StatItem::new("Today", "$36.21").delta(StatDelta::percent(
        -11.9,
        "vs workdays by 17:36",
        TrendPolarity::LowerIsBetter, // negative cost delta renders as Positive emerald
    )),
    StatItem::new("Cache read", "58.0%").delta(StatDelta::neutral("was 58.0% before")),
])
```

### `SegmentedControl`

Compact pill switcher with a spring-animated sliding active indicator that automatically measures and tracks variable-width items:

```rust
segmented_control("metric-selector", active_metric)
    .size(ComponentSize::Sm)
    .items([
        SegmentedItem::new("cost", "Cost"),
        SegmentedItem::new("tokens", "Tokens"),
        SegmentedItem::new("requests", "Requests"),
    ])
    .on_change(cx.listener(|this, val: &SharedString, _, cx| {
        this.active_metric = val.clone();
        cx.notify();
    }))
```

Use `.on_change_with_position(|val, click_pos, window, cx| ...)` when you want the pointer click position (for example, to seed a radial `ViewTransition`).

---

## 5. Coordinating Components Together

A complete live reference implementation lives in [`crates/gpui_playground/src/pages/analytics.rs`](../crates/gpui_playground/src/pages/analytics.rs):

### Pattern A: `StackedBarChart` Drag-Brush → `TimeRangePicker` Zoom

```rust
let range = self.time_range.read(cx).range();
let range_start = range.start;
let time_range = self.time_range.clone();

stacked_bar_chart("daily-bars")
    .series(series)
    .points(points)
    .selectable(true)
    .on_brush_select(move |cols, _, cx| {
        let start = range_start.add_days(*cols.start() as i64);
        let end = range_start.add_days(*cols.end() as i64);
        time_range.update(cx, |s, cx| s.apply_custom_range(start, end, cx));
    })
```

Pair with a conditional `ChartCard::header_action` when `active_preset == TimeRangePreset::Custom` that calls `s.apply_preset(TimeRangePreset::Last30Days, cx)` for one-click zoom reset.

### Pattern B: `BreakdownList` Click-to-Narrow → `FilterBar`

```rust
let filter = self.filter.clone();
breakdown_list("cost-by-model")
    .items(items)
    .selected_key(active_single_model)
    .on_select(move |model_key, _, cx| {
        filter.update(cx, |s, cx| s.toggle_narrow("model", model_key.clone(), cx));
    })
```

Clicking a row inserts `Model = <name>` into `FilterBar` (re-filtering all sibling charts and KPIs); clicking the same row again toggles the chip off cleanly.

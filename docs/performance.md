# Performance Benchmarks & Optimizations

This document records the reproducible benchmark suite, measured bottlenecks, implemented optimizations, and before/after results for `gpui` core and `gpui::components`.

---

## Running the Benchmarks

All benchmarks run in `--release` mode with `--test-threads=1` to eliminate CPU contention between tests.

### 1. Component & Frame Pipeline Suite (`gpui::components::perf_tests`)

```bash
cargo test --release -p gpui --lib --features components,lucide \
  -- components::perf_tests --ignored --nocapture --test-threads=1
```

### 2. Real Headless WGPU `ViewTransition` Suite (`gpui_wgpu`)

```bash
# On hardware Vulkan GPU:
cargo test --release -p gpui_wgpu --lib \
  -- view_transition_perf_30_frames --ignored --nocapture --test-threads=1

# On headless Linux / CI with Lavapipe software Vulkan:
VK_ICD_FILENAMES=/usr/share/vulkan/icd.d/lvp_icd.x86_64.json \
  cargo test --release -p gpui_wgpu --lib \
  -- view_transition_perf_30_frames gpu_offloading_perf_benchmarks --ignored --nocapture --test-threads=1
```

---

## Measured Results (Before vs. After)

Measured in `--release` on Linux (x86_64, Rust 1.99, Lavapipe Vulkan ICD for WGPU headless GPU test). A 120 Hz frame budget is **8,333 µs (8.33 ms)**.

| Workload | Baseline (Before) | Optimized (After) | Improvement |
| --- | --- | --- | --- |
| **`CartesianPlot` line hover scrub** (4 `LineLayer`s × 2,500 = 10,000 pts, 60f) | 1,209.7 µs med · 74.44 ms total | **103.5 µs med · 6.68 ms total** | **11.7× faster** (-91.4%) |
| **GPU Scene `QuadAnimation` advance** (120-row settings page, 60 frames) | 2,674.9 µs med (full relayout) | **0.2 µs med · 0.02 ms total** | **Zero relayout (>10,000× CPU)** |
| **WGPU `prepare_instances` upload** (4,000 quads + 400 shadows) | 242.2 µs/frame (full + 7 BindGroups) | **4.4 µs/frame (delta, 0 BindGroups)** | **55.4× faster** (~550 KB → ~128 B) |
| **`FilterBar` open popover nav** (10 active chips, 120 facet opts, 40 frames) | 2,384.5 µs med · 99.60 ms total | **393.2 µs med · 16.09 ms total** | **6.06× faster** (-83.5%) |
| **`CartesianPlot` bar 2 px drag-brush** (90d × 6 series, 60 moves) | 488.0 µs med · 29.88 ms (62 renders) | **0.4 µs med · 5.38 ms (16 renders)** | **5.55× faster** (-82.0%, -74% renders) |
| **WGPU `ViewTransition` composite** (1280×800, 2,500 quads + 250 shadows, 30f) | 5.61 ms steady · 176.96 ms total | **1.13 ms steady · 45.09 ms total** | **5.0× faster steady** / **3.93× total** |
| **WGPU Dual-Kawase `BackdropBlur`** (1280×800 over 4,000 quads + 400 shadows) | *(Previously flat scrim only)* | **9.40 ms/f (Lavapipe SW Vulkan)** | Hardware GPU frosted glass |
| **`FilterClause::matches_value` (`Contains`)** (100,000 evaluations) | 0.082 µs/op · 8.25 ms total | **0.027 µs/op · 2.69 ms total** | **3.07× faster** (-67.4%) |
| **`CartesianPlot` bar hover** (90d × 6 series, 60 frames) | 596.6 µs med · 37.81 ms total | **422.5 µs med · 26.61 ms total** | **1.41× faster** (-29.2%) |
| **Settings parent re-render** (120 `SettingRow`s + 120 `Styled` `Input`s, 50f) | 3,684.4 µs med · 196.71 ms total | **2,615.0 µs med · 134.01 ms total** | **1.41× faster** (-29.0%) |
| **Full Analytics dashboard re-render** (all widgets combined, 60f) | 765.0 µs med · 47.63 ms total | **727.3 µs med · 44.22 ms total** | **1.05×** (~8.7% of 120 Hz budget) |
| **`CalendarDate` epoch + add + weekday** (200,000 ops) | 0.007 µs/op · 1.48 ms total | **0.007 µs/op · 1.48 ms total** | ~135M ops/sec (zero-alloc) |

---

## Optimization Breakdown

### 1. GPU `ViewTransition` Generation & Bind-Group Caching (`scene.rs`, `wgpu_renderer.rs`)
- **Problem**: `WgpuRendererCore::render_frame_with_transition` previously re-uploaded all instance buffers and re-rasterized the entire live scene into `transition_new` on every transition vsync frame, and allocated a fresh `wgpu::BindGroup` every frame.
- **Fix**:
  - Added a global monotonic `Scene::generation() -> u64` stamped in `Scene::finish()` and bumped in `Scene::advance_animations()` only when animated sprites exist.
  - Cached `transition_new_generation` and `transition_bind_group` on `WgpuRendererCore`, invalidating cleanly on texture resize, `clear_transition_snapshot`, and mid-flight interruption capture.
  - Static transition frames now submit **only** a 32-byte uniform write and a single 3-vertex fullscreen composite pass, while dynamic updates/spinners invalidate immediately.
- **Impact**: **5.0× faster** steady transition frames (`5.61 ms` → `1.13 ms` on Lavapipe; `176.96 ms` → `45.09 ms` across 30 frames).

### 2. Core `gpui::Path` Tile-Binning Cache + `CartesianPlot` Geometry Cache & Decimation (`path_builder.rs`, `path_winding.rs`, `scene.rs`, `charts/plot.rs`, `charts/core.rs`)
- **Problem**: Every `on_mouse_move` crosshair scrub on a 10,000-point line plot previously cloned all series data, ran Lyon `PathBuilder` stroke/dash/fill tessellation from scratch, ran CPU tile-binning (`path_winding::bin`) inside `PaintedPath::new`, and performed `O(N)` linear interpolation scans.
- **Fix**:
  - Added a lazy scale-keyed `binned_tiles` cache directly on `gpui::Path` (`Arc<PathTiles>`) with an `O(1)` visibility fast-path (`PathTiles::all_tiles_visible`) so unclipped repainted paths share binned tiles zero-copy.
  - Cached tessellated area, casing halo, stroke, and reference-line `Path`s inside `PlotInteractionState`, keyed by an exact `FxHasher` fingerprint of `(plot_rect, x_domain, y_domain, layer data/style)`.
  - Added per-pixel-column min/max envelope decimation (`decimate_monotonic_points`) for dense monotonic-X series (`> 2× plot_width`) so peaks and valleys remain pixel-accurate while bounding curve count to `O(plot_width)`.
  - Replaced `O(N)` linear interpolation with `O(log N)` binary search (`LineLayer::sample_at`) and shared layers/colors via `Rc<[T]>`.
- **Impact**: **11.7× faster** hover scrub (`1,209.7 µs` → `103.5 µs` median on 10,000 points).

### 3. `FilterBar` Virtualized Popover, Facet Caching & Zero-Alloc `Contains` (`filter_bar.rs`)
- **Problem**: Navigating a 120-option facet popover called `facet_provider` twice per keypress (once in `handle_key_down`, once in `render`), built all 120 rows unconditionally in an unbounded flex column, and allocated `String`s via `.to_lowercase()` inside `FilterClause::matches_value` for `Contains`.
- **Fix**:
  - Virtualized suggestion lists with `> 10` items via GPUI's `uniform_list` (`280 px` viewport height + `UniformListScrollHandle` keyboard scroll-into-view).
  - Cached `matching_facets` per `(field, query)` on `FilterBarState` with clean invalidation on clause/query/draft/open mutations.
  - Added an allocation-free ASCII case-insensitive substring matcher (`contains_case_insensitive`) with Unicode fallback.
- **Impact**: **6.06× faster** popover navigation (`2,384.5 µs` → `393.2 µs`) and **3.07× faster** `Contains` matching (`8.25 ms` → `2.69 ms` / 100k).

### 4. `CartesianPlot` Snapped-Column Brush Dirty Check & Quad Reduction (`charts/plot.rs`)
- **Problem**: Drag-brushing across discrete bars triggered `cx.notify()` on every sub-column mouse movement even when the snapped column index range `lo..=hi` did not change, cloned column data on every render, and painted an extra opaque `surface_bg` separator quad between every stacked segment.
- **Fix**:
  - Compared `prev_brush != next_brush` in `on_mouse_move` so renders occur only when the brushed column range changes.
  - Shared `Rc<[PlotLayer]>` between paint and tooltip closures and removed redundant opaque separator quads (leaving the 0.75 px hairline gap directly, which also lets the translucent brush band show cleanly between segments).
- **Impact**: **5.55× faster** drag-brush total (`29.88 ms` → `5.38 ms`, **62 → 16 renders**) and **1.41× faster** hover (`596.6 µs` → `422.5 µs`).

### 5. `Input`/`TextBox` Listener Gating, Paint Cache & `Styled` Read-Before-Write (`input.rs`, `textbox.rs`, `select.rs`, `combobox.rs`, `slider.rs`)
- **Problem**: Rendering 120 `Styled` `Input`s on a settings page took 120 mutable `Entity::update` leases in `Input::render`, registered 22+ `on_action` closures and idle mouse handlers per unfocused input (~3,000 closure allocations + dispatch-tree entries/frame), reshaped unchanged text every frame, and took another 120 mutable `Entity::update` leases inside `InputTextElement::paint`.
- **Fix**:
  - Added immutable read-before-write guards (`if self.state.read(cx).frame_style != self.style`) across all five entity-backed `Styled` wrappers.
  - Gated keyboard `on_action` listeners and `window.handle_input` on `focused`, and drag listeners (`on_mouse_move`/`on_mouse_up`) on `focused || self.is_selecting`.
  - Moved `InputState`'s `last_layout`, `last_bounds`, `scroll_offset`, and a scale/font/color-keyed `Rc<ShapedLine>` cache into an `Rc<RefCell<InputPaintCache>>`, eliminating all mutable entity updates during `paint()` and skipping text reshaping on unchanged frames.
- **Impact**: **1.41× faster** 120-input settings page re-render (`3,684.4 µs` → `2,615.0 µs`, saving `> 1.0 ms` per frame).

### 6. Shared Zero-Allocation Primitives (`theme.rs`, `charts/core.rs`, `calendar.rs`, `segmented.rs`)
- Replaced repeated per-tick and per-cell `FontFeatures(Arc::new(vec![("tnum".to_string(), 1)]))` heap allocations with a single `OnceLock`-backed `tabular_numerals()`.
- Replaced per-child `format!("...-{}", idx)` `ElementId` string allocations with GPUI's native zero-allocation `(&'static str, usize)` scoped tuple IDs (`("leg", idx)`, `("seg", idx)`, `("day", offset)`, `("opt-val", idx)`, etc.) and static `DAY_LABELS` in `CalendarGrid`.

### 7. GPU Offloading: Zero-Relayout `QuadAnimation`, Persistent Storage Buffer Delta Uploads & Dual-Kawase `BackdropBlur` (`scene.rs`, `window.rs`, `style.rs`, `wgpu_renderer.rs`, `shaders_backdrop_blur.wgsl`)
- **Zero-Relayout Scene `QuadAnimation`**:
  - Added `QuadAnimation` (`QuadAnimationMode::{Oneshot, SyncedPulse}` and analytical closed-form `QuadAnimationCurve::{EaseOutQuint, Spring}`) directly on `Scene` without altering `Quad`'s `#[repr(C)]` ABI across Metal, DirectX 11, WebGL2, or Vulkan.
  - Upgraded `Skeleton` (synced 120 FPS shimmer pulse), `SegmentedControl` (sliding pill), `ProgressBar` (animated fill bar), and iconless `Toggle` (track color morph + sliding thumb) to `Window::paint_animated_quad(...)`.
  - Subsequent animation frames skip `Window::draw()`, view `render()`, and Taffy layout completely (`2,674.9 µs` → `0.2 µs` median on a 120-row page), and automatically disarm `scene_animation_interval` once settled (`0%` idle cost).
- **Persistent GPU Storage Buffer & Delta-Only Uploads (`gpui_wgpu`)**:
  - Separated `Scene::base_generation()` (structural) from `Scene::generation()` (content) and tracked `animated_quad_range()` / `animated_monochrome_sprite_range()`.
  - `WgpuRendererCore::prepare_instances` caches all 7 `wgpu::BindGroup`s per `base_generation` and uploads only the tiny mutated animated slice (`~128 B` instead of re-uploading hundreds of kilobytes of static quads, shadows, paths, and text sprites), reducing `prepare_instances` from `242.2 µs` to `4.4 µs` (**55.4× faster**, `0` BindGroup allocations/frame).
- **Multi-Pass Dual-Kawase GPU Frosted-Glass `BackdropBlur` (`shaders_backdrop_blur.wgsl`)**:
  - Added `Styled::backdrop_blur(Pixels)` and `Primitive::BackdropBlur`. What a blur is, for every renderer (DirectX, Metal, WGPU), is in `crates/gpui/src/backdrop_blur.rs`: a dual-Kawase pyramid whose depth and sample offset come from the radius (a standard deviation), composited as `mix(backdrop, blurred, coverage)` so translucent windows stay correct, with a CPU reference the renderers' tests compare against.
  - Zero overhead on frames without backdrop blur; enabled by default on in-window `Dialog` backdrops (`10 px`) and demonstrated live in `gpui_playground`.

---

## Guidelines for Keeping GPUI Apps at 120 FPS

1. **Avoid cloning large datasets in `Render::render`**: Pre-aggregate or store chart data cleanly on your view/model rather than rebuilding thousands of points on every hover frame.
2. **Use `uniform_list` for long scrollable lists**: Any list with >30–50 rows (logs, tables, settings lists, facet pickers) should virtualize via `uniform_list` to keep Taffy layout `O(visible_rows)`.
3. **Reuse `Path` values across frames**: Building a `Path` once and painting `path.clone()` now reuses both the Lyon curves (`Arc<[PathCurve]>`) and GPUI's binned GPU tiles (`Arc<PathTiles>`).
4. **Use `tabular_numerals()`** instead of allocating `FontFeatures` inline inside render loops.
5. **Prefer scoped `(&'static str, usize)` IDs** inside elements that already establish an `.id(...)` root scope.

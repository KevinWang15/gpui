# GPU View Transitions & Radial Theme Switching

GPUI includes a first-class, compositor-level **View Transition** engine (`crates/gpui/src/view_transition.rs` and `crates/gpui_wgpu/src/shaders_view_transition.wgsl`) that smoothly animates full-window visual state changes—such as switching between Light and Dark themes with a circular reveal expanding outward from the user's click coordinate—at 60–120 FPS without duplicating element trees or re-running layout on every frame.

---

## Architecture Overview

A naïve UI-tree approach to radial theme transitions would render two complete copies of the window in parallel and clip one copy per frame—doubling layout, text shaping, and scene-building costs on every vsync tick.

GPUI takes a **GPU compositor** approach instead:

```
1. Snapshot (before state mutation)
   ┌───────────────────────────┐
   │ rendered_frame.scene (Old)│ ──▶ Offscreen GPU texture `transition_old`
   └───────────────────────────┘

2. Commit state once
   update(window, cx)  ──▶ Single normal Window::draw builds `new` Scene (generation = G)

3. Animate at vsync (zero layout passes while UI is idle)
   Frame 1:  Render `new` Scene (G) ──▶ Offscreen GPU texture `transition_new`
             Composite(`transition_old`, `transition_new`, params) ──▶ Swapchain
   Frames 2..N (while Scene::generation() == G):
             Skip scene rasterization; upload 32-byte uniform `ViewTransitionParams`
             Composite(`transition_old`, `transition_new`, params) ──▶ Swapchain
   Frame N+1 (elapsed >= duration):
             Release offscreen textures; resume direct rendering to swapchain
```

### Key Properties

- **Single layout pass**: `Window::start_view_transition` snapshots the previous scene on the GPU, invokes the state mutation closure once, and then drives the remaining animation frames via GPUI's `scene_animation_interval` fast path without re-running element layout or paint.
- **Cached offscreen new scene (`Scene::generation`)**: `Scene::finish()` stamps a monotonic `u64` generation (`Scene::generation()`). `WgpuRendererCore` caches `transition_new` and its `wgpu::BindGroup` keyed by generation. On static transition frames, the GPU executes **only** a single 3-vertex fullscreen triangle pass (`~1.1 ms` even on software Vulkan Lavapipe, and microseconds on hardware GPUs).
- **Live updates mid-transition**: If the user interacts mid-transition (typing, hovering, scrolling) or an animated sprite (`Spinner`) advances via `Scene::advance_animations`, `Scene::generation()` increments automatically and `transition_new` re-rasterizes for that frame so the live UI never freezes.
- **Smooth mid-flight interruption**: Triggering a second transition before the first completes snapshots the *currently visible composited frame* as the new `transition_old`, so rapid light/dark clicking sweeps seamlessly without visual snaps.
- **Viewport resize safety**: If the window is resized or its display scale factor changes mid-transition, `ActiveViewTransition::evaluate` returns `None`, cleanly aborting the transition and releasing offscreen textures.
- **Reduced motion & graceful fallback**:
  - When `cx.reduce_motion()` is enabled, transitions snap instantaneously by default, or fall back to a brief `140 ms` cross-fade if `.reduce_motion_crossfade(true)` is set.
  - Platform backends that do not implement `PlatformWindow::capture_transition_snapshot` return `false` and fall back to an immediate state commit with zero overhead.

---

## Core API (`gpui::ViewTransition`)

```rust
use std::time::Duration;
use gpui::{ViewTransition, Window, App, Point, Pixels};

fn switch_view_with_radial_reveal(
    click_origin: Point<Pixels>,
    window: &mut Window,
    cx: &mut App,
) {
    let transition = ViewTransition::radial_reveal(click_origin)
        .duration(Duration::from_millis(420))
        .feather(gpui::px(1.5))
        .reduce_motion_crossfade(true);

    window.start_view_transition(transition, cx, |window, cx| {
        // Mutate global or view state here (e.g., Theme::set_global, page switch, etc.)
        cx.refresh_windows();
    });
}
```

### Transition Geometries (`ViewTransitionKind`)

| Constructor | Geometry | Typical Use Case |
| --- | --- | --- |
| `ViewTransition::radial_reveal(origin)` | Expands a feathered circle of the **new** scene outward from `origin` to the farthest window corner (`hypot(max(ox, w-ox), max(oy, h-oy))`). | Click-origin Light ↔ Dark theme switches. |
| `ViewTransition::radial_collapse(origin)` | Collapses a feathered circle of the **old** scene inward toward `origin`, revealing the new scene outside. | Reverse / closing radial transitions. |
| `ViewTransition::cross_fade()` | Smooth full-window alpha dissolve (`220 ms` default). | OS system appearance changes or non-pointer-triggered theme updates. |

### Lifecycle Controls on `Window`

- `window.start_view_transition(transition, cx, update)` — Captures the current rendered scene, runs `update(window, cx)`, and schedules transition frames.
- `window.cancel_view_transition()` — Immediately aborts any in-flight transition and releases GPU snapshot textures.
- `window.has_active_view_transition() -> bool` — Queries whether a full-window transition is currently animating.

---

## High-Level Theme Switching & `ThemeSwitcher`

### 1. Drop-in 3-Way Light / System / Dark Switcher

`ThemeSwitcher` (`crates/gpui/src/components/theme_switcher.rs`) wraps `SegmentedControl` with `Sun` / `Monitor` / `Moon` icons and automatically passes the exact pointer click coordinate into `Theme::set_preference_with_transition`:

```rust
use gpui::components::{ThemeSwitcher, ComponentSize, theme_switcher};

let switcher = theme_switcher("navbar-theme")
    .size(ComponentSize::Sm);
```

### 2. Custom Brand Theme Preservation

If your application builds a customized `Theme` (e.g., custom accent, surface palette, or flat chrome), attach `.on_change(...)` and pass the resolved theme to `Theme::switch_with_transition`:

```rust
use gpui::components::{Theme, ThemeMode, theme_switcher};

let switcher = theme_switcher("custom-theme-switcher").on_change(|pref, origin, window, cx| {
    let mode = Theme::resolve_mode(pref, window);
    let next_theme = my_brand_theme(mode).with_preference(pref);
    Theme::switch_with_transition(next_theme, Some(origin), window, cx);
});
```

When the resolved `ThemeMode` does not change (for example, clicking **System** while the OS is already in Light mode and Light is active), `Theme::switch_with_transition` updates `Theme::preference` immediately without running an unnecessary animation.

### 3. Following OS Appearance Automatically

Call `Theme::observe_system_appearance` once during window setup and hold the returned `Subscription`:

```rust
let appearance_sub = Theme::observe_system_appearance(window, |mode, pref, _cx| {
    my_brand_theme(mode).with_preference(pref)
});
```

Whenever the OS toggles Light/Dark appearance while `ThemePreference::System` is active, GPUI automatically cross-fades to the matching theme.

---

## Platform Backend Support

| Backend | Support Status | Notes |
| --- | --- | --- |
| **Linux X11 (`gpui_linux` + `gpui_wgpu`)** | Full GPU radial & cross-fade | Wired via `WgpuRenderer::capture_transition_snapshot` / `clear_transition_snapshot`. |
| **Linux Wayland (`gpui_linux` + `gpui_wgpu`)** | Full GPU radial & cross-fade | Same WGPU compositor pipeline. |
| **Web / WASM (`gpui_web` + `gpui_wgpu`)** | Full GPU radial & cross-fade | Uses the same `shaders_view_transition.wgsl` shader. |
| **Other backends** | Automatic graceful fallback | Default `PlatformWindow::capture_transition_snapshot` returns `false`; state commits instantaneously. |

---

## Shader Details (`shaders_view_transition.wgsl`)

The compositor pass draws a single oversized fullscreen triangle (`vertex_index` `0..3`, zero vertex buffers) and performs texel-exact `textureLoad` reads from `t_old` and `t_new` in device pixel coordinates (`frag_pos.xy`):

- **`RadialExpand` (`kind == 0`)**: `show_old = smoothstep(radius - feather, radius + feather, length(frag_pos.xy - origin_px))`
- **`RadialCollapse` (`kind == 1`)**: `show_old = 1.0 - smoothstep(radius - feather, radius + feather, length(frag_pos.xy - origin_px))`
- **`CrossFade` (`kind == 2`)**: `show_old = 1.0 - clamp(progress, 0.0, 1.0)`
- Output: `mix(new_color, old_color, show_old)`

The shader is validated against Naga in `gpui_wgpu`'s unit tests (`view_transition_shader_is_valid_wgsl`).

//! What a [`BackdropBlur`](crate::BackdropBlur) is, for every renderer.
//!
//! The blur radius is a standard deviation, as in CSS `backdrop-filter:
//! blur()`: the backdrop is blurred so that a point spreads out with that
//! standard deviation, in device pixels. Renderers realize it with a
//! dual-Kawase filter (Marius Bjørge, "Bandwidth-Efficient Rendering",
//! SIGGRAPH 2015): the backdrop is downsampled `levels` times by halves, then
//! upsampled back, each pass sampling `offset` texels away. [`BlurPlan`]
//! picks the levels and the offset for a radius from tables measured on
//! [`reference`], the CPU version of the same passes; the tests check that
//! the filter's spread matches the radius.
//!
//! Every renderer runs the same steps:
//! 1. When a blur batch comes up, copy what has been drawn so far: the
//!    *snapshot* (level 0).
//! 2. Run [`BlurPlan::passes`]: downsample level `k - 1` into level `k` for
//!    `k = 1..=levels`, then upsample level `k + 1` into level `k` down to
//!    level 1. Level `k` is the target size halved `k` times
//!    ([`level_size`]).
//! 3. Composite each blur of the batch: upsample level 1 once more with the
//!    same tent filter, at the target's resolution, and write
//!    `mix(snapshot, blurred, coverage)` with blending off, where `coverage`
//!    is the rounded rectangle's antialiased coverage inside the content
//!    mask. Mixing with the snapshot (not blending over the target) keeps
//!    premultiplied, translucent targets correct: a transparent backdrop
//!    stays transparent.
//!
//! The passes, with `o` the plan's offset and `t` the source's texel size:
//! - downsample: `(4·s(uv) + s(uv ± (t·o, t·o)) for the 4 diagonals) / 8`
//! - upsample: `(s(uv ± (2·t·o, 0)) + s(uv ± (0, 2·t·o))
//!   + 2·s(uv ± (t·o, t·o)) for the 4 diagonals) / 12`
//!
//! `s` samples bilinearly with clamp-to-edge addressing, and `uv` is the
//! destination texel's center.
//!
//! Ordering: a blur reads up to [`BlurPlan::reach`] beyond its bounds, so the
//! scene orders it after everything painted earlier within that reach, and it
//! never shares a draw order with another primitive it overlaps except ones
//! painted after it. Blurs must not be painted inside `Window::paint_layer`:
//! a layer's primitives share one draw order, so the blur couldn't see the
//! ones painted before it in the layer.

/// The most halvings a blur uses (a blur of up to about 180 device pixels).
pub const MAX_BLUR_LEVELS: u32 = 6;

/// Radii below this are not blurred at all.
pub const MIN_BLUR_RADIUS: f32 = 0.5;

/// Measured spread of one plan level, divided by `2^levels`, for offsets
/// `0.0, 0.25, …, 2.5` (see [`OFFSET_STEP`]). From [`reference`] on an
/// impulse; the tests re-measure them.
const SPREAD_LEVEL_1: [f32; 11] = [0.661, 0.692, 0.878, 1.07, 1.331, 1.575, 1.854, 2.175, 2.504, 2.81, 3.126];
const SPREAD_LEVEL_2: [f32; 11] = [0.696, 0.732, 0.949, 1.17, 1.467, 1.743, 2.058, 2.418, 2.766, 3.071, 3.377];
/// Levels 3 and up agree within 1%.
const SPREAD_LEVEL_3_UP: [f32; 11] = [0.705, 0.744, 0.968, 1.197, 1.503, 1.787, 2.112, 2.483, 2.836, 3.156, 3.492];
const OFFSET_STEP: f32 = 0.25;
/// Plans keep the offset at or below this (wider offsets leave gaps between
/// taps), except at [`MAX_BLUR_LEVELS`], where it may go to the end of the
/// table.
const PREFERRED_MAX_OFFSET: f32 = 2.0;

/// How a renderer realizes a blur radius: the number of halvings and the
/// sample offset, in texels of each pass's source.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BlurPlan {
    /// Number of halvings, `1..=MAX_BLUR_LEVELS`.
    pub levels: u32,
    /// Sample offset in source texels.
    pub offset: f32,
}

/// One render pass of a [`BlurPlan`]: read level `source`, write level
/// `target` (level 0 is the snapshot, at the target's resolution).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BlurPass {
    /// Downsample or upsample.
    pub kind: BlurPassKind,
    /// The level read.
    pub source: u32,
    /// The level written.
    pub target: u32,
}

/// The two filters a [`BlurPass`] runs.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BlurPassKind {
    /// The 5-tap filter, halving the resolution.
    Downsample,
    /// The 8-tap tent filter, doubling it.
    Upsample,
}

fn spread_table(levels: u32) -> &'static [f32; 11] {
    match levels {
        1 => &SPREAD_LEVEL_1,
        2 => &SPREAD_LEVEL_2,
        _ => &SPREAD_LEVEL_3_UP,
    }
}

/// The table's value at `offset`, interpolated linearly.
fn spread_at(table: &[f32; 11], offset: f32) -> f32 {
    let x = (offset / OFFSET_STEP).clamp(0.0, (table.len() - 1) as f32);
    let i = (x.floor() as usize).min(table.len() - 2);
    let f = x - i as f32;
    table[i] + (table[i + 1] - table[i]) * f
}

/// The offset whose table value is `spread`, clamped to the table.
fn offset_for(table: &[f32; 11], spread: f32) -> f32 {
    if spread <= table[0] {
        return 0.0;
    }
    for i in 0..table.len() - 1 {
        if spread <= table[i + 1] {
            let f = (spread - table[i]) / (table[i + 1] - table[i]);
            return (i as f32 + f) * OFFSET_STEP;
        }
    }
    (table.len() - 1) as f32 * OFFSET_STEP
}

impl BlurPlan {
    /// The plan for a blur radius (a standard deviation) in device pixels:
    /// the fewest levels that reach it with an offset of at most 2. `None`
    /// below [`MIN_BLUR_RADIUS`]. Radii below what one level does with no
    /// offset (about 1.3 pixels) get that; radii beyond the largest plan get
    /// the largest plan.
    pub fn for_radius(radius: f32) -> Option<Self> {
        // NaN is not a radius either.
        if radius.is_nan() || radius < MIN_BLUR_RADIUS {
            return None;
        }
        for levels in 1..=MAX_BLUR_LEVELS {
            let table = spread_table(levels);
            let scale = (1u32 << levels) as f32;
            let max_offset = if levels == MAX_BLUR_LEVELS {
                (table.len() - 1) as f32 * OFFSET_STEP
            } else {
                PREFERRED_MAX_OFFSET
            };
            if radius <= spread_at(table, max_offset) * scale || levels == MAX_BLUR_LEVELS {
                let offset = offset_for(table, radius / scale).min(max_offset);
                return Some(BlurPlan { levels, offset });
            }
        }
        None
    }

    /// The standard deviation this plan achieves, in device pixels.
    pub fn radius(&self) -> f32 {
        spread_at(spread_table(self.levels), self.offset) * (1u32 << self.levels) as f32
    }

    /// How far beyond a pixel the blur reads, in device pixels: the filter's
    /// whole support, so nothing outside it changes the result.
    pub fn reach(&self) -> f32 {
        let o = self.offset;
        let n = (1u32 << self.levels) as f32;
        // Downsampling into level k reads (o + 1) texels of level k - 1; each
        // upsample out of level k (the composite's included) reads (2o + 1)
        // texels of level k. One pixel of margin for texel-center rounding.
        (o + 1.0) * (n - 1.0) + (2.0 * o + 1.0) * (2.0 * n - 2.0) + 1.0
    }

    /// The render passes to run before compositing, in order.
    pub fn passes(&self) -> impl Iterator<Item = BlurPass> {
        let levels = self.levels;
        let down = (1..=levels).map(|target| BlurPass {
            kind: BlurPassKind::Downsample,
            source: target - 1,
            target,
        });
        let up = (1..levels).rev().map(|target| BlurPass {
            kind: BlurPassKind::Upsample,
            source: target + 1,
            target,
        });
        down.chain(up)
    }
}

/// A batch's blurs grouped by plan, each group in paint order; blurs too
/// small to see are left out. Each group runs its plan's passes once.
pub fn group_by_plan(blurs: &[crate::BackdropBlur]) -> Vec<(BlurPlan, Vec<&crate::BackdropBlur>)> {
    let mut groups: Vec<(BlurPlan, Vec<&crate::BackdropBlur>)> = Vec::new();
    for blur in blurs {
        let Some(plan) = BlurPlan::for_radius(blur.blur_radius.0) else { continue };
        match groups.iter_mut().find(|(p, _)| *p == plan) {
            Some((_, members)) => members.push(blur),
            None => groups.push((plan, vec![blur])),
        }
    }
    groups
}

/// The size of level `level` for a target of `width` by `height`: halved
/// `level` times, at least 1.
pub fn level_size(width: u32, height: u32, level: u32) -> (u32, u32) {
    ((width >> level).max(1), (height >> level).max(1))
}

/// Coverage of a rounded rectangle at a pixel center, antialiased over one
/// pixel: `radii` are (top left, top right, bottom right, bottom left), as
/// the composite shaders take them.
pub fn rounded_rect_coverage(pixel: [f32; 2], origin: [f32; 2], size: [f32; 2], radii: [f32; 4]) -> f32 {
    let half = [size[0] * 0.5, size[1] * 0.5];
    let rel = [pixel[0] - (origin[0] + half[0]), pixel[1] - (origin[1] + half[1])];
    let corner = match (rel[0] < 0.0, rel[1] < 0.0) {
        (true, true) => radii[0],
        (false, true) => radii[1],
        (false, false) => radii[2],
        (true, false) => radii[3],
    };
    let r = corner.min(half[0].min(half[1]));
    let q = [rel[0].abs() - (half[0] - r), rel[1].abs() - (half[1] - r)];
    let outside = (q[0].max(0.0).powi(2) + q[1].max(0.0).powi(2)).sqrt();
    let dist = outside + q[0].max(q[1]).min(0.0) - r;
    (0.5 - dist).clamp(0.0, 1.0)
}

/// The CPU reference of the passes and the composite, for tests (here and in
/// the renderers'): what a renderer must produce, up to rounding.
#[cfg(any(test, feature = "test-support"))]
pub mod reference {
    use super::{BlurPassKind, BlurPlan, level_size};

    /// A premultiplied RGBA image of `f32` channels, row by row.
    #[derive(Clone, Debug, PartialEq)]
    pub struct Image {
        /// Width in pixels.
        pub width: u32,
        /// Height in pixels.
        pub height: u32,
        /// `width * height` premultiplied RGBA pixels.
        pub pixels: Vec<[f32; 4]>,
    }

    impl Image {
        /// An image filled with `color`.
        pub fn filled(width: u32, height: u32, color: [f32; 4]) -> Self {
            Image { width, height, pixels: vec![color; (width * height) as usize] }
        }

        /// The pixel at `(x, y)`.
        pub fn at(&self, x: u32, y: u32) -> [f32; 4] {
            self.pixels[(y * self.width + x) as usize]
        }

        /// Bilinear sample at `(u, v)` in texel units (texel centers at
        /// `i + 0.5`), clamped to the edges.
        fn sample(&self, u: f32, v: f32) -> [f32; 4] {
            let (x, y) = (u - 0.5, v - 0.5);
            let (x0, y0) = (x.floor(), y.floor());
            let (fx, fy) = (x - x0, y - y0);
            let clamp = |c: f32, max: u32| (c as i64).clamp(0, max as i64 - 1) as u32;
            let (xa, xb) = (clamp(x0, self.width), clamp(x0 + 1.0, self.width));
            let (ya, yb) = (clamp(y0, self.height), clamp(y0 + 1.0, self.height));
            let mut out = [0.0; 4];
            for (px, w) in [
                (self.at(xa, ya), (1.0 - fx) * (1.0 - fy)),
                (self.at(xb, ya), fx * (1.0 - fy)),
                (self.at(xa, yb), (1.0 - fx) * fy),
                (self.at(xb, yb), fx * fy),
            ] {
                for c in 0..4 {
                    out[c] += px[c] * w;
                }
            }
            out
        }

        /// The sample at the texel center of a `width` by `height` target's
        /// pixel `(x, y)`, plus `(dx, dy)` source texels: how the shaders
        /// address their source with UVs.
        fn sample_for(&self, x: u32, y: u32, width: u32, height: u32, dx: f32, dy: f32) -> [f32; 4] {
            let u = (x as f32 + 0.5) / width as f32 * self.width as f32 + dx;
            let v = (y as f32 + 0.5) / height as f32 * self.height as f32 + dy;
            self.sample(u, v)
        }
    }

    const DOWN: [(f32, f32, f32); 5] = [(0.0, 0.0, 4.0), (-1.0, -1.0, 1.0), (1.0, -1.0, 1.0), (-1.0, 1.0, 1.0), (1.0, 1.0, 1.0)];
    const UP: [(f32, f32, f32); 8] = [
        (-2.0, 0.0, 1.0),
        (-1.0, 1.0, 2.0),
        (0.0, 2.0, 1.0),
        (1.0, 1.0, 2.0),
        (2.0, 0.0, 1.0),
        (1.0, -1.0, 2.0),
        (0.0, -2.0, 1.0),
        (-1.0, -1.0, 2.0),
    ];

    fn filter(src: &Image, width: u32, height: u32, kind: BlurPassKind, offset: f32) -> Image {
        let (taps, total): (&[(f32, f32, f32)], f32) = match kind {
            BlurPassKind::Downsample => (&DOWN, 8.0),
            BlurPassKind::Upsample => (&UP, 12.0),
        };
        let mut out = Image::filled(width, height, [0.0; 4]);
        for y in 0..height {
            for x in 0..width {
                let mut sum = [0.0; 4];
                for &(dx, dy, w) in taps {
                    let s = src.sample_for(x, y, width, height, dx * offset, dy * offset);
                    for c in 0..4 {
                        sum[c] += s[c] * w;
                    }
                }
                out.pixels[(y * width + x) as usize] = sum.map(|c| c / total);
            }
        }
        out
    }

    /// The blurred backdrop at the snapshot's resolution: the passes, then
    /// the composite's final upsample (without the mix).
    pub fn blur(snapshot: &Image, plan: BlurPlan) -> Image {
        let mut levels = vec![snapshot.clone()];
        for pass in plan.passes() {
            let (w, h) = level_size(snapshot.width, snapshot.height, pass.target);
            let next = filter(&levels[pass.source as usize], w, h, pass.kind, plan.offset);
            if (pass.target as usize) < levels.len() {
                levels[pass.target as usize] = next;
            } else {
                levels.push(next);
            }
        }
        filter(&levels[1], snapshot.width, snapshot.height, BlurPassKind::Upsample, plan.offset)
    }

    /// The composite of one blur: the snapshot with the blurred backdrop
    /// mixed in by the rounded rectangle's coverage (no content mask).
    pub fn composite(snapshot: &Image, plan: BlurPlan, origin: [f32; 2], size: [f32; 2], radii: [f32; 4]) -> Image {
        let blurred = blur(snapshot, plan);
        let mut out = snapshot.clone();
        for y in 0..snapshot.height {
            for x in 0..snapshot.width {
                let cov = super::rounded_rect_coverage([x as f32 + 0.5, y as f32 + 0.5], origin, size, radii);
                let i = (y * snapshot.width + x) as usize;
                for c in 0..4 {
                    out.pixels[i][c] = snapshot.pixels[i][c] + (blurred.pixels[i][c] - snapshot.pixels[i][c]) * cov;
                }
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::reference::{Image, blur, composite};
    use super::*;

    /// The horizontal standard deviation of a blurred impulse.
    fn measured_radius(plan: BlurPlan, size: u32) -> f32 {
        let mut img = Image::filled(size, size, [0.0; 4]);
        let c = size / 2;
        // A 2x2 impulse centered on a texel corner, so every level stays
        // symmetric around it.
        for (x, y) in [(c - 1, c - 1), (c, c - 1), (c - 1, c), (c, c)] {
            img.pixels[(y * size + x) as usize] = [0.25; 4];
        }
        let out = blur(&img, plan);
        let (mut total, mut var) = (0.0f64, 0.0f64);
        for y in 0..size {
            for x in 0..size {
                let w = out.at(x, y)[0] as f64;
                let dx = x as f64 + 0.5 - c as f64;
                total += w;
                var += w * dx * dx;
            }
        }
        (var / total).sqrt() as f32
    }

    #[test]
    fn plans_reach_their_radius() {
        for radius in [1.5f32, 2.0, 3.0, 5.0, 6.0, 8.0, 11.0, 12.0, 16.0, 24.0] {
            let plan = BlurPlan::for_radius(radius).unwrap();
            let size = (plan.reach() as u32 * 2 + 16).next_power_of_two().max(64);
            let got = measured_radius(plan, size);
            assert!((got - radius).abs() / radius < 0.04, "radius {radius}: plan {plan:?} measured {got}");
            assert!((plan.radius() - radius).abs() / radius < 0.01, "radius {radius}: plan says {}", plan.radius());
        }
    }

    #[test]
    fn plans_grow_with_the_radius_and_keep_small_offsets() {
        assert_eq!(BlurPlan::for_radius(0.4), None);
        assert_eq!(BlurPlan::for_radius(f32::NAN), None);
        let mut last = 0.0;
        let mut r = 0.5f32;
        while r < 400.0 {
            let plan = BlurPlan::for_radius(r).unwrap();
            assert!(plan.radius() >= last - 1e-3, "{r}: {plan:?}");
            assert!(plan.levels <= MAX_BLUR_LEVELS);
            if plan.levels < MAX_BLUR_LEVELS {
                assert!(plan.offset <= PREFERRED_MAX_OFFSET + 1e-4, "{r}: {plan:?}");
            }
            last = plan.radius();
            r *= 1.07;
        }
        assert_eq!(BlurPlan::for_radius(1000.0).unwrap().levels, MAX_BLUR_LEVELS);
    }

    #[test]
    fn passes_go_down_then_up_to_level_one() {
        let plan = BlurPlan { levels: 3, offset: 1.0 };
        let passes: Vec<(BlurPassKind, u32, u32)> = plan.passes().map(|p| (p.kind, p.source, p.target)).collect();
        use BlurPassKind::*;
        assert_eq!(passes, [(Downsample, 0, 1), (Downsample, 1, 2), (Downsample, 2, 3), (Upsample, 3, 2), (Upsample, 2, 1)]);
        assert_eq!(BlurPlan { levels: 1, offset: 1.0 }.passes().count(), 1);
    }

    #[test]
    fn nothing_beyond_the_reach_changes_the_result() {
        for radius in [2.0f32, 6.0, 14.0] {
            let plan = BlurPlan::for_radius(radius).unwrap();
            let reach = plan.reach().ceil() as u32;
            let size = (reach * 2 + 32).next_power_of_two();
            let mut img = Image::filled(size, size, [0.0; 4]);
            let c = size / 2;
            img.pixels[(c * size + c) as usize] = [1.0; 4];
            let out = blur(&img, plan);
            for y in 0..size {
                for x in 0..size {
                    let far = (x as i64 - c as i64).unsigned_abs() as u32 > reach || (y as i64 - c as i64).unsigned_abs() as u32 > reach;
                    if far {
                        assert_eq!(out.at(x, y)[0], 0.0, "radius {radius}: ({x}, {y}) beyond reach {reach}");
                    }
                }
            }
        }
    }

    #[test]
    fn flat_and_transparent_backdrops_stay_as_they_are() {
        let plan = BlurPlan::for_radius(6.0).unwrap();
        for color in [[0.0f32; 4], [0.2, 0.1, 0.05, 0.5], [0.3, 0.6, 0.9, 1.0]] {
            let img = Image::filled(64, 48, color);
            let out = composite(&img, plan, [8.0, 8.0], [40.0, 30.0], [6.0; 4]);
            for p in &out.pixels {
                for c in 0..4 {
                    assert!((p[c] - color[c]).abs() < 1e-5, "{color:?} became {p:?}");
                }
            }
        }
    }

    #[test]
    fn composite_blurs_inside_and_leaves_outside() {
        let plan = BlurPlan::for_radius(4.0).unwrap();
        let mut img = Image::filled(64, 64, [0.0; 4]);
        for y in 0..64 {
            for x in 32..64 {
                img.pixels[y * 64 + x] = [1.0, 1.0, 1.0, 1.0];
            }
        }
        let out = composite(&img, plan, [16.0, 16.0], [32.0, 32.0], [0.0; 4]);
        // Outside: untouched. Inside, at the edge between black and white: blurred.
        assert_eq!(out.at(4, 32), img.at(4, 32));
        assert_eq!(out.at(60, 4), img.at(60, 4));
        let edge = out.at(31, 32)[0];
        assert!(edge > 0.2 && edge < 0.5, "{edge}");
        // Translucent result keeps premultiplied alpha equal to the color.
        let p = out.at(33, 32);
        assert!((p[3] - p[0]).abs() < 1e-5);
    }

    mod ordering {
        use crate::{
            BackdropBlur, BorderStyle, Bounds, ContentMask, Corners, Edges, PrimitiveBatch, Quad, Scene, ScaledPixels, point, size,
        };

        fn rect(x: f32, y: f32, w: f32, h: f32) -> Bounds<ScaledPixels> {
            Bounds { origin: point(ScaledPixels(x), ScaledPixels(y)), size: size(ScaledPixels(w), ScaledPixels(h)) }
        }
        fn quad(b: Bounds<ScaledPixels>) -> Quad {
            Quad {
                order: 0,
                border_style: BorderStyle::Solid,
                bounds: b,
                content_mask: ContentMask { bounds: rect(0.0, 0.0, 1000.0, 1000.0) },
                background: crate::red().into(),
                border_color: crate::red(),
                corner_radii: Corners::default(),
                border_widths: Edges::default(),
            }
        }
        fn blur(b: Bounds<ScaledPixels>, radius: f32) -> BackdropBlur {
            BackdropBlur {
                order: 0,
                blur_radius: ScaledPixels(radius),
                bounds: b,
                corner_radii: Corners::default(),
                content_mask: ContentMask { bounds: rect(0.0, 0.0, 1000.0, 1000.0) },
            }
        }
        /// Batch kinds in draw order: 'b' blur, 'q' quads.
        fn kinds(scene: &Scene) -> String {
            scene
                .batches()
                .map(|b| match b {
                    PrimitiveBatch::BackdropBlurs(r) => "b".repeat(r.len()),
                    PrimitiveBatch::Quads(r) => "q".repeat(r.len()),
                    _ => "?".into(),
                })
                .collect()
        }

        #[test]
        fn what_the_blur_reads_is_drawn_before_it() {
            let mut scene = Scene::default();
            // A quad near the blur but outside its bounds, within its reach,
            // and one overlapping a different quad so it gets a high order.
            scene.insert_primitive(quad(rect(300.0, 0.0, 10.0, 10.0)));
            scene.insert_primitive(quad(rect(300.0, 0.0, 10.0, 10.0)));
            scene.insert_primitive(quad(rect(300.0, 0.0, 10.0, 10.0)));
            scene.insert_primitive(blur(rect(320.0, 0.0, 50.0, 50.0), 8.0));
            // Painted after: drawn after.
            scene.insert_primitive(quad(rect(330.0, 10.0, 10.0, 10.0)));
            scene.finish();
            assert_eq!(kinds(&scene), "qqqbq");
        }

        #[test]
        fn a_blur_over_a_blur_sees_it() {
            let mut scene = Scene::default();
            scene.insert_primitive(blur(rect(0.0, 0.0, 100.0, 100.0), 4.0));
            scene.insert_primitive(blur(rect(50.0, 50.0, 100.0, 100.0), 4.0));
            scene.finish();
            assert_eq!(kinds(&scene), "bb".chars().map(|c| c.to_string()).collect::<Vec<_>>().join(""));
            assert_eq!(scene.batches().count(), 2, "two batches, one per draw order");
        }

        #[test]
        fn blurs_too_small_to_see_are_dropped() {
            let mut scene = Scene::default();
            scene.insert_primitive(blur(rect(0.0, 0.0, 10.0, 10.0), 0.2));
            scene.finish();
            assert_eq!(scene.batches().count(), 0);
        }
    }

    #[test]
    fn coverage_is_antialiased_and_rounded() {
        let cov = |x: f32, y: f32| rounded_rect_coverage([x, y], [10.0, 10.0], [20.0, 20.0], [5.0; 4]);
        assert_eq!(cov(20.0, 20.0), 1.0);
        assert_eq!(cov(5.0, 20.0), 0.0);
        assert!((cov(10.0, 20.0) - 0.5).abs() < 1e-5);
        // The very corner is outside the rounding.
        assert_eq!(cov(10.5, 10.5), 0.0);
    }
}

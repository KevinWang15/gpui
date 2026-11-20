// Backdrop blur, as `gpui::backdrop_blur` defines it: the dual-Kawase
// downsample and upsample passes, the composite, and the final blit.
//
// Every pass shares one bind group layout:
// - binding 0: dynamic uniform `BlurParams`
// - binding 1: `t_src`, the level the pass reads
// - binding 2: linear, clamp-to-edge sampler
// - binding 3: `t_snapshot`, the copy of the target taken for this blur batch
struct BlurParams {
    // Composite: the target's size in pixels.
    viewport_size: vec2<f32>,
    // The source level's texel size, in UV.
    texel_size: vec2<f32>,
    // The plan's sample offset, in source texels.
    offset: f32,
    // The target holds straight (not premultiplied) alpha: the blur works on
    // premultiplied values, so level 0 is premultiplied when read and the
    // composite converts back.
    straight_alpha: u32,
    // This pass reads level 0 (the snapshot).
    from_snapshot: u32,
    _pad: u32,
    bounds_origin: vec2<f32>,
    bounds_size: vec2<f32>,
    corner_radii: vec4<f32>, // (top_left, top_right, bottom_right, bottom_left)
    mask_origin: vec2<f32>,
    mask_size: vec2<f32>,
}

@group(0) @binding(0) var<uniform> params: BlurParams;
@group(0) @binding(1) var t_src: texture_2d<f32>;
@group(0) @binding(2) var s_linear: sampler;
@group(0) @binding(3) var t_snapshot: texture_2d<f32>;

struct FullscreenVarying {
    @builtin(position) position: vec4<f32>,
    @location(0) uv: vec2<f32>,
}

@vertex
fn vs_fullscreen(@builtin(vertex_index) vertex_index: u32) -> FullscreenVarying {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    let pos = positions[vertex_index];
    let uv = vec2<f32>(pos.x * 0.5 + 0.5, 0.5 - pos.y * 0.5);
    return FullscreenVarying(vec4<f32>(pos, 0.0, 1.0), uv);
}

fn premultiply(c: vec4<f32>) -> vec4<f32> {
    return vec4<f32>(c.rgb * c.a, c.a);
}

fn source(uv: vec2<f32>) -> vec4<f32> {
    let c = textureSample(t_src, s_linear, uv);
    if (params.from_snapshot != 0u && params.straight_alpha != 0u) {
        return premultiply(c);
    }
    return c;
}

// The 5-tap downsample.
@fragment
fn fs_kawase_downsample(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let d = params.texel_size * params.offset;
    var sum = source(input.uv) * 4.0;
    sum += source(input.uv + vec2<f32>(-d.x, -d.y));
    sum += source(input.uv + vec2<f32>(d.x, -d.y));
    sum += source(input.uv + vec2<f32>(-d.x, d.y));
    sum += source(input.uv + vec2<f32>(d.x, d.y));
    return sum / 8.0;
}

// The 8-tap tent upsample.
fn tent(uv: vec2<f32>) -> vec4<f32> {
    let d = params.texel_size * params.offset;
    var sum = source(uv + vec2<f32>(-d.x * 2.0, 0.0));
    sum += source(uv + vec2<f32>(-d.x, d.y)) * 2.0;
    sum += source(uv + vec2<f32>(0.0, d.y * 2.0));
    sum += source(uv + vec2<f32>(d.x, d.y)) * 2.0;
    sum += source(uv + vec2<f32>(d.x * 2.0, 0.0));
    sum += source(uv + vec2<f32>(d.x, -d.y)) * 2.0;
    sum += source(uv + vec2<f32>(0.0, -d.y * 2.0));
    sum += source(uv + vec2<f32>(-d.x, -d.y)) * 2.0;
    return sum / 12.0;
}

@fragment
fn fs_kawase_upsample(input: FullscreenVarying) -> @location(0) vec4<f32> {
    return tent(input.uv);
}

// Exact full-resolution copy of the offscreen scene to the frame.
@fragment
fn fs_blit(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(t_src, 0));
    let coord = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), dims - vec2<i32>(1));
    return textureLoad(t_src, coord, 0);
}

// One quad over the blur's bounds within its content mask.
@vertex
fn vs_backdrop_blur(@builtin(vertex_index) vertex_index: u32) -> FullscreenVarying {
    let unit_vertex = vec2<f32>(f32(vertex_index & 1u), 0.5 * f32(vertex_index & 2u));
    let clip_min = max(params.bounds_origin, params.mask_origin);
    let clip_max = min(params.bounds_origin + params.bounds_size, params.mask_origin + params.mask_size);
    let clipped_size = max(clip_max - clip_min, vec2<f32>(0.0));
    let px_pos = clip_min + unit_vertex * clipped_size;
    let ndc = vec2<f32>(
        (px_pos.x / params.viewport_size.x) * 2.0 - 1.0,
        1.0 - (px_pos.y / params.viewport_size.y) * 2.0,
    );
    return FullscreenVarying(vec4<f32>(ndc, 0.0, 1.0), px_pos / params.viewport_size);
}

// `gpui::backdrop_blur::rounded_rect_coverage`.
fn coverage(pixel: vec2<f32>) -> f32 {
    let half_size = params.bounds_size * 0.5;
    let rel = pixel - (params.bounds_origin + half_size);
    var corner = params.corner_radii.w; // bottom_left
    if (rel.x < 0.0 && rel.y < 0.0) {
        corner = params.corner_radii.x;
    } else if (rel.x >= 0.0 && rel.y < 0.0) {
        corner = params.corner_radii.y;
    } else if (rel.x >= 0.0 && rel.y >= 0.0) {
        corner = params.corner_radii.z;
    }
    let r = min(corner, min(half_size.x, half_size.y));
    let q = abs(rel) - (half_size - vec2<f32>(r));
    let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    return clamp(0.5 - dist, 0.0, 1.0);
}

// mix(snapshot, blurred, coverage), written with blending off.
@fragment
fn fs_backdrop_blur(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let blurred = tent(input.uv);
    let dims = vec2<i32>(textureDimensions(t_snapshot, 0));
    let coord = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), dims - vec2<i32>(1));
    var snapshot = textureLoad(t_snapshot, coord, 0);
    if (params.straight_alpha != 0u) {
        snapshot = premultiply(snapshot);
    }
    let cov = coverage(input.position.xy);
    if (cov <= 0.0) {
        discard;
    }
    let out = mix(snapshot, blurred, cov);
    if (params.straight_alpha != 0u) {
        if (out.a <= 0.0) {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(out.rgb / out.a, out.a);
    }
    return out;
}

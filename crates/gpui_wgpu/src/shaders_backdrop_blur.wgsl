// Dual-Kawase GPU frosted-glass backdrop blur, SDF rounded composite, and bit-exact blit shaders.
//
// All four passes share a single bind group layout:
// - binding 0: dynamic uniform `BackdropBlurParams` (80 bytes)
// - binding 1: source texture `t_src`
// - binding 2: linear clamp-to-edge sampler `s_linear`

struct BackdropBlurParams {
    viewport_size: vec2<f32>,
    texel_size: vec2<f32>,
    blur_radius: f32,
    kawase_offset: f32,
    premultiplied_alpha: u32,
    _pad: u32,
    bounds_origin: vec2<f32>,
    bounds_size: vec2<f32>,
    corner_radii: vec4<f32>, // (top_left, top_right, bottom_right, bottom_left)
    mask_origin: vec2<f32>,
    mask_size: vec2<f32>,
}

@group(0) @binding(0) var<uniform> params: BackdropBlurParams;
@group(0) @binding(1) var t_src: texture_2d<f32>;
@group(0) @binding(2) var s_linear: sampler;

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

// 5-tap Dual-Kawase downsample (2x resolution reduction per pass).
@fragment
fn fs_kawase_downsample(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let hp = params.texel_size * params.kawase_offset;
    var sum = textureSample(t_src, s_linear, input.uv) * 4.0;
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(-hp.x, -hp.y));
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(hp.x, -hp.y));
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(-hp.x, hp.y));
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(hp.x, hp.y));
    return sum / 8.0;
}

// 8-tap Dual-Kawase tent-filter upsample (silky smooth artifact-free blur).
@fragment
fn fs_kawase_upsample(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let hp = params.texel_size * params.kawase_offset;
    var sum = textureSample(t_src, s_linear, input.uv + vec2<f32>(-hp.x * 2.0, 0.0));
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(-hp.x, hp.y)) * 2.0;
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(0.0, hp.y * 2.0));
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(hp.x, hp.y)) * 2.0;
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(hp.x * 2.0, 0.0));
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(hp.x, -hp.y)) * 2.0;
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(0.0, -hp.y * 2.0));
    sum += textureSample(t_src, s_linear, input.uv + vec2<f32>(-hp.x, -hp.y)) * 2.0;
    return sum / 12.0;
}

// Bit-exact full-resolution blit from offscreen scene texture to swapchain target.
@fragment
fn fs_blit(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(t_src, 0));
    let coord = clamp(vec2<i32>(input.position.xy), vec2<i32>(0), dims - vec2<i32>(1));
    return textureLoad(t_src, coord, 0);
}

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
    let uv = px_pos / params.viewport_size;
    return FullscreenVarying(vec4<f32>(ndc, 0.0, 1.0), uv);
}

fn pick_corner_radius(pos: vec2<f32>, radii: vec4<f32>) -> f32 {
    if (pos.x < 0.0) {
        if (pos.y < 0.0) {
            return radii.x; // top_left
        } else {
            return radii.w; // bottom_left
        }
    } else {
        if (pos.y < 0.0) {
            return radii.y; // top_right
        } else {
            return radii.z; // bottom_right
        }
    }
}

@fragment
fn fs_backdrop_blur(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let frag_px = input.position.xy;
    let half_size = params.bounds_size * 0.5;
    let center = params.bounds_origin + half_size;
    let rel = frag_px - center;
    let radius = min(pick_corner_radius(rel, params.corner_radii), min(half_size.x, half_size.y));

    let q = abs(rel) - (half_size - vec2<f32>(radius));
    let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - radius;

    let edge_alpha = clamp(0.5 - dist, 0.0, 1.0);
    let strength = clamp(params.blur_radius / 3.0, 0.0, 1.0);
    let alpha = edge_alpha * strength;
    if (alpha <= 0.001) {
        discard;
    }

    let blurred = textureSample(t_src, s_linear, clamp(input.uv, vec2<f32>(0.0), vec2<f32>(1.0)));
    if (params.premultiplied_alpha != 0u) {
        return vec4<f32>(blurred.rgb * alpha, alpha);
    }
    return vec4<f32>(blurred.rgb, alpha);
}

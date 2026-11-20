// Backdrop blur and liquid glass, as `gpui::backdrop_blur` defines them: the
// dual-Kawase downsample and upsample passes, the composite, and the final
// blit. The HLSL (gpui_windows) and MSL (gpui_apple) are the same.
//
// Every pass shares one bind group layout:
// - binding 0: dynamic uniform `BlurParams` (`gpui::backdrop_blur::BlurParams`)
// - binding 1: `t_src`, the level the pass reads (the composite: level 1, or
//   the snapshot for clear glass)
// - binding 2: linear, clamp-to-edge sampler
// - binding 3: `t_snapshot`, the copy of the target taken for this blur batch
struct BlurParams {
    viewport_size: vec2<f32>,
    texel_size: vec2<f32>,
    offset: f32,
    // The target holds straight (not premultiplied) alpha: the blur works on
    // premultiplied values, so level 0 is premultiplied when read and the
    // composite converts back.
    straight_alpha: u32,
    from_snapshot: u32,
    // 1: blurred (level 1 is bound), 2: glass, bits 8+: progressive mode.
    flags: u32,
    bounds_origin: vec2<f32>,
    bounds_size: vec2<f32>,
    corner_radii: vec4<f32>, // (top_left, top_right, bottom_right, bottom_left)
    mask_origin: vec2<f32>,
    mask_size: vec2<f32>,
    refraction: f32,
    dispersion: f32,
    bevel_width: f32,
    specular: f32,
    pointer_pos: vec2<f32>,
    scale_factor: f32,
    shadow_opacity: f32,
    tint_rgba: vec4<f32>,
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
    let c = textureSampleLevel(t_src, s_linear, uv, 0.0);
    if (params.from_snapshot != 0u && params.straight_alpha != 0u) {
        return premultiply(c);
    }
    return c;
}

// The snapshot, premultiplied, sampled bilinearly (glass reads it at
// refracted positions).
fn snapshot_at(uv: vec2<f32>) -> vec4<f32> {
    let c = textureSampleLevel(t_snapshot, s_linear, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
    if (params.straight_alpha != 0u) {
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

// One quad over the blur's painted bounds (`BackdropBlur::painted_bounds`:
// the bounds, plus the glass's drop shadow) within its content mask.
@vertex
fn vs_backdrop_blur(@builtin(vertex_index) vertex_index: u32) -> FullscreenVarying {
    let unit_vertex = vec2<f32>(f32(vertex_index & 1u), 0.5 * f32(vertex_index & 2u));
    let scale = max(params.scale_factor, 1.0);
    let has_shadow = params.shadow_opacity > 0.0005;
    let pad_min = select(vec2<f32>(0.0), vec2<f32>(40.0, 32.0) * scale, has_shadow);
    let pad_max = select(vec2<f32>(0.0), vec2<f32>(40.0, 52.0) * scale, has_shadow);
    let clip_min = max(params.bounds_origin - pad_min, params.mask_origin);
    let clip_max = min(params.bounds_origin + params.bounds_size + pad_max, params.mask_origin + params.mask_size);
    let clipped_size = max(clip_max - clip_min, vec2<f32>(0.0));
    let px_pos = clip_min + unit_vertex * clipped_size;
    let ndc = vec2<f32>(
        (px_pos.x / params.viewport_size.x) * 2.0 - 1.0,
        1.0 - (px_pos.y / params.viewport_size.y) * 2.0,
    );
    return FullscreenVarying(vec4<f32>(ndc, 0.0, 1.0), px_pos / params.viewport_size);
}

fn pick_corner_radius(pos: vec2<f32>, radii: vec4<f32>) -> f32 {
    if (pos.x < 0.0) {
        if (pos.y < 0.0) {
            return radii.x; // top_left
        }
        return radii.w; // bottom_left
    }
    if (pos.y < 0.0) {
        return radii.y; // top_right
    }
    return radii.z; // bottom_right
}

fn rounded_rect_sdf(rel: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let r = min(pick_corner_radius(rel, radii), min(half_size.x, half_size.y));
    let q = abs(rel) - (half_size - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// The blurred backdrop at `uv`: the composite's final upsample.
fn blurred_at(uv: vec2<f32>) -> vec4<f32> {
    return tent(clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)));
}

// The backdrop through the glass at three refracted positions (one per
// color channel), `blur_mix` of the way from sharp to blurred.
fn glass_backdrop(uv_r: vec2<f32>, uv_g: vec2<f32>, uv_b: vec2<f32>, blur_mix: f32, blurred: bool) -> vec4<f32> {
    let sg = snapshot_at(uv_g);
    let sharp = vec4<f32>(snapshot_at(uv_r).r, sg.g, snapshot_at(uv_b).b, sg.a);
    let bg = blurred_at(uv_g);
    let soft = vec4<f32>(blurred_at(uv_r).r, bg.g, blurred_at(uv_b).b, bg.a);
    if (!blurred) {
        return sharp;
    }
    return mix(sharp, soft, blur_mix);
}

// The glass color at `frag_px` (premultiplied): refraction, dispersion,
// progressive blur, tint, and the lit bevel.
fn glass_color(frag_px: vec2<f32>, rel: vec2<f32>, half_size: vec2<f32>, q: vec2<f32>, dist: f32, blurred: bool) -> vec4<f32> {
    let scale = max(params.scale_factor, 1.0);
    let q_pos = max(q, vec2<f32>(0.0));
    let outside_len = length(q_pos);
    let depth_in = max(-dist, 0.0);
    let bevel = clamp(params.bevel_width, 4.0 * scale, min(half_size.x, half_size.y));
    let u = clamp(depth_in / max(bevel, 1.0), 0.0, 1.0);
    let one_minus_u = 1.0 - u;
    // The outward normal of the rounded rectangle, smooth across corners.
    let sgn = vec2<f32>(select(-1.0, 1.0, rel.x >= 0.0), select(-1.0, 1.0, rel.y >= 0.0));
    var grad: vec2<f32>;
    if (outside_len > 0.0001) {
        grad = (q_pos / outside_len) * sgn;
    } else {
        let diag_blend = smoothstep(-2.5 * scale, 2.5 * scale, q.x - q.y);
        grad = normalize(vec2<f32>(sgn.x * diag_blend, sgn.y * (1.0 - diag_blend)) + vec2<f32>(1e-5));
    }
    // A convex bevel and a slight dome.
    let rim_slope = pow(one_minus_u, 1.85);
    let body_tilt = (rel / half_size) * (0.10 * min(params.refraction, 1.5));
    let xy_tilt = grad * (rim_slope * 0.76) + body_tilt;
    let n_z = sqrt(max(1.0 - min(dot(xy_tilt, xy_tilt), 0.96), 0.04));
    let normal = normalize(vec3<f32>(xy_tilt, n_z));
    // Snell refraction offset in pixels (`BackdropBlur::reach` bounds it).
    let ior = 1.0 + 0.42 * params.refraction;
    let eta = 1.0 / ior;
    let k = max(1.0 - eta * eta * (1.0 - normal.z * normal.z), 0.0);
    let snell_xy = normal.xy * (eta * normal.z - sqrt(k));
    let thickness_px = bevel * (1.05 * params.refraction);
    let border_dist = min(
        min(frag_px.x, params.viewport_size.x - frag_px.x),
        min(frag_px.y, params.viewport_size.y - frag_px.y),
    );
    let refr_px = snell_xy * thickness_px * smoothstep(0.0, 16.0 * scale, border_dist);
    // Chromatic dispersion along the bevel.
    let disp_amt = params.dispersion * (0.14 + 0.36 * rim_slope);
    let uv_r = (frag_px + refr_px * (1.0 - disp_amt)) / params.viewport_size;
    let uv_g = (frag_px + refr_px) / params.viewport_size;
    let uv_b = (frag_px + refr_px * (1.0 + disp_amt * 1.25)) / params.viewport_size;
    // Progressive blur: 0 sharp, 1 the full blur.
    let mode = params.flags >> 8u;
    let local_uv = clamp((frag_px - params.bounds_origin) / max(params.bounds_size, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
    var prog = 1.0;
    if (mode == 1u) {
        prog = smoothstep(0.0, 1.0, local_uv.y);
    } else if (mode == 2u) {
        prog = smoothstep(1.0, 0.0, local_uv.y);
    } else if (mode == 3u) {
        prog = smoothstep(0.0, 1.0, local_uv.x);
    } else if (mode == 4u) {
        prog = smoothstep(1.0, 0.0, local_uv.x);
    } else if (mode == 5u) {
        prog = smoothstep(0.10, 1.0, pow(one_minus_u, 0.85));
    }
    let backdrop = glass_backdrop(uv_r, uv_g, uv_b, prog, blurred);
    // The lighting works on straight color, over the backdrop's coverage.
    let a = backdrop.a;
    var rgb = select(vec3<f32>(0.0), backdrop.rgb / max(a, 1e-4), a > 0.0);
    let tint_a = params.tint_rgba.a * select(0.35 + 0.65 * prog, 1.0, mode == 0u);
    rgb = mix(rgb, params.tint_rgba.rgb, tint_a);
    let luma = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
    rgb = mix(vec3<f32>(luma), rgb, 1.0 + 0.12 * min(params.refraction, 1.2));
    // Fresnel rim, key and bounce highlights, the bevel lip, an inner caustic.
    let v_dir = vec3<f32>(0.0, 0.0, 1.0);
    let fresnel = pow(1.0 - clamp(normal.z, 0.0, 1.0), 2.2);
    let key_h = normalize(normalize(vec3<f32>(-0.55, -0.78, 0.65)) + v_dir);
    let key_spec = pow(max(dot(normal, key_h), 0.0), 18.0);
    let bounce_h = normalize(normalize(vec3<f32>(0.55, 0.78, 0.45)) + v_dir);
    let bounce_spec = pow(max(dot(normal, bounce_h), 0.0), 10.0) * 0.45;
    let rim_line = smoothstep(2.4 * scale, 0.0, depth_in);
    let rim_dir_weight = 0.42 + 0.58 * max(dot(grad, normalize(vec2<f32>(-0.55, -0.83))), 0.0)
        + 0.34 * max(dot(grad, normalize(vec2<f32>(0.55, 0.83))), 0.0);
    let inner_caustic = exp(-((u - 0.15) * (u - 0.15)) / (2.0 * 0.09 * 0.09))
        * (0.24 + 0.56 * max(dot(grad, normalize(vec2<f32>(-0.5, -0.86))), 0.0));
    // A light following the pointer.
    var ptr_spec = 0.0;
    if (params.pointer_pos.x >= -1000.0) {
        let ptr_delta = params.pointer_pos - frag_px;
        let ptr_dist = length(ptr_delta);
        let ptr_sigma = 210.0 * scale;
        let ptr_falloff = exp(-(ptr_dist * ptr_dist) / (2.0 * ptr_sigma * ptr_sigma));
        let ptr_h = normalize(normalize(vec3<f32>(ptr_delta, 140.0 * scale)) + v_dir);
        ptr_spec = pow(max(dot(normal, ptr_h), 0.0), 22.0) * ptr_falloff * (0.28 + 0.72 * fresnel + 0.35 * rim_line);
    }
    let spec_energy = (fresnel * 0.25
        + key_spec * (0.24 + 0.44 * fresnel)
        + bounce_spec * (0.14 + 0.28 * fresnel)
        + rim_line * rim_dir_weight * 0.44
        + inner_caustic * 0.18
        + ptr_spec * 0.48)
        * params.specular;
    // A faint dark rim over bright backdrops, so clear glass still shows on white.
    let bright_bg = smoothstep(0.48, 0.92, luma);
    let outer_bevel_shade = smoothstep(2.0 * scale, 0.0, depth_in) * (0.07 + 0.22 * bright_bg) * params.specular;
    let inner_rim_contrast = exp(-((depth_in - 2.6 * scale) * (depth_in - 2.6 * scale)) / (2.0 * (1.8 * scale) * (1.8 * scale)))
        * 0.075 * bright_bg * params.specular;
    rgb = rgb * (1.0 - outer_bevel_shade - inner_rim_contrast) + vec3<f32>(0.97, 0.99, 1.04) * spec_energy;
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(rgb * a, a);
}

// The glass's drop shadow, outside the shape only.
fn shadow_alpha(rel: vec2<f32>, half_size: vec2<f32>, outside: f32) -> f32 {
    if (params.shadow_opacity <= 0.0005 || outside <= 0.001) {
        return 0.0;
    }
    let scale = max(params.scale_factor, 1.0);
    let d_amb = max(rounded_rect_sdf(rel - vec2<f32>(0.0, 9.0 * scale), half_size, params.corner_radii) + 3.0 * scale, 0.0);
    let amb = exp(-(d_amb * d_amb) / (2.0 * (12.5 * scale) * (12.5 * scale))) * smoothstep(38.0 * scale, 16.0 * scale, d_amb);
    let d_con = max(rounded_rect_sdf(rel - vec2<f32>(0.0, 2.5 * scale), half_size, params.corner_radii) + 0.5 * scale, 0.0);
    let con = exp(-(d_con * d_con) / (2.0 * (3.6 * scale) * (3.6 * scale)));
    return clamp((amb * 0.72 + con * 0.34) * params.shadow_opacity * outside, 0.0, 0.85);
}

// The snapshot with the blur (or glass) mixed in by coverage, and the
// glass's shadow over it outside the shape; written with blending off.
@fragment
fn fs_backdrop_blur(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let frag_px = input.position.xy;
    let blurred = (params.flags & 1u) != 0u;
    let glass = (params.flags & 2u) != 0u;
    let half_size = max(params.bounds_size * 0.5, vec2<f32>(0.5));
    let rel = frag_px - (params.bounds_origin + half_size);
    let r = min(pick_corner_radius(rel, params.corner_radii), min(half_size.x, half_size.y));
    let q = abs(rel) - (half_size - vec2<f32>(r));
    let dist = length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
    let cov = clamp(0.5 - dist, 0.0, 1.0);
    let shadow_a = shadow_alpha(rel, half_size, 1.0 - cov);

    var inside = blurred_at(input.uv);
    if (glass) {
        inside = glass_color(frag_px, rel, half_size, q, dist, blurred);
    }
    let dims = vec2<i32>(textureDimensions(t_snapshot, 0));
    let coord = clamp(vec2<i32>(frag_px), vec2<i32>(0), dims - vec2<i32>(1));
    var backdrop = textureLoad(t_snapshot, coord, 0);
    if (params.straight_alpha != 0u) {
        backdrop = premultiply(backdrop);
    }
    if (cov <= 0.0 && shadow_a <= 0.0) {
        discard;
    }
    var out = mix(backdrop, inside, cov);
    // The shadow: premultiplied "over".
    let shadow = vec4<f32>(vec3<f32>(0.02, 0.04, 0.09) * shadow_a, shadow_a);
    out = shadow + out * (1.0 - shadow_a);
    if (params.straight_alpha != 0u) {
        if (out.a <= 0.0) {
            return vec4<f32>(0.0);
        }
        return vec4<f32>(out.rgb / out.a, out.a);
    }
    return out;
}

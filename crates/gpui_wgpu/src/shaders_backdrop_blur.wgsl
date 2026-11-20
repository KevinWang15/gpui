// Dual-Kawase GPU frosted-glass backdrop blur, optical LiquidGlass refraction/dispersion/Fresnel,
// continuous progressive blur, and bit-exact blit shaders.
//
// All four passes share a single bind group layout:
// - binding 0: dynamic uniform `BackdropBlurParams` (128 bytes)
// - binding 1: primary source texture `t_src` (scene or Kawase mip)
// - binding 2: linear clamp-to-edge sampler `s_linear`
// - binding 3: full-resolution sharp scene copy `t_sharp` (used by LiquidGlass / progressive blur)

struct BackdropBlurParams {
    viewport_size: vec2<f32>,
    texel_size: vec2<f32>,
    blur_radius: f32,
    kawase_offset: f32,
    premultiplied_alpha: u32,
    progressive_mode: u32,
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

@group(0) @binding(0) var<uniform> params: BackdropBlurParams;
@group(0) @binding(1) var t_src: texture_2d<f32>;
@group(0) @binding(2) var s_linear: sampler;
@group(0) @binding(3) var t_sharp: texture_2d<f32>;

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

fn rounded_rect_sdf(rel: vec2<f32>, half_size: vec2<f32>, radii: vec4<f32>) -> f32 {
    let r = min(pick_corner_radius(rel, radii), min(half_size.x, half_size.y));
    let q = abs(rel) - (half_size - vec2<f32>(r));
    return length(max(q, vec2<f32>(0.0))) + min(max(q.x, q.y), 0.0) - r;
}

fn sample_glass_layer(
    uv_r: vec2<f32>,
    uv_g: vec2<f32>,
    uv_b: vec2<f32>,
    blur_mix: f32,
) -> vec4<f32> {
    let cr = clamp(uv_r, vec2<f32>(0.0), vec2<f32>(1.0));
    let cg = clamp(uv_g, vec2<f32>(0.0), vec2<f32>(1.0));
    let cb = clamp(uv_b, vec2<f32>(0.0), vec2<f32>(1.0));

    if (blur_mix <= 0.001) {
        let sg = textureSampleLevel(t_sharp, s_linear, cg, 0.0);
        return vec4<f32>(
            textureSampleLevel(t_sharp, s_linear, cr, 0.0).r,
            sg.g,
            textureSampleLevel(t_sharp, s_linear, cb, 0.0).b,
            sg.a,
        );
    }
    if (blur_mix >= 0.999) {
        let bg = textureSampleLevel(t_src, s_linear, cg, 0.0);
        return vec4<f32>(
            textureSampleLevel(t_src, s_linear, cr, 0.0).r,
            bg.g,
            textureSampleLevel(t_src, s_linear, cb, 0.0).b,
            bg.a,
        );
    }
    let sg = textureSampleLevel(t_sharp, s_linear, cg, 0.0);
    let sharp = vec4<f32>(
        textureSampleLevel(t_sharp, s_linear, cr, 0.0).r,
        sg.g,
        textureSampleLevel(t_sharp, s_linear, cb, 0.0).b,
        sg.a,
    );
    let bg = textureSampleLevel(t_src, s_linear, cg, 0.0);
    let blurred = vec4<f32>(
        textureSampleLevel(t_src, s_linear, cr, 0.0).r,
        bg.g,
        textureSampleLevel(t_src, s_linear, cb, 0.0).b,
        bg.a,
    );
    return mix(sharp, blurred, blur_mix);
}

@fragment
fn fs_backdrop_blur(input: FullscreenVarying) -> @location(0) vec4<f32> {
    let frag_px = input.position.xy;
    let half_size = max(params.bounds_size * 0.5, vec2<f32>(0.5));
    let center = params.bounds_origin + half_size;
    let rel = frag_px - center;
    let radius = min(pick_corner_radius(rel, params.corner_radii), min(half_size.x, half_size.y));

    let q = abs(rel) - (half_size - vec2<f32>(radius));
    let q_pos = max(q, vec2<f32>(0.0));
    let outside_len = length(q_pos);
    let dist = outside_len + min(max(q.x, q.y), 0.0) - radius;

    let scale = max(params.scale_factor, 1.0);
    let edge_alpha = clamp(0.5 - dist, 0.0, 1.0);
    let outside_alpha = 1.0 - edge_alpha;

    // Exterior-only analytical optical drop shadow (0% shadow leaks under the transparent glass interior!).
    var shadow_a = 0.0;
    if (params.shadow_opacity > 0.0005 && outside_alpha > 0.001) {
        let dist_amb = rounded_rect_sdf(rel - vec2<f32>(0.0, 9.0 * scale), half_size, params.corner_radii);
        let d_amb = max(dist_amb + 3.0 * scale, 0.0);
        let amb = exp(-(d_amb * d_amb) / (2.0 * (12.5 * scale) * (12.5 * scale)))
            * smoothstep(38.0 * scale, 16.0 * scale, d_amb);

        let dist_con = rounded_rect_sdf(rel - vec2<f32>(0.0, 2.5 * scale), half_size, params.corner_radii);
        let d_con = max(dist_con + 0.5 * scale, 0.0);
        let con = exp(-(d_con * d_con) / (2.0 * (3.6 * scale) * (3.6 * scale)));

        shadow_a = clamp((amb * 0.72 + con * 0.34) * params.shadow_opacity * outside_alpha, 0.0, 0.85);
    }

    let shadow_rgb = vec3<f32>(0.02, 0.04, 0.09);
    if (edge_alpha <= 0.001) {
        if (shadow_a <= 0.001) {
            discard;
        }
        if (params.premultiplied_alpha != 0u) {
            return vec4<f32>(shadow_rgb * shadow_a, shadow_a);
        }
        return vec4<f32>(shadow_rgb, shadow_a);
    }

    let is_glass = params.refraction > 0.0005
        || params.specular > 0.0005
        || params.progressive_mode != 0u
        || params.tint_rgba.a > 0.0005
        || params.shadow_opacity > 0.0005;

    // Classic fast path for standard `div().backdrop_blur(...)` elements.
    if (!is_glass) {
        let strength = clamp(params.blur_radius / (3.0 * scale), 0.0, 1.0);
        let alpha = edge_alpha * strength;
        if (alpha <= 0.001) {
            discard;
        }
        let blurred = textureSampleLevel(
            t_src,
            s_linear,
            clamp(input.uv, vec2<f32>(0.0), vec2<f32>(1.0)),
            0.0,
        );
        if (params.premultiplied_alpha != 0u) {
            return vec4<f32>(blurred.rgb * alpha, alpha);
        }
        return vec4<f32>(blurred.rgb, alpha);
    }

    // --- Optical LiquidGlass path ---
    let depth_in = max(-dist, 0.0);
    let bevel = clamp(params.bevel_width, 4.0 * scale, min(half_size.x, half_size.y));
    let u = clamp(depth_in / max(bevel, 1.0), 0.0, 1.0);
    let one_minus_u = 1.0 - u;

    // Smooth exact analytical outward SDF normal (handles straight edges + rounded corners seamlessly).
    let sgn = vec2<f32>(select(-1.0, 1.0, rel.x >= 0.0), select(-1.0, 1.0, rel.y >= 0.0));
    var grad: vec2<f32>;
    if (outside_len > 0.0001) {
        grad = (q_pos / outside_len) * sgn;
    } else {
        let diag_blend = smoothstep(-2.5 * scale, 2.5 * scale, q.x - q.y);
        grad = normalize(vec2<f32>(sgn.x * diag_blend, sgn.y * (1.0 - diag_blend)) + vec2<f32>(1e-5));
    }

    // Convex 3D glass bevel profile + subtle central dome lens curvature (strictly monotonic — no fold-back).
    let rim_slope = pow(one_minus_u, 1.85);
    let body_tilt = (rel / half_size) * (0.10 * min(params.refraction, 1.5));
    let xy_tilt = grad * (rim_slope * 0.76) + body_tilt;
    let n_z = sqrt(max(1.0 - min(dot(xy_tilt, xy_tilt), 0.96), 0.04));
    let normal = normalize(vec3<f32>(xy_tilt, n_z));

    // Snell's Law refraction offset in physical pixels (pulls curved edges inward like thick optical glass).
    let ior = 1.0 + 0.42 * params.refraction;
    let eta = 1.0 / ior;
    let k = max(1.0 - eta * eta * (1.0 - normal.z * normal.z), 0.0);
    let snell_xy = normal.xy * (eta * normal.z - sqrt(k));
    let thickness_px = bevel * (1.05 * params.refraction);
    let border_dist = min(
        min(frag_px.x, params.viewport_size.x - frag_px.x),
        min(frag_px.y, params.viewport_size.y - frag_px.y),
    );
    let vp_edge_damp = smoothstep(0.0, 16.0 * scale, border_dist);
    let refr_px = snell_xy * thickness_px * vp_edge_damp;

    // Chromatic RGB prism dispersion concentrated along the curved glass bevel.
    let disp_amt = params.dispersion * (0.14 + 0.36 * rim_slope);
    let uv_r = (frag_px + refr_px * (1.0 - disp_amt)) / params.viewport_size;
    let uv_g = (frag_px + refr_px) / params.viewport_size;
    let uv_b = (frag_px + refr_px * (1.0 + disp_amt * 1.25)) / params.viewport_size;

    // Continuous progressive blur gradient (0 = crystal-clear sharp, 1 = full Dual-Kawase frost).
    let local_uv = clamp((frag_px - params.bounds_origin) / max(params.bounds_size, vec2<f32>(1.0)), vec2<f32>(0.0), vec2<f32>(1.0));
    var prog = 1.0;
    if (params.progressive_mode == 1u) {
        prog = smoothstep(0.0, 1.0, local_uv.y);
    } else if (params.progressive_mode == 2u) {
        prog = smoothstep(1.0, 0.0, local_uv.y);
    } else if (params.progressive_mode == 3u) {
        prog = smoothstep(0.0, 1.0, local_uv.x);
    } else if (params.progressive_mode == 4u) {
        prog = smoothstep(1.0, 0.0, local_uv.x);
    } else if (params.progressive_mode == 5u) {
        prog = smoothstep(0.10, 1.0, pow(one_minus_u, 0.85));
    }
    let blur_mix = clamp(params.blur_radius / (5.0 * scale), 0.0, 1.0) * prog;

    let glass_sample = sample_glass_layer(uv_r, uv_g, uv_b, blur_mix);
    var rgb = glass_sample.rgb;

    // Subtle glass body tint (modulated by progressive curve when progressive blur is active).
    let tint_a = params.tint_rgba.a * select(0.35 + 0.65 * prog, 1.0, params.progressive_mode == 0u);
    rgb = mix(rgb, params.tint_rgba.rgb, tint_a);

    // Subtle optical brightness & saturation lift typical of thick clear glass.
    let luma = dot(rgb, vec3<f32>(0.299, 0.587, 0.114));
    rgb = mix(vec3<f32>(luma), rgb, 1.0 + 0.12 * min(params.refraction, 1.2));

    // Analytical 3D Fresnel specular rim + key/bounce edge catchlights + live cursor sheen.
    let v_dir = vec3<f32>(0.0, 0.0, 1.0);
    let fresnel = pow(1.0 - clamp(normal.z, 0.0, 1.0), 2.2);

    let key_l = normalize(vec3<f32>(-0.55, -0.78, 0.65));
    let key_h = normalize(key_l + v_dir);
    let key_spec = pow(max(dot(normal, key_h), 0.0), 18.0);

    let bounce_l = normalize(vec3<f32>(0.55, 0.78, 0.45));
    let bounce_h = normalize(bounce_l + v_dir);
    let bounce_spec = pow(max(dot(normal, bounce_h), 0.0), 10.0) * 0.45;

    // Crisp outer glass bevel lip + bright inner caustic ring.
    let rim_line = smoothstep(2.4 * scale, 0.0, depth_in);
    let rim_dir_weight = 0.42 + 0.58 * max(dot(grad, normalize(vec2<f32>(-0.55, -0.83))), 0.0)
        + 0.34 * max(dot(grad, normalize(vec2<f32>(0.55, 0.83))), 0.0);
    let inner_caustic = exp(-((u - 0.15) * (u - 0.15)) / (2.0 * 0.09 * 0.09))
        * (0.24 + 0.56 * max(dot(grad, normalize(vec2<f32>(-0.5, -0.86))), 0.0));

    // Live pointer light sliding along the 3D curved corners.
    var ptr_spec = 0.0;
    if (params.pointer_pos.x >= -1000.0) {
        let ptr_delta = params.pointer_pos - frag_px;
        let ptr_dist = length(ptr_delta);
        let ptr_sigma = 210.0 * scale;
        let ptr_falloff = exp(-(ptr_dist * ptr_dist) / (2.0 * ptr_sigma * ptr_sigma));
        let ptr_l = normalize(vec3<f32>(ptr_delta, 140.0 * scale));
        let ptr_h = normalize(ptr_l + v_dir);
        ptr_spec = pow(max(dot(normal, ptr_h), 0.0), 22.0)
            * ptr_falloff
            * (0.28 + 0.72 * fresnel + 0.35 * rim_line);
    }

    let spec_energy = (fresnel * 0.25
        + key_spec * (0.24 + 0.44 * fresnel)
        + bounce_spec * (0.14 + 0.28 * fresnel)
        + rim_line * rim_dir_weight * 0.44
        + inner_caustic * 0.18
        + ptr_spec * 0.48)
        * params.specular;

    // Adaptive silhouette definition: delicate cool-slate grazing refraction rim on bright
    // backgrounds so crystal glass pops crisply over pure white, invisible on dark backgrounds.
    let bright_bg = smoothstep(0.48, 0.92, luma);
    let outer_bevel_shade = smoothstep(2.0 * scale, 0.0, depth_in)
        * (0.07 + 0.22 * bright_bg)
        * params.specular;
    let inner_rim_contrast = exp(-((depth_in - 2.6 * scale) * (depth_in - 2.6 * scale)) / (2.0 * (1.8 * scale) * (1.8 * scale)))
        * 0.075 * bright_bg * params.specular;

    rgb = rgb * (1.0 - outer_bevel_shade - inner_rim_contrast)
        + vec3<f32>(0.97, 0.99, 1.04) * spec_energy;
    rgb = clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0));

    let out_alpha = clamp(edge_alpha + shadow_a, 0.0, 1.0);
    let premul_rgb = rgb * edge_alpha + shadow_rgb * shadow_a;
    if (params.premultiplied_alpha != 0u) {
        return vec4<f32>(premul_rgb, out_alpha);
    }
    return vec4<f32>(premul_rgb / max(out_alpha, 0.0001), out_alpha);
}

struct ViewTransitionParams {
    origin_px: vec2<f32>,
    radius_px: f32,
    feather_px: f32,
    progress: f32,
    kind: u32,
    viewport_px: vec2<f32>,
}

@group(0) @binding(0) var<uniform> params: ViewTransitionParams;
@group(0) @binding(1) var t_old: texture_2d<f32>;
@group(0) @binding(2) var t_new: texture_2d<f32>;
@group(0) @binding(3) var s_linear: sampler;

const VT_PI: f32 = 3.14159265359;

@vertex
fn vs_view_transition(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

fn vt_sample_old(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(t_old, s_linear, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

fn vt_sample_new(uv: vec2<f32>) -> vec4<f32> {
    return textureSampleLevel(t_new, s_linear, clamp(uv, vec2<f32>(0.0), vec2<f32>(1.0)), 0.0);
}

fn vt_load_old(frag_pos: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let coord = clamp(vec2<i32>(frag_pos), vec2<i32>(0), dims_i - vec2<i32>(1));
    return textureLoad(t_old, coord, 0);
}

fn vt_load_new(frag_pos: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let coord = clamp(vec2<i32>(frag_pos), vec2<i32>(0), dims_i - vec2<i32>(1));
    return textureLoad(t_new, coord, 0);
}

fn vt_rect_sdf(p: vec2<f32>, dims: vec2<f32>) -> f32 {
    return max(max(-p.x, p.x - dims.x), max(-p.y, p.y - dims.y));
}

// Deterministic sin-free 2D hash -> [0, 1] (identical across GPU vendors).
fn vt_hash21(p: vec2<f32>) -> f32 {
    var p3 = fract(vec3<f32>(p.x, p.y, p.x) * 0.1031);
    p3 += vec3<f32>(dot(p3, vec3<f32>(p3.y + 33.33, p3.z + 33.33, p3.x + 33.33)));
    return fract((p3.x + p3.y) * p3.z);
}

fn vt_noise2d(p: vec2<f32>) -> f32 {
    let i = floor(p);
    let f = fract(p);
    let u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    let a = vt_hash21(i);
    let b = vt_hash21(i + vec2<f32>(1.0, 0.0));
    let c = vt_hash21(i + vec2<f32>(0.0, 1.0));
    let d = vt_hash21(i + vec2<f32>(1.0, 1.0));
    return mix(mix(a, b, u.x), mix(c, d, u.x), u.y);
}

fn vt_fbm(p_in: vec2<f32>) -> f32 {
    var p = p_in;
    var val = 0.0;
    var amp = 0.5;
    for (var i = 0; i < 4; i += 1) {
        val += amp * vt_noise2d(p);
        // Mild rotation + lacunarity 2.02 eliminates axis-aligned grid artifacts.
        p = vec2<f32>(p.x * 1.60 - p.y * 1.24, p.x * 1.24 + p.y * 1.60) + vec2<f32>(13.7, 7.3);
        amp *= 0.5;
    }
    return val / 0.9375;
}

fn vt_warped_fbm(q: vec2<f32>) -> f32 {
    let wx = vt_fbm(q + vec2<f32>(5.2, 1.3));
    let wy = vt_fbm(q + vec2<f32>(1.7, 9.2));
    return vt_fbm(q + vec2<f32>(wx - 0.5, wy - 0.5) * 1.35);
}

fn vt_shockwave(frag_pos: vec2<f32>, dims: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let scale = max(params.feather_px, 1.0);
    let p = clamp(params.progress, 0.0, 1.0);
    let delta = frag_pos - params.origin_px;
    let dist = length(delta);
    let dir = delta / max(dist, 0.001);

    let band = 82.0 * scale;
    let d = dist - params.radius_px;
    let wake_d = d + band * 0.88;
    let time_env = smoothstep(0.0, 0.07, p) * smoothstep(1.0, 0.84, p);

    // Fast exact path for pixels outside the active shockwave + trailing wake.
    if ((abs(d) > band * 2.4 && abs(wake_d) > band * 1.8) || time_env <= 0.0005) {
        if (d < 0.0) {
            return vt_load_new(frag_pos, dims_i);
        }
        return vt_load_old(frag_pos, dims_i);
    }

    let sigma_main = band * 0.44;
    let wave_env = exp(-(d * d) / (2.0 * sigma_main * sigma_main)) * time_env;
    let ripple = sin((d / band) * VT_PI * 2.0) * wave_env;

    let sigma_wake = band * 0.30;
    let wake_env = exp(-(wake_d * wake_d) / (2.0 * sigma_wake * sigma_wake)) * time_env * 0.28;
    let wake_ripple = sin((wake_d / band) * 4.8) * wake_env;

    // Gently damp displacement near viewport edges so borders never smear.
    let border_dist = min(min(frag_pos.x, dims.x - frag_pos.x), min(frag_pos.y, dims.y - frag_pos.y));
    let edge_damp = smoothstep(0.0, 22.0 * scale, border_dist);

    let disp_px = (ripple + wake_ripple) * (20.0 * scale) * edge_damp;
    let uv_r = (frag_pos - dir * disp_px * 0.76) / dims;
    let uv_g = (frag_pos - dir * disp_px * 1.00) / dims;
    let uv_b = (frag_pos - dir * disp_px * 1.28) / dims;

    let old_g = vt_sample_old(uv_g);
    let old_col = vec4<f32>(vt_sample_old(uv_r).r, old_g.g, vt_sample_old(uv_b).b, old_g.a);
    let new_g = vt_sample_new(uv_g);
    let new_col = vec4<f32>(vt_sample_new(uv_r).r, new_g.g, vt_sample_new(uv_b).b, new_g.a);

    let mix_w = 18.0 * scale;
    let show_old = smoothstep(-mix_w * 0.5, mix_w * 0.5, d);
    var color = mix(new_col, old_col, show_old);

    // Specular caustic glint riding the leading crest + subtle depth trough behind it.
    let crest_sigma = band * 0.13;
    let crest_d = d - band * 0.05;
    let crest = exp(-(crest_d * crest_d) / (2.0 * crest_sigma * crest_sigma)) * time_env;
    let light_dir = normalize(vec2<f32>(-0.55, -0.83));
    let spec_dir = 0.58 + 0.42 * max(dot(dir, light_dir), 0.0);
    let caustic = crest * spec_dir * 0.23;

    let trough_sigma = band * 0.17;
    let trough_d = d + band * 0.22;
    let trough = exp(-(trough_d * trough_d) / (2.0 * trough_sigma * trough_sigma)) * time_env * 0.10;

    let lit_rgb = color.rgb * (1.0 - trough) + vec3<f32>(caustic * 0.90, caustic * 0.97, caustic * 1.08) * color.a;
    color = vec4<f32>(clamp(lit_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
    return color;
}

fn vt_vellum_backside(back_tex: vec4<f32>, shade: f32, spec: f32, edge_rim: f32) -> vec4<f32> {
    let luma = dot(back_tex.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let vellum = mix(vec3<f32>(0.16, 0.18, 0.24), vec3<f32>(0.95, 0.96, 0.98), smoothstep(0.16, 0.72, luma));
    let base_rgb = mix(back_tex.rgb, vellum, 0.50) * shade
        + vec3<f32>(spec * 0.96, spec * 0.98, spec * 1.02)
        + vec3<f32>(edge_rim * 0.22);
    return vec4<f32>(clamp(base_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), back_tex.a);
}

fn vt_page_curl(frag_pos: vec2<f32>, dims: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let scale = max(params.feather_px, 1.0);
    let p = clamp(params.progress, 0.0, 1.0);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos, dims_i);
    }

    let start_corner = vec2<f32>(
        select(0.0, dims.x, params.origin_px.x >= dims.x * 0.5),
        select(0.0, dims.y, params.origin_px.y >= dims.y * 0.5),
    );
    let end_corner = dims - start_corner;
    let diag_vec = end_corner - start_corner;
    let diag = max(length(diag_vec), 1.0);
    let curl_dir = diag_vec / diag;

    let R = clamp(min(dims.x, dims.y) * 0.135, 48.0 * scale, 148.0 * scale);
    let start_fade = smoothstep(0.0, 0.07, p);
    let end_fade = smoothstep(1.0, 0.88, p);
    let axis_s = mix(0.0, diag + R * 2.65, p);

    let s = dot(frag_pos - start_corner, curl_dir);
    let d = s - axis_s;

    // Region A: ahead of cylinder tangent (unpeeled old page, possibly covered by folded-back flap).
    if (d > 0.0) {
        let p_back = frag_pos - (VT_PI * R + 2.0 * d) * curl_dir;
        let bdist = vt_rect_sdf(p_back, dims);

        var base_old = vt_load_old(frag_pos, dims_i);
        // Subtle crease AO right next to the cylinder tangent + soft drop shadow from lifted flap.
        let crease_ao = 1.0 - 0.11 * exp(-d / (R * 0.28)) * start_fade;
        let flap_shadow = select(
            1.0,
            1.0 - 0.34 * exp(-bdist / (R * 0.28)) * smoothstep(R * 1.25, 0.0, bdist) * start_fade,
            bdist > 0.0,
        );
        base_old = vec4<f32>(base_old.rgb * crease_ao * flap_shadow, base_old.a);

        if (bdist <= 0.85) {
            let back_tex = vt_sample_old(p_back / dims);
            // Match cylinder top lighting seamlessly at d = 0 (n = (0, 0, 1)) for C1 continuity.
            let l_dir = normalize(vec3<f32>(-0.42, -0.55, 0.72));
            let h_vec = normalize(l_dir + vec3<f32>(0.0, 0.0, 1.0));
            let flat_diffuse = 0.62 + 0.38 * l_dir.z;
            let flat_spec = pow(max(h_vec.z, 0.0), 20.0) * 0.40 * start_fade;
            let shade = mix(flat_diffuse, 0.96, smoothstep(0.0, R * 1.4, d));
            let spec = flat_spec * exp(-d / (R * 0.55));
            let edge_rim = smoothstep(2.2 * scale, 0.0, abs(bdist)) * start_fade;
            let back_col = vt_vellum_backside(back_tex, shade, spec, edge_rim);
            let aa = smoothstep(0.85, -0.85, bdist);
            return mix(base_old, back_col, aa);
        }
        return base_old;
    }

    // Region B: directly on/under the 3D cylinder (-R <= d <= 0).
    if (d >= -R) {
        let sin_t = clamp(-d / R, 0.0, 1.0);
        let theta = asin(sin_t);
        let cos_t = sqrt(max(1.0 - sin_t * sin_t, 0.0));

        // Base layer: revealed NEW page with deep contact shadow under the curving cylinder.
        let new_raw = vt_load_new(frag_pos, dims_i);
        let under_shadow = 1.0 - 0.48 * pow(cos_t, 1.15) * start_fade;
        var out_col = vec4<f32>(new_raw.rgb * under_shadow, new_raw.a);

        // Bottom half of cylinder: front side of OLD page curving up off the plane.
        let s_bot = axis_s - R * theta;
        let p_bot = frag_pos + (s_bot - s) * curl_dir;
        let dist_bot = vt_rect_sdf(p_bot, dims);
        if (dist_bot <= 0.85) {
            let bot_tex = vt_sample_old(p_bot / dims);
            let bot_shade = 0.80 + 0.20 * cos_t;
            let bot_col = vec4<f32>(bot_tex.rgb * bot_shade, bot_tex.a);
            let aa_bot = smoothstep(0.85, -0.85, dist_bot);
            out_col = mix(out_col, bot_col, aa_bot);
        }

        // Top half of cylinder: translucent curled-over backside with 3D cylindrical specular ridge!
        let theta_top = VT_PI - theta;
        let s_top = axis_s - R * theta_top;
        let p_top = frag_pos + (s_top - s) * curl_dir;
        let dist_top = vt_rect_sdf(p_top, dims);
        if (dist_top <= 0.85) {
            let top_tex = vt_sample_old(p_top / dims);
            let n_cyl = normalize(vec3<f32>(-curl_dir * sin_t, cos_t));
            let l_dir = normalize(vec3<f32>(-0.42, -0.55, 0.72));
            let v_dir = vec3<f32>(0.0, 0.0, 1.0);
            let h_vec = normalize(l_dir + v_dir);
            let ndl = max(dot(n_cyl, l_dir), 0.0);
            let diffuse = 0.62 + 0.38 * ndl;
            let spec = pow(max(dot(n_cyl, h_vec), 0.0), 20.0) * 0.40 * start_fade;
            let edge_rim = smoothstep(2.2 * scale, 0.0, abs(dist_top)) * start_fade;
            let top_col = vt_vellum_backside(top_tex, diffuse, spec, edge_rim);
            let aa_top = smoothstep(0.85, -0.85, dist_top);
            out_col = mix(out_col, top_col, aa_top);
        }
        return out_col;
    }

    // Region C: behind cylinder (d < -R) -> revealed NEW page with soft trailing shadow.
    let new_raw = vt_load_new(frag_pos, dims_i);
    let dist_behind = -d - R;
    let trail_shadow = 1.0
        - 0.26 * exp(-dist_behind / (R * 0.36)) * smoothstep(R * 1.45, 0.0, dist_behind) * start_fade * end_fade;
    return vec4<f32>(new_raw.rgb * trail_shadow, new_raw.a);
}

fn vt_liquid_dissolve(frag_pos: vec2<f32>, dims: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let scale = max(params.feather_px, 1.0);
    let p = clamp(params.progress, 0.0, 1.0);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos, dims_i);
    }

    let q = frag_pos / (120.0 * scale);
    let norm_dist = clamp(length(frag_pos - params.origin_px) / max(params.radius_px, 1.0), 0.0, 1.0);
    let n = vt_warped_fbm(q);
    let field = norm_dist * 0.66 + n * 0.34;
    let threshold = mix(-0.11, 1.11, p);
    let diff = field - threshold;
    let time_env = smoothstep(0.0, 0.07, p) * smoothstep(1.0, 0.88, p);

    // Fast exact path for ~92% of screen pixels away from the active liquid frontier.
    if (abs(diff) > 0.095 || time_env <= 0.0005) {
        if (diff < 0.0) {
            return vt_load_new(frag_pos, dims_i);
        }
        return vt_load_old(frag_pos, dims_i);
    }

    // Surface-tension refraction tightly localized on the organic liquid tendrils.
    let eps = 0.045;
    let nx = vt_warped_fbm(q + vec2<f32>(eps, 0.0)) - vt_warped_fbm(q - vec2<f32>(eps, 0.0));
    let ny = vt_warped_fbm(q + vec2<f32>(0.0, eps)) - vt_warped_fbm(q - vec2<f32>(0.0, eps));
    let grad = vec2<f32>(nx, ny) / (2.0 * eps);

    let border_dist = min(min(frag_pos.x, dims.x - frag_pos.x), min(frag_pos.y, dims.y - frag_pos.y));
    let edge_damp = smoothstep(0.0, 20.0 * scale, border_dist);

    let meniscus = exp(-(diff * diff) / (2.0 * 0.028 * 0.028)) * time_env;
    let disp_px = grad * meniscus * (18.0 * scale) * edge_damp;

    let uv_r = (frag_pos + disp_px * 0.80) / dims;
    let uv_g = (frag_pos + disp_px * 1.00) / dims;
    let uv_b = (frag_pos + disp_px * 1.22) / dims;

    let old_g = vt_sample_old(uv_g);
    let old_col = vec4<f32>(vt_sample_old(uv_r).r, old_g.g, vt_sample_old(uv_b).b, old_g.a);
    let new_g = vt_sample_new(uv_g);
    let new_col = vec4<f32>(vt_sample_new(uv_r).r, new_g.g, vt_sample_new(uv_b).b, new_g.a);

    let show_old = smoothstep(-0.018, 0.018, diff);
    var color = mix(new_col, old_col, show_old);

    // Iridescent glossy liquid meniscus rim + delicate inner shadow.
    let rim = exp(-(diff * diff) / (2.0 * 0.009 * 0.009)) * time_env;
    let rim_tint = mix(vec3<f32>(0.72, 0.90, 1.0), vec3<f32>(0.96, 0.78, 1.0), n);
    let inner_d = diff + 0.018;
    let inner_shade = exp(-(inner_d * inner_d) / (2.0 * 0.014 * 0.014)) * time_env * 0.10;
    let lit_rgb = color.rgb * (1.0 - inner_shade) + rim_tint * (rim * 0.34 * color.a);
    color = vec4<f32>(clamp(lit_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), color.a);
    return color;
}

fn vt_mirror_uv(uv: vec2<f32>) -> vec2<f32> {
    let t = abs(uv - 2.0 * floor(uv * 0.5));
    return 1.0 - abs(t - 1.0);
}

fn vt_rotate2d(v: vec2<f32>, angle: f32) -> vec2<f32> {
    let c = cos(angle);
    let s = sin(angle);
    return vec2<f32>(v.x * c - v.y * s, v.x * s + v.y * c);
}

// Interleaved Gradient Noise (blue-noise-like screen-space dither) -> [0, 1].
// Eliminates both streak banding and grainy white-noise speckles across long radial warps.
fn vt_ign(p: vec2<f32>) -> f32 {
    return fract(52.9829189 * fract(dot(p, vec2<f32>(0.06711056, 0.00583715))));
}

// Continuous prismatic spectral weights along a normalized motion tap `s` in [-0.5, 0.5].
// Produces rich crimson/gold/cyan/cobalt chromatic dispersion across every streak while
// preserving exact neutral white balance on flat regions via per-channel normalization.
fn vt_spectrum_weight(s: f32, chroma: f32) -> vec3<f32> {
    let bell = exp(-(s * s) / (2.0 * 0.34 * 0.34));
    let wr = exp(-((s + 0.34) * (s + 0.34)) / (2.0 * 0.20 * 0.20));
    let wg = exp(-(s * s) / (2.0 * 0.18 * 0.18));
    let wb = exp(-((s - 0.34) * (s - 0.34)) / (2.0 * 0.20 * 0.20));
    return mix(vec3<f32>(bell), vec3<f32>(wr, wg, wb), chroma);
}

fn vt_chromatic_warp_layer(
    use_new: bool,
    p_rel: vec2<f32>,
    r: f32,
    aspect: f32,
    pivot: vec2<f32>,
    base_zoom: f32,
    base_angle: f32,
    swirl: f32,
    barrel: f32,
    blur_radial: f32,
    blur_rot: f32,
    chroma: f32,
    jitter: f32,
) -> vec4<f32> {
    var acc_rgb = vec3<f32>(0.0);
    var w_sum = vec3<f32>(0.0);
    var acc_a = 0.0;
    var a_sum = 0.0;
    let inv_aspect = vec2<f32>(1.0 / aspect, 1.0);

    for (var i = 0; i < 24; i += 1) {
        let s = ((f32(i) + jitter) / 24.0) - 0.5;
        let tap_zoom = max(base_zoom * exp(s * blur_radial), 0.06);
        let tap_angle = base_angle + s * blur_rot + swirl * r;
        let p_rot = vt_rotate2d(p_rel, tap_angle);
        let radial_k = 1.0 / max(1.0 + barrel * r * r, 0.25);
        let p_warped = (p_rot * radial_k) / tap_zoom;
        let sample_uv = vt_mirror_uv(p_warped * inv_aspect + pivot);

        let tex = select(vt_sample_old(sample_uv), vt_sample_new(sample_uv), use_new);
        let spec_w = vt_spectrum_weight(s, chroma);
        let bell = exp(-(s * s) / (2.0 * 0.34 * 0.34));
        acc_rgb += tex.rgb * spec_w;
        w_sum += spec_w;
        acc_a += tex.a * bell;
        a_sum += bell;
    }

    let out_rgb = acc_rgb / max(w_sum, vec3<f32>(0.0001));
    let out_a = acc_a / max(a_sum, 0.0001);
    return vec4<f32>(clamp(out_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), clamp(out_a, 0.0, 1.0));
}

fn vt_chromatic_warp(frag_pos: vec2<f32>, dims: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let p = clamp(params.progress, 0.0, 1.0);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos, dims_i);
    }

    let uv = frag_pos / dims;
    let aspect = dims.x / max(dims.y, 1.0);
    let norm_origin = clamp(params.origin_px / dims, vec2<f32>(0.0), vec2<f32>(1.0));
    let pivot = mix(vec2<f32>(0.5, 0.5), norm_origin, 0.14);
    let p_rel = (uv - pivot) * vec2<f32>(aspect, 1.0);
    let r = length(p_rel);

    // Blue-noise-like screen-space dither yields ultra-smooth streaks with zero grain.
    let jitter = vt_ign(floor(frag_pos) + vec2<f32>(floor(p * 120.0) * 5.588238, 0.0));

    // Whip-shaped velocity envelope: crisp readability near endpoints, explosive radial streaks at apex.
    let vel = pow(sin(p * VT_PI), 1.35);
    let apex = exp(-((p - 0.5) * (p - 0.5)) / (2.0 * 0.08 * 0.08));
    let chroma = smoothstep(0.0, 0.06, vel);

    // Outgoing whip-zoom-in + CCW spin (frames 1 -> 2 -> 3).
    let a = clamp(p / 0.55, 0.0, 1.0);
    let e_out = pow(a, 2.25);
    let zoom_old = exp(1.42 * e_out);
    let ang_old = 0.62 * e_out;
    let swirl_old = 0.34 * e_out;
    let barrel_old = 1.18 * e_out;
    let blur_r_old = vel * (0.20 + 0.92 * r);
    let blur_rot_old = vel * (0.24 + 0.12 * r);

    // Incoming hyper-zoom from mirrored tunnel + continuous CCW whip-spin settle (frames 4 -> 5 -> 6).
    let b = clamp((1.0 - p) / 0.55, 0.0, 1.0);
    let e_in = pow(b, 1.95);
    let zoom_new = exp(-1.66 * e_in);
    let ang_new = -0.68 * e_in;
    let swirl_new = -0.30 * e_in;
    let barrel_new = 0.80 * e_in;
    let blur_r_new = vel * (0.22 + 0.96 * r);
    let blur_rot_new = vel * (0.22 + 0.10 * r);

    // Tight whip-cut right through peak velocity so neither frame washes into flat grey.
    let mix_new = smoothstep(0.44, 0.54, p);
    var color: vec4<f32>;
    if (mix_new <= 0.0005) {
        color = vt_chromatic_warp_layer(
            false, p_rel, r, aspect, pivot,
            zoom_old, ang_old, swirl_old, barrel_old,
            blur_r_old, blur_rot_old, chroma, jitter,
        );
    } else if (mix_new >= 0.9995) {
        color = vt_chromatic_warp_layer(
            true, p_rel, r, aspect, pivot,
            zoom_new, ang_new, swirl_new, barrel_new,
            blur_r_new, blur_rot_new, chroma, jitter,
        );
    } else {
        let old_col = vt_chromatic_warp_layer(
            false, p_rel, r, aspect, pivot,
            zoom_old, ang_old, swirl_old, barrel_old,
            blur_r_old, blur_rot_old, chroma, jitter,
        );
        let new_col = vt_chromatic_warp_layer(
            true, p_rel, r, aspect, pivot,
            zoom_new, ang_new, swirl_new, barrel_new,
            blur_r_new, blur_rot_new, chroma, jitter,
        );
        color = mix(old_col, new_col, mix_new);
    }

    // Vibrant prismatic saturation pop + subtle cinema lens vignette & apex core glint.
    let luma = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    let sat_boost = 1.0 + 0.28 * vel;
    let boosted = mix(vec3<f32>(luma), color.rgb, sat_boost);
    let vignette = 1.0 - 0.12 * vel * clamp(r * r, 0.0, 1.4);
    let core_glow = apex * smoothstep(0.95, 0.0, r) * 0.06 * color.a;
    let final_rgb = clamp(boosted * vignette + vec3<f32>(core_glow), vec3<f32>(0.0), vec3<f32>(1.0));
    return vec4<f32>(final_rgb, color.a);
}

@fragment
fn fs_view_transition(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let dims_i = vec2<i32>(textureDimensions(t_new, 0));
    let dims = max(params.viewport_px, vec2<f32>(dims_i));

    if (params.kind == 3u) {
        return vt_shockwave(frag_pos.xy, dims, dims_i);
    }
    if (params.kind == 4u) {
        return vt_page_curl(frag_pos.xy, dims, dims_i);
    }
    if (params.kind == 5u) {
        return vt_liquid_dissolve(frag_pos.xy, dims, dims_i);
    }
    if (params.kind == 6u) {
        return vt_chromatic_warp(frag_pos.xy, dims, dims_i);
    }

    let old_color = vt_load_old(frag_pos.xy, dims_i);
    let new_color = vt_load_new(frag_pos.xy, dims_i);

    var show_old: f32;
    if (params.kind == 2u) {
        // CrossFade
        show_old = 1.0 - clamp(params.progress, 0.0, 1.0);
    } else {
        let dist = length(frag_pos.xy - params.origin_px);
        let feather = max(params.feather_px, 0.5);
        if (params.kind == 0u) {
            // RadialExpand: new scene revealed inside expanding circle, old outside.
            show_old = smoothstep(params.radius_px - feather, params.radius_px + feather, dist);
        } else {
            // RadialCollapse: old scene collapses inward into origin, new outside.
            show_old = 1.0 - smoothstep(params.radius_px - feather, params.radius_px + feather, dist);
        }
    }

    return mix(new_color, old_color, show_old);
}

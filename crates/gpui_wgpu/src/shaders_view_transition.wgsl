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

// Single-pass Golden-Angle Vogel disk Gaussian blur with IGN rotation (zero box/star artifacts).
fn vt_disk_blur(
    use_new: bool,
    frag_pos: vec2<f32>,
    dims: vec2<f32>,
    dims_i: vec2<i32>,
    radius_px: f32,
) -> vec4<f32> {
    if (radius_px <= 0.35) {
        if (use_new) {
            return vt_load_new(frag_pos, dims_i);
        }
        return vt_load_old(frag_pos, dims_i);
    }

    let inv_dims = 1.0 / dims;
    let base_angle = vt_ign(floor(frag_pos)) * (VT_PI * 2.0);
    let golden_angle = 2.39996323;

    var acc = vec4<f32>(0.0);
    var w_sum = 0.0;
    for (var i = 0; i < 24; i += 1) {
        let frac_r = sqrt((f32(i) + 0.5) / 24.0);
        let theta = base_angle + f32(i) * golden_angle;
        let offset_px = vec2<f32>(cos(theta), sin(theta)) * (frac_r * radius_px);
        let uv = (frag_pos + offset_px) * inv_dims;
        let w = exp(-2.35 * frac_r * frac_r);
        var tex: vec4<f32>;
        if (use_new) {
            tex = vt_sample_new(uv);
        } else {
            tex = vt_sample_old(uv);
        }
        acc += tex * w;
        w_sum += w;
    }
    return acc / max(w_sum, 0.0001);
}

// 0.3s BlurFade:
//   - 0.0s..0.2s (p in 0.0..2/3): old view blurs out smoothly to peak radius.
//   - 0.1s..0.2s (p in 1/3..2/3): opacity cross-fade between blurred old and blurred new.
//   - 0.1s..0.3s (p in 1/3..1.0): new view blurs in smoothly from peak radius to crisp focus.
fn vt_blur_fade(frag_pos: vec2<f32>, dims: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let scale = max(params.feather_px, 1.0);
    let p = clamp(params.progress, 0.0, 1.0);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos, dims_i);
    }

    let max_radius = 26.0 * scale;
    let t_one_third = 1.0 / 3.0;
    let t_two_thirds = 2.0 / 3.0;

    let old_u = smoothstep(0.0, t_two_thirds, p);
    let new_u = 1.0 - smoothstep(t_one_third, 1.0, p);
    let r_old = max_radius * old_u;
    let r_new = max_radius * new_u;

    let mix_new = smoothstep(t_one_third, t_two_thirds, p);
    if (mix_new <= 0.0005) {
        return vt_disk_blur(false, frag_pos, dims, dims_i, r_old);
    }
    if (mix_new >= 0.9995) {
        return vt_disk_blur(true, frag_pos, dims, dims_i, r_new);
    }
    let old_col = vt_disk_blur(false, frag_pos, dims, dims_i, r_old);
    let new_col = vt_disk_blur(true, frag_pos, dims, dims_i, r_new);
    return mix(old_col, new_col, mix_new);
}

// 0.3s asynchronous Glitch transition: staggered multi-scale blocks and horizontal tear strips
// glitch at different times with crisp RGB channel separation and snap into the next view.
fn vt_glitch(frag_pos: vec2<f32>, dims: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let scale = max(params.feather_px, 1.0);
    let p = clamp(params.progress, 0.0, 1.0);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos, dims_i);
    }

    let uv = frag_pos / dims;
    // Quantize digital glitch steps (~18 discrete frames across 0.3s) for crisp electronic snaps.
    let step_t = floor(p * 18.0);
    let endpoint_env = smoothstep(0.0, 0.04, p) * (1.0 - smoothstep(0.94, 1.0, p));

    // Multi-scale irregular regions so different parts of the view flip at different times.
    let macro_cell = floor(uv * vec2<f32>(6.0, 5.0));
    let macro_h = vt_hash21(macro_cell + vec2<f32>(3.1, 7.7));

    // Horizontal mid-blocks with jittered column breaks.
    let mid_row = floor(uv.y * 16.0);
    let row_jitter = vt_hash21(vec2<f32>(mid_row, 11.3));
    let mid_cols = 2.0 + floor(row_jitter * 4.0);
    let mid_cell = vec2<f32>(floor((uv.x + row_jitter * 0.37) * mid_cols), mid_row);
    let mid_h = vt_hash21(mid_cell + vec2<f32>(19.4, 2.9));

    // Fine scanline tear slivers that fire during each region's active glitch window.
    let sliver_row = floor(uv.y * 54.0);
    let sliver_h = vt_hash21(vec2<f32>(sliver_row, step_t * 1.37 + 5.1));
    let block_flicker = vt_hash21(mid_cell + vec2<f32>(step_t * 3.13, step_t * 7.41));

    // Per-region switch time in [0.10, 0.86], combining macro chunks and jagged horizontal bars.
    let cell_t = 0.10 + (macro_h * 0.56 + mid_h * 0.44) * 0.76;
    let delta_t = p - cell_t;
    let local_win = 0.16;
    let glitch_amt = clamp(1.0 - abs(delta_t) / local_win, 0.0, 1.0) * endpoint_env;

    // Fast exact path for regions not currently glitching.
    if (glitch_amt <= 0.001) {
        if (delta_t >= 0.0) {
            return vt_load_new(frag_pos, dims_i);
        }
        return vt_load_old(frag_pos, dims_i);
    }

    // Decide whether this glitching chunk/sliver shows the new or old view (brief digital stutter).
    var show_new = delta_t >= 0.0;
    if (abs(delta_t) < 0.065 && block_flicker > 0.56) {
        show_new = !show_new;
    }
    if (sliver_h > 0.84 && glitch_amt > 0.25) {
        show_new = !show_new;
    }

    // Sharp horizontal tear displacement + micro vertical jitter on active slivers.
    let dir_sign = select(-1.0, 1.0, vt_hash21(mid_cell + vec2<f32>(step_t * 2.1, 9.4)) > 0.5);
    let slice_burst = select(0.35, 1.0, sliver_h > 0.62);
    let shift_px = dir_sign * glitch_amt * (16.0 + 48.0 * slice_burst * block_flicker) * scale;
    let v_jitter_px = (vt_hash21(vec2<f32>(mid_row, step_t * 4.7)) - 0.5) * 6.0 * glitch_amt * scale * select(0.0, 1.0, sliver_h > 0.78);

    // Wide horizontal RGB chromatic aberration split across the tearing chunk.
    let ca_px = glitch_amt * (9.0 + 22.0 * slice_burst) * scale;
    let base_p = frag_pos + vec2<f32>(shift_px, v_jitter_px);
    let uv_r = (base_p - vec2<f32>(ca_px, 0.0)) / dims;
    let uv_g = base_p / dims;
    let uv_b = (base_p + vec2<f32>(ca_px * 1.15, 0.0)) / dims;

    var col: vec4<f32>;
    if (show_new) {
        let g = vt_sample_new(uv_g);
        col = vec4<f32>(vt_sample_new(uv_r).r, g.g, vt_sample_new(uv_b).b, g.a);
    } else {
        let g = vt_sample_old(uv_g);
        col = vec4<f32>(vt_sample_old(uv_r).r, g.g, vt_sample_old(uv_b).b, g.a);
    }

    // Subtle CRT scanline modulation + rare thin neon cyan/magenta bitstream flash inside active bars.
    let scanline = 1.0 - 0.10 * glitch_amt * (0.5 + 0.5 * sin(frag_pos.y * (VT_PI / scale)));
    var rgb = col.rgb * scanline;
    if (sliver_h > 0.91 && glitch_amt > 0.35) {
        let tint = select(vec3<f32>(0.08, 0.95, 1.0), vec3<f32>(1.0, 0.14, 0.68), block_flicker > 0.5);
        rgb = mix(rgb, tint * col.a, 0.32 * glitch_amt);
    }
    return vec4<f32>(clamp(rgb, vec3<f32>(0.0), vec3<f32>(1.0)), col.a);
}

// Smooth 2-octave value noise (silky organic curves without high-frequency crinkles).
fn vt_smooth_noise2(p: vec2<f32>) -> f32 {
    let a = vt_noise2d(p);
    let b = vt_noise2d(vec2<f32>(p.x * 1.72 - p.y * 1.18, p.x * 1.18 + p.y * 1.72) + vec2<f32>(7.4, 13.1));
    return a * 0.68 + b * 0.32;
}

// Organic double-blob radial reveal: two independent smooth random blobs (outer & inner)
// expanding from origin, with silky spectral chromatic aberration and prismatic glass color
// filling the varying-width ribbon between the two blob contours.
fn vt_prismatic_blob(frag_pos: vec2<f32>, dims: vec2<f32>, dims_i: vec2<i32>) -> vec4<f32> {
    let scale = max(params.feather_px, 1.0);
    let p = clamp(params.progress, 0.0, 1.0);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos, dims_i);
    }

    let delta = frag_pos - params.origin_px;
    let dist = length(delta);
    let dir = delta / max(dist, 0.001);
    let theta = atan2(delta.y, delta.x);

    // Origin-derived seed so each click position spawns a fresh pair of organic blob shapes.
    let seed = fract(params.origin_px * vec2<f32>(0.0137, 0.0193) + vec2<f32>(0.37, 0.71));

    let band = clamp(min(dims.x, dims.y) * 0.135, 64.0 * scale, 140.0 * scale);
    let max_r = max(params.radius_px, 1.0);
    let r_base_out = mix(-band * 0.35, max_r + band * 2.15, p);
    let r_base_in = mix(-band * 1.45, max_r + band * 1.15, p);

    // Fast coarse radial reject for pixels far outside the double-blob envelope.
    let max_amp = band * 0.96;
    if (dist > r_base_out + max_amp + 12.0 * scale) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (dist < r_base_in - max_amp - 12.0 * scale) {
        return vt_load_new(frag_pos, dims_i);
    }

    // Smooth organic OUTER blob silhouette (seamless around 2*PI via unit `dir`, zero crinkles).
    let n_out = vt_smooth_noise2(
        dir * 1.15 + vec2<f32>(seed.x * 7.3 + 1.4, seed.y * 5.9 + 2.8) + vec2<f32>(p * 0.55, -p * 0.38),
    );
    let lobes_out = 0.42 * sin(theta * 2.0 + seed.x * 6.28318 + 0.5 + p * 0.65)
        + 0.33 * sin(theta * 3.0 - seed.y * 6.28318 + 2.1 - p * 0.45)
        + 0.16 * sin(theta * 4.0 + seed.x * 4.12 - 0.9 + p * 0.35)
        + 0.09 * sin(theta * 5.0 - seed.y * 3.74 + 1.3);
    let shape_out = clamp((n_out - 0.5) * 1.05 + lobes_out * 0.78, -1.0, 1.0);

    // Independent smooth organic INNER blob silhouette (different noise domain & lobe harmonics).
    let n_in = vt_smooth_noise2(
        dir * 1.25 + vec2<f32>(seed.y * 9.1 + 11.7, seed.x * 6.4 + 8.3) + vec2<f32>(-p * 0.42, p * 0.60),
    );
    let lobes_in = 0.40 * cos(theta * 2.0 - seed.y * 6.28318 + 1.9 - p * 0.55)
        + 0.34 * sin(theta * 3.0 + seed.x * 6.28318 + 0.4 + p * 0.50)
        + 0.18 * cos(theta * 4.0 + seed.y * 5.14 - 1.4 - p * 0.30)
        + 0.08 * cos(theta * 5.0 + seed.x * 2.91 + 0.8);
    let shape_in = clamp((n_in - 0.5) * 1.05 + lobes_in * 0.78, -1.0, 1.0);

    // Cap lobe amplitude proportionally to base radius near origin so neither blob ever pinches/folds.
    let amp_out = min(max_amp, max(r_base_out, 0.0) * 0.44);
    let amp_in = min(max_amp, max(r_base_in, 0.0) * 0.44);

    let r_outer = r_base_out + amp_out * shape_out;
    let r_inner = min(r_base_in + amp_in * shape_in, r_outer - band * 0.24);

    let aa = max(2.2 * scale, 1.5);
    if (dist > r_outer + aa * 1.5) {
        return vt_load_old(frag_pos, dims_i);
    }
    if (dist < r_inner - aa * 1.5) {
        return vt_load_new(frag_pos, dims_i);
    }

    // Inside the prismatic chromatic-aberration zone between the inner blob and the outer blob!
    let band_w = max(r_outer - r_inner, 1.0);
    let u = clamp((dist - r_inner) / band_w, 0.0, 1.0);
    let ribbon = sin(u * VT_PI);
    let perp = vec2<f32>(-dir.y, dir.x);

    // Gently damp displacement near viewport edges so screen borders never smear.
    let border_dist = min(min(frag_pos.x, dims.x - frag_pos.x), min(frag_pos.y, dims.y - frag_pos.y));
    let edge_damp = smoothstep(0.0, 24.0 * scale, border_dist);

    // 14-tap IGN-dithered spectral chromatic aberration refracting cleanly across the double-blob band.
    let swirl_dir = normalize(dir + perp * ((shape_out - shape_in) * 0.58));
    let max_disp = (32.0 * scale) * (0.26 + 0.74 * ribbon) * edge_damp;
    let blend_old = smoothstep(0.10, 0.90, u);
    let inv_dims = 1.0 / dims;
    let jitter = vt_ign(floor(frag_pos)) - 0.5;
    let tang_wave = sin(u * VT_PI * 2.0 + (n_out + n_in) * 3.0) * 8.0 * scale * ribbon * edge_damp;

    var acc_rgb = vec3<f32>(0.0);
    var w_sum = vec3<f32>(0.0);
    var acc_a = 0.0;
    var a_sum = 0.0;
    for (var i = 0; i < 14; i += 1) {
        let s = clamp(((f32(i) + 0.5 + jitter) / 14.0) - 0.5, -0.5, 0.5);
        let offset_px = swirl_dir * (s * 2.0 * max_disp) + perp * tang_wave;
        let sample_uv = (frag_pos + offset_px) * inv_dims;
        let tex = mix(vt_sample_new(sample_uv), vt_sample_old(sample_uv), blend_old);
        let spec_w = vt_spectrum_weight(s, 1.0);
        let bell = exp(-(s * s) / (2.0 * 0.34 * 0.34));
        acc_rgb += tex.rgb * spec_w;
        w_sum += spec_w;
        acc_a += tex.a * bell;
        a_sum += bell;
    }
    let refracted_rgb = acc_rgb / max(w_sum, vec3<f32>(0.0001));
    let refracted_a = acc_a / max(a_sum, 0.0001);

    // Flowing prismatic / iridescent rainbow sheen filling the zone between outer and inner blobs.
    // Using `theta / (2*PI)` wraps by exactly 1.0 full cycle across [-PI, +PI] (zero seam).
    let prism_phase = u * 1.18 + (n_out * 0.55 + n_in * 0.45) * 0.85 + theta / (VT_PI * 2.0) + p * 0.55;
    let prism_rgb = 0.52 + 0.48 * cos((VT_PI * 2.0) * (prism_phase + vec3<f32>(0.0, 0.33, 0.67)));

    // Crisp luminous glass rim highlights tracing BOTH the outer blob and inner blob contours.
    let rim_sigma = 4.2 * scale;
    let outer_rim = exp(-((dist - r_outer) * (dist - r_outer)) / (2.0 * rim_sigma * rim_sigma));
    let inner_rim = exp(-((dist - r_inner) * (dist - r_inner)) / (2.0 * rim_sigma * rim_sigma));
    let rim_glow = clamp(outer_rim + inner_rim, 0.0, 1.0);

    // Boost chromatic saturation inside the band and infuse translucent prismatic iridescent glass.
    let luma = dot(refracted_rgb, vec3<f32>(0.299, 0.587, 0.114));
    let vivid_rgb = clamp(mix(vec3<f32>(luma), refracted_rgb, 1.44), vec3<f32>(0.0), vec3<f32>(1.0));
    let band_rgb = mix(vivid_rgb, prism_rgb * refracted_a, 0.36 * ribbon + 0.28 * rim_glow)
        + prism_rgb * (0.20 * rim_glow * refracted_a);
    let band_col = vec4<f32>(clamp(band_rgb, vec3<f32>(0.0), vec3<f32>(1.0)), refracted_a);

    // Smoothly AA into crisp `t_new` inside the inner blob and crisp `t_old` outside the outer blob.
    let enter_band = smoothstep(r_inner - aa, r_inner + aa, dist);
    let exit_band = smoothstep(r_outer - aa, r_outer + aa, dist);
    var result = mix(vt_load_new(frag_pos, dims_i), band_col, enter_band);
    result = mix(result, vt_load_old(frag_pos, dims_i), exit_band);
    return result;
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
    if (params.kind == 7u) {
        return vt_blur_fade(frag_pos.xy, dims, dims_i);
    }
    if (params.kind == 8u) {
        return vt_glitch(frag_pos.xy, dims, dims_i);
    }
    if (params.kind == 9u) {
        return vt_prismatic_blob(frag_pos.xy, dims, dims_i);
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

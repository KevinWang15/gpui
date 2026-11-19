struct ViewTransitionParams {
    origin_px: vec2<f32>,
    radius_px: f32,
    feather_px: f32,
    progress: f32,
    kind: u32,
    _pad0: u32,
    _pad1: u32,
}

@group(0) @binding(0) var<uniform> params: ViewTransitionParams;
@group(0) @binding(1) var t_old: texture_2d<f32>;
@group(0) @binding(2) var t_new: texture_2d<f32>;

@vertex
fn vs_view_transition(@builtin(vertex_index) vertex_index: u32) -> @builtin(position) vec4<f32> {
    var positions = array<vec2<f32>, 3>(
        vec2<f32>(-1.0, -1.0),
        vec2<f32>(3.0, -1.0),
        vec2<f32>(-1.0, 3.0),
    );
    return vec4<f32>(positions[vertex_index], 0.0, 1.0);
}

@fragment
fn fs_view_transition(@builtin(position) frag_pos: vec4<f32>) -> @location(0) vec4<f32> {
    let dims = vec2<i32>(textureDimensions(t_new, 0));
    let coord = clamp(vec2<i32>(frag_pos.xy), vec2<i32>(0), dims - vec2<i32>(1));
    let old_color = textureLoad(t_old, coord, 0);
    let new_color = textureLoad(t_new, coord, 0);

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

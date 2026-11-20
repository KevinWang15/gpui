#include "alpha_correction.hlsl"

cbuffer GlobalParams: register(b0) {
    float4 gamma_ratios;
    float2 global_viewport_size;
    float grayscale_enhanced_contrast;
    float subpixel_enhanced_contrast;
    uint is_bgr;
    float depth_scale;
    uint2 global_pad;
};

cbuffer BatchParams: register(b1) {
    uint batch_start_index;
    uint3 batch_pad;
};

Texture2D<float4> t_sprite: register(t0);
SamplerState s_sprite: register(s0);

struct SubpixelSpriteFragmentOutput {
    float4 foreground : SV_Target0;
    float4 alpha : SV_Target1;
};

struct Bounds {
    float2 origin;
    float2 size;
};

struct Corners {
    float top_left;
    float top_right;
    float bottom_right;
    float bottom_left;
};

struct Edges {
    float top;
    float right;
    float bottom;
    float left;
};

struct Hsla {
    float h;
    float s;
    float l;
    float a;
};

struct LinearColorStop {
    Hsla color;
    float percentage;
};

struct Background {
    // 0u is Solid
    // 1u is LinearGradient
    // 2u is PatternSlash
    uint tag;
    // 0u is sRGB color
    // 1u is Oklab color
    uint color_space;
    Hsla solid;
    float gradient_angle_or_pattern_height;
    LinearColorStop colors[2];
    uint pad;
};

struct PreparedBackground {
    nointerpolation float4 color0: COLOR1;
    nointerpolation float4 color1: COLOR2;
    nointerpolation uint tag: TEXCOORD2;
};

struct AtlasTextureId {
    uint index;
    uint kind;
};

struct AtlasBounds {
    int2 origin;
    int2 size;
};

struct AtlasTile {
    AtlasTextureId texture_id;
    uint tile_id;
    uint padding;
    AtlasBounds bounds;
};

struct TransformationMatrix {
    float2x2 rotation_scale;
    float2 translation;
};

static const float M_PI_F = 3.141592653f;
static const float3 GRAYSCALE_FACTORS = float3(0.2126f, 0.7152f, 0.0722f);

static const uint SIMPLE_PRIMITIVE_FLAG = 1u << 31;

float4 to_device_position_impl(float2 position) {
    float2 device_position = position / global_viewport_size * float2(2.0, -2.0) + float2(-1.0, 1.0);
    return float4(device_position, 0., 1.);
}

float4 to_device_position(float2 unit_vertex, Bounds bounds) {
    float2 position = unit_vertex * bounds.size + bounds.origin;
    return to_device_position_impl(position);
}

float quad_depth(uint quad_id) {
    return saturate(float(quad_id + 1u) * depth_scale);
}

static const float4 CULLED_POSITION = float4(0.0, 0.0, 0.0, 1.0);

float4 distance_from_clip_rect_impl(float2 position, Bounds clip_bounds) {
    float2 tl = position - clip_bounds.origin;
    float2 br = clip_bounds.origin + clip_bounds.size - position;
    return float4(tl.x, br.x, tl.y, br.y);
}

float4 distance_from_clip_rect_transformed(float2 unit_vertex, Bounds bounds, Bounds clip_bounds, TransformationMatrix transformation) {
    float2 position = unit_vertex * bounds.size + bounds.origin;
    float2 transformed = mul(position, transformation.rotation_scale) + transformation.translation;
    return distance_from_clip_rect_impl(transformed, clip_bounds);
}

struct ClippedVertex {
    float2 position;
    // The corner as a fraction of the original bounds.
    float2 unit_vertex;
};

ClippedVertex clip_to_mask(float2 unit_vertex, Bounds bounds, Bounds mask) {
    float2 origin = max(bounds.origin, mask.origin);
    float2 corner = min(bounds.origin + bounds.size, mask.origin + mask.size);
    float2 size = max(corner - origin, float2(0.0, 0.0));

    ClippedVertex result;
    result.position = origin + unit_vertex * size;
    result.unit_vertex = (result.position - bounds.origin) / bounds.size;
    return result;
}

// Convert linear RGB to sRGB
float3 linear_to_srgb(float3 color) {
    return pow(color, float3(1.0 / 2.2, 1.0 / 2.2, 1.0 / 2.2));
}

// Convert sRGB to linear RGB
float3 srgb_to_linear(float3 color) {
    return pow(color, float3(2.2, 2.2, 2.2));
}

/// Hsla to sRGB conversion.
float4 hsla_to_rgba(Hsla hsla) {
    float h = hsla.h * 6.0; // Now, it's an angle but scaled in [0, 6) range
    float s = hsla.s;
    float l = hsla.l;
    float a = hsla.a;

    float c = (1.0 - abs(2.0 * l - 1.0)) * s;
    float x = c * (1.0 - abs(fmod(h, 2.0) - 1.0));
    float m = l - c / 2.0;

    float r = 0.0;
    float g = 0.0;
    float b = 0.0;

    if (h >= 0.0 && h < 1.0) {
        r = c;
        g = x;
        b = 0.0;
    } else if (h >= 1.0 && h < 2.0) {
        r = x;
        g = c;
        b = 0.0;
    } else if (h >= 2.0 && h < 3.0) {
        r = 0.0;
        g = c;
        b = x;
    } else if (h >= 3.0 && h < 4.0) {
        r = 0.0;
        g = x;
        b = c;
    } else if (h >= 4.0 && h < 5.0) {
        r = x;
        g = 0.0;
        b = c;
    } else {
        r = c;
        g = 0.0;
        b = x;
    }

    float4 rgba;
    rgba.x = (r + m);
    rgba.y = (g + m);
    rgba.z = (b + m);
    rgba.w = a;
    return rgba;
}

// Converts a sRGB color to the Oklab color space.
// Reference: https://bottosson.github.io/posts/oklab/#converting-from-linear-srgb-to-oklab
float4 srgb_to_oklab(float4 color) {
    // Convert non-linear sRGB to linear sRGB
    color = float4(srgb_to_linear(color.rgb), color.a);

    float l = 0.4122214708 * color.r + 0.5363325363 * color.g + 0.0514459929 * color.b;
    float m = 0.2119034982 * color.r + 0.6806995451 * color.g + 0.1073969566 * color.b;
    float s = 0.0883024619 * color.r + 0.2817188376 * color.g + 0.6299787005 * color.b;

    float l_ = pow(l, 1.0/3.0);
    float m_ = pow(m, 1.0/3.0);
    float s_ = pow(s, 1.0/3.0);

    return float4(
        0.2104542553 * l_ + 0.7936177850 * m_ - 0.0040720468 * s_,
        1.9779984951 * l_ - 2.4285922050 * m_ + 0.4505937099 * s_,
        0.0259040371 * l_ + 0.7827717662 * m_ - 0.8086757660 * s_,
        color.a
    );
}

// Converts an Oklab color to the sRGB color space.
float4 oklab_to_srgb(float4 color) {
    float l_ = color.r + 0.3963377774 * color.g + 0.2158037573 * color.b;
    float m_ = color.r - 0.1055613458 * color.g - 0.0638541728 * color.b;
    float s_ = color.r - 0.0894841775 * color.g - 1.2914855480 * color.b;

    float l = l_ * l_ * l_;
    float m = m_ * m_ * m_;
    float s = s_ * s_ * s_;

    float3 linear_rgb = float3(
        4.0767416621 * l - 3.3077115913 * m + 0.2309699292 * s,
        -1.2684380046 * l + 2.6097574011 * m - 0.3413193965 * s,
        -0.0041960863 * l - 0.7034186147 * m + 1.7076147010 * s
    );

    // Convert linear sRGB to non-linear sRGB
    return float4(linear_to_srgb(linear_rgb), color.a);
}

float4 over(float4 below, float4 above) {
    float alpha = above.a + below.a * (1.0 - above.a);
    float3 premultiplied = above.rgb * above.a + below.rgb * below.a * (1.0 - above.a);
    return float4(premultiplied / (alpha == 0.0 ? 1.0 : alpha), alpha);
}

float2 to_tile_position(float2 unit_vertex, AtlasTile tile) {
    float2 atlas_size;
    t_sprite.GetDimensions(atlas_size.x, atlas_size.y);
    return (float2(tile.bounds.origin) + unit_vertex * float2(tile.bounds.size)) / atlas_size;
}

// Selects corner radius based on quadrant.
float pick_corner_radius(float2 center_to_point, Corners corner_radii) {
    if (center_to_point.x < 0.) {
        if (center_to_point.y < 0.) {
            return corner_radii.top_left;
        } else {
            return corner_radii.bottom_left;
        }
    } else {
        if (center_to_point.y < 0.) {
            return corner_radii.top_right;
        } else {
            return corner_radii.bottom_right;
        }
    }
}

float4 to_device_position_transformed(float2 unit_vertex, Bounds bounds,
                                      TransformationMatrix transformation) {
    float2 position = unit_vertex * bounds.size + bounds.origin;
    float2 transformed = mul(position, transformation.rotation_scale) + transformation.translation;
    float2 device_position = transformed / global_viewport_size * float2(2.0, -2.0) + float2(-1.0, 1.0);
    return float4(device_position, 0.0, 1.0);
}

// Implementation of quad signed distance field
float quad_sdf_impl(float2 corner_center_to_point, float corner_radius) {
    if (corner_radius == 0.0) {
        // Fast path for unrounded corners
        return max(corner_center_to_point.x, corner_center_to_point.y);
    } else {
        // Signed distance of the point from a quad that is inset by corner_radius
        // It is negative inside this quad, and positive outside
        float signed_distance_to_inset_quad =
            // 0 inside the inset quad, and positive outside
            length(max(float2(0.0, 0.0), corner_center_to_point)) +
            // 0 outside the inset quad, and negative inside
            min(0.0, max(corner_center_to_point.x, corner_center_to_point.y));

        return signed_distance_to_inset_quad - corner_radius;
    }
}

float quad_sdf(float2 pt, Bounds bounds, Corners corner_radii) {
    float2 half_size = bounds.size / 2.;
    float2 center = bounds.origin + half_size;
    float2 center_to_point = pt - center;
    float corner_radius = pick_corner_radius(center_to_point, corner_radii);
    float2 corner_to_point = abs(center_to_point) - half_size;
    float2 corner_center_to_point = corner_to_point + corner_radius;
    return quad_sdf_impl(corner_center_to_point, corner_radius);
}

PreparedBackground prepare_background(Background background) {
    PreparedBackground output;
    output.tag = background.tag;
    output.color1 = float4(0.0, 0.0, 0.0, 0.0);

    if (background.tag == 1) {
        output.color0 = hsla_to_rgba(background.colors[0].color);
        output.color1 = hsla_to_rgba(background.colors[1].color);
        // Interpolating in Oklab means converting the endpoints once rather
        // than at every stop evaluation.
        if (background.color_space == 1) {
            output.color0 = srgb_to_oklab(output.color0);
            output.color1 = srgb_to_oklab(output.color1);
        }
    } else {
        output.color0 = hsla_to_rgba(background.solid);
    }

    return output;
}

// Ordered (Bayer 4x4) dither threshold, remapped to [-1, +1].
float dither_offset(float2 position) {
    uint2 cell = uint2(position) & 3u;
    uint z = cell.x ^ cell.y;
    uint threshold = ((z & 1u) << 3)
        | ((cell.y & 1u) << 2)
        | (z & 2u)
        | ((cell.y & 2u) >> 1);
    return (float(threshold) + 0.5) * (1.0 / 8.0) - 1.0;
}

float4 background_color(PreparedBackground prepared, Background background, Bounds bounds,
                        float2 position) {
    float4 color = prepared.color0;
    if (prepared.tag == 0u) {
        return color;
    }

    switch (prepared.tag) {
        case 1: {
            // -90 degrees to match the CSS gradient angle.
            float radians = (fmod(background.gradient_angle_or_pattern_height, 360.0) - 90.0)
                * (M_PI_F / 180.0);
            float2 direction = float2(cos(radians), sin(radians));

            // Expand the short side to be the same as the long side
            if (bounds.size.x > bounds.size.y) {
                direction.y *= bounds.size.y / bounds.size.x;
            } else {
                direction.x *= bounds.size.x / bounds.size.y;
            }

            float extent = abs(direction.x) > abs(direction.y)
                ? bounds.size.x
                : bounds.size.y;
            float direction_length = length(direction);
            float span = background.colors[1].percentage - background.colors[0].percentage;
            // The half-extent term is exactly half of `extent`, so the
            // position-independent part reduces to this.
            float offset = 0.5 - background.colors[0].percentage;

            // t = dot(position - center, gradient.xy) + gradient.z
            float3 gradient = float3(0.0, 0.0, 0.0);
            if (direction_length != 0.0 && extent != 0.0) {
                if (span == 0.0) {
                    const float STEP = 1e30;
                    gradient = float3(direction / (direction_length * extent) * STEP, offset * STEP);
                } else {
                    gradient = float3(direction / (direction_length * extent * span), offset / span);
                }
            }
            float2 center = bounds.origin + bounds.size * 0.5;
            float t = saturate(dot(position - center, gradient.xy) + gradient.z);
            color = lerp(prepared.color0, prepared.color1, t);
            if (background.color_space == 1) {
                color = oklab_to_srgb(color);
            }

            // Dither to reduce banding in gradients (especially dark/alpha).
            float dither = dither_offset(position);
            color.rgb += dither * 1.0 / 255.0;
            color.a   += dither * 1.5 / 255.0;
            break;
        }
        case 2: {
            float pattern_height_encoded = background.gradient_angle_or_pattern_height;
            float pattern_width = (pattern_height_encoded / 65535.0) / 255.0;
            float pattern_interval = fmod(pattern_height_encoded, 65535.0) / 255.0;
            float pattern_height = pattern_width + pattern_interval;
            // Only the rotated x coordinate is used, and a row-vector multiply
            // by the rotation reduces it to dot(v, float2(cos a, sin a)).
            float stripe_angle = M_PI_F / 4.0;
            float2 stripe_direction = float2(cos(stripe_angle), sin(stripe_angle));
            float pattern_period = pattern_height * sin(stripe_angle);
            float threshold = pattern_period * (pattern_width / pattern_height) / 2.0;
            float pattern = fmod(dot(position - bounds.origin, stripe_direction), pattern_period);
            float distance = min(pattern, pattern_period - pattern) - threshold;
            color.a *= saturate(0.5 - distance);
            break;
        }
        case 3: {
            // checkerboard
            float cell_size = background.gradient_angle_or_pattern_height;
            float2 relative_position = position - bounds.origin;
            float x_index = floor(relative_position.x / cell_size);
            float y_index = floor(relative_position.y / cell_size);
            color.a *= saturate((x_index + y_index) % 2.0);
            break;
        }
    }

    return color;
}

// Returns the dash velocity of a corner given the dash velocity of the two
// sides, by returning the slower velocity (larger dashes).
//
// Since 0 is used for dash velocity when the border width is 0 (instead of
// +inf), this returns the other dash velocity in that case.
//
// An alternative to this might be to appropriately interpolate the dash
// velocity around the corner, but that seems overcomplicated.
float corner_dash_velocity(float dv1, float dv2) {
    if (dv1 == 0.0) {
        return dv2;
    } else if (dv2 == 0.0) {
        return dv1;
    } else {
        return min(dv1, dv2);
    }
}

// Returns alpha used to render antialiased dashes.
// `t` is within the dash when `fmod(t, period) < length`.
float dash_alpha(
    float t, float period, float length, float dash_velocity,
    float antialias_threshold
) {
    float half_period = period / 2.0;
    float half_length = length / 2.0;
    // Value in [-half_period, half_period]
    // The dash is in [-half_length, half_length]
    float centered = fmod(t + half_period - half_length, period) - half_period;
    // Signed distance for the dash, negative values are inside the dash
    float signed_distance = abs(centered) - half_length;
    // Antialiased alpha based on the signed distance
    return saturate(antialias_threshold - signed_distance / dash_velocity);
}

// This approximates distance to the nearest point to a quarter ellipse in a way
// that is sufficient for anti-aliasing when the ellipse is not very eccentric.
// The components of `point` are expected to be positive.
//
// Negative on the outside and positive on the inside.
float quarter_ellipse_sdf(float2 pt, float2 radii) {
    // Scale the space to treat the ellipse like a unit circle
    float2 circle_vec = pt / radii;
    float unit_circle_sdf = length(circle_vec) - 1.0;
    // Approximate up-scaling of the length by using the average of the radii.
    //
    // TODO: A better solution would be to use the gradient of the implicit
    // function for an ellipse to approximate a scaling factor.
    return unit_circle_sdf * (radii.x + radii.y) * -0.5;
}

/*
**
**              Quads
**
*/

struct Quad {
    uint order;
    uint border_style;
    Bounds bounds;
    Bounds content_mask;
    Background background;
    Hsla border_color;
    Corners corner_radii;
    Edges border_widths;
};

bool has_rounded_corners(Corners corner_radii) {
    return corner_radii.top_left != 0.0
        || corner_radii.top_right != 0.0
        || corner_radii.bottom_right != 0.0
        || corner_radii.bottom_left != 0.0;
}

bool has_border(Edges border_widths) {
    return border_widths.top != 0.0
        || border_widths.right != 0.0
        || border_widths.bottom != 0.0
        || border_widths.left != 0.0;
}

bool is_shaped(Quad quad) {
    return has_rounded_corners(quad.corner_radii) || has_border(quad.border_widths);
}

struct QuadFragmentInput {
    nointerpolation uint quad_id: TEXCOORD0;
    float4 position: SV_Position;
    nointerpolation float4 border_color: COLOR0;
    PreparedBackground background;
};

// Opaque rectangles are drawn by the opaque pass instead of the quad pass.
bool is_opaque_rectangle(Quad quad) {
    return quad.background.tag == 0u
        && quad.background.solid.a >= 1.0
        && !has_rounded_corners(quad.corner_radii)
        && !has_border(quad.border_widths);
}

StructuredBuffer<Quad> quads: register(t1);

QuadFragmentInput quad_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    uint quad_id = batch_start_index + instance_id;
    Quad quad = quads[quad_id];

    if (is_opaque_rectangle(quad)) {
        QuadFragmentInput culled = (QuadFragmentInput)0;
        culled.position = CULLED_POSITION;
        return culled;
    }

    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    ClippedVertex vertex = clip_to_mask(unit_vertex, quad.bounds, quad.content_mask);
    float4 device_position = to_device_position_impl(vertex.position);

    float4 border_color = hsla_to_rgba(quad.border_color);

    QuadFragmentInput output;
    output.position = device_position;
    output.position.z = quad_depth(quad_id);
    output.border_color = border_color;
    bool simple = !is_shaped(quad) && quad.background.tag == 0u;
    output.quad_id = quad_id | (simple ? SIMPLE_PRIMITIVE_FLAG : 0u);
    output.background = prepare_background(quad.background);
    return output;
}

float4 quad_fragment(QuadFragmentInput input): SV_Target {
    if ((input.quad_id & SIMPLE_PRIMITIVE_FLAG) != 0u) {
        return input.background.color0;
    }

    Quad quad = quads[input.quad_id];
    float4 background =
        background_color(input.background, quad.background, quad.bounds, input.position.xy);
    bool unrounded = !has_rounded_corners(quad.corner_radii);

    float2 size = quad.bounds.size;
    float2 half_size = size / 2.;
    float2 the_point = input.position.xy - quad.bounds.origin;
    float2 center_to_point = the_point - half_size;

    // Signed distance field threshold for inclusion of pixels. 0.5 is the
    // minimum distance between the center of the pixel and the edge.
    const float antialias_threshold = 0.5;

    // Radius of the nearest corner
    float corner_radius = pick_corner_radius(center_to_point, quad.corner_radii);

    float2 border = float2(
        center_to_point.x < 0.0 ? quad.border_widths.left : quad.border_widths.right,
        center_to_point.y < 0.0 ? quad.border_widths.top : quad.border_widths.bottom
    );

    // 0-width borders are reduced so that `inner_sdf >= antialias_threshold`.
    // The purpose of this is to not draw antialiasing pixels in this case.
    float2 reduced_border = float2(
        border.x == 0.0 ? -antialias_threshold : border.x,
        border.y == 0.0 ? -antialias_threshold : border.y
    );

    // Vector from the corner of the quad bounds to the point, after mirroring
    // the point into the bottom right quadrant. Both components are <= 0.
    float2 corner_to_point = abs(center_to_point) - half_size;

    // Vector from the point to the center of the rounded corner's circle, also
    // mirrored into bottom right quadrant.
    float2 corner_center_to_point = corner_to_point + corner_radius;

    // Whether the nearest point on the border is rounded
    bool is_near_rounded_corner =
        corner_center_to_point.x >= 0.0 &&
        corner_center_to_point.y >= 0.0;

    // Vector from straight border inner corner to point.
    //
    // 0-width borders are turned into width -1 so that inner_sdf is > 1.0 near
    // the border. Without this, antialiasing pixels would be drawn.
    float2 straight_border_inner_corner_to_point = corner_to_point + reduced_border;

    // Whether the point is beyond the inner edge of the straight border
    bool is_beyond_inner_straight_border =
        straight_border_inner_corner_to_point.x > 0.0 ||
        straight_border_inner_corner_to_point.y > 0.0;

    // Whether the point is far enough inside the quad, such that the pixels are
    // not affected by the straight border.
    bool is_within_inner_straight_border =
        straight_border_inner_corner_to_point.x < -antialias_threshold &&
        straight_border_inner_corner_to_point.y < -antialias_threshold;

    // Fast path for points that must be part of the background
    if (is_within_inner_straight_border && !is_near_rounded_corner) {
        return background;
    }

    // Signed distance of the point to the outside edge of the quad's border
    float outer_sdf = quad_sdf_impl(corner_center_to_point, corner_radius);

    // Approximate signed distance of the point to the inside edge of the quad's
    // border. It is negative outside this edge (within the border), and
    // positive inside.
    //
    // This is not always an accurate signed distance:
    // * The rounded portions with varying border width use an approximation of
    //   nearest-point-on-ellipse.
    // * When it is quickly known to be outside the edge, -1.0 is used.
    float inner_sdf = 0.0;
    if (corner_center_to_point.x <= 0.0 || corner_center_to_point.y <= 0.0) {
        // Fast paths for straight borders
        inner_sdf = -max(straight_border_inner_corner_to_point.x,
                        straight_border_inner_corner_to_point.y);
    } else if (is_beyond_inner_straight_border) {
        // Fast path for points that must be outside the inner edge
        inner_sdf = -1.0;
    } else if (reduced_border.x == reduced_border.y) {
        // Fast path for circular inner edge.
        inner_sdf = -(outer_sdf + reduced_border.x);
    } else {
        float2 ellipse_radii = max(float2(0.0, 0.0), float2(corner_radius, corner_radius) - reduced_border);
        inner_sdf = quarter_ellipse_sdf(corner_center_to_point, ellipse_radii);
    }

    // Negative when inside the border
    float border_sdf = max(inner_sdf, outer_sdf);

    float4 color = background;
    if (border_sdf < antialias_threshold) {
        float4 border_color = input.border_color;
        // Dashed border logic when border_style == 1
        if (quad.border_style == 1) {
            // Position along the perimeter in "dash space", where each dash
            // period has length 1
            float t = 0.0;

            // Total number of dash periods, so that the dash spacing can be
            // adjusted to evenly divide it
            float max_t = 0.0;

            // Border width is proportional to dash size. This is the behavior
            // used by browsers, but also avoids dashes from different segments
            // overlapping when dash size is smaller than the border width.
            //
            // Dash pattern: (2 * border width) dash, (1 * border width) gap
            const float dash_length_per_width = 2.0;
            const float dash_gap_per_width = 1.0;
            const float dash_period_per_width = dash_length_per_width + dash_gap_per_width;

            // Since the dash size is determined by border width, the density of
            // dashes varies. Multiplying a pixel distance by this returns a
            // position in dash space - it has units (dash period / pixels). So
            // a dash velocity of (1 / 10) is 1 dash every 10 pixels.
            float dash_velocity = 0.0;

            // Dividing this by the border width gives the dash velocity
            const float dv_numerator = 1.0 / dash_period_per_width;

            if (unrounded) {
                // When corners aren't rounded, the dashes are separately laid
                // out on each straight line, rather than around the whole
                // perimeter. This way each line starts and ends with a dash.
                bool is_horizontal = corner_center_to_point.x < corner_center_to_point.y;
                // Choosing the right border width for dashed borders.
                // TODO: A better solution exists taking a look at the whole file.
                // this does not fix single dashed borders at the corners
                float2 dashed_border = float2(
                    max(quad.border_widths.bottom, quad.border_widths.top),
                    max(quad.border_widths.right, quad.border_widths.left)
                );
                float border_width = is_horizontal ? dashed_border.x : dashed_border.y;
                dash_velocity = dv_numerator / border_width;
                t = is_horizontal ? the_point.x : the_point.y;
                t *= dash_velocity;
                max_t = is_horizontal ? size.x : size.y;
                max_t *= dash_velocity;
            } else {
                // When corners are rounded, the dashes are laid out clockwise
                // around the whole perimeter.

                float r_tr = quad.corner_radii.top_right;
                float r_br = quad.corner_radii.bottom_right;
                float r_bl = quad.corner_radii.bottom_left;
                float r_tl = quad.corner_radii.top_left;

                float w_t = quad.border_widths.top;
                float w_r = quad.border_widths.right;
                float w_b = quad.border_widths.bottom;
                float w_l = quad.border_widths.left;

                // Straight side dash velocities
                float dv_t = w_t <= 0.0 ? 0.0 : dv_numerator / w_t;
                float dv_r = w_r <= 0.0 ? 0.0 : dv_numerator / w_r;
                float dv_b = w_b <= 0.0 ? 0.0 : dv_numerator / w_b;
                float dv_l = w_l <= 0.0 ? 0.0 : dv_numerator / w_l;

                // Straight side lengths in dash space
                float s_t = (size.x - r_tl - r_tr) * dv_t;
                float s_r = (size.y - r_tr - r_br) * dv_r;
                float s_b = (size.x - r_br - r_bl) * dv_b;
                float s_l = (size.y - r_bl - r_tl) * dv_l;

                float corner_dash_velocity_tr = corner_dash_velocity(dv_t, dv_r);
                float corner_dash_velocity_br = corner_dash_velocity(dv_b, dv_r);
                float corner_dash_velocity_bl = corner_dash_velocity(dv_b, dv_l);
                float corner_dash_velocity_tl = corner_dash_velocity(dv_t, dv_l);

                // Corner lengths in dash space
                float c_tr = r_tr * (M_PI_F / 2.0) * corner_dash_velocity_tr;
                float c_br = r_br * (M_PI_F / 2.0) * corner_dash_velocity_br;
                float c_bl = r_bl * (M_PI_F / 2.0) * corner_dash_velocity_bl;
                float c_tl = r_tl * (M_PI_F / 2.0) * corner_dash_velocity_tl;

                // Cumulative dash space upto each segment
                float upto_tr = s_t;
                float upto_r = upto_tr + c_tr;
                float upto_br = upto_r + s_r;
                float upto_b = upto_br + c_br;
                float upto_bl = upto_b + s_b;
                float upto_l = upto_bl + c_bl;
                float upto_tl = upto_l + s_l;
                max_t = upto_tl + c_tl;

                if (is_near_rounded_corner) {
                    float radians = atan2(corner_center_to_point.y, corner_center_to_point.x);
                    float corner_t = radians * corner_radius;

                    if (center_to_point.x >= 0.0) {
                        if (center_to_point.y < 0.0) {
                            dash_velocity = corner_dash_velocity_tr;
                            // Subtracted because radians is pi/2 to 0 when
                            // going clockwise around the top right corner,
                            // since the y axis has been flipped
                            t = upto_r - corner_t * dash_velocity;
                        } else {
                            dash_velocity = corner_dash_velocity_br;
                            // Added because radians is 0 to pi/2 when going
                            // clockwise around the bottom-right corner
                            t = upto_br + corner_t * dash_velocity;
                        }
                    } else {
                        if (center_to_point.y >= 0.0) {
                            dash_velocity = corner_dash_velocity_bl;
                            // Subtracted because radians is pi/1 to 0 when
                            // going clockwise around the bottom-left corner,
                            // since the x axis has been flipped
                            t = upto_l - corner_t * dash_velocity;
                        } else {
                            dash_velocity = corner_dash_velocity_tl;
                            // Added because radians is 0 to pi/2 when going
                            // clockwise around the top-left corner, since both
                            // axis were flipped
                            t = upto_tl + corner_t * dash_velocity;
                        }
                    }
                } else {
                    // Straight borders
                    bool is_horizontal = corner_center_to_point.x < corner_center_to_point.y;
                    if (is_horizontal) {
                        if (center_to_point.y < 0.0) {
                            dash_velocity = dv_t;
                            t = (the_point.x - r_tl) * dash_velocity;
                        } else {
                            dash_velocity = dv_b;
                            t = upto_bl - (the_point.x - r_bl) * dash_velocity;
                        }
                    } else {
                        if (center_to_point.x < 0.0) {
                            dash_velocity = dv_l;
                            t = upto_tl - (the_point.y - r_tl) * dash_velocity;
                        } else {
                            dash_velocity = dv_r;
                            t = upto_r + (the_point.y - r_tr) * dash_velocity;
                        }
                    }
                }
            }
            float dash_length = dash_length_per_width / dash_period_per_width;
            float desired_dash_gap = dash_gap_per_width / dash_period_per_width;

            // Straight borders should start and end with a dash, so max_t is
            // reduced to cause this.
            max_t -= unrounded ? dash_length : 0.0;
            if (max_t >= 1.0) {
                // Adjust dash gap to evenly divide max_t
                float dash_count = floor(max_t);
                float dash_period = max_t / dash_count;
                border_color.a *= dash_alpha(t, dash_period, dash_length, dash_velocity, antialias_threshold);
            } else if (unrounded) {
                // When there isn't enough space for the full gap between the
                // two start / end dashes of a straight border, reduce gap to
                // make them fit.
                float dash_gap = max_t - dash_length;
                if (dash_gap > 0.0) {
                    float dash_period = dash_length + dash_gap;
                    border_color.a *= dash_alpha(t, dash_period, dash_length, dash_velocity, antialias_threshold);
                }
            }
        }

        border_color.a *= saturate(antialias_threshold - inner_sdf);
        color = over(background, border_color);
    }

    return color * float4(1.0, 1.0, 1.0, saturate(antialias_threshold - outer_sdf));
}

/*
**
**              Opaque quads
**
*/

struct OpaqueQuadFragmentInput {
    float4 position: SV_Position;
    nointerpolation float4 color: COLOR0;
};

OpaqueQuadFragmentInput opaque_quad_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    uint quad_id = batch_start_index - instance_id;
    Quad quad = quads[quad_id];

    OpaqueQuadFragmentInput output;
    if (!is_opaque_rectangle(quad)) {
        output.position = CULLED_POSITION;
        output.color = float4(0.0, 0.0, 0.0, 0.0);
        return output;
    }

    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    ClippedVertex vertex = clip_to_mask(unit_vertex, quad.bounds, quad.content_mask);

    output.position = to_device_position_impl(vertex.position);
    output.position.z = quad_depth(quad_id);
    output.color = hsla_to_rgba(quad.background.solid);
    return output;
}

float4 opaque_quad_fragment(OpaqueQuadFragmentInput input): SV_Target {
    return input.color;
}

/*
**
**              Shadows
**
*/

struct Shadow {
    uint order;
    float blur_radius;
    Bounds bounds;
    Corners corner_radii;
    Bounds content_mask;
    Hsla color;
    Bounds shape_bounds;
    Corners shape_corner_radii;
    uint inset;
    uint pad; // align to 8 bytes
};

struct PreparedBlurredRoundedRect {
    nointerpolation float4 shape: TEXCOORD1;
    nointerpolation float4 r1: TEXCOORD2;
    nointerpolation float4 exponent: TEXCOORD3;
    nointerpolation float4 scale: TEXCOORD4;
};

struct ShadowFragmentInput {
    nointerpolation uint shadow_id: TEXCOORD0;
    float4 position: SV_Position;
    nointerpolation float4 color: COLOR;
    PreparedBlurredRoundedRect blur;
};

StructuredBuffer<Shadow> shadows: register(t1);

float erf7(float x) {
    x = clamp(x, -10., 10.) * (2. / sqrt(M_PI_F));
    float xx = x * x;
    x = x + (0.24295 + (0.03395 + 0.0104 * xx) * xx) * (x * xx);
    return x / sqrt(1. + x * x);
}

// Raph Levien's blurred rounded rectangle approximation, whose blur s is sigma * sqrt(2):
// https://raphlinus.github.io/graphics/2020/04/21/blurred-rounded-rects.html
PreparedBlurredRoundedRect prepare_blurred_rounded_rect(float2 size, Corners corner_radii, float sigma) {
    float s = max(sigma * sqrt(2.), 1e-6);
    float s_inv = 1. / s;
    float min_edge = min(size.x, size.y);
    float r_max = 0.5 * min_edge;
    float4 radii = float4(corner_radii.top_left, corner_radii.top_right,
                          corner_radii.bottom_right, corner_radii.bottom_left);
    float4 r0 = min(sqrt(radii * radii + 1.15 * 1.15 * s * s), r_max);
    float4 r1 = min(sqrt(radii * radii + 4. * s * s), r_max);

    float2 half_extent = 0.5 * s_inv * size;
    float delta = 1.25 * s * (exp(-half_extent.x * half_extent.x) - exp(-half_extent.y * half_extent.y));
    float2 adjusted_size = float2(size.x + min(delta, 0.), size.y - max(delta, 0.));
    float4 peak = s_inv * 0.5 * (max(adjusted_size.x, adjusted_size.y) - 0.5 * radii);

    PreparedBlurredRoundedRect output;
    output.shape = float4(0.5 * adjusted_size, s_inv, min_edge);
    output.r1 = r1;
    output.exponent = 2. * r1 / r0;
    output.scale = 0.5 * float4(erf7(peak.x), erf7(peak.y), erf7(peak.z), erf7(peak.w));
    return output;
}

float pick_corner(float2 center_to_point, float4 values) {
    Corners corners = { values.x, values.y, values.z, values.w };
    return pick_corner_radius(center_to_point, corners);
}

float blurred_rounded_rect(float2 center_to_point, PreparedBlurredRoundedRect prepared) {
    float r1 = pick_corner(center_to_point, prepared.r1);
    float exponent = pick_corner(center_to_point, prepared.exponent);
    float scale = pick_corner(center_to_point, prepared.scale);
    float s_inv = prepared.shape.z;
    float min_edge = prepared.shape.w;

    float2 p0 = abs(center_to_point) - (prepared.shape.xy - r1);
    float2 p1 = max(p0, 0.);
    float d_pos;
    [branch] if (p1.x > 0. && p1.y > 0.) {
        d_pos = pow(pow(p1.x, exponent) + pow(p1.y, exponent), 1. / exponent);
    } else {
        d_pos = p1.x + p1.y;
    }

    float d_neg = min(max(p0.x, p0.y), 0.);
    float d = d_pos + d_neg - r1;
    return scale * (erf7(s_inv * (min_edge + d)) - erf7(s_inv * d));
}

ShadowFragmentInput shadow_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint shadow_id = batch_start_index + instance_id;
    Shadow shadow = shadows[shadow_id];

    ClippedVertex vertex = clip_to_mask(unit_vertex, shadow.bounds, shadow.content_mask);
    float4 device_position = to_device_position_impl(vertex.position);
    float4 color = hsla_to_rgba(shadow.color);

    ShadowFragmentInput output;
    output.position = device_position;
    output.color = color;
    output.shadow_id = shadow_id;
    output.blur = prepare_blurred_rounded_rect(shadow.shape_bounds.size, shadow.shape_corner_radii, shadow.blur_radius);

    return output;
}

float4 shadow_fragment(ShadowFragmentInput input): SV_TARGET {
    Shadow shadow = shadows[input.shadow_id];

    float alpha;
    if (shadow.blur_radius == 0.) {
        float distance = quad_sdf(input.position.xy, shadow.shape_bounds, shadow.shape_corner_radii);
        alpha = saturate(0.5 - distance);
    } else {
        float2 center = shadow.shape_bounds.origin + shadow.shape_bounds.size / 2.;
        alpha = blurred_rounded_rect(input.position.xy - center, input.blur);
    }

    if (shadow.inset != 0u) {
        // The inset shadow is the complement of the (blurred) hole rect, clipped to the element.
        // `saturate(0.5 - d)` gives a 1-pixel antialiased edge: d <= -0.5 -> 1, d >= 0.5 -> 0.
        alpha = 1.0 - alpha;
        float element_distance = quad_sdf(input.position.xy, shadow.bounds, shadow.corner_radii);
        alpha *= saturate(0.5 - element_distance);
    }

    return input.color * float4(1., 1., 1., alpha);
}

/*
**
**              Path Tiles
**
*/

struct PathCurve {
    float2 p0;
    float2 p1;
    float2 a;
    float2 b;
    float sign;
    uint pad;
};

struct PathTile {
    uint paint;
    uint curve_start;
    uint curve_count;
    int backdrop;
    float2 top_left;
    float2 bottom_right;
    float2 reference;
    float2 curves_top_left;
    float2 curves_bottom_right;
};

struct PathPaint {
    Bounds bounds;
    Bounds content_mask;
    Background color;
    uint even_odd;
    uint pad;
};

StructuredBuffer<PathTile> path_tiles: register(t1);
StructuredBuffer<PathCurve> path_curves: register(t2);
StructuredBuffer<PathPaint> path_paints: register(t3);

struct PathTileFragmentInput {
    float4 position: SV_Position;
    nointerpolation uint tile_id: TEXCOORD0;
    nointerpolation float4 curve_box: TEXCOORD1;
    PreparedBackground background;
};

PathTileFragmentInput path_tile_vertex(uint vertex_id: SV_VertexID, uint tile_id: SV_InstanceID) {
    uint tile_index = batch_start_index + tile_id;
    PathTile tile = path_tiles[tile_index];
    PathPaint paint = path_paints[tile.paint];

    // Neighboring tiles store their shared edge as the same number, so clipping each corner on its
    // own gives them bit-identical edges, which the rasterizer neither overlaps nor leaves gaps
    // between. Interpolating from an origin and a size would not.
    float2 mask_top_left = paint.content_mask.origin;
    float2 mask_bottom_right = paint.content_mask.origin + paint.content_mask.size;
    float2 top_left = clamp(tile.top_left, mask_top_left, mask_bottom_right);
    float2 bottom_right = clamp(tile.bottom_right, top_left, mask_bottom_right);
    float2 position = float2(
        (vertex_id & 1u) != 0u ? bottom_right.x : top_left.x,
        (vertex_id & 2u) != 0u ? bottom_right.y : top_left.y);

    PathTileFragmentInput output;
    output.position = to_device_position_impl(position);
    output.tile_id = tile_index;
    output.curve_box = float4(tile.curves_top_left, tile.curves_bottom_right);
    output.background = prepare_background(paint.color);
    return output;
}

// Solves a*t^2 + b*t + c = 0 for the root in [0, 1] on a monotone piece.
float monotone_quadratic_root(float a, float b, float c, float direction) {
    float discriminant = max(b * b - 4.0 * a * c, 0.0);
    float denominator = b + direction * sqrt(discriminant);
    if (denominator == 0.0) {
        return 0.0;
    }
    return saturate(-2.0 * c / denominator);
}

// Coverage estimate adapted from https://github.com/EricLengyel/Slug/blob/be3c13eb7d63f9e8aa5c583e42d92c374cb91d98/SlugPixelShader.hlsl
// Copyright (c) 2017 Eric Lengyel.
// Licensed under the MIT license.

// Accumulates the curve's winding along the pixel's leftward and upward rays. Like Slug, a crossing
// within half a pixel of the center counts fractionally.
void add_curve_winding(PathCurve curve, float2 center, inout float2 winding, inout float2 weight) {
    float min_x = min(curve.p0.x, curve.p1.x);
    float max_x = max(curve.p0.x, curve.p1.x);

    [branch]
    if (curve.p0.y <= center.y && center.y < curve.p1.y) {
        [branch]
        if (max_x <= center.x - 0.5) {
            winding.x += curve.sign;
        } else {
            float t = monotone_quadratic_root(curve.a.y, curve.b.y, curve.p0.y - center.y, 1.0);
            float x = (curve.a.x * t + curve.b.x) * t + curve.p0.x;
            float distance = center.x - x;
            winding.x += curve.sign * saturate(distance + 0.5);
            weight.x = max(weight.x, saturate(1.0 - 2.0 * abs(distance)));
        }
    }

    [branch]
    if (min_x <= center.x && center.x < max_x) {
        float direction = curve.p1.x > curve.p0.x ? 1.0 : -1.0;
        [branch]
        if (curve.p1.y <= center.y - 0.5) {
            winding.y -= curve.sign * direction;
        } else if (curve.p0.y < center.y + 0.5) {
            float t = monotone_quadratic_root(curve.a.x, curve.b.x, curve.p0.x - center.x, direction);
            float y = (curve.a.y * t + curve.b.y) * t + curve.p0.y;
            float distance = center.y - y;
            winding.y -= curve.sign * direction * saturate(distance + 0.5);
            weight.y = max(weight.y, saturate(1.0 - 2.0 * abs(distance)));
        }
    }
}

// The backdrop gives the unlisted curves' winding along a leftward ray from the tile's reference
// corner. We need it at the current pixel, along both leftward and upward rays. Because paths are
// closed, chains of unlisted curves can only begin or end where a listed curve does. So, we can
// compute the winding at the current pixel by correcting the backdrop for each listed endpoint
// between its ray and the pixel.
void correct_for_endpoint(float2 endpoint, float step, float2 reference, float2 center,
                          inout float2 winding) {
    if (endpoint.x <= reference.x) {
        winding.x -= step * (float(endpoint.y <= center.y) - float(endpoint.y <= reference.y));
    }
    if (endpoint.y <= reference.y && endpoint.x <= center.x) {
        winding.y += step;
    }
}

float4 path_tile_fragment(PathTileFragmentInput input): SV_Target {
    float2 center = floor(input.position.xy) + 0.5;
    if (input.background.tag == 0u
        && (any(center < input.curve_box.xy) || any(center > input.curve_box.zw))) {
        return input.background.color0;
    }
    PathTile tile = path_tiles[input.tile_id];
    PathPaint paint = path_paints[tile.paint];

    float2 winding = float(tile.backdrop);
    float2 weight = float2(0.0, 0.0);
    [loop]
    for (uint i = 0u; i < tile.curve_count; i++) {
        PathCurve curve = path_curves[tile.curve_start + i];
        if (min(curve.p0.x, curve.p1.x) >= center.x + 0.5) {
            break;
        }
        if (curve.p0.y < center.y + 0.5) {
            add_curve_winding(curve, center, winding, weight);
            correct_for_endpoint(curve.p0, curve.sign, tile.reference, center, winding);
            correct_for_endpoint(curve.p1, -curve.sign, tile.reference, center, winding);
        }
    }

    // Weight the winding computed by each ray by its nearest crossing's distance from the pixel center,
    // using Slug's technique.
    float coverage = max(
        abs(dot(winding, weight)) / max(weight.x + weight.y, 1.0 / 65536.0),
        min(abs(winding.x), abs(winding.y)));
    coverage = paint.even_odd != 0u
        ? 1.0 - abs(1.0 - frac(coverage * 0.5) * 2.0)
        : saturate(coverage);

    if (coverage == 0.0) {
        return float4(0.0, 0.0, 0.0, 0.0);
    }

    float4 color =
        background_color(input.background, paint.color, paint.bounds, input.position.xy);
    return float4(color.rgb, color.a * coverage);
}

/*
**
**              Underlines
**
*/

struct Underline {
    uint order;
    uint pad;
    Bounds bounds;
    Bounds content_mask;
    Hsla color;
    float thickness;
    uint wavy;
};

struct UnderlineFragmentInput {
  nointerpolation uint underline_id: TEXCOORD0;
  float4 position: SV_Position;
  nointerpolation float4 color: COLOR;
};

StructuredBuffer<Underline> underlines: register(t1);

UnderlineFragmentInput underline_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint underline_id = batch_start_index + instance_id;
    Underline underline = underlines[underline_id];
    ClippedVertex vertex = clip_to_mask(unit_vertex, underline.bounds, underline.content_mask);
    float4 device_position = to_device_position_impl(vertex.position);
    float4 color = hsla_to_rgba(underline.color);

    UnderlineFragmentInput output;
    output.position = device_position;
    output.color = color;
    output.underline_id = underline_id | (underline.wavy == 0u ? SIMPLE_PRIMITIVE_FLAG : 0u);
    return output;
}

float4 underline_fragment(UnderlineFragmentInput input): SV_Target {
    const float WAVE_FREQUENCY = 2.0;
    const float WAVE_HEIGHT_RATIO = 0.8;

    if ((input.underline_id & SIMPLE_PRIMITIVE_FLAG) != 0u) {
        return input.color;
    }

    Underline underline = underlines[input.underline_id];
    float half_thickness = underline.thickness * 0.5;
    float2 origin = underline.bounds.origin;

    float2 st = ((input.position.xy - origin) / underline.bounds.size.y) - float2(0., 0.5);
    float frequency = (M_PI_F * WAVE_FREQUENCY * underline.thickness) / underline.bounds.size.y;
    float amplitude = (underline.thickness * WAVE_HEIGHT_RATIO) / underline.bounds.size.y;

    float sine = sin(st.x * frequency) * amplitude;
    float dSine = cos(st.x * frequency) * amplitude * frequency;
    float distance = (st.y - sine) / sqrt(1. + dSine * dSine);
    float distance_in_pixels = distance * underline.bounds.size.y;
    float distance_from_top_border = distance_in_pixels - half_thickness;
    float distance_from_bottom_border = distance_in_pixels + half_thickness;
    float alpha = saturate(
        0.5 - max(-distance_from_bottom_border, distance_from_top_border));
    return input.color * float4(1., 1., 1., alpha);
}

/*
**
**              Monochrome sprites
**
*/

struct MonochromeSprite {
    uint order;
    uint pad;
    AtlasTile tile;
    Bounds bounds;
    Bounds content_mask;
    Hsla color;
    TransformationMatrix transformation;
};

struct MonochromeSpriteFragmentInput {
    float4 position: SV_Position;
    float2 tile_position: POSITION;
    nointerpolation float4 color: COLOR;
    float4 clip_distance: SV_ClipDistance;
};

StructuredBuffer<MonochromeSprite> mono_sprites: register(t1);

MonochromeSpriteFragmentInput monochrome_sprite_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint sprite_id = batch_start_index + instance_id;
    MonochromeSprite sprite = mono_sprites[sprite_id];
    float4 device_position =
        to_device_position_transformed(unit_vertex, sprite.bounds, sprite.transformation);
    float4 clip_distance = distance_from_clip_rect_transformed(unit_vertex, sprite.bounds, sprite.content_mask, sprite.transformation);
    float2 tile_position = to_tile_position(unit_vertex, sprite.tile);
    float4 color = hsla_to_rgba(sprite.color);

    MonochromeSpriteFragmentInput output;
    output.position = device_position;
    output.tile_position = tile_position;
    output.color = color;
    output.clip_distance = clip_distance;
    return output;
}

float4 monochrome_sprite_fragment(MonochromeSpriteFragmentInput input): SV_Target {
    float sample = t_sprite.Sample(s_sprite, input.tile_position).r;
    float alpha_corrected = apply_contrast_and_gamma_correction(sample, input.color.rgb, grayscale_enhanced_contrast, gamma_ratios);
    return float4(input.color.rgb, input.color.a * alpha_corrected);
}

MonochromeSpriteFragmentInput subpixel_sprite_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    return monochrome_sprite_vertex(vertex_id, instance_id);
}

SubpixelSpriteFragmentOutput subpixel_sprite_fragment(MonochromeSpriteFragmentInput input) {
    float3 sample = t_sprite.Sample(s_sprite, input.tile_position).rgb;
    if (is_bgr) {
        sample = sample.bgr;
    }
    float3 alpha_corrected = apply_contrast_and_gamma_correction3(sample, input.color.rgb, subpixel_enhanced_contrast, gamma_ratios);

    SubpixelSpriteFragmentOutput output;
    output.foreground = float4(input.color.rgb, 1.0f);
    output.alpha = float4(input.color.a * alpha_corrected, 1.0f);
    return output;
}

/*
**
**              Polychrome sprites
**
*/

struct PolychromeSprite {
    uint order;
    uint pad;
    AtlasTile tile;
    uint grayscale;
    float opacity;
    Bounds bounds;
    Bounds content_mask;
    Corners corner_radii;
};

struct PolychromeSpriteFragmentInput {
    nointerpolation uint sprite_id: TEXCOORD0;
    float4 position: SV_Position;
    float2 tile_position: POSITION;
    nointerpolation float opacity: TEXCOORD1;
};

StructuredBuffer<PolychromeSprite> poly_sprites: register(t1);

PolychromeSpriteFragmentInput polychrome_sprite_vertex(uint vertex_id: SV_VertexID, uint instance_id: SV_InstanceID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    uint sprite_id = batch_start_index + instance_id;
    PolychromeSprite sprite = poly_sprites[sprite_id];
    ClippedVertex vertex = clip_to_mask(unit_vertex, sprite.bounds, sprite.content_mask);
    float4 device_position = to_device_position_impl(vertex.position);
    float2 tile_position = to_tile_position(vertex.unit_vertex, sprite.tile);

    PolychromeSpriteFragmentInput output;
    output.position = device_position;
    output.tile_position = tile_position;
    bool simple = !has_rounded_corners(sprite.corner_radii) && sprite.grayscale == 0u;
    output.sprite_id = sprite_id | (simple ? SIMPLE_PRIMITIVE_FLAG : 0u);
    output.opacity = sprite.opacity;
    return output;
}

float4 polychrome_sprite_fragment(PolychromeSpriteFragmentInput input): SV_Target {
    float4 color = t_sprite.Sample(s_sprite, input.tile_position);
    if ((input.sprite_id & SIMPLE_PRIMITIVE_FLAG) != 0u) {
        return float4(color.rgb, color.a * input.opacity);
    }

    PolychromeSprite sprite = poly_sprites[input.sprite_id];
    if (sprite.grayscale != 0u) {
        color.rgb = dot(color.rgb, GRAYSCALE_FACTORS);
    }
    float distance = quad_sdf(input.position.xy, sprite.bounds, sprite.corner_radii);
    color.a *= input.opacity * saturate(0.5 - distance);
    return color;
}

/*
**
**              View transitions
**
*/

// A full-window transition between two frames: `t_transition_old` (the frame
// captured before the change) and `t_transition_new` (this frame), matching
// gpui_wgpu's shaders_view_transition.wgsl pixel-for-pixel across all modes:
//   0: RadialExpand, 1: RadialCollapse, 2: CrossFade,
//   3: Shockwave (optical refraction + RGB dispersion + caustic ridge),
//   4: PageCurl (3D cylindrical curl + specular ridge + cast shadow),
//   5: LiquidDissolve (domain-warped FBM liquid melt + meniscus refraction).
cbuffer ViewTransitionParams: register(b2) {
    float2 vt_origin;
    float vt_radius;
    float vt_feather;
    float vt_progress;
    uint vt_kind;
    float2 vt_viewport_size;
};

Texture2D<float4> t_transition_old: register(t0);
Texture2D<float4> t_transition_new: register(t1);
SamplerState s_transition: register(s0);

// One triangle that covers the viewport: (-1, -1), (3, -1), (-1, 3).
float4 view_transition_vertex(uint vertex_id: SV_VertexID): SV_Position {
    float2 p = float2((vertex_id << 1) & 2, vertex_id & 2);
    return float4(p * 2.0 - 1.0, 0.0, 1.0);
}

float4 vt_sample_old(float2 uv) {
    return t_transition_old.SampleLevel(s_transition, saturate(uv), 0.0);
}

float4 vt_sample_new(float2 uv) {
    return t_transition_new.SampleLevel(s_transition, saturate(uv), 0.0);
}

float4 vt_load_old(float2 frag_pos) {
    return t_transition_old.Load(int3(int2(frag_pos), 0));
}

float4 vt_load_new(float2 frag_pos) {
    return t_transition_new.Load(int3(int2(frag_pos), 0));
}

float vt_rect_sdf(float2 p, float2 dims) {
    return max(max(-p.x, p.x - dims.x), max(-p.y, p.y - dims.y));
}

float vt_hash21(float2 p) {
    float3 p3 = frac(float3(p.x, p.y, p.x) * 0.1031);
    p3 += dot(p3, p3.yzx + 33.33);
    return frac((p3.x + p3.y) * p3.z);
}

float vt_noise2d(float2 p) {
    float2 i = floor(p);
    float2 f = frac(p);
    float2 u = f * f * f * (f * (f * 6.0 - 15.0) + 10.0);
    float a = vt_hash21(i);
    float b = vt_hash21(i + float2(1.0, 0.0));
    float c = vt_hash21(i + float2(0.0, 1.0));
    float d = vt_hash21(i + float2(1.0, 1.0));
    return lerp(lerp(a, b, u.x), lerp(c, d, u.x), u.y);
}

float vt_fbm(float2 p) {
    float val = 0.0;
    float amp = 0.5;
    [unroll]
    for (int i = 0; i < 4; i++) {
        val += amp * vt_noise2d(p);
        p = float2(p.x * 1.60 - p.y * 1.24, p.x * 1.24 + p.y * 1.60) + float2(13.7, 7.3);
        amp *= 0.5;
    }
    return val / 0.9375;
}

float vt_warped_fbm(float2 q) {
    float wx = vt_fbm(q + float2(5.2, 1.3));
    float wy = vt_fbm(q + float2(1.7, 9.2));
    return vt_fbm(q + float2(wx - 0.5, wy - 0.5) * 1.35);
}

float4 vt_shockwave(float2 frag_pos, float2 dims) {
    float scale = max(vt_feather, 1.0);
    float p = saturate(vt_progress);
    float2 delta = frag_pos - vt_origin;
    float dist = length(delta);
    float2 dir = delta / max(dist, 0.001);

    float band = 82.0 * scale;
    float d = dist - vt_radius;
    float wake_d = d + band * 0.88;
    float time_env = smoothstep(0.0, 0.07, p) * smoothstep(1.0, 0.84, p);

    if ((abs(d) > band * 2.4 && abs(wake_d) > band * 1.8) || time_env <= 0.0005) {
        return d < 0.0 ? vt_load_new(frag_pos) : vt_load_old(frag_pos);
    }

    float sigma_main = band * 0.44;
    float wave_env = exp(-(d * d) / (2.0 * sigma_main * sigma_main)) * time_env;
    float ripple = sin((d / band) * M_PI_F * 2.0) * wave_env;

    float sigma_wake = band * 0.30;
    float wake_env = exp(-(wake_d * wake_d) / (2.0 * sigma_wake * sigma_wake)) * time_env * 0.28;
    float wake_ripple = sin((wake_d / band) * 4.8) * wake_env;

    float border_dist = min(min(frag_pos.x, dims.x - frag_pos.x), min(frag_pos.y, dims.y - frag_pos.y));
    float edge_damp = smoothstep(0.0, 22.0 * scale, border_dist);

    float disp_px = (ripple + wake_ripple) * (20.0 * scale) * edge_damp;
    float2 uv_r = (frag_pos - dir * disp_px * 0.76) / dims;
    float2 uv_g = (frag_pos - dir * disp_px * 1.00) / dims;
    float2 uv_b = (frag_pos - dir * disp_px * 1.28) / dims;

    float4 old_g = vt_sample_old(uv_g);
    float4 old_col = float4(vt_sample_old(uv_r).r, old_g.g, vt_sample_old(uv_b).b, old_g.a);
    float4 new_g = vt_sample_new(uv_g);
    float4 new_col = float4(vt_sample_new(uv_r).r, new_g.g, vt_sample_new(uv_b).b, new_g.a);

    float mix_w = 18.0 * scale;
    float show_old = smoothstep(-mix_w * 0.5, mix_w * 0.5, d);
    float4 color = lerp(new_col, old_col, show_old);

    float crest_sigma = band * 0.13;
    float crest_d = d - band * 0.05;
    float crest = exp(-(crest_d * crest_d) / (2.0 * crest_sigma * crest_sigma)) * time_env;
    float2 light_dir = normalize(float2(-0.55, -0.83));
    float spec_dir = 0.58 + 0.42 * max(dot(dir, light_dir), 0.0);
    float caustic = crest * spec_dir * 0.23;

    float trough_sigma = band * 0.17;
    float trough_d = d + band * 0.22;
    float trough = exp(-(trough_d * trough_d) / (2.0 * trough_sigma * trough_sigma)) * time_env * 0.10;

    float3 lit_rgb = color.rgb * (1.0 - trough) + float3(caustic * 0.90, caustic * 0.97, caustic * 1.08) * color.a;
    return float4(saturate(lit_rgb), color.a);
}

float4 vt_vellum_backside(float4 back_tex, float shade, float spec, float edge_rim) {
    float luma = dot(back_tex.rgb, float3(0.299, 0.587, 0.114));
    float3 vellum = lerp(float3(0.16, 0.18, 0.24), float3(0.95, 0.96, 0.98), smoothstep(0.16, 0.72, luma));
    float3 base_rgb = lerp(back_tex.rgb, vellum, 0.50) * shade
        + float3(spec * 0.96, spec * 0.98, spec * 1.02)
        + float3(edge_rim * 0.22, edge_rim * 0.22, edge_rim * 0.22);
    return float4(saturate(base_rgb), back_tex.a);
}

float4 vt_page_curl(float2 frag_pos, float2 dims) {
    float scale = max(vt_feather, 1.0);
    float p = saturate(vt_progress);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos);
    }

    float2 start_corner = float2(
        vt_origin.x >= dims.x * 0.5 ? dims.x : 0.0,
        vt_origin.y >= dims.y * 0.5 ? dims.y : 0.0
    );
    float2 end_corner = dims - start_corner;
    float2 diag_vec = end_corner - start_corner;
    float diag = max(length(diag_vec), 1.0);
    float2 curl_dir = diag_vec / diag;

    float R = clamp(min(dims.x, dims.y) * 0.135, 48.0 * scale, 148.0 * scale);
    float start_fade = smoothstep(0.0, 0.07, p);
    float end_fade = smoothstep(1.0, 0.88, p);
    float axis_s = lerp(0.0, diag + R * 2.65, p);

    float s = dot(frag_pos - start_corner, curl_dir);
    float d = s - axis_s;

    if (d > 0.0) {
        float2 p_back = frag_pos - (M_PI_F * R + 2.0 * d) * curl_dir;
        float bdist = vt_rect_sdf(p_back, dims);

        float4 base_old = vt_load_old(frag_pos);
        float crease_ao = 1.0 - 0.11 * exp(-d / (R * 0.28)) * start_fade;
        float flap_shadow = bdist > 0.0
            ? (1.0 - 0.34 * exp(-bdist / (R * 0.28)) * smoothstep(R * 1.25, 0.0, bdist) * start_fade)
            : 1.0;
        base_old.rgb *= crease_ao * flap_shadow;

        if (bdist <= 0.85) {
            float4 back_tex = vt_sample_old(p_back / dims);
            float3 l_dir = normalize(float3(-0.42, -0.55, 0.72));
            float3 h_vec = normalize(l_dir + float3(0.0, 0.0, 1.0));
            float flat_diffuse = 0.62 + 0.38 * l_dir.z;
            float flat_spec = pow(max(h_vec.z, 0.0), 20.0) * 0.40 * start_fade;
            float shade = lerp(flat_diffuse, 0.96, smoothstep(0.0, R * 1.4, d));
            float spec = flat_spec * exp(-d / (R * 0.55));
            float edge_rim = smoothstep(2.2 * scale, 0.0, abs(bdist)) * start_fade;
            float4 back_col = vt_vellum_backside(back_tex, shade, spec, edge_rim);
            float aa = smoothstep(0.85, -0.85, bdist);
            return lerp(base_old, back_col, aa);
        }
        return base_old;
    }

    if (d >= -R) {
        float sin_t = saturate(-d / R);
        float theta = asin(sin_t);
        float cos_t = sqrt(max(1.0 - sin_t * sin_t, 0.0));

        float4 new_raw = vt_load_new(frag_pos);
        float under_shadow = 1.0 - 0.48 * pow(cos_t, 1.15) * start_fade;
        float4 out_col = float4(new_raw.rgb * under_shadow, new_raw.a);

        float s_bot = axis_s - R * theta;
        float2 p_bot = frag_pos + (s_bot - s) * curl_dir;
        float dist_bot = vt_rect_sdf(p_bot, dims);
        if (dist_bot <= 0.85) {
            float4 bot_tex = vt_sample_old(p_bot / dims);
            float bot_shade = 0.80 + 0.20 * cos_t;
            float4 bot_col = float4(bot_tex.rgb * bot_shade, bot_tex.a);
            float aa_bot = smoothstep(0.85, -0.85, dist_bot);
            out_col = lerp(out_col, bot_col, aa_bot);
        }

        float theta_top = M_PI_F - theta;
        float s_top = axis_s - R * theta_top;
        float2 p_top = frag_pos + (s_top - s) * curl_dir;
        float dist_top = vt_rect_sdf(p_top, dims);
        if (dist_top <= 0.85) {
            float4 top_tex = vt_sample_old(p_top / dims);
            float3 n_cyl = normalize(float3(-curl_dir * sin_t, cos_t));
            float3 l_dir = normalize(float3(-0.42, -0.55, 0.72));
            float3 v_dir = float3(0.0, 0.0, 1.0);
            float3 h_vec = normalize(l_dir + v_dir);
            float ndl = max(dot(n_cyl, l_dir), 0.0);
            float diffuse = 0.62 + 0.38 * ndl;
            float spec = pow(max(dot(n_cyl, h_vec), 0.0), 20.0) * 0.40 * start_fade;
            float edge_rim = smoothstep(2.2 * scale, 0.0, abs(dist_top)) * start_fade;
            float4 top_col = vt_vellum_backside(top_tex, diffuse, spec, edge_rim);
            float aa_top = smoothstep(0.85, -0.85, dist_top);
            out_col = lerp(out_col, top_col, aa_top);
        }
        return out_col;
    }

    float4 new_raw = vt_load_new(frag_pos);
    float dist_behind = -d - R;
    float trail_shadow = 1.0
        - 0.26 * exp(-dist_behind / (R * 0.36)) * smoothstep(R * 1.45, 0.0, dist_behind) * start_fade * end_fade;
    return float4(new_raw.rgb * trail_shadow, new_raw.a);
}

float4 vt_liquid_dissolve(float2 frag_pos, float2 dims) {
    float scale = max(vt_feather, 1.0);
    float p = saturate(vt_progress);
    if (p <= 0.0005) {
        return vt_load_old(frag_pos);
    }
    if (p >= 0.9995) {
        return vt_load_new(frag_pos);
    }

    float2 q = frag_pos / (120.0 * scale);
    float norm_dist = saturate(length(frag_pos - vt_origin) / max(vt_radius, 1.0));
    float n = vt_warped_fbm(q);
    float field = norm_dist * 0.66 + n * 0.34;
    float threshold = lerp(-0.11, 1.11, p);
    float diff = field - threshold;
    float time_env = smoothstep(0.0, 0.07, p) * smoothstep(1.0, 0.88, p);

    if (abs(diff) > 0.095 || time_env <= 0.0005) {
        return diff < 0.0 ? vt_load_new(frag_pos) : vt_load_old(frag_pos);
    }

    float eps = 0.045;
    float nx = vt_warped_fbm(q + float2(eps, 0.0)) - vt_warped_fbm(q - float2(eps, 0.0));
    float ny = vt_warped_fbm(q + float2(0.0, eps)) - vt_warped_fbm(q - float2(0.0, eps));
    float2 grad = float2(nx, ny) / (2.0 * eps);

    float border_dist = min(min(frag_pos.x, dims.x - frag_pos.x), min(frag_pos.y, dims.y - frag_pos.y));
    float edge_damp = smoothstep(0.0, 20.0 * scale, border_dist);

    float meniscus = exp(-(diff * diff) / (2.0 * 0.028 * 0.028)) * time_env;
    float2 disp_px = grad * meniscus * (18.0 * scale) * edge_damp;

    float2 uv_r = (frag_pos + disp_px * 0.80) / dims;
    float2 uv_g = (frag_pos + disp_px * 1.00) / dims;
    float2 uv_b = (frag_pos + disp_px * 1.22) / dims;

    float4 old_g = vt_sample_old(uv_g);
    float4 old_col = float4(vt_sample_old(uv_r).r, old_g.g, vt_sample_old(uv_b).b, old_g.a);
    float4 new_g = vt_sample_new(uv_g);
    float4 new_col = float4(vt_sample_new(uv_r).r, new_g.g, vt_sample_new(uv_b).b, new_g.a);

    float show_old = smoothstep(-0.018, 0.018, diff);
    float4 color = lerp(new_col, old_col, show_old);

    float rim = exp(-(diff * diff) / (2.0 * 0.009 * 0.009)) * time_env;
    float3 rim_tint = lerp(float3(0.72, 0.90, 1.0), float3(0.96, 0.78, 1.0), n);
    float inner_d = diff + 0.018;
    float inner_shade = exp(-(inner_d * inner_d) / (2.0 * 0.014 * 0.014)) * time_env * 0.10;
    float3 lit_rgb = color.rgb * (1.0 - inner_shade) + rim_tint * (rim * 0.34 * color.a);
    return float4(saturate(lit_rgb), color.a);
}

float4 view_transition_fragment(float4 position: SV_Position): SV_Target {
    float2 dims = max(vt_viewport_size, float2(1.0, 1.0));
    if (vt_kind == 3u) {
        return vt_shockwave(position.xy, dims);
    }
    if (vt_kind == 4u) {
        return vt_page_curl(position.xy, dims);
    }
    if (vt_kind == 5u) {
        return vt_liquid_dissolve(position.xy, dims);
    }

    float4 old_color = vt_load_old(position.xy);
    float4 new_color = vt_load_new(position.xy);
    float show_old;
    if (vt_kind == 2u) {
        // Cross-fade.
        show_old = 1.0 - saturate(vt_progress);
    } else {
        float edge = smoothstep(vt_radius - max(vt_feather, 0.5), vt_radius + max(vt_feather, 0.5), length(position.xy - vt_origin));
        // 0: the new frame inside a growing circle; 1: the old frame inside a shrinking one.
        show_old = vt_kind == 0u ? edge : 1.0 - edge;
    }
    return lerp(new_color, old_color, show_old);
}

/*
**
**              Backdrop blur and liquid glass (gpui::backdrop_blur)
**
*/

// The dual-Kawase passes, the composite and the glass, as
// `gpui::backdrop_blur` defines them (the WGSL in gpui_wgpu is the same).
// The target holds premultiplied alpha. The layout is
// `gpui::backdrop_blur::BlurParams` (128 bytes).
cbuffer BackdropBlurParams: register(b2) {
    float2 blur_viewport_size;   // composite: the target's size in pixels
    float2 blur_texel_size;      // the source level's texel size, in UV
    float blur_offset;           // the plan's sample offset, in source texels
    uint blur_straight_alpha;    // always 0 here
    uint blur_from_snapshot;
    uint blur_flags;             // 1: blurred, 2: glass, bits 8+: progressive mode
    float2 blur_bounds_origin;
    float2 blur_bounds_size;
    float4 blur_corner_radii; // (top_left, top_right, bottom_right, bottom_left)
    float2 blur_mask_origin;
    float2 blur_mask_size;
    float blur_refraction;
    float blur_dispersion;
    float blur_bevel_width;
    float blur_specular;
    float2 blur_pointer_pos;
    float blur_scale_factor;
    float blur_shadow_opacity;
    float4 blur_tint_rgba;
};

Texture2D<float4> t_blur_src: register(t0);
// The copy of the target taken for this blur batch (level 0).
Texture2D<float4> t_blur_snapshot: register(t1);
SamplerState s_blur: register(s0);

struct BlurFullscreenInput {
    float4 position : SV_Position;
    float2 uv : TEXCOORD0;
};

BlurFullscreenInput blur_fullscreen_vertex_impl(uint vertex_id) {
    float2 p = float2((vertex_id << 1) & 2, vertex_id & 2);
    BlurFullscreenInput output;
    output.position = float4(p.x * 2.0 - 1.0, 1.0 - p.y * 2.0, 0.0, 1.0);
    output.uv = p;
    return output;
}

BlurFullscreenInput kawase_downsample_vertex(uint vertex_id: SV_VertexID) {
    return blur_fullscreen_vertex_impl(vertex_id);
}

float4 blur_source(float2 uv) {
    return t_blur_src.SampleLevel(s_blur, uv, 0.0);
}

// The 5-tap downsample.
float4 kawase_downsample_fragment(BlurFullscreenInput input): SV_Target {
    float2 d = blur_texel_size * blur_offset;
    float4 sum = blur_source(input.uv) * 4.0;
    sum += blur_source(input.uv + float2(-d.x, -d.y));
    sum += blur_source(input.uv + float2(d.x, -d.y));
    sum += blur_source(input.uv + float2(-d.x, d.y));
    sum += blur_source(input.uv + float2(d.x, d.y));
    return sum / 8.0;
}

// The 8-tap tent upsample.
float4 blur_tent(float2 uv) {
    float2 d = blur_texel_size * blur_offset;
    float4 sum = blur_source(uv + float2(-d.x * 2.0, 0.0));
    sum += blur_source(uv + float2(-d.x, d.y)) * 2.0;
    sum += blur_source(uv + float2(0.0, d.y * 2.0));
    sum += blur_source(uv + float2(d.x, d.y)) * 2.0;
    sum += blur_source(uv + float2(d.x * 2.0, 0.0));
    sum += blur_source(uv + float2(d.x, -d.y)) * 2.0;
    sum += blur_source(uv + float2(0.0, -d.y * 2.0));
    sum += blur_source(uv + float2(-d.x, -d.y)) * 2.0;
    return sum / 12.0;
}

BlurFullscreenInput kawase_upsample_vertex(uint vertex_id: SV_VertexID) {
    return blur_fullscreen_vertex_impl(vertex_id);
}

float4 kawase_upsample_fragment(BlurFullscreenInput input): SV_Target {
    return blur_tent(input.uv);
}

// One quad over the blur's painted bounds (`BackdropBlur::painted_bounds`)
// within its content mask.
BlurFullscreenInput backdrop_blur_vertex(uint vertex_id: SV_VertexID) {
    float2 unit_vertex = float2(float(vertex_id & 1u), 0.5 * float(vertex_id & 2u));
    float scale = max(blur_scale_factor, 1.0);
    bool has_shadow = blur_shadow_opacity > 0.0005;
    float2 pad_min = has_shadow ? float2(40.0, 32.0) * scale : float2(0.0, 0.0);
    float2 pad_max = has_shadow ? float2(40.0, 52.0) * scale : float2(0.0, 0.0);
    float2 clip_min = max(blur_bounds_origin - pad_min, blur_mask_origin);
    float2 clip_max = min(blur_bounds_origin + blur_bounds_size + pad_max, blur_mask_origin + blur_mask_size);
    float2 clipped_size = max(clip_max - clip_min, float2(0.0, 0.0));
    float2 px_pos = clip_min + unit_vertex * clipped_size;
    BlurFullscreenInput output;
    output.position = float4((px_pos.x / blur_viewport_size.x) * 2.0 - 1.0, 1.0 - (px_pos.y / blur_viewport_size.y) * 2.0, 0.0, 1.0);
    output.uv = px_pos / blur_viewport_size;
    return output;
}

float blur_pick_corner_radius(float2 pos, float4 radii) {
    if (pos.x < 0.0) {
        return pos.y < 0.0 ? radii.x : radii.w;
    }
    return pos.y < 0.0 ? radii.y : radii.z;
}

float blur_rounded_rect_sdf(float2 rel, float2 half_size, float4 radii) {
    float r = min(blur_pick_corner_radius(rel, radii), min(half_size.x, half_size.y));
    float2 q = abs(rel) - (half_size - float2(r, r));
    return length(max(q, float2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
}

// The blurred backdrop at `uv`: the composite's final upsample.
float4 blurred_at(float2 uv) {
    return blur_tent(saturate(uv));
}

float4 snapshot_at(float2 uv) {
    return t_blur_snapshot.SampleLevel(s_blur, saturate(uv), 0.0);
}

// The backdrop through the glass at three refracted positions, `blur_mix`
// of the way from sharp to blurred.
float4 glass_backdrop(float2 uv_r, float2 uv_g, float2 uv_b, float blur_mix, bool blurred) {
    float4 sg = snapshot_at(uv_g);
    float4 sharp = float4(snapshot_at(uv_r).r, sg.g, snapshot_at(uv_b).b, sg.a);
    float4 bg = blurred_at(uv_g);
    float4 soft = float4(blurred_at(uv_r).r, bg.g, blurred_at(uv_b).b, bg.a);
    return blurred ? lerp(sharp, soft, blur_mix) : sharp;
}

// The glass color (premultiplied): refraction, dispersion, progressive blur,
// tint and the lit bevel.
float4 glass_color(float2 frag_px, float2 rel, float2 half_size, float2 q, float dist, bool blurred) {
    float scale = max(blur_scale_factor, 1.0);
    float2 q_pos = max(q, float2(0.0, 0.0));
    float outside_len = length(q_pos);
    float depth_in = max(-dist, 0.0);
    float bevel = clamp(blur_bevel_width, 4.0 * scale, min(half_size.x, half_size.y));
    float u = saturate(depth_in / max(bevel, 1.0));
    float one_minus_u = 1.0 - u;
    float2 sgn = float2(rel.x >= 0.0 ? 1.0 : -1.0, rel.y >= 0.0 ? 1.0 : -1.0);
    float2 grad;
    if (outside_len > 0.0001) {
        grad = (q_pos / outside_len) * sgn;
    } else {
        float diag_blend = smoothstep(-2.5 * scale, 2.5 * scale, q.x - q.y);
        grad = normalize(float2(sgn.x * diag_blend, sgn.y * (1.0 - diag_blend)) + float2(1e-5, 1e-5));
    }
    float rim_slope = pow(one_minus_u, 1.85);
    float2 body_tilt = (rel / half_size) * (0.10 * min(blur_refraction, 1.5));
    float2 xy_tilt = grad * (rim_slope * 0.76) + body_tilt;
    float n_z = sqrt(max(1.0 - min(dot(xy_tilt, xy_tilt), 0.96), 0.04));
    float3 normal = normalize(float3(xy_tilt, n_z));
    float ior = 1.0 + 0.42 * blur_refraction;
    float eta = 1.0 / ior;
    float k = max(1.0 - eta * eta * (1.0 - normal.z * normal.z), 0.0);
    float2 snell_xy = normal.xy * (eta * normal.z - sqrt(k));
    float thickness_px = bevel * (1.05 * blur_refraction);
    float border_dist = min(min(frag_px.x, blur_viewport_size.x - frag_px.x), min(frag_px.y, blur_viewport_size.y - frag_px.y));
    float2 refr_px = snell_xy * thickness_px * smoothstep(0.0, 16.0 * scale, border_dist);
    float disp_amt = blur_dispersion * (0.14 + 0.36 * rim_slope);
    float2 uv_r = (frag_px + refr_px * (1.0 - disp_amt)) / blur_viewport_size;
    float2 uv_g = (frag_px + refr_px) / blur_viewport_size;
    float2 uv_b = (frag_px + refr_px * (1.0 + disp_amt * 1.25)) / blur_viewport_size;
    uint mode = blur_flags >> 8u;
    float2 local_uv = saturate((frag_px - blur_bounds_origin) / max(blur_bounds_size, float2(1.0, 1.0)));
    float prog = 1.0;
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
    float4 backdrop = glass_backdrop(uv_r, uv_g, uv_b, prog, blurred);
    float a = backdrop.a;
    float3 rgb = a > 0.0 ? backdrop.rgb / max(a, 1e-4) : float3(0.0, 0.0, 0.0);
    float tint_a = blur_tint_rgba.a * (mode == 0u ? 1.0 : 0.35 + 0.65 * prog);
    rgb = lerp(rgb, blur_tint_rgba.rgb, tint_a);
    float luma = dot(rgb, float3(0.299, 0.587, 0.114));
    rgb = lerp(float3(luma, luma, luma), rgb, 1.0 + 0.12 * min(blur_refraction, 1.2));
    float3 v_dir = float3(0.0, 0.0, 1.0);
    float fresnel = pow(1.0 - saturate(normal.z), 2.2);
    float3 key_h = normalize(normalize(float3(-0.55, -0.78, 0.65)) + v_dir);
    float key_spec = pow(max(dot(normal, key_h), 0.0), 18.0);
    float3 bounce_h = normalize(normalize(float3(0.55, 0.78, 0.45)) + v_dir);
    float bounce_spec = pow(max(dot(normal, bounce_h), 0.0), 10.0) * 0.45;
    float rim_line = smoothstep(2.4 * scale, 0.0, depth_in);
    float rim_dir_weight = 0.42 + 0.58 * max(dot(grad, normalize(float2(-0.55, -0.83))), 0.0)
        + 0.34 * max(dot(grad, normalize(float2(0.55, 0.83))), 0.0);
    float inner_caustic = exp(-((u - 0.15) * (u - 0.15)) / (2.0 * 0.09 * 0.09))
        * (0.24 + 0.56 * max(dot(grad, normalize(float2(-0.5, -0.86))), 0.0));
    float ptr_spec = 0.0;
    if (blur_pointer_pos.x >= -1000.0) {
        float2 ptr_delta = blur_pointer_pos - frag_px;
        float ptr_dist = length(ptr_delta);
        float ptr_sigma = 210.0 * scale;
        float ptr_falloff = exp(-(ptr_dist * ptr_dist) / (2.0 * ptr_sigma * ptr_sigma));
        float3 ptr_h = normalize(normalize(float3(ptr_delta, 140.0 * scale)) + v_dir);
        ptr_spec = pow(max(dot(normal, ptr_h), 0.0), 22.0) * ptr_falloff * (0.28 + 0.72 * fresnel + 0.35 * rim_line);
    }
    float spec_energy = (fresnel * 0.25
        + key_spec * (0.24 + 0.44 * fresnel)
        + bounce_spec * (0.14 + 0.28 * fresnel)
        + rim_line * rim_dir_weight * 0.44
        + inner_caustic * 0.18
        + ptr_spec * 0.48)
        * blur_specular;
    float bright_bg = smoothstep(0.48, 0.92, luma);
    float outer_bevel_shade = smoothstep(2.0 * scale, 0.0, depth_in) * (0.07 + 0.22 * bright_bg) * blur_specular;
    float inner_rim_contrast = exp(-((depth_in - 2.6 * scale) * (depth_in - 2.6 * scale)) / (2.0 * (1.8 * scale) * (1.8 * scale)))
        * 0.075 * bright_bg * blur_specular;
    rgb = rgb * (1.0 - outer_bevel_shade - inner_rim_contrast) + float3(0.97, 0.99, 1.04) * spec_energy;
    rgb = saturate(rgb);
    return float4(rgb * a, a);
}

// The glass's drop shadow, outside the shape only.
float blur_shadow_alpha(float2 rel, float2 half_size, float outside) {
    if (blur_shadow_opacity <= 0.0005 || outside <= 0.001) {
        return 0.0;
    }
    float scale = max(blur_scale_factor, 1.0);
    float d_amb = max(blur_rounded_rect_sdf(rel - float2(0.0, 9.0 * scale), half_size, blur_corner_radii) + 3.0 * scale, 0.0);
    float amb = exp(-(d_amb * d_amb) / (2.0 * (12.5 * scale) * (12.5 * scale))) * smoothstep(38.0 * scale, 16.0 * scale, d_amb);
    float d_con = max(blur_rounded_rect_sdf(rel - float2(0.0, 2.5 * scale), half_size, blur_corner_radii) + 0.5 * scale, 0.0);
    float con = exp(-(d_con * d_con) / (2.0 * (3.6 * scale) * (3.6 * scale)));
    return clamp((amb * 0.72 + con * 0.34) * blur_shadow_opacity * outside, 0.0, 0.85);
}

// The snapshot with the blur (or glass) mixed in by coverage, and the
// glass's shadow over it outside the shape; written with blending off.
float4 backdrop_blur_fragment(BlurFullscreenInput input): SV_Target {
    float2 frag_px = input.position.xy;
    bool blurred = (blur_flags & 1u) != 0u;
    bool glass = (blur_flags & 2u) != 0u;
    float2 half_size = max(blur_bounds_size * 0.5, float2(0.5, 0.5));
    float2 rel = frag_px - (blur_bounds_origin + half_size);
    float r = min(blur_pick_corner_radius(rel, blur_corner_radii), min(half_size.x, half_size.y));
    float2 q = abs(rel) - (half_size - float2(r, r));
    float dist = length(max(q, float2(0.0, 0.0))) + min(max(q.x, q.y), 0.0) - r;
    float cov = saturate(0.5 - dist);
    float shadow_a = blur_shadow_alpha(rel, half_size, 1.0 - cov);
    float4 inside = glass ? glass_color(frag_px, rel, half_size, q, dist, blurred) : blurred_at(input.uv);
    float4 backdrop = t_blur_snapshot.Load(int3(int2(frag_px), 0));
    if (cov <= 0.0 && shadow_a <= 0.0) {
        discard;
    }
    float4 result = lerp(backdrop, inside, cov);
    float4 shadow = float4(float3(0.02, 0.04, 0.09) * shadow_a, shadow_a);
    return shadow + result * (1.0 - shadow_a);
}

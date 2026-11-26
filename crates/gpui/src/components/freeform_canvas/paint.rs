//! Low-level GPUI quad & badge painting helpers for [`super::FreeformCanvas`].

use crate::{
    AnyElement, Bounds, FontWeight, Hsla, IntoElement, LineCap, LineJoin, ParentElement,
    PathBuilder, PathStyle, Pixels, Point, StrokeOptions, Styled, Window, div, fill, point, px,
    size,
};

use super::geometry::{
    CANVAS_HANDLE_THICKNESS, CanvasRect, GuideAxis, GuideKind, SnapGuide,
};
use crate::components::theme::tabular_numerals;

const MAX_DOT_GRID_QUADS: f32 = 12_000.0;

pub(super) fn resolve_dot_grid_step(grid_step: f32, width: f32, height: f32) -> f32 {
    let mut step = grid_step.max(4.0);
    if width <= 4.0 || height <= 4.0 {
        return step;
    }
    while (width / step) * (height / step) > MAX_DOT_GRID_QUADS {
        step *= 2.0;
    }
    step
}

pub(super) fn paint_dot_grid(
    bounds: Bounds<Pixels>,
    grid_step: f32,
    color: Hsla,
    window: &mut Window,
) {
    if grid_step < 4.0 {
        return;
    }
    let w = bounds.size.width.0;
    let h = bounds.size.height.0;
    let step = resolve_dot_grid_step(grid_step, w, h);
    let ox = bounds.origin.x.0;
    let oy = bounds.origin.y.0;

    // On fine grids (< 8px), distinguish major 2x intersections from whisper-soft minor sub-dots
    // so dense steps stay readable without creating harsh visual dot-matrix noise on light surfaces.
    let fine = step < 8.0;
    let major_size = if fine { 1.2 } else { 1.25 };
    let minor_size = 1.0;
    let mut major_col = color;
    major_col.a = (color.a * if fine { 0.90 } else { 0.95 }).clamp(0.0, 0.40);
    let mut minor_col = color;
    minor_col.a = (color.a * 0.36).clamp(0.0, 0.20);

    let mut ix: usize = 1;
    loop {
        let x = ix as f32 * step;
        if x >= w - 2.0 {
            break;
        }
        let mut iy: usize = 1;
        loop {
            let y = iy as f32 * step;
            if y >= h - 2.0 {
                break;
            }
            let is_major = !fine || (ix.is_multiple_of(2) && iy.is_multiple_of(2));
            let dot_size = if is_major { major_size } else { minor_size };
            let half = dot_size * 0.5;
            let dot_col = if is_major { major_col } else { minor_col };
            window.paint_quad(fill(
                Bounds::new(
                    point(px(ox + x - half), px(oy + y - half)),
                    size(px(dot_size), px(dot_size)),
                ),
                dot_col,
            ));
            iy += 1;
        }
        ix += 1;
    }
}

pub(super) fn paint_dashed_rect(
    bounds: Bounds<Pixels>,
    rect: CanvasRect,
    color: Hsla,
    window: &mut Window,
) {
    let x0 = bounds.origin.x.0 + rect.x - 1.0;
    let y0 = bounds.origin.y.0 + rect.y - 1.0;
    let w = (rect.width + 2.0).max(2.0);
    let h = (rect.height + 2.0).max(2.0);
    let dash = 4.0_f32;
    let step = 7.0_f32;

    let mut x = 0.0_f32;
    while x < w {
        let len = (w - x).min(dash);
        window.paint_quad(fill(
            Bounds::new(point(px(x0 + x), px(y0)), size(px(len), px(1.0))),
            color,
        ));
        window.paint_quad(fill(
            Bounds::new(
                point(px(x0 + x), px(y0 + h - 1.0)),
                size(px(len), px(1.0)),
            ),
            color,
        ));
        x += step;
    }

    let mut y = 0.0_f32;
    while y < h {
        let len = (h - y).min(dash);
        window.paint_quad(fill(
            Bounds::new(point(px(x0), px(y0 + y)), size(px(1.0), px(len))),
            color,
        ));
        window.paint_quad(fill(
            Bounds::new(
                point(px(x0 + w - 1.0), px(y0 + y)),
                size(px(1.0), px(len)),
            ),
            color,
        ));
        y += step;
    }
}

pub(super) fn paint_marquee_rect(
    clip_bounds: Bounds<Pixels>,
    start: Point<Pixels>,
    current: Point<Pixels>,
    fill_color: Hsla,
    stroke_color: Hsla,
    window: &mut Window,
) {
    let min_x = clip_bounds.origin.x.0;
    let min_y = clip_bounds.origin.y.0;
    let max_x = min_x + clip_bounds.size.width.0;
    let max_y = min_y + clip_bounds.size.height.0;

    let x0 = start.x.0.min(current.x.0).clamp(min_x, max_x);
    let y0 = start.y.0.min(current.y.0).clamp(min_y, max_y);
    let x1 = start.x.0.max(current.x.0).clamp(min_x, max_x);
    let y1 = start.y.0.max(current.y.0).clamp(min_y, max_y);
    let w = (x1 - x0).max(1.0);
    let h = (y1 - y0).max(1.0);

    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0)), size(px(w), px(h))),
        fill_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0)), size(px(w), px(1.0))),
        stroke_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0 + h - 1.0)), size(px(w), px(1.0))),
        stroke_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0), px(y0)), size(px(1.0), px(h))),
        stroke_color,
    ));
    window.paint_quad(fill(
        Bounds::new(point(px(x0 + w - 1.0), px(y0)), size(px(1.0), px(h))),
        stroke_color,
    ));
}

pub(super) fn paint_snap_guides(
    bounds: Bounds<Pixels>,
    guides: &[SnapGuide],
    edge_color: Hsla,
    center_color: Hsla,
    gap_color: Hsla,
    window: &mut Window,
) {
    for g in guides {
        let color = match g.kind {
            GuideKind::SiblingEdge | GuideKind::SiblingCenter => edge_color,
            GuideKind::CanvasCenter | GuideKind::CanvasMargin => center_color,
            GuideKind::SiblingGap => gap_color,
        };
        let quad_bounds = match g.axis {
            GuideAxis::Vertical => Bounds::new(
                point(
                    px(bounds.origin.x.0 + g.position - 0.5),
                    px(bounds.origin.y.0 + g.span_start),
                ),
                size(px(1.0), px((g.span_end - g.span_start).max(1.0))),
            ),
            GuideAxis::Horizontal => Bounds::new(
                point(
                    px(bounds.origin.x.0 + g.span_start),
                    px(bounds.origin.y.0 + g.position - 0.5),
                ),
                size(px((g.span_end - g.span_start).max(1.0)), px(1.0)),
            ),
        };
        window.paint_quad(fill(quad_bounds, color.opacity(0.9)));
    }
}

pub(super) fn build_guide_badges(
    guides: &[SnapGuide],
    cw: f32,
    ch: f32,
    gap_color: Hsla,
) -> Vec<AnyElement> {
    guides
        .iter()
        .filter_map(|g| {
            let label = g.label.clone()?;
            let (bx, by) = match g.axis {
                GuideAxis::Vertical => (
                    (g.position - 14.0).clamp(2.0, (cw - 32.0).max(2.0)),
                    ((g.span_start + g.span_end) * 0.5 - 8.0).clamp(2.0, (ch - 18.0).max(2.0)),
                ),
                GuideAxis::Horizontal => (
                    ((g.span_start + g.span_end) * 0.5 - 14.0).clamp(2.0, (cw - 32.0).max(2.0)),
                    (g.position - 8.0).clamp(2.0, (ch - 18.0).max(2.0)),
                ),
            };
            Some(
                div()
                    .absolute()
                    .left(px(bx))
                    .top(px(by))
                    .px(px(4.))
                    .py(px(1.))
                    .rounded(px(3.))
                    .bg(gap_color)
                    .text_color(crate::white())
                    .text_size(px(9.5))
                    .font_weight(FontWeight::SEMIBOLD)
                    .font_features(tabular_numerals())
                    .child(label)
                    .into_any_element(),
            )
        })
        .collect()
}

/// Paints a rounded-cap 90° SouthEast corner resize grip whose vertical and horizontal arms
/// sit colinearly on the shared East/South rail (`cw + 6.0`, `ch + 6.0` inside the 14×14 hit box).
pub(super) fn paint_se_resize_grip(bounds: Bounds<Pixels>, color: Hsla, window: &mut Window) {
    let ox = bounds.origin.x.0;
    let oy = bounds.origin.y.0;
    // The 14x14 hit box starts at (cw - 3, ch - 3), so the shared rail centerline (cw + 6, ch + 6)
    // sits at local (9.0, 9.0). Curve cleanly around the corner with a 5.5px bend radius and
    // rounded caps matching the East/South pill handles.
    let rail = 9.0_f32;
    let tip = 1.5_f32;
    let bend = 3.5_f32;
    let mut pb = PathBuilder::stroke(px(CANVAS_HANDLE_THICKNESS)).with_style(PathStyle::Stroke(
        StrokeOptions {
            line_width: CANVAS_HANDLE_THICKNESS,
            line_cap: LineCap::Round,
            line_join: LineJoin::Round,
            ..Default::default()
        },
    ));
    pb.move_to(point(px(ox + rail), px(oy + tip)));
    pb.line_to(point(px(ox + rail), px(oy + bend)));
    pb.curve_to(point(px(ox + bend), px(oy + rail)), point(px(ox + rail), px(oy + rail)));
    pb.line_to(point(px(ox + tip), px(oy + rail)));
    if let Ok(path) = pb.build() {
        window.paint_path(path, color);
    }
}

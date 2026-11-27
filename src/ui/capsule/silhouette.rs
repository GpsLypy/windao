use eframe::egui::{self, Color32, Painter, Pos2, Rect, Stroke, pos2};

const ARC_STEPS: usize = 16;

/// Sample the boundary from the top down. A positive shoulder flares outward;
/// a negative value is the rounded top of the compact pill during animation.
pub(crate) fn rows(rect: Rect, shoulder: f32, bottom_radius: f32) -> Vec<(Pos2, Pos2)> {
    let shoulder = shoulder.clamp(-rect.height() / 2.0, rect.height() / 2.0);
    let body_inset = shoulder.max(0.0).min(rect.width() / 4.0);
    let upper = if shoulder > 0.0 {
        shoulder.min(rect.width() / 4.0)
    } else {
        (-shoulder).min(rect.width() / 2.0)
    };
    let lower = bottom_radius
        .min((rect.width() - 2.0 * body_inset) / 2.0)
        .min(rect.height() - upper);
    let mut rows = Vec::with_capacity(2 * (ARC_STEPS + 1));
    let mut row = |y: f32, inset: f32| {
        rows.push((pos2(rect.left() + inset, y), pos2(rect.right() - inset, y)));
    };
    for step in 0..=ARC_STEPS {
        let angle = std::f32::consts::FRAC_PI_2 * step as f32 / ARC_STEPS as f32;
        if shoulder > 0.0 {
            row(
                rect.top() + upper * (1.0 - angle.cos()),
                upper * angle.sin(),
            );
        } else {
            row(
                rect.top() + upper * (1.0 - angle.cos()),
                upper * (1.0 - angle.sin()),
            );
        }
    }
    for step in 0..=ARC_STEPS {
        let angle = std::f32::consts::FRAC_PI_2 * step as f32 / ARC_STEPS as f32;
        row(
            rect.bottom() - lower * (1.0 - angle.sin()),
            body_inset + lower * (1.0 - angle.cos()),
        );
    }
    rows
}

pub(super) fn paint(
    painter: &Painter,
    rect: Rect,
    shoulder: f32,
    radius: f32,
    fill: Color32,
    border: Stroke,
) {
    let rows = rows(rect, shoulder, radius);
    let mut mesh = egui::Mesh::default();
    for (index, &(left, right)) in rows.iter().enumerate() {
        mesh.colored_vertex(left, fill);
        mesh.colored_vertex(right, fill);
        if index > 0 {
            let next = index as u32 * 2;
            mesh.add_triangle(next - 2, next - 1, next);
            mesh.add_triangle(next - 1, next + 1, next);
        }
    }
    painter.add(mesh);
    // Leave the screen-edge attachment seamless.
    painter.add(egui::Shape::line(perimeter(&rows).collect(), border));
}

pub(crate) fn perimeter(rows: &[(Pos2, Pos2)]) -> impl Iterator<Item = Pos2> + '_ {
    rows.iter()
        .map(|&(left, _)| left)
        .chain(rows.iter().rev().map(|&(_, right)| right))
}

use super::Palette;
use chrono::{Local, Timelike};
use eframe::egui::{self, Pos2, Rect, Stroke, Ui, vec2};
use std::time::Duration;

pub(super) fn draw(
    ui: &Ui,
    center: Pos2,
    s: f32,
    playing: bool,
    drinking: bool,
    reduce_motion: bool,
) {
    let tone = Palette::from_ui(ui);
    let time = ui.input(|i| i.time);
    let night = matches!(Local::now().hour(), 0..=5 | 23);
    let singing = playing && !drinking;
    let blink = !reduce_motion && time.rem_euclid(5.0) > 4.78;
    let bob = if singing && !reduce_motion {
        (time as f32 * 3.5).sin() * 1.1
    } else {
        0.0
    };
    let center = center + vec2(0.0, bob * s);
    let at = |x, y| center + vec2(x, y) * s;
    let p = ui.painter();
    let stroke = Stroke::new(1.4 * s, tone.ink);
    // Small standing cat with ears cut into the silhouette.
    p.add(egui::Shape::convex_polygon(
        vec![
            at(-8.0, 1.0),
            at(-7.0, -8.0),
            at(-2.0, -5.0),
            at(2.0, -5.0),
            at(7.0, -8.0),
            at(8.0, 1.0),
            at(6.0, 6.0),
            at(-6.0, 6.0),
        ],
        tone.selected,
        stroke,
    ));
    for x in [-3.2, 3.2] {
        if blink || (night && !singing) {
            p.line_segment([at(x - 1.2, -0.4), at(x + 1.2, -0.4)], stroke);
        } else {
            p.circle_filled(at(x, -0.4), 1.1 * s, tone.ink);
        }
    }
    if drinking {
        p.rect_filled(
            Rect::from_center_size(at(5.0, 5.5), vec2(7.0, 5.0) * s),
            1,
            tone.signal,
        );
    } else if singing {
        p.circle_filled(at(0.0, 2.8), 1.25 * s, tone.signal);
        p.line_segment([at(9.0, -7.0), at(9.0, -11.0)], Stroke::new(s, tone.signal));
        p.circle_filled(at(8.0, -6.8), 1.4 * s, tone.signal);
    } else {
        p.line_segment([at(-0.9, 2.2), at(0.9, 2.2)], Stroke::new(s, tone.ink));
    }
    if !reduce_motion {
        let wait = if singing {
            0.16
        } else if blink {
            0.22
        } else {
            4.8 - time.rem_euclid(5.0)
        };
        ui.ctx()
            .request_repaint_after(Duration::from_secs_f64(wait.max(0.02)));
    }
}

use eframe::egui::{self, Color32, Rect, Response, Stroke, StrokeKind, Ui, vec2};

pub(super) fn focus_outline(ui: &Ui, response: &Response, radius: u8, ink: Color32, scale: f32) {
    if response.has_focus() {
        ui.painter().rect_stroke(
            response.rect.shrink(2.0 * scale),
            radius.saturating_sub(1),
            Stroke::new(scale, ink),
            StrokeKind::Inside,
        );
    }
}

pub(super) fn navigation_background(ui: &Ui, response: &Response, selected: bool, scale: f32) {
    let visuals = ui.visuals();
    let rect = response.rect;
    let radius = (5.0 * scale).round() as u8;
    let ink = visuals.text_color();
    let fill = if selected {
        visuals.selection.bg_fill
    } else if response.is_pointer_button_down_on() {
        ink.gamma_multiply(0.12)
    } else if response.hovered() {
        ink.gamma_multiply(0.07)
    } else {
        Color32::TRANSPARENT
    };
    ui.painter().rect_filled(rect, radius, fill);
    if selected {
        let indicator = Rect::from_center_size(
            egui::pos2(rect.left() + 2.5 * scale, rect.center().y),
            vec2(3.0, 16.0) * scale,
        );
        ui.painter()
            .rect_filled(indicator, 2, visuals.hyperlink_color);
    }
    focus_outline(ui, response, radius, ink, scale);
}

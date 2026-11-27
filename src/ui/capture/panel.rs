use eframe::egui::{
    self, Align2, Button, Color32, FontId, Frame, Margin, Rect, Stroke, StrokeKind, Ui, vec2,
};

#[derive(Clone, Copy)]
pub(super) enum Action {
    Screenshot,
    Region,
    Application,
    Folder,
    Cancel,
}

#[derive(Clone, Copy, Default, PartialEq)]
enum Mode {
    #[default]
    Screenshot,
    Recording,
}

#[derive(Default)]
pub(super) struct Panel {
    mode: Mode,
}

const SCREENSHOT: Tile = Tile {
    action: Action::Screenshot,
    title: "开始截屏",
    detail: "区域、窗口或全屏",
    color: [130, 179, 255],
};
const REGION: Tile = Tile {
    action: Action::Region,
    title: "区域录屏",
    detail: "Windows 截图工具",
    color: [124, 220, 202],
};
const APPLICATION: Tile = Tile {
    action: Action::Application,
    title: "应用录屏",
    detail: "Xbox Game Bar",
    color: [190, 164, 250],
};

impl Panel {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        height: f32,
        scale: f32,
        modern: bool,
        pending: bool,
        error: Option<&str>,
    ) -> Option<Action> {
        if height == 0.0 {
            return None;
        }
        let scale = scale.min((height / 136.0).max(0.8));
        egui::ScrollArea::vertical()
            .id_salt("capture-controls")
            .max_height(height)
            .auto_shrink([false, false])
            .show(ui, |ui| {
                let bottom = ui.cursor().top() + height;
                let width = ui.available_width();
                let radius = (9.0 * scale).round() as u8;
                let gap = 8.0 * scale;
                let footer_height = 28.0 * scale;
                ui.spacing_mut().item_spacing.y = 0.0;
                Frame::new()
                    .fill(ui.visuals().panel_fill)
                    .stroke(Stroke::new(1.0_f32, ui.visuals().window_stroke.color))
                    .corner_radius(radius)
                    .inner_margin(Margin::same((3.0 * scale).round() as i8))
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.x = 2.0 * scale;
                        ui.horizontal(|ui| {
                            let size = vec2((width / 2.0 - gap).min(72.0 * scale), 27.0 * scale);
                            for (mode, title) in
                                [(Mode::Screenshot, "截屏"), (Mode::Recording, "录屏")]
                            {
                                let selected = self.mode == mode;
                                let button = Button::selectable(
                                    selected,
                                    egui::RichText::new(title).size(13.0 * scale),
                                )
                                .frame_when_inactive(selected)
                                .corner_radius(radius.saturating_sub(3))
                                .min_size(size)
                                .small();
                                let response = ui.add_enabled(!pending, button);
                                response.widget_info(|| {
                                    egui::WidgetInfo::selected(
                                        egui::WidgetType::SelectableLabel,
                                        !pending,
                                        selected,
                                        title,
                                    )
                                });
                                if response.clicked() {
                                    self.mode = mode;
                                }
                            }
                        });
                    });

                let tiles: &[Tile] = match (self.mode, modern) {
                    (Mode::Screenshot, _) => &[SCREENSHOT],
                    (Mode::Recording, true) => &[REGION, APPLICATION],
                    (Mode::Recording, false) => &[APPLICATION],
                };
                ui.add_space(6.0 * scale);
                let tile_room = bottom - ui.cursor().top() - footer_height - 8.0 * scale;
                let tile_height = tile_room.clamp(64.0 * scale, 108.0 * scale);
                let mut action = ui
                    .horizontal(|ui| {
                        ui.spacing_mut().item_spacing.x = gap;
                        let tile_width =
                            (width - gap * (tiles.len() - 1) as f32) / tiles.len() as f32;
                        let mut selected = None;
                        for tile in tiles {
                            if tile
                                .show(ui, vec2(tile_width, tile_height), radius, scale, !pending)
                                .clicked()
                            {
                                selected = Some(tile.action);
                            }
                        }
                        selected
                    })
                    .inner;
                if pending {
                    ui.add_space(5.0 * scale);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("请在 3 秒内切换到要录制的应用");
                        if ui.button("取消").clicked() {
                            action = Some(Action::Cancel);
                        }
                    });
                } else if let Some(error) = error {
                    ui.add_space(5.0 * scale);
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }

                ui.add_space(
                    (bottom - ui.cursor().top() - footer_height - 3.0 * scale).max(3.0 * scale),
                );
                ui.add(egui::Separator::default().spacing(3.0 * scale));
                if folder_button(ui, scale).clicked() {
                    action = Some(Action::Folder);
                }
                action
            })
            .inner
    }
}

fn folder_button(ui: &mut Ui, scale: f32) -> egui::Response {
    const TITLE: &str = "录制文件夹";
    let response = ui.add(
        Button::new("")
            .min_size(vec2(94.0, 25.0) * scale)
            .frame_when_inactive(false)
            .small(),
    );
    response.widget_info(|| egui::WidgetInfo::labeled(egui::WidgetType::Button, true, TITLE));
    let ink = if response.hovered() || response.has_focus() {
        ui.visuals().text_color()
    } else {
        ui.visuals().weak_text_color()
    };
    let origin = response.rect.left_center();
    let at = |x, y| origin + vec2(x, y) * scale;
    let stroke = Stroke::new(1.15 * scale, ink);
    let painter = ui.painter();
    painter.add(egui::Shape::closed_line(
        vec![
            at(7., 4.),
            at(7., -4.),
            at(11., -4.),
            at(13., -2.),
            at(20., -2.),
            at(20., 4.),
        ],
        stroke,
    ));
    painter.text(
        at(26.0, 0.0),
        Align2::LEFT_CENTER,
        TITLE,
        FontId::proportional(11.0 * scale),
        ink,
    );
    response.on_hover_text("打开 Game Bar 捕获文件夹")
}

#[derive(Clone, Copy)]
struct Tile {
    action: Action,
    title: &'static str,
    detail: &'static str,
    color: [u8; 3],
}

impl Tile {
    fn show(
        &self,
        ui: &mut Ui,
        size: egui::Vec2,
        radius: u8,
        scale: f32,
        enabled: bool,
    ) -> egui::Response {
        let response = ui.add_enabled(enabled, Button::new("").frame(false).min_size(size));
        response.widget_info(|| {
            egui::WidgetInfo::labeled(egui::WidgetType::Button, enabled, self.title)
        });
        let rect = response.rect;
        let visuals = ui.visuals();
        let surface = visuals.window_fill;
        let text = visuals.text_color();
        let pale = Color32::from_rgb(self.color[0], self.color[1], self.color[2]);
        let tint = if text.r() > surface.r() {
            pale
        } else {
            pale.lerp_to_gamma(text, 0.64)
        };
        let intensity = if response.is_pointer_button_down_on() {
            0.29
        } else if response.hovered() {
            0.23
        } else {
            0.15
        };
        let painter = ui.painter();
        let color = |ink: Color32| {
            if enabled {
                ink
            } else {
                ink.lerp_to_gamma(surface, 0.43)
            }
        };
        painter.rect(
            rect,
            radius,
            color(surface.lerp_to_gamma(tint, intensity)),
            Stroke::new(1.0_f32, color(surface.lerp_to_gamma(tint, 0.46))),
            StrokeKind::Inside,
        );
        crate::ui::controls::focus_outline(ui, &response, radius, tint, scale);
        let center = rect.center();
        self.icon(ui, center - vec2(0.0, 18.0 * scale), color(tint), scale);
        painter.text(
            center + vec2(0.0, 4.0 * scale),
            Align2::CENTER_CENTER,
            self.title,
            FontId::proportional(14.0 * scale),
            color(text),
        );
        let detail = painter.layout_no_wrap(
            self.detail.into(),
            FontId::proportional(11.0 * scale),
            color(visuals.weak_text_color()),
        );
        if detail.size().x + 12.0 * scale <= rect.width() {
            let pos = center + vec2(-detail.size().x / 2.0, 21.0 * scale - detail.size().y / 2.0);
            painter.galley(pos, detail, color(visuals.weak_text_color()));
        }
        response.on_hover_text(self.detail)
    }

    fn icon(&self, ui: &Ui, center: egui::Pos2, ink: Color32, scale: f32) {
        let painter = ui.painter();
        let stroke = Stroke::new(1.6 * scale, ink);
        let at = |x, y| center + vec2(x, y) * scale;
        let line = |points: &[(f32, f32)]| {
            painter.add(egui::Shape::line(
                points.iter().map(|&(x, y)| at(x, y)).collect(),
                stroke,
            ));
        };
        match self.action {
            Action::Screenshot | Action::Region => {
                for x in [-1.0, 1.0] {
                    for y in [-1.0, 1.0] {
                        line(&[(x * 4.0, y * 9.0), (x * 9.0, y * 9.0), (x * 9.0, y * 4.0)]);
                    }
                }
                if matches!(self.action, Action::Region) {
                    painter.circle_filled(center, 3.0 * scale, ink);
                }
            }
            Action::Application => {
                painter.rect_stroke(
                    Rect::from_min_max(at(-11.0, -8.0), at(11.0, 8.0)),
                    3,
                    stroke,
                    StrokeKind::Inside,
                );
                line(&[(-10.0, -3.0), (10.0, -3.0)]);
                painter.circle_filled(at(0.0, 2.0), 2.6 * scale, ink);
            }
            _ => unreachable!("only capture actions have tiles"),
        }
    }
}

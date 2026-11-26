use eframe::egui::{
    self, Align2, Color32, FontId, Frame, Margin, Pos2, Rect, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, vec2,
};

use super::{Page, Workbench, accent};

impl Page {
    fn group(self) -> &'static str {
        match self {
            Self::AiUsage | Self::Today | Self::Calendar | Self::Notifications | Self::Tasks => {
                "工作区"
            }
            Self::Focus | Self::Notes | Self::Dates => "工具",
            #[cfg(windows)]
            Self::Music | Self::Weather => "工具",
            Self::Settings => "偏好",
            #[cfg(windows)]
            _ => "桌面工具",
        }
    }
}

impl Workbench {
    pub(super) fn navigation(&mut self, root: &mut Ui) {
        if self.page == Page::Settings {
            self.settings_navigation(root);
            return;
        }
        if root.available_width() < 805.0 {
            egui::Panel::top("workbench-navigation")
                .frame(
                    Frame::new()
                        .fill(root.visuals().window_fill)
                        .inner_margin(Margin::symmetric(16, 6)),
                )
                .show_inside(root, |ui| {
                    ui.horizontal(|ui| {
                        ui.strong("NotchCalendar");
                        ui.separator();
                        egui::ComboBox::from_id_salt("workbench-pages-menu")
                            .selected_text(self.page.title())
                            .show_ui(ui, |ui| {
                                for &page in Page::ALL {
                                    if ui
                                        .selectable_label(self.page == page, page.title())
                                        .clicked()
                                    {
                                        self.navigate(page);
                                    }
                                }
                            })
                            .response
                            .on_hover_text("切换工作台页面（Ctrl+Tab）");
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                            super::quit_button(ui);
                        });
                    });
                });
            return;
        }
        let surface = if root.visuals().dark_mode {
            Color32::from_rgb(37, 39, 43)
        } else {
            Color32::from_rgb(233, 235, 240)
        };
        egui::Panel::left("workbench-sidebar")
            .exact_size(224.0)
            .resizable(false)
            .frame(
                Frame::new()
                    .fill(surface)
                    .inner_margin(Margin::symmetric(12, 15)),
            )
            .show_inside(root, |ui| {
                ui.horizontal(|ui| {
                    if self.brand_icon.is_none()
                        && let Ok(icon) = eframe::icon_data::from_png_bytes(include_bytes!(
                            "../../assets/icon.png"
                        ))
                    {
                        let image = egui::ColorImage::from_rgba_unmultiplied(
                            [icon.width as usize, icon.height as usize],
                            &icon.rgba,
                        );
                        self.brand_icon = Some(ui.ctx().load_texture(
                            "notchcalendar-icon",
                            image,
                            egui::TextureOptions::LINEAR,
                        ));
                    }
                    if let Some(texture) = &self.brand_icon {
                        ui.image((texture.id(), Vec2::splat(26.0)));
                    }
                    ui.label(egui::RichText::new("NotchCalendar").strong().size(15.0));
                });
                ui.add_space(6.0);
                let query = ui.add(
                    egui::TextEdit::singleline(&mut self.navigation_query)
                        .hint_text("快速入口")
                        .desired_width(f32::INFINITY),
                );
                if ui.input_mut(|i| i.consume_key(egui::Modifiers::CTRL, egui::Key::K)) {
                    query.request_focus();
                }
                let enter = query.lost_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter));
                if (query.has_focus() || query.lost_focus())
                    && ui.input(|i| i.key_pressed(egui::Key::Escape))
                {
                    self.navigation_query.clear();
                }
                query.on_hover_text("搜索工作台页面（Ctrl+K）");
                let filter = self.navigation_query.trim().to_owned();
                let height = (ui.available_height() - 92.0).max(48.0);
                let mut choice = None;
                ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(height)
                    .id_salt("workbench-pages")
                    .show(ui, |ui| {
                        ui.spacing_mut().item_spacing.y = 2.0;
                        let mut previous = "";
                        let mut count = 0;
                        for &page in Page::ALL {
                            if (page == Page::Settings && filter.is_empty())
                                || !page.title().contains(&filter)
                            {
                                continue;
                            }
                            let group = page.group();
                            if previous != group {
                                ui.add_space(8.0);
                                ui.horizontal(|ui| {
                                    ui.add_space(8.0);
                                    ui.weak(egui::RichText::new(group).size(11.0));
                                });
                                previous = group;
                            }
                            if nav_row(ui, page, self.page == page) || (enter && count == 0) {
                                choice = Some(page);
                            }
                            count += 1;
                        }
                        if count == 0 {
                            ui.weak("没有匹配的页面");
                        }
                    });
                ui.separator();
                if nav_row(ui, Page::Settings, self.page == Page::Settings) {
                    choice = Some(Page::Settings);
                }
                super::quit_button(ui);
                if let Some(page) = choice {
                    self.navigate(page);
                    self.navigation_query.clear();
                }
            });
    }
}

fn nav_row(ui: &mut Ui, page: Page, selected: bool) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());

    response.widget_info(|| {
        WidgetInfo::selected(WidgetType::SelectableLabel, true, selected, page.title())
    });
    let visuals = ui.style().interact_selectable(&response, selected);
    if selected || response.hovered() || response.has_focus() {
        ui.painter().rect_filled(rect, 7, visuals.weak_bg_fill);
    }
    if response.has_focus() {
        ui.painter().rect_stroke(
            rect,
            7,
            Stroke::new(1.5_f32, accent(ui.visuals().dark_mode)),
            StrokeKind::Inside,
        );
    }
    if selected {
        let tick = Rect::from_center_size(
            Pos2::new(rect.left() + 1.5, rect.center().y),
            vec2(3.0, 16.0),
        );
        ui.painter()
            .rect_filled(tick, 2, accent(ui.visuals().dark_mode));
    }
    let color = if selected {
        ui.visuals().strong_text_color()
    } else {
        ui.visuals().text_color()
    };
    let center = Pos2::new(rect.left() + 23.0, rect.center().y);
    let (tile, glyph) = tile_colors(page);
    ui.painter()
        .rect_filled(Rect::from_center_size(center, Vec2::splat(22.0)), 5, tile);
    draw_icon(ui, page, center, glyph, 1.0);
    ui.painter().text(
        Pos2::new(rect.left() + 43.0, rect.center().y),
        Align2::LEFT_CENTER,
        page.title(),
        FontId::proportional(13.0),
        color,
    );
    response.clicked()
}

fn tile_colors(page: Page) -> (Color32, Color32) {
    let color = match page {
        #[cfg(windows)]
        Page::Mailbox => Color32::from_rgb(61, 110, 150),
        #[cfg(windows)]
        Page::AiWorkbench | Page::Translation => Color32::from_rgb(53, 104, 129),
        #[cfg(windows)]
        Page::Capture => Color32::from_rgb(146, 72, 91),
        #[cfg(windows)]
        Page::Developer | Page::Authenticator => Color32::from_rgb(65, 109, 89),
        Page::AiUsage => Color32::from_rgb(57, 108, 147),
        Page::Today => Color32::from_rgb(37, 119, 237),
        Page::Calendar => Color32::from_rgb(207, 68, 75),
        Page::Tasks => Color32::from_rgb(192, 102, 16),
        Page::Focus => Color32::from_rgb(102, 87, 190),
        Page::Notes => Color32::from_rgb(246, 196, 50),
        Page::Dates => Color32::from_rgb(168, 71, 105),
        Page::Notifications => Color32::from_rgb(62, 113, 139),
        Page::Settings => Color32::from_rgb(114, 120, 130),
        #[cfg(windows)]
        Page::Music => Color32::from_rgb(211, 69, 120),
        #[cfg(windows)]
        Page::Clipboard => Color32::from_rgb(186, 102, 25),
        #[cfg(windows)]
        Page::Performance => Color32::from_rgb(51, 131, 82),
        #[cfg(windows)]
        Page::Weather | Page::Wireless => Color32::from_rgb(39, 129, 154),
        #[cfg(windows)]
        Page::Plugins => Color32::from_rgb(123, 89, 159),
        #[cfg(windows)]
        Page::Tools | Page::Files => Color32::from_rgb(110, 116, 124),
        #[cfg(windows)]
        Page::Market => Color32::from_rgb(112, 122, 72),
    };
    (
        color,
        if page == Page::Notes {
            Color32::from_gray(48)
        } else {
            Color32::WHITE
        },
    )
}

pub(super) fn draw_icon(ui: &Ui, page: Page, center: Pos2, color: Color32, scale: f32) {
    let p = ui.painter();
    let stroke = Stroke::new(1.45 * scale, color);
    let at = |x, y| center + vec2(x, y) * scale;
    let line = |a: (f32, f32), b: (f32, f32)| {
        p.line_segment([at(a.0, a.1), at(b.0, b.1)], stroke);
    };
    let rect = |x, y, w, h| {
        p.rect_stroke(
            Rect::from_min_size(at(x, y), vec2(w, h) * scale),
            2,
            stroke,
            StrokeKind::Inside,
        );
    };
    let ring = |x, y, r| {
        p.circle_stroke(at(x, y), r * scale, stroke);
    };
    match page {
        #[cfg(windows)]
        Page::AiWorkbench => {
            rect(-8., -6., 16., 10.);
            line((-4., 4.), (-5., 7.));
            line((-5., 7.), (0., 4.));
            for x in [-4., 0., 4.] {
                p.circle_filled(at(x, -1.), scale, color);
            }
        }
        #[cfg(windows)]
        Page::Translation => {
            line((-8., -5.), (1., -5.));
            line((-4., -7.), (-4., -5.));
            line((-1., -5.), (-7., 2.));
            line((-7., -3.), (-2., 1.));
            line((0., 7.), (4., -3.));
            line((4., -3.), (8., 7.));
            line((2., 3.), (6., 3.));
        }
        #[cfg(windows)]
        Page::Capture => {
            rect(-8., -6., 16., 12.);
            ring(0., 0., 3.);
        }
        #[cfg(windows)]
        Page::Developer => {
            line((-4., -4.), (-8., 0.));
            line((-8., 0.), (-4., 4.));
            line((4., -4.), (8., 0.));
            line((8., 0.), (4., 4.));
            line((2., -6.), (-2., 6.));
        }
        #[cfg(windows)]
        Page::Authenticator => {
            ring(0., -3., 4.);
            rect(-6., -1., 12., 8.);
            line((0., 2.), (0., 4.));
        }
        Page::AiUsage => {
            line((-7., 5.), (-7., 0.));
            line((-2., 5.), (-2., -5.));
            line((3., 5.), (3., -2.));
            line((7., 5.), (7., -7.));
        }
        Page::Today => {
            ring(0., 0., 4.);
            for (x, y) in [
                (0., 1.),
                (1., 0.),
                (0., -1.),
                (-1., 0.),
                (0.71, 0.71),
                (-0.71, 0.71),
                (0.71, -0.71),
                (-0.71, -0.71),
            ] {
                line((x * 6., y * 6.), (x * 8., y * 8.));
            }
        }
        Page::Calendar => {
            rect(-7., -6., 14., 13.);
            line((-6., -2.), (6., -2.));
            line((-3., -8.), (-3., -5.));
            line((3., -8.), (3., -5.));
        }
        Page::Tasks => {
            rect(-7., -7., 14., 14.);
            line((-4., 0.), (-1., 3.));
            line((-1., 3.), (4., -3.));
        }
        #[cfg(windows)]
        Page::Mailbox => {
            rect(-8., -5., 16., 11.);
            line((-7., -4.), (0., 1.));
            line((0., 1.), (7., -4.));
        }
        Page::Notifications => {
            ring(0., -1., 5.);
            line((-6., 4.), (6., 4.));
            line((-1.5, 7.), (1.5, 7.));
        }
        Page::Focus => {
            ring(0., 1., 6.);
            line((-2., -8.), (2., -8.));
            line((0., 1.), (3., -2.));
        }
        Page::Notes => {
            rect(-6., -7., 12., 14.);
            line((-3., -2.), (3., -2.));
            line((-3., 2.), (1., 2.));
        }
        Page::Dates => {
            rect(-6., -7., 12., 14.);
            line((-3., -4.), (3., -4.));
            ring(0., 2., 2.);
        }
        Page::Settings => {
            for (y, x) in [(-4., -2.), (4., 3.)] {
                line((-7., y), (x - 2., y));
                ring(x, y, 2.);
                line((x + 2., y), (7., y));
            }
        }
        #[cfg(windows)]
        Page::Music => {
            line((-2., 4.), (-2., -5.));
            line((-2., -5.), (6., -7.));
            line((6., -7.), (6., 2.));
            ring(-4., 5., 2.);
            ring(4., 3., 2.);
        }
        #[cfg(windows)]
        Page::Weather => {
            ring(-2., -2., 4.);
            line((-7., 4.), (6., 4.));
            line((-4., 7.), (3., 7.));
        }
        #[cfg(windows)]
        Page::Files => {
            line((-7., -4.), (-3., -4.));
            line((-3., -4.), (-1., -2.));
            line((-1., -2.), (7., -2.));
            line((7., -2.), (7., 6.));
            line((7., 6.), (-7., 6.));
            line((-7., 6.), (-7., -4.));
        }
        #[cfg(windows)]
        Page::Market => {
            line((-7., -7.), (-7., 6.));
            line((-7., 6.), (7., 6.));
            line((-5., 2.), (-1., -2.));
            line((-1., -2.), (2., 0.));
            line((2., 0.), (7., -5.));
        }
        #[cfg(windows)]
        Page::Tools => {
            rect(-7., -4., 14., 11.);
            rect(-3., -7., 6., 4.);
            line((-6., 0.), (6., 0.));
        }
        #[cfg(windows)]
        Page::Performance => {
            line((-7., 1.), (-4., 1.));
            line((-4., 1.), (-1., -6.));
            line((-1., -6.), (2., 6.));
            line((2., 6.), (5., 0.));
            line((5., 0.), (8., 0.));
        }
        #[cfg(windows)]
        Page::Clipboard => {
            rect(-6., -6., 12., 13.);
            rect(-3., -8., 6., 4.);
        }
        #[cfg(windows)]
        Page::Wireless => {
            line((0., -7.), (0., 7.));
            line((0., -7.), (5., -3.));
            line((5., -3.), (-4., 4.));
            line((-4., -4.), (5., 3.));
            line((5., 3.), (0., 7.));
        }
        #[cfg(windows)]
        Page::Plugins => {
            for (x, y) in [(-7., -7.), (1., -7.), (-7., 1.), (1., 1.)] {
                rect(x, y, 6., 6.);
            }
        }
    }
}

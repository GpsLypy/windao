use eframe::egui::{
    self, Align2, Color32, FontId, Frame, Margin, Pos2, Rect, ScrollArea, Sense, Stroke,
    StrokeKind, Ui, Vec2, WidgetInfo, WidgetType, vec2,
};

use super::{Page, Workbench, controls};

impl Page {
    const GROUPS: &'static [&'static str] = &[
        "工作区",
        "工具",
        #[cfg(windows)]
        "桌面工具",
        "偏好",
    ];

    fn group(self) -> &'static str {
        match self {
            Self::AiUsage | Self::Today | Self::Calendar | Self::Notifications | Self::Tasks => {
                "工作区"
            }
            Self::Focus | Self::Notes | Self::Dates | Self::Calculator | Self::News => "工具",
            #[cfg(windows)]
            Self::Music | Self::Weather => "工具",
            #[cfg(all(test, not(windows)))]
            Self::Capture => "工具",
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
        let surface = root.visuals().panel_fill;
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
                        let mut count = 0;
                        for &group in Page::GROUPS {
                            let mut pages = Page::ALL
                                .iter()
                                .copied()
                                .filter(|page| {
                                    page.group() == group
                                        && (*page != Page::Settings || !filter.is_empty())
                                        && page.title().contains(&filter)
                                })
                                .peekable();
                            if pages.peek().is_none() {
                                continue;
                            }
                            ui.add_space(8.0);
                            ui.horizontal(|ui| {
                                ui.add_space(12.0);
                                ui.weak(egui::RichText::new(group).size(12.0));
                            });
                            for page in pages {
                                if nav_row(ui, page, self.page == page) || (enter && count == 0) {
                                    choice = Some(page);
                                }
                                count += 1;
                            }
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
    navigation_row(ui, page.title(), selected, Some(page))
}

pub(super) fn navigation_row(ui: &mut Ui, label: &str, selected: bool, icon: Option<Page>) -> bool {
    let (rect, response) = ui.allocate_exact_size(vec2(ui.available_width(), 34.0), Sense::click());
    response.widget_info(|| {
        WidgetInfo::selected(
            WidgetType::SelectableLabel,
            ui.is_enabled(),
            selected,
            label,
        )
    });
    controls::navigation_background(ui, &response, selected, 1.0);
    let ink = ui.visuals().text_color();
    let x = if let Some(page) = icon {
        let glyph = if selected {
            ui.visuals().hyperlink_color
        } else {
            ink
        };
        draw_icon(
            ui,
            page,
            Pos2::new(rect.left() + 21.0, rect.center().y),
            glyph,
            1.0,
        );
        rect.left() + 40.0
    } else {
        rect.left() + 13.0
    };
    ui.painter().text(
        Pos2::new(x, rect.center().y),
        Align2::LEFT_CENTER,
        label,
        FontId::proportional(14.0),
        ink,
    );
    response.clicked()
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
        Page::Calculator => {
            rect(-7., -8., 14., 16.);
            line((-4., -3.), (4., -3.));
            for x in [-3., 3.] {
                for y in [1., 5.] {
                    p.circle_filled(at(x, y), scale, color);
                }
            }
        }
        Page::News => {
            rect(-8., -7., 16., 14.);
            line((-5., -3.), (5., -3.));
            line((-5., 0.), (5., 0.));
            line((-5., 3.), (2., 3.));
        }
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
        #[cfg(any(windows, test))]
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

use chrono::{Datelike, Local, NaiveDate, Timelike};
use eframe::egui::{
    self, Align, Align2, Button, FontId, Layout, RichText, Sense, Stroke, StrokeKind, Ui, vec2,
};

use super::{Page, Workbench, accent, calendar, card, focus, primary, subhead};
use crate::workspace::{Store, TaskStatus};

impl Workbench {
    pub(super) fn today(&mut self, ui: &mut Ui, store: &mut Store) {
        let now = Local::now();
        let today = now.date_naive();
        let highlight = accent(ui.visuals().dark_mode);
        ui.horizontal(|ui| {
            let (badge, response) = ui.allocate_exact_size(vec2(50.0, 54.0), Sense::click());
            let painter = ui.painter();
            painter.rect_filled(badge, 10, highlight.gamma_multiply(0.09));
            if response.has_focus() || response.hovered() {
                painter.rect_stroke(
                    badge,
                    10,
                    Stroke::new(1.0_f32, highlight),
                    StrokeKind::Inside,
                );
            }
            painter.text(
                badge.center() - vec2(0.0, 16.0),
                Align2::CENTER_CENTER,
                format!("{}月", today.month()),
                FontId::proportional(11.0),
                highlight,
            );
            painter.text(
                badge.center() + vec2(0.0, 7.0),
                Align2::CENTER_CENTER,
                today.day(),
                FontId::proportional(25.0),
                highlight,
            );
            response.widget_info(|| {
                egui::WidgetInfo::labeled(egui::WidgetType::Button, true, "打开今天的日历")
            });
            if response.on_hover_text("打开今天的日历").clicked() {
                self.calendar.select_date(today);
                self.navigate(Page::Calendar);
            }
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 2.0;
                ui.label(RichText::new("今天").size(27.0).strong());
                ui.weak(format!(
                    "{}月{}日  {}",
                    today.month(),
                    today.day(),
                    calendar::weekday(today)
                ));
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let format = if store.state().settings.dock.clock_24h {
                    "%H:%M"
                } else {
                    "%I:%M %p"
                };
                ui.weak(
                    RichText::new(now.format(format).to_string())
                        .monospace()
                        .size(13.0),
                );
                if ui.available_width() > 200.0
                    && let Some(lunar) = calendar::lunar_caption(today)
                {
                    ui.weak(lunar);
                }
            });
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_secs(60 - u64::from(now.second())));
        ui.add_space(12.0);
        ui.separator();
        ui.add_space(12.0);

        let timestamp = now.timestamp_millis();
        if let Some(event) = store
            .state()
            .events
            .iter()
            .filter(|e| !e.all_day && calendar::on_date(e, today) && e.end_at > timestamp)
            .min_by_key(|e| e.start_at)
        {
            let color = accent(ui.visuals().dark_mode);
            egui::Frame::new()
                .fill(color.gamma_multiply(0.08))
                .stroke(egui::Stroke::new(0.75_f32, color.gamma_multiply(0.28)))
                .corner_radius(14)
                .inner_margin(20)
                .show(ui, |ui| {
                    ui.set_min_width(ui.available_width());
                    ui.colored_label(
                        color,
                        if event.start_at <= timestamp {
                            "进行中"
                        } else {
                            "接下来"
                        },
                    );
                    ui.label(RichText::new(&event.title).size(20.0).strong());
                    ui.weak(calendar::event_time(event));
                });
            ui.add_space(8.0);
        }
        if ui.available_width() >= 680.0 {
            ui.columns(2, |columns| {
                self.agenda_today(&mut columns[0], store, today);
                self.focus_today(&mut columns[1], store, timestamp);
            });
        } else {
            self.agenda_today(ui, store, today);
            self.focus_today(ui, store, timestamp);
        }
        ui.add_space(6.0);
        self.tasks_today(ui, store, today, timestamp);
    }

    fn focus_today(&mut self, ui: &mut Ui, store: &mut Store, timestamp: i64) {
        card(ui, |ui| {
            if section_header(ui, "专注计时", "打开专注") {
                self.navigate(Page::Focus);
            }
            let timer = &store.state().focus;
            let running = timer.target_at.is_some();
            if running {
                ui.ctx()
                    .request_repaint_after(std::time::Duration::from_secs(1));
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(focus::digits(timer.seconds_left(timestamp)))
                        .monospace()
                        .size(32.0),
                );
                if timer.label.is_empty() {
                    ui.weak(focus::status(timer));
                } else {
                    ui.label(&timer.label);
                }
            });
            let label = if running {
                "暂停计时"
            } else if timer.session_id.is_some() {
                "继续计时"
            } else {
                "开始计时"
            };
            if primary(ui, label).clicked() {
                let result = store.update(|state| {
                    if running {
                        state.focus.pause(timestamp);
                    } else {
                        state.focus.start(timestamp);
                    }
                    Ok(())
                });
                if let Err(message) = result {
                    self.notice = Some(Err(message));
                }
            }
        });
    }

    fn agenda_today(&mut self, ui: &mut Ui, store: &Store, today: NaiveDate) {
        card(ui, |ui| {
            if section_header(ui, "今日日程", "打开日历") {
                self.calendar.select_date(today);
                self.navigate(Page::Calendar);
            }
            let mut events: Vec<_> = store
                .state()
                .events
                .iter()
                .filter(|event| calendar::on_date(event, today))
                .collect();
            events.sort_by_key(|event| (!event.all_day, event.start_at));
            if events.is_empty() {
                ui.add_space(9.0);
                ui.label(RichText::new("今天没有日程").strong());
                ui.add_space(8.0);
            }
            for event in events.into_iter().take(4) {
                ui.label(
                    RichText::new(calendar::event_time(event))
                        .small()
                        .color(accent(ui.visuals().dark_mode)),
                );
                ui.add(egui::Label::new(&event.title).truncate())
                    .on_hover_text(&event.title);
            }
            if ui.button("添加日程").clicked() {
                self.calendar.new_event(today);
                self.navigate(Page::Calendar);
            }
        });
    }

    fn tasks_today(&mut self, ui: &mut Ui, store: &mut Store, today: NaiveDate, now: i64) {
        card(ui, |ui| {
            if section_header(ui, "待处理", "打开待办") {
                self.navigate(Page::Tasks);
            }
            let mut tasks: Vec<_> = store
                .state()
                .tasks
                .iter()
                .filter(|task| task.is_actionable_on(today))
                .collect();
            tasks.sort_by_key(|task| task.due_at_on(today).unwrap_or(i64::MAX));
            if tasks.is_empty() {
                ui.weak("暂无待处理待办");
            }
            let mut completed = None;
            for task in tasks.into_iter().take(4) {
                let mut checked = false;
                if ui
                    .push_id(task.id, |ui| ui.checkbox(&mut checked, &task.title))
                    .inner
                    .changed()
                {
                    completed = Some(task.id);
                }
            }
            if let Some(id) = completed {
                let result = store.update(|state| {
                    if let Some(task) = state.tasks.iter_mut().find(|task| task.id == id) {
                        task.set_status(TaskStatus::Completed, now);
                    }
                    Ok(())
                });
                if let Err(message) = result {
                    self.notice = Some(Err(message));
                }
            }
            if ui.button("添加待办").clicked() {
                self.tasks.new_task();
                self.navigate(Page::Tasks);
            }
        });
    }
}

fn section_header(ui: &mut Ui, title: &str, link: &str) -> bool {
    ui.horizontal(|ui| {
        subhead(ui, title);
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.add(
                Button::new(
                    RichText::new(link)
                        .small()
                        .color(accent(ui.visuals().dark_mode)),
                )
                .frame(false),
            )
            .clicked()
        })
        .inner
    })
    .inner
}

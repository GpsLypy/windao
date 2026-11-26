use chrono::{Datelike, Local, NaiveDate, Timelike};
use eframe::egui::{
    self, Align, Align2, Button, FontId, Layout, RichText, Sense, Stroke, StrokeKind, Ui, vec2,
};

use super::{Page, Workbench, accent, calendar, card, focus, primary, subhead};
use crate::workspace::{FocusKind, Store, TaskStatus};

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
                self.planning_today(&mut columns[1], store, today, timestamp);
            });
        } else {
            self.agenda_today(ui, store, today);
            self.planning_today(ui, store, today, timestamp);
        }
        ui.add_space(6.0);
        if ui.available_width() >= 680.0 {
            ui.columns(2, |columns| {
                self.focus_today(&mut columns[0], store, timestamp);
                self.rhythm_today(&mut columns[1], store, today);
            });
        } else {
            self.focus_today(ui, store, timestamp);
            self.rhythm_today(ui, store, today);
        }
        ui.add_space(6.0);
        self.tasks_today(ui, store, today, timestamp);
    }

    fn planning_today(&mut self, ui: &mut Ui, store: &Store, date: NaiveDate, now: i64) {
        let local_hour = |hour| {
            date.and_hms_opt(hour, 0, 0)?
                .and_local_timezone(Local)
                .earliest()
                .map(|dt| dt.timestamp_millis())
        };
        let (Some(start), Some(end)) = (local_hour(9), local_hour(18)) else {
            return;
        };
        let openings = free_intervals(
            start,
            end,
            store
                .state()
                .events
                .iter()
                .filter(|event| !event.all_day)
                .map(|event| (event.start_at, event.end_at)),
        );
        let remaining: Vec<_> = openings
            .iter()
            .map(|&(left, right)| (left.max(now), right))
            .filter(|&(left, right)| left < right)
            .collect();
        let minutes = remaining
            .iter()
            .map(|&(left, right)| right - left)
            .sum::<i64>()
            / 60_000;
        card(ui, |ui| {
            if section_header(ui, "可用时间", "打开日历") {
                self.calendar.select_date(date);
                self.navigate(Page::Calendar);
            }
            ui.horizontal_wrapped(|ui| {
                ui.label(
                    RichText::new(format!("还可安排 {minutes} 分钟"))
                        .size(19.0)
                        .strong(),
                );
                ui.weak("09:00–18:00");
            });
            let (track, _) =
                ui.allocate_exact_size(vec2(ui.available_width(), 9.0), Sense::hover());
            let highlight = accent(ui.visuals().dark_mode);
            ui.painter().rect_filled(
                track,
                3,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            );
            let x = |time: i64| {
                track.left() + track.width() * (time - start) as f32 / (end - start) as f32
            };
            for &(left, right) in &remaining {
                let rect = egui::Rect::from_min_max(
                    egui::pos2(x(left), track.top()),
                    egui::pos2(x(right), track.bottom()),
                );
                ui.painter()
                    .rect_filled(rect, 2, highlight.gamma_multiply(0.6));
            }
            if (start..end).contains(&now) {
                ui.painter().line_segment(
                    [
                        egui::pos2(x(now), track.top() - 2.0),
                        egui::pos2(x(now), track.bottom() + 2.0),
                    ],
                    Stroke::new(2.0_f32, ui.visuals().strong_text_color()),
                );
            }
            match remaining.first() {
                Some(&(left, right)) => {
                    let fmt = if store.state().settings.dock.clock_24h {
                        "%H:%M"
                    } else {
                        "%I:%M %p"
                    };
                    let range = |t| super::display_time(t, fmt);
                    ui.label(format!(
                        "{}  {}–{}",
                        if left == now {
                            "现在有空"
                        } else {
                            "下一段空档"
                        },
                        range(left),
                        range(right)
                    ));
                }
                None if now >= end => {
                    ui.weak("今天的规划时段已结束");
                }
                None => {
                    ui.weak("今天的规划时段已排满");
                }
            }
            ui.weak("日程前后各留 5 分钟，全天事项不占用空档。");
        });
    }

    fn rhythm_today(&mut self, ui: &mut Ui, store: &Store, today: NaiveDate) {
        let state = store.state();
        let today_timestamp =
            |at| super::local_timestamp(at).is_some_and(|dt| dt.date_naive() == today);
        let minutes: u64 = state
            .focus
            .history
            .iter()
            .filter(|entry| entry.kind == FocusKind::Focus && today_timestamp(entry.completed_at))
            .map(|entry| u64::from(entry.minutes))
            .sum();
        let completed = state
            .tasks
            .iter()
            .filter(|task| {
                if task.repeat.is_repeating() {
                    task.completed_dates.binary_search(&today).is_ok()
                } else {
                    task.status == TaskStatus::Completed
                        && task.completed_at.is_some_and(today_timestamp)
                }
            })
            .count();
        card(ui, |ui| {
            if section_header(ui, "今日节奏", "查看专注记录") {
                self.navigate(Page::Focus);
            }
            ui.horizontal_wrapped(|ui| {
                ui.vertical(|ui| {
                    ui.weak("已专注");
                    ui.label(RichText::new(format!("{minutes} 分钟")).size(19.0).strong());
                });
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    ui.weak("已完成待办");
                    ui.label(RichText::new(format!("{completed} 项")).size(19.0).strong());
                });
            });
            if ui.button("打开便笺").clicked() {
                self.navigate(Page::Notes);
            }
        });
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

// A timed event blocks its interval plus the same five-minute margin on either side.
fn free_intervals(
    start: i64,
    end: i64,
    events: impl Iterator<Item = (i64, i64)>,
) -> Vec<(i64, i64)> {
    let mut busy: Vec<_> = events
        .filter(|&(a, b)| b > a)
        .map(|(a, b)| {
            (
                start.max(a.saturating_sub(300_000)),
                end.min(b.saturating_add(300_000)),
            )
        })
        .filter(|&(a, b)| a < b)
        .collect();
    busy.sort_unstable();
    let mut cursor = start;
    let mut free = Vec::new();
    for (a, b) in busy {
        if cursor < a {
            free.push((cursor, a));
        }
        cursor = cursor.max(b);
    }
    if cursor < end {
        free.push((cursor, end));
    }
    free
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

#[cfg(test)]
mod tests {
    use super::free_intervals;

    #[test]
    fn planning_clips_cross_midnight_events_and_merges_overlapping_buffers() {
        let m = 60_000;
        let events = [
            (-30 * m, 15 * m),
            (25 * m, 35 * m),
            (36 * m, 40 * m),
            (85 * m, 130 * m),
        ];
        assert_eq!(
            free_intervals(0, 100 * m, events.into_iter()),
            vec![(45 * m, 80 * m)]
        );
        assert_eq!(
            free_intervals(0, 100 * m, std::iter::empty()),
            vec![(0, 100 * m)]
        );
    }
}

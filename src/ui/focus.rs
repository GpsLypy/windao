use std::{
    f32::consts::{FRAC_PI_2, TAU},
    time::Duration,
};

use chrono::Local;
use eframe::egui::{self, FontId, RichText, Stroke, Ui, vec2};
use uuid::Uuid;

use super::{card, display_time, field, primary, subhead, title};
use crate::workspace::{Focus, FocusKind, HEALTH_INTERVAL, HealthKind, Store};

#[derive(Default)]
pub(super) struct Timer {
    draft: Option<FocusDraft>,
    reset_confirmation: Option<Uuid>,
}

struct FocusDraft {
    minutes: u32,
    label: String,
    kind: FocusKind,
}

pub(super) fn status(focus: &Focus) -> &'static str {
    match (
        focus.session_id.is_some(),
        focus.target_at.is_some(),
        focus.kind,
    ) {
        (_, true, FocusKind::Focus) => "专注中",
        (_, true, FocusKind::Break) => "休息中",
        (true, false, _) => "已暂停",
        _ => "准备就绪",
    }
}

pub(super) fn digits(seconds: u32) -> String {
    format!("{:02}:{:02}", seconds / 60, seconds % 60)
}

impl Timer {
    pub(super) fn show(
        &mut self,
        ui: &mut Ui,
        store: &mut Store,
        notice: &mut Option<Result<String, String>>,
    ) {
        title(ui, "专注");
        let now = Local::now();
        let focus = &store.state().focus;
        let seconds = focus.seconds_left(now.timestamp_millis());
        let timer_running = focus.target_at.is_some();
        let session = focus.session_id;
        let configured = self.draft.get_or_insert_with(|| FocusDraft {
            minutes: focus.duration_minutes,
            label: focus.label.clone(),
            kind: focus.kind,
        });
        if timer_running {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        if self.reset_confirmation != session {
            self.reset_confirmation = None;
        }
        let mut action = None;
        let duration = if session.is_some() {
            focus.duration_minutes
        } else {
            configured.minutes
        };
        let remaining = if session.is_some() {
            seconds
        } else {
            duration * 60
        };
        let kind = if session.is_some() {
            focus.kind
        } else {
            configured.kind
        };
        let tint = session_color(ui.visuals().dark_mode, kind, duration);
        let mut controls = |ui: &mut Ui| {
            if session.is_none() {
                ui.horizontal_wrapped(|ui| {
                    for (minutes, kind, text) in [
                        (45, FocusKind::Focus, "深度工作 45 分钟"),
                        (25, FocusKind::Focus, "专注 25 分钟"),
                        (5, FocusKind::Break, "休息 5 分钟"),
                    ] {
                        if ui
                            .selectable_label(
                                configured.minutes == minutes && configured.kind == kind,
                                RichText::new(text).color(session_color(
                                    ui.visuals().dark_mode,
                                    kind,
                                    minutes,
                                )),
                            )
                            .clicked()
                        {
                            configured.minutes = minutes;
                            configured.kind = kind;
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(
                        egui::DragValue::new(&mut configured.minutes)
                            .range(5..=180)
                            .suffix(" 分钟"),
                    );
                    ui.selectable_value(&mut configured.kind, FocusKind::Focus, "专注");
                    ui.selectable_value(&mut configured.kind, FocusKind::Break, "休息");
                });
                if configured.kind == FocusKind::Focus {
                    field(ui, "专注目标", &mut configured.label, "可选", 240);
                }
                let fill = tint;
                let ink = if ui.visuals().dark_mode {
                    egui::Color32::BLACK
                } else {
                    egui::Color32::WHITE
                };
                if ui
                    .add_sized(
                        vec2(ui.available_width(), 29.0),
                        egui::Button::new(RichText::new("▶  开始").color(ink))
                            .fill(fill)
                            .corner_radius(15),
                    )
                    .clicked()
                {
                    action = Some(Action::Start);
                }
            } else {
                ui.horizontal_wrapped(|ui| {
                    if timer_running {
                        if primary(ui, "暂停").clicked() {
                            action = Some(Action::Pause);
                        }
                    } else if primary(ui, "继续").clicked() {
                        action = Some(Action::Resume);
                    }
                    if self.reset_confirmation == session {
                        if ui.button("确认重置").clicked() {
                            action = Some(Action::Reset);
                        }
                        if ui.button("取消").clicked() {
                            self.reset_confirmation = None;
                        }
                    } else if ui.button("重置").clicked() {
                        self.reset_confirmation = session;
                    }
                });
            }
        };
        let label = status(focus);
        if ui.available_width() >= 385.0 {
            ui.horizontal_top(|ui| {
                dial(ui, remaining, duration, label, tint, 146.0);
                ui.add_space(12.0);
                ui.vertical(|ui| {
                    if session.is_some() && !focus.label.is_empty() {
                        subhead(ui, &focus.label);
                    }
                    controls(ui);
                });
            });
        } else {
            ui.vertical_centered(|ui| {
                dial(ui, remaining, duration, label, tint, 146.0);
            });
            controls(ui);
        }
        if let Some(action) = action {
            let now = now.timestamp_millis();
            let result = store.update(|state| {
                match action {
                    Action::Start => {
                        let label = if configured.kind == FocusKind::Focus {
                            configured.label.clone()
                        } else {
                            String::new()
                        };
                        state
                            .focus
                            .configure(configured.minutes, label, configured.kind)?;
                        state.focus.start(now);
                    }
                    Action::Pause => state.focus.pause(now),
                    Action::Resume => state.focus.start(now),
                    Action::Reset => state.focus.reset(),
                }
                Ok(())
            });
            if let Err(error) = result {
                *notice = Some(Err(error));
            }
        }

        ui.add_space(12.0);
        health_reminders(ui, store, notice, now);
        ui.add_space(12.0);
        card(ui, |ui| {
            subhead(ui, "专注记录");
            let history = &store.state().focus.history;
            let today = now.date_naive();
            let minutes: u32 = history
                .iter()
                .filter(|item| {
                    item.kind == FocusKind::Focus
                        && super::local_timestamp(item.completed_at)
                            .is_some_and(|time| time.date_naive() == today)
                })
                .map(|item| item.minutes)
                .sum();
            ui.label(RichText::new(format!("今日专注 {minutes} 分钟")).strong());
            if history.is_empty() {
                ui.weak("还没有完成的计时");
            }
            for item in history.iter().rev().take(40) {
                ui.horizontal_wrapped(|ui| {
                    ui.weak(display_time(item.completed_at, "%m-%d %H:%M"));
                    let label = if item.label.is_empty() {
                        if item.kind == FocusKind::Break {
                            "休息"
                        } else {
                            "专注"
                        }
                    } else {
                        &item.label
                    };
                    ui.label(format!("{label} · {} 分钟", item.minutes));
                });
            }
        });
    }
}

fn health_reminders(
    ui: &mut Ui,
    store: &mut Store,
    notice: &mut Option<Result<String, String>>,
    now: chrono::DateTime<Local>,
) {
    let today = now.date_naive();
    card(ui, |ui| {
        subhead(ui, "饮水与久坐");
        if !store.state().settings.reminders {
            ui.weak("桌面提醒已在设置中关闭。");
        }
        for kind in [HealthKind::Water, HealthKind::Stand] {
            let mut timer = store.state().health.timer(kind);
            let enabled = timer.next_at.is_some();
            let (interval_changed, toggle) = ui
                .push_id(kind.label(), |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(kind.label());
                        let changed = ui
                            .add(
                                egui::DragValue::new(&mut timer.minutes)
                                    .range(HEALTH_INTERVAL)
                                    .suffix(" 分钟"),
                            )
                            .changed();
                        let toggle = ui.button(if enabled { "停止" } else { "开启" }).clicked();
                        if let Some(next) = timer.next_at {
                            ui.weak(format!("下次提醒 {}", display_time(next, "%m-%d %H:%M")));
                        }
                        (changed, toggle)
                    })
                    .inner
                })
                .inner;
            if toggle || interval_changed {
                if enabled && toggle {
                    timer.next_at = None;
                } else if enabled || toggle {
                    timer.schedule(now.timestamp_millis());
                }
                if let Err(error) = store.update(|state| {
                    *state.health.timer_mut(kind) = timer;
                    Ok(())
                }) {
                    *notice = Some(Err(error));
                }
            }
        }
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!(
                    "今日饮水 {} 次",
                    store.state().health.water_count(today)
                ))
                .strong(),
            );
            if ui.button("记录一次").clicked()
                && let Err(error) = store.update(|state| {
                    state.health.record_water(today);
                    Ok(())
                })
            {
                *notice = Some(Err(error));
            }
        });
    });
}

enum Action {
    Start,
    Pause,
    Resume,
    Reset,
}

pub(super) fn session_color(dark: bool, kind: FocusKind, minutes: u32) -> egui::Color32 {
    let rgb = match (dark, kind, minutes >= 45) {
        (true, FocusKind::Break, _) => [155, 214, 186],
        (false, FocusKind::Break, _) => [40, 116, 81],
        (true, FocusKind::Focus, true) => [237, 168, 115],
        (false, FocusKind::Focus, true) => [159, 79, 30],
        (true, FocusKind::Focus, false) => [145, 181, 239],
        (false, FocusKind::Focus, false) => [55, 103, 175],
    };
    egui::Color32::from_rgb(rgb[0], rgb[1], rgb[2])
}

pub(super) fn dial(
    ui: &mut Ui,
    seconds: u32,
    duration: u32,
    status: &str,
    tint: egui::Color32,
    side: f32,
) {
    let scale = side / 146.0;
    let (rect, response) = ui.allocate_exact_size(vec2(side, side), egui::Sense::hover());
    response.widget_info(|| {
        egui::WidgetInfo::labeled(
            egui::WidgetType::Label,
            true,
            format!("{status} {}", digits(seconds)),
        )
    });
    let center = rect.center();
    let radius = rect.width() * 0.46;
    ui.painter().circle_stroke(
        center,
        radius,
        Stroke::new(
            3.0 * scale,
            if ui.visuals().dark_mode {
                egui::Color32::from_gray(72)
            } else {
                egui::Color32::from_gray(195)
            },
        ),
    );
    let fraction = if duration == 0 {
        0.0
    } else {
        (1.0 - seconds as f32 / (duration * 60) as f32).clamp(0.015, 1.0)
    };
    let point_count = (72.0 * fraction).ceil() as u32;
    if point_count > 0 {
        let points = (0..=point_count)
            .map(|index| {
                let angle = index as f32 / point_count as f32 * TAU * fraction - FRAC_PI_2;
                center + vec2(angle.cos(), angle.sin()) * radius
            })
            .collect();
        ui.painter()
            .add(egui::Shape::line(points, Stroke::new(3.0_f32, tint)));
    }
    ui.painter().text(
        center - vec2(0.0, 8.0) * scale,
        egui::Align2::CENTER_CENTER,
        digits(seconds),
        FontId::monospace(29.0 * scale),
        ui.visuals().text_color(),
    );
    ui.painter().text(
        center + vec2(0.0, 18.0) * scale,
        egui::Align2::CENTER_CENTER,
        status,
        FontId::proportional(12.0 * scale),
        ui.visuals().weak_text_color(),
    );
}

use std::{collections::VecDeque, time::Instant};

use chrono::{DateTime, Local, Utc};
use chrono_tz::{America, Asia, Europe, Tz};
use eframe::egui::{self, Align, Color32, Layout, Pos2, RichText, Shape, Stroke, Ui};

#[cfg(windows)]
use crate::system_metrics::Sampler;
use crate::system_metrics::{Memory, Sample};

use super::{accent, card, in_capsule, subhead, title};

#[cfg(windows)]
const SAMPLE_INTERVAL: std::time::Duration = std::time::Duration::from_secs(1);
const HISTORY_SECONDS: f32 = 60.0;
const CLOCKS: [(&str, Tz); 5] = [
    ("北京", Asia::Shanghai),
    ("东京", Asia::Tokyo),
    ("伦敦", Europe::London),
    ("纽约", America::New_York),
    ("洛杉矶", America::Los_Angeles),
];

#[derive(Default)]
pub struct Performance {
    #[cfg(windows)]
    sampler: Sampler,
    history: VecDeque<(Instant, Sample)>,
}

impl Performance {
    #[cfg(windows)]
    pub fn show(&mut self, ui: &mut Ui, store: &mut crate::workspace::Store) {
        let now = Instant::now();
        if self
            .history
            .back()
            .is_some_and(|(latest, _)| now.duration_since(*latest) > SAMPLE_INTERVAL * 3)
        {
            self.sampler = Sampler::default();
            self.history.clear();
        }
        if self
            .history
            .back()
            .is_none_or(|(latest, _)| now.duration_since(*latest) >= SAMPLE_INTERVAL)
        {
            self.history.push_back((now, self.sampler.poll(now)));
            self.history.retain(|(sampled_at, _)| {
                now.duration_since(*sampled_at).as_secs_f32() <= HISTORY_SECONDS
            });
        }

        let &(sampled_at, _) = self.history.back().expect("initialized performance sample");
        ui.ctx()
            .request_repaint_after(SAMPLE_INTERVAL.saturating_sub(now.duration_since(sampled_at)));

        self.render(ui, Utc::now());
        let options = |ui: &mut Ui, store: &mut crate::workspace::Store| {
            let mut enabled = store.state().settings.show_system_status;
            if ui.checkbox(&mut enabled, "显示电源与输入状态").changed()
                && let Err(error) = store.update(|state| {
                    state.settings.show_system_status = enabled;
                    Ok(())
                })
            {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            let mut pinned = store.state().settings.capsule_device_metrics;
            if ui
                .checkbox(&mut pinned, "在横向胶囊显示 CPU、内存和电量")
                .changed()
                && let Err(error) = store.update(|state| {
                    state.settings.capsule_device_metrics = pinned;
                    Ok(())
                })
            {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            let mut changes = store.state().settings.capsule_system_changes;
            if ui
                .checkbox(&mut changes, "在收起的胶囊短暂提示网络、音量和蓝牙变化")
                .changed()
                && let Err(error) = store.update(|state| {
                    state.settings.capsule_system_changes = changes;
                    Ok(())
                })
            {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        };
        if in_capsule(ui) {
            ui.collapsing("设备状态与设置", |ui| {
                options(ui, store);
                device_status(ui, store);
            });
        } else {
            card(ui, |ui| {
                subhead(ui, "设备状态");
                options(ui, store);
                device_status(ui, store);
            });
        }
    }

    fn render(&self, ui: &mut Ui, now: DateTime<Utc>) {
        let &(sampled_at, sample) = self.history.back().expect("initialized performance sample");
        let compact = in_capsule(ui);
        title(ui, "系统性能");
        if !compact {
            ui.weak("最近 60 秒，每秒更新");
            ui.add_space(10.0);
        }

        let dark = ui.visuals().dark_mode;
        let cpu_color = accent(dark);
        let memory_color = if dark {
            Color32::from_rgb(188, 162, 255)
        } else {
            Color32::from_rgb(108, 66, 185)
        };
        let upload_color = if dark {
            Color32::from_rgb(255, 185, 125)
        } else {
            Color32::from_rgb(154, 77, 23)
        };

        let counter = |ui: &mut Ui, label: &str, detail: Option<String>, line: ChartLine| {
            card(ui, |ui| {
                let heading = ui.add(egui::Label::new(label).truncate());
                if let Some(detail) = detail {
                    heading.on_hover_text(detail);
                }
                let usage = line.1(sample);
                let text = usage.map_or_else(|| "—".into(), percent);
                let reading = RichText::new(text).monospace().size(19.0).strong();
                if usage.is_some() {
                    ui.label(reading);
                } else {
                    ui.label(reading.weak()).on_hover_text("暂无数据");
                }
                self.draw_chart(ui, sampled_at, 100.0, &[line]);
            });
        };
        let cpu = |ui: &mut Ui| {
            counter(
                ui,
                "CPU 使用率",
                None,
                (cpu_color, |sample| sample.cpu_percent),
            );
        };
        let memory = |ui: &mut Ui| {
            let capacity = sample.memory.map(|memory| {
                const GIB: f64 = 1024.0 * 1024.0 * 1024.0;
                format!(
                    "已用 {:.1} / {:.1} GiB",
                    memory.used as f64 / GIB,
                    memory.total as f64 / GIB
                )
            });
            counter(
                ui,
                "内存使用率",
                capacity,
                (memory_color, |sample| sample.memory.map(Memory::percent)),
            );
        };
        if ui.available_width() >= if compact { 220.0 } else { 480.0 } {
            ui.columns(2, |columns| {
                cpu(&mut columns[0]);
                memory(&mut columns[1]);
            });
        } else {
            cpu(ui);
            memory(ui);
        }
        if !compact {
            ui.add_space(8.0);
        }

        card(ui, |ui| {
            subhead(ui, "网络速率");
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("下载").color(cpu_color));
                value(ui, sample.traffic.map(|traffic| rate(traffic.received)));
                ui.add_space(12.0);
                ui.label(RichText::new("上传").color(upload_color));
                value(ui, sample.traffic.map(|traffic| rate(traffic.sent)));
            });
            let peak = self
                .history
                .iter()
                .filter_map(|(_, sample)| sample.traffic)
                .map(|traffic| traffic.received.max(traffic.sent) as f32)
                .fold(1024.0, f32::max)
                .log2()
                .ceil()
                .exp2();
            self.draw_chart(
                ui,
                sampled_at,
                peak,
                &[
                    (cpu_color, |sample| {
                        sample.traffic.map(|traffic| traffic.received as f32)
                    }),
                    (upload_color, |sample| {
                        sample.traffic.map(|traffic| traffic.sent as f32)
                    }),
                ],
            );
            if !compact {
                ui.weak(format!("图表上限 {}", rate(f64::from(peak))));
            }
        });
        if compact {
            ui.collapsing("世界时钟", |ui| clock_rows(ui, now));
        } else {
            ui.add_space(16.0);
            clock_rows(ui, now);
        }
    }

    fn draw_chart(&self, ui: &mut Ui, now: Instant, max: f32, series: &[ChartLine]) {
        let compact = in_capsule(ui);
        let (response, painter) = ui.allocate_painter(
            egui::vec2(ui.available_width(), if compact { 30.0 } else { 54.0 }),
            egui::Sense::hover(),
        );
        let rect = response.rect.shrink(1.0);
        if compact {
            response.on_hover_text("最近 60 秒，每秒更新");
        }
        let grid_stroke = Stroke::new(1.0_f32, ui.visuals().widgets.noninteractive.bg_stroke.color);
        for fraction in [0.0, 0.5, 1.0] {
            let y = egui::lerp(rect.y_range(), fraction);
            painter.hline(rect.x_range(), y, grid_stroke);
        }

        for &(color, resolve) in series {
            let mut points = Vec::with_capacity(self.history.len());
            for &(sampled_at, sample) in &self.history {
                if let Some(value) = resolve(sample) {
                    let age = now.duration_since(sampled_at).as_secs_f32();
                    let x = egui::lerp(rect.x_range(), 1.0 - age / HISTORY_SECONDS);
                    let y = egui::lerp(rect.y_range(), 1.0 - (value / max).clamp(0.0, 1.0));
                    points.push(Pos2::new(x, y));
                } else {
                    paint_trace(&painter, &mut points, color);
                }
            }
            paint_trace(&painter, &mut points, color);
        }
    }
}

#[cfg(windows)]
fn device_status(ui: &mut Ui, store: &crate::workspace::Store) {
    if store.state().settings.show_system_status {
        let status = crate::windows::system_status::DeviceStatus::read();
        let power = status.battery.map_or_else(
            || status.power.clone(),
            |n| format!("{} · {n}%", status.power),
        );
        ui.horizontal_wrapped(|ui| {
            ui.label(format!("电源 {power}"));
            ui.label(if status.caps {
                "大写锁定已开启"
            } else {
                "大写锁定已关闭"
            });
            ui.label(format!(
                "输入语言 {}",
                status.language.as_deref().unwrap_or("未知")
            ));
        });
    }
}

type ChartLine = (Color32, fn(Sample) -> Option<f32>);

fn paint_trace(painter: &egui::Painter, points: &mut Vec<Pos2>, color: Color32) {
    match points.as_slice() {
        [] => {}
        [point] => {
            painter.circle_filled(*point, 1.6, color);
        }
        _ => {
            painter.add(Shape::line(
                std::mem::take(points),
                Stroke::new(1.6_f32, color),
            ));
        }
    }
    points.clear();
}

fn value(ui: &mut Ui, value: Option<String>) {
    if let Some(value) = value {
        ui.label(RichText::new(value).monospace().size(19.0).strong());
    } else {
        ui.weak("暂无数据");
    }
}

fn percent(value: f32) -> String {
    format!("{value:.1}%")
}

fn rate(bytes: f64) -> String {
    const KIB: f64 = 1024.0;
    const MIB: f64 = KIB * 1024.0;
    const GIB: f64 = MIB * 1024.0;
    if bytes < KIB {
        format!("{bytes:.0} B/s")
    } else if bytes < MIB {
        format!("{:.1} KiB/s", bytes / KIB)
    } else if bytes < GIB {
        format!("{:.1} MiB/s", bytes / MIB)
    } else {
        format!("{:.1} GiB/s", bytes / GIB)
    }
}

fn clock_rows(ui: &mut Ui, now: DateTime<Utc>) {
    let local = now.with_timezone(&Local);
    if !in_capsule(ui) {
        subhead(ui, "世界时钟");
        ui.add_space(6.0);
    }
    card(ui, |ui| {
        clock_row(
            ui,
            "本地",
            local.format("%H:%M").to_string(),
            local.format("%m-%d  UTC%:z").to_string(),
        );
        for (city, timezone) in CLOCKS {
            let time = now.with_timezone(&timezone);
            ui.separator();
            clock_row(
                ui,
                city,
                time.format("%H:%M").to_string(),
                time.format("%m-%d  UTC%:z").to_string(),
            );
        }
    });
}

fn clock_row(ui: &mut Ui, city: &str, time: String, date_and_offset: String) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.y = 3.0;
        ui.vertical(|ui| {
            ui.label(RichText::new(city).strong());
            ui.weak(RichText::new(date_and_offset).monospace());
        });
        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
            ui.label(RichText::new(time).monospace().size(23.0).strong());
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::system_metrics::Traffic;
    use std::time::Duration;

    #[test]
    fn performance_renders_counters_and_localized_clocks_at_both_widths() {
        let ctx = egui::Context::default();
        super::super::configure_style(&ctx);
        let sampled_at = Instant::now();
        let now = "2026-07-06T00:30:00Z".parse::<DateTime<Utc>>().unwrap();
        let mut performance = Performance::default();
        performance.history.push_back((
            sampled_at - Duration::from_secs(1),
            Sample {
                cpu_percent: Some(37.2),
                ..Default::default()
            },
        ));
        performance.history.push_back((
            sampled_at,
            Sample {
                cpu_percent: Some(38.5),
                memory: Some(Memory { used: 3, total: 4 }),
                traffic: Some(Traffic {
                    received: 4096.0,
                    sent: 250.0,
                }),
            },
        ));
        for (width, theme) in [(350.0, egui::Theme::Light), (780.0, egui::Theme::Dark)] {
            ctx.set_theme(theme);
            let input = egui::RawInput {
                screen_rect: Some(egui::Rect::from_min_size(
                    Pos2::ZERO,
                    egui::vec2(width, 1200.0),
                )),
                ..Default::default()
            };
            let render = |ui: &mut Ui| {
                egui::CentralPanel::default().show_inside(ui, |ui| performance.render(ui, now));
            };
            let _ = ctx.run_ui(input.clone(), render);
            let output = ctx.run_ui(input, render);
            if width >= 480.0 {
                let baseline = |label| {
                    output
                        .shapes
                        .iter()
                        .find_map(|clipped| match &clipped.shape {
                            Shape::Text(text) if text.galley.job.text == label => Some(text.pos.y),
                            _ => None,
                        })
                        .unwrap()
                };
                assert_eq!(baseline("CPU 使用率"), baseline("内存使用率"));
                assert_eq!(baseline("38.5%"), baseline("75.0%"));
            }
            let text = output
                .shapes
                .iter()
                .filter_map(|clipped| match &clipped.shape {
                    Shape::Text(text) => {
                        assert!(clipped.clip_rect.contains_rect(text.visual_bounding_rect()));
                        Some(text.galley.job.text.as_str())
                    }
                    _ => None,
                })
                .collect::<Vec<_>>();
            for expected in [
                "38.5%",
                "75.0%",
                "4.0 KiB/s",
                "250 B/s",
                "世界时钟",
                "东京",
                "01:30",
                "20:30",
                "07-05  UTC-04:00",
            ] {
                assert!(
                    text.contains(&expected),
                    "missing {expected} at width {width}: {text:?}"
                );
            }
        }
    }
}

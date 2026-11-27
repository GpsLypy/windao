use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

use chrono::{Local, TimeZone};
use eframe::egui::{self, RichText, Ui};

use super::{card, in_capsule, subhead, title};
use crate::{
    ai_usage::{ProviderUsage, UsageReport},
    workspace::Store,
};

const REFRESH: Duration = Duration::from_secs(300);

#[derive(Default)]
pub(crate) struct AiMeter {
    report: Option<UsageReport>,
    worker: Option<Receiver<UsageReport>>,
    refresh_at: Option<Instant>,
    failed: bool,
    #[cfg(windows)]
    accounts: super::provider_balances::ProviderBalances,
}

impl AiMeter {
    pub(crate) fn poll(&mut self, ctx: &egui::Context, enabled: bool) {
        if !enabled {
            self.report = None;
            self.worker = None;
            self.refresh_at = None;
            self.failed = false;
            return;
        }
        let now = Instant::now();
        if let Some(worker) = &self.worker {
            match worker.try_recv() {
                Ok(report) => {
                    self.report = Some(report);
                    self.worker = None;
                    self.failed = false;
                }
                Err(TryRecvError::Disconnected) => {
                    self.worker = None;
                    self.failed = true;
                }
                Err(TryRecvError::Empty) => return,
            }
        }
        if let Some(next) = self.refresh_at
            && now < next
        {
            ctx.request_repaint_after(next - now);
            return;
        }
        let (send, recv) = mpsc::channel();
        self.refresh_at = Some(now + REFRESH);
        let ctx = ctx.clone();
        let started = std::thread::Builder::new()
            .name("local-usage".into())
            .spawn(move || {
                let _ = send.send(crate::ai_usage::read_local());
                ctx.request_repaint_of(egui::ViewportId::ROOT);
                ctx.request_repaint_of(egui::ViewportId::from_hash_of("windao-workbench"));
            });
        match started {
            Ok(_) => self.worker = Some(recv),
            Err(_) => self.failed = true,
        }
    }

    #[cfg(windows)]
    pub(crate) fn account_caption(
        &mut self,
        ctx: &egui::Context,
        directory: &std::path::Path,
    ) -> Option<String> {
        self.accounts.tick(ctx, directory);
        self.accounts.caption()
    }
    pub(crate) fn caption(&self) -> Option<String> {
        let r = self.report.as_ref()?;
        if r.codex.last_seen.is_none() && r.claude.last_seen.is_none() {
            return None;
        }
        Some(format!(
            "AI 今日 {}{}",
            if r.codex.partial || r.claude.partial {
                "≥"
            } else {
                ""
            },
            tokens(r.codex.today_tokens.saturating_add(r.claude.today_tokens))
        ))
    }

    pub(super) fn show(&mut self, ui: &mut Ui, store: &mut Store) {
        title(ui, "AI 用量");
        let was_enabled = store.state().settings.local_ai_usage;
        let mut enabled = was_enabled;
        let mut pinned = store.state().settings.capsule_ai_usage;
        let mut save_error = None;
        let compact = in_capsule(ui);
        let options = |ui: &mut Ui, enabled: &mut bool, pinned: &mut bool| {
            ui.checkbox(enabled, "读取 Codex 和 Claude Code 用量")
                .on_hover_text("按本机日期统计今天和近 7 天用量，订阅限额来自客户端记录。");
            if *enabled {
                ui.checkbox(pinned, "在胶囊显示今日用量");
            }
        };
        if compact {
            ui.horizontal_wrapped(|ui| {
                if was_enabled {
                    ui.menu_button("本机统计设置", |ui| {
                        options(ui, &mut enabled, &mut pinned)
                    });
                    if ui
                        .add_enabled(self.worker.is_none(), egui::Button::new("刷新"))
                        .clicked()
                    {
                        self.refresh_at = None;
                    }
                } else {
                    ui.checkbox(&mut enabled, "读取本机用量")
                        .on_hover_text("读取 Codex 和 Claude Code 保存在本机的用量记录。");
                }
            });
        } else {
            card(ui, |ui| {
                subhead(ui, "本机使用记录");
                options(ui, &mut enabled, &mut pinned);
                if enabled
                    && ui
                        .add_enabled(self.worker.is_none(), egui::Button::new("刷新用量"))
                        .clicked()
                {
                    self.refresh_at = None;
                }
            });
        }
        if (was_enabled != enabled || pinned != store.state().settings.capsule_ai_usage)
            && let Err(error) = store.update(|state| {
                state.settings.local_ai_usage = enabled;
                state.settings.capsule_ai_usage = enabled && pinned;
                Ok(())
            })
        {
            save_error = Some(error);
        }
        self.poll(ui.ctx(), store.state().settings.local_ai_usage);
        if let Some(error) = save_error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        if self.failed {
            ui.label("读取用量中断，可点击刷新重试。");
        }
        if let Some(report) = &self.report {
            if compact && ui.available_width() >= 420.0 {
                ui.columns(2, |columns| {
                    card(&mut columns[0], |ui| usage_card(ui, "Codex", &report.codex));
                    card(&mut columns[1], |ui| {
                        usage_card(ui, "Claude Code", &report.claude)
                    });
                });
            } else {
                for (name, usage) in [("Codex", &report.codex), ("Claude Code", &report.claude)] {
                    card(ui, |ui| usage_card(ui, name, usage));
                }
            }
        } else if self.worker.is_some() {
            ui.horizontal(|ui| {
                ui.spinner();
                ui.label("正在读取本机用量…");
            });
        }
        #[cfg(windows)]
        self.accounts.show(ui, store.directory());
    }
}

fn tokens(n: u64) -> String {
    match n {
        0..1_000 => n.to_string(),
        1_000..1_000_000 => format!("{:.1}k", n as f64 / 1_000.),
        _ => format!("{:.2}M", n as f64 / 1_000_000.),
    }
}

fn usage_card(ui: &mut Ui, name: &str, usage: &ProviderUsage) {
    subhead(ui, name);
    if !usage.source_found {
        ui.weak("尚未找到使用记录。运行一次客户端后可刷新。");
        return;
    }
    if usage.last_seen.is_none() {
        ui.weak("近期文件中没有可用的用量记录。");
    } else {
        ui.horizontal_wrapped(|ui| {
            ui.label(
                RichText::new(format!("今日 {} token", tokens(usage.today_tokens)))
                    .size(18.)
                    .strong(),
            );
            ui.label(format!("近 7 天 {} token", tokens(usage.week_tokens)));
        });
    }
    if usage.partial {
        ui.weak("部分记录未能读取，统计仅包含已读取的记录。");
    }
    let now = Local::now().timestamp();
    for w in &usage.windows {
        let reset = w.resets_at.and_then(|s| Local.timestamp_opt(s, 0).single());
        let period = if w.minutes.is_multiple_of(1_440) {
            format!("{} 天", w.minutes / 1_440)
        } else if w.minutes.is_multiple_of(60) {
            format!("{} 小时", w.minutes / 60)
        } else {
            format!("{} 分钟", w.minutes)
        };
        if w.resets_at.is_some_and(|t| t <= now) {
            ui.weak(format!("{period}窗口已到重置时间，等待客户端更新。"));
            continue;
        }
        ui.label(format!("{period}窗口  ·  已用 {:.0}%", w.used_percent));
        ui.add(
            egui::ProgressBar::new((w.used_percent / 100.).clamp(0., 1.) as f32)
                .desired_width(ui.available_width().min(360.)),
        );
        if let Some(t) = reset {
            ui.weak(format!("重置时间 {}", t.format("%m-%d %H:%M")));
        }
        if let Some(t) = Local.timestamp_opt(w.observed_at, 0).single() {
            ui.weak(format!("记录于 {}", t.format("%m-%d %H:%M")));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn disabled_scan_discards_pending_result_and_partial_caption_is_explicit() {
        let (send, recv) = mpsc::channel();
        let mut meter = AiMeter {
            report: Some(UsageReport::default()),
            worker: Some(recv),
            refresh_at: Some(Instant::now() + REFRESH),
            ..Default::default()
        };
        assert!(meter.caption().is_none());
        let ctx = egui::Context::default();
        send.send(UsageReport {
            codex: ProviderUsage {
                last_seen: Some(1),
                today_tokens: 491,
                partial: true,
                ..Default::default()
            },
            ..Default::default()
        })
        .unwrap();
        meter.poll(&ctx, true);
        assert_eq!(meter.caption().as_deref(), Some("AI 今日 ≥491"));
        let (abandoned, recv) = mpsc::channel();
        meter.worker = Some(recv);
        meter.poll(&ctx, false);
        assert!(meter.caption().is_none() && meter.refresh_at.is_none() && meter.worker.is_none());
        assert!(abandoned.send(UsageReport::default()).is_err());
    }
}

use std::{
    fs,
    io::{self, Write},
    path::Path,
};

use chrono::{DateTime, Datelike, Local, NaiveTime, TimeZone};
use eframe::egui::{self, RichText, Ui};
use serde::{Deserialize, Serialize};

use super::{card, field, primary, subhead};

const FILE: &str = "salary.json";
const MAX_DAILY_CENTS: u64 = 100_000_000;

#[derive(Clone, Debug, Serialize, Deserialize)]
struct Schedule {
    daily_cents: u64,
    starts: NaiveTime,
    ends: NaiveTime,
    pause: Option<(NaiveTime, NaiveTime)>,
}

impl Schedule {
    fn valid(&self) -> bool {
        (1..=MAX_DAILY_CENTS).contains(&self.daily_cents)
            && self.starts < self.ends
            && self.pause.is_none_or(|(start, end)| {
                self.starts <= start
                    && start < end
                    && end <= self.ends
                    && (start, end) != (self.starts, self.ends)
            })
    }

    fn today<Tz: TimeZone>(&self, now: DateTime<Tz>) -> Option<(u64, f32)> {
        if now.weekday().number_from_monday() > 5 {
            return Some((0, 0.0));
        }
        let date = now.date_naive();
        let timezone = now.timezone();
        let window = |start, end| -> Option<(i64, i64)> {
            let start = timezone
                .from_local_datetime(&date.and_time(start))
                .earliest()?;
            let end = timezone
                .from_local_datetime(&date.and_time(end))
                .earliest()?;
            let total = (end.timestamp() - start.timestamp()).max(0);
            let elapsed = (now.timestamp() - start.timestamp()).clamp(0, total);
            Some((elapsed, total))
        };
        let (mut elapsed, mut total) = window(self.starts, self.ends)?;
        if let Some((start, end)) = self.pause {
            let (paused_elapsed, paused_total) = window(start, end)?;
            elapsed -= paused_elapsed;
            total -= paused_total;
        }
        if total <= 0 {
            return None;
        }
        Some((
            self.daily_cents * elapsed as u64 / total as u64,
            elapsed as f32 / total as f32,
        ))
    }
}

struct Draft {
    amount: String,
    starts: String,
    ends: String,
    pause_start: String,
    pause_end: String,
}

impl Draft {
    fn from_schedule(schedule: Option<&Schedule>) -> Self {
        let format_time = |time: NaiveTime| time.format("%H:%M").to_string();
        let pause = schedule.and_then(|schedule| schedule.pause);
        Self {
            amount: schedule.map_or_else(String::new, |schedule| decimal(schedule.daily_cents)),
            starts: schedule.map_or_else(|| "09:00".to_owned(), |s| format_time(s.starts)),
            ends: schedule.map_or_else(|| "18:00".to_owned(), |s| format_time(s.ends)),
            pause_start: pause.map_or_else(
                || {
                    if schedule.is_none() {
                        "12:00".to_owned()
                    } else {
                        String::new()
                    }
                },
                |(start, _)| format_time(start),
            ),
            pause_end: pause.map_or_else(
                || {
                    if schedule.is_none() {
                        "13:00".to_owned()
                    } else {
                        String::new()
                    }
                },
                |(_, end)| format_time(end),
            ),
        }
    }

    fn schedule(&self) -> Result<Schedule, &'static str> {
        let daily_cents = parse_cents(&self.amount).ok_or("请输入有效日薪，最多保留两位小数。")?;
        let time = |value: &str| {
            NaiveTime::parse_from_str(value.trim(), "%H:%M")
                .map_err(|_| "请输入有效时间，格式为 HH:MM。")
        };
        let pause = match (self.pause_start.trim(), self.pause_end.trim()) {
            ("", "") => None,
            (start, end) => Some((time(start)?, time(end)?)),
        };
        let schedule = Schedule {
            daily_cents,
            starts: time(&self.starts)?,
            ends: time(&self.ends)?,
            pause,
        };
        if !schedule.valid() {
            return Err("请设置正数日薪和同一天内的工作时段，休息须在工作时段内。");
        }
        Ok(schedule)
    }
}

fn decimal(cents: u64) -> String {
    format!("{}.{:02}", cents / 100, cents % 100)
}

fn parse_cents(amount: &str) -> Option<u64> {
    let (whole, fraction) = amount.trim().split_once('.').unwrap_or((amount.trim(), ""));
    if whole.is_empty() || !whole.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    let fraction = match fraction.as_bytes() {
        [] => 0,
        [tens @ b'0'..=b'9'] => u64::from(tens - b'0') * 10,
        [tens @ b'0'..=b'9', ones @ b'0'..=b'9'] => {
            u64::from(tens - b'0') * 10 + u64::from(ones - b'0')
        }
        _ => return None,
    };
    let cents = whole
        .parse::<u64>()
        .ok()?
        .checked_mul(100)?
        .checked_add(fraction)?;
    (1..=MAX_DAILY_CENTS).contains(&cents).then_some(cents)
}

#[derive(Default)]
pub struct Salary {
    loaded: bool,
    saved: Option<Schedule>,
    draft: Option<Draft>,
    error: Option<String>,
}

impl Salary {
    pub fn show(&mut self, ui: &mut Ui, profile: &Path) {
        if !self.loaded {
            self.loaded = true;
            match fs::read(profile.join(FILE)) {
                Ok(bytes) => match serde_json::from_slice::<Schedule>(&bytes) {
                    Ok(schedule) if schedule.valid() => self.saved = Some(schedule),
                    _ => self.error = Some("工资设置无法读取，请重新设置。".to_owned()),
                },
                Err(error) if error.kind() == io::ErrorKind::NotFound => {}
                Err(_) => self.error = Some("工资设置无法读取，请检查本机文件权限。".to_owned()),
            }
        }

        card(ui, |ui| {
            subhead(ui, "实时工资");
            if let Some(draft) = &mut self.draft {
                ui.weak("按周一至周五、本机时间估算。休息时段可留空。");
                field(ui, "日薪（人民币）", &mut draft.amount, "输入日薪", 16);
                field(ui, "上班时间", &mut draft.starts, "HH:MM", 5);
                field(ui, "下班时间", &mut draft.ends, "HH:MM", 5);
                field(ui, "休息开始", &mut draft.pause_start, "HH:MM", 5);
                field(ui, "休息结束", &mut draft.pause_end, "HH:MM", 5);
                let (save_clicked, cancel_clicked) = ui
                    .horizontal(|ui| {
                        (
                            primary(ui, "保存设置").clicked(),
                            ui.button("取消").clicked(),
                        )
                    })
                    .inner;
                if save_clicked {
                    let saved = draft
                        .schedule()
                        .map_err(str::to_owned)
                        .and_then(|schedule| {
                            save(profile, &schedule)
                                .map_err(|_| "工资设置保存失败，请检查本机文件权限。".to_owned())?;
                            Ok(schedule)
                        });
                    match saved {
                        Ok(schedule) => {
                            self.saved = Some(schedule);
                            self.draft = None;
                            self.error = None;
                        }
                        Err(error) => self.error = Some(error),
                    }
                }
                if cancel_clicked {
                    self.draft = None;
                    self.error = None;
                }
            } else if let Some(schedule) = &self.saved {
                let now = Local::now();
                if let Some((earned, progress)) = schedule.today(now) {
                    ui.weak("今日估算");
                    ui.label(
                        RichText::new(format!("¥ {}", decimal(earned)))
                            .monospace()
                            .size(27.0)
                            .strong(),
                    );
                    ui.add(egui::ProgressBar::new(progress).show_percentage());
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_secs(1));
                } else {
                    ui.weak("本机时区切换影响今天的工时，无法估算。");
                }
                let shift = format!(
                    "周一至周五  {}–{}",
                    schedule.starts.format("%H:%M"),
                    schedule.ends.format("%H:%M")
                );
                ui.weak(match schedule.pause {
                    Some((start, end)) => format!(
                        "{shift}  ·  休息 {}–{}",
                        start.format("%H:%M"),
                        end.format("%H:%M")
                    ),
                    None => shift,
                });
                ui.horizontal(|ui| {
                    if ui.button("修改设置").clicked() {
                        self.draft = Some(Draft::from_schedule(self.saved.as_ref()));
                    }
                    if ui.button("关闭并清除").clicked() {
                        match fs::remove_file(profile.join(FILE)) {
                            Err(error) if error.kind() != io::ErrorKind::NotFound => {
                                self.error =
                                    Some("工资设置清除失败，请检查本机文件权限。".to_owned());
                            }
                            _ => {
                                self.saved = None;
                                self.error = None;
                            }
                        }
                    }
                });
            } else {
                ui.weak("填写日薪与工作时段后，按已过工时估算今日收入。设置仅保存在这台电脑。");
                if ui.button("设置并启用").clicked() {
                    self.draft = Some(Draft::from_schedule(None));
                    self.error = None;
                }
            }
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
        });
    }
}

fn save(profile: &Path, schedule: &Schedule) -> io::Result<()> {
    let mut temp = tempfile::NamedTempFile::new_in(profile)?;
    serde_json::to_writer(temp.as_file_mut(), schedule)?;
    temp.flush()?;
    temp.as_file().sync_all()?;
    temp.persist(profile.join(FILE))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono_tz::Asia::Shanghai;

    #[test]
    fn salary_accrues_by_paid_seconds_and_pauses_during_breaks_and_weekends() {
        let mut draft = Draft::from_schedule(None);
        assert!(draft.schedule().is_err());
        draft.amount = "900.12".to_owned();
        let schedule = draft.schedule().unwrap();
        let estimate = |day, hour, minute, second| {
            let now = Shanghai
                .with_ymd_and_hms(2026, 11, day, hour, minute, second)
                .unwrap();
            schedule.today(now).unwrap().0
        };
        assert_eq!(estimate(26, 8, 59, 59), 0);
        assert_eq!(estimate(26, 9, 0, 1), 3);
        assert_eq!(estimate(26, 12, 0, 0), estimate(26, 12, 59, 59));
        assert_eq!(estimate(26, 17, 59, 59), 90_008);
        assert_eq!(estimate(26, 18, 0, 0), 90_012);
        assert_eq!(estimate(28, 18, 0, 0), 0);
    }

    #[test]
    fn salary_is_disabled_until_the_user_sets_an_amount() {
        let profile = tempfile::tempdir().unwrap();
        let ctx = egui::Context::default();
        super::super::configure_style(&ctx);
        let mut salary = Salary::default();
        let _ = ctx.run_ui(egui::RawInput::default(), |ui| {
            salary.show(ui, profile.path())
        });
        assert!(salary.loaded);
        assert!(salary.saved.is_none());
        assert!(salary.draft.is_none());
        assert!(!profile.path().join(FILE).exists());
    }

    #[test]
    fn salary_settings_replace_atomically_in_a_separate_local_file() {
        let profile = tempfile::tempdir().unwrap();
        let mut draft = Draft::from_schedule(None);
        for amount in ["300.5", "420.02"] {
            draft.amount = amount.to_owned();
            let schedule = draft.schedule().unwrap();
            save(profile.path(), &schedule).unwrap();
            let bytes = fs::read(profile.path().join(FILE)).unwrap();
            let saved = serde_json::from_slice::<Schedule>(&bytes).unwrap();
            assert_eq!(saved.daily_cents, schedule.daily_cents);
        }
        assert_eq!(fs::read_dir(profile.path()).unwrap().count(), 1);
    }
}

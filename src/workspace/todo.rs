use std::collections::HashSet;
use std::fs::File;
use std::io::Read;
use std::path::Path;

use chrono::{DateTime, Datelike, Days, Local, Months, NaiveDate, TimeZone, Weekday};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

use super::{LegacyTask, MAX_RECORDS, Priority, Result, Store, Task, TaskStatus, valid_title};

const MAX_COMPLETION_DATES: usize = 3660;
const MAX_CHECKLIST_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(tag = "rule", content = "days", rename_all = "camelCase")]
pub enum TaskRepeat {
    #[default]
    Once,
    Weekdays(u8),
    Monthly,
    Yearly,
}

impl TaskRepeat {
    pub const EVERY_DAY: u8 = 0b111_1111;
    pub const WORKDAYS: u8 = 0b001_1111;
    pub const WEEKEND: u8 = 0b110_0000;

    pub fn is_repeating(self) -> bool {
        self != Self::Once
    }

    fn runs_on(self, date: NaiveDate, anchor: Option<NaiveDate>) -> bool {
        match self {
            Self::Once => true,
            Self::Weekdays(mask) => mask & weekday_bit(date.weekday()) != 0,
            Self::Monthly | Self::Yearly => anchor.is_some_and(|first| {
                (self != Self::Yearly || date.month() == first.month())
                    && at_month_day(date, first.day()) == Some(date)
            }),
        }
    }

    fn valid(self, date: Option<i64>) -> bool {
        match self {
            Self::Once => true,
            Self::Weekdays(mask) => (1..=Self::EVERY_DAY).contains(&mask),
            Self::Monthly | Self::Yearly => date.is_some(),
        }
    }
}

pub fn weekday_bit(day: Weekday) -> u8 {
    1 << day.num_days_from_monday()
}

pub(super) fn local_date(timestamp: i64) -> Option<NaiveDate> {
    DateTime::from_timestamp_millis(timestamp).map(|time| time.with_timezone(&Local).date_naive())
}

fn at_month_day(month: NaiveDate, day: u32) -> Option<NaiveDate> {
    month
        .with_day(1)?
        .checked_add_months(Months::new(1))?
        .pred_opt()
        .and_then(|last| month.with_day(day.min(last.day())))
}

impl Task {
    pub fn scheduled_on(&self, date: NaiveDate) -> bool {
        self.repeat_start.is_none_or(|start| date >= start)
            && self.repeat.runs_on(date, self.due_at.and_then(local_date))
    }

    pub fn status_on(&self, date: NaiveDate) -> TaskStatus {
        if self.repeat.is_repeating()
            && self.status == TaskStatus::Active
            && self.completed_dates.binary_search(&date).is_ok()
        {
            TaskStatus::Completed
        } else {
            self.status
        }
    }

    pub fn is_actionable_on(&self, date: NaiveDate) -> bool {
        self.scheduled_on(date) && self.status_on(date) == TaskStatus::Active
    }

    pub fn today_rank(&self, date: NaiveDate) -> (bool, bool) {
        (
            self.status_on(date) == TaskStatus::Completed,
            !self.scheduled_on(date),
        )
    }

    pub fn due_at_on(&self, date: NaiveDate) -> Option<i64> {
        if !self.repeat.is_repeating() {
            return self.due_at;
        }
        if !self.scheduled_on(date) {
            return None;
        }
        let anchor = DateTime::from_timestamp_millis(self.due_at?)?.with_timezone(&Local);
        if anchor.date_naive() == date {
            return self.due_at;
        }
        let time = date.and_time(anchor.time());
        Local
            .from_local_datetime(&time)
            .earliest()
            .or_else(|| {
                // A skipped local time is due at the first valid minute after a clock jump.
                (1..=180).find_map(|minutes| {
                    let shifted = time.checked_add_signed(chrono::Duration::minutes(minutes))?;
                    (shifted.date() == date)
                        .then(|| Local.from_local_datetime(&shifted).earliest())
                        .flatten()
                })
            })
            .map(|time| time.timestamp_millis())
    }

    pub(super) fn next_scheduled_on(&self, date: NaiveDate) -> Option<NaiveDate> {
        let date = self.repeat_start.map_or(date, |start| start.max(date));
        match self.repeat {
            TaskRepeat::Once => self.due_at.and_then(local_date).filter(|due| *due >= date),
            TaskRepeat::Weekdays(mask) => (0..7)
                .filter_map(|days| date.checked_add_days(Days::new(days)))
                .find(|date| mask & weekday_bit(date.weekday()) != 0),
            TaskRepeat::Monthly => {
                let day = local_date(self.due_at?)?.day();
                at_month_day(date, day)
                    .filter(|due| *due >= date)
                    .or_else(|| {
                        at_month_day(date.with_day(1)?.checked_add_months(Months::new(1))?, day)
                    })
            }
            TaskRepeat::Yearly => {
                let anchor = local_date(self.due_at?)?;
                let month = NaiveDate::from_ymd_opt(date.year(), anchor.month(), 1)?;
                at_month_day(month, anchor.day())
                    .filter(|due| *due >= date)
                    .or_else(|| {
                        at_month_day(month.checked_add_months(Months::new(12))?, anchor.day())
                    })
            }
        }
    }

    pub fn set_status(&mut self, status: TaskStatus, now: i64) {
        if self.repeat.is_repeating() {
            if let Some(date) = local_date(now) {
                match (status, self.completed_dates.binary_search(&date)) {
                    (TaskStatus::Completed, Err(index)) if self.scheduled_on(date) => {
                        self.completed_dates.insert(index, date);
                        if self.completed_dates.len() > MAX_COMPLETION_DATES {
                            self.completed_dates.remove(0);
                        }
                    }
                    (TaskStatus::Active, Ok(index)) if self.status == TaskStatus::Active => {
                        self.completed_dates.remove(index);
                    }
                    _ => {}
                }
                self.status = match status {
                    TaskStatus::Completed => TaskStatus::Active,
                    other => other,
                };
                self.completed_at = None;
            }
        } else if status != self.status {
            self.status = status;
            self.completed_at = (status == TaskStatus::Completed).then_some(now);
        }
    }

    pub(super) fn valid_recurrence(&self) -> bool {
        self.repeat.valid(self.due_at)
            && self.completed_dates.len() <= MAX_COMPLETION_DATES
            && self.completed_dates.is_sorted_by(|a, b| a < b)
            && if self.repeat.is_repeating() {
                self.status != TaskStatus::Completed && self.completed_at.is_none()
            } else {
                self.completed_dates.is_empty() && self.repeat_start.is_none()
            }
    }
}

#[derive(Clone, Copy, Debug, Default, Deserialize)]
#[serde(rename_all = "lowercase")]
pub(super) enum LegacyRepeat {
    #[default]
    Never,
    Daily,
    Weekdays,
    Weekly,
    Monthly,
    Yearly,
}

impl LegacyRepeat {
    pub(super) fn upgrade(self, due: Option<i64>) -> Result<TaskRepeat> {
        let repeat = match self {
            Self::Never => return Ok(TaskRepeat::Once),
            Self::Daily => TaskRepeat::Weekdays(TaskRepeat::EVERY_DAY),
            Self::Weekdays => TaskRepeat::Weekdays(TaskRepeat::WORKDAYS),
            Self::Weekly => TaskRepeat::Weekdays(weekday_bit(
                due.and_then(local_date)
                    .ok_or("旧版重复待办缺少有效的截止时间。")?
                    .weekday(),
            )),
            Self::Monthly => TaskRepeat::Monthly,
            Self::Yearly => TaskRepeat::Yearly,
        };
        if !repeat.valid(due) {
            return Err("旧版重复待办缺少有效的截止时间。".into());
        }
        Ok(repeat)
    }
}

pub(super) fn upgrade_legacy_tasks(legacy: Vec<LegacyTask>) -> Result<Vec<Task>> {
    let parents: HashSet<_> = legacy
        .iter()
        .filter_map(|task| task.generated_from)
        .collect();
    legacy
        .into_iter()
        .map(|mut task| {
            if parents.contains(&task.id) {
                // Electron saves completed instances as separate entries. Keep them and
                // make only the latest instance continue the series.
                task.repeat = LegacyRepeat::Never;
            } else if let Some(next) = task.pending_repeat_at.filter(|_| task.completed) {
                let mut current = Task::try_from(task)?;
                if current.repeat.is_repeating() {
                    let next = DateTime::from_timestamp_millis(next)
                        .filter(|_| next >= 0)
                        .ok_or("旧版重复待办的下次截止时间无效。")?
                        .with_timezone(&Local);
                    current.repeat_start = Some(next.date_naive());
                }
                return Ok(current);
            }
            Task::try_from(task)
        })
        .collect()
}

impl Store {
    pub fn import_checklist(&mut self, path: &Path, now: i64) -> Result<usize> {
        let mut data = String::new();
        File::open(path)
            .and_then(|file| file.take(MAX_CHECKLIST_BYTES + 1).read_to_string(&mut data))
            .map_err(|error| format!("无法读取岛仔待办文件（{error}）。"))?;
        if data.len() as u64 > MAX_CHECKLIST_BYTES {
            return Err("岛仔待办文件超过 2 MiB。".into());
        }
        let checklist = parse_checklist(&data, now)?;
        let mut added = 0;
        self.update(|state| {
            let known: HashSet<_> = state.tasks.iter().map(|task| task.id).collect();
            let tasks: Vec<_> = checklist
                .tasks
                .into_iter()
                .filter(|task| !known.contains(&task.id))
                .collect();
            added = tasks.len();
            let new_ids: HashSet<_> = tasks.iter().map(|task| task.id).collect();
            let delivered: Vec<_> = checklist
                .delivered
                .into_iter()
                .filter(|(id, _)| new_ids.contains(id))
                .map(|(_, key)| key)
                .collect();
            super::reminders::mark_delivered(&mut state.delivered_reminder_ids, delivered);
            state.tasks.extend(tasks);
            Ok(())
        })?;
        Ok(added)
    }
}

#[derive(Default)]
struct Checklist {
    tasks: Vec<Task>,
    delivered: Vec<(Uuid, String)>,
}

fn parse_checklist(data: &str, now: i64) -> Result<Checklist> {
    let mut checklist = Checklist::default();
    for (line_index, line) in data.trim_start_matches('\u{feff}').lines().enumerate() {
        if line.trim().is_empty() {
            continue;
        }
        let mut parts = line.splitn(3, '|');
        let flag = parts.next().unwrap_or_default();
        let date = parts.next();
        let content = parts.next();
        let repeat_mask = flag
            .strip_prefix('r')
            .filter(|mask| !mask.is_empty() && mask.bytes().all(|byte| byte.is_ascii_digit()));
        let invalid_row = || format!("岛仔待办文件第 {} 行有误。", line_index + 1);
        let parsed = if date.is_some() && (matches!(flag, "x" | "o") || repeat_mask.is_some()) {
            Some(date.zip(content).ok_or_else(invalid_row)?)
        } else {
            None
        };
        let repeat = match parsed {
            Some(_) if repeat_mask.is_some() => {
                TaskRepeat::Weekdays(repeat_mask.and_then(|mask| mask.parse().ok()).unwrap_or(0))
            }
            _ => TaskRepeat::Once,
        };
        let done = parsed
            .filter(|(date, _)| *date != "-" && (flag == "x" || repeat.is_repeating()))
            .map(|(date, _)| NaiveDate::parse_from_str(date, "%Y-%m-%d"))
            .transpose()
            .map_err(|_| invalid_row())?;
        let content = parsed.map_or(line, |(_, content)| content).trim();
        let metadata = parsed.and_then(|_| content.strip_prefix("@due:"));
        let (due_at, reminded, title) = if let Some(fields) = metadata {
            let mut fields = fields.splitn(3, '|');
            let date = fields.next().and_then(parse_local_minute);
            let reminded = fields.next().ok_or_else(invalid_row)?;
            let title = fields.next().ok_or_else(invalid_row)?;
            let valid_reminded = if repeat.is_repeating() {
                reminded == "0" || NaiveDate::parse_from_str(reminded, "%Y-%m-%d").is_ok()
            } else {
                matches!(reminded, "0" | "1")
            };
            (
                Some(date.filter(|_| valid_reminded).ok_or_else(invalid_row)?),
                reminded,
                title,
            )
        } else {
            (None, "", content)
        };
        if !valid_title(title, 500) || !repeat.valid(due_at) || checklist.tasks.len() == MAX_RECORDS
        {
            return Err(invalid_row());
        }
        let digest = Sha256::new()
            .chain_update(b"notch-calendar-daozai-todo")
            .chain_update((line_index as u64).to_le_bytes())
            .chain_update(line.trim())
            .finalize();
        let mut bytes: [u8; 16] = digest[..16].try_into().expect("16 hash bytes");
        bytes[6] = bytes[6] & 0x0f | 0x80;
        bytes[8] = bytes[8] & 0x3f | 0x80;
        let id = Uuid::from_bytes(bytes);
        let status = if done.is_some() && !repeat.is_repeating() {
            TaskStatus::Completed
        } else {
            TaskStatus::Active
        };
        let task = Task {
            id,
            title: title.into(),
            category: "none".into(),
            due_at,
            status,
            priority: Priority::None,
            importance: None,
            urgency: None,
            tags: Vec::new(),
            notes: String::new(),
            created_at: now,
            completed_at: done
                .filter(|_| !repeat.is_repeating())
                .and_then(|date| {
                    Local
                        .from_local_datetime(&date.and_hms_opt(12, 0, 0)?)
                        .earliest()
                })
                .map(|date| date.timestamp_millis()),
            reminder_minutes: Vec::new(),
            reminders_disabled: false,
            repeat,
            repeat_start: None,
            completed_dates: done.filter(|_| repeat.is_repeating()).into_iter().collect(),
        };
        let reminded_due = if repeat.is_repeating() {
            NaiveDate::parse_from_str(reminded, "%Y-%m-%d")
                .ok()
                .and_then(|date| task.due_at_on(date))
        } else {
            task.due_at.filter(|_| reminded == "1")
        };
        if let Some(due) = reminded_due {
            checklist.delivered.push((id, format!("todo:{id}:{due}:0")));
        }
        checklist.tasks.push(task);
    }
    Ok(checklist)
}

fn parse_local_minute(text: &str) -> Option<i64> {
    let date = chrono::NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M").ok()?;
    Local
        .from_local_datetime(&date)
        .earliest()
        .map(|time| time.timestamp_millis())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn day(text: &str) -> NaiveDate {
        text.parse().unwrap()
    }

    fn at(text: &str) -> i64 {
        parse_local_minute(text).unwrap()
    }

    #[test]
    fn completion_is_scoped_to_matching_weekdays() {
        let now = at("2026-11-23 10:00");
        let mut checklist = parse_checklist("r5|-|每周一、三", now).unwrap();
        let task = &mut checklist.tasks[0];
        assert!(task.is_actionable_on(day("2026-11-23")));
        task.set_status(TaskStatus::Completed, now);
        assert_eq!(task.status_on(day("2026-11-23")), TaskStatus::Completed);
        assert_eq!(task.today_rank(day("2026-11-23")), (true, false));
        assert_eq!(task.today_rank(day("2026-11-24")), (false, true));
        assert!(!task.is_actionable_on(day("2026-11-24")));
        assert!(task.is_actionable_on(day("2026-11-25")));
        task.set_status(TaskStatus::Completed, at("2026-11-24 10:00"));
        assert_eq!(task.completed_dates, [day("2026-11-23")]);
        task.set_status(TaskStatus::Active, now);
        assert!(task.completed_dates.is_empty());
    }

    #[test]
    fn old_rust_tasks_without_repeat_fields_remain_readable() {
        let task: Task = serde_json::from_value(serde_json::json!({
            "id": Uuid::new_v4(),
            "title": "旧版一次性待办",
            "category": "none",
            "status": "active",
            "priority": "none",
            "notes": "",
            "createdAt": 0
        }))
        .unwrap();
        assert_eq!(task.repeat, TaskRepeat::Once);
        assert!(task.completed_dates.is_empty());
        assert!(task.valid_recurrence());
    }

    #[test]
    fn checklist_import_is_idempotent_and_preserves_delivery_state() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("todo.txt");
        let content = "o|-|正常任务\nr5|2026-11-23|@due:2026-11-23 09:00|2026-11-25|周会 | 记录\n";
        std::fs::write(&path, content).unwrap();
        let now = at("2026-11-25 09:00");
        let profile = directory.path().join("profile");
        {
            let mut store = Store::open(profile.clone()).unwrap();
            assert_eq!(store.import_checklist(&path, now).unwrap(), 2);
            assert_eq!(store.import_checklist(&path, now).unwrap(), 0);
        }
        let store = Store::open(profile).unwrap();
        let task = &store.state().tasks[1];
        assert_eq!(task.title, "周会 | 记录");
        assert_eq!(task.status_on(day("2026-11-23")), TaskStatus::Completed);
        assert_eq!(store.next_reminder_at(now), Some(at("2026-11-30 09:00")));
        assert_eq!(std::fs::read_to_string(&path).unwrap(), content);
    }

    #[test]
    fn checklist_retains_plain_titles_and_rejects_malformed_structured_rows() {
        let now = at("2026-11-25 09:00");
        let checklist =
            parse_checklist("@due:查看待办提醒\nreview | 验收 | 截图\no|-|正常任务", now).unwrap();
        assert_eq!(checklist.tasks[0].title, "@due:查看待办提醒");
        assert_eq!(checklist.tasks[1].title, "review | 验收 | 截图");
        assert_eq!(checklist.tasks[2].title, "正常任务");
        assert!(parse_checklist("o|-|@due:2026-11-23 09:00|0", now).is_err());
        assert!(parse_checklist("r5|2026-99-23|周会", now).is_err());
    }

    #[test]
    fn recurring_reminders_survive_reopen_and_fire_for_the_next_occurrence() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("todo.txt");
        std::fs::write(&path, "r5|-|@due:2026-11-23 09:00|0|周会\n").unwrap();
        let now = at("2026-11-23 08:30");
        let first = at("2026-11-23 09:00");
        let second = at("2026-11-25 09:00");
        let profile = directory.path().join("profile");
        {
            let mut store = Store::open(profile.clone()).unwrap();
            store.import_checklist(&path, now).unwrap();
            assert_eq!(store.next_reminder_at(now), Some(first));
            store
                .deliver_due_reminders(first, |reminder| {
                    assert_eq!(reminder.body, "周会");
                    true
                })
                .unwrap();
        }
        let mut store = Store::open(profile).unwrap();
        assert_eq!(store.next_reminder_at(first), Some(second));
        store
            .deliver_due_reminders(first, |_| panic!("previous reminder reappeared"))
            .unwrap();
        store
            .deliver_due_reminders(second, |reminder| {
                assert_eq!(reminder.body, "周会");
                true
            })
            .unwrap();
        assert_eq!(store.next_reminder_at(second), Some(at("2026-11-30 09:00")));
    }

    #[test]
    fn electron_repeat_migration_keeps_completed_rows_and_live_series() {
        let parent = Uuid::new_v4();
        let make_task = |id, due, completed, generated_from| LegacyTask {
            id,
            title: "还款提醒".into(),
            category: "none".into(),
            completed,
            due_at: Some(at(due)),
            status: None,
            priority: Priority::None,
            importance: None,
            urgency: None,
            notes: String::new(),
            created_at: 0,
            completed_at: completed.then(|| at(due)),
            reminder_minutes: Vec::new(),
            reminders_disabled: false,
            repeat: LegacyRepeat::Monthly,
            generated_from,
            pending_repeat_at: None,
        };
        let mut latest = make_task(Uuid::new_v4(), "2026-12-31 09:00", true, Some(parent));
        latest.pending_repeat_at = Some(at("2027-01-31 09:00"));
        let tasks = upgrade_legacy_tasks(vec![
            make_task(parent, "2026-10-31 09:00", true, None),
            latest,
        ])
        .unwrap();
        assert_eq!(tasks[0].repeat, TaskRepeat::Once);
        assert_eq!(tasks[0].status, TaskStatus::Completed);
        assert_eq!(tasks[1].repeat, TaskRepeat::Monthly);
        assert!(!tasks[1].scheduled_on(day("2026-12-31")));
        assert!(tasks[1].scheduled_on(day("2027-02-28")));
        assert!(tasks[1].scheduled_on(day("2027-03-31")));
        assert_eq!(
            tasks[1].next_scheduled_on(day("2027-02-01")),
            Some(day("2027-02-28"))
        );
    }
}

#[cfg(test)]
mod summary_tests {
    use super::*;
    #[test]
    fn weekday_summary_counts_unfinished_retries_and_deduplicates_after_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().into()).unwrap();
        let at = Local
            .with_ymd_and_hms(2026, 11, 27, 17, 0, 0)
            .earliest()
            .unwrap()
            .timestamp_millis();
        store
            .update(|s| {
                s.settings.desktop_todo.summary_enabled = true;
                s.settings.desktop_todo.summary_weekdays = true;
                for status in [
                    TaskStatus::Active,
                    TaskStatus::Deferred,
                    TaskStatus::Completed,
                    TaskStatus::Cancelled,
                ] {
                    let mut task: Task = serde_json::from_value(serde_json::json!({
                        "id": Uuid::new_v4(), "title": "记录", "category": "none",
                        "status": "active", "priority": "none", "notes": "", "createdAt": at
                    }))
                    .unwrap();
                    task.set_status(status, at);
                    s.tasks.push(task);
                }
                Ok(())
            })
            .unwrap();
        assert_eq!(store.next_reminder_at(at - 1), Some(at));
        store.deliver_due_reminders(at, |_| false).unwrap();
        assert_eq!(store.next_reminder_at(at), Some(at));
        store
            .deliver_due_reminders(at, |r| {
                assert_eq!(r.body, "还有 2 项待办未完成");
                true
            })
            .unwrap();
        drop(store);
        let store = Store::open(dir.path().into()).unwrap();
        let monday = Local
            .with_ymd_and_hms(2026, 11, 30, 17, 0, 0)
            .earliest()
            .unwrap()
            .timestamp_millis();
        assert_eq!(store.next_reminder_at(at), Some(monday));
    }
}

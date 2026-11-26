use super::{Reminder, Result, valid_timestamp};
use chrono::{DateTime, Local, NaiveDate, Timelike};
use serde::{Deserialize, Serialize};
use std::ops::RangeInclusive;

pub const HEALTH_INTERVAL: RangeInclusive<u32> = 5..=240;

#[derive(Clone, Copy)]
pub enum HealthKind {
    Water,
    Stand,
}

impl HealthKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Water => "饮水提醒",
            Self::Stand => "久坐提醒",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthTimer {
    pub minutes: u32,
    pub next_at: Option<i64>,
}

impl HealthTimer {
    fn new(minutes: u32) -> Self {
        Self {
            minutes,
            next_at: None,
        }
    }

    pub fn schedule(&mut self, now: i64) {
        self.next_at = Some(now.saturating_add(i64::from(self.minutes) * 60_000));
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReminderHours {
    pub start: u16,
    pub end: u16,
}

impl ReminderHours {
    fn contains(self, minute: u16) -> bool {
        if self.start <= self.end {
            (self.start..self.end).contains(&minute)
        } else {
            minute >= self.start || minute < self.end
        }
    }
}

fn default_goal() -> u32 {
    2000
}
fn default_serving() -> u32 {
    250
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthReminders {
    water: HealthTimer,
    stand: HealthTimer,
    water_day: Option<NaiveDate>,
    water_count: u32,
    #[serde(default = "default_goal")]
    pub goal_ml: u32,
    #[serde(default = "default_serving")]
    pub serving_ml: u32,
    #[serde(default)]
    water_ml: u32,
    #[serde(default)]
    last_serving_ml: Option<u32>,
    #[serde(default)]
    pub hours: Option<ReminderHours>,
    #[serde(default)]
    pub quiet: Option<ReminderHours>,
}

impl Default for HealthReminders {
    fn default() -> Self {
        Self {
            water: HealthTimer::new(60),
            stand: HealthTimer::new(45),
            water_day: None,
            water_count: 0,
            water_ml: 0,
            last_serving_ml: None,
            goal_ml: default_goal(),
            serving_ml: default_serving(),
            hours: None,
            quiet: None,
        }
    }
}

impl HealthReminders {
    pub(super) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(super) fn validate(&self) -> Result<()> {
        if !(100..=10000).contains(&self.goal_ml)
            || !(10..=2000).contains(&self.serving_ml)
            || self
                .last_serving_ml
                .is_some_and(|n| n == 0 || n > self.water_ml || self.water_count == 0)
            || (self.water_day.is_none() && self.water_ml != 0)
            || [self.hours, self.quiet]
                .into_iter()
                .flatten()
                .any(|span| span.start >= 1440 || span.end >= 1440 || span.start == span.end)
            || !(0..1440).any(|m| self.allows_minute(m))
        {
            return Err("请检查饮水量与提醒时段，至少保留一分钟可提醒时间。".into());
        }
        if [self.water, self.stand].iter().any(|timer| {
            !HEALTH_INTERVAL.contains(&timer.minutes) || !timer.next_at.is_none_or(valid_timestamp)
        }) || (self.water_day.is_none() && self.water_count != 0)
        {
            return Err("饮水与久坐提醒配置无效。".into());
        }
        Ok(())
    }

    pub fn timer(&self, kind: HealthKind) -> HealthTimer {
        match kind {
            HealthKind::Water => self.water,
            HealthKind::Stand => self.stand,
        }
    }

    pub fn timer_mut(&mut self, kind: HealthKind) -> &mut HealthTimer {
        match kind {
            HealthKind::Water => &mut self.water,
            HealthKind::Stand => &mut self.stand,
        }
    }

    pub(super) fn deadlines(&self) -> impl Iterator<Item = (HealthKind, i64)> {
        [HealthKind::Water, HealthKind::Stand]
            .into_iter()
            .filter_map(|kind| self.timer(kind).next_at.map(|deadline| (kind, deadline)))
    }

    pub fn water_count(&self, today: NaiveDate) -> u32 {
        if self.water_day == Some(today) {
            self.water_count
        } else {
            0
        }
    }

    pub fn water_ml(&self, today: NaiveDate) -> u32 {
        if self.water_day == Some(today) {
            self.water_ml
        } else {
            0
        }
    }

    pub fn can_undo_water(&self, today: NaiveDate) -> bool {
        self.water_day == Some(today) && self.last_serving_ml.is_some()
    }

    pub fn undo_water(&mut self, today: NaiveDate) {
        if self.water_day == Some(today)
            && let Some(amount) = self.last_serving_ml.take()
        {
            self.water_ml = self.water_ml.saturating_sub(amount);
            self.water_count = self.water_count.saturating_sub(1);
        }
    }

    fn allows_minute(&self, minute: u16) -> bool {
        self.hours.is_none_or(|span| span.contains(minute))
            && self.quiet.is_none_or(|span| !span.contains(minute))
    }

    pub(super) fn next_allowed(&self, at: i64) -> i64 {
        if self.hours.is_none() && self.quiet.is_none() {
            return at;
        }
        let minute = |at| {
            DateTime::from_timestamp_millis(at).map(|dt| {
                let local = dt.with_timezone(&Local);
                (local.hour() * 60 + local.minute()) as u16
            })
        };
        if minute(at).is_none_or(|m| self.allows_minute(m)) {
            return at;
        }
        let base = at - at.rem_euclid(60_000);
        (1..=2880)
            .map(|n| base.saturating_add(n * 60_000))
            .find(|candidate| minute(*candidate).is_some_and(|m| self.allows_minute(m)))
            .unwrap_or(at)
    }

    pub fn fit_deadlines(&mut self, now: i64) {
        for kind in [HealthKind::Water, HealthKind::Stand] {
            if let Some(at) = self.timer(kind).next_at {
                let target = self.next_allowed(at.max(now));
                if target > at.max(now) || at >= now {
                    self.timer_mut(kind).next_at = Some(target);
                }
            }
        }
    }

    pub fn record_water(&mut self, today: NaiveDate) {
        let was = self.water_ml(today);
        self.water_ml = was.saturating_add(self.serving_ml);
        self.last_serving_ml = (self.water_ml > was).then_some(self.water_ml - was);
        self.water_count = self.water_count(today).saturating_add(1);
        self.water_day = Some(today);
    }
}

pub(super) fn notification(deadlines: &[(HealthKind, i64)]) -> Option<Reminder> {
    let (title, body) = match deadlines {
        [(HealthKind::Water, _)] => ("饮水提醒", "记得喝口水。"),
        [(HealthKind::Stand, _)] => ("久坐提醒", "起来走走，活动一下。"),
        [_, _] => ("饮水与久坐提醒", "喝口水，再起来活动一下。"),
        _ => return None,
    };
    Some(Reminder {
        title: title.into(),
        body: body.into(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::workspace::Store;

    #[test]
    fn reminders_coalesce_after_sleep_and_reschedule_from_delivery() {
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        let start = 1_800_000_000_000;
        assert_eq!(store.next_reminder_at(start), None);
        store
            .update(|state| {
                for kind in [HealthKind::Water, HealthKind::Stand] {
                    state.health.timer_mut(kind).schedule(start);
                }
                Ok(())
            })
            .unwrap();
        let overdue = start + 3 * 60 * 60_000;
        let first = start + 45 * 60_000;
        assert_eq!(store.next_reminder_at(overdue), Some(first));
        store.deliver_due_reminders(overdue, |_| false).unwrap();
        assert_eq!(store.next_reminder_at(overdue), Some(first));
        store
            .deliver_due_reminders(overdue, |reminder| {
                assert_eq!(reminder.title, "饮水与久坐提醒");
                true
            })
            .unwrap();
        assert_eq!(store.state().notifications.len(), 1);
        assert_eq!(store.next_reminder_at(overdue), Some(overdue + 45 * 60_000));
        drop(store);
        let mut store = Store::open(directory.path().into()).unwrap();
        assert_eq!(store.next_reminder_at(overdue), Some(overdue + 45 * 60_000));
        store
            .update(|state| {
                state.settings.reminders = false;
                Ok(())
            })
            .unwrap();
        assert_eq!(store.next_reminder_at(overdue), None);
    }

    #[test]
    fn drinking_count_starts_at_zero_each_local_date() {
        let mut reminders = HealthReminders::default();
        let today = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let tomorrow = today.succ_opt().unwrap();
        reminders.record_water(today);
        reminders.record_water(today);
        assert_eq!(reminders.water_count(today), 2);
        assert_eq!(reminders.water_count(tomorrow), 0);
        reminders.record_water(tomorrow);
        assert_eq!(reminders.water_count(tomorrow), 1);
    }
}

#[cfg(test)]
mod schedule_tests {
    use super::*;
    use crate::workspace::Store;
    use chrono::TimeZone;
    #[test]
    fn wake_during_lunch_defers_both_timers_and_keeps_them_coalesced() {
        let at = |h, m| {
            Local
                .with_ymd_and_hms(2026, 11, 26, h, m, 0)
                .single()
                .unwrap()
                .timestamp_millis()
        };
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().into()).unwrap();
        store
            .update(|s| {
                s.health.hours = Some(ReminderHours {
                    start: 9 * 60,
                    end: 18 * 60,
                });
                s.health.quiet = Some(ReminderHours {
                    start: 12 * 60,
                    end: 13 * 60,
                });
                for k in [HealthKind::Water, HealthKind::Stand] {
                    s.health.timer_mut(k).next_at = Some(at(11, 50));
                }
                Ok(())
            })
            .unwrap();
        store
            .deliver_due_reminders(at(12, 20), |_| panic!("quiet hours"))
            .unwrap();
        assert_eq!(store.next_health_reminder_at(), Some(at(13, 0)));
        store
            .deliver_due_reminders(at(13, 0), |r| r.title == "饮水与久坐提醒")
            .unwrap();
        assert!(store.next_health_reminder_at().unwrap() > at(13, 0));
    }

    #[test]
    fn old_counts_preserved_and_undo_uses_amount_recorded() {
        let day = NaiveDate::from_ymd_opt(2026, 11, 26).unwrap();
        let mut h: HealthReminders=serde_json::from_str(r#"{"water":{"minutes":60,"nextAt":null},"stand":{"minutes":45,"nextAt":null},"waterDay":"2026-11-26","waterCount":3}"#).unwrap();
        h.record_water(day);
        h.serving_ml = 500;
        assert_eq!(h.water_ml(day), 250);
        h.undo_water(day);
        assert_eq!((h.water_ml(day), h.water_count(day)), (0, 3));
        assert!(!h.can_undo_water(day));
        let night = ReminderHours {
            start: 22 * 60,
            end: 6 * 60,
        };
        assert!(night.contains(23 * 60) && night.contains(1));
        assert!(!night.contains(12 * 60));
    }
}

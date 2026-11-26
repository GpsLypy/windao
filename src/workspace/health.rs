use super::{Reminder, Result, valid_timestamp};
use chrono::NaiveDate;
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

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct HealthReminders {
    water: HealthTimer,
    stand: HealthTimer,
    water_day: Option<NaiveDate>,
    water_count: u32,
}

impl Default for HealthReminders {
    fn default() -> Self {
        Self {
            water: HealthTimer::new(60),
            stand: HealthTimer::new(45),
            water_day: None,
            water_count: 0,
        }
    }
}

impl HealthReminders {
    pub(super) fn is_default(&self) -> bool {
        self == &Self::default()
    }

    pub(super) fn validate(&self) -> Result<()> {
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

    pub fn record_water(&mut self, today: NaiveDate) {
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

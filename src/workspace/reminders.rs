use super::{Task, TaskStatus, Workspace, todo::local_date};
use std::collections::HashSet;
use uuid::Uuid;

const EVENT_GRACE: i64 = 5 * 60_000;
const TODO_GRACE: i64 = 30 * 60_000;
pub(super) const MAX_DELIVERED: usize = 32_000;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Reminder {
    pub title: String,
    pub body: String,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum Kind {
    Event(Uuid),
    Task(Uuid),
}

struct Scheduled {
    key: String,
    at: i64,
    kind: Kind,
    label: String,
}

impl Scheduled {
    fn still_relevant(&self, now: i64) -> bool {
        let grace = match self.kind {
            Kind::Event(_) => EVENT_GRACE,
            Kind::Task(_) => TODO_GRACE,
        };
        self.at >= now.saturating_sub(grace)
    }

    fn single_reminder(&self) -> Reminder {
        Reminder {
            title: match self.kind {
                Kind::Event(_) => "日程开始了",
                Kind::Task(_) => "待办提醒",
            }
            .into(),
            body: self.label.clone(),
        }
    }
}

pub(super) struct Schedule {
    enabled: bool,
    entries: Vec<Scheduled>,
    recurring: Vec<Task>,
    delivered: HashSet<String>,
    private_task_titles: bool,
}

impl Schedule {
    pub(super) fn from_workspace(state: &Workspace) -> Self {
        let delivered: HashSet<_> = state
            .delivered_reminder_ids
            .iter()
            .map(String::as_str)
            .collect();
        let mut entries = Vec::new();
        for event in state.events.iter().filter(|event| !event.all_day) {
            let key = format!("event:{}:{}", event.id, event.start_at);
            if !delivered.contains(key.as_str()) {
                entries.push(Scheduled {
                    key,
                    at: event.start_at,
                    kind: Kind::Event(event.id),
                    label: event.title.clone(),
                });
            }
        }
        for task in state
            .tasks
            .iter()
            .filter(|task| task.status == TaskStatus::Active && !task.reminders_disabled)
        {
            if task.repeat.is_repeating() {
                continue;
            }
            let Some(due) = task.due_at else { continue };
            for &offset in offsets(task) {
                let entry = task_entry(task, due, offset, state.private_task_titles);
                if !already_delivered(&entry.key, |key| delivered.contains(key)) {
                    entries.push(entry);
                }
            }
        }
        entries.sort_by_key(|entry| entry.at);
        Self {
            enabled: state.settings.reminders,
            entries,
            recurring: state
                .tasks
                .iter()
                .filter(|task| {
                    task.repeat.is_repeating()
                        && task.status == TaskStatus::Active
                        && task.due_at.is_some()
                        && !task.reminders_disabled
                })
                .cloned()
                .collect(),
            delivered: state.delivered_reminder_ids.iter().cloned().collect(),
            private_task_titles: state.private_task_titles,
        }
    }

    pub(super) fn elapsed_keys(&self, now: i64) -> Vec<String> {
        self.entries
            .iter()
            .take_while(|entry| entry.at < now)
            .map(|entry| entry.key.clone())
            .chain(
                self.recurring_entries(now)
                    .into_iter()
                    .filter(|entry| entry.at < now)
                    .map(|entry| entry.key),
            )
            .collect()
    }

    pub(super) fn next_at(&self, now: i64) -> Option<i64> {
        let single = self
            .relevant(now)
            .find(|entry| entry.still_relevant(now))
            .map(|entry| entry.at);
        let recurring = self
            .recurring_entries(now)
            .into_iter()
            .map(|entry| entry.at)
            .min();
        single.into_iter().chain(recurring).min()
    }

    pub(super) fn due(&self, now: i64) -> Option<(Vec<String>, Reminder)> {
        let recurring = self.recurring_entries(now);
        let mut pending: Vec<_> = self
            .relevant(now)
            .take_while(|entry| entry.at <= now)
            .filter(|entry| entry.still_relevant(now))
            .chain(recurring.iter().filter(|entry| entry.at <= now))
            .collect();
        pending.sort_by_key(|entry| entry.at);
        let reminder = summarize(&pending)?;
        let keys = pending.into_iter().map(|entry| entry.key.clone()).collect();
        Some((keys, reminder))
    }

    fn relevant(&self, now: i64) -> impl Iterator<Item = &Scheduled> {
        let start = if self.enabled {
            self.entries
                .partition_point(|entry| entry.at < now.saturating_sub(TODO_GRACE))
        } else {
            self.entries.len()
        };
        self.entries[start..].iter()
    }

    fn recurring_entries(&self, now: i64) -> Vec<Scheduled> {
        if !self.enabled {
            return Vec::new();
        }
        let earliest = now.saturating_sub(TODO_GRACE);
        let mut entries = Vec::new();
        for task in &self.recurring {
            for &offset in offsets(task) {
                let lead = i64::from(offset) * 60_000;
                let Some(mut date) = local_date(earliest.saturating_add(lead)) else {
                    continue;
                };
                while let Some(occurrence) = task.next_scheduled_on(date) {
                    if task.is_actionable_on(occurrence)
                        && let Some(due) = task.due_at_on(occurrence)
                    {
                        let entry = task_entry(task, due, offset, self.private_task_titles);
                        if entry.at >= earliest
                            && !already_delivered(&entry.key, |key| self.delivered.contains(key))
                        {
                            entries.push(entry);
                            break;
                        }
                    }
                    let Some(next) = occurrence.succ_opt() else {
                        break;
                    };
                    date = next;
                }
            }
        }
        entries
    }
}

fn offsets(task: &Task) -> &[u32] {
    if task.reminder_minutes.is_empty() {
        &[0]
    } else {
        &task.reminder_minutes
    }
}

fn task_entry(task: &Task, due: i64, offset: u32, private: bool) -> Scheduled {
    Scheduled {
        key: format!("todo:{}:{due}:{offset}", task.id),
        at: due - i64::from(offset) * 60_000,
        kind: Kind::Task(task.id),
        label: if private {
            "有一项待办需要关注".into()
        } else {
            task.title.clone()
        },
    }
}

fn already_delivered(key: &str, contains: impl Fn(&str) -> bool) -> bool {
    contains(key) || key.strip_suffix(":0").is_some_and(contains)
}

fn summarize(pending: &[&Scheduled]) -> Option<Reminder> {
    let first = pending.first()?;
    let items: HashSet<_> = pending.iter().map(|entry| entry.kind).collect();
    if items.len() == 1 {
        return Some(first.single_reminder());
    }
    let events = items
        .iter()
        .filter(|kind| matches!(kind, Kind::Event(_)))
        .count();
    let tasks = items.len() - events;
    let body = match (events, tasks) {
        (0, tasks) => format!("{tasks} 项待办需要关注"),
        (events, 0) => format!("{events} 场日程已开始"),
        (events, tasks) => format!("{events} 场日程已开始，{tasks} 项待办需要关注"),
    };
    Some(Reminder {
        title: "WinDao 提醒".into(),
        body,
    })
}

pub(super) fn mark_delivered(delivered: &mut Vec<String>, ids: Vec<String>) {
    delivered.extend(ids);
    let over_limit = delivered.len().saturating_sub(MAX_DELIVERED);
    delivered.drain(..over_limit);
}

mod notes_extra;
pub use notes_extra::{Card, MeetingNote};
mod calendar_sources;
pub(crate) mod event_identity;
#[cfg(windows)]
pub(crate) use calendar_sources::MAX_SOURCES;
pub use calendar_sources::{CalendarSource, CalendarSourceKind};
#[path = "backup.rs"]
pub(crate) mod backup;

use chrono::{DateTime, NaiveDate};
use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};
use tempfile::NamedTempFile;
use uuid::Uuid;

mod health;
pub use health::{HEALTH_INTERVAL, HealthKind, HealthReminders, ReminderHours};
mod reminders;
pub use reminders::Reminder;
use reminders::Schedule;
mod todo;
pub use todo::{TaskRepeat, weekday_bit};

const FORMAT: &str = "notchcalendar-windows-rust";
const VERSION: u32 = 1;
const MAX_FILE_BYTES: u64 = 32 * 1024 * 1024;
const MAX_RECORDS: usize = 2_000;
const MAX_FOCUS_SESSIONS: usize = 1_000;
pub(crate) const MAX_NOTES: usize = 100_000;
const MAX_NOTIFICATIONS: usize = 100;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SavedDate {
    pub id: Uuid,
    pub title: String,
    pub date: NaiveDate,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Notification {
    pub at: i64,
    pub title: String,
    pub body: String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum TaskbarApp {
    Qq,
    WeChat,
    WeCom,
    Feishu,
    DingTalk,
}

impl TaskbarApp {
    pub fn label(self) -> &'static str {
        match self {
            Self::Qq => "QQ",
            Self::WeChat => "微信",
            Self::WeCom => "企业微信",
            Self::Feishu => "飞书",
            Self::DingTalk => "钉钉",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct TaskbarNotification {
    pub app: TaskbarApp,
    pub at: i64,
}

struct AcceptedNotification {
    reminder_ids: Vec<String>,
    notification: Notification,
    health_deadlines: Vec<(HealthKind, i64)>,
}

type Result<T> = std::result::Result<T, String>;

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Event {
    pub id: Uuid,
    pub title: String,
    pub start_at: i64,
    pub end_at: i64,
    pub location: String,
    #[serde(default)]
    pub all_day: bool,
    pub start_date: Option<NaiveDate>,
    pub end_date: Option<NaiveDate>,
    pub url: Option<String>,
    pub subscription_id: Option<Uuid>,
    pub source: Option<CalendarSourceIdentity>,
}

#[derive(Clone, Debug, PartialEq, Eq, Hash, Deserialize, Serialize)]
pub struct CalendarSourceIdentity {
    pub uid: String,
    pub occurrence: String,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Priority {
    #[default]
    None,
    Low,
    Medium,
    High,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum TaskStatus {
    #[default]
    Active,
    Deferred,
    Completed,
    Cancelled,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Task {
    pub id: Uuid,
    pub title: String,
    pub category: String,
    pub due_at: Option<i64>,
    pub status: TaskStatus,
    pub priority: Priority,
    #[serde(default)]
    pub importance: Option<Importance>,
    #[serde(default)]
    pub urgency: Option<Urgency>,
    pub notes: String,
    pub created_at: i64,
    pub completed_at: Option<i64>,
    #[serde(default)]
    pub reminder_minutes: Vec<u32>,
    #[serde(default)]
    pub reminders_disabled: bool,
    #[serde(default)]
    pub repeat: TaskRepeat,
    pub repeat_start: Option<NaiveDate>,
    #[serde(default)]
    pub completed_dates: Vec<NaiveDate>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Importance {
    Important,
    NotImportant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum Urgency {
    Urgent,
    NotUrgent,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum FocusKind {
    #[default]
    Focus,
    Break,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Completion {
    pub id: Uuid,
    pub label: String,
    pub minutes: u32,
    pub completed_at: i64,
    #[serde(default)]
    pub kind: FocusKind,
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Focus {
    pub duration_minutes: u32,
    pub remaining_seconds: u32,
    pub target_at: Option<i64>,
    pub session_id: Option<Uuid>,
    pub label: String,
    #[serde(default)]
    pub kind: FocusKind,
    pub history: Vec<Completion>,
}

impl Default for Focus {
    fn default() -> Self {
        Self {
            duration_minutes: 25,
            remaining_seconds: 25 * 60,
            target_at: None,
            session_id: None,
            label: String::new(),
            kind: FocusKind::Focus,
            history: Vec::new(),
        }
    }
}

impl Focus {
    pub fn seconds_left(&self, now: i64) -> u32 {
        self.target_at.map_or(self.remaining_seconds, |target| {
            let seconds = target.saturating_sub(now).max(0).saturating_add(999) / 1000;
            seconds.min(i64::from(self.duration_minutes) * 60) as u32
        })
    }

    pub fn configure(&mut self, minutes: u32, label: String, kind: FocusKind) -> Result<()> {
        if self.session_id.is_some() {
            return Err("请先重置当前计时，再调整专注时长。".into());
        }
        if !(1..=180).contains(&minutes) || length(label.trim()) > 240 {
            return Err("请设置 1–180 分钟的时长和 240 字以内的名称。".into());
        }
        self.duration_minutes = minutes;
        self.remaining_seconds = minutes * 60;
        self.label = label.trim().to_owned();
        self.kind = kind;
        Ok(())
    }

    pub fn start(&mut self, now: i64) {
        self.settle(now);
        if self.target_at.is_some() {
            return;
        }
        if self.remaining_seconds == 0 {
            self.remaining_seconds = self.duration_minutes * 60;
        }
        self.session_id.get_or_insert_with(Uuid::new_v4);
        self.target_at = Some(now.saturating_add(i64::from(self.remaining_seconds) * 1000));
    }

    pub fn pause(&mut self, now: i64) {
        self.settle(now);
        self.remaining_seconds = self.seconds_left(now);
        self.target_at = None;
    }

    pub fn reset(&mut self) {
        self.remaining_seconds = self.duration_minutes * 60;
        self.target_at = None;
        self.session_id = None;
    }

    fn settle(&mut self, now: i64) -> bool {
        let Some(target) = self.target_at.filter(|&target| target <= now) else {
            return false;
        };
        if let Some(id) = self.session_id.take()
            && !self.history.iter().any(|item| item.id == id)
        {
            if self.history.len() == MAX_FOCUS_SESSIONS {
                self.history.remove(0);
            }
            let label = match self.kind {
                FocusKind::Focus => self.label.clone(),
                FocusKind::Break => String::new(),
            };
            self.history.push(Completion {
                id,
                label,
                minutes: self.duration_minutes,
                completed_at: target,
                kind: self.kind,
            });
        }
        self.target_at = None;
        self.remaining_seconds = 0;
        true
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Appearance {
    Light,
    Dark,
    #[default]
    System,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DesktopTodoBounds {
    /// Virtual-screen pixels keep negative monitor coordinates unambiguous.
    pub position: [i32; 2],
    /// Content size in logical points follows the user's display scale.
    pub size: [u32; 2],
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DesktopTodoSettings {
    pub open: bool,
    pub pinned: bool,
    pub bounds: Option<DesktopTodoBounds>,
    pub bubble_visible: bool,
    pub bubble_position: Option<[i32; 2]>,
}

impl Default for DesktopTodoSettings {
    fn default() -> Self {
        Self {
            open: false,
            pinned: true,
            bounds: None,
            bubble_visible: true,
            bubble_position: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Settings {
    pub always_on_top: bool,
    pub reduced_motion: bool,
    pub open_on_hover: bool,
    pub appearance: Appearance,
    pub display_device: Option<String>,
    #[serde(default)]
    pub dock: crate::dock::DockSettings,
    #[serde(default = "default_reminders")]
    pub reminders: bool,
    #[serde(default)]
    pub meeting_lead_minutes: u16,
    #[serde(default)]
    pub taskbar_notifications: bool,
    #[serde(default)]
    pub show_system_status: bool,
    #[serde(default)]
    pub capsule_device_metrics: bool,
    #[serde(default)]
    pub capsule_system_changes: bool,
    #[serde(default)]
    pub local_ai_usage: bool,
    #[serde(default)]
    pub capsule_ai_usage: bool,
    #[serde(default)]
    pub desktop_todo: DesktopTodoSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            always_on_top: true,
            reduced_motion: false,
            open_on_hover: true,
            appearance: Appearance::System,
            display_device: None,
            dock: Default::default(),
            reminders: true,
            meeting_lead_minutes: 0,
            taskbar_notifications: false,
            show_system_status: false,
            capsule_device_metrics: false,
            capsule_system_changes: false,
            local_ai_usage: false,
            capsule_ai_usage: false,
            desktop_todo: DesktopTodoSettings::default(),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Deserialize, Serialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct Workspace {
    format: String,
    version: u32,
    revision: u64,
    pub events: Vec<Event>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub calendar_sources: Vec<CalendarSource>,
    pub tasks: Vec<Task>,
    pub focus: Focus,
    #[serde(default, skip_serializing_if = "HealthReminders::is_default")]
    pub health: HealthReminders,
    pub notes: String,
    #[serde(default)]
    pub cards: Vec<Card>,
    #[serde(default)]
    pub meeting_notes: Vec<MeetingNote>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub dates: Vec<SavedDate>,
    pub settings: Settings,
    #[serde(default)]
    delivered_reminder_ids: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notifications: Vec<Notification>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub taskbar_notifications: Vec<TaskbarNotification>,
    #[serde(default)]
    private_task_titles: bool,
}

impl Default for Workspace {
    fn default() -> Self {
        Self {
            format: FORMAT.to_owned(),
            version: VERSION,
            revision: 0,
            events: Vec::new(),
            calendar_sources: Vec::new(),
            tasks: Vec::new(),
            focus: Focus::default(),
            health: HealthReminders::default(),
            notes: String::new(),
            cards: Vec::new(),
            meeting_notes: Vec::new(),
            dates: Vec::new(),
            settings: Settings::default(),
            delivered_reminder_ids: Vec::new(),
            notifications: Vec::new(),
            taskbar_notifications: Vec::new(),
            private_task_titles: false,
        }
    }
}

impl Workspace {
    fn validate(&self) -> Result<()> {
        if self.format != FORMAT || self.version != VERSION {
            return Err("工作区格式或版本暂不支持。".into());
        }
        if self.events.len() > MAX_RECORDS || self.tasks.len() > MAX_RECORDS {
            return Err("日程和待办各可保存 2,000 条。".into());
        }
        if !unique(self.events.iter().map(|item| item.id))
            || !unique(self.tasks.iter().map(|item| item.id))
        {
            return Err("日程或待办包含重复记录。".into());
        }
        for event in &self.events {
            validate_event(event)?;
        }
        calendar_sources::validate(&self.calendar_sources)?;
        self.settings.dock.validate()?;
        self.health.validate()?;
        for task in &self.tasks {
            if !valid_title(&task.title, 500)
                || !valid_title(&task.category, 64)
                || length(&task.notes) > 10_000
                || !valid_timestamp(task.created_at)
                || !task.due_at.is_none_or(valid_timestamp)
                || !task.completed_at.is_none_or(valid_timestamp)
                || (task.completed_at.is_some() && task.status != TaskStatus::Completed)
                || !task.valid_recurrence()
                || task.reminder_minutes.len() > 5
                || (task.due_at.is_none() && !task.reminder_minutes.is_empty())
                || task
                    .reminder_minutes
                    .iter()
                    .any(|minutes| *minutes > 10_080)
                || task.reminder_minutes.iter().collect::<HashSet<_>>().len()
                    != task.reminder_minutes.len()
            {
                return Err("待办的标题、备注或日期有误。".into());
            }
        }
        notes_extra::validate(&self.cards, &self.meeting_notes)?;
        if self.dates.len() > MAX_RECORDS {
            return Err("我的日子最多保存 2,000 条。".into());
        }
        if !unique(self.dates.iter().map(|item| item.id))
            || self.dates.iter().any(|item| !valid_title(&item.title, 120))
        {
            return Err("我的日子包含重复记录或无效名称。".into());
        }
        if length(&self.notes) > MAX_NOTES {
            return Err("便笺最多保存 10 万字符。".into());
        }
        if self.notifications.len() > MAX_NOTIFICATIONS
            || self.notifications.iter().any(|notice| {
                !valid_timestamp(notice.at)
                    || !valid_title(&notice.title, 100)
                    || length(&notice.body) > 2_000
            })
        {
            return Err("通知记录无效。".into());
        }
        if self.taskbar_notifications.len() > MAX_NOTIFICATIONS
            || self
                .taskbar_notifications
                .iter()
                .any(|notice| !valid_timestamp(notice.at))
        {
            return Err("任务栏提示记录无效。".into());
        }
        if self.delivered_reminder_ids.len() > reminders::MAX_DELIVERED
            || self.delivered_reminder_ids.iter().any(|id| id.len() > 180)
        {
            return Err("提醒记录超出工作区限制。".into());
        }
        let focus = &self.focus;
        if !(1..=180).contains(&focus.duration_minutes)
            || focus.remaining_seconds > 10_800
            || length(&focus.label) > 240
            || (focus.target_at.is_some() && focus.session_id.is_none())
            || !focus.target_at.is_none_or(valid_timestamp)
            || focus.history.len() > MAX_FOCUS_SESSIONS
            || !unique(focus.history.iter().map(|item| item.id))
            || focus.history.iter().any(|item| {
                !(1..=180).contains(&item.minutes)
                    || !valid_timestamp(item.completed_at)
                    || length(&item.label) > 240
            })
        {
            return Err("专注计时或历史记录有误。".into());
        }
        Ok(())
    }
}

pub(crate) fn validate_event(event: &Event) -> Result<()> {
    if event.title.trim().is_empty()
        || length(&event.title) > 240
        || length(&event.location) > 240
        || !valid_timestamp(event.start_at)
        || !valid_timestamp(event.end_at)
        || event.end_at <= event.start_at
    {
        return Err("请检查日程标题、地点和起止时间。".into());
    }
    if event.all_day {
        if event.start_date.map(utc_midnight) != Some(event.start_at)
            || event.end_date.map(utc_midnight) != Some(event.end_at)
        {
            return Err("全天日程需要有效的开始和结束日期。".into());
        }
    } else if event.start_date.is_some() || event.end_date.is_some() {
        return Err("定时日程请使用指定的起止时间。".into());
    }
    if event
        .url
        .as_deref()
        .is_some_and(|value| !valid_meeting_url(value))
    {
        return Err("请填写完整且不含账号密码的 HTTP 或 HTTPS 会议链接。".into());
    }
    if event.source.as_ref().is_some_and(|source| {
        source.uid.is_empty()
            || source.occurrence.is_empty()
            || length(&source.uid) > 1024
            || length(&source.occurrence) > 128
    }) {
        return Err("日程来源标识无效。".into());
    }
    Ok(())
}

pub(crate) fn valid_meeting_url(value: &str) -> bool {
    length(value) <= 2048
        && url::Url::parse(value).is_ok_and(|url| {
            matches!(url.scheme(), "http" | "https")
                && url.host().is_some()
                && url.username().is_empty()
                && url.password().is_none()
        })
}

const fn default_reminders() -> bool {
    true
}

fn utc_midnight(date: NaiveDate) -> i64 {
    date.and_time(chrono::NaiveTime::MIN)
        .and_utc()
        .timestamp_millis()
}

fn valid_timestamp(value: i64) -> bool {
    value >= 0 && DateTime::from_timestamp_millis(value).is_some()
}

fn valid_title(text: &str, max: usize) -> bool {
    !text.trim().is_empty() && !text.contains(['\r', '\n']) && length(text) <= max
}

fn length(text: &str) -> usize {
    text.encode_utf16().count()
}

fn unique(ids: impl Iterator<Item = Uuid>) -> bool {
    let mut seen = HashSet::new();
    ids.into_iter().all(|id| seen.insert(id))
}

#[derive(Debug, PartialEq, Eq)]
pub struct ImportSummary {
    pub events: usize,
    pub tasks: usize,
    pub focus_sessions: usize,
    pub has_notes: bool,
    pub cards: usize,
    pub meeting_notes: usize,
}

#[derive(Deserialize)]
struct LegacyNotes {
    text: String,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyTask {
    id: Uuid,
    title: String,
    category: String,
    completed: bool,
    due_at: Option<i64>,
    status: Option<TaskStatus>,
    #[serde(default)]
    priority: Priority,
    #[serde(default)]
    importance: Option<Importance>,
    #[serde(default)]
    urgency: Option<Urgency>,
    #[serde(default)]
    notes: String,
    created_at: i64,
    completed_at: Option<i64>,
    #[serde(default)]
    reminder_minutes: Vec<u32>,
    #[serde(default)]
    reminders_disabled: bool,
    #[serde(default)]
    repeat: todo::LegacyRepeat,
    generated_from: Option<Uuid>,
    pending_repeat_at: Option<i64>,
}

impl TryFrom<LegacyTask> for Task {
    type Error = String;

    fn try_from(task: LegacyTask) -> Result<Self> {
        let status = task.status.unwrap_or(if task.completed {
            TaskStatus::Completed
        } else {
            TaskStatus::Active
        });
        if (status == TaskStatus::Completed) != task.completed {
            return Err("旧工作区的待办完成状态不一致。".into());
        }
        let repeat = task.repeat.upgrade(task.due_at)?;
        let mut converted = Self {
            id: task.id,
            title: task.title,
            category: task.category,
            due_at: task.due_at,
            status,
            priority: task.priority,
            importance: task.importance,
            urgency: task.urgency,
            notes: task.notes,
            created_at: task.created_at,
            completed_at: task.completed_at.filter(|_| task.completed),
            reminder_minutes: task.reminder_minutes,
            reminders_disabled: task.reminders_disabled,
            repeat,
            repeat_start: task
                .due_at
                .and_then(todo::local_date)
                .filter(|_| repeat.is_repeating()),
            completed_dates: Vec::new(),
        };
        if repeat.is_repeating() && converted.status == TaskStatus::Completed {
            converted
                .completed_dates
                .extend(converted.completed_at.and_then(todo::local_date));
            converted.status = TaskStatus::Active;
            converted.completed_at = None;
        }
        Ok(converted)
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySettings {
    always_on_top: bool,
    reduced_motion: Option<bool>,
    open_on_hover: Option<bool>,
    appearance: Option<Appearance>,
    #[serde(default = "default_reminders")]
    reminders: bool,
}

impl From<LegacySettings> for Settings {
    fn from(settings: LegacySettings) -> Self {
        Self {
            always_on_top: settings.always_on_top,
            reduced_motion: settings.reduced_motion.unwrap_or(false),
            open_on_hover: settings.open_on_hover.unwrap_or(true),
            appearance: settings.appearance.unwrap_or(Appearance::Light),
            display_device: None,
            dock: Default::default(),
            reminders: settings.reminders,
            meeting_lead_minutes: 0,
            taskbar_notifications: false,
            show_system_status: false,
            capsule_device_metrics: false,
            capsule_system_changes: false,
            local_ai_usage: false,
            capsule_ai_usage: false,
            desktop_todo: DesktopTodoSettings::default(),
        }
    }
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyWorkspace {
    version: u32,
    revision: u64,
    todos: Vec<LegacyTask>,
    events: Vec<Event>,
    notes: LegacyNotes,
    #[serde(default)]
    meeting_notes: Vec<MeetingNote>,
    focus: Focus,
    settings: LegacySettings,
    #[serde(default)]
    delivered_reminder_ids: Vec<String>,
    todo_workspace: Option<LegacyTodoWorkspace>,
}

#[derive(Deserialize)]
struct LegacyTodoWorkspace {
    preferences: LegacyTodoPreferences,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacyTodoPreferences {
    #[serde(default)]
    hide_task_titles: bool,
}

impl TryFrom<LegacyWorkspace> for Workspace {
    type Error = String;

    fn try_from(legacy: LegacyWorkspace) -> Result<Self> {
        if legacy.version != 1 {
            return Err("旧工作区的格式版本暂不支持。".into());
        }
        let tasks = todo::upgrade_legacy_tasks(legacy.todos)?;
        Ok(Self {
            revision: legacy.revision.max(1),
            events: legacy.events,
            tasks,
            notes: legacy.notes.text,
            meeting_notes: legacy.meeting_notes,
            focus: legacy.focus,
            settings: legacy.settings.into(),
            delivered_reminder_ids: legacy
                .delivered_reminder_ids
                .into_iter()
                .filter(|id| id.starts_with("event:") || id.starts_with("todo:"))
                .collect(),
            private_task_titles: legacy
                .todo_workspace
                .is_some_and(|workspace| workspace.preferences.hide_task_titles),
            ..Self::default()
        })
    }
}

pub struct Store {
    primary: PathBuf,
    backup: PathBuf,
    state: Workspace,
    persisted: bool,
    schedule: Schedule,
    accepted_notification: Option<AcceptedNotification>,
    _lock: File,
}

impl Store {
    pub fn open(directory: PathBuf) -> Result<Self> {
        fs::create_dir_all(&directory).map_err(|err| format!("无法创建工作区目录（{err}）"))?;
        let lock = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(directory.join("workspace.lock"))
            .map_err(|err| format!("无法打开工作区锁（{err}）"))?;
        lock.try_lock()
            .map_err(|err| format!("无法锁定工作区，请关闭其他运行实例（{err}）"))?;

        let primary = directory.join("workspace.json");
        let backup = directory.join("workspace.json.previous");
        let (mut state, persisted) = load_workspace(&primary, &backup)?;
        calendar_sources::label_cached_sources(&mut state.calendar_sources, &state.events);
        let schedule = Schedule::from_workspace(&state);
        Ok(Self {
            primary,
            backup,
            state,
            persisted,
            schedule,
            accepted_notification: None,
            _lock: lock,
        })
    }

    pub fn state(&self) -> &Workspace {
        &self.state
    }

    #[cfg(windows)]
    pub fn directory(&self) -> &Path {
        self.primary
            .parent()
            .expect("workspace file has a profile directory")
    }

    pub fn update(&mut self, mutate: impl FnOnce(&mut Workspace) -> Result<()>) -> Result<()> {
        let mut next = self.state.clone();
        mutate(&mut next)?;
        if next == self.state {
            return Ok(());
        }
        next.revision = self
            .state
            .revision
            .checked_add(1)
            .ok_or("工作区版本计数已到上限。")?;
        self.commit(next)
    }

    pub fn tick(&mut self, now: i64) -> Result<bool> {
        if self.state.focus.target_at.is_none_or(|target| target > now) {
            return Ok(false);
        }
        self.update(|state| {
            state.focus.settle(now);
            Ok(())
        })?;
        Ok(true)
    }

    pub fn next_reminder_at(&self, now: i64) -> Option<i64> {
        if self.accepted_notification.is_none() {
            self.schedule
                .next_at(now)
                .into_iter()
                .chain(self.next_health_reminder_at())
                .min()
        } else {
            Some(now)
        }
    }

    pub fn next_health_reminder_at(&self) -> Option<i64> {
        self.state
            .settings
            .reminders
            .then(|| {
                self.state
                    .health
                    .deadlines()
                    .map(|(_, deadline)| deadline)
                    .min()
            })
            .flatten()
    }

    pub fn deliver_due_reminders(
        &mut self,
        now: i64,
        send: impl FnOnce(&Reminder) -> bool,
    ) -> Result<()> {
        self.save_accepted_notification()?;
        if let Some((ids, reminder)) = self.schedule.due(now) {
            if send(&reminder) {
                self.save_notification(now, ids, reminder)?;
            }
        } else if self.state.settings.reminders {
            if self.state.health.deadlines().any(|(_, at)| at <= now)
                && self.state.health.next_allowed(now) > now
            {
                self.update(|state| {
                    state.health.fit_deadlines(now);
                    Ok(())
                })?;
            }
            let deadlines: Vec<_> = self
                .state
                .health
                .deadlines()
                .filter(|(_, deadline)| *deadline <= now)
                .collect();
            if let Some(reminder) = health::notification(&deadlines)
                && send(&reminder)
            {
                self.queue_notification(now, Vec::new(), reminder);
                self.accepted_notification
                    .as_mut()
                    .expect("queued")
                    .health_deadlines = deadlines;
                self.save_accepted_notification()?;
            }
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn deliver_focus_notification(
        &mut self,
        now: i64,
        send: impl FnOnce(&Reminder) -> bool,
    ) -> Result<bool> {
        self.save_accepted_notification()?;
        let reminder = Reminder {
            title: "专注计时结束".into(),
            body: "本次计时已保存到专注记录。".into(),
        };
        if !send(&reminder) {
            return Ok(false);
        }
        self.queue_notification(now, Vec::new(), reminder);
        Ok(true)
    }

    fn save_notification(
        &mut self,
        now: i64,
        reminder_ids: Vec<String>,
        reminder: Reminder,
    ) -> Result<()> {
        self.queue_notification(now, reminder_ids, reminder);
        self.save_accepted_notification()
    }

    fn queue_notification(&mut self, now: i64, reminder_ids: Vec<String>, reminder: Reminder) {
        self.accepted_notification = Some(AcceptedNotification {
            reminder_ids,
            health_deadlines: Vec::new(),
            notification: Notification {
                at: now,
                title: reminder.title,
                body: reminder.body,
            },
        });
    }

    fn save_accepted_notification(&mut self) -> Result<()> {
        if let Some(accepted) = &self.accepted_notification {
            // Retry the write without showing an already accepted notification again.
            let ids = accepted.reminder_ids.clone();
            let notification = accepted.notification.clone();
            let health_deadlines = accepted.health_deadlines.clone();
            self.update(|state| {
                for (kind, deadline) in health_deadlines {
                    let timer = state.health.timer_mut(kind);
                    if timer.next_at == Some(deadline) {
                        timer.schedule(notification.at);
                    }
                }
                reminders::mark_delivered(&mut state.delivered_reminder_ids, ids);
                state.health.fit_deadlines(notification.at);
                state.notifications.insert(0, notification);
                state.notifications.truncate(MAX_NOTIFICATIONS);

                Ok(())
            })?;
            self.accepted_notification = None;
        }
        Ok(())
    }

    #[cfg(windows)]
    pub fn record_taskbar_notifications(
        &mut self,
        notifications: Vec<TaskbarNotification>,
    ) -> Result<()> {
        self.update(|state| {
            for notification in notifications {
                let duplicate = state.taskbar_notifications.iter().any(|previous| {
                    previous.app == notification.app
                        && previous.at.abs_diff(notification.at) < 5_000
                });
                if !duplicate {
                    state.taskbar_notifications.insert(0, notification);
                }
            }
            state.taskbar_notifications.truncate(MAX_NOTIFICATIONS);
            Ok(())
        })
    }

    pub fn can_import(&self) -> bool {
        self.state.revision == 0
    }

    pub fn import_legacy(&mut self, path: &Path) -> Result<ImportSummary> {
        if !self.can_import() {
            return Err("当前 Rust 工作区已有修改，请使用新的工作区导入。".into());
        }
        let legacy = read_json::<LegacyWorkspace>(path)
            .map_err(|err| format!("无法读取旧版工作区（{err}）"))?;
        let mut next = Workspace::try_from(legacy)?;
        let cards_path = path.with_file_name("productivity.v1.json");
        match read_json::<notes_extra::LegacyProductivity>(&cards_path) {
            Ok(notes) if notes.version == 1 => next.cards = notes.cards,
            Ok(_) => return Err("旧版卡片的格式版本暂不支持。".into()),
            Err(error) if error.kind() == io::ErrorKind::NotFound => {}
            Err(error) => return Err(format!("无法读取旧版卡片（{error}）")),
        }
        next.calendar_sources = calendar_sources::migrate(path, &next.events);
        next.validate()?;
        let now = chrono::Utc::now().timestamp_millis();
        let elapsed = Schedule::from_workspace(&next).elapsed_keys(now);
        reminders::mark_delivered(&mut next.delivered_reminder_ids, elapsed);
        let summary = ImportSummary {
            events: next.events.len(),
            tasks: next.tasks.len(),
            focus_sessions: next.focus.history.len(),
            has_notes: !next.notes.is_empty(),
            cards: next.cards.len(),
            meeting_notes: next.meeting_notes.len(),
        };
        self.commit(next)?;
        Ok(summary)
    }

    fn commit(&mut self, next: Workspace) -> Result<()> {
        next.validate()?;
        if self.persisted {
            write_atomic(&self.backup, &self.state)
                .map_err(|err| format!("无法保存工作区恢复副本（{err}）"))?;
        }
        write_atomic(&self.primary, &next).map_err(|err| format!("无法保存工作区（{err}）"))?;
        self.state = next;
        self.schedule = Schedule::from_workspace(&self.state);
        self.persisted = true;
        Ok(())
    }
}

fn load_workspace(primary: &Path, backup: &Path) -> Result<(Workspace, bool)> {
    let primary_record = read_json::<serde_json::Value>(primary);
    if let Ok(value) = &primary_record {
        let found_format = value.get("format").and_then(serde_json::Value::as_str);
        let found_version = value.get("version").and_then(serde_json::Value::as_u64);
        let foreign_format = found_format.is_some_and(|format| format != FORMAT)
            || (found_format.is_none() && value.get("todos").is_some());
        let unknown_version = found_version.is_some_and(|version| version != u64::from(VERSION));
        if foreign_format || unknown_version {
            return Err("工作区格式或版本暂不支持。原文件已保留。".into());
        }
    }
    let missing = primary_record
        .as_ref()
        .is_err_and(|err| err.kind() == io::ErrorKind::NotFound);
    let failure = match primary_record {
        Ok(value) => match serde_json::from_value::<Workspace>(value) {
            Ok(state) => match state.validate() {
                Ok(()) => return Ok((state, true)),
                Err(err) => err,
            },
            Err(err) => err.to_string(),
        },
        Err(err) => err.to_string(),
    };

    let recovered = match read_json::<Workspace>(backup) {
        Ok(state) => state,
        Err(err) if missing && err.kind() == io::ErrorKind::NotFound => {
            return Ok((Workspace::default(), false));
        }
        Err(err) => {
            return Err(format!(
                "无法读取工作区及恢复副本。原文件已保留（{failure}；{err}）"
            ));
        }
    };
    recovered.validate()?;
    if !missing {
        let archived = primary.with_file_name(format!("workspace.corrupt-{}.json", Uuid::new_v4()));
        fs::rename(primary, archived).map_err(|err| format!("无法保留损坏的工作区（{err}）"))?;
    }
    write_atomic(primary, &recovered).map_err(|err| format!("无法恢复工作区（{err}）"))?;
    Ok((recovered, true))
}

fn read_json<T: DeserializeOwned>(path: &Path) -> io::Result<T> {
    let mut data = Vec::new();
    File::open(path)?
        .take(MAX_FILE_BYTES + 1)
        .read_to_end(&mut data)?;
    if data.len() as u64 > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "数据文件超过 32 MiB",
        ));
    }
    serde_json::from_slice(&data).map_err(io::Error::other)
}

fn write_atomic(path: &Path, state: &Workspace) -> io::Result<()> {
    workspace_tempfile(path, state)?
        .persist(path)
        .map_err(|error| error.error)?;
    Ok(())
}

fn workspace_tempfile(path: &Path, state: &Workspace) -> io::Result<NamedTempFile> {
    let directory = path
        .parent()
        .ok_or_else(|| io::Error::other("缺少工作区目录"))?;
    let mut temp = NamedTempFile::new_in(directory)?;
    serde_json::to_writer(&mut temp, state).map_err(io::Error::other)?;
    temp.write_all(b"\n")?;
    if temp.as_file().metadata()?.len() > MAX_FILE_BYTES {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "数据文件超过 32 MiB",
        ));
    }
    temp.as_file().sync_all()?;
    Ok(temp)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use tempfile::tempdir;

    fn legacy_state() -> serde_json::Value {
        json!({
            "version": 1,
            "revision": 12,
            "todos": [
                {
                    "id": "7e1d304e-73bc-41be-9a19-b2fb865e1089",
                    "title": "检查草稿", "category": "work", "completed": false,
                    "dueAt": 1764547200000i64, "status": "deferred", "priority": "high",
                    "notes": "依照会议记录", "createdAt": 1764460800000i64,
                    "steps": [{ "title": "原版子任务" }]
                },
                {
                    "id": "b9d9becb-93e8-496b-bb28-ca44007e15e6",
                    "title": "已完成的旧任务", "category": "none", "completed": true,
                    "dueAt": null, "createdAt": 1764460800000i64
                }
            ],
            "events": [
                {
                    "id": "3c44db4d-dc4c-413d-adff-279964886414",
                    "title": "全天日程", "location": "办公室", "allDay": true,
                    "startDate": "2025-12-01", "endDate": "2025-12-02",
                    "startAt": 1764547200000i64, "endAt": 1764633600000i64,
                    "subscriptionId": "212dda8e-4e0e-4e1f-b1c1-38be9fc495fb"
                },
                {
                    "id": "70c42abe-f763-4b20-84a2-7081c4bdd0eb",
                    "title": "视频会议", "location": "线上",
                    "startAt": 1764547200000i64, "endAt": 1764550800000i64,
                    "url": "https://example.com/meeting"
                }
            ],
            "notes": { "text": "旧版便笺", "updatedAt": 1764460800000i64 },
            "focus": {
                "durationMinutes": 25, "remainingSeconds": 1500, "targetAt": 1764547200000i64,
                "sessionId": "23ee0085-aa79-4b71-a4c4-adbd9085f5f3", "label": "专注写作", "history": []
            },
            "settings": { "alwaysOnTop": false, "displayId": 123456 },
            "meetingNotes": [{
                "key": "local:ee1d304e-73bc-41be-9a19-b2fb865e1089",
                "eventId": "ee1d304e-73bc-41be-9a19-b2fb865e1089",
                "title": "评审", "body": "旧版会议笔记",
                "startsAt": 1764547200000i64, "updatedAt": 1764547200000i64
            }]
        })
    }

    #[test]
    fn imports_existing_workspace_once_and_preserves_original_bytes() {
        let legacy = tempdir().unwrap();
        let source = legacy.path().join("workspace.json");
        let original = serde_json::to_vec(&legacy_state()).unwrap();
        fs::write(&source, &original).unwrap();
        let cards_path = source.with_file_name("productivity.v1.json");
        let cards = serde_json::to_vec(&json!({
            "version": 1,
            "revision": 1,
            "folders": [],
            "bookmarks": [],
            "cards": [{
                "id": "old-card-1",
                "title": "旧卡片\n第二行",
                "text": "保留旧版卡片内容",
                "updatedAt": 1764460800000.5
            }]
        }))
        .unwrap();
        fs::write(&cards_path, &cards).unwrap();
        let native = tempdir().unwrap();
        let mut store = Store::open(native.path().to_owned()).unwrap();

        assert_eq!(
            store.import_legacy(&source).unwrap(),
            ImportSummary {
                events: 2,
                tasks: 2,
                focus_sessions: 0,
                has_notes: true,
                cards: 1,
                meeting_notes: 1,
            }
        );
        assert_eq!(store.state().tasks[0].status, TaskStatus::Deferred);
        assert_eq!(store.state().tasks[0].priority, Priority::High);
        assert_eq!(store.state().tasks[1].status, TaskStatus::Completed);
        assert_eq!(store.state().notes, "旧版便笺");
        assert_eq!(store.state().cards[0].id, "old-card-1");
        assert_eq!(store.state().cards[0].title, "旧卡片\n第二行");
        assert_eq!(store.state().cards[0].text, "保留旧版卡片内容");
        assert_eq!(
            store.state().cards[0].updated_at.as_f64(),
            Some(1764460800000.5)
        );
        assert_eq!(store.state().settings.appearance, Appearance::Light);
        assert!(!store.state().settings.always_on_top);
        assert!(store.state().settings.display_device.is_none());
        assert!(store.state().events[0].subscription_id.is_some());
        assert_eq!(
            store.state().events[0].start_date,
            NaiveDate::from_ymd_opt(2025, 12, 1)
        );
        assert!(store.tick(1764547200010).unwrap());
        assert!(!store.tick(1764547200020).unwrap());
        assert_eq!(store.state().focus.history[0].completed_at, 1764547200000);
        assert!(store.import_legacy(&source).is_err());
        let state = store.state().clone();
        drop(store);
        assert_eq!(
            Store::open(native.path().to_owned()).unwrap().state(),
            &state
        );
        assert_eq!(fs::read(&source).unwrap(), original);
        assert_eq!(fs::read(&cards_path).unwrap(), cards);
    }

    #[test]
    fn rejects_invalid_import_and_does_not_consume_import_opportunity() {
        let native = tempdir().unwrap();
        let legacy = tempdir().unwrap();
        let source = legacy.path().join("workspace.json");
        let mut invalid = legacy_state();
        invalid["events"][1]["url"] = json!("file:///C:/Windows");
        fs::write(&source, serde_json::to_vec(&invalid).unwrap()).unwrap();
        let mut store = Store::open(native.path().to_owned()).unwrap();
        assert!(store.import_legacy(&source).is_err());
        assert!(store.can_import());
        assert!(!native.path().join("workspace.json").exists());
    }

    #[test]
    fn unusable_write_does_not_publish_transaction() {
        let directory = tempdir().unwrap();
        let mut store = Store::open(directory.path().to_owned()).unwrap();
        fs::create_dir(&store.primary).unwrap();
        assert!(
            store
                .update(|state| {
                    state.notes = "保存失败，文本留在编辑器".into();
                    Ok(())
                })
                .is_err()
        );
        assert!(store.state().notes.is_empty());
        assert!(store.can_import());
        assert!(store.primary.is_dir());
    }

    #[test]
    fn recovers_last_valid_snapshot_without_discarding_corrupt_file() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path().to_owned()).unwrap();
        for notes in ["第一份记录", "第二份记录"] {
            store
                .update(|state| {
                    state.notes = notes.to_owned();
                    Ok(())
                })
                .unwrap();
        }
        fs::write(&store.primary, "损坏的内容").unwrap();
        assert!(Store::open(dir.path().to_owned()).is_err());
        drop(store);

        let store = Store::open(dir.path().to_owned()).unwrap();
        assert_eq!(store.state().notes, "第一份记录");
        let archive = fs::read_dir(dir.path())
            .unwrap()
            .flatten()
            .find(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with("workspace.corrupt-")
            })
            .unwrap();
        assert_eq!(fs::read_to_string(archive.path()).unwrap(), "损坏的内容");
    }

    #[test]
    fn refuses_to_downgrade_a_newer_workspace_using_older_recovery_data() {
        let dir = tempdir().unwrap();
        let backup = dir.path().join("workspace.json.previous");
        write_atomic(&backup, &Workspace::default()).unwrap();
        let primary = dir.path().join("workspace.json");
        let future = json!({ "format": FORMAT, "version": VERSION + 1 });
        let original = serde_json::to_vec(&future).unwrap();
        fs::write(&primary, &original).unwrap();
        assert!(Store::open(dir.path().to_owned()).is_err());
        assert_eq!(fs::read(primary).unwrap(), original);
    }

    #[test]
    fn focus_uses_an_absolute_deadline_across_pause_resume_and_restarts() {
        let mut focus = Focus::default();
        focus.configure(5, "写稿".into(), FocusKind::Focus).unwrap();
        focus.start(1_000);
        let session = focus.session_id.unwrap();
        focus.pause(61_000);
        assert_eq!(focus.seconds_left(600_000), 240);
        focus.start(600_000);
        assert_eq!(focus.session_id, Some(session));
        assert_eq!(focus.target_at, Some(840_000));
        assert!(focus.settle(841_000));
        assert!(!focus.settle(842_000));
        assert_eq!(focus.history[0].id, session);
        assert_eq!(focus.history[0].completed_at, 840_000);
    }

    #[test]
    fn migrated_reminders_preserve_offsets_privacy_and_delivery_state_across_restarts() {
        let now = chrono::Utc::now().timestamp_millis();
        let mut data = legacy_state();
        data["todos"][0]["status"] = json!("active");
        data["todos"][0]["dueAt"] = json!(now + 20 * 60_000);
        data["todos"][0]["reminderMinutes"] = json!([15, 0]);
        data["todoWorkspace"] = json!({"preferences": {"hideTaskTitles": true}});
        let event_at = now + 20 * 60_000;
        data["events"][1]["startAt"] = json!(event_at);
        data["events"][1]["endAt"] = json!(event_at + 60_000);
        let event_id = data["events"][1]["id"].as_str().unwrap();
        data["deliveredReminderIds"] = json!([format!("event:{event_id}:{event_at}")]);
        let legacy = tempdir().unwrap();
        let source = legacy.path().join("workspace.json");
        fs::write(&source, serde_json::to_vec(&data).unwrap()).unwrap();

        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path().to_owned()).unwrap();
        store.import_legacy(&source).unwrap();
        assert_eq!(store.next_reminder_at(now), Some(now + 5 * 60_000));
        store
            .deliver_due_reminders(now, |_| panic!("reminder sent early"))
            .unwrap();
        let first_reminder_at = now + 5 * 60_000;
        let delivered_before_rejection = store.state().delivered_reminder_ids.clone();
        let mut rejected = false;
        store
            .deliver_due_reminders(first_reminder_at, |_| {
                rejected = true;
                false
            })
            .unwrap();
        assert!(rejected);
        assert_eq!(
            store.state().delivered_reminder_ids,
            delivered_before_rejection
        );
        assert_eq!(
            store.next_reminder_at(first_reminder_at),
            Some(first_reminder_at)
        );
        let mut reminders = Vec::new();
        store
            .deliver_due_reminders(first_reminder_at, |reminder| {
                reminders.push(reminder.clone());
                true
            })
            .unwrap();
        assert_eq!(
            reminders,
            vec![Reminder {
                title: "待办提醒".into(),
                body: "有一项待办需要关注".into()
            }]
        );
        drop(store);

        let mut store = Store::open(dir.path().to_owned()).unwrap();
        store
            .deliver_due_reminders(first_reminder_at, |_| panic!("repeated persisted reminder"))
            .unwrap();
        assert_eq!(store.next_reminder_at(first_reminder_at), Some(event_at));
        let mut next_reminders = 0;
        store
            .deliver_due_reminders(event_at, |_| {
                next_reminders += 1;
                true
            })
            .unwrap();
        store
            .deliver_due_reminders(event_at, |_| panic!("repeated reminder"))
            .unwrap();
        assert_eq!(next_reminders, 1);
    }

    #[test]
    fn reminders_coalesce_on_resume_without_repeating_all_day_or_stale_items() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path().to_owned()).unwrap();
        let data = serde_json::from_value::<LegacyWorkspace>(legacy_state()).unwrap();
        let migrated = Workspace::try_from(data).unwrap();
        let at = migrated.events[1].start_at;
        store
            .update(|state| {
                state.events = migrated.events;
                state.tasks = migrated.tasks;
                state.tasks[0].due_at = Some(at);
                state.tasks[0].status = TaskStatus::Active;
                let mut old = state.events[1].clone();
                old.id = Uuid::new_v4();
                old.start_at -= 31 * 60_000;
                old.end_at -= 31 * 60_000;
                state.events.push(old);
                Ok(())
            })
            .unwrap();
        let reminders_due = at + 60_000;
        let mut reminders = Vec::new();
        store
            .deliver_due_reminders(reminders_due, |reminder| {
                reminders.push(reminder.clone());
                true
            })
            .unwrap();
        assert_eq!(
            reminders,
            vec![Reminder {
                title: "NotchCalendar 提醒".into(),
                body: "1 场日程已开始，1 项待办需要关注".into(),
            }]
        );
        assert_eq!(store.next_reminder_at(reminders_due), None);
    }

    #[test]
    fn a_save_failure_does_not_resend_an_accepted_reminder() {
        let dir = tempdir().unwrap();
        let mut store = Store::open(dir.path().to_owned()).unwrap();
        let data = serde_json::from_value::<LegacyWorkspace>(legacy_state()).unwrap();
        let migrated = Workspace::try_from(data).unwrap();
        let at = migrated.events[1].start_at;
        store
            .update(|state| {
                state.events = migrated.events;
                Ok(())
            })
            .unwrap();
        fs::create_dir(&store.backup).unwrap();
        let mut notified = false;
        assert!(
            store
                .deliver_due_reminders(at, |_| {
                    notified = true;
                    true
                })
                .is_err()
        );
        assert!(notified);
        assert!(store.state().delivered_reminder_ids.is_empty());
        assert!(store.state().notifications.is_empty());
        assert_eq!(store.next_reminder_at(at), Some(at));
        assert!(
            store
                .deliver_due_reminders(at, |_| panic!("repeated reminder after save failure"))
                .is_err()
        );

        fs::remove_dir(&store.backup).unwrap();
        store
            .deliver_due_reminders(at, |_| panic!("repeated reminder on save retry"))
            .unwrap();
        assert_eq!(store.next_reminder_at(at), None);
        assert_eq!(store.state().notifications.len(), 1);
        drop(store);
        let mut store = Store::open(dir.path().to_owned()).unwrap();
        assert_eq!(store.state().notifications.len(), 1);
        store
            .deliver_due_reminders(at, |_| panic!("repeated reminder after restart"))
            .unwrap();
    }
}

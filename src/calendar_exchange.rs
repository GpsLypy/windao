use crate::workspace::{CalendarSourceIdentity, Event, Store, valid_meeting_url, validate_event};
use chrono::{
    DateTime, Datelike, Days, Local, NaiveDate, NaiveDateTime, Offset, SecondsFormat, TimeZone, Utc,
};
use ical::IcalParser;
use ical::parser::ical::component::IcalEvent;
use ical::property::Property;
use rrule::{Frequency, RRule, RRuleSet, Tz, Unvalidated};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::fs::File;
use std::io::{Cursor, Read, Write};
use std::path::Path;
use tempfile::NamedTempFile;
use uuid::Uuid;

pub(crate) const MAX_BYTES: u64 = 2 * 1024 * 1024;
const MAX_EVENTS: usize = 2_000;
const MAX_STEPS: usize = 20_000;
const MAX_WARNINGS: usize = 100;
const DAY_MILLIS: i64 = 86_400_000;
const LOCAL_UID_SUFFIX: &str = "@notchcalendar.windows";

pub struct ImportPreview {
    pub added: usize,
    pub updated: usize,
    pub unchanged: usize,
    pub skipped: usize,
    pub warnings: Vec<String>,
    base: Vec<Event>,
    events: Vec<Event>,
}

impl ImportPreview {
    pub fn apply(self, store: &mut Store) -> Result<(), String> {
        store.update(move |state| {
            if state.events != self.base {
                return Err("日程在预览期间已更新，请重新打开导入预览。".into());
            }
            state.events = self.events;
            Ok(())
        })
    }

    fn warn(&mut self, warning: String) {
        if self.warnings.len() < MAX_WARNINGS {
            self.warnings.push(warning);
        }
    }
}

pub fn preview_import(path: &Path, existing: &[Event], now: i64) -> Result<ImportPreview, String> {
    let mut input = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BYTES + 1).read_to_end(&mut input))
        .map_err(|err| format!("无法读取日历文件（{err}）"))?;
    if input.len() as u64 > MAX_BYTES {
        return Err("日历文件不能超过 2 MiB。".into());
    }
    let input = std::str::from_utf8(&input).map_err(|_| "日历文件应使用 UTF-8 编码。")?;
    parse(input.trim_start_matches('\u{feff}'), existing, now)
}

fn parse(input: &str, existing: &[Event], now: i64) -> Result<ImportPreview, String> {
    parse_calendar(input, existing, now, false)
}

pub(crate) fn parse_snapshot(input: &str, now: i64) -> Result<Vec<Event>, String> {
    let preview = parse_calendar(input.trim_start_matches('\u{feff}'), &[], now, true)?;
    if preview.skipped > 0 {
        return Err("日历来源包含无法读取的日程，已保留上次同步结果。".into());
    }
    Ok(preview.events)
}

fn parse_calendar(
    input: &str,
    existing: &[Event],
    now: i64,
    allow_empty: bool,
) -> Result<ImportPreview, String> {
    let mut parser = IcalParser::new(Cursor::new(input.as_bytes()));
    let calendar = parser
        .next()
        .and_then(Result::ok)
        .filter(|_| parser.next().is_none())
        .ok_or("无法读取这份 ICS 文件，请从原日历重新导出。")?;
    if property_value(&calendar.properties, "METHOD")
        .is_some_and(|value| value.eq_ignore_ascii_case("CANCEL"))
    {
        return Err("请选择日历快照，取消通知无法导入。".into());
    }
    if (calendar.events.is_empty() && !allow_empty) || calendar.events.len() > MAX_EVENTS {
        return Err("请选择含 1–2,000 个 VEVENT 日程组件的文件。".into());
    }
    if allow_empty && property_value(&calendar.properties, "VERSION") != Some("2.0") {
        return Err("日历服务器应返回 iCalendar 2.0 格式。".into());
    }
    let mut preview = ImportPreview {
        added: 0,
        updated: 0,
        unchanged: 0,
        skipped: 0,
        warnings: Vec::new(),
        base: existing.to_vec(),
        events: existing.to_vec(),
    };
    let mut groups = BTreeMap::<String, Vec<&IcalEvent>>::new();
    for event in &calendar.events {
        let uid = property_value(&event.properties, "UID")
            .map(decode_text)
            .unwrap_or_default();
        let uid = uid.trim();
        if uid.is_empty() || uid.encode_utf16().count() > 1024 {
            preview.skipped += 1;
            preview.warn("跳过一条缺少有效 UID 的日程。".into());
            continue;
        }
        groups.entry(uid.to_owned()).or_default().push(event);
    }

    let range = Horizon::new(now)?;
    let mut total_steps = 0;
    let mut imported = Vec::new();
    for (uid, components) in groups {
        let summary = event_title(&components[0].properties);
        match expand(&uid, &components, &range, &mut total_steps, &mut preview) {
            Ok(events) => imported.extend(events),
            Err(error) => {
                preview.skipped += 1;
                preview.warn(format!("跳过「{}」：{error}", limit_text(&summary, 70)));
            }
        }
        if total_steps > MAX_STEPS || imported.len() > MAX_EVENTS {
            return Err("日程重复计算超过 20,000 次或日程超过 2,000 条，请分批导出。".into());
        }
    }
    merge(&mut preview, imported)?;
    Ok(preview)
}

fn merge(preview: &mut ImportPreview, imported: Vec<Event>) -> Result<(), String> {
    let mut by_id: HashMap<_, _> = preview
        .events
        .iter()
        .enumerate()
        .map(|(index, event)| (event.id, index))
        .collect();
    let mut by_source: HashMap<_, _> = preview
        .events
        .iter()
        .enumerate()
        .filter_map(|(index, event)| event.source.clone().map(|source| (source, index)))
        .collect();

    let mut seen_ids = HashSet::new();
    let mut seen_sources = HashSet::new();
    for mut candidate in imported {
        if !seen_ids.insert(candidate.id)
            || candidate
                .source
                .as_ref()
                .is_some_and(|source| !seen_sources.insert(source.clone()))
        {
            preview.skipped += 1;
            preview.warn("跳过文件中重复的日程标识。".into());
            continue;
        }
        let index = candidate
            .source
            .as_ref()
            .and_then(|key| by_source.get(key))
            .or_else(|| by_id.get(&candidate.id))
            .copied();
        let target = if let Some(index) = index {
            let present = &preview.events[index];
            if present.subscription_id.is_some() {
                preview.skipped += 1;
                preview.warn(format!(
                    "保留只读订阅快照「{}」。",
                    limit_text(&present.title, 70)
                ));
                continue;
            }
            candidate.id = present.id;
            candidate.source = candidate.source.or_else(|| present.source.clone());
            if &candidate == present {
                preview.unchanged += 1;
                continue;
            }
            if let Some(previous) = &present.source {
                by_source.remove(previous);
            }
            preview.updated += 1;
            index
        } else {
            if preview.events.len() == MAX_EVENTS {
                return Err("合并后超过 2,000 条日程，请缩小导入范围。".into());
            }
            preview.added += 1;
            preview.events.len()
        };
        by_id.insert(candidate.id, target);
        if let Some(source) = &candidate.source {
            by_source.insert(source.clone(), target);
        }
        if target == preview.events.len() {
            preview.events.push(candidate);
        } else {
            preview.events[target] = candidate;
        }
    }
    Ok(())
}

#[derive(Clone, Copy)]
enum Time {
    Day(NaiveDate),
    Instant(DateTime<Tz>),
}

impl Time {
    fn parse(value: &str, property: &Property) -> Result<Self, String> {
        let tzid = parameter(property, "TZID");
        let declared_date =
            parameter(property, "VALUE").is_some_and(|value| value.eq_ignore_ascii_case("DATE"));
        let result = if declared_date || value.len() == 8 {
            if tzid.is_some() {
                return Err("全天日期不能包含时区。".into());
            }
            let date =
                NaiveDate::parse_from_str(value, "%Y%m%d").map_err(|_| "全天日期格式无效。")?;
            Self::Day(date)
        } else {
            let (value, zone) = match value.strip_suffix('Z') {
                Some(_) if tzid.is_some() => return Err("UTC 时间不能另设时区。".into()),
                Some(value) => (value, Tz::UTC),
                None => (value, timezone(tzid)?),
            };
            let datetime = NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%S")
                .map_err(|_| "开始或结束时间格式无效。")?;
            let instant = zone
                .from_local_datetime(&datetime)
                .earliest()
                .ok_or("日程时间落在时钟调整的空档内。")?;
            Self::Instant(instant)
        };
        if !(1970..=2100).contains(&result.date().year()) {
            return Err("支持 1970–2100 年的日程。".into());
        }
        Ok(result)
    }

    fn date(self) -> NaiveDate {
        match self {
            Self::Day(date) => date,
            Self::Instant(time) => time.date_naive(),
        }
    }

    fn recurrence_date(self) -> DateTime<Tz> {
        match self {
            Self::Day(date) => date
                .and_time(chrono::NaiveTime::MIN)
                .and_utc()
                .with_timezone(&Tz::UTC),
            Self::Instant(time) => time,
        }
    }

    fn millis(self) -> i64 {
        self.recurrence_date().timestamp_millis()
    }

    fn occurrence_key(self) -> String {
        match self {
            Self::Day(date) => date.to_string(),
            Self::Instant(date) if date.timezone().is_local() => {
                format!("{}@floating", date.format("%Y-%m-%dT%H:%M:%S"))
            }
            Self::Instant(date) => date
                .with_timezone(&Utc)
                .to_rfc3339_opts(SecondsFormat::Millis, true),
        }
    }

    fn from_recurrence(master: Self, occurrence: DateTime<Tz>) -> Self {
        match master {
            Self::Day(_) => Self::Day(occurrence.date_naive()),
            Self::Instant(_) => Self::Instant(occurrence),
        }
    }

    fn add_millis(self, millis: i64) -> Result<Self, String> {
        match self {
            Self::Day(date) if millis > 0 && millis % DAY_MILLIS == 0 => date
                .checked_add_days(Days::new((millis / DAY_MILLIS) as u64))
                .map(Self::Day),
            Self::Day(_) => None,
            Self::Instant(time) => chrono::TimeDelta::try_milliseconds(millis)
                .and_then(|delta| time.checked_add_signed(delta))
                .map(Self::Instant),
        }
        .ok_or_else(|| "日程结束时间超出范围。".into())
    }
}

fn timezone(tzid: Option<&str>) -> Result<Tz, String> {
    let Some(tzid) = tzid else {
        return Ok(Tz::LOCAL);
    };
    tzid.parse::<chrono_tz::Tz>()
        .map(Into::into)
        .map_err(|_| format!("时区「{}」无法识别。", limit_text(tzid, 90)))
}

struct Horizon {
    start: i64,
    end: i64,
    first_day: NaiveDate,
    last_day_exclusive: NaiveDate,
}

impl Horizon {
    fn new(now: i64) -> Result<Self, String> {
        let today = DateTime::from_timestamp_millis(now)
            .ok_or("当前时间超出范围。")?
            .with_timezone(&Local)
            .date_naive();
        let first_day = today
            .checked_sub_days(Days::new(30))
            .ok_or("当前时间超出范围。")?;
        let last_day_exclusive = today
            .checked_add_days(Days::new(366))
            .ok_or("当前时间超出范围。")?;
        let boundary = |date: NaiveDate| {
            let time = date.and_time(chrono::NaiveTime::MIN);
            Local
                .from_local_datetime(&time)
                .earliest()
                .map_or(time.and_utc().timestamp_millis(), |value| {
                    value.timestamp_millis()
                })
        };
        Ok(Self {
            start: boundary(first_day),
            end: boundary(last_day_exclusive),
            first_day,
            last_day_exclusive,
        })
    }

    fn includes(&self, start: Time, end: Time) -> bool {
        match (start, end) {
            (Time::Day(start), Time::Day(end)) => {
                start < self.last_day_exclusive && end > self.first_day
            }
            _ => start.millis() < self.end && end.millis() > self.start,
        }
    }
}

fn expand(
    uid: &str,
    components: &[&IcalEvent],
    horizon: &Horizon,
    total_steps: &mut usize,
    preview: &mut ImportPreview,
) -> Result<Vec<Event>, String> {
    let mut masters = Vec::new();
    let mut exceptions = BTreeMap::<String, &IcalEvent>::new();
    for component in components {
        if let Some(property) = property(&component.properties, "RECURRENCE-ID") {
            if parameter(property, "RANGE").is_some() {
                return Err("暂不支持整组改期，请在原日历中展开后重新导出。".into());
            }
            let key = Time::parse(property.value.as_deref().unwrap_or_default(), property)?
                .occurrence_key();
            if exceptions
                .get(&key)
                .is_none_or(|previous| sequence(previous) <= sequence(component))
            {
                exceptions.insert(key, component);
            }
        } else {
            masters.push(*component);
        }
    }
    masters.sort_by_key(|event| std::cmp::Reverse(sequence(event)));
    if masters.len() > 1 {
        preview.warn(format!(
            "「{}」含重复 UID，采用最高序列号。",
            limit_text(&event_title(&masters[0].properties), 70)
        ));
    }
    let mut events = Vec::new();
    let Some(master) = masters.first() else {
        for (key, exception) in exceptions {
            if !cancelled(exception) {
                let EventTimes { start, end, .. } = dates(exception)?;
                events.push(build(uid, &key, exception, start, end, preview)?);
            }
        }
        return Ok(events);
    };
    if cancelled(master) {
        return Ok(events);
    }
    let times = dates(master)?;
    let EventTimes { start, end, .. } = times;
    let rules = recurrences(master, start)?;
    let Some(rules) = rules else {
        if exceptions.is_empty() {
            events.push(build(uid, "single", master, start, end, preview)?);
            return Ok(events);
        }
        return Err("此日程同时包含独立活动和缺失重复规则的改期。".into());
    };
    let mut seen_occurrences = HashSet::new();
    for occurrence in &rules.limit() {
        *total_steps += 1;
        if *total_steps > MAX_STEPS
            || occurrence.timestamp_millis() >= horizon.end.saturating_add(2 * DAY_MILLIS)
        {
            break;
        }
        let begin = Time::from_recurrence(start, occurrence);
        let key = begin.occurrence_key();
        if !seen_occurrences.insert(key.clone()) {
            continue;
        }
        if let Some(exception) = exceptions.remove(&key) {
            append_if_visible(&mut events, uid, &key, exception, horizon, preview)?;
            continue;
        }
        let finish = times.occurrence_end(begin)?;
        if horizon.includes(begin, finish) {
            events.push(build(uid, &key, master, begin, finish, preview)?);
        }
        if events.len() > MAX_EVENTS {
            return Ok(events);
        }
    }
    for (key, exception) in exceptions {
        append_if_visible(&mut events, uid, &key, exception, horizon, preview)?;
    }
    Ok(events)
}

fn append_if_visible(
    events: &mut Vec<Event>,
    uid: &str,
    occurrence: &str,
    event: &IcalEvent,
    horizon: &Horizon,
    preview: &mut ImportPreview,
) -> Result<(), String> {
    if !cancelled(event) {
        let EventTimes { start, end, .. } = dates(event)?;
        if horizon.includes(start, end) {
            events.push(build(uid, occurrence, event, start, end, preview)?);
        }
    }
    Ok(())
}

fn recurrences(master: &IcalEvent, start: Time) -> Result<Option<RRuleSet>, String> {
    let repeat: Vec<_> = master
        .properties
        .iter()
        .filter(|prop| {
            ["RRULE", "RDATE", "EXDATE", "EXRULE"]
                .iter()
                .any(|name| prop.name.eq_ignore_ascii_case(name))
        })
        .collect();
    if repeat.is_empty() {
        return Ok(None);
    }
    if repeat.len() > 100 {
        return Err("此日程包含过多重复规则。".into());
    }
    let base = start.recurrence_date();
    let mut rules = RRuleSet::new(base).rdate(base);
    let mut date_count = 0;
    for prop in repeat {
        let value = prop.value.as_deref().ok_or("重复规则内容为空。")?;
        match prop.name.to_ascii_uppercase().as_str() {
            "RRULE" => {
                if value.len() > 4096 {
                    return Err("重复规则内容过长。".into());
                }
                let rule: RRule<Unvalidated> = value
                    .to_ascii_uppercase()
                    .parse()
                    .map_err(|_| "重复规则无效。")?;
                if !matches!(
                    rule.get_freq(),
                    Frequency::Daily | Frequency::Weekly | Frequency::Monthly | Frequency::Yearly
                ) {
                    return Err("仅支持按日、周、月或年重复。".into());
                }
                let rule = rule
                    .validate(base)
                    .map_err(|_| "重复规则与开始时间不匹配。")?;
                rules = rules.rrule(rule);
            }
            "RDATE" | "EXDATE" => {
                for value in value.split(',') {
                    date_count += 1;
                    if date_count > MAX_EVENTS {
                        return Err("重复日程指定的日期过多。".into());
                    }
                    let time = Time::parse(value, prop)?;
                    if !same_time_type(time, start) {
                        return Err("重复日期与日程开始时间类型不匹配。".into());
                    }
                    rules = if prop.name.eq_ignore_ascii_case("RDATE") {
                        rules.rdate(time.recurrence_date())
                    } else {
                        rules.exdate(time.recurrence_date())
                    };
                }
            }
            _ => return Err("该文件使用已废弃的 EXRULE 规则，请重新导出。".into()),
        }
    }
    Ok(Some(rules))
}

struct EventTimes {
    start: Time,
    end: Time,
    duration: Option<NominalDuration>,
}

impl EventTimes {
    fn occurrence_end(&self, start: Time) -> Result<Time, String> {
        match self.duration {
            Some(duration) => duration.end(start),
            None => start.add_millis(self.end.millis() - self.start.millis()),
        }
    }
}

fn dates(event: &IcalEvent) -> Result<EventTimes, String> {
    let props = &event.properties;
    let start = read_time(props, "DTSTART")?.ok_or("缺少有效的开始时间。")?;
    let explicit_end = read_time(props, "DTEND")?;
    let duration = property_value(props, "DURATION")
        .map(parse_duration)
        .transpose()?;
    let end = match (explicit_end, duration, start) {
        (Some(end), None, _) => end,
        (None, Some(duration), _) => duration.end(start)?,
        (None, None, Time::Day(_)) => start.add_millis(DAY_MILLIS)?,
        (None, None, Time::Instant(_)) => start.add_millis(1)?,
        _ => return Err("请提供有效的结束时间或时长。".into()),
    };
    if !same_time_type(start, end) || end.millis() <= start.millis() {
        return Err("结束时间应晚于开始时间，并使用相同的时间类型。".into());
    }
    Ok(EventTimes {
        start,
        end,
        duration,
    })
}

fn read_time(props: &[Property], name: &str) -> Result<Option<Time>, String> {
    property(props, name)
        .map(|prop| Time::parse(prop.value.as_deref().unwrap_or_default(), prop))
        .transpose()
}

fn same_time_type(first: Time, second: Time) -> bool {
    matches!(
        (first, second),
        (Time::Instant(_), Time::Instant(_)) | (Time::Day(_), Time::Day(_))
    )
}

#[derive(Clone, Copy)]
struct NominalDuration {
    days: u64,
    time_millis: Option<i64>,
}

impl NominalDuration {
    fn end(self, start: Time) -> Result<Time, String> {
        let days = Days::new(self.days);
        let end = match start {
            Time::Day(date) if self.time_millis.is_none() => {
                date.checked_add_days(days).map(Time::Day)
            }
            Time::Day(_) => return Err("全天日程时长应使用天或周。".into()),
            Time::Instant(_) if self.days == 0 => Some(start),
            Time::Instant(time) => time
                .naive_local()
                .checked_add_days(days)
                .and_then(|local| duration_end_local(local, time.timezone()))
                .map(Time::Instant),
        }
        .ok_or("日程结束时间超出范围。")?;
        self.time_millis
            .map_or(Ok(end), |millis| end.add_millis(millis))
    }
}

fn duration_end_local(local: NaiveDateTime, zone: Tz) -> Option<DateTime<Tz>> {
    zone.from_local_datetime(&local).earliest().or_else(|| {
        let Tz::Tz(named) = zone else { return None };
        // Use the offset before the gap for a time skipped by a clock change.
        let (_, before) = chrono_tz::GapInfo::new(&local, &named)?.begin?;
        before
            .fix()
            .from_local_datetime(&local)
            .single()
            .map(|time| time.with_timezone(&zone))
    })
}

fn parse_duration(raw: &str) -> Result<NominalDuration, String> {
    let failure = "日程时长格式无效。";
    let raw = raw
        .strip_prefix('+')
        .unwrap_or(raw)
        .strip_prefix('P')
        .ok_or(failure)?;
    let mut in_time = false;
    let mut previous = 0;
    let mut digits = String::new();
    let mut days = 0;
    let mut time_millis = 0i64;
    for unit in raw.chars() {
        if unit.is_ascii_digit() {
            digits.push(unit);
            continue;
        }
        if unit == 'T' && !in_time && digits.is_empty() && previous < 2 {
            in_time = true;
            continue;
        }
        let (order, factor) = match (in_time, unit) {
            (false, 'W') if previous == 0 => (5, 7),
            (false, 'D') => (1, 1),
            (true, 'H') => (2, 3_600_000),
            (true, 'M') => (3, 60_000),
            (true, 'S') => (4, 1000),
            _ => return Err(failure.into()),
        };
        if order <= previous {
            return Err(failure.into());
        }
        previous = order;
        let count = digits.parse::<i64>().map_err(|_| failure)?;
        digits.clear();
        let amount = count.checked_mul(factor).ok_or(failure)?;
        if in_time {
            time_millis = time_millis.checked_add(amount).ok_or(failure)?;
        } else {
            days = amount as u64;
        }
    }
    if !digits.is_empty() || days == 0 && time_millis == 0 || in_time && previous < 2 {
        return Err(failure.into());
    }
    Ok(NominalDuration {
        days,
        time_millis: in_time.then_some(time_millis),
    })
}

fn build(
    uid: &str,
    occurrence: &str,
    event: &IcalEvent,
    start: Time,
    end: Time,
    preview: &mut ImportPreview,
) -> Result<Event, String> {
    let props = &event.properties;
    let title = event_title(props);
    let location = property_value(props, "LOCATION")
        .map(decode_text)
        .unwrap_or_default();
    if title.encode_utf16().count() > 240 || location.trim().encode_utf16().count() > 240 {
        preview.warn(format!(
            "「{}」的标题或地点已截短。",
            limit_text(&title, 70)
        ));
    }
    let local_id = (occurrence == "single")
        .then(|| uid.to_ascii_lowercase())
        .and_then(|uid| {
            uid.strip_suffix(LOCAL_UID_SUFFIX)
                .and_then(|text| Uuid::parse_str(text).ok())
        });
    let source = if local_id.is_some() {
        property_value(props, "X-NOTCH-SOURCE-UID")
            .zip(property_value(props, "X-NOTCH-SOURCE-OCCURRENCE"))
            .map(|(uid, occurrence)| CalendarSourceIdentity {
                uid: decode_text(uid),
                occurrence: decode_text(occurrence),
            })
    } else {
        Some(CalendarSourceIdentity {
            uid: uid.to_owned(),
            occurrence: occurrence.to_owned(),
        })
    };
    let url = meeting_link(props);
    if property(props, "URL").is_some() && url.is_none() {
        preview.warn(format!(
            "「{}」的会议链接无法导入。",
            limit_text(&title, 70)
        ));
    }
    let all_day = matches!(start, Time::Day(_));
    let event = Event {
        id: local_id.unwrap_or_else(Uuid::new_v4),
        title: limit_text(&title, 240),
        location: limit_text(location.trim(), 240),
        start_at: start.millis(),
        end_at: end.millis(),
        all_day,
        start_date: all_day.then(|| start.date()),
        end_date: all_day.then(|| end.date()),
        url,
        subscription_id: None,
        source,
    };
    validate_event(&event)?;
    Ok(event)
}

fn meeting_link(props: &[Property]) -> Option<String> {
    if let Some(url) = property_value(props, "URL")
        .map(str::trim)
        .filter(|url| valid_meeting_url(url))
    {
        return Some(url.to_owned());
    }
    let known_domains = [
        "zoom.us",
        "meet.google.com",
        "teams.microsoft.com",
        "teams.live.com",
        "webex.com",
        "whereby.com",
    ];
    for name in ["DESCRIPTION", "LOCATION"] {
        let text = property_value(props, name)
            .map(decode_text)
            .unwrap_or_default();
        for candidate in
            text.split(|c: char| c.is_whitespace() || matches!(c, '<' | '>' | '"' | '\\' | '('))
        {
            let candidate =
                candidate.trim_end_matches([')', ',', '.', ';', '，', '。', '；', '）']);
            if !valid_meeting_url(candidate) {
                continue;
            }
            let host = url::Url::parse(candidate).ok()?.host_str()?.to_owned();
            if known_domains.iter().any(|domain| {
                host == *domain
                    || host
                        .strip_suffix(domain)
                        .is_some_and(|prefix| prefix.ends_with('.'))
            }) {
                return Some(candidate.to_owned());
            }
        }
    }
    None
}

fn sequence(event: &IcalEvent) -> u32 {
    property_value(&event.properties, "SEQUENCE")
        .and_then(|value| value.parse().ok())
        .unwrap_or_default()
}

fn cancelled(event: &IcalEvent) -> bool {
    property_value(&event.properties, "STATUS")
        .is_some_and(|value| value.eq_ignore_ascii_case("CANCELLED"))
}

fn event_title(props: &[Property]) -> String {
    let title = property_value(props, "SUMMARY")
        .map(decode_text)
        .unwrap_or_default();
    let title = title.trim();
    if title.is_empty() {
        "未命名日程".into()
    } else {
        title.to_owned()
    }
}

fn property<'a>(props: &'a [Property], name: &str) -> Option<&'a Property> {
    props
        .iter()
        .find(|prop| prop.name.eq_ignore_ascii_case(name))
}

fn property_value<'a>(props: &'a [Property], name: &str) -> Option<&'a str> {
    property(props, name)?.value.as_deref()
}

fn parameter<'a>(property: &'a Property, name: &str) -> Option<&'a str> {
    property
        .params
        .as_ref()?
        .iter()
        .find(|(key, _)| key.eq_ignore_ascii_case(name))?
        .1
        .first()
        .map(String::as_str)
}

fn limit_text(text: &str, max: usize) -> String {
    let mut units = 0;
    text.chars()
        .take_while(|character| {
            units += character.len_utf16();
            units <= max
        })
        .collect()
}

fn decode_text(text: &str) -> String {
    let mut value = String::with_capacity(text.len());
    let mut chars = text.chars();
    while let Some(character) = chars.next() {
        match (
            character,
            if character == '\\' {
                chars.next()
            } else {
                None
            },
        ) {
            ('\\', Some('n' | 'N')) => value.push('\n'),
            ('\\', Some(c @ ('\\' | ',' | ';'))) => value.push(c),
            ('\\', Some(c)) => {
                value.push('\\');
                value.push(c);
            }
            _ => value.push(character),
        }
    }
    value
}

pub fn export_file(path: &Path, events: &[Event], now: i64) -> Result<(), String> {
    let output = encode(events, now)?;
    let directory = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let mut file =
        NamedTempFile::new_in(directory).map_err(|err| format!("无法创建日历文件（{err}）"))?;
    file.write_all(output.as_bytes())
        .and_then(|()| file.as_file().sync_all())
        .map_err(|err| format!("无法写入日历文件（{err}）"))?;
    file.persist(path)
        .map_err(|err| format!("无法保存日历文件（{}）", err.error))?;
    Ok(())
}

fn encode(events: &[Event], now: i64) -> Result<String, String> {
    let stamp = ical_utc(now)?;
    let mut output = String::from(
        "BEGIN:VCALENDAR\r\nVERSION:2.0\r\nPRODID:-//NotchCalendar//Windows//ZH\r\nCALSCALE:GREGORIAN\r\n",
    );
    let mut exported_uids = HashSet::new();
    for event in events {
        validate_event(event)?;
        if !exported_uids.insert(event.id) {
            return Err("包含重复日程，无法导出。".into());
        }
        output.push_str("BEGIN:VEVENT\r\n");
        append_line(
            &mut output,
            "UID",
            &format!("{}{LOCAL_UID_SUFFIX}", event.id),
        );
        append_line(&mut output, "DTSTAMP", &stamp);
        append_line(&mut output, "SUMMARY", &encode_text(&event.title));
        if !event.location.is_empty() {
            append_line(&mut output, "LOCATION", &encode_text(&event.location));
        }
        if let Some(value) = &event.url {
            if value.contains(['\r', '\n']) {
                return Err("日期中的链接包含无效字符。".into());
            }
            append_line(&mut output, "URL", value);
        }
        if let (Some(start), Some(end)) = (event.start_date, event.end_date) {
            append_line(
                &mut output,
                "DTSTART;VALUE=DATE",
                &start.format("%Y%m%d").to_string(),
            );
            append_line(
                &mut output,
                "DTEND;VALUE=DATE",
                &end.format("%Y%m%d").to_string(),
            );
        } else {
            append_line(&mut output, "DTSTART", &ical_utc(event.start_at)?);
            append_line(&mut output, "DTEND", &ical_utc(event.end_at)?);
        }
        if let Some(source) = &event.source {
            append_line(&mut output, "X-NOTCH-SOURCE-UID", &encode_text(&source.uid));
            append_line(
                &mut output,
                "X-NOTCH-SOURCE-OCCURRENCE",
                &encode_text(&source.occurrence),
            );
        }
        output.push_str("END:VEVENT\r\n");
    }
    output.push_str("END:VCALENDAR\r\n");
    if output.len() as u64 > MAX_BYTES {
        return Err("导出的日历超过 2 MiB。".into());
    }
    Ok(output)
}

fn ical_utc(time: i64) -> Result<String, String> {
    let time = DateTime::from_timestamp_millis(time).ok_or("日程时间超出范围。")?;
    Ok(time.format("%Y%m%dT%H%M%SZ").to_string())
}

fn encode_text(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace("\r\n", "\n")
        .replace(['\r', '\n'], "\\n")
        .replace(',', "\\,")
        .replace(';', "\\;")
}

fn append_line(output: &mut String, name: &str, value: &str) {
    output.push_str(name);
    output.push(':');
    let mut bytes = name.len() + 1;
    for character in value.chars() {
        if bytes + character.len_utf8() > 75 {
            output.push_str("\r\n ");
            bytes = 1;
        }
        output.push(character);
        bytes += character.len_utf8();
    }
    output.push_str("\r\n");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn instant(value: &str) -> i64 {
        DateTime::parse_from_rfc3339(value)
            .unwrap()
            .timestamp_millis()
    }

    fn now() -> i64 {
        instant("2026-11-04T12:00:00Z")
    }

    fn calendar(events: &[&str]) -> String {
        let mut result = String::from("BEGIN:VCALENDAR\r\nVERSION:2.0\r\n");
        for event in events {
            result.push_str("BEGIN:VEVENT\r\n");
            result.push_str(event);
            result.push_str("\r\nEND:VEVENT\r\n");
        }
        result.push_str("END:VCALENDAR\r\n");
        result
    }

    #[test]
    fn existing_windows_fixture_imports_recurring_all_day_and_reports_unknown_timezone() {
        let fixture = include_str!("../tests/fixtures/calendar.ics");
        let preview = parse(fixture, &[], now()).unwrap();
        assert_eq!((preview.added, preview.skipped), (4, 1));
        assert!(preview.warnings[0].contains("Undefined/Zone"));
        let all_day = preview.events.iter().find(|event| event.all_day).unwrap();
        assert_eq!(all_day.end_date, NaiveDate::from_ymd_opt(2026, 11, 6));
        assert_eq!(all_day.end_at - all_day.start_at, 2 * DAY_MILLIS);
    }

    #[test]
    fn eastern_recurrence_tracks_dst_and_detached_overrides_preserve_identity() {
        let data = calendar(&[
            "UID:series\r\nSUMMARY:例会\r\nDTSTART;TZID=America/New_York:20261031T090000\r\nDTEND;TZID=America/New_York:20261031T100000\r\nRRULE:FREQ=DAILY;COUNT=5\r\nEXDATE;TZID=America/New_York:20261102T090000",
            "UID:series\r\nRECURRENCE-ID;TZID=America/New_York:20261103T090000\r\nDTSTART;TZID=America/New_York:20261103T110000\r\nDURATION:PT1H\r\nSUMMARY:例会改期",
            "UID:series\r\nRECURRENCE-ID;TZID=America/New_York:20261104T090000\r\nSTATUS:CANCELLED",
        ]);
        let preview = parse(&data, &[], now()).unwrap();
        assert_eq!(preview.events.len(), 3);
        assert_eq!(preview.events[0].start_at, instant("2026-10-31T13:00:00Z"));
        assert_eq!(preview.events[1].start_at, instant("2026-11-01T14:00:00Z"));
        let moved = preview
            .events
            .iter()
            .find(|event| event.title == "例会改期")
            .unwrap();
        assert_eq!(moved.start_at, instant("2026-11-03T16:00:00Z"));
        assert_eq!(
            moved.source.as_ref().unwrap().occurrence,
            "2026-11-03T14:00:00.000Z"
        );
    }

    #[test]
    fn nominal_duration_and_explicit_end_follow_dst_for_recurrences() {
        let preview = parse(
            &calendar(&[
                "UID:nominal\r\nSUMMARY:Nominal\r\nDTSTART;TZID=America/New_York:20261031T090000\r\nDURATION:P1DT1H\r\nRRULE:FREQ=DAILY;COUNT=2",
                "UID:explicit\r\nSUMMARY:Explicit\r\nDTSTART;TZID=America/New_York:20261031T090000\r\nDTEND;TZID=America/New_York:20261101T100000\r\nRRULE:FREQ=DAILY;COUNT=2",
                "UID:gap\r\nSUMMARY:Gap\r\nDTSTART;TZID=America/New_York:20270313T023000\r\nDURATION:P1D",
                "UID:all-day\r\nSUMMARY:All day\r\nDTSTART;VALUE=DATE:20261101\r\nDURATION:P1W",
            ]),
            &[],
            now(),
        )
        .unwrap();
        assert_eq!(preview.added, 6);
        let ends = |title| {
            preview
                .events
                .iter()
                .filter(|event| event.title == title)
                .map(|event| event.end_at)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            ends("Nominal"),
            [
                instant("2026-11-01T15:00:00Z"),
                instant("2026-11-02T15:00:00Z"),
            ]
        );
        assert_eq!(
            ends("Explicit"),
            [
                instant("2026-11-01T15:00:00Z"),
                instant("2026-11-02T16:00:00Z"),
            ]
        );
        assert_eq!(ends("Gap"), [instant("2027-03-14T07:30:00Z")]);
        let all_day = preview.events.iter().find(|event| event.all_day).unwrap();
        assert_eq!(all_day.end_date, NaiveDate::from_ymd_opt(2026, 11, 8));
    }

    #[test]
    fn duplicate_merge_keeps_id_and_does_not_modify_subscribed_snapshot() {
        let data =
            calendar(&["UID:one\r\nSUMMARY:原版\r\nDTSTART:20261104T090000Z\r\nDURATION:PT1H"]);
        let mut first = parse(&data, &[], now()).unwrap().events;
        let changed = data.replace("原版", "改版");
        let preview = parse(&changed, &first, now()).unwrap();
        assert_eq!((preview.added, preview.updated), (0, 1));
        assert_eq!(preview.events[0].id, first[0].id);
        first[0].subscription_id = Some(Uuid::new_v4());
        let preview = parse(&changed, &first, now()).unwrap();
        assert_eq!((preview.updated, preview.skipped), (0, 1));
        assert_eq!(preview.events, first);
    }

    #[test]
    fn export_round_trip_preserves_folded_unicode_text_urls_and_identity() {
        let dir = tempfile::tempdir().unwrap();
        let source = dir.path().join("fixture.ics");
        let calendar_data = calendar(&[
            "UID:all-day\r\nDTSTART;VALUE=DATE:20261104\r\nSUMMARY:全天日程",
            "UID:meeting\r\nDTSTART:20261104T090000Z\r\nDURATION:PT1H\r\nURL:javascript:alert(1)\r\nDESCRIPTION:请加入 https://meet.google.com/aaa-bbbb-ccc。",
        ]);
        std::fs::write(&source, calendar_data).unwrap();
        let preview = preview_import(&source, &[], now()).unwrap();
        let mut events = preview.events;
        let meeting = events.iter_mut().find(|event| !event.all_day).unwrap();
        assert_eq!(
            meeting.url.as_deref(),
            Some("https://meet.google.com/aaa-bbbb-ccc")
        );
        meeting.title = "中文,分号;转义\\换行\n下一行".repeat(10);
        let dest = dir.path().join("export.ics");
        export_file(&dest, &events, now()).unwrap();
        assert!(
            std::fs::read_to_string(&dest)
                .unwrap()
                .lines()
                .all(|line| line.len() <= 75)
        );
        let preview = preview_import(&dest, &events, now()).unwrap();
        assert_eq!(
            (
                preview.added,
                preview.updated,
                preview.unchanged,
                preview.skipped
            ),
            (0, 0, 2, 0)
        );
        assert_eq!(preview.events, events);
    }

    #[test]
    fn preview_rejects_changed_calendar_and_applies_over_unrelated_note_edits() {
        let event = "UID:one\r\nDTSTART:20261104T090000Z\r\nDURATION:PT1H";
        let initial = calendar(&[event]);
        let dir = tempfile::tempdir().unwrap();
        let mut store = Store::open(dir.path().to_owned()).unwrap();
        let preview = parse(&initial, &store.state().events, now()).unwrap();
        store
            .update(|state| {
                state.notes = "此便笺不影响日历预览".into();
                Ok(())
            })
            .unwrap();
        preview.apply(&mut store).unwrap();
        let changed = calendar(&["UID:two\r\nDTSTART:20261105T090000Z\r\nDURATION:PT1H"]);
        let preview = parse(&changed, &store.state().events, now()).unwrap();
        store
            .update(|state| {
                state.events[0].title = "刚刚修改".into();
                Ok(())
            })
            .unwrap();
        assert!(preview.apply(&mut store).is_err());
        assert_eq!(store.state().events.len(), 1);
    }

    #[test]
    fn huge_repeat_is_rejected_and_unsupported_series_is_reported() {
        let data = calendar(&[
            "UID:ancient\r\nDTSTART:19700101T090000Z\r\nDURATION:PT1H\r\nRRULE:FREQ=DAILY",
        ]);
        assert!(parse(&data, &[], now()).err().unwrap().contains("20,000"));
        let data = calendar(&[
            "UID:hourly\r\nDTSTART:20261104T090000Z\r\nDURATION:PT1H\r\nRRULE:FREQ=HOURLY",
            "UID:one\r\nDTSTART:20261104T090000Z\r\nDURATION:PT1H",
        ]);
        let preview = parse(&data, &[], now()).unwrap();
        assert_eq!((preview.added, preview.skipped), (1, 1));
    }
}

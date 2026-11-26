//! Read-only Exchange ActiveSync. WBXML and recurrence conversion adapted from
//! Daozai v0.6.70 (MIT, see THIRD_PARTY_NOTICES.txt).
use crate::{calendar_exchange::parse_snapshot, workspace::Event};
use chrono::{DateTime, Utc};

// MS-ASWBXML 的 AirSync、Calendar、FolderHierarchy 代码页。
const AIR: u8 = 0;
const CAL: u8 = 4;
const FOLDER: u8 = 7;

/// 设备未登记策略（142）、策略已更新（143）、策略标识无效（144）：需要重新握手。

#[derive(Clone, Debug, Default)]
struct Node {
    page: u8,
    tag: u8,
    value: String,
    children: Vec<Node>,
}
impl Node {
    fn child(&self, page: u8, tag: u8) -> Option<&Node> {
        self.children
            .iter()
            .find(|n| n.page == page && n.tag == tag)
    }
    fn text(&self, page: u8, tag: u8) -> &str {
        self.child(page, tag).map_or("", |n| n.value.as_str())
    }
    fn required(&self, page: u8, tag: u8) -> Result<&str, String> {
        let value = self.text(page, tag);
        if value.is_empty() {
            Err(invalid())
        } else {
            Ok(value)
        }
    }
}
fn invalid() -> String {
    String::from("日历服务器返回的数据不完整，请稍后重试")
}
fn node(page: u8, tag: u8, value: &str, children: Vec<Node>) -> Node {
    Node {
        page,
        tag,
        value: value.into(),
        children,
    }
}
fn encode(root: &Node) -> Vec<u8> {
    fn write(n: &Node, page: &mut u8, out: &mut Vec<u8>) {
        if *page != n.page {
            out.extend([0, n.page]);
            *page = n.page;
        }
        let content = !n.value.is_empty() || !n.children.is_empty();
        out.push(n.tag | if content { 0x40 } else { 0 });
        if !n.value.is_empty() {
            out.push(3);
            out.extend(n.value.as_bytes());
            out.push(0);
        }
        for child in &n.children {
            write(child, page, out);
        }
        if content {
            out.push(1);
        }
    }
    let mut bytes = vec![3, 1, 106, 0];
    write(root, &mut 0, &mut bytes);
    bytes
}
fn decode(bytes: &[u8]) -> Result<Node, String> {
    fn integer(bytes: &[u8], at: &mut usize) -> Result<usize, String> {
        let mut value = 0usize;
        for _ in 0..5 {
            let b = *bytes.get(*at).ok_or_else(invalid)?;
            *at += 1;
            value = value
                .checked_mul(128)
                .and_then(|v| v.checked_add((b & 127) as usize))
                .ok_or_else(invalid)?;
            if b & 128 == 0 {
                return Ok(value);
            }
        }
        Err(invalid())
    }
    let mut at = 1;
    if bytes.first() != Some(&3)
        || integer(bytes, &mut at)? != 1
        || integer(bytes, &mut at)? != 106
        || integer(bytes, &mut at)? != 0
    {
        return Err(invalid());
    }
    let mut tokens = 0;
    let mut page = 0;
    let mut stack = vec![Node::default()];
    while at < bytes.len() {
        tokens += 1;
        if tokens > 100_000 {
            return Err(invalid());
        }
        let token = bytes[at];
        at += 1;
        match token {
            0 => {
                page = *bytes.get(at).ok_or_else(invalid)?;
                at += 1;
            }
            1 => {
                if stack.len() <= 1 {
                    return Err(invalid());
                }
                let n = stack.pop().unwrap();
                stack.last_mut().unwrap().children.push(n);
            }
            3 | 0xc3 => {
                if stack.len() <= 1 {
                    return Err(invalid());
                }
                let len = if token == 3 {
                    bytes[at..]
                        .iter()
                        .position(|b| *b == 0)
                        .ok_or_else(invalid)?
                } else {
                    integer(bytes, &mut at)?
                };
                let end = at
                    .checked_add(len)
                    .filter(|end| *end <= bytes.len())
                    .ok_or_else(invalid)?;
                stack
                    .last_mut()
                    .unwrap()
                    .value
                    .push_str(&String::from_utf8_lossy(&bytes[at..end]));
                at = end + usize::from(token == 3);
            }
            t if t & 0x80 == 0 && t & 0x3f >= 5 => {
                let n = node(page, t & 0x3f, "", vec![]);
                if t & 0x40 != 0 {
                    if stack.len() >= 64 {
                        return Err(invalid());
                    }
                    stack.push(n);
                } else {
                    stack.last_mut().unwrap().children.push(n);
                }
            }
            _ => return Err(invalid()),
        }
    }
    if stack.len() != 1 || stack[0].children.len() != 1 {
        return Err(invalid());
    }
    Ok(stack.pop().unwrap().children.remove(0))
}

fn check_status(n: &Node, page: u8, tag: u8, label: &str) -> Result<(), String> {
    match n.required(page, tag)? {
        "1" => Ok(()),
        code if ["142", "143", "144"].contains(&code) => {
            Err(format!("{label} 的设备策略需要管理员批准。"))
        }
        code => Err(format!(
            "{label} 同步失败（Exchange {code}），请检查日历授权后重试"
        )),
    }
}
fn sync_request(id: &str, key: &str) -> Node {
    let mut fields = vec![node(AIR, 0x0b, key, vec![]), node(AIR, 0x12, id, vec![])];
    if key != "0" {
        fields.extend([
            node(AIR, 0x13, "1", vec![]),
            node(AIR, 0x15, "100", vec![]),
            node(AIR, 0x17, "", vec![node(AIR, 0x18, "0", vec![])]),
        ]);
    }
    node(
        AIR,
        5,
        "",
        vec![node(AIR, 0x1c, "", vec![node(AIR, 0x0f, "", fields)])],
    )
}

/// FolderSync 返回的一个文件夹。
struct Folder {
    id: String,
    parent: String,
    kind: String,
}

/// 默认日历（8）、新建日历（13）一定同步；挂在日历下面的通用（1）或未知类型（18）子文件夹也是日历
/// （共享、订阅日历常这样组织）。邮件、联系人、任务等文件夹一律不碰，不下载邮件内容。
fn is_calendar(folder: &Folder, all: &[Folder]) -> bool {
    if matches!(folder.kind.as_str(), "8" | "13") {
        return true;
    }
    if !matches!(folder.kind.as_str(), "1" | "18") {
        return false;
    }
    let mut parent = folder.parent.as_str();
    for _ in 0..16 {
        let Some(up) = all.iter().find(|f| f.id == parent) else {
            return false;
        };
        if matches!(up.kind.as_str(), "8" | "13") {
            return true;
        }
        parent = up.parent.as_str();
    }
    false
}

/// 完整读取一个日历文件夹的全部日程（按页跟随，直到没有更多）。
fn sync_folder(
    send: &mut impl FnMut(&str, &Node) -> Result<Node, String>,
    id: &str,
    label: &str,
) -> Result<std::collections::BTreeMap<String, Node>, String> {
    let mut key = "0".to_string();
    let mut items = std::collections::BTreeMap::new();
    for batch in 0..101 {
        if batch == 100 {
            return Err(format!("{label} 日程数量过多，无法完整同步"));
        }
        let result = send("Sync", &sync_request(id, &key))?;
        if result.child(AIR, 0x0e).is_some() {
            check_status(&result, AIR, 0x0e, label)?;
        }
        let collection = result
            .child(AIR, 0x1c)
            .and_then(|n| n.child(AIR, 0x0f))
            .ok_or_else(invalid)?;
        check_status(collection, AIR, 0x0e, label)?;
        let next = collection.required(AIR, 0x0b)?.to_string();
        if next == "0" || next == key && collection.child(AIR, 0x14).is_some() {
            return Err(invalid());
        }
        if let Some(commands) = collection.child(AIR, 0x16) {
            for change in &commands.children {
                if change.page != AIR {
                    continue;
                }
                let server_id = change.required(AIR, 0x0d)?;
                match change.tag {
                    7 | 8 => {
                        items.insert(
                            server_id.to_string(),
                            change.child(AIR, 0x1d).ok_or_else(invalid)?.clone(),
                        );
                    }
                    9 | 0x21 => {
                        items.remove(server_id);
                    }
                    _ => return Err(invalid()),
                }
            }
        }
        let initial = key == "0";
        key = next;
        if !initial && collection.child(AIR, 0x14).is_none() {
            break;
        }
    }
    Ok(items)
}

fn date(value: &str) -> Result<DateTime<Utc>, String> {
    chrono::NaiveDateTime::parse_from_str(value, "%Y%m%dT%H%M%SZ")
        .map(|d| d.and_utc())
        .map_err(|_| invalid())
}
fn escaped(value: &str) -> String {
    value
        .replace('\\', "\\\\")
        .replace('\r', "")
        .replace('\n', "\\n")
        .replace(';', "\\;")
        .replace(',', "\\,")
}
fn recurrence(n: &Node) -> Result<String, String> {
    let kind = n.required(CAL, 0x1c)?;
    let freq = match kind {
        "0" => "DAILY",
        "1" => "WEEKLY",
        "2" | "3" => "MONTHLY",
        "5" | "6" => "YEARLY",
        _ => return Err(String::from("暂不支持返回的重复日程类型")),
    };
    if !matches!(n.text(CAL, 0x37), "" | "0" | "1") {
        return Err(String::from("暂不支持非公历的重复日程"));
    }
    let mut rule = format!("RRULE:FREQ={freq}");
    for (tag, name) in [
        (0x1f, "INTERVAL"),
        (0x1e, "COUNT"),
        (0x21, "BYMONTHDAY"),
        (0x23, "BYMONTH"),
    ] {
        if !n.text(CAL, tag).is_empty() {
            let value = n.text(CAL, tag).parse::<u32>().map_err(|_| invalid())?;
            if value == 0 {
                return Err(invalid());
            }
            rule.push_str(&format!(";{name}={value}"));
        }
    }
    if !n.text(CAL, 0x1d).is_empty() {
        rule.push_str(&format!(
            ";UNTIL={}",
            date(n.text(CAL, 0x1d))?.format("%Y%m%dT%H%M%SZ")
        ));
    }
    if !n.text(CAL, 0x20).is_empty() {
        let bits = n.text(CAL, 0x20).parse::<u8>().map_err(|_| invalid())?;
        let days: Vec<_> = ["SU", "MO", "TU", "WE", "TH", "FR", "SA"]
            .into_iter()
            .enumerate()
            .filter_map(|(i, day)| (bits & (1 << i) != 0).then_some(day))
            .collect();
        if days.is_empty() {
            return Err(invalid());
        }
        rule.push_str(&format!(";BYDAY={}", days.join(",")));
    }
    if matches!(kind, "3" | "6") {
        let week = n
            .required(CAL, 0x22)?
            .parse::<i32>()
            .map_err(|_| invalid())?;
        if !(1..=5).contains(&week) {
            return Err(invalid());
        }
        rule.push_str(&format!(";BYSETPOS={}", if week == 5 { -1 } else { week }));
    }
    rule.push('\n');
    Ok(rule)
}
fn event_ics(item: &Node, fallback_uid: &str) -> Result<String, String> {
    if matches!(item.text(CAL, 0x18), "5" | "7") {
        return Ok(String::new());
    }
    let uid = if item.text(CAL, 0x28).is_empty() {
        fallback_uid
    } else {
        item.text(CAL, 0x28)
    };
    let start = date(item.required(CAL, 0x27)?)?;
    let end = date(item.required(CAL, 0x12)?)?;
    let all_day = item.text(CAL, 6) == "1";
    let timezone = item.text(CAL, 5);
    let mut offset = 0i64;
    if !timezone.is_empty() {
        use base64::Engine;
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(timezone)
            .map_err(|_| invalid())?;
        if bytes.len() != 172 {
            return Err(invalid());
        }
        let number = |at| i32::from_le_bytes(bytes[at..at + 4].try_into().unwrap()) as i64;
        offset = -(number(0) + number(84)) * 60;
        if !(-86400..=86400).contains(&offset) {
            return Err(invalid());
        }
        if item.child(CAL, 0x1b).is_some() && bytes[154..156] != [0, 0] {
            return Err(String::from("暂不支持含夏令时切换的重复日程"));
        }
    }
    // 按日历时区展开重复规则，避免 UTC 日期跨日后把周几或月日算错。
    let recurring = item.child(CAL, 0x1b).is_some();
    let recurrence_zone = if recurring && offset != 0 && !all_day {
        if offset % 3600 != 0 || !(-12 * 3600..=14 * 3600).contains(&offset) {
            return Err(String::from("暂不支持此时区的重复日程"));
        }
        Some(format!("Etc/GMT{:+}", -offset / 3600))
    } else {
        None
    };
    let time_line = |name: &str, at: DateTime<Utc>| {
        if all_day {
            format!(
                "{name};VALUE=DATE:{}\n",
                (at + chrono::Duration::seconds(offset)).format("%Y%m%d")
            )
        } else if let Some(zone) = &recurrence_zone {
            format!(
                "{name};TZID={zone}:{}\n",
                (at + chrono::Duration::seconds(offset)).format("%Y%m%dT%H%M%S")
            )
        } else {
            format!("{name}:{}\n", at.format("%Y%m%dT%H%M%SZ"))
        }
    };
    let mut result = format!(
        "BEGIN:VEVENT\nUID:{}\nSUMMARY:{}\n{}{}",
        escaped(uid),
        escaped(item.text(CAL, 0x26)),
        time_line("DTSTART", start),
        time_line("DTEND", end)
    );
    if let Some(rule) = item.child(CAL, 0x1b) {
        result.push_str(&recurrence(rule)?);
    }
    result.push_str("END:VEVENT\n");
    if let Some(exceptions) = item.child(CAL, 0x14) {
        for exception in &exceptions.children {
            if exception.page != CAL || exception.tag != 0x13 {
                continue;
            }
            let original = date(exception.required(CAL, 0x16)?)?;
            let mut changed = item.clone();
            changed
                .children
                .retain(|n| !(n.page == CAL && matches!(n.tag, 0x1b | 0x14)));
            let duration = end - start;
            if original.checked_add_signed(duration).is_none() {
                return Err(invalid());
            }
            for (tag, value) in [(0x27, original), (0x12, original + duration)] {
                changed
                    .children
                    .retain(|n| !(n.page == CAL && n.tag == tag));
                changed.children.push(node(
                    CAL,
                    tag,
                    &value.format("%Y%m%dT%H%M%SZ").to_string(),
                    vec![],
                ));
            }
            for field in &exception.children {
                if field.page == CAL && matches!(field.tag, 0x14 | 0x1b) {
                    return Err(invalid());
                }
                changed
                    .children
                    .retain(|n| !(n.page == field.page && n.tag == field.tag));
                changed.children.push(field.clone());
            }
            let mut instance = event_ics(&changed, uid)?;
            let cancelled =
                exception.text(CAL, 0x15) == "1" || matches!(exception.text(CAL, 0x18), "5" | "7");
            if cancelled {
                instance = format!(
                    "BEGIN:VEVENT\nUID:{}\n{}STATUS:CANCELLED\nEND:VEVENT\n",
                    escaped(uid),
                    time_line("DTSTART", original)
                );
            }
            result.push_str(&instance.replacen(
                "END:VEVENT",
                &format!("{}END:VEVENT", time_line("RECURRENCE-ID", original)),
                1,
            ));
        }
    }
    Ok(result)
}

pub(super) fn fetch(
    now: i64,
    mut transport: impl FnMut(&str, Vec<u8>) -> Result<Vec<u8>, String>,
) -> Result<Vec<Event>, String> {
    let mut remaining = 16 * 1024 * 1024usize;
    let mut send = |name: &str, root: &Node| {
        let bytes = transport(name, encode(root))?;
        remaining = remaining
            .checked_sub(bytes.len())
            .ok_or("Exchange 日程超出本次同步范围。")?;
        decode(&bytes)
    };
    let folders = send(
        "FolderSync",
        &node(FOLDER, 0x16, "", vec![node(FOLDER, 0x12, "0", vec![])]),
    )?;
    check_status(&folders, FOLDER, 0x0c, "Exchange")?;
    let changes = folders.child(FOLDER, 0x0e).ok_or_else(invalid)?;
    let folders: Vec<_> = changes
        .children
        .iter()
        .filter(|n| n.page == FOLDER && n.tag == 0x0f)
        .map(|n| Folder {
            id: n.text(FOLDER, 8).into(),
            parent: n.text(FOLDER, 9).into(),
            kind: n.text(FOLDER, 0x0a).into(),
        })
        .collect();
    let mut calendars = folders
        .iter()
        .filter(|f| is_calendar(f, &folders))
        .peekable();
    if calendars.peek().is_none() {
        return Err("账户没有可读取的 Exchange 日历。".into());
    }
    let mut events = Vec::new();
    for (index, folder) in calendars.enumerate() {
        if index >= 32 || folder.id.is_empty() {
            return Err(invalid());
        }
        let mut calendar = String::from("BEGIN:VCALENDAR\nVERSION:2.0\n");
        for (id, item) in sync_folder(&mut send, &folder.id, "Exchange")? {
            calendar.push_str(&event_ics(&item, &format!("{}:{}", folder.id, id))?);
            if calendar.len() as u64 > super::MAX_BYTES {
                return Err("Exchange 日历超出读取范围。".into());
            }
        }
        calendar.push_str("END:VCALENDAR");
        events.extend(parse_snapshot(&calendar, now)?);
        if events.len() > 2000 {
            return Err("Exchange 超出 2,000 条日程。".into());
        }
    }
    Ok(events)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn reads_calendar_but_skips_mail_folders() {
        let folder = |name, kind| {
            node(
                FOLDER,
                15,
                "",
                vec![
                    node(FOLDER, 8, name, vec![]),
                    node(FOLDER, 9, "0", vec![]),
                    node(FOLDER, 10, kind, vec![]),
                ],
            )
        };
        let folders = node(
            FOLDER,
            0x16,
            "",
            vec![
                node(FOLDER, 12, "1", vec![]),
                node(
                    FOLDER,
                    14,
                    "",
                    vec![folder("cal", "8"), folder("inbox", "2")],
                ),
            ],
        );
        let page = |key, extra| {
            node(
                AIR,
                5,
                "",
                vec![node(
                    AIR,
                    28,
                    "",
                    vec![node(
                        AIR,
                        15,
                        "",
                        vec![
                            node(AIR, 14, "1", vec![]),
                            node(AIR, 11, key, vec![]),
                            extra,
                        ],
                    )],
                )],
            )
        };
        let entry = node(
            AIR,
            29,
            "",
            vec![
                node(CAL, 0x26, "例会", vec![]),
                node(CAL, 0x27, "20261126T090000Z", vec![]),
                node(CAL, 0x12, "20261126T100000Z", vec![]),
            ],
        );
        let mut replies = [
            folders,
            page("1", node(AIR, 22, "", vec![])),
            page(
                "2",
                node(
                    AIR,
                    22,
                    "",
                    vec![node(
                        AIR,
                        7,
                        "",
                        vec![node(AIR, 13, "event1", vec![]), entry],
                    )],
                ),
            ),
        ]
        .into_iter();
        let events = fetch(
            date("20261126T000000Z").unwrap().timestamp_millis(),
            |name, input| {
                if name == "Sync" {
                    let n = decode(&input)?;
                    assert_eq!(
                        n.child(AIR, 28)
                            .unwrap()
                            .child(AIR, 15)
                            .unwrap()
                            .text(AIR, 18),
                        "cal"
                    );
                }
                Ok(encode(&replies.next().unwrap()))
            },
        )
        .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].title, "例会");
        assert!(replies.next().is_none());
    }
}

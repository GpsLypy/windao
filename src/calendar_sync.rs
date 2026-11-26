mod caldav;
#[cfg(windows)]
mod http;

use crate::{
    calendar_exchange::{MAX_BYTES, parse_snapshot},
    workspace::{CalendarSourceIdentity, CalendarSourceKind, Event, Store},
};
use sha2::{Digest, Sha256};
use url::Url;
use uuid::Uuid;

type Result<T> = std::result::Result<T, String>;

pub(super) struct Response {
    status: u16,
    body: String,
    location: Option<String>,
}

pub(super) struct Request<'a> {
    method: &'a str,
    url: &'a Url,
    authorization: Option<&'a str>,
    depth: Option<u8>,
    body: &'a str,
}

#[cfg(windows)]
pub(crate) fn get_json(url: &Url) -> Result<(u16, String)> {
    let response = http::Http::open()?.request_accept(
        Request {
            method: "GET",
            url,
            authorization: None,
            depth: None,
            body: "",
        },
        "application/json",
    )?;
    Ok((response.status, response.body))
}

fn source_url(raw: &str) -> Result<Url> {
    Url::parse(raw.trim())
        .ok()
        .filter(|url| {
            raw.trim().len() <= 4096
                && url.scheme() == "https"
                && url.has_host()
                && url.username().is_empty()
                && url.password().is_none()
                && url.fragment().is_none()
        })
        .ok_or_else(|| "请输入有效的 HTTPS 地址，用户名和密码请填写在单独的字段中。".into())
}

fn status_error(stage: &str, status: u16) -> String {
    match status {
        401 | 403 => "日历服务器拒绝访问。请检查账号、应用专用密码或订阅权限。".into(),
        _ => format!("{stage}失败（HTTP {status}）。"),
    }
}

fn snapshot(
    kind: CalendarSourceKind,
    source_id: Uuid,
    url: &Url,
    auth: Option<&str>,
    now: i64,
    mut request: impl FnMut(Request<'_>) -> Result<Response>,
) -> Result<Vec<Event>> {
    let mut events = match kind {
        CalendarSourceKind::Ics => {
            let response = request(Request {
                method: "GET",
                url,
                authorization: None,
                depth: None,
                body: "",
            })?;
            if response.status != 200 {
                return Err(status_error("获取订阅", response.status));
            }
            if response.body.len() as u64 > MAX_BYTES {
                return Err("订阅日历不能超过 2 MiB。".into());
            }
            parse_snapshot(&response.body, now)?
        }
        CalendarSourceKind::CalDav => caldav::fetch(url, auth, now, &mut request)?,
    };
    let mut seen = std::collections::HashSet::new();
    for event in &mut events {
        namespace(source_id, event);
    }
    events.retain(|event| seen.insert(event.id));
    Ok(events)
}

fn namespace(source_id: Uuid, event: &mut Event) {
    let identity = match &event.source {
        Some(source) => format!(
            "{}:{}:{}",
            source.uid.encode_utf16().count(),
            source.uid,
            source.occurrence
        ),
        None => format!("local:{}", event.id),
    };
    let digest = Sha256::digest(format!("{source_id}:{identity}"));
    let mut id: [u8; 16] = digest[..16].try_into().expect("SHA-256 has 32 bytes");
    // Match the identity previously used by the Electron calendar subscription implementation.
    id[6] = (id[6] & 0x0f) | 0x40;
    id[8] = (id[8] & 0x0f) | 0xa0;
    let original = event
        .source
        .take()
        .unwrap_or_else(|| CalendarSourceIdentity {
            uid: event.id.to_string(),
            occurrence: "single".into(),
        });
    event.id = Uuid::from_bytes(id);
    event.source = Some(CalendarSourceIdentity {
        uid: format!(
            "subscription:{source_id}:{:x}",
            Sha256::digest(original.uid.as_bytes())
        ),
        occurrence: original.occurrence,
    });
    event.subscription_id = Some(source_id);
}

fn persist_snapshot(
    store: &mut Store,
    source_id: Uuid,
    connection: &str,
    now: i64,
    result: Result<Vec<Event>>,
) -> Result<()> {
    store.update(move |state| {
        let Some(source) = state.calendar_sources.iter_mut().find(|source| {
            source.id == source_id && source.protected_connection.as_deref() == Some(connection)
        }) else {
            return Ok(());
        };
        match result {
            Ok(events) => {
                state
                    .events
                    .retain(|event| event.subscription_id != Some(source_id));
                state.events.extend(events);
                source.updated_at = Some(now);
                source.error = None;
            }
            Err(error) => source.error = Some(error),
        }
        Ok(())
    })
}

#[cfg(windows)]
mod manager;
#[cfg(windows)]
pub(crate) use manager::{CalendarSync, Connection, migrate_electron_connection};

#[cfg(test)]
mod tests;

use super::{Result, http::Http, persist_snapshot, snapshot, source_url};
use crate::{
    credential_protection::{decrypt_electron, dpapi},
    workspace::{CalendarSource, CalendarSourceKind, Event, MAX_SOURCES, Store},
};
use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::Utc;
use eframe::egui::{Context, ViewportId};
use serde::{Deserialize, Serialize};
use std::{
    collections::{HashMap, VecDeque},
    path::Path,
    sync::mpsc::{self, TryRecvError},
    thread,
    time::{Duration, Instant},
};
use uuid::Uuid;
use zeroize::{Zeroize, Zeroizing};

const REFRESH_INTERVAL: Duration = Duration::from_secs(10 * 60);

#[derive(Default, Deserialize, Serialize)]
pub(crate) struct Connection {
    pub url: String,
    pub username: String,
    pub password: String,
}

impl Drop for Connection {
    fn drop(&mut self) {
        self.url.zeroize();
        self.username.zeroize();
        self.password.zeroize();
    }
}

impl Connection {
    fn validate(&self, kind: CalendarSourceKind) -> Result<url::Url> {
        let url = source_url(&self.url)?;
        if kind != CalendarSourceKind::Ics
            && (self.username.trim().is_empty()
                || self.username.contains(':')
                || self.username.len() > 1024
                || self.password.is_empty()
                || self.password.len() > 4096)
        {
            return Err("请输入日历用户名和应用专用密码。".into());
        }
        Ok(url)
    }

    fn protect(&self) -> Result<String> {
        let data = Zeroizing::new(serde_json::to_vec(self).map_err(|_| "无法保存日历连接。")?);
        let protected = dpapi(&data, true).map_err(|_| "Windows 无法加密日历连接。")?;
        Ok(STANDARD.encode(protected))
    }

    fn unprotect(cipher: &str) -> Result<Self> {
        let error = "Windows 无法解锁日历连接，请重新连接。";
        let data = STANDARD.decode(cipher).map_err(|_| error)?;
        let plain = Zeroizing::new(dpapi(&data, false).map_err(|_| error)?);
        serde_json::from_slice(&plain).map_err(|_| error.into())
    }
}

pub(crate) fn migrate_electron_connection(workspace: &Path, encoded: &str) -> Option<String> {
    if encoded.len() > 18_000 {
        return None;
    }
    let cipher = STANDARD.decode(encoded).ok()?;
    let decoded = Zeroizing::new(decrypt_electron(&cipher, workspace.parent()?).ok()?);
    let url = std::str::from_utf8(&decoded).ok()?.to_owned();
    let connection = Connection {
        url,
        username: String::new(),
        password: String::new(),
    };
    connection.validate(CalendarSourceKind::Ics).ok()?;
    connection.protect().ok()
}

struct Refresh {
    source: Uuid,
    connection: String,
    result: Result<Vec<Event>>,
}

#[derive(Default)]
pub(crate) struct CalendarSync {
    next_refresh: Option<Instant>,
    queued: VecDeque<Uuid>,
    active: Option<(Uuid, mpsc::Receiver<Refresh>)>,
    local_errors: HashMap<Uuid, String>,
}

impl CalendarSync {
    pub(crate) fn configure(
        &mut self,
        ctx: &Context,
        store: &mut Store,
        id: Option<Uuid>,
        name: &str,
        kind: CalendarSourceKind,
        connection: &Connection,
    ) -> Result<()> {
        let name = name.trim();
        if name.is_empty() || name.encode_utf16().count() > 100 {
            return Err("请输入 1 到 100 字的日历名称。".into());
        }
        connection.validate(kind)?;
        let protected = connection.protect()?;
        let source_id = id.unwrap_or_else(Uuid::new_v4);
        store.update(|state| {
            if id.is_some() {
                let source = state
                    .calendar_sources
                    .iter_mut()
                    .find(|source| source.id == source_id)
                    .ok_or("日历来源已移除。")?;
                source.name = name.into();
                source.kind = kind;
                source.protected_connection = Some(protected);
                source.error = None;
            } else {
                if state.calendar_sources.len() >= MAX_SOURCES {
                    return Err("最多可以连接 10 个日历来源。".into());
                }
                state.calendar_sources.push(CalendarSource {
                    id: source_id,
                    kind,
                    name: name.into(),
                    protected_connection: Some(protected),
                    updated_at: None,
                    error: None,
                });
            }
            Ok(())
        })?;
        if !self.queued.contains(&source_id) {
            self.queued.push_back(source_id);
        }
        self.local_errors.remove(&source_id);
        ctx.request_repaint_of(ViewportId::ROOT);
        Ok(())
    }

    pub(crate) fn refresh(&mut self, ctx: &Context, store: &Store, id: Option<Uuid>) {
        for source in &store.state().calendar_sources {
            if source.protected_connection.is_some()
                && id.is_none_or(|id| id == source.id)
                && !self.is_updating(source.id)
            {
                self.queued.push_back(source.id);
                self.local_errors.remove(&source.id);
            }
        }
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(crate) fn remove(&mut self, store: &mut Store, id: Uuid) -> Result<()> {
        store.update(|state| {
            state.calendar_sources.retain(|source| source.id != id);
            state
                .events
                .retain(|event| event.subscription_id != Some(id));
            Ok(())
        })?;
        self.queued.retain(|next| *next != id);
        self.local_errors.remove(&id);
        Ok(())
    }

    pub(crate) fn is_updating(&self, id: Uuid) -> bool {
        self.active
            .as_ref()
            .is_some_and(|(source, _)| *source == id)
            || self.queued.contains(&id)
    }

    pub(crate) fn local_error(&self, id: Uuid) -> Option<&str> {
        self.local_errors.get(&id).map(String::as_str)
    }

    pub(crate) fn tick(&mut self, ctx: &Context, store: &mut Store) {
        if let Some((source, active)) = &self.active {
            match active.try_recv() {
                Ok(refresh) => {
                    self.active = None;
                    let now = Utc::now().timestamp_millis();
                    if let Err(error) = persist_snapshot(
                        store,
                        refresh.source,
                        &refresh.connection,
                        now,
                        refresh.result,
                    ) {
                        self.local_errors.insert(refresh.source, error);
                    }
                }
                Err(TryRecvError::Disconnected) => {
                    self.local_errors.insert(*source, "日历刷新已中断。".into());
                    self.active = None;
                }
                Err(TryRecvError::Empty) => {}
            }
        }

        if store
            .state()
            .calendar_sources
            .iter()
            .any(|source| source.protected_connection.is_some())
        {
            let now = Instant::now();
            if self.next_refresh.is_none_or(|next| next <= now) {
                self.refresh(ctx, store, None);
                self.next_refresh = Some(now + REFRESH_INTERVAL);
            }
            if let Some(next) = self.next_refresh {
                ctx.request_repaint_after_for(
                    next.saturating_duration_since(now),
                    ViewportId::ROOT,
                );
            }
        } else {
            self.next_refresh = None;
        }

        while self.active.is_none() {
            let Some(id) = self.queued.pop_front() else {
                break;
            };
            if let Some(source) = store
                .state()
                .calendar_sources
                .iter()
                .find(|source| source.id == id)
                && let Some(connection) = &source.protected_connection
            {
                self.start(ctx, source, connection.clone());
            }
        }
    }

    fn start(&mut self, ctx: &Context, source: &CalendarSource, connection: String) {
        let id = source.id;
        let kind = source.kind;
        let context = ctx.clone();
        let (tx, rx) = mpsc::sync_channel(1);
        let result = thread::Builder::new()
            .name("calendar-refresh".into())
            .spawn(move || {
                let result = refresh(kind, id, &connection);
                let _ = tx.send(Refresh {
                    source: id,
                    connection,
                    result,
                });
                context.request_repaint_of(ViewportId::ROOT);
            });
        match result {
            Ok(_) => self.active = Some((id, rx)),
            Err(_) => {
                self.local_errors.insert(id, "无法启动日历刷新。".into());
            }
        }
    }
}

fn refresh(kind: CalendarSourceKind, id: Uuid, sealed: &str) -> Result<Vec<Event>> {
    let connection = Connection::unprotect(sealed)?;
    let url = connection.validate(kind)?;
    let auth = (kind != CalendarSourceKind::Ics).then(|| {
        let input = Zeroizing::new(format!(
            "{}:{}",
            connection.username.trim(),
            connection.password
        ));
        Zeroizing::new(format!("Basic {}", STANDARD.encode(input.as_bytes())))
    });
    let http = Http::open()?;
    if kind == CalendarSourceKind::Exchange {
        let mut endpoint = url;
        if endpoint.path().trim_end_matches('/').is_empty() {
            endpoint.set_path("/Microsoft-Server-ActiveSync");
        }
        endpoint.set_query(None);
        let device_id = id.simple().to_string();
        let events = super::exchange::fetch(Utc::now().timestamp_millis(), |command, body| {
            let mut target = endpoint.clone();
            target.query_pairs_mut().extend_pairs([
                ("Cmd", command),
                ("User", connection.username.trim()),
                ("DeviceId", &device_id),
                ("DeviceType", "NotchCalendar"),
            ]);
            http.exchange(
                super::Request {
                    method: "POST",
                    url: &target,
                    authorization: auth.as_deref().map(String::as_str),
                    depth: None,
                    body: "",
                },
                &body,
            )
        })?;
        return Ok(super::namespaced(id, events));
    }
    snapshot(
        kind,
        id,
        &url,
        auth.as_ref().map(|auth| auth.as_str()),
        Utc::now().timestamp_millis(),
        |request| http.request(request),
    )
}

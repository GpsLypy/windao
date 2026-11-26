use super::{Event, Result, length, read_json, valid_timestamp};
use serde::{Deserialize, Serialize};
use std::{collections::HashSet, path::Path};
use uuid::Uuid;

pub(crate) const MAX_SOURCES: usize = 10;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CalendarSourceKind {
    #[default]
    Ics,
    CalDav,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct CalendarSource {
    pub id: Uuid,
    pub kind: CalendarSourceKind,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub protected_connection: Option<String>,
    pub updated_at: Option<i64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
}

pub(super) fn validate(sources: &[CalendarSource]) -> Result<()> {
    let mut ids = HashSet::new();
    if sources.len() > MAX_SOURCES
        || sources.iter().any(|source| {
            !ids.insert(source.id)
                || source.name.trim().is_empty()
                || length(&source.name) > 100
                || !source.updated_at.is_none_or(valid_timestamp)
                || source
                    .protected_connection
                    .as_ref()
                    .is_some_and(|value| value.is_empty() || value.len() > 64_000)
                || source
                    .error
                    .as_ref()
                    .is_some_and(|value| length(value) > 300)
        })
    {
        return Err("日历来源配置格式无效。".into());
    }
    Ok(())
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct LegacySource {
    id: Uuid,
    name: String,
    updated_at: Option<i64>,
    #[cfg(windows)]
    url_cipher: Option<String>,
}

pub(super) fn migrate(path: &Path, events: &[Event]) -> Vec<CalendarSource> {
    let legacy =
        read_json::<Vec<LegacySource>>(&path.with_file_name("calendar-subscriptions.v1.json"))
            .unwrap_or_default();
    let mut ids = HashSet::new();
    let mut sources: Vec<_> = legacy
        .into_iter()
        .filter(|source| {
            !source.name.trim().is_empty() && length(&source.name) <= 100 && ids.insert(source.id)
        })
        .take(MAX_SOURCES)
        .map(|source| {
            #[cfg(windows)]
            let protected_connection = source.url_cipher.as_deref().and_then(|encoded| {
                crate::calendar_sync::migrate_electron_connection(path, encoded)
            });
            #[cfg(not(windows))]
            let protected_connection = None;
            CalendarSource {
                id: source.id,
                name: source.name,
                kind: CalendarSourceKind::Ics,
                protected_connection,
                updated_at: source.updated_at.filter(|at| valid_timestamp(*at)),
                error: None,
            }
        })
        .collect();
    label_cached_sources(&mut sources, events);
    sources
}

pub(super) fn label_cached_sources(sources: &mut Vec<CalendarSource>, events: &[Event]) {
    let mut ids: HashSet<_> = sources.iter().map(|source| source.id).collect();
    for id in events.iter().filter_map(|event| event.subscription_id) {
        if sources.len() == MAX_SOURCES {
            break;
        }
        if ids.insert(id) {
            sources.push(CalendarSource {
                id,
                kind: CalendarSourceKind::Ics,
                name: "旧版日历订阅".into(),
                protected_connection: None,
                updated_at: None,
                error: None,
            });
        }
    }
}

#[cfg(test)]
#[test]
fn keeps_cached_legacy_source_metadata_when_the_connection_cannot_be_read() {
    let directory = tempfile::tempdir().unwrap();
    let workspace = directory.path().join("workspace.json");
    let id = Uuid::new_v4();
    let original = serde_json::to_vec(&serde_json::json!([{
        "id": id, "name": "工作日历", "updatedAt": 1793756800000i64, "count": 1, "urlCipher": "invalid"
    }])).unwrap();
    let sources_path = directory.path().join("calendar-subscriptions.v1.json");
    std::fs::write(&sources_path, &original).unwrap();
    let sources = migrate(&workspace, &[]);
    assert_eq!(sources.len(), 1);
    assert_eq!(sources[0].id, id);
    assert_eq!(sources[0].name, "工作日历");
    assert_eq!(sources[0].updated_at, Some(1793756800000));
    assert!(sources[0].protected_connection.is_none());
    assert_eq!(std::fs::read(sources_path).unwrap(), original);
}

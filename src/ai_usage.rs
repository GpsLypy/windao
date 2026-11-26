//! Read-only local usage summaries. The caller owns consent and schedules scans off the UI thread.
use std::{
    collections::{BTreeMap, HashMap},
    env,
    path::{Path, PathBuf},
};

use chrono::{DateTime, Days, Local, NaiveDate, Utc};
use serde::Deserialize;
use uuid::Uuid;

mod files;
use files::Logs;

const MAX_OBSERVATIONS: usize = 65_536;

#[derive(Default, Debug)]
pub struct UsageReport {
    pub codex: ProviderUsage,
    pub claude: ProviderUsage,
}

#[derive(Default, Debug)]
pub struct ProviderUsage {
    pub today_tokens: u64,
    pub week_tokens: u64,
    pub last_seen: Option<i64>,
    pub windows: Vec<UsageWindow>,
    pub partial: bool,
    pub source_found: bool,
}

#[derive(Debug, Clone)]
pub struct UsageWindow {
    pub minutes: u32,
    pub used_percent: f64,
    pub resets_at: Option<i64>,
    pub observed_at: i64,
}

struct Period {
    today: NaiveDate,
    first_day: NaiveDate,
    now: i64,
}

impl Period {
    fn new(now: DateTime<Local>) -> Self {
        let today = now.date_naive();
        Self {
            today,
            first_day: today - Days::new(6),
            now: now.timestamp(),
        }
    }

    fn record(&self, summary: &mut ProviderUsage, at: i64, count: u64) {
        let Some(day) =
            DateTime::from_timestamp(at, 0).map(|v| v.with_timezone(&Local).date_naive())
        else {
            return;
        };
        summary.last_seen = Some(summary.last_seen.unwrap_or(at).max(at));
        if day >= self.first_day && day <= self.today {
            summary.week_tokens = summary.week_tokens.saturating_add(count);
        }
        if day == self.today {
            summary.today_tokens = summary.today_tokens.saturating_add(count);
        }
    }

    fn since(&self) -> i64 {
        self.now - 8 * 86_400 // Includes the oldest local date across DST transitions.
    }
}

pub fn read_local() -> UsageReport {
    let user_home = env::home_dir();
    let root = |variable, default| {
        env::var_os(variable)
            .map(PathBuf::from)
            .filter(|p| p.is_absolute())
            .or_else(|| user_home.as_ref().map(|p| p.join(default)))
    };
    let period = Period::new(Local::now());
    UsageReport {
        codex: root("CODEX_HOME", ".codex")
            .map(|path| codex(&path, &period))
            .unwrap_or_default(),
        claude: root("CLAUDE_CONFIG_DIR", ".claude")
            .map(|path| claude(&path, &period))
            .unwrap_or_default(),
    }
}

#[derive(Deserialize)]
struct Rollout {
    timestamp: DateTime<Utc>,
    #[serde(rename = "type")]
    kind: String,
    payload: CodexPayload,
}

#[derive(Deserialize)]
struct CodexPayload {
    #[serde(rename = "type")]
    kind: Option<String>,
    info: Option<CodexInfo>,
    rate_limits: Option<Limits>,
    id: Option<Uuid>,
    forked_from_id: Option<Uuid>,
}

#[derive(Deserialize)]
struct CodexInfo {
    total_token_usage: TokenTotal,
    last_token_usage: Option<TokenTotal>,
}
#[derive(Deserialize)]
struct TokenTotal {
    total_tokens: u64,
}
#[derive(Deserialize)]
struct Limits {
    limit_id: Option<String>,
    primary: Option<RawWindow>,
    secondary: Option<RawWindow>,
}
#[derive(Deserialize)]
struct RawWindow {
    window_minutes: Option<u32>,
    used_percent: Option<f64>,
    resets_at: Option<i64>,
}

impl Limits {
    fn observed(self, at: i64) -> Vec<UsageWindow> {
        [self.primary, self.secondary]
            .into_iter()
            .flatten()
            .filter_map(|w| {
                let minutes = w.window_minutes.filter(|v| *v > 0)?;
                let used_percent = w.used_percent.filter(|v| (0.0..=100.0).contains(v))?;
                Some(UsageWindow {
                    minutes,
                    used_percent,
                    resets_at: w.resets_at,
                    observed_at: at,
                })
            })
            .collect()
    }
}

#[derive(Default)]
struct Session {
    // A rate-limit-only update can repeat the last cumulative token count.
    totals: BTreeMap<u64, TokenObservation>,
    forked_at: Option<i64>,
}

#[derive(Clone, Copy)]
struct TokenObservation {
    at: i64,
    last: Option<u64>,
    truncated: bool,
}

fn codex(root: &Path, period: &Period) -> ProviderUsage {
    let mut logs = Logs::codex(root, period.since());
    let mut summary = ProviderUsage::default();
    let mut sessions = HashMap::<Uuid, Session>::new();
    let mut latest_limits = None;
    let mut observations = 0;
    logs.lines(|path, line, truncated| {
        let Ok(row) = serde_json::from_slice::<Rollout>(line) else {
            return;
        };
        let at = row.timestamp.timestamp();
        if at > period.now {
            return;
        }
        let id = path
            .file_stem()
            .and_then(|n| n.to_str())
            .and_then(|s| s.get(s.len().checked_sub(36)?..))
            .and_then(|s| Uuid::parse_str(s).ok());
        let Some(id) = id else {
            return;
        };
        if row.kind == "session_meta"
            && row.payload.id == Some(id)
            && row.payload.forked_from_id.is_some()
        {
            sessions.entry(id).or_default().forked_at = Some(at);
            return;
        }
        if row.kind != "event_msg" || row.payload.kind.as_deref() != Some("token_count") {
            return;
        }
        if let Some(limits) = row.payload.rate_limits
            && limits.limit_id.as_deref().is_none_or(|v| v == "codex")
            && latest_limits.is_none_or(|seen| at >= seen)
        {
            let windows = limits.observed(at);
            if !windows.is_empty() {
                latest_limits = Some(at);
                summary.windows = windows;
                summary.last_seen = Some(at);
            }
        }
        let Some(info) = row.payload.info else {
            return;
        };
        let current = info.total_token_usage.total_tokens;
        let totals = &mut sessions.entry(id).or_default().totals;
        if let Some(existing) = totals.get_mut(&current) {
            existing.at = existing.at.min(at);
            existing.truncated &= truncated;
        } else if observations < MAX_OBSERVATIONS {
            totals.insert(
                current,
                TokenObservation {
                    at,
                    last: info.last_token_usage.map(|v| v.total_tokens),
                    truncated,
                },
            );
            observations += 1;
        } else {
            summary.partial = true;
        }
    });
    for session in sessions.into_values() {
        let mut previous = None;
        for (total, obs) in session.totals {
            let inherited = session.forked_at.is_some_and(|fork| obs.at < fork);
            let delta = match previous {
                Some(value) => total - value,
                None if obs.truncated || session.forked_at.is_some() => {
                    obs.last.unwrap_or(0).min(total)
                }
                None => total,
            };
            previous = Some(total);
            if !inherited {
                period.record(&mut summary, obs.at, delta);
            }
        }
    }
    summary.windows.retain(|w| {
        w.resets_at
            .unwrap_or(w.observed_at + i64::from(w.minutes) * 60)
            > period.now
    });
    summary.partial |= logs.partial;
    summary.source_found = logs.found;
    summary
}

#[derive(Deserialize)]
struct ClaudeRow {
    #[serde(rename = "type")]
    kind: String,
    timestamp: DateTime<Utc>,
    #[serde(rename = "requestId")]
    request: Option<String>,
    message: ClaudeMessage,
}
#[derive(Deserialize)]
struct ClaudeMessage {
    id: Option<String>,
    usage: Option<ClaudeTokens>,
}
#[derive(Deserialize, Default)]
struct ClaudeTokens {
    input_tokens: u64,
    output_tokens: u64,
    #[serde(default)]
    cache_creation_input_tokens: u64,
    #[serde(default)]
    cache_read_input_tokens: u64,
}
impl ClaudeTokens {
    fn total(&self) -> u64 {
        [
            self.input_tokens,
            self.output_tokens,
            self.cache_creation_input_tokens,
            self.cache_read_input_tokens,
        ]
        .into_iter()
        .fold(0, u64::saturating_add)
    }
}
fn valid_id(id: Option<String>) -> Option<String> {
    id.filter(|s| !s.is_empty() && s.len() <= 256)
}

fn claude(root: &Path, period: &Period) -> ProviderUsage {
    let mut logs = Logs::claude(root, period.since());
    let mut summary = ProviderUsage::default();
    // Each assistant content block can repeat the same API message; prefer its final usage.
    let mut messages = HashMap::<String, (i64, u64)>::new();
    let mut request_aliases = HashMap::new();
    logs.lines(|_, line, _| {
        let Ok(row) = serde_json::from_slice::<ClaudeRow>(line) else {
            return;
        };
        let at = row.timestamp.timestamp();
        if row.kind != "assistant" || at > period.now || at < period.since() {
            return;
        }
        let Some(tokens) = row.message.usage else {
            return;
        };
        let request = valid_id(row.request);
        let id = valid_id(row.message.id);
        if let (Some(request), Some(id)) = (&request, &id)
            && request_aliases.len() < MAX_OBSERVATIONS
        {
            request_aliases.insert(id.clone(), request.clone());
        }
        let Some(key) = request.or(id) else {
            return;
        };
        if !messages.contains_key(&key) && messages.len() >= MAX_OBSERVATIONS {
            summary.partial = true;
            return;
        }
        let record = messages.entry(key).or_insert((at, 0));
        record.0 = record.0.min(at);
        record.1 = record.1.max(tokens.total());
    });
    for (id, request) in request_aliases {
        if request != id
            && let Some((at, tokens)) = messages.remove(&id)
        {
            let message = messages.entry(request).or_insert((at, 0));
            message.0 = message.0.min(at);
            message.1 = message.1.max(tokens);
        }
    }
    for (at, tokens) in messages.into_values() {
        period.record(&mut summary, at, tokens);
    }
    summary.source_found = logs.found;
    summary.partial |= logs.partial;
    summary
}

#[cfg(test)]
mod tests;

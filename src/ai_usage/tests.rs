use super::*;
use chrono::{Duration, TimeZone};
use serde_json::{Value, json};
use std::{fs, io::Write};

const ID: &str = "4209917d-c25f-4a22-a7d9-ebb8bc6cb871";
fn period() -> Period {
    Period::new(
        Local
            .with_ymd_and_hms(2025, 1, 20, 12, 0, 0)
            .single()
            .unwrap(),
    )
}
fn at(t: i64) -> String {
    DateTime::from_timestamp(t, 0).unwrap().to_rfc3339()
}
fn rollout() -> String {
    format!("rollout-2025-01-20T10-00-00-{ID}.jsonl")
}
fn log(path: &Path, rows: &[Value]) {
    fs::create_dir_all(path.parent().unwrap()).unwrap();
    let mut file = fs::File::create(path).unwrap();
    for row in rows {
        writeln!(file, "{row}").unwrap();
    }
}
fn count(t: i64, total: u64, last: u64, limits: Value) -> Value {
    json!({"timestamp":at(t),"type":"event_msg","payload":{"type":"token_count","info":{"total_token_usage":{"total_tokens":total},"last_token_usage":{"total_tokens":last}},"rate_limits":limits}})
}

#[test]
fn codex_counts_deltas_by_day_and_deduplicates_archives_and_rate_updates() {
    let dir = tempfile::tempdir().unwrap();
    let period = period();
    let (old, yesterday, now) = (
        period.now - Duration::days(8).num_seconds(),
        period.now - 86_400,
        period.now,
    );
    let limits = json!({"limit_id":"codex","primary":{"used_percent":25.5,"window_minutes":300,"resets_at":now+1200},"secondary":{"used_percent":50,"window_minutes":10080,"resets_at":now+3000}});
    let records = [
        count(old, 90, 90, Value::Null),
        count(yesterday, 100, 10, Value::Null),
        count(now - 10, 115, 15, limits.clone()),
        count(now, 115, 15, limits),
    ];
    log(
        &dir.path().join("sessions/2025/01/20").join(rollout()),
        &records,
    );
    log(
        &dir.path().join("archived_sessions").join(rollout()),
        &records,
    );
    log(
        &dir.path().join("auth.json"),
        &[count(now, 999999, 88, Value::Null)],
    );
    let result = codex(dir.path(), &period);
    assert!(result.source_found && !result.partial);
    assert_eq!(
        (result.today_tokens, result.week_tokens, result.last_seen),
        (15, 25, Some(now))
    );
    assert_eq!(result.windows.len(), 2);
    assert_eq!(
        (
            result.windows[0].minutes,
            result.windows[0].used_percent,
            result.windows[0].resets_at,
            result.windows[0].observed_at
        ),
        (300, 25.5, Some(now + 1200), now)
    );
}

#[test]
fn rejects_expired_limits_other_buckets_and_fork_history() {
    let dir = tempfile::tempdir().unwrap();
    let p = period();
    let now = p.now;
    let metadata = json!({"type":"session_meta","timestamp":at(now-15),"payload":{"id":ID,"forked_from_id":"464a05be-4da1-4270-aa6c-0d1145080fd6"}});
    let records = [
        metadata,
        count(now - 40, 100, 100, Value::Null),
        count(
            now - 14,
            108,
            8,
            json!({"primary":{"window_minutes":300,"used_percent":101},"secondary":{"window_minutes":10080,"used_percent":20,"resets_at":now-1}}),
        ),
        count(
            now,
            108,
            8,
            json!({"limit_id":"other-model","primary":{"window_minutes":300,"used_percent":10,"resets_at":now+1200}}),
        ),
    ];
    log(
        &dir.path().join("archived_sessions").join(rollout()),
        &records,
    );
    let result = codex(dir.path(), &p);
    assert_eq!(result.today_tokens, 8);
    assert!(result.windows.is_empty());
}

fn assistant(t: i64, id: Option<&str>, request: &str, out: u64) -> Value {
    json!({"type":"assistant","timestamp":at(t),"requestId":request,"message":{"id":id,"role":"assistant","content":[{"type":"text","text":"do not retain"}],"usage":{"input_tokens":10,"cache_creation_input_tokens":4,"cache_read_input_tokens":6,"output_tokens":out}}})
}
#[test]
fn claude_sums_cache_tokens_and_deduplicates_partial_messages_and_subagents() {
    let dir = tempfile::tempdir().unwrap();
    let p = period();
    let root = dir.path().join("projects/project");
    let early = assistant(p.now - 10, Some("msg-1"), "req-1", 1);
    let final_block = assistant(p.now - 9, Some("msg-1"), "req-1", 3);
    log(
        &root.join(format!("{ID}.jsonl")),
        &[
            early.clone(),
            final_block,
            assistant(p.now - 10, Some("block-2"), "req-1", 1),
            assistant(p.now - 11, None, "req-1", 1),
            assistant(p.now - 86400, Some("msg-2"), "req-2", 4),
        ],
    );
    log(
        &root.join(ID).join("subagents/agent-f231.jsonl"),
        &[early, assistant(p.now, Some("msg-3"), "req-3", 5)],
    );
    log(
        &root.join("unrelated.jsonl"),
        &[assistant(p.now, Some("msg-secret"), "req-secret", 1000)],
    );
    let result = claude(dir.path(), &p);
    assert_eq!(
        (result.today_tokens, result.week_tokens, result.last_seen),
        (48, 72, Some(p.now))
    );
    assert!(result.source_found && !result.partial && result.windows.is_empty());
}
#[test]
fn truncated_codex_log_uses_observed_increment_without_assigning_history_to_today() {
    let dir = tempfile::tempdir().unwrap();
    let p = period();
    let logs = dir.path().join("archived_sessions");
    fs::create_dir_all(&logs).unwrap();
    let mut f = fs::File::create(logs.join(rollout())).unwrap();
    f.set_len(13 << 20).unwrap();
    use std::io::{Seek, SeekFrom};
    f.seek(SeekFrom::End(0)).unwrap();
    writeln!(f, "\n{}", count(p.now, 998, 7, Value::Null)).unwrap();
    let result = codex(dir.path(), &p);
    assert_eq!(result.week_tokens, 7);
    assert!(result.partial);
}

#[test]
fn missing_roots_are_distinct_from_empty_sessions() {
    let dir = tempfile::tempdir().unwrap();
    assert!(!claude(dir.path(), &period()).source_found);
    fs::create_dir(dir.path().join("projects")).unwrap();
    let result = claude(dir.path(), &period());
    assert!(result.source_found && result.last_seen.is_none());
}

#[cfg(unix)]
#[test]
fn refuses_symlinked_projects_and_files() {
    use std::os::unix::fs::symlink;
    let dir = tempfile::tempdir().unwrap();
    let p = period();
    let outside = dir.path().join("unrelated");
    let path = outside.join(format!("{ID}.jsonl"));
    log(&path, &[assistant(p.now, Some("msg-1"), "r", 1000)]);
    let project = dir.path().join("projects/safe");
    fs::create_dir_all(&project).unwrap();
    symlink(&outside, dir.path().join("projects/linked-dir")).unwrap();
    symlink(path, project.join(format!("{ID}.jsonl"))).unwrap();
    assert!(claude(dir.path(), &p).last_seen.is_none());
}

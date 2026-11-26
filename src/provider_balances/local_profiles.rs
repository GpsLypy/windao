//! Local profiles bind to a user-selected usage source. No login data is required.
use super::{Quota, Reading, Result};
use chrono::{Local, Utc};
use serde::Deserialize;
use sha2::{Digest, Sha256};
use std::{fs::File, io::Read, path::Path};

pub(crate) fn codex(root: &Path) -> Result<(Reading, i64)> {
    if !root.is_absolute() || !root.is_dir() {
        return Err("请选择此账户的 Codex 数据目录。");
    }
    let report = crate::ai_usage::read_codex_profile(root);
    let windows: Vec<_> = report
        .windows
        .into_iter()
        .map(|w| {
            let label = if w.minutes.is_multiple_of(1440) {
                format!("{} 天", w.minutes / 1440)
            } else if w.minutes.is_multiple_of(60) {
                format!("{} 小时", w.minutes / 60)
            } else {
                format!("{} 分钟", w.minutes)
            };
            (
                Quota {
                    label,
                    used_percent: w.used_percent,
                    resets_at: w.resets_at,
                },
                w.observed_at,
            )
        })
        .collect();
    if let Some(observed) = windows.iter().map(|(_, time)| *time).max() {
        Ok((
            Reading::Plan(windows.into_iter().map(|(row, _)| row).collect()),
            observed,
        ))
    } else {
        let observed = report
            .last_seen
            .ok_or("近期没有用量记录。请在此账户使用 Codex 后刷新。")?;
        Ok((
            Reading::Units {
                summary: format!(
                    "今日 {}{} token",
                    if report.partial { "≥" } else { "" },
                    report.today_tokens
                ),
                lines: vec![
                    format!("近 7 天 {} token", report.week_tokens),
                    format!("本机日期 {}", Local::now().date_naive()),
                ],
            },
            observed,
        ))
    }
}

#[derive(Deserialize)]
struct History {
    version: u32,
    samples: Vec<Sample>,
}
#[derive(Deserialize)]
struct Sample {
    t: i64,
    org: String,
    u: Percentages,
}
#[derive(Deserialize)]
struct Percentages {
    fh: Option<f64>,
    sd: Option<f64>,
}

pub(crate) fn claude(path: &Path, bound_org: Option<&str>) -> Result<(Reading, i64, String)> {
    if !path.is_absolute()
        || path.file_name().and_then(|v| v.to_str()) != Some("plan-usage-history.json")
    {
        return Err("请选择 Claude 桌面版的 plan-usage-history.json。");
    }
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|f| f.take(4 * 1024 * 1024 + 1).read_to_end(&mut bytes))
        .map_err(|_| "无法读取 Claude 的用量历史。")?;
    if bytes.len() > 4 * 1024 * 1024 {
        return Err("Claude 用量历史超过 4 MB。");
    }
    let history: History =
        serde_json::from_slice(bytes.strip_prefix(&[0xef, 0xbb, 0xbf]).unwrap_or(&bytes))
            .map_err(|_| "Claude 用量历史格式无效。")?;
    if history.version != 2 {
        return Err("请选择包含组织与时间的 v2 用量历史。");
    }
    let now = Utc::now().timestamp_millis();
    let latest = history
        .samples
        .iter()
        .filter(|s| !s.org.is_empty() && s.t > 0 && s.t <= now)
        .max_by_key(|s| s.t)
        .ok_or("请在 Claude 桌面版打开用量页面后刷新。")?;
    let org = format!("{:x}", Sha256::digest(latest.org.as_bytes()));
    if bound_org.is_some_and(|expected| expected != org) {
        return Err("此来源已切换组织。请切回原组织，或为新组织添加独立账户。");
    }
    let mut rows = vec![];
    for (label, percent) in [("5 小时", latest.u.fh), ("7 天", latest.u.sd)] {
        if let Some(percent) = percent.filter(|p| (0.0..=100.0).contains(p)) {
            rows.push(Quota {
                label: label.into(),
                used_percent: percent,
                resets_at: None,
            });
        }
    }
    if rows.is_empty() {
        return Err("Claude 未记录可用的订阅额度。");
    }
    Ok((Reading::Plan(rows), latest.t / 1000, org))
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn desktop_keeps_one_organization_and_reads_latest_observation() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("plan-usage-history.json");
        let save = |org: &str| {
            std::fs::write(
                &path,
                json!({"version":2,"samples":[
                    {"t":1250000050000_i64,"org":org,"u":{"fh":0,"sd":65.5}},
                    {"t":1250000000000_i64,"org":"older","u":{"fh":11}},
                    {"t":i64::MAX,"org":"future","u":{"fh":99}}
                ]})
                .to_string(),
            )
            .unwrap()
        };
        save("first");
        let (report, time, org) = claude(&path, None).unwrap();
        assert_eq!(time, 1250000050);
        assert!(
            matches!(report, Reading::Plan(ref rows) if rows.len()==2 && rows[0].used_percent==0.0 && rows[1].used_percent==65.5)
        );
        assert!(claude(&path, Some(&org)).is_ok());
        save("second");
        assert!(claude(&path, Some(&org)).is_err());
        assert!(codex(dir.path()).is_err());
    }
}

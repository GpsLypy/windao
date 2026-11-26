//! Google Antigravity CLI publishes this JSON to custom status lines on stdin.
//! https://antigravity.google/docs/cli/statusline/
use super::{Reading, Result};
use serde_json::{Map, Value, json};

fn label(v: &str) -> bool {
    !v.trim().is_empty() && v.chars().count() <= 100 && !v.chars().any(char::is_control)
}

/// Keep only published display fields. Never persist the full status line payload.
pub(super) fn snapshot(input: &Value) -> Result<Value> {
    if input["product"].as_str() != Some("antigravity") {
        return Err("请选择来自 Antigravity CLI 状态栏的用量 JSON。");
    }
    let buckets = input["quota"]
        .as_object()
        .filter(|m| !m.is_empty() && m.len() <= 64)
        .ok_or("Antigravity CLI 尚未提供额度记录。")?;
    let mut clean = Map::new();
    for (name, bucket) in buckets {
        if !label(name) {
            return Err("额度分组名称无效。");
        }
        let mut row = Map::new();
        if bucket["disabled"].as_bool() == Some(true) {
            row.insert("disabled".into(), Value::Bool(true));
        } else if let Some(fraction) = bucket["remaining_fraction"]
            .as_f64()
            .filter(|x| (0.0..=1.0).contains(x))
        {
            row.insert("remaining_fraction".into(), json!(fraction));
        } else if let Some(amount) = bucket["remaining_amount"].as_f64().filter(|x| *x >= 0.) {
            row.insert("remaining_amount".into(), json!(amount));
        } else {
            return Err("额度数据缺少有效的剩余比例或数量。");
        }
        if let Some(reset) = bucket["reset_time"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        {
            row.insert("reset_time".into(), json!(reset.to_rfc3339()));
        }
        clean.insert(name.clone(), Value::Object(row));
    }
    let model = input["model"]["display_name"]
        .as_str()
        .or_else(|| input["model"]["id"].as_str())
        .filter(|s| label(s));
    Ok(json!({"product":"antigravity","model":{"display_name":model},"quota":clean}))
}

pub(super) fn parse(input: &Value) -> Result<Reading> {
    let value = snapshot(input)?;
    let mut lines = Vec::new();
    if let Some(model) = value["model"]["display_name"].as_str() {
        lines.push(format!("当前模型 · {model}"));
    }
    let mut summary = None;
    for (name, bucket) in value["quota"].as_object().expect("validated buckets") {
        if bucket["disabled"].as_bool() == Some(true) {
            lines.push(format!("{name} · 已停用"));
            continue;
        }
        let remaining = if let Some(fraction) = bucket["remaining_fraction"].as_f64() {
            format!("剩余 {:.1}%", fraction * 100.)
        } else {
            format!(
                "剩余 {}",
                bucket["remaining_amount"]
                    .as_f64()
                    .expect("validated amount")
            )
        };
        let text = format!("{name} · {remaining}");
        summary.get_or_insert_with(|| text.clone());
        lines.push(text);
        if let Some(reset) = bucket["reset_time"]
            .as_str()
            .and_then(|s| chrono::DateTime::parse_from_rfc3339(s).ok())
        {
            lines.push(format!(
                "重置时间 {}",
                reset.with_timezone(&chrono::Local).format("%Y-%m-%d %H:%M")
            ));
        }
    }
    Ok(Reading::Units {
        summary: summary.unwrap_or_else(|| "额度分组已停用".into()),
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn callback_writes_only_model_and_quota_for_the_selected_account() {
        use crate::provider_balances::{
            Provider,
            profiles::{Profiles, read_report},
            statusline,
        };
        let tmp = tempfile::tempdir().unwrap();
        let mut accounts = Profiles::default();
        let id = accounts.add(Provider::Antigravity, "工作", 0, "").unwrap();
        accounts.save(tmp.path()).unwrap();
        let input = json!({
            "product":"antigravity", "email":"hidden@example.test", "model":{"id":"Gemini"},
            "transcript_path":"private-location", "quota":{"weekly":{"remaining_fraction":0.75}}
        })
        .to_string();
        let mut stdout = Vec::new();
        statusline::receive(
            Provider::Antigravity,
            input.as_bytes(),
            &mut stdout,
            tmp.path(),
            id,
        )
        .unwrap();
        let path = tmp.path().join("ai-statusline").join(format!("{id}.json"));
        let stored = std::fs::read_to_string(&path).unwrap();
        assert!(!stored.contains("example.test") && !stored.contains("private-location"));
        assert!(String::from_utf8(stdout).unwrap().contains("75.0%"));
        assert!(
            matches!(read_report(Provider::Antigravity,&path),Ok(Reading::Units{summary,..}) if summary=="weekly · 剩余 75.0%")
        );
    }

    #[test]
    fn public_status_line_keeps_quota_buckets_distinct_from_model_context() {
        let input = json!({"product":"antigravity","cwd":"private-path","email":"private-email",
            "model":{"display_name":"Gemini","other":"private-field"},"context_window":{"remaining_percentage":22},
            "quota":{"gemini-weekly":{"remaining_fraction":0.9378,"reset_time":"2026-07-06T07:50:32Z"},
                     "credits":{"remaining_amount":4}, "off":{"disabled":true}}});
        let sanitized = snapshot(&input).unwrap();
        assert!(!sanitized.to_string().contains("private"));
        let Reading::Units { lines, .. } = parse(&sanitized).unwrap() else {
            panic!()
        };
        for expected in [
            "当前模型 · Gemini",
            "gemini-weekly · 剩余 93.8%",
            "credits · 剩余 4",
            "off · 已停用",
        ] {
            assert!(lines.contains(&expected.to_owned()));
        }
        assert!(
            parse(&json!({"product":"antigravity","context_window":{"remaining_percentage":80}}))
                .is_err()
        );
    }
}

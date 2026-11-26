//! Conversions from service-reported balances to the units shown on the account card.
use super::{INVALID, Provider, Quota, Reading, Result, number};
use serde_json::Value;

fn timestamp(v: &Value) -> Option<i64> {
    chrono::DateTime::parse_from_rfc3339(v.as_str().unwrap_or(""))
        .map(|d| d.timestamp())
        .ok()
        .or_else(|| {
            let n = number(v)?;
            (n.is_finite() && n > 0.).then_some(if n > 10_000_000_000. {
                (n / 1000.) as i64
            } else {
                n as i64
            })
        })
}
fn window(label: &str, used: f64, reset: &Value) -> Result<Quota> {
    if !(0.0..=100.0).contains(&used) {
        return Err(INVALID);
    }
    Ok(Quota {
        label: label.into(),
        used_percent: used,
        resets_at: timestamp(reset),
    })
}
pub(super) fn parse(provider: Provider, v: &Value) -> Result<Reading> {
    match provider {
        Provider::KimiCode => {
            let mut rows = Vec::new();
            for (key, label) in [
                ("limit_5h", "5 小时"),
                ("limit_7d", "本周"),
                ("limit_month_total", "本月总额度"),
                ("limit_month_code", "本月编程"),
            ] {
                let record = &v["usages"][key];
                if let Some(ratio) = number(&record["used_ratio"]) {
                    rows.push(window(label, ratio * 100., &record["reset_time"])?);
                }
            }
            if rows.is_empty() {
                let detail = |label: &str, item: &Value| -> Result<Quota> {
                    let limit = number(&item["limit"]).filter(|v| *v > 0.).ok_or(INVALID)?;
                    let used = number(&item["used"])
                        .or_else(|| Some(limit - number(&item["remaining"])?))
                        .ok_or(INVALID)?;
                    window(label, used / limit * 100., &item["resetTime"])
                };
                if v["usage"].is_object() {
                    rows.push(detail("本周", &v["usage"])?);
                }
                for limit in v["limits"].as_array().into_iter().flatten().take(20) {
                    let unit = match limit["window"]["timeUnit"].as_str() {
                        Some("TIME_UNIT_HOUR") => "小时",
                        Some("TIME_UNIT_MINUTE") => "分钟",
                        Some("TIME_UNIT_DAY") => "天",
                        _ => "周期",
                    };
                    let label = format!(
                        "{} {unit}",
                        limit["window"]["duration"].as_i64().unwrap_or(1)
                    );
                    rows.push(detail(&label, &limit["detail"])?);
                }
            }
            if rows.is_empty() {
                return Err(INVALID);
            }
            Ok(Reading::Plan(rows))
        }
        Provider::ClaudeStatus => {
            let mut rows = Vec::new();
            for (key, label) in [("five_hour", "5 小时"), ("seven_day", "近 7 天")] {
                let from_cli = &v["rate_limits"][key];
                let item = if from_cli.is_object() {
                    from_cli
                } else {
                    &v[key]
                };
                if let Some(used) =
                    number(&item["used_percentage"]).or_else(|| number(&item["utilization"]))
                {
                    rows.push(window(label, used, &item["resets_at"])?);
                }
            }
            if rows.is_empty() {
                return Err(INVALID);
            }
            Ok(Reading::Plan(rows))
        }
        Provider::Gemini => {
            let usage = v
                .as_array()
                .into_iter()
                .flatten()
                .find_map(|item| item.pointer("/data/viewer/rpcs/getUsageInfo"))
                .unwrap_or(v);
            let mut rows = Vec::new();
            for pool in usage["pools"].as_array().ok_or(INVALID)? {
                let label = match pool["poolType"].as_str() {
                    Some("GXU_FIVE_HOURLY") => "5 小时",
                    Some("GXU_WEEKLY") => "本周",
                    _ => continue,
                };
                rows.push(window(
                    label,
                    number(&pool["percentageUsed"]).ok_or(INVALID)? * 100.,
                    &pool["gxu"]["windowRefreshesAt"],
                )?);
            }
            if rows.is_empty() {
                return Err(INVALID);
            }
            Ok(Reading::Plan(rows))
        }
        Provider::GrokBot => {
            if v["usesPooledEnterpriseAllowance"] == true || v["hasNonZeroIncludedLimit"] == false {
                return Err("此账户没有独立的个人额度。");
            }
            Ok(Reading::Plan(vec![window(
                "本周",
                number(&v["usagePercent"]).ok_or(INVALID)?,
                &v["nextResetTimestampUtc"],
            )?]))
        }
        Provider::WorkBuddy => {
            if v["code"].as_i64() != Some(0) {
                return Err(INVALID);
            }
            let packages = v["data"]["Packages"].as_array().ok_or(INVALID)?;
            let mut lines = Vec::new();
            let mut total = 0.;
            for item in packages.iter().take(128) {
                let amount = number(&item["CycleRemainCapacity"])
                    .filter(|v| *v >= 0.)
                    .ok_or(INVALID)?;
                total += amount;
                let label = item["PackageCode"]
                    .as_str()
                    .unwrap_or("资源包")
                    .chars()
                    .take(50)
                    .collect::<String>();
                lines.push(format!("{label}　{amount} 积分"));
            }
            if let Some(plan) = v["data"]["SubscriptionPackageName"]
                .as_str()
                .filter(|s| !s.is_empty() && s.chars().count() <= 80)
            {
                lines.insert(0, format!("套餐　{plan}"));
            }
            if !total.is_finite() || packages.len() > 128 {
                return Err(INVALID);
            }
            Ok(Reading::Units {
                summary: format!("{total} 积分"),
                lines,
            })
        }
        Provider::KiroUs | Provider::KiroEu => kiro(v),
        _ => Err(INVALID),
    }
}
fn kiro(v: &Value) -> Result<Reading> {
    let credit = v["usageBreakdownList"]
        .as_array()
        .ok_or(INVALID)?
        .iter()
        .find(|r| r["resourceType"] == "CREDIT")
        .ok_or(INVALID)?;
    let amount = |row: &Value, a: &str, b: &str| {
        number(&row[a])
            .or_else(|| number(&row[b]))
            .filter(|v| *v >= 0.)
            .ok_or(INVALID)
    };
    let over = if credit["currentOveragesWithPrecision"].is_null()
        && credit["currentOverages"].is_null()
    {
        0.
    } else {
        amount(credit, "currentOveragesWithPrecision", "currentOverages")?
    };
    let now = chrono::Utc::now().timestamp();
    let mut lines = vec![];
    let mut total = 0.;
    let mut add = |label: &str, row: &Value, overage: f64, expiry: Option<i64>| -> Result<()> {
        let limit = amount(row, "usageLimitWithPrecision", "usageLimit")?;
        let used = amount(row, "currentUsageWithPrecision", "currentUsage")?;
        let remaining = (limit - (used - overage).max(0.)).max(0.);
        total += remaining;
        let date = expiry
            .and_then(|s| chrono::DateTime::from_timestamp(s, 0))
            .map(|d| format!(" · {}", d.format("%Y-%m-%d")))
            .unwrap_or_default();
        lines.push(format!("{label}　{remaining:.1} / {limit:.1}{date}"));
        Ok(())
    };
    add(
        "月度额度",
        credit,
        over,
        timestamp(&credit["nextDateReset"]).or(timestamp(&v["nextDateReset"])),
    )?;
    fn active(row: &Value, field: &str, expires: &str, now: i64) -> Result<bool> {
        if row.is_null() {
            return Ok(false);
        }
        match row[field].as_str() {
            Some(s) if s.eq_ignore_ascii_case("EXPIRED") || s.eq_ignore_ascii_case("INACTIVE") => {
                Ok(false)
            }
            Some(s) if s.eq_ignore_ascii_case("ACTIVE") => {
                if row[expires].is_null() {
                    return Ok(true);
                }
                timestamp(&row[expires]).map(|t| t > now).ok_or(INVALID)
            }
            _ => Err(INVALID),
        }
    }
    let trial = &credit["freeTrialInfo"];
    if active(trial, "freeTrialStatus", "freeTrialExpiry", now)? {
        add("试用", trial, 0., timestamp(&trial["freeTrialExpiry"]))?;
    }
    let bonuses = match &credit["bonuses"] {
        Value::Array(v) => v.as_slice(),
        v @ Value::Object(_) => std::slice::from_ref(v),
        Value::Null => &[],
        _ => return Err(INVALID),
    };
    for bonus in bonuses {
        if active(bonus, "status", "expiresAt", now)? {
            let label = bonus["displayName"]
                .as_str()
                .filter(|s| !s.is_empty() && s.chars().count() <= 50)
                .unwrap_or("赠送或加购");
            add(label, bonus, 0., timestamp(&bonus["expiresAt"]))?;
        }
    }
    if let Some(plan) = v["subscriptionInfo"]["subscriptionTitle"]
        .as_str()
        .filter(|s| !s.is_empty() && s.chars().count() <= 80)
    {
        lines.insert(0, format!("套餐　{plan}"));
    }
    if v["overageConfiguration"]["overageStatus"]
        .as_str()
        .is_some_and(|s| s.eq_ignore_ascii_case("ENABLED"))
        && (!credit["overageCapWithPrecision"].is_null() || !credit["overageCap"].is_null())
    {
        let cap = amount(credit, "overageCapWithPrecision", "overageCap")?;
        lines.push(format!("超额上限　{cap:.1}"));
    }
    if over > 0. {
        lines.push(format!("超额使用　{over:.1}"));
    }
    if !total.is_finite() {
        return Err(INVALID);
    }
    Ok(Reading::Units {
        summary: format!("{total:.1} 点"),
        lines,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn reads_official_windows_and_rejects_unknown_allowances() {
        let gemini = json!([{ "data": {"viewer": {"rpcs":{"getUsageInfo":{
            "pools":[{"poolType":"GXU_WEEKLY","percentageUsed":0,"gxu":{"windowRefreshesAt":"2030-01-01T00:00:00Z"}},{"poolType":"UNKNOWN","percentageUsed":1}]
        }}}}}]);
        assert!(
            matches!(parse(Provider::Gemini,&gemini),Ok(Reading::Plan(rows)) if rows.len()==1 && rows[0].used_percent==0.0 && rows[0].resets_at==Some(1893456000))
        );
        assert!(
            parse(
                Provider::GrokBot,
                &json!({"usagePercent":40,"usesPooledEnterpriseAllowance":true})
            )
            .is_err()
        );
        assert!(
            matches!(parse(Provider::GrokBot,&json!({"usagePercent":100})), Ok(Reading::Plan(rows)) if rows[0].used_percent==100.0)
        );
        assert!(
            matches!(parse(Provider::KimiCode,&json!({"usage":{"limit":100,"remaining":40},"limits":[{"window":{"duration":5,"timeUnit":"TIME_UNIT_HOUR"},"detail":{"limit":20,"used":20}}]})), Ok(Reading::Plan(rows)) if rows.len()==2 && rows[0].used_percent==60.0)
        );
        assert!(
            parse(
                Provider::KimiCode,
                &json!({"usages":{"limit_5h":{"used_ratio":1.2}}})
            )
            .is_err()
        );
        assert!(
            matches!(parse(Provider::WorkBuddy,&json!({"code":0,"data":{"Packages":[{"PackageCode":"starter","CycleRemainCapacity":0}]}})),Ok(Reading::Units{summary,..}) if summary=="0 积分")
        );
    }

    #[test]
    fn kiro_keeps_overage_separate_and_skips_expired_pools() {
        let usage = json!({"usageBreakdownList":[{"resourceType":"CREDIT","usageLimit":50,"currentUsage":61,"currentOverages":11,
            "freeTrialInfo":{"freeTrialStatus":"ACTIVE","freeTrialExpiry":1,"usageLimit":100,"currentUsage":0},
            "bonuses":[{"status":"ACTIVE","expiresAt":1,"usageLimit":40,"currentUsage":0},{"status":"ACTIVE","expiresAt":4102444800_i64,"usageLimit":10,"currentUsage":2}]}]});
        assert!(
            matches!(kiro(&usage),Ok(Reading::Units{summary,lines}) if summary=="8.0 点" && lines.len()==3 && lines[2].contains("11.0"))
        );
    }
}

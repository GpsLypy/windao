//! DeepSeek /user/balance, Moonshot /v1/users/me/balance, and the official
//! zai-org/zai-coding-plugins glm-plan-usage plugin's quota/limit protocol.
use serde::{Deserialize, Serialize};
use serde_json::Value;
pub(crate) mod antigravity;
#[cfg(any(test, windows))]
pub(crate) mod authorized;
pub(crate) mod cc_switch;
pub(crate) mod local_profiles;
pub(crate) mod profiles;
pub(crate) mod relay;
pub(crate) mod statusline;
mod subscriptions;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub(crate) enum Provider {
    #[default]
    DeepSeek,
    GlmChina,
    GlmGlobal,
    KimiChina,
    KimiGlobal,
    KimiCode,
    KiroUs,
    KiroEu,
    GrokBot,
    WorkBuddy,
    Gemini,
    ClaudeStatus,
    Antigravity,
    CodexLocal,
    ClaudeDesktop,
    Relay,
}

impl Provider {
    pub const ALL: [Self; 16] = [
        Self::CodexLocal,
        Self::ClaudeDesktop,
        Self::DeepSeek,
        Self::GlmChina,
        Self::GlmGlobal,
        Self::KimiChina,
        Self::KimiGlobal,
        Self::KimiCode,
        Self::KiroUs,
        Self::KiroEu,
        Self::GrokBot,
        Self::WorkBuddy,
        Self::Gemini,
        Self::ClaudeStatus,
        Self::Antigravity,
        Self::Relay,
    ];

    pub fn title(self) -> &'static str {
        match self {
            Self::Relay => "中转站",
            Self::KimiCode => "Kimi Code 订阅",
            Self::KiroUs => "Kiro（美国东部）",
            Self::KiroEu => "Kiro（欧洲中部）",
            Self::GrokBot => "Grok Bot",
            Self::WorkBuddy => "WorkBuddy",
            Self::Gemini => "Gemini 网页订阅",
            Self::CodexLocal => "Codex 本机账户",
            Self::ClaudeDesktop => "Claude 桌面版",
            Self::ClaudeStatus => "Claude Code 状态栏",
            Self::Antigravity => "Antigravity CLI 状态栏",
            Self::DeepSeek => "DeepSeek API",
            Self::GlmChina => "智谱 GLM Coding Plan",
            Self::GlmGlobal => "Z.AI GLM Coding Plan",
            Self::KimiChina => "Kimi API（中国）",
            Self::KimiGlobal => "Kimi API（国际）",
        }
    }

    pub fn endpoint(self) -> &'static str {
        match self {
            Self::KimiCode => "https://api.kimi.com/coding/v1/usages",
            Self::KiroUs => "https://codewhisperer.us-east-1.amazonaws.com/",
            Self::KiroEu => "https://q.eu-central-1.amazonaws.com/",
            Self::WorkBuddy => "https://www.codebuddy.cn/billing/meter/get-user-resource-summary",
            Self::GrokBot => {
                "https://api2.cursor.sh/aiserver.v1.DashboardService/GetSandUsageStatus"
            }
            Self::Gemini
            | Self::Antigravity
            | Self::ClaudeStatus
            | Self::CodexLocal
            | Self::ClaudeDesktop
            | Self::Relay => "",
            Self::DeepSeek => "https://api.deepseek.com/user/balance",
            Self::GlmChina => "https://open.bigmodel.cn/api/monitor/usage/quota/limit",
            Self::GlmGlobal => "https://api.z.ai/api/monitor/usage/quota/limit",
            Self::KimiChina => "https://api.moonshot.cn/v1/users/me/balance",
            Self::KimiGlobal => "https://api.moonshot.ai/v1/users/me/balance",
        }
    }

    pub fn report_only(self) -> bool {
        matches!(
            self,
            Self::Gemini
                | Self::Antigravity
                | Self::ClaudeStatus
                | Self::CodexLocal
                | Self::ClaudeDesktop
                | Self::GrokBot
        )
    }
    pub fn is_plan(self) -> bool {
        matches!(self, Self::GlmChina | Self::GlmGlobal)
    }

    #[cfg(windows)]
    pub fn key_file(self, dir: &std::path::Path) -> std::path::PathBuf {
        dir.join(match self {
            Self::Relay => "relay.dpapi",
            Self::KimiCode => "kimi-code.dpapi",
            Self::KiroUs | Self::KiroEu => "kiro.dpapi",
            Self::WorkBuddy => "workbuddy.dpapi",
            Self::Gemini
            | Self::Antigravity
            | Self::ClaudeStatus
            | Self::CodexLocal
            | Self::ClaudeDesktop
            | Self::GrokBot => "report-only",
            Self::DeepSeek => "deepseek-api.dpapi",
            Self::GlmChina => "glm-cn-plan.dpapi",
            Self::GlmGlobal => "glm-global-plan.dpapi",
            Self::KimiChina => "moonshot-cn-api.dpapi",
            Self::KimiGlobal => "moonshot-global-api.dpapi",
        })
    }
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub(crate) struct Balance {
    pub currency: String,
    pub available: f64,
    pub cash: f64,
    pub granted: f64,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub(crate) struct Quota {
    pub label: String,
    pub used_percent: f64,
    pub resets_at: Option<i64>,
}

#[derive(Debug, PartialEq, Clone, Serialize, Deserialize)]
pub(crate) enum Reading {
    Units {
        summary: String,
        lines: Vec<String>,
    },
    Money {
        available: bool,
        balances: Vec<Balance>,
    },
    Plan(Vec<Quota>),
}

type Result<T> = std::result::Result<T, &'static str>;
const INVALID: &str = "服务商返回的额度数据无效，请稍后刷新。";
const AUTH: &str = "认证失败，请检查 API Key 和所选地区。";

fn number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str()?.parse().ok())
        .filter(|v| v.is_finite())
}

pub(crate) fn response(provider: Provider, status: u16, body: &str) -> Result<Reading> {
    match status {
        200 => {}
        401 | 403 => return Err(AUTH),
        429 => return Err("请求过于频繁，请稍后再试。"),
        300..=399 => return Err("服务商地址发生跳转，请检查所选地区。"),
        _ => return Err("服务商暂时无法处理请求，请稍后刷新。"),
    }
    if body.len() > 256 * 1024 {
        return Err(INVALID);
    }
    let body = body
        .trim_start()
        .strip_prefix(")]}'")
        .unwrap_or(body)
        .trim_start();
    let v: Value = serde_json::from_str(body).map_err(|_| INVALID)?;
    match provider {
        Provider::DeepSeek => {
            let available = v["is_available"].as_bool().ok_or(INVALID)?;
            let rows = v["balance_infos"]
                .as_array()
                .filter(|v| (1..=2).contains(&v.len()))
                .ok_or(INVALID)?;
            let mut balances = Vec::<Balance>::new();
            for row in rows {
                let currency = match row["currency"].as_str() {
                    Some("CNY") => "CNY",
                    Some("USD") => "USD",
                    _ => return Err(INVALID),
                };
                if balances.iter().any(|b| b.currency == currency) {
                    return Err(INVALID);
                }
                balances.push(Balance {
                    currency: currency.into(),
                    available: number(&row["total_balance"]).ok_or(INVALID)?,
                    cash: number(&row["topped_up_balance"]).ok_or(INVALID)?,
                    granted: number(&row["granted_balance"]).ok_or(INVALID)?,
                });
            }
            Ok(Reading::Money {
                available,
                balances,
            })
        }
        Provider::KimiChina | Provider::KimiGlobal => {
            let data = &v["data"];
            if v["status"].as_bool() == Some(false) || !v["error"].is_null() {
                return Err(INVALID);
            }
            let total = number(&data["available_balance"]).ok_or(INVALID)?;
            Ok(Reading::Money {
                available: total > 0.0,
                balances: vec![Balance {
                    currency: if provider == Provider::KimiChina {
                        "CNY"
                    } else {
                        "USD"
                    }
                    .into(),
                    available: total,
                    cash: number(&data["cash_balance"]).ok_or(INVALID)?,
                    granted: number(&data["voucher_balance"]).ok_or(INVALID)?,
                }],
            })
        }
        Provider::GlmChina | Provider::GlmGlobal => quota_response(&v),
        Provider::Antigravity => antigravity::parse(&v),
        other => subscriptions::parse(other, &v),
    }
}

fn quota_response(v: &Value) -> Result<Reading> {
    let code = v["code"]
        .as_i64()
        .or_else(|| v["code"].as_str()?.parse().ok());
    if !v["error"].is_null()
        || v["success"].as_bool() == Some(false)
        || code.is_some_and(|c| c != 0 && c != 200)
    {
        return Err(match code {
            Some(401 | 1000..=1004) => AUTH,
            Some(1302) => "请求过于频繁，请稍后再试。",
            Some(1309 | 1315 | 1220) => "请检查此 Key 是否关联有效的 Coding Plan 套餐。",
            _ => "额度查询未成功，请检查套餐状态和地区。",
        });
    }
    let data = v.get("data").unwrap_or(v);
    let rows = data["limits"]
        .as_array()
        .filter(|rows| rows.len() <= 32)
        .ok_or(INVALID)?;
    let mut quotas = Vec::new();
    for row in rows {
        let (kind, default_period) = match row["type"].as_str() {
            Some("TOKENS_LIMIT") => ("编程额度", "5 小时"),
            Some("CREDIT_LIMIT") => ("编程额度", ""),
            Some("TIME_LIMIT") => ("工具调用", "每月"),
            _ => continue,
        };
        let unit = match row["unit"].as_i64() {
            Some(1) => Some("天"),
            Some(3) => Some("小时"),
            Some(5) => Some("分钟"),
            Some(6) => Some("周"),
            _ => None,
        };
        let period = match (row["number"].as_u64().filter(|v| *v > 0), unit) {
            (Some(n), Some(unit)) => format!("{n} {unit}"),
            _ => default_period.to_owned(),
        };
        let percent = number(&row["percentage"])
            .filter(|v| (0.0..=100.0).contains(v))
            .ok_or(INVALID)?;
        let resets_at = row["nextResetTime"]
            .as_i64()
            .filter(|ts| *ts > 0)
            .map(|ts| if ts > 10_000_000_000 { ts / 1000 } else { ts })
            .filter(|ts| chrono::DateTime::from_timestamp(*ts, 0).is_some());
        quotas.push(Quota {
            label: format!("{period} {kind}").trim().to_owned(),
            used_percent: percent,
            resets_at,
        });
    }
    if quotas.is_empty() {
        return Err("此账号未返回可显示的套餐额度。");
    }
    Ok(Reading::Plan(quotas))
}

fn valid_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 4096 && key.bytes().all(|byte| byte.is_ascii_graphic())
}

#[cfg(windows)]
pub(super) mod connection {
    use super::*;
    use crate::credential_protection::dpapi;
    use std::{
        fs,
        io::{Read, Write},
        path::Path,
    };
    use zeroize::Zeroizing;

    const STORAGE: &str = "无法读取已保存的 API Key，请重新输入。";

    pub(crate) fn load(provider: Provider, dir: &Path) -> Result<Zeroizing<String>> {
        let mut bytes = Vec::new();
        fs::File::open(provider.key_file(dir))
            .map_err(|_| STORAGE)?
            .take(32_769)
            .read_to_end(&mut bytes)
            .map_err(|_| STORAGE)?;
        if bytes.len() > 32_768 {
            return Err(STORAGE);
        }
        let key = Zeroizing::new(dpapi(&bytes, false).map_err(|_| STORAGE)?);
        let value = std::str::from_utf8(&key).map_err(|_| STORAGE)?;
        if !(valid_key(value)
            || provider == Provider::WorkBuddy
                && value.len() <= 16384
                && serde_json::from_str::<serde_json::Value>(value).is_ok())
        {
            return Err(STORAGE);
        }
        Ok(Zeroizing::new(value.to_owned()))
    }

    pub(crate) fn save(provider: Provider, dir: &Path, key: &str) -> Result<()> {
        let error = "查询成功，但 Windows 未能保存密钥。";
        fs::create_dir_all(dir).map_err(|_| error)?;
        let bytes = dpapi(key.as_bytes(), true).map_err(|_| error)?;
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|_| error)?;
        file.write_all(&bytes)
            .and_then(|()| file.as_file().sync_all())
            .map_err(|_| error)?;
        file.persist(provider.key_file(dir)).map_err(|_| error)?;
        Ok(())
    }

    pub(crate) fn fetch(profile: &profiles::Profile, key: &str) -> Result<Reading> {
        let provider = profile.provider;
        if !valid_key(key) {
            return Err("请输入有效的 API Key，不能含有空格或换行。");
        }
        let mut header = Zeroizing::new(String::new());
        if !provider.is_plan() {
            header.push_str("Bearer ");
        }
        header.push_str(key);
        if provider == Provider::Relay {
            let base = relay::base(&profile.endpoint)?;
            if let Some(query) = &profile.cc_switch {
                let (status, body) = query
                    .request(key, &header, |headers| {
                        crate::calendar_sync::authorized_json("GET", &base, headers, "")
                    })
                    .map_err(|_| "连接 CC Switch 查询地址失败。")?;
                return query.read(status, &body);
            }
            if let Some(user) = profile.relay_user {
                let user = user.to_string();
                return relay::user_balance(&base, |url, needs_token| {
                    if needs_token {
                        crate::calendar_sync::authorized_json(
                            "GET",
                            url,
                            &[("Authorization", &header), ("New-Api-User", &user)],
                            "",
                        )
                    } else {
                        crate::calendar_sync::get_json_response(url, None)
                    }
                    .map_err(|_| "连接 New API 站点失败。")
                });
            }
            if !profile.balance_field.is_empty() {
                let (status, body) = crate::calendar_sync::get_json_response(&base, Some(&header))
                    .map_err(|_| "连接自定义接口失败。")?;
                if status != 200 {
                    return Err("自定义接口查询失败，请核对地址及使用权限。");
                }
                return relay::custom(&body, &profile.balance_field, &profile.unit);
            }
            return relay::query(&base, |url| {
                crate::calendar_sync::get_json_response(url, Some(&header))
                    .map_err(|_| "连接中转站失败。")
            });
        }
        let (status, body) = crate::calendar_sync::get_provider_json(provider, &header)
            .map_err(|_| "连接服务商失败，请检查网络后重试。")?;
        response(provider, status, &body)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn only_fixed_https_destinations_and_printable_credentials() {
        let mut origins = std::collections::HashSet::new();
        for provider in Provider::ALL
            .into_iter()
            .filter(|p| !p.report_only() && *p != Provider::Relay)
        {
            let url = url::Url::parse(provider.endpoint()).unwrap();
            assert_eq!(url.scheme(), "https");
            assert!(origins.insert(url.host_str().unwrap().to_owned()));
            assert!(!provider.title().is_empty());
            assert_eq!(provider.is_plan(), url.path().ends_with("quota/limit"));
        }
        for key in ["", "a b", "key\r\nInjected: x", "nul\0", &"a".repeat(4097)] {
            assert!(!valid_key(key));
        }
        assert!(valid_key("user-provided.fixture_only-123"));
    }

    #[test]
    fn independent_currencies_and_negative_kimi_cash_survive_parsing() {
        let v = json!({"is_available":true,"balance_infos":[{"currency":"CNY","total_balance":"10.05","granted_balance":"10","topped_up_balance":"0.05"},{"currency":"USD","total_balance":"0","granted_balance":"0","topped_up_balance":"0"}]});
        let Reading::Money {
            available,
            balances,
        } = response(Provider::DeepSeek, 200, &v.to_string()).unwrap()
        else {
            panic!()
        };
        assert!(available);
        assert_eq!(balances.len(), 2);
        assert_eq!(balances[0].available, 10.05);
        for (provider, currency) in [(Provider::KimiChina, "CNY"), (Provider::KimiGlobal, "USD")] {
            let v = json!({"status":true,"data":{"available_balance":-0.2,"cash_balance":-0.2,"voucher_balance":0}});
            assert_eq!(
                response(provider, 200, &v.to_string()),
                Ok(Reading::Money {
                    available: false,
                    balances: vec![Balance {
                        currency: currency.into(),
                        available: -0.2,
                        cash: -0.2,
                        granted: 0.0
                    }]
                })
            );
        }
    }

    #[test]
    fn glm_keeps_multiple_quota_windows_and_does_not_echo_responses() {
        let v = json!({"success":true,"code":200,"data":{"limits":[{"type":"CREDIT_LIMIT","unit":3,"number":5,"percentage":30,"nextResetTime":1795500000000_i64},{"type":"CREDIT_LIMIT","unit":6,"number":1,"percentage":20},{"type":"TIME_LIMIT","percentage":0}]}});
        let Reading::Plan(quotas) = response(Provider::GlmChina, 200, &v.to_string()).unwrap()
        else {
            panic!()
        };
        assert_eq!(
            quotas[0],
            Quota {
                label: "5 小时 编程额度".into(),
                used_percent: 30.0,
                resets_at: Some(1795500000)
            }
        );
        assert_eq!(quotas[1].label, "1 周 编程额度");
        assert_eq!(quotas[2].label, "每月 工具调用");
        for (status, body) in [
            (200, r#"{"code":1001,"success":false,"msg":"PRIVATE"}"#),
            (401, "PRIVATE"),
            (302, "PRIVATE"),
            (
                200,
                r#"{"data":{"limits":[{"type":"TOKENS_LIMIT","percentage":"NaN"}]},"error":"PRIVATE"}"#,
            ),
        ] {
            assert!(
                !response(Provider::GlmGlobal, status, body)
                    .unwrap_err()
                    .contains("PRIVATE")
            );
        }
    }
}

#[cfg(all(windows, test))]
mod credential_tests {
    use super::{Provider, connection};
    #[test]
    fn remembered_credentials_are_separate_from_backup_and_other_regions() {
        let dir = tempfile::tempdir().unwrap();
        let provider = Provider::GlmChina;
        let key = "synthetic-dpapi-test-key";
        let store = crate::workspace::Store::open(dir.path().into()).unwrap();
        connection::save(provider, dir.path(), key).unwrap();
        let sealed = std::fs::read(provider.key_file(dir.path())).unwrap();
        assert!(!sealed.windows(key.len()).any(|part| part == key.as_bytes()));
        assert_eq!(&**connection::load(provider, dir.path()).unwrap(), key);
        assert!(connection::load(Provider::GlmGlobal, dir.path()).is_err());
        let backup = dir.path().join("backup.json");
        crate::workspace::backup::export(&store, &backup).unwrap();
        let json = std::fs::read_to_string(backup).unwrap();
        assert!(!json.contains(key));
        assert!(!json.contains("dpapi"));
    }
}

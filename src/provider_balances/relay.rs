use super::{Reading, Result, number};
use serde_json::Value;
use url::Url;

pub(crate) fn base(address: &str) -> Result<Url> {
    let url = Url::parse(address.trim()).map_err(|_| "请填写中转站的 HTTPS 地址。")?;
    if url.scheme() != "https"
        || !url.has_host()
        || url.password().is_some()
        || !url.username().is_empty()
        || url.query().is_some()
        || url.fragment().is_some()
    {
        return Err("地址只能包含 HTTPS 主机及可选的服务路径。");
    }
    Ok(url)
}
fn report(amount: f64, unit: &str, lines: Vec<String>) -> Result<Reading> {
    if !amount.is_finite()
        || unit.is_empty()
        || unit.chars().count() > 12
        || unit.chars().any(char::is_control)
    {
        return Err("中转站返回的余额或单位无效。");
    }
    Ok(Reading::Units {
        summary: format!("{amount} {unit}"),
        lines,
    })
}
pub(crate) fn custom(raw: &str, pointer: &str, unit: &str) -> Result<Reading> {
    if raw.len() > 262144 || !pointer.starts_with('/') || pointer.len() > 200 {
        return Err("自定义接口的余额字段无效。");
    }
    let json: Value = serde_json::from_str(raw).map_err(|_| "中转站未返回 JSON。")?;
    if !json["error"].is_null() || json["success"] == false {
        return Err("中转站返回查询失败。");
    }
    let value = json
        .pointer(pointer)
        .and_then(number)
        .ok_or("所选 JSON 字段未返回数值。")?;
    report(value, unit, Vec::new())
}
pub(crate) fn query(
    url: &Url,
    mut request: impl FnMut(&Url) -> Result<(u16, String)>,
) -> Result<Reading> {
    let fetch = |path: &str,
                 request: &mut dyn FnMut(&Url) -> Result<(u16, String)>|
     -> Result<Option<Value>> {
        let mut target = url.clone();
        let prefix = url.path().trim_end_matches('/');
        let prefix = if path == "/v1/usage" {
            prefix.strip_suffix("/v1").unwrap_or(prefix)
        } else {
            prefix
        };
        target.set_path(&format!("{prefix}{path}"));
        let (code, body) = request(&target)?;
        match code {
            401 | 403 => return Err("中转站拒绝此密钥，请检查地址及权限。"),
            429 => return Err("查询过于频繁。"),
            404 | 405 => return Ok(None),
            300..=399 => return Err("中转站发生跳转，请填写最终地址。"),
            200 => {}
            _ => return Err("中转站暂时不可用。"),
        }
        if body.len() > 262_144 {
            return Err("中转站返回内容过大。");
        }
        Ok(serde_json::from_str::<Value>(&body).ok().filter(|v| {
            v.is_object() && v["error"].is_null() && v["success"] != false && v["isValid"] != false
        }))
    };
    for path in [
        "/user/balance",
        "/api/usage/token/",
        "/v1/usage",
        "/dashboard/billing/subscription",
    ] {
        let Some(value) = fetch(path, &mut request)? else {
            continue;
        };
        match path {
            "/user/balance" if value["is_active"] != false => {
                if let Some(n) = number(&value["balance"]) {
                    return report(n, value["unit"].as_str().unwrap_or("USD"), vec![]);
                }
                if value["balance_infos"].is_array() {
                    return super::response(super::Provider::DeepSeek, 200, &value.to_string());
                }
            }
            "/api/usage/token/" => {
                let data = if value["object"] == "token_usage" {
                    &value
                } else {
                    &value["data"]
                };
                if data["object"] != "token_usage" {
                    continue;
                }
                if data["unlimited_quota"] == true {
                    return Ok(Reading::Units {
                        summary: "不限额".into(),
                        lines: vec![],
                    });
                }
                if let Some(n) = number(&data["total_available"]) {
                    return report(n, "站点配额", vec![]);
                }
            }
            "/v1/usage" => {
                if let Some(n) =
                    number(&value["remaining"]).or_else(|| number(&value["quota"]["remaining"]))
                {
                    return report(
                        n,
                        value["unit"]
                            .as_str()
                            .or(value["quota"]["unit"].as_str())
                            .unwrap_or("USD"),
                        vec![],
                    );
                }
                if value["mode"] == "unrestricted" {
                    return Ok(Reading::Units {
                        summary: "不限额".into(),
                        lines: vec![],
                    });
                }
            }
            "/dashboard/billing/subscription" if value["object"] == "billing_subscription" => {
                let limit = number(&value["hard_limit_usd"]).ok_or("未返回计费上限。")?;
                let used = fetch("/dashboard/billing/usage", &mut request)?
                    .filter(|v| v["object"] == "list")
                    .and_then(|v| number(&v["total_usage"]))
                    .ok_or("未返回计费用量。")?
                    / 100.;
                if limit == 100_000_000. {
                    return Ok(Reading::Units {
                        summary: "限额未知".into(),
                        lines: vec![format!("已用 {used} 站点单位")],
                    });
                }
                return report(
                    limit - used,
                    "站点单位",
                    vec![format!("已用 {used} / {limit}")],
                );
            }
            _ => {}
        }
    }
    Err("站点未返回兼容的余额或额度接口。")
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn probes_one_origin_and_uses_provider_units() {
        let url = base("https://relay.example/api/v1").unwrap();
        let mut seen = 0;
        let value = query(&url, |target| {
            assert_eq!(target.origin(), url.origin());
            seen += 1;
            if target.path().ends_with("/v1/usage") {
                Ok((200, r#"{"remaining":0,"unit":"CNY"}"#.into()))
            } else {
                Ok((404, String::new()))
            }
        })
        .unwrap();
        assert!(matches!(value,Reading::Units{summary,..} if summary=="0 CNY"));
        assert_eq!(seen, 3);
        assert!(base("https://token@relay.example").is_err());
        assert!(query(&url, |_| Ok((302, String::new()))).is_err());
    }
}

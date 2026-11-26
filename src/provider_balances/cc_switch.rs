//! Import the data-only subset of CC Switch usage_script, without a JS runtime.
use super::{Reading, Result, number, relay};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use url::Url;

mod parser;
const UNSUPPORTED: &str =
    "此 CC Switch 配置超出支持范围。请使用内置模板或单个 GET 与数值字段表达式。";
const BAD_RESPONSE: &str = "站点返回的额度数据无效。";
const NEW_API: &str = include_str!("../../tests/fixtures/cc-switch/newapi.js");
const SUB2: &str = include_str!("../../tests/fixtures/cc-switch/sub2api.js");

#[derive(Clone, Serialize, Deserialize)]
pub(crate) struct Query {
    headers: Headers,
    pub user_id: Option<u64>,
    extractor: Extractor,
}
#[derive(Clone, Copy, PartialEq, Serialize, Deserialize)]
pub(crate) enum Auth {
    Bearer,
    Raw,
    ApiKey,
}
#[derive(Clone, Serialize, Deserialize)]
struct Headers {
    auth: Auth,
    agent: bool,
    json: bool,
}
#[derive(Clone, Serialize, Deserialize)]
enum Extractor {
    NewApi,
    Sub2,
    Fields {
        remaining: Vec<Op>,
        used: Option<Vec<Op>>,
        total: Option<Vec<Op>>,
        unit: String,
        valid: Option<String>,
    },
}
#[derive(Clone, Serialize, Deserialize)]
enum Op {
    Field(String),
    Number(f64),
    Add,
    Sub,
    Mul,
    Div,
}

fn calculate(ops: &[Op], json: &Value) -> Result<f64> {
    let mut stack = Vec::<f64>::new();
    for op in ops {
        let n = match op {
            Op::Number(n) => *n,
            Op::Field(path) => json.pointer(path).and_then(number).ok_or(BAD_RESPONSE)?,
            _ => {
                let b = stack.pop().ok_or(BAD_RESPONSE)?;
                let a = stack.pop().ok_or(BAD_RESPONSE)?;
                match op {
                    Op::Add => a + b,
                    Op::Sub => a - b,
                    Op::Mul => a * b,
                    Op::Div => a / b,
                    _ => unreachable!(),
                }
            }
        };
        if !n.is_finite() {
            return Err(BAD_RESPONSE);
        }
        stack.push(n);
    }
    if stack.len() != 1 {
        return Err(BAD_RESPONSE);
    }
    Ok(stack[0])
}

fn checked_endpoint(base: &Url, template: &str) -> Result<Url> {
    let raw = match template.strip_prefix("{{baseUrl}}") {
        Some(suffix) if suffix.is_empty() || suffix.starts_with('/') => {
            format!("{}{suffix}", base.as_str().trim_end_matches('/'))
        }
        Some(_) => return Err(UNSUPPORTED),
        None => template.to_string(),
    };
    if raw.len() > 2048 || raw.contains(['{', '}', '\\']) || raw.chars().any(char::is_control) {
        return Err(UNSUPPORTED);
    }
    let target = relay::base(&raw)?;
    if target.origin() != base.origin() {
        return Err("查询地址必须与账户的 HTTPS 地址同源（主机和端口一致）。");
    }
    Ok(target)
}

pub(crate) fn import(raw: &str, site: &str, user: &str) -> Result<(Url, Query)> {
    if raw.len() > 32_768 {
        return Err("配置最多 32 KiB。");
    }
    let wrapper = serde_json::from_str::<Value>(raw).ok();
    let cfg = wrapper.as_ref().map(|v| {
        v.pointer("/meta/usage_script")
            .or_else(|| v.get("usage_script"))
            .unwrap_or(v)
    });
    if cfg.is_some_and(|v| v.get("language").is_some_and(|x| x != "javascript")) {
        return Err(UNSUPPORTED);
    }
    let code = match cfg {
        Some(v) => v["code"].as_str().ok_or(UNSUPPORTED)?,
        None => raw,
    };
    let site = if site.trim().is_empty() {
        cfg.and_then(|v| v["baseUrl"].as_str()).unwrap_or("")
    } else {
        site
    };
    let base = relay::base(site)?;
    let tokens = parser::tokens(code)?;
    let (template, headers, needs_user, extractor) = if parser::preset(&tokens, NEW_API) {
        (
            "{{baseUrl}}/api/user/self".to_string(),
            Headers {
                auth: Auth::Bearer,
                agent: true,
                json: true,
            },
            true,
            Extractor::NewApi,
        )
    } else if parser::preset(&tokens, SUB2) {
        let root = base.as_str().trim_end_matches('/');
        let root = root.strip_suffix("/v1").unwrap_or(root);
        (
            format!("{root}/v1/usage"),
            Headers {
                auth: Auth::Bearer,
                agent: true,
                json: false,
            },
            false,
            Extractor::Sub2,
        )
    } else {
        parser::custom(&tokens)?
    };
    let endpoint = checked_endpoint(&base, &template)?;
    let user_id = if needs_user {
        let id = if user.trim().is_empty() {
            cfg.and_then(|v| v["userId"].as_str()).unwrap_or("")
        } else {
            user.trim()
        };
        Some(
            id.parse::<u64>()
                .ok()
                .filter(|n| *n > 0)
                .ok_or("请填写 New API 用户 ID。")?,
        )
    } else {
        None
    };
    Ok((
        endpoint,
        Query {
            headers,
            user_id,
            extractor,
        },
    ))
}

fn label(text: &str, max: usize) -> Result<&str> {
    if text.is_empty() || text.chars().count() > max || text.chars().any(char::is_control) {
        Err(BAD_RESPONSE)
    } else {
        Ok(text)
    }
}
impl Query {
    pub fn request<T>(
        &self,
        key: &str,
        bearer: &str,
        send: impl FnOnce(&[(&str, &str)]) -> T,
    ) -> T {
        let mut headers = vec![match self.headers.auth {
            Auth::Bearer => ("Authorization", bearer),
            Auth::Raw => ("Authorization", key),
            Auth::ApiKey => ("X-Api-Key", key),
        }];
        let user = self.user_id.map(|id| id.to_string());
        if let Some(id) = &user {
            headers.push(("New-Api-User", id));
        }
        if self.headers.agent {
            headers.push(("User-Agent", "cc-switch/1.0"));
        }
        if self.headers.json {
            headers.push(("Content-Type", "application/json"));
        }
        send(&headers)
    }
    pub fn read(&self, status: u16, raw: &str) -> Result<Reading> {
        if status != 200 {
            return Err("CC Switch 接口查询失败，请核对站点和凭据。");
        }
        if raw.len() > 262_144 {
            return Err(BAD_RESPONSE);
        }
        let data: Value = serde_json::from_str(raw).map_err(|_| BAD_RESPONSE)?;
        if !data.is_object()
            || !data["error"].is_null()
            || data["success"] == false
            || data["isValid"] == false
        {
            return Err(BAD_RESPONSE);
        }
        let (remaining, used, total, unit, plan) = match &self.extractor {
            Extractor::Fields {
                remaining,
                used,
                total,
                unit,
                valid,
            } => {
                if valid
                    .as_ref()
                    .is_some_and(|path| data.pointer(path).and_then(Value::as_bool) != Some(true))
                {
                    return Err(BAD_RESPONSE);
                }
                (
                    Some(calculate(remaining, &data)?),
                    used.as_ref().map(|v| calculate(v, &data)).transpose()?,
                    total.as_ref().map(|v| calculate(v, &data)).transpose()?,
                    unit.as_str(),
                    None,
                )
            }
            Extractor::NewApi => {
                if data["success"] != true {
                    return Err(BAD_RESPONSE);
                }
                let d = &data["data"];
                let quota = number(&d["quota"]).ok_or(BAD_RESPONSE)?;
                let used = number(&d["used_quota"]).ok_or(BAD_RESPONSE)?;
                (
                    Some(quota / 500000.),
                    Some(used / 500000.),
                    Some((quota + used) / 500000.),
                    "USD",
                    d["group"].as_str(),
                )
            }
            Extractor::Sub2 => {
                let q = &data["quota"];
                let total = number(&q["limit"]).filter(|n| *n > 0.);
                let used = total.and_then(|_| number(&q["used"]));
                (
                    data["remaining"].as_f64().filter(|n| *n >= 0.),
                    used,
                    total,
                    data["unit"]
                        .as_str()
                        .or(q["unit"].as_str())
                        .unwrap_or("USD"),
                    data["planName"].as_str(),
                )
            }
        };
        let unit = label(unit, 12)?;
        let plan = plan
            .filter(|s| !s.is_empty())
            .map(|s| label(s, 80))
            .transpose()?;
        let summary = match remaining {
            Some(n) if n.is_finite() => format!("{n} {unit}"),
            None => plan.ok_or(BAD_RESPONSE)?.into(),
            _ => return Err(BAD_RESPONSE),
        };
        let mut lines = Vec::new();
        if remaining.is_some()
            && let Some(p) = plan
        {
            lines.push(p.to_owned());
        }
        for (name, n) in [("已用", used), ("总额", total)] {
            if let Some(n) = n {
                if !n.is_finite() {
                    return Err(BAD_RESPONSE);
                }
                lines.push(format!("{name} {n} {unit}"));
            }
        }
        Ok(Reading::Units { summary, lines })
    }
}

#[cfg(test)]
mod tests;

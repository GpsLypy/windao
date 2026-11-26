//! Explicit browser consent and account-owner supplied access for quota requests.
use super::Provider;
use serde_json::Value;
use url::Url;

pub(crate) fn account_page(provider: Provider) -> Option<&'static str> {
    match provider {
        Provider::Gemini => Some("https://gemini.google.com/usage"),
        Provider::GrokBot => Some("https://cursor.com/dashboard"),
        _ => None,
    }
}

pub(crate) fn write_share_tool(directory: &std::path::Path) -> Result<std::path::PathBuf, String> {
    use std::io::Write;
    let directory = directory.join("windao-usage-share");
    std::fs::create_dir_all(&directory).map_err(|_| "无法创建扩展目录。")?;
    for (name, contents) in [
        (
            "manifest.json",
            include_str!("../../assets/quota-share/manifest.json"),
        ),
        (
            "share.js",
            include_str!("../../assets/quota-share/share.js"),
        ),
        (
            "save.html",
            include_str!("../../assets/quota-share/save.html"),
        ),
        ("save.js", include_str!("../../assets/quota-share/save.js")),
    ] {
        let mut tmp =
            tempfile::NamedTempFile::new_in(&directory).map_err(|_| "无法写入分享扩展。")?;
        tmp.write_all(contents.as_bytes())
            .and_then(|_| tmp.as_file().sync_all())
            .map_err(|_| "写入扩展失败。")?;
        tmp.persist(directory.join(name))
            .map_err(|_| "安装分享扩展失败。")?;
    }
    Ok(directory)
}

fn login_target(value: &Value) -> Result<(String, Url), String> {
    let state = value
        .pointer("/data/state")
        .and_then(Value::as_str)
        .filter(|s| !s.is_empty() && s.len() <= 512)
        .ok_or("官网未返回授权请求。")?;
    let url = value
        .pointer("/data/authUrl")
        .and_then(Value::as_str)
        .and_then(|s| Url::parse(s).ok())
        .ok_or("官网未返回授权地址。")?;
    let exact = |name: &str, expected: &str| {
        let mut values = url.query_pairs().filter(|(k, _)| k == name);
        values.next().is_some_and(|(_, v)| v == expected) && values.next().is_none()
    };
    if value["code"].as_i64() != Some(0)
        || url.origin().ascii_serialization() != "https://www.codebuddy.cn"
        || url.path() != "/login"
        || !url.username().is_empty()
        || url.password().is_some()
        || url.fragment().is_some()
        || !exact("state", state)
        || !exact("platform", "windao")
    {
        return Err("官网授权地址与当前 WinDao 请求不匹配。".into());
    }
    Ok((state.to_owned(), url))
}

#[cfg(windows)]
pub(crate) use online::{connect_workbuddy, fetch};
#[cfg(windows)]
mod online {
    use super::{Provider, Url, Value, login_target};
    use crate::calendar_sync::authorized_json;
    use crate::provider_balances::{Reading, response};
    use serde::{Deserialize, Serialize};
    use std::{
        sync::atomic::{AtomicBool, Ordering},
        time::{Duration, Instant},
    };
    use zeroize::{Zeroize, Zeroizing};

    #[derive(Deserialize, Serialize)]
    struct Session {
        token: String,
        uid: String,
        domain: String,
    }
    impl Drop for Session {
        fn drop(&mut self) {
            self.token.zeroize();
        }
    }

    fn valid_token(token: &str) -> bool {
        !token.is_empty() && token.len() <= 65_536 && token.bytes().all(|b| b.is_ascii_graphic())
    }
    fn official(path: &str) -> Url {
        Url::parse(&format!("https://www.codebuddy.cn{path}")).expect("static WorkBuddy address")
    }

    pub(crate) fn fetch(provider: Provider, credential: &str) -> Result<Reading, String> {
        let (status, body) = match provider {
            Provider::KiroUs | Provider::KiroEu => {
                if !valid_token(credential) {
                    return Err("请输入有效的 Kiro 访问令牌。".into());
                }
                let address = if provider == Provider::KiroEu {
                    "https://q.eu-central-1.amazonaws.com/"
                } else {
                    "https://codewhisperer.us-east-1.amazonaws.com/"
                };
                let url = Url::parse(address).expect("static regional address");
                let auth = Zeroizing::new(format!("Bearer {credential}"));
                authorized_json(
                    "POST",
                    &url,
                    &[
                        ("Authorization", &auth),
                        ("Content-Type", "application/x-amz-json-1.0"),
                        ("X-Amz-Target", "AmazonCodeWhispererService.GetUsageLimits"),
                    ],
                    "{}",
                )?
            }
            Provider::WorkBuddy => {
                let session: Session = serde_json::from_str(credential)
                    .map_err(|_| "请通过 WorkBuddy 官网重新连接。")?;
                if !valid_token(&session.token) || session.uid.is_empty() {
                    return Err("WorkBuddy 授权信息无效。".into());
                }
                let auth = Zeroizing::new(format!("Bearer {}", session.token));
                authorized_json(
                    "POST",
                    &official("/billing/meter/get-user-resource-summary"),
                    &[
                        ("Authorization", &auth),
                        ("X-User-Id", &session.uid),
                        ("X-Domain", &session.domain),
                        ("Content-Type", "application/json"),
                    ],
                    "{}",
                )?
            }
            _ => return Err("此来源不支持访问令牌查询。".into()),
        };
        response(provider, status, &body).map_err(str::to_owned)
    }

    pub(crate) fn connect_workbuddy(
        cancel: &AtomicBool,
        open: impl FnOnce(&str) -> Result<(), String>,
    ) -> Result<(Zeroizing<String>, Reading), String> {
        let check = || {
            if cancel.load(Ordering::Acquire) {
                Err("已取消 WorkBuddy 授权。".to_owned())
            } else {
                Ok(())
            }
        };
        check()?;
        let (status, body) = authorized_json(
            "POST",
            &official("/v2/plugin/auth/state?platform=windao"),
            &[
                ("X-No-Authorization", "true"),
                ("X-No-User-Id", "true"),
                ("Content-Type", "application/json"),
            ],
            "{}",
        )?;
        if status != 200 {
            return Err(format!("官网无法发起授权（HTTP {status}）。"));
        }
        let (state, login) =
            login_target(&serde_json::from_str(&body).map_err(|_| "官网授权响应无效。")?)?;
        check()?;
        open(login.as_str())?;
        let mut poll = official("/v2/plugin/auth/token");
        poll.query_pairs_mut().append_pair("state", &state);
        let deadline = Instant::now() + Duration::from_secs(300);
        while Instant::now() < deadline {
            for _ in 0..10 {
                check()?;
                std::thread::sleep(Duration::from_millis(200));
            }
            let (status, raw) =
                authorized_json("GET", &poll, &[("X-No-Authorization", "true")], "")?;
            if status == 400 {
                continue;
            }
            if status != 200 {
                return Err(format!("官网授权轮询返回 HTTP {status}。"));
            }
            let raw = Zeroizing::new(raw);
            let value: Value = serde_json::from_str(&raw).map_err(|_| "官网授权结果无效。")?;
            if value["code"].as_i64() == Some(11217) {
                continue;
            }
            if value["code"].as_i64() != Some(0) {
                return Err("官网授权已结束，请重新连接。".into());
            }
            check()?;
            let token = value
                .pointer("/data/accessToken")
                .and_then(Value::as_str)
                .filter(|t| valid_token(t))
                .ok_or("官网未返回访问令牌。")?;
            let domain = value
                .pointer("/data/domain")
                .and_then(Value::as_str)
                .unwrap_or_default();
            let auth = Zeroizing::new(format!("Bearer {token}"));
            let (status, raw) = authorized_json(
                "GET",
                &official("/v2/plugin/accounts"),
                &[("Authorization", &auth), ("X-Domain", domain)],
                "",
            )?;
            if status != 200 {
                return Err(format!("无法读取授权账户（HTTP {status}）。"));
            }
            let accounts: Value = serde_json::from_str(&raw).map_err(|_| "官网账号格式无效。")?;
            let uid = accounts
                .pointer("/data/accounts")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
                .find(|a| {
                    a["enterpriseId"].as_str().unwrap_or_default().is_empty()
                        && a["isCurrentOneIdEnterprise"] != true
                })
                .and_then(|a| a["uid"].as_str())
                .filter(|s| !s.is_empty())
                .ok_or("请使用个人 WorkBuddy 账号授权。")?;
            let session = Session {
                token: token.to_owned(),
                uid: uid.to_owned(),
                domain: domain.to_owned(),
            };
            let secret =
                Zeroizing::new(serde_json::to_string(&session).map_err(|_| "无法保存授权结果。")?);
            check()?;
            let result = fetch(Provider::WorkBuddy, &secret)?;
            check()?;
            return Ok((secret, result));
        }
        Err("等待官网授权超时，请重新连接。".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn browser_authorization_pins_issuer_state_and_own_client() {
        let valid = "https://www.codebuddy.cn/login?state=abc&platform=windao";
        let target = |url: &str| {
            login_target(&serde_json::json!({"code":0,"data":{"state":"abc","authUrl":url}}))
        };
        assert!(target(valid).is_ok());
        for url in [
            valid.replace("windao", "other"),
            valid.replace("abc", "changed"),
            valid.replace(".cn/", ".cn.example/"),
            format!("{valid}&state=other"),
        ] {
            assert!(target(&url).is_err());
        }
        assert!(account_page(Provider::Gemini).unwrap().ends_with("/usage"));
        assert!(account_page(Provider::KiroUs).is_none());
    }
}

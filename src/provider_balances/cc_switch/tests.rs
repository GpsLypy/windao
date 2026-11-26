use super::*;
use serde_json::json;

const GENERAL: &str = include_str!("../../../tests/fixtures/cc-switch/general.js");
const SITE: &str = "https://quota.example/tenant";
fn summary(query: &Query, body: Value) -> String {
    let Reading::Units { summary, .. } = query.read(200, &body.to_string()).unwrap() else {
        panic!("expected units")
    };
    summary
}
#[test]
fn general_and_saved_provider_import_without_credentials() {
    let wrapper = json!({"meta":{"usage_script":{"code":GENERAL,"baseUrl":SITE,"language":"javascript","apiKey":"discarded-fixture"}}});
    let (url, q) = import(&wrapper.to_string(), "", "").unwrap();
    assert_eq!(url.as_str(), "https://quota.example/tenant/user/balance");
    assert_eq!(summary(&q, json!({"balance":0})), "0 USD");
    let stored = serde_json::to_string(&q).unwrap();
    assert!(!stored.contains("discarded-fixture") && !stored.contains("function"));
    q.request("fixture", "Bearer fixture", |headers| {
        assert!(headers.contains(&("Authorization", "Bearer fixture")))
    });
    assert!(q.read(302, "{}").is_err());
}
#[test]
fn newapi_uses_account_token_and_per_user_id_and_quota_scale() {
    let config = json!({"code":NEW_API.replace("Default Plan","默认套餐").replace("Query failed","查询失败"),"baseUrl":SITE,"userId":"42","accessToken":"discarded-fixture"});
    let (url, q) = import(&config.to_string(), "", "").unwrap();
    assert_eq!(url.path(), "/tenant/api/user/self");
    q.request("fixture", "Bearer fixture", |headers| {
        assert!(
            headers.contains(&("New-Api-User", "42"))
                && headers.contains(&("Authorization", "Bearer fixture"))
        )
    });
    assert_eq!(
        q.read(
            200,
            r#"{"success":true,"data":{"quota":500000,"used_quota":250000,"group":"work"}}"#
        ),
        Ok(Reading::Units {
            summary: "1 USD".into(),
            lines: vec!["work".into(), "已用 0.5 USD".into(), "总额 1.5 USD".into()]
        })
    );
    assert!(import(NEW_API, SITE, "").is_err());
    assert!(
        q.read(
            200,
            r#"{"success":false,"data":{"quota":500000,"used_quota":0}}"#
        )
        .is_err()
    );
}
#[test]
fn sub2_handles_version_suffix_limited_and_unlimited_plans() {
    for site in [SITE, &format!("{SITE}/v1/")] {
        let (url, q) = import(SUB2, site, "").unwrap();
        assert_eq!(url.path(), "/tenant/v1/usage");
        assert_eq!(
            summary(
                &q,
                json!({"remaining":0,"quota":{"unit":"CNY","limit":2,"used":2}})
            ),
            "0 CNY"
        );
        assert_eq!(
            summary(&q, json!({"remaining":-1,"planName":"工作订阅"})),
            "工作订阅"
        );
        assert!(q.read(200, r#"{"remaining":-1}"#).is_err());
    }
}
#[test]
fn custom_arithmetic_fields_use_precedence_array_index_and_validity_guard() {
    let code = r#"({request:{headers:{"X-Api-Key":"{{apiKey}}"},method:"GET",url:"https://quota.example/credits"},extractor: response => ({remaining:(response.rows[0].all-response.used*2)/100.0,unit:'CNY',isValid:response.ok})})"#;
    let (_, q) = import(code, SITE, "").unwrap();
    assert_eq!(
        summary(&q, json!({"rows":[{"all":"153"}],"used":1.5,"ok":true})),
        "1.5 CNY"
    );
    q.request("fixture", "Bearer fixture", |headers| {
        assert_eq!(headers, [("X-Api-Key", "fixture")])
    });
    assert!(
        q.read(200, r#"{"rows":[{"all":3}],"used":1,"ok":false}"#)
            .is_err()
    );
    assert!(q.read(200, r#"{"ok":true}"#).is_err());
    let (_, invalid) = import(&code.replace("/100.0", "/0"), SITE, "").unwrap();
    assert!(
        invalid
            .read(200, r#"{"rows":[{"all":3}],"used":1,"ok":true}"#)
            .is_err()
    );
}
#[test]
fn rejects_unrelated_origins_literals_and_executable_configuration() {
    for url in [
        "http://quota.example/b",
        "https://other.example/x",
        "https://quota.example:444/x",
        "https://key@quota.example/x",
        "{{baseUrl}}/q?key={{apiKey}}",
        "{{baseUrl}}.elsewhere.example/x",
    ] {
        assert!(import(&GENERAL.replace("{{baseUrl}}/user/balance", url), SITE, "").is_err());
    }
    for (old, new) in [
        ("GET", "POST"),
        ("Bearer {{apiKey}}", "Bearer literal"),
        ("Authorization", "Cookie"),
        ("response.balance", "fetch('extra')"),
        ("response.balance", "1"),
        ("response.balance", "response.balance; doMore()"),
        ("return {", "while(true){} return {"),
    ] {
        assert!(import(&GENERAL.replace(old, new), SITE, "").is_err());
    }
    assert!(import(&format!("{GENERAL}; extra()"), SITE, "").is_err());
    assert!(import(&NEW_API.replace("500000", "250000"), SITE, "42").is_err());
}

#[test]
fn saved_profiles_reopen_with_only_normalized_query_and_work_alongside_legacy_accounts() {
    use crate::provider_balances::{Provider, profiles::Profiles};
    let dir = tempfile::tempdir().unwrap();
    let mut profiles = Profiles::default();
    profiles.add(Provider::Relay, "旧账户", 0, SITE).unwrap();
    let source =
        json!({"usage_script":{"code":GENERAL,"apiKey":"discarded-fixture","baseUrl":SITE}});
    let (endpoint, query) = import(&source.to_string(), "", "").unwrap();
    profiles
        .add(Provider::Relay, "导入账户", 1, endpoint.as_str())
        .unwrap();
    profiles.accounts[1].cc_switch = Some(query);
    profiles.save(dir.path()).unwrap();
    let stored = std::fs::read_to_string(dir.path().join("ai-accounts.json")).unwrap();
    assert!(!stored.contains("discarded-fixture") && !stored.contains("function"));
    let reopened = Profiles::open(dir.path()).unwrap();
    assert!(reopened.accounts[0].cc_switch.is_none());
    assert_eq!(reopened.accounts[1].endpoint, endpoint.as_str());
    assert_eq!(
        summary(
            reopened.accounts[1].cc_switch.as_ref().unwrap(),
            json!({"balance":1.25})
        ),
        "1.25 USD"
    );
}

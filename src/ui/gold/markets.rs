use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::Path;

pub(super) const GLOBAL: &[(&str, &str)] = &[
    ("gb_$dji", "道琼斯"),
    ("gb_ixic", "纳斯达克"),
    ("gb_$inx", "标普500"),
    ("int_hangseng", "恒生指数"),
    ("int_nikkei", "日经225"),
    ("DINIW", "美元指数"),
    ("fx_susdcny", "美元/人民币"),
    ("fx_susdjpy", "美元/日元"),
    ("fx_seurusd", "欧元/美元"),
];
pub(super) const CHINA: &[(&str, &str)] = &[
    ("sh000001", "上证指数"),
    ("sz399001", "深证成指"),
    ("sz399006", "创业板指"),
    ("sh000300", "沪深300"),
    ("sh000688", "科创50"),
];

pub(super) fn stock_id(raw: &str) -> Option<String> {
    let raw = raw.trim().to_ascii_lowercase();
    let (market, code) = if let Some(code) = raw.strip_prefix("sh") {
        ("sh", code)
    } else if let Some(code) = raw.strip_prefix("sz") {
        ("sz", code)
    } else {
        (if raw.starts_with('6') { "sh" } else { "sz" }, raw.as_str())
    };
    if code.len() != 6 || !code.bytes().all(|v| v.is_ascii_digit()) {
        return None;
    }
    match (market, code.as_bytes()[0]) {
        ("sh", b'6') | ("sz", b'0' | b'3') => Some(format!("{market}{code}")),
        _ => None,
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub(super) struct Stock {
    pub id: String,
    pub name: String,
}
impl Stock {
    fn valid(&self) -> bool {
        stock_id(&self.id).as_ref() == Some(&self.id)
            && !self.name.trim().is_empty()
            && self.name.chars().count() <= 80
            && !self.name.chars().any(char::is_control)
    }
}
pub(super) fn read_watch(dir: &Path) -> Result<Vec<Stock>, String> {
    let path = dir.join("market-watchlist.json");
    let file = match std::fs::File::open(path) {
        Ok(file) => file,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(vec![]),
        Err(_) => return Err("无法读取自选股。".into()),
    };
    if !file.metadata().is_ok_and(|m| m.len() < 64 * 1024) {
        return Err("自选股文件过大。".into());
    }
    let rows: Vec<Stock> = serde_json::from_reader(file).map_err(|_| "自选股文件格式无效。")?;
    let mut seen = HashSet::new();
    if rows.len() > 8 || !rows.iter().all(|r| r.valid() && seen.insert(&r.id)) {
        return Err("自选股记录无效。".into());
    }
    Ok(rows)
}
pub(super) fn write_watch(dir: &Path, rows: &[Stock]) -> Result<(), String> {
    let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|_| "无法保存自选股。")?;
    serde_json::to_writer(&mut file, rows).map_err(|_| "无法写入自选股。")?;
    file.as_file()
        .sync_all()
        .and_then(|_| {
            file.persist(dir.join("market-watchlist.json"))
                .map(|_| ())
                .map_err(|e| e.error)
        })
        .map_err(|_| "无法保存自选股。".into())
}
pub(super) fn search(text: &str) -> Result<Vec<Stock>, String> {
    let value = text
        .trim()
        .trim_end_matches(';')
        .strip_prefix("v_hint=")
        .ok_or("股票搜索格式无效。")?;
    let data: String = serde_json::from_str(value).map_err(|_| "股票搜索数据无效。")?;
    let mut seen = HashSet::new();
    Ok(data
        .split('^')
        .filter_map(|row| {
            let f: Vec<_> = row.split('~').collect();
            if f.len() != 5 || f[4] != "GP-A" || !matches!(f[0], "sh" | "sz") {
                return None;
            }
            let item = Stock {
                id: format!("{}{}", f[0], f[1]),
                name: f[2].into(),
            };
            (item.valid() && seen.insert(item.id.clone())).then_some(item)
        })
        .take(12)
        .collect())
}

#[derive(Debug)]
pub(super) struct Reading {
    pub price: f64,
    pub change: f64,
    pub percent: f64,
    pub stamp: String,
}
fn num(fields: &[&str], index: usize) -> Option<f64> {
    fields
        .get(index)?
        .parse::<f64>()
        .ok()
        .filter(|v| v.is_finite())
}
fn reading(id: &str, f: &[&str], global: bool) -> Option<Reading> {
    let (price, change, percent, stamp) = if !global {
        if f.len() < 38 || !id.ends_with(f[2]) || f[2].len() != 6 {
            return None;
        }
        let stamp = chrono::NaiveDateTime::parse_from_str(f[30], "%Y%m%d%H%M%S")
            .ok()?
            .format("%m-%d %H:%M")
            .to_string();
        (num(f, 3)?, num(f, 31)?, num(f, 32)?, stamp)
    } else {
        let price = num(f, 1)?;
        if id.starts_with("int_") {
            (price, num(f, 2)?, num(f, 3)?, "未提供".into())
        } else {
            let (change, percent, time) = if id.starts_with("gb_") {
                (num(f, 4)?, num(f, 2)?, f.get(3)?.to_string())
            } else if id == "DINIW" {
                let prev = num(f, 3)?;
                if prev <= 0. {
                    return None;
                }
                (
                    price - prev,
                    (price - prev) / prev * 100.,
                    format!("{} {}", f.get(10)?, f[0]),
                )
            } else {
                (num(f, 11)?, num(f, 10)?, format!("{} {}", f.get(17)?, f[0]))
            };
            let stamp = chrono::NaiveDateTime::parse_from_str(&time, "%Y-%m-%d %H:%M:%S")
                .ok()?
                .format("%m-%d %H:%M")
                .to_string();
            (price, change, percent, stamp)
        }
    };
    if price <= 0. || price - change <= 0. || !percent.is_finite() || percent.abs() >= 10000. {
        return None;
    }
    Some(Reading {
        price,
        change,
        percent,
        stamp,
    })
}

pub(super) fn quotes(
    text: &str,
    ids: &[String],
    global: bool,
) -> Result<HashMap<String, Reading>, String> {
    let prefix = if global { "var hq_str_" } else { "v_" };
    let mut seen = HashSet::new();
    let mut rows = HashMap::new();
    for line in text.lines() {
        let Some((id, raw)) = line
            .trim()
            .trim_end_matches(';')
            .strip_prefix(prefix)
            .and_then(|v| v.split_once('='))
        else {
            continue;
        };
        if !ids.iter().any(|v| v == id) {
            continue;
        }
        if !seen.insert(id) {
            rows.remove(id);
            continue;
        }
        let Ok(body) = serde_json::from_str::<String>(raw) else {
            continue;
        };
        if let Some(r) = reading(
            id,
            &body
                .split(if global { ',' } else { '~' })
                .collect::<Vec<_>>(),
            global,
        ) {
            rows.insert(id.to_owned(), r);
        }
    }
    if rows.is_empty() {
        return Err("来源未返回所选证券的有效行情。".into());
    }
    Ok(rows)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parsing_market_formats_ignores_unsolicited_symbols_and_duplicates() {
        let ids = GLOBAL
            .iter()
            .map(|(id, _)| id.to_string())
            .collect::<Vec<_>>();
        let text = r#"var hq_str_gb_ixic="NASDAQ,101,1,2026-11-26 11:00:00,1";
var hq_str_int_hangseng="HANG SENG,3010,10,0.33";
var hq_str_unknown="fake,50,0,0";"#;
        let rows = quotes(text, &ids, true).unwrap();
        assert_eq!(rows.len(), 2);
        assert_eq!(rows["gb_ixic"].stamp, "11-26 11:00");
        assert_eq!(rows["gb_ixic"].price, 101.);
        assert_eq!(rows["gb_ixic"].change, 1.);
        assert_eq!(rows["gb_ixic"].percent, 1.);
        assert!(quotes(&format!("{text}\n{text}"), &ids, true).is_err());
        let mut fields = vec![""; 38];
        for (i, v) in [
            (1, "测试"),
            (2, "000001"),
            (3, "10.5"),
            (30, "20261126150000"),
            (31, "0.1"),
            (32, "0.96"),
        ] {
            fields[i] = v;
        }
        let line = format!("v_sz000001=\"{}\";", fields.join("~"));
        assert_eq!(
            quotes(&line, &["sz000001".into()], false).unwrap()["sz000001"].stamp,
            "11-26 15:00"
        );
        assert!(quotes(&line, &[CHINA[1].0.into()], false).is_err());
    }
    #[test]
    fn stock_search_and_watchlist_round_trip() {
        let rows=search(r#"v_hint="sh~601318~\u4e2d\u56fd\u5e73\u5b89~zgpa~GP-A^hk~02318~other~other~GP^sh~601318~重复~zgpa~GP-A";"#).unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].name, "中国平安");
        assert_eq!(stock_id("000001").as_deref(), Some("sz000001"));
        assert!(stock_id("sh000001").is_none());
        let dir = tempfile::tempdir().unwrap();
        write_watch(dir.path(), &rows).unwrap();
        assert_eq!(read_watch(dir.path()).unwrap()[0].id, "sh601318");
    }
}

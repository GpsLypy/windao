use super::{City, Conditions, Daily, Report};
use chrono::{DateTime, NaiveDate};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Default, Serialize, Deserialize)]
#[serde(default)]
pub(super) struct Preferences {
    pub resident: bool,
    pub pause_refresh: bool,
    pub city: Option<City>,
    pub monthly_limit: u32,
    pub host: String,
    pub encrypted_key: Vec<u8>,
    pub enabled: bool,
    pub saved: Vec<City>,
}

impl Preferences {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("weather-settings.json");
        std::fs::File::open(path)
            .ok()
            .filter(|f| f.metadata().is_ok_and(|m| m.len() < 128 * 1024))
            .and_then(|f| serde_json::from_reader(f).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let mut file =
            tempfile::NamedTempFile::new_in(dir).map_err(|_| "无法创建天气设置文件。")?;
        serde_json::to_writer(&mut file, self).map_err(|_| "无法写入天气设置。")?;
        file.as_file()
            .sync_all()
            .and_then(|_| {
                file.persist(dir.join("weather-settings.json"))
                    .map(|_| ())
                    .map_err(|e| e.error)
            })
            .map_err(|_| "无法保存天气设置。".into())
    }
}

pub(super) fn valid_host(host: &str) -> bool {
    host.len() <= 253
        && host.ends_with(".qweatherapi.com")
        && host.split('.').all(|part| {
            !part.is_empty()
                && part.len() <= 63
                && !part.starts_with('-')
                && !part.ends_with('-')
                && part.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'-')
        })
}

pub(super) fn locate_with(
    mut get: impl FnMut(&str) -> Result<String, String>,
) -> Result<City, String> {
    for (url, backup) in [
        ("https://ipwho.is/?lang=zh-CN", false),
        ("https://free.freeipapi.com/api/v1/json", true),
    ] {
        if let Ok(body) = get(url)
            && let Ok(city) = location(&body, backup)
        {
            return Ok(city);
        }
    }
    Err("两个定位服务均未返回有效城市，请搜索城市。".into())
}

fn location(body: &str, backup: bool) -> Result<City, String> {
    #[derive(Deserialize)]
    struct Location {
        success: Option<bool>,
        #[serde(alias = "cityName")]
        city: String,
        #[serde(alias = "regionName")]
        region: Option<String>,
        #[serde(alias = "countryName")]
        country: Option<String>,
        latitude: f64,
        longitude: f64,
    }
    let v: Location = serde_json::from_str(body).map_err(|_| "无法读取网络位置。")?;
    let city = City {
        name: v.city,
        admin1: v.region,
        country: v.country,
        latitude: v.latitude,
        longitude: v.longitude,
    };
    if (!backup && v.success != Some(true)) || !city.valid() {
        return Err("未找到网络位置，请搜索城市。".into());
    }
    Ok(city)
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Now {
    obs_time: String,
    temp: String,
    feels_like: String,
    humidity: String,
    wind_speed: String,
    text: String,
}
#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct Day {
    fx_date: NaiveDate,
    temp_min: String,
    temp_max: String,
    text_day: String,
    sunrise: Option<String>,
    sunset: Option<String>,
}
#[derive(Deserialize)]
struct Current {
    code: String,
    now: Option<Now>,
}
#[derive(Deserialize)]
struct Forecast {
    code: String,
    daily: Option<Vec<Day>>,
}

pub(super) fn report(current: &str, forecast: &str) -> Result<Report, String> {
    let current: Current = serde_json::from_str(current).map_err(|_| "和风天气实况格式无效。")?;
    let future: Forecast = serde_json::from_str(forecast).map_err(|_| "和风天气预报格式无效。")?;
    if current.code != "200" || future.code != "200" {
        return Err("和风天气拒绝了请求，请检查凭据和配额。".into());
    }
    let now = current.now.ok_or("和风天气缺少实况。")?;
    let obs = DateTime::parse_from_rfc3339(&now.obs_time)
        .or_else(|_| DateTime::parse_from_str(&now.obs_time, "%Y-%m-%dT%H:%M%:z"))
        .map_err(|_| "实况时间无效。")?;
    let days = future
        .daily
        .filter(|d| d.len() == 7)
        .ok_or("和风天气七日预报不完整。")?;
    let daily = Daily {
        time: days.iter().map(|d| d.fx_date).collect(),
        weather_code: vec![None; 7],
        descriptions: days.iter().map(|d| d.text_day.clone()).collect(),
        temperature_2m_min: days.iter().map(|d| d.temp_min.parse().ok()).collect(),
        temperature_2m_max: days.iter().map(|d| d.temp_max.parse().ok()).collect(),
        precipitation_probability_max: vec![None; 7],
        sunrise: days
            .iter()
            .map(|d| d.sunrise.as_ref().map(|t| format!("{}T{t}", d.fx_date)))
            .collect(),
        sunset: days
            .iter()
            .map(|d| d.sunset.as_ref().map(|t| format!("{}T{t}", d.fx_date)))
            .collect(),
    };
    Ok(Report {
        qweather: true,
        timezone: obs.offset().to_string(),
        current: Conditions {
            time: obs.naive_local(),
            temperature_2m: now.temp.parse().ok(),
            apparent_temperature: now.feels_like.parse().ok(),
            relative_humidity_2m: now.humidity.parse().ok(),
            wind_speed_10m: now.wind_speed.parse().ok(),
            weather_code: None,
            description: Some(now.text),
        },
        daily,
    })
}

#[derive(Default, Serialize, Deserialize)]
pub(super) struct Usage {
    month: String,
    pub calls: u32,
}
impl Usage {
    fn load(path: &Path) -> Result<Self, String> {
        match std::fs::File::open(path) {
            Ok(file) => serde_json::from_reader(file).map_err(|_| "和风天气本机计数损坏。".into()),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err("无法读取和风天气本机计数。".into()),
        }
    }
    pub fn current(dir: &Path) -> Result<Self, String> {
        let month = chrono::Local::now().format("%Y-%m").to_string();
        let usage = Self::load(&dir.join("weather-call-count.json"))?;
        Ok(if usage.month == month {
            usage
        } else {
            Self { month, calls: 0 }
        })
    }
    fn charge(dir: &Path, limit: u32) -> Result<(), String> {
        static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());
        let _guard = LOCK.lock().map_err(|_| "无法登记本机请求。")?;
        let mut usage = Self::current(dir)?;
        if limit > 0 && usage.calls >= limit {
            return Err("已达到本机设置的和风天气月请求上限。".into());
        }
        usage.calls = usage.calls.saturating_add(1);
        let mut file = tempfile::NamedTempFile::new_in(dir).map_err(|_| "无法登记本机请求。")?;
        serde_json::to_writer(&mut file, &usage).map_err(|_| "无法登记本机请求。")?;
        file.persist(dir.join("weather-call-count.json"))
            .map_err(|_| "保存天气请求计数失败。")?;
        Ok(())
    }
}
#[derive(Serialize, Deserialize)]
struct DaysCache {
    host: String,
    position: String,
    fetched: i64,
    forecast: String,
}
impl DaysCache {
    fn read(path: &Path, host: &str, position: &str, now: i64) -> Option<String> {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > 256 * 1024 {
            return None;
        }
        let v: Self = serde_json::from_reader(file).ok()?;
        (v.host == host
            && v.position == position
            && (0..6 * 3600).contains(&(now.saturating_sub(v.fetched))))
        .then_some(v.forecast)
    }
}
#[cfg(windows)]
pub(super) fn fetch(
    host: &str,
    encrypted: &[u8],
    latitude: f64,
    longitude: f64,
    directory: &Path,
    limit: u32,
) -> Result<Report, String> {
    if !valid_host(host) {
        return Err("请填写和风天气控制台提供的专属 API Host。".into());
    }
    let raw = zeroize::Zeroizing::new(
        crate::credential_protection::dpapi(encrypted, false)
            .map_err(|_| "无法读取和风天气凭据。")?,
    );
    let key = std::str::from_utf8(&raw).map_err(|_| "和风天气凭据无效。")?;
    let request = |path: &str| {
        let mut url = url::Url::parse(&format!("https://{host}/v7/weather/{path}"))
            .map_err(|_| "和风天气地址无效。")?;
        url.query_pairs_mut()
            .append_pair("location", &format!("{longitude:.2},{latitude:.2}"))
            .append_pair("lang", "zh");
        Usage::charge(directory, limit)?;
        crate::calendar_sync::get_weather_json(&url, key)
    };
    let current = request("now")?;
    let now = chrono::Utc::now().timestamp();
    let position = format!("{longitude:.2},{latitude:.2}");
    let path = super::cache_path(directory, "daily", host, latitude, longitude);
    if let Some(cached) = DaysCache::read(&path, host, &position, now).or_else(|| {
        DaysCache::read(
            &directory.join("weather-qweather-7d.json"),
            host,
            &position,
            now,
        )
    }) {
        return report(&current, &cached);
    }
    let forecast = request("7d")?;
    let reading = report(&current, &forecast)?;
    let cached = DaysCache {
        host: host.to_owned(),
        position,
        fetched: now,
        forecast,
    };
    if let Ok(mut tmp) = tempfile::NamedTempFile::new_in(directory)
        && serde_json::to_writer(&mut tmp, &cached).is_ok()
        && tmp.persist(path).is_ok()
    {
        super::prune(directory, "daily");
    }
    Ok(reading)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn fallback_location_uses_the_second_service_after_an_invalid_primary() {
        let mut calls = Vec::new();
        let found = locate_with(|url| {
            calls.push(url.to_owned());
            Ok(if calls.len() == 1 { r#"{"success":false}"# } else { r#"{"cityName":"杭州","regionName":"浙江","countryName":"中国","latitude":30.3,"longitude":120.2}"# }.into())
        }).unwrap();
        assert_eq!(calls.len(), 2);
        assert_eq!(found.name, "杭州");
        assert!(
            locate_with(|_| Ok(r#"{"cityName":"杭州","latitude":91,"longitude":0}"#.into()))
                .is_err()
        );
    }

    #[test]
    fn weather_provider_and_location_are_validated() {
        assert!(valid_host("example.re.qweatherapi.com"));
        assert!(!valid_host("example.re.qweatherapi.com.evil.invalid"));
        assert!(!valid_host("https://example.qweatherapi.com"));
        let location = location(r#"{"success":true,"city":"杭州","region":"浙江","country":"中国","latitude":30.3,"longitude":120.2}"#, false).unwrap();
        assert_eq!(location.name, "杭州");
        let now = r#"{"code":"200","now":{"obsTime":"2026-11-26T17:00+08:00","temp":"19","feelsLike":"18","humidity":"58","windSpeed":"12","text":"多云"}}"#;
        let days = (1..=7).map(|day|serde_json::json!({"fxDate":format!("2026-12-{day:02}"),"tempMin":"2","tempMax":"11","textDay":"晴","sunrise":"06:40","sunset":"17:10"})).collect::<Vec<_>>();
        let json = serde_json::json!({"code":"200","daily":days}).to_string();
        let parsed = report(now, &json).unwrap();
        assert_eq!(parsed.current.temperature_2m, Some(19.0));
        assert_eq!(parsed.daily.sunrise[0].as_deref(), Some("2026-12-01T06:40"));
        assert!(parsed.daily.valid());
        assert!(report(now, r#"{"code":"403"}"#).is_err());
        let dir = tempfile::tempdir().unwrap();
        let prefs = Preferences {
            resident: true,
            saved: vec![location],
            ..Default::default()
        };
        Usage::charge(dir.path(), 1).unwrap();
        assert!(Usage::charge(dir.path(), 1).is_err());
        assert_eq!(Usage::current(dir.path()).unwrap().calls, 1);
        let path = dir.path().join("forecast.json");
        let snapshot = DaysCache {
            host: "example.re.qweatherapi.com".into(),
            position: "124,28".into(),
            fetched: 100,
            forecast: "fixture".into(),
        };
        std::fs::write(&path, serde_json::to_vec(&snapshot).unwrap()).unwrap();
        assert!(DaysCache::read(&path, &snapshot.host, &snapshot.position, 101).is_some());
        assert!(DaysCache::read(&path, &snapshot.host, &snapshot.position, 100 + 21600).is_none());
        assert!(DaysCache::read(&path, &snapshot.host, "other", 101).is_none());
        prefs.save(dir.path()).unwrap();
        assert_eq!(Preferences::load(dir.path()).saved[0].name, "杭州");
    }
}

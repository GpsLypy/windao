mod provider;
use chrono::{NaiveDate, NaiveDateTime};
use serde::{Deserialize, Serialize};
use std::path::Path;

#[cfg(windows)]
use super::{accent, card, field, primary, subhead, title};
#[cfg(windows)]
use eframe::egui::{Button, Key, RichText, Ui};
#[cfg(windows)]
use std::sync::mpsc::{self, Receiver, TryRecvError};
#[cfg(windows)]
use url::Url;

#[cfg(windows)]
const GEOCODING: &str = "https://geocoding-api.open-meteo.com/v1/search";
#[cfg(windows)]
const FORECAST: &str = "https://api.open-meteo.com/v1/forecast";

#[derive(Clone, Debug, Deserialize, Serialize)]
struct City {
    name: String,
    admin1: Option<String>,
    country: Option<String>,
    latitude: f64,
    longitude: f64,
}

impl City {
    fn label(&self) -> String {
        let mut parts = vec![self.name.as_str()];
        for part in [self.admin1.as_deref(), self.country.as_deref()]
            .into_iter()
            .flatten()
        {
            if !part.is_empty() && !parts.contains(&part) {
                parts.push(part);
            }
        }
        parts.join(" · ")
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct CityResults {
    #[serde(default)]
    results: Vec<City>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Report {
    #[serde(default)]
    qweather: bool,
    timezone: String,
    current: Conditions,
    daily: Daily,
}

#[derive(Debug, Deserialize, Serialize)]
struct Conditions {
    #[serde(default)]
    description: Option<String>,
    #[serde(deserialize_with = "local_time")]
    time: NaiveDateTime,
    temperature_2m: Option<f32>,
    apparent_temperature: Option<f32>,
    relative_humidity_2m: Option<u8>,
    wind_speed_10m: Option<f32>,
    weather_code: Option<u8>,
}

#[derive(Debug, Deserialize, Serialize)]
struct Daily {
    #[serde(default)]
    descriptions: Vec<String>,
    time: Vec<NaiveDate>,
    weather_code: Vec<Option<u8>>,
    temperature_2m_min: Vec<Option<f32>>,
    temperature_2m_max: Vec<Option<f32>>,
    precipitation_probability_max: Vec<Option<u8>>,
    #[serde(default)]
    sunrise: Vec<Option<String>>,
    #[serde(default)]
    sunset: Vec<Option<String>>,
}

impl Daily {
    fn valid(&self) -> bool {
        self.time.len() == 7
            && [
                self.weather_code.len(),
                self.temperature_2m_min.len(),
                self.temperature_2m_max.len(),
                self.precipitation_probability_max.len(),
            ]
            .iter()
            .all(|&size| size == self.time.len())
    }
}

#[derive(Serialize, Deserialize)]
struct CachedForecast {
    city: City,
    report: Report,
    fetched_at: i64,
}

impl CachedForecast {
    fn load(path: &Path) -> Option<Self> {
        let file = std::fs::File::open(path).ok()?;
        if file.metadata().ok()?.len() > 128 * 1024 {
            return None;
        }
        let cached: Self = serde_json::from_reader(file).ok()?;
        (cached.report.daily.valid()
            && cached.city.latitude.abs() <= 90.0
            && cached.city.longitude.abs() <= 180.0
            && !cached.city.name.is_empty())
        .then_some(cached)
    }
    fn save(path: &Path, city: &City, report: &Report, fetched_at: i64) -> std::io::Result<()> {
        #[derive(Serialize)]
        struct Snapshot<'a> {
            city: &'a City,
            report: &'a Report,
            fetched_at: i64,
        }
        let mut file = tempfile::NamedTempFile::new_in(path.parent().unwrap_or(Path::new(".")))?;
        serde_json::to_writer(
            &mut file,
            &Snapshot {
                city,
                report,
                fetched_at,
            },
        )?;
        file.as_file().sync_all()?;
        file.persist(path)?;
        Ok(())
    }
}

#[cfg(windows)]
#[derive(Default)]
pub(super) struct Weather {
    prefs: provider::Preferences,
    directory: std::path::PathBuf,
    usage: u32,
    host: String,
    key: String,
    query: String,
    loaded: bool,
    refresh_at: i64,
    fetched_at: Option<i64>,
    cities: Option<Vec<City>>,
    selected: Option<usize>,
    report: Option<Report>,
    error: Option<String>,
    request: Option<Receiver<Result<Reply, String>>>,
}

#[cfg(windows)]
enum Reply {
    Cities(Vec<City>),
    Located(City),
    Report(Box<Report>),
}

#[cfg(windows)]
impl Weather {
    pub(super) fn caption(&self) -> Option<String> {
        self.prefs
            .resident
            .then_some(self.report.as_ref()?.current.temperature_2m?)
            .map(|v| format!("{v:.0}°"))
    }
    pub(super) fn tick(&mut self, ctx: &eframe::egui::Context, directory: &Path, visible: bool) {
        let path = directory.join("weather-cache.json");
        let now = chrono::Utc::now().timestamp();
        if !self.loaded {
            self.loaded = true;
            self.directory = directory.to_owned();
            self.usage = provider::Usage::current(directory).map_or(0, |u| u.calls);
            self.prefs = provider::Preferences::load(directory);
            self.host = self.prefs.host.clone();
            if let Some(cache) = CachedForecast::load(&path) {
                self.query = cache.city.name.clone();
                self.cities = Some(vec![cache.city]);
                self.selected = Some(0);
                self.report = Some(cache.report);
                self.fetched_at = Some(cache.fetched_at);
                self.refresh_at = cache.fetched_at.min(now).saturating_add(900);
            }
        }
        self.poll(&path, now);
        if !visible && !self.prefs.resident {
            return;
        }
        if let Some(index) = self.selected
            && self.request.is_none()
        {
            if now >= self.refresh_at {
                self.forecast(ctx, index);
            } else {
                ctx.request_repaint_after(std::time::Duration::from_secs(
                    (self.refresh_at - now) as u64,
                ));
            }
        }
    }
    pub(super) fn show(&mut self, ui: &mut Ui, directory: &Path) {
        self.tick(ui.ctx(), directory, true);
        let now = chrono::Utc::now().timestamp();
        title(ui, "天气");
        if ui
            .checkbox(&mut self.prefs.resident, "在折叠胶囊显示温度")
            .changed()
        {
            self.error = self.prefs.save(directory).err();
        }
        ui.collapsing("天气来源与收藏城市", |ui| {
            ui.label(if self.prefs.enabled {
                "当前来源 · 和风天气"
            } else {
                "当前来源 · Open-Meteo"
            });
            ui.label(format!("本机本月和风天气请求 · {} 次", self.usage));
            ui.horizontal(|ui| {
                ui.label("本机月请求上限（0 为不限）");
                if ui
                    .add(
                        eframe::egui::DragValue::new(&mut self.prefs.monthly_limit)
                            .range(0..=10_000_000u32),
                    )
                    .changed()
                {
                    self.error = self.prefs.save(directory).err();
                }
            });
            field(
                ui,
                "和风天气 API Host",
                &mut self.host,
                "控制台中的专属域名",
                253,
            );
            ui.label("和风天气 API Key");
            ui.add(
                eframe::egui::TextEdit::singleline(&mut self.key)
                    .password(true)
                    .char_limit(256),
            );
            ui.horizontal_wrapped(|ui| {
                if ui.button("启用和风天气").clicked() {
                    let host = self.host.trim().to_ascii_lowercase();
                    if !provider::valid_host(&host)
                        || (self.key.is_empty()
                            && (host != self.prefs.host || self.prefs.encrypted_key.is_empty()))
                    {
                        self.error = Some("请输入专属 API Host 和对应的 API Key。".into());
                    } else {
                        let key = if self.key.is_empty() {
                            Ok(self.prefs.encrypted_key.clone())
                        } else {
                            crate::credential_protection::dpapi(self.key.as_bytes(), true)
                        };
                        match key {
                            Ok(key) => {
                                zeroize::Zeroize::zeroize(&mut self.key);
                                self.prefs.host = host;
                                self.prefs.encrypted_key = key;
                                self.prefs.enabled = true;
                                self.request = None;
                                self.refresh_at = 0;
                                self.error = self.prefs.save(directory).err();
                            }
                            Err(_) => self.error = Some("Windows 无法保存天气密钥。".into()),
                        }
                    }
                }
                if ui.button("使用 Open-Meteo").clicked() {
                    self.prefs.enabled = false;
                    self.request = None;
                    self.refresh_at = 0;
                    self.error = self.prefs.save(directory).err();
                }
            });
            if let Some(city) = self.cities.as_ref().and_then(|v| v.get(self.selected?))
                && self.prefs.saved.len() < 5
                && !self
                    .prefs
                    .saved
                    .iter()
                    .any(|c| c.latitude == city.latitude && c.longitude == city.longitude)
                && ui.button("收藏当前城市").clicked()
            {
                self.prefs.saved.push(city.clone());
                self.error = self.prefs.save(directory).err();
            }
            let mut pick = None;
            let mut remove = None;
            for (index, city) in self.prefs.saved.iter().enumerate() {
                ui.horizontal(|ui| {
                    if ui.button(city.label()).clicked() {
                        pick = Some(city.clone());
                    }
                    if ui.small_button("移除").clicked() {
                        remove = Some(index);
                    }
                });
            }
            if let Some(index) = remove {
                self.prefs.saved.remove(index);
                self.error = self.prefs.save(directory).err();
            }
            if let Some(city) = pick {
                self.cities = Some(vec![city]);
                self.selected = None;
                self.forecast(ui.ctx(), 0);
            }
        });
        card(ui, |ui| {
            let input = field(ui, "城市名称", &mut self.query, "例如 北京、Shanghai", 100);
            let pending = self.request.is_some();
            let has_query = !self.query.trim().is_empty();
            let enter = input.lost_focus() && ui.input(|i| i.key_pressed(Key::Enter));
            ui.horizontal_wrapped(|ui| {
                if ui
                    .add_enabled_ui(!pending && has_query, |ui| primary(ui, "搜索城市"))
                    .inner
                    .clicked()
                    || (enter && !pending && has_query)
                {
                    let mut url = Url::parse(GEOCODING).expect("fixed geocoding URL");
                    url.query_pairs_mut()
                        .append_pair("name", self.query.trim())
                        .append_pair("count", "8")
                        .append_pair("language", "zh")
                        .append_pair("format", "json");
                    self.start(ui.ctx(), move || {
                        parse_cities(&get(&url)?).map(Reply::Cities)
                    });
                }
                if ui
                    .add_enabled(!pending, Button::new("按网络位置定位"))
                    .on_hover_text("通过 ipwho.is 查询当前网络所在城市")
                    .clicked()
                {
                    self.start(ui.ctx(), || {
                        let url =
                            Url::parse("https://ipwho.is/?lang=zh-CN").expect("fixed location URL");
                        let (status, body) = crate::calendar_sync::get_json(&url)
                            .map_err(|_| "无法连接定位服务。")?;
                        if status != 200 {
                            return Err(format!("定位服务返回 HTTP {status}。"));
                        }
                        provider::location(&body).map(Reply::Located)
                    });
                }
                if ui
                    .add_enabled(
                        self.request.is_none() && self.selected.is_some(),
                        Button::new("刷新天气"),
                    )
                    .clicked()
                    && let Some(index) = self.selected
                {
                    self.forecast(ui.ctx(), index);
                }
                if self.request.is_some() {
                    ui.spinner();
                    ui.weak("正在获取数据");
                }
            });
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            let mut chosen = None;
            match &self.cities {
                None => {
                    ui.weak("搜索并选择城市后查看天气。");
                }
                Some(cities) if cities.is_empty() => {
                    ui.weak("没有找到城市，可以试试英文名称。");
                }
                Some(cities) => {
                    ui.horizontal_wrapped(|ui| {
                        for (index, city) in cities.iter().enumerate() {
                            if ui
                                .add_enabled(
                                    self.request.is_none(),
                                    Button::new(city.label())
                                        .selected(self.selected == Some(index)),
                                )
                                .clicked()
                            {
                                chosen = Some(index);
                            }
                        }
                    });
                }
            }
            if let Some(index) = chosen {
                self.forecast(ui.ctx(), index);
            }
        });
        if let Some(report) = &self.report {
            ui.add_space(12.0);
            card(ui, |ui| {
                let city = &self.cities.as_ref().expect("report city")
                    [self.selected.expect("selected city")];
                subhead(ui, &city.label());
                if let Some(at) = self.fetched_at
                    && (now.saturating_sub(at) >= 900 || self.error.is_some())
                {
                    ui.colored_label(
                        ui.visuals().warn_fg_color,
                        format!(
                            "上次获取 · {}",
                            super::display_time(at.saturating_mul(1000), "%m-%d %H:%M")
                        ),
                    );
                }
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(temperature(report.current.temperature_2m))
                            .size(34.0)
                            .strong()
                            .color(accent(ui.visuals().dark_mode)),
                    );
                    ui.label(
                        report
                            .current
                            .description
                            .as_deref()
                            .unwrap_or_else(|| condition(report.current.weather_code)),
                    );
                    ui.weak(format!(
                        "体感 {}",
                        temperature(report.current.apparent_temperature)
                    ));
                });
                ui.horizontal_wrapped(|ui| {
                    if let Some(value) = report.current.relative_humidity_2m {
                        ui.label(format!("湿度 {value}%"));
                    }
                    if let Some(value) = report.current.wind_speed_10m {
                        ui.label(format!("风速 {value:.1} km/h"));
                    }
                    if let Some(day) = report
                        .daily
                        .time
                        .iter()
                        .position(|day| *day == report.current.time.date())
                    {
                        for (label, times) in [
                            ("日出", &report.daily.sunrise),
                            ("日落", &report.daily.sunset),
                        ] {
                            if let Some(Some(raw)) = times.get(day)
                                && let Ok(value) =
                                    NaiveDateTime::parse_from_str(raw, "%Y-%m-%dT%H:%M")
                            {
                                ui.label(format!("{label} {}", value.format("%H:%M")));
                            }
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.weak(format!(
                        "实况 {} · {}",
                        report.current.time.format("%m-%d %H:%M"),
                        report.timezone
                    ));
                    if report.qweather {
                        ui.hyperlink_to("和风天气", "https://www.qweather.com/");
                    } else {
                        ui.hyperlink_to("Open-Meteo", "https://open-meteo.com/");
                    }
                });
                ui.add_space(12.0);
                subhead(ui, "未来 7 天");
                egui_table(ui, &report.daily);
            });
        }
    }

    fn forecast(&mut self, ctx: &eframe::egui::Context, index: usize) {
        let city = &self.cities.as_ref().expect("forecast city")[index];
        if self.selected != Some(index) {
            self.report = None;
        }
        self.selected = Some(index);
        self.refresh_at = chrono::Utc::now().timestamp() + 120;
        if self.prefs.enabled {
            let host = self.prefs.host.clone();
            let encrypted = self.prefs.encrypted_key.clone();
            let (lat, lon) = (city.latitude, city.longitude);
            let directory = self.directory.clone();
            let limit = self.prefs.monthly_limit;
            self.start(ctx, move || {
                provider::fetch(&host, &encrypted, lat, lon, &directory, limit)
                    .map(Box::new)
                    .map(Reply::Report)
            });
            return;
        }
        let mut url = Url::parse(FORECAST).expect("fixed forecast URL");
        url.query_pairs_mut()
            .append_pair("latitude", &city.latitude.to_string())
            .append_pair("longitude", &city.longitude.to_string())
            .append_pair("timezone", "auto")
            .append_pair("forecast_days", "7")
            .append_pair(
                "current",
                "temperature_2m,apparent_temperature,weather_code,relative_humidity_2m,wind_speed_10m",
            )
            .append_pair(
                "daily",
                "weather_code,temperature_2m_min,temperature_2m_max,precipitation_probability_max,sunrise,sunset",
            );
        self.start(ctx, move || {
            parse_report(&get(&url)?).map(Box::new).map(Reply::Report)
        });
    }

    fn start(
        &mut self,
        ctx: &eframe::egui::Context,
        run: impl FnOnce() -> Result<Reply, String> + Send + 'static,
    ) {
        let (send, receive) = mpsc::channel();
        let context = ctx.clone();
        let viewport = context.viewport_id();
        self.error = None;
        match std::thread::Builder::new()
            .name("weather".into())
            .spawn(move || {
                let _ = send.send(run());
                context.request_repaint_of(viewport);
            }) {
            Ok(_) => self.request = Some(receive),
            Err(_) => self.error = Some("无法启动天气查询，请重试。".into()),
        }
    }

    fn poll(&mut self, path: &Path, now: i64) {
        let Some(request) = &self.request else {
            return;
        };
        let result = match request.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("天气查询已中断，请重试。".into()),
        };
        self.request = None;
        self.usage = provider::Usage::current(&self.directory).map_or(self.usage, |u| u.calls);
        self.refresh_at = now + 120;
        match result {
            Ok(Reply::Located(city)) => {
                self.query = city.name.clone();
                self.cities = Some(vec![city]);
                self.report = None;
                self.selected = Some(0);
                self.refresh_at = 0;
            }
            Ok(Reply::Cities(cities)) => {
                self.cities = Some(cities);
                self.selected = None;
                self.report = None;
            }
            Ok(Reply::Report(report)) => {
                if let Some(city) = self
                    .cities
                    .as_ref()
                    .and_then(|cities| cities.get(self.selected?))
                    && CachedForecast::save(path, city, &report, now).is_err()
                {
                    self.error = Some("天气已更新，本机缓存写入失败。".into());
                }
                self.fetched_at = Some(now);
                self.refresh_at = now + 900;
                self.report = Some(*report);
            }
            Err(error) => {
                if error.contains("429") {
                    self.refresh_at = now + 900;
                } else if error.contains("月请求上限") {
                    self.refresh_at = now + 3600;
                }
                self.error = Some(error);
            }
        }
    }
}

#[cfg(windows)]
fn egui_table(ui: &mut Ui, daily: &Daily) {
    eframe::egui::Grid::new("forecast")
        .striped(true)
        .spacing([12.0, 5.0])
        .show(ui, |ui| {
            for heading in ["日期", "天气", "最低 / 最高", "降水概率"] {
                ui.weak(heading);
            }
            ui.end_row();
            for (index, day) in daily.time.iter().enumerate() {
                ui.label(day.format("%m-%d").to_string());
                ui.label(
                    daily
                        .descriptions
                        .get(index)
                        .map(String::as_str)
                        .unwrap_or_else(|| condition(daily.weather_code[index])),
                );
                ui.label(format!(
                    "{} / {}",
                    temperature(daily.temperature_2m_min[index]),
                    temperature(daily.temperature_2m_max[index])
                ));
                ui.label(
                    daily.precipitation_probability_max[index]
                        .map_or_else(|| "—".into(), |value| format!("{value}%")),
                );
                ui.end_row();
            }
        });
}

#[cfg(windows)]
fn temperature(value: Option<f32>) -> String {
    value.map_or_else(|| "—".into(), |value| format!("{value:.0} °C"))
}

#[cfg(windows)]
fn condition(code: Option<u8>) -> &'static str {
    match code {
        Some(0) => "晴",
        Some(1..=2) => "少云",
        Some(3) => "阴",
        Some(45 | 48) => "雾",
        Some(51 | 53 | 55 | 56 | 57) => "毛毛雨",
        Some(61 | 63 | 65 | 66 | 67) => "雨",
        Some(71 | 73 | 75 | 77 | 85 | 86) => "雪",
        Some(80..=82) => "阵雨",
        Some(95 | 96 | 99) => "雷暴",
        _ => "天气未知",
    }
}

#[cfg(windows)]
fn get(url: &Url) -> Result<String, String> {
    let (status, body) = crate::calendar_sync::get_json(url)
        .map_err(|_| "无法连接 Open-Meteo，请检查网络后重试。")?;
    if status != 200 {
        return Err(format!("Open-Meteo 暂时无法返回数据（HTTP {status}）。"));
    }
    Ok(body)
}

fn parse_cities(body: &str) -> Result<Vec<City>, String> {
    let response: CityResults =
        serde_json::from_str(body).map_err(|_| "城市搜索结果的格式无效。")?;
    Ok(response
        .results
        .into_iter()
        .filter(|city| {
            !city.name.is_empty()
                && (-90.0..=90.0).contains(&city.latitude)
                && (-180.0..=180.0).contains(&city.longitude)
        })
        .take(8)
        .collect())
}

fn parse_report(body: &str) -> Result<Report, String> {
    let report: Report = serde_json::from_str(body).map_err(|_| "天气数据的格式无效。")?;
    if !report.daily.valid() {
        return Err("Open-Meteo 返回的七日预报数据不完整。".into());
    }
    Ok(report)
}

fn local_time<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<NaiveDateTime, D::Error> {
    let time = String::deserialize(deserializer)?;
    NaiveDateTime::parse_from_str(&time, "%Y-%m-%dT%H:%M")
        .or_else(|_| time.parse())
        .map_err(serde::de::Error::custom)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_same_name_cities_and_empty_search() {
        let cities = parse_cities(r#"{"results":[{"name":"北京","admin1":"北京市","country":"中国","latitude":39.9,"longitude":116.4},{"name":"北京","admin1":"重庆市","country":"中国","latitude":30.7,"longitude":108.6}]}"#).unwrap();
        assert_eq!(cities[0].label(), "北京 · 北京市 · 中国");
        assert_eq!(cities[1].label(), "北京 · 重庆市 · 中国");
        assert!(parse_cities("{}").unwrap().is_empty());
    }

    #[test]
    fn validates_daily_arrays_and_preserves_missing_readings() {
        let days = [
            "2026-11-21",
            "2026-11-22",
            "2026-11-23",
            "2026-11-24",
            "2026-11-25",
            "2026-11-26",
            "2026-11-27",
        ];
        let mut response = serde_json::json!({
            "timezone": "Asia/Shanghai",
            "current": {"time": "2026-11-21T02:30", "temperature_2m": 8.0, "apparent_temperature": null, "weather_code": 0, "wind_speed_10m": 12.5, "relative_humidity_2m": 64},
            "daily": {"time": days, "sunrise": ["2026-11-21T06:27",null], "sunset": ["2026-11-21T17:16"], "weather_code": [0, 1, 2, null, 3, 45, 55], "temperature_2m_min": vec![3; 7], "temperature_2m_max": vec![10; 7], "precipitation_probability_max": vec![12; 7]}
        });
        let report = parse_report(&response.to_string()).unwrap();
        assert_eq!(report.timezone, "Asia/Shanghai");
        assert_eq!(report.current.time.to_string(), "2026-11-21 02:30:00");
        assert_eq!(report.current.temperature_2m, Some(8.0));
        assert_eq!(report.current.apparent_temperature, None);
        assert_eq!(report.current.weather_code, Some(0));
        assert_eq!(report.daily.weather_code[3], None);
        assert_eq!(report.current.relative_humidity_2m, Some(64));
        assert_eq!(report.current.wind_speed_10m, Some(12.5));
        assert_eq!(report.daily.sunrise[0].as_deref(), Some("2026-11-21T06:27"));
        assert_eq!(report.daily.sunset[0].as_deref(), Some("2026-11-21T17:16"));
        let root = tempfile::tempdir().unwrap();
        let path = root.path().join("weather-cache.json");
        let city =
            parse_cities(r#"{"results":[{"name":"上海","latitude":31.2,"longitude":121.5}]}"#)
                .unwrap()
                .remove(0);
        CachedForecast::save(&path, &city, &report, 123).unwrap();
        let cached = CachedForecast::load(&path).unwrap();
        assert_eq!(cached.fetched_at, 123);
        assert_eq!(cached.report.current.time, report.current.time);
        std::fs::write(&path, b"broken").unwrap();
        assert!(CachedForecast::load(&path).is_none());
        response["daily"]["weather_code"] = serde_json::json!([0]);
        assert!(parse_report(&response.to_string()).is_err());
    }
}

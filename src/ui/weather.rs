use chrono::{NaiveDate, NaiveDateTime};
use serde::Deserialize;

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

#[derive(Debug, Deserialize)]
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

#[derive(Debug, Deserialize)]
struct CityResults {
    #[serde(default)]
    results: Vec<City>,
}

#[derive(Debug, Deserialize)]
struct Report {
    timezone: String,
    current: Conditions,
    daily: Daily,
}

#[derive(Debug, Deserialize)]
struct Conditions {
    #[serde(deserialize_with = "local_time")]
    time: NaiveDateTime,
    temperature_2m: Option<f32>,
    apparent_temperature: Option<f32>,
    weather_code: Option<u8>,
}

#[derive(Debug, Deserialize)]
struct Daily {
    time: Vec<NaiveDate>,
    weather_code: Vec<Option<u8>>,
    temperature_2m_min: Vec<Option<f32>>,
    temperature_2m_max: Vec<Option<f32>>,
    precipitation_probability_max: Vec<Option<u8>>,
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

#[cfg(windows)]
#[derive(Default)]
pub(super) struct Weather {
    query: String,
    cities: Option<Vec<City>>,
    selected: Option<usize>,
    report: Option<Report>,
    error: Option<String>,
    request: Option<Receiver<Result<Reply, String>>>,
}

#[cfg(windows)]
enum Reply {
    Cities(Vec<City>),
    Report(Report),
}

#[cfg(windows)]
impl Weather {
    pub(super) fn show(&mut self, ui: &mut Ui) {
        self.poll();
        title(ui, "天气");
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
                    self.start(ui, move || parse_cities(&get(&url)?).map(Reply::Cities));
                }
                if ui
                    .add_enabled(
                        self.request.is_none() && self.selected.is_some(),
                        Button::new("刷新天气"),
                    )
                    .clicked()
                    && let Some(index) = self.selected
                {
                    self.forecast(ui, index);
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
                self.forecast(ui, index);
            }
        });
        if let Some(report) = &self.report {
            ui.add_space(12.0);
            card(ui, |ui| {
                let city = &self.cities.as_ref().expect("report city")
                    [self.selected.expect("selected city")];
                subhead(ui, &city.label());
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        RichText::new(temperature(report.current.temperature_2m))
                            .size(34.0)
                            .strong()
                            .color(accent(ui.visuals().dark_mode)),
                    );
                    ui.label(condition(report.current.weather_code));
                    ui.weak(format!(
                        "体感 {}",
                        temperature(report.current.apparent_temperature)
                    ));
                });
                ui.horizontal_wrapped(|ui| {
                    ui.weak(format!(
                        "实况 {} · {}",
                        report.current.time.format("%m-%d %H:%M"),
                        report.timezone
                    ));
                    ui.hyperlink_to("Open-Meteo", "https://open-meteo.com/");
                });
                ui.add_space(12.0);
                subhead(ui, "未来 7 天");
                egui_table(ui, &report.daily);
            });
        }
    }

    fn forecast(&mut self, ui: &Ui, index: usize) {
        let city = &self.cities.as_ref().expect("forecast city")[index];
        if self.selected != Some(index) {
            self.report = None;
        }
        self.selected = Some(index);
        let mut url = Url::parse(FORECAST).expect("fixed forecast URL");
        url.query_pairs_mut()
            .append_pair("latitude", &city.latitude.to_string())
            .append_pair("longitude", &city.longitude.to_string())
            .append_pair("timezone", "auto")
            .append_pair("forecast_days", "7")
            .append_pair(
                "current",
                "temperature_2m,apparent_temperature,weather_code",
            )
            .append_pair(
                "daily",
                "weather_code,temperature_2m_min,temperature_2m_max,precipitation_probability_max",
            );
        self.start(ui, move || parse_report(&get(&url)?).map(Reply::Report));
    }

    fn start(&mut self, ui: &Ui, run: impl FnOnce() -> Result<Reply, String> + Send + 'static) {
        let (send, receive) = mpsc::channel();
        let context = ui.ctx().clone();
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

    fn poll(&mut self) {
        let Some(request) = &self.request else {
            return;
        };
        let result = match request.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("天气查询已中断，请重试。".into()),
        };
        self.request = None;
        match result {
            Ok(Reply::Cities(cities)) => {
                self.cities = Some(cities);
                self.selected = None;
                self.report = None;
            }
            Ok(Reply::Report(report)) => self.report = Some(report),
            Err(error) => self.error = Some(error),
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
                ui.label(condition(daily.weather_code[index]));
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
            "current": {"time": "2026-11-21T02:30", "temperature_2m": 8.0, "apparent_temperature": null, "weather_code": 0},
            "daily": {"time": days, "weather_code": [0, 1, 2, null, 3, 45, 55], "temperature_2m_min": vec![3; 7], "temperature_2m_max": vec![10; 7], "precipitation_probability_max": vec![12; 7]}
        });
        let report = parse_report(&response.to_string()).unwrap();
        assert_eq!(report.timezone, "Asia/Shanghai");
        assert_eq!(report.current.time.to_string(), "2026-11-21 02:30:00");
        assert_eq!(report.current.temperature_2m, Some(8.0));
        assert_eq!(report.current.apparent_temperature, None);
        assert_eq!(report.current.weather_code, Some(0));
        assert_eq!(report.daily.weather_code[3], None);
        response["daily"]["weather_code"] = serde_json::json!([0]);
        assert!(parse_report(&response.to_string()).is_err());
    }
}

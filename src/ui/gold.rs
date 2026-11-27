#[cfg(windows)]
mod dashboard;
mod markets;
#[cfg(windows)]
use std::{
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};

use chrono::{DateTime, Utc};
#[cfg(windows)]
use eframe::egui::{Color32, RichText, Ui};
use serde::Deserialize;
#[cfg(windows)]
use url::Url;

#[cfg(windows)]
use super::{card, subhead};

#[cfg(windows)]
const ENDPOINT: &str = "https://api.gold-api.com/price/XAU";
#[cfg(windows)]
const REFRESH_INTERVAL: Duration = Duration::from_secs(30);

#[cfg(windows)]
#[derive(Default)]
pub(super) struct Gold {
    dashboard: dashboard::Dashboard,
    quote: Option<Quote>,
    pending: Option<Receiver<Result<Quote, String>>>,
    refresh_after: Option<Instant>,
    error: Option<String>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Quote {
    symbol: String,
    currency: Option<String>,
    price: f64,
    updated_at: DateTime<Utc>,
}

#[cfg(windows)]
impl Gold {
    pub(super) fn show(&mut self, ui: &mut Ui, directory: &std::path::Path) {
        if !self.dashboard.show(ui, directory) {
            return;
        }
        self.poll();
        card(ui, |ui| {
            let compact = super::in_capsule(ui);
            if !compact {
                subhead(ui, "现货金价");
            }
            if let Some(quote) = &self.quote {
                let color = if ui.visuals().dark_mode {
                    Color32::from_rgb(234, 195, 113)
                } else {
                    Color32::from_rgb(122, 81, 23)
                };
                ui.label(
                    RichText::new(format!("{:.2}", quote.price))
                        .color(color)
                        .size(28.0)
                        .strong(),
                );
                ui.label("XAU · 美元 / 金衡盎司").on_hover_text(format!(
                    "Gold API · {}",
                    quote.updated_at.format("%Y-%m-%d %H:%M:%S UTC")
                ));
            }
            if let Some(error) = &self.error {
                ui.colored_label(ui.visuals().error_fg_color, error);
            }
            ui.horizontal_wrapped(|ui| {
                let wait = self.refresh_after.map_or(Duration::ZERO, |deadline| {
                    deadline.saturating_duration_since(Instant::now())
                });
                if !wait.is_zero() {
                    ui.ctx().request_repaint_after(wait);
                }
                if ui
                    .add_enabled(
                        self.pending.is_none() && wait.is_zero(),
                        eframe::egui::Button::new("刷新金价"),
                    )
                    .on_disabled_hover_text("每 30 秒可刷新一次")
                    .clicked()
                {
                    self.refresh(ui);
                }
                if self.pending.is_some() {
                    ui.spinner();
                    ui.weak("正在获取报价");
                }
            });
        });
    }

    fn refresh(&mut self, ui: &Ui) {
        let (send, receive) = mpsc::channel();
        let context = ui.ctx().clone();
        let viewport = ui.ctx().viewport_id();
        self.error = None;
        match std::thread::Builder::new()
            .name("spot-gold".into())
            .spawn(move || {
                let _ = send.send(fetch_quote());
                context.request_repaint_of(viewport);
            }) {
            Ok(_) => {
                self.refresh_after = Some(Instant::now() + REFRESH_INTERVAL);
                self.pending = Some(receive);
            }
            Err(_) => self.error = Some("无法启动报价查询，请重试。".into()),
        }
    }

    fn poll(&mut self) {
        let Some(receive) = &self.pending else {
            return;
        };
        let result = match receive.try_recv() {
            Ok(result) => result,
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => Err("报价查询中断，请重试。".into()),
        };
        self.pending = None;
        match result {
            Ok(quote) => self.quote = Some(quote),
            Err(error) => self.error = Some(error),
        }
    }
}

#[cfg(windows)]
fn fetch_quote() -> Result<Quote, String> {
    let url = Url::parse(ENDPOINT).expect("fixed gold API URL");
    let (status, body) = crate::calendar_sync::get_json(&url)
        .map_err(|_| "无法连接 Gold API，请检查网络后重试。")?;
    if status != 200 {
        return Err(format!("Gold API 暂时无法提供报价（HTTP {status}）。"));
    }
    parse_quote(&body)
}

fn parse_quote(body: &str) -> Result<Quote, String> {
    let quote: Quote = serde_json::from_str(body).map_err(|_| "Gold API 的报价格式无效。")?;
    if quote.symbol != "XAU"
        || quote.currency.as_deref().is_some_and(|v| v != "USD")
        || !quote.price.is_finite()
        || quote.price <= 0.0
    {
        return Err("Gold API 未返回有效的美元现货金价。".into());
    }
    Ok(quote)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_usd_spot_gold_and_requires_source_time() {
        let body = r#"{"symbol":"XAU","currency":"USD","price":2000.25,"updatedAt":"2025-01-01T12:03:00Z"}"#;
        let quote = parse_quote(body).unwrap();
        assert_eq!(quote.price, 2000.25);
        assert!(parse_quote(&body.replace("\"currency\":\"USD\",", "")).is_ok());
        assert_eq!(quote.updated_at.to_rfc3339(), "2025-01-01T12:03:00+00:00");
        assert!(parse_quote(&body.replace("USD", "EUR")).is_err());
        assert!(parse_quote(&body.replace("XAU", "XAG")).is_err());
        assert!(parse_quote(&body.replace("2000.25", "-1.0")).is_err());
        assert!(parse_quote(&body.replace("updatedAt", "otherTime")).is_err());
    }
}

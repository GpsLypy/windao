use super::markets::{self, CHINA, GLOBAL, Reading, Stock};
use eframe::egui::{self, Button, Ui};
use std::{
    collections::HashMap,
    path::Path,
    sync::mpsc::{self, Receiver, TryRecvError},
    time::{Duration, Instant},
};
use url::Url;

#[derive(Clone, Copy, Default, PartialEq)]
enum Tab {
    #[default]
    Global,
    China,
    Gold,
}

enum Reply {
    Quotes(HashMap<String, Reading>),
    Stocks(Vec<Stock>),
}

#[derive(Default)]
pub(super) struct Dashboard {
    tab: Tab,
    loaded: bool,
    watch: Vec<Stock>,
    query: String,
    results: Vec<Stock>,
    quotes: HashMap<String, Reading>,
    pending: Option<Receiver<Result<Reply, String>>>,
    error: Option<String>,
    deadline: Option<Instant>,
}
impl Dashboard {
    pub fn show(&mut self, ui: &mut Ui, dir: &Path) -> bool {
        if !self.loaded {
            self.loaded = true;
            match markets::read_watch(dir) {
                Ok(rows) => self.watch = rows,
                Err(e) => self.error = Some(e),
            }
        }
        self.poll();
        let before = self.tab;
        ui.horizontal_wrapped(|ui| {
            for (tab, label) in [
                (Tab::Global, "全球市场"),
                (Tab::China, "A 股与自选"),
                (Tab::Gold, "现货金价"),
            ] {
                ui.selectable_value(&mut self.tab, tab, label);
            }
        });
        if before != self.tab {
            self.pending = None;
            self.error = None;
            self.deadline = None;
        }
        if self.tab == Tab::Gold {
            return true;
        }
        let global = self.tab == Tab::Global;
        let mut rows: Vec<Stock> = (if global { GLOBAL } else { CHINA })
            .iter()
            .map(|&(id, name)| Stock {
                id: id.into(),
                name: name.into(),
            })
            .collect();
        if !global {
            rows.extend(self.watch.iter().cloned());
        }
        let ids: Vec<_> = rows.iter().map(|r| r.id.clone()).collect();
        let due = self.deadline.is_none_or(|v| Instant::now() >= v);
        let manual = ui
            .horizontal(|ui| {
                let source = if global {
                    "新浪财经 · 全球市场"
                } else {
                    "腾讯财经 · 沪深 A 股"
                };
                let clicked = ui
                    .add_enabled(self.pending.is_none(), Button::new("刷新行情"))
                    .on_hover_text(source)
                    .clicked();
                if self.pending.is_some() {
                    ui.spinner();
                }
                clicked
            })
            .inner;
        if (manual || due) && self.pending.is_none() {
            self.deadline = Some(Instant::now() + Duration::from_secs(60));
            self.start(ui, move || {
                let base = if global {
                    "https://hq.sinajs.cn/list="
                } else {
                    "https://qt.gtimg.cn/q="
                };
                let url = Url::parse(&format!("{base}{}", ids.join(",")))
                    .map_err(|_| "行情地址无效。")?;
                markets::quotes(&crate::calendar_sync::market_feed(&url)?, &ids, global)
                    .map(Reply::Quotes)
            });
        }
        if let Some(deadline) = self.deadline {
            ui.ctx().request_repaint_after(
                deadline
                    .saturating_duration_since(Instant::now())
                    .max(Duration::from_secs(1)),
            );
        }
        let mut remove = None;
        let narrow = super::super::in_capsule(ui) && ui.available_width() < 440.0;
        egui::Grid::new("market-quotes")
            .striped(true)
            .spacing([12., 6.])
            .show(ui, |ui| {
                for label in ["证券", "最新", "涨跌", "涨跌幅", "报价时间（北京）"]
                {
                    if !narrow || !matches!(label, "涨跌" | "报价时间（北京）") {
                        ui.weak(label);
                    }
                }
                ui.end_row();
                for row in &rows {
                    ui.label(&row.name).on_hover_text(&row.id);
                    if let Some(q) = self.quotes.get(&row.id) {
                        let decimals = if matches!(row.id.as_str(), "fx_susdcny" | "fx_seurusd") {
                            4
                        } else {
                            2
                        };
                        ui.monospace(format!("{:.*}", decimals, q.price));
                        if !narrow {
                            ui.monospace(format!("{:+.*}", decimals, q.change));
                        }
                        ui.monospace(format!("{:+.2}%", q.percent))
                            .on_hover_text(format!(
                                "涨跌 {:+.*} · 北京时间 {}",
                                decimals, q.change, q.stamp
                            ));
                        if !narrow {
                            ui.weak(&q.stamp);
                        }
                    } else {
                        for _ in 0..if narrow { 2 } else { 4 } {
                            ui.weak("—");
                        }
                    }
                    if !global
                        && self.watch.iter().any(|v| v.id == row.id)
                        && ui.small_button("移除").clicked()
                    {
                        remove = Some(row.id.clone());
                    }
                    ui.end_row();
                }
            });
        if let Some(id) = remove {
            let rows = self.watch.iter().filter(|v| v.id != id).cloned().collect();
            self.save(dir, rows);
        }
        if !global {
            ui.separator();
            ui.label(format!("自选股（{}/8）", self.watch.len()));
            let mut find = false;
            ui.horizontal_wrapped(|ui| {
                let input = ui.add(
                    egui::TextEdit::singleline(&mut self.query)
                        .hint_text("名称、拼音或六位股票代码")
                        .char_limit(80),
                );
                if input.changed() {
                    self.results.clear();
                }
                find = ui
                    .add_enabled(
                        self.pending.is_none() && !self.query.trim().is_empty(),
                        Button::new("搜索"),
                    )
                    .clicked()
                    || (self.pending.is_none()
                        && input.lost_focus()
                        && ui.input(|i| i.key_pressed(egui::Key::Enter)));
            });
            if find && !self.query.trim().is_empty() {
                let query = self.query.trim().to_owned();
                self.start(ui, move || {
                    let mut url =
                        Url::parse("https://smartbox.gtimg.cn/s3/").expect("fixed market URL");
                    url.query_pairs_mut()
                        .append_pair("q", &query)
                        .append_pair("t", "all");
                    markets::search(&crate::calendar_sync::market_feed(&url)?).map(Reply::Stocks)
                });
            }
            let mut add = None;
            ui.horizontal_wrapped(|ui| {
                for stock in &self.results {
                    let available =
                        self.watch.len() < 8 && !self.watch.iter().any(|r| r.id == stock.id);
                    if ui
                        .add_enabled(
                            available,
                            Button::new(format!("添加 {} · {}", stock.name, stock.id)),
                        )
                        .clicked()
                    {
                        add = Some(stock.clone());
                    }
                }
            });
            if let Some(stock) = add {
                let mut rows = self.watch.clone();
                rows.push(stock);
                self.save(dir, rows);
            }
        }
        if let Some(error) = &self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        false
    }
    fn save(&mut self, dir: &Path, rows: Vec<Stock>) {
        match markets::write_watch(dir, &rows) {
            Ok(()) => {
                self.watch = rows;
                self.deadline = None;
            }
            Err(e) => self.error = Some(e),
        }
    }
    fn start(&mut self, ui: &Ui, run: impl FnOnce() -> Result<Reply, String> + Send + 'static) {
        let (tx, rx) = mpsc::channel();
        let ctx = ui.ctx().clone();
        let viewport = ctx.viewport_id();
        self.error = None;
        match std::thread::Builder::new()
            .name("market-snapshot".into())
            .spawn(move || {
                let _ = tx.send(run());
                ctx.request_repaint_of(viewport);
            }) {
            Ok(_) => self.pending = Some(rx),
            Err(_) => self.error = Some("无法启动行情查询。".into()),
        }
    }
    fn poll(&mut self) {
        let Some(rx) = &self.pending else {
            return;
        };
        match rx.try_recv() {
            Err(TryRecvError::Empty) => return,
            Err(TryRecvError::Disconnected) => self.error = Some("行情连接中断。".into()),
            Ok(Err(e)) => self.error = Some(e),
            Ok(Ok(Reply::Quotes(rows))) => {
                let global = self.tab == Tab::Global;
                self.quotes
                    .retain(|id, _| GLOBAL.iter().any(|r| r.0 == id) != global);
                self.quotes.extend(rows);
            }
            Ok(Ok(Reply::Stocks(rows))) => {
                if rows.is_empty() {
                    self.error = Some("没有找到匹配的沪深 A 股。".into());
                }
                self.results = rows;
            }
        }
        self.pending = None;
    }
}

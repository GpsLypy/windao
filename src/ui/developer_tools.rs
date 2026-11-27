use base64::{Engine, engine::general_purpose::STANDARD};
use chrono::{DateTime, Local, NaiveDateTime, TimeZone, Utc};
use eframe::egui::{self, Ui};
use qrcode::{Color, EcLevel, QrCode};
use std::{
    io::{Cursor, Read},
    path::Path,
};
type Result<T> = std::result::Result<T, &'static str>;
#[cfg(windows)]
mod images;
#[derive(Clone, Copy, PartialEq, Default)]
enum Tool {
    #[default]
    Json,
    Time,
    Uuid,
    Qr,
    Name,
    Image,
    Base64,
    Url,
}
const TOOLS: [(Tool, &str); 8] = [
    (Tool::Json, "JSON"),
    (Tool::Time, "时间戳"),
    (Tool::Uuid, "UUID"),
    (Tool::Qr, "二维码"),
    (Tool::Name, "命名转换"),
    (Tool::Image, "图片压缩"),
    (Tool::Base64, "Base64"),
    (Tool::Url, "URL 编解码"),
];
#[derive(Default)]
struct Draft {
    input: String,
    output: String,
    message: Option<&'static str>,
}
pub(crate) struct DeveloperTools {
    tool: Tool,
    drafts: [Draft; 8],
    json: u8,
    to_date: bool,
    millis: bool,
    utc: bool,
    count: u16,
    uppercase: bool,
    style: usize,
    b64_decode: bool,
    url_decode: bool,
    qr: Option<image::GrayImage>,
    texture: Option<egui::TextureHandle>,
    #[cfg(windows)]
    images: images::Images,
}
impl Default for DeveloperTools {
    fn default() -> Self {
        Self {
            tool: Tool::Json,
            drafts: Default::default(),
            json: 0,
            to_date: true,
            millis: false,
            utc: false,
            count: 1,
            uppercase: false,
            style: 0,
            b64_decode: false,
            url_decode: false,
            qr: None,
            texture: None,
            #[cfg(windows)]
            images: Default::default(),
        }
    }
}
fn name(text: &str, style: usize) -> String {
    text.lines()
        .map(|line| {
            let chars: Vec<char> = line.trim().chars().collect();
            let mut words = Vec::new();
            let mut word = String::new();
            for (i, &ch) in chars.iter().enumerate() {
                let prev = i.checked_sub(1).and_then(|n| chars.get(n));
                let next = chars.get(i + 1);
                let boundary = !ch.is_alphanumeric()
                    || ch.is_uppercase()
                        && (prev.is_some_and(|c| c.is_lowercase() || c.is_numeric())
                            || prev.is_some_and(|c| c.is_uppercase())
                                && next.is_some_and(|c| c.is_lowercase()));
                if boundary && !word.is_empty() {
                    words.push(std::mem::take(&mut word));
                }
                if ch.is_alphanumeric() {
                    word.extend(ch.to_lowercase());
                }
            }
            if !word.is_empty() {
                words.push(word);
            }
            let caps = |s: &String| {
                let mut chars = s.chars();
                let mut v = chars
                    .next()
                    .map(|c| c.to_uppercase().to_string())
                    .unwrap_or_default();
                v.push_str(chars.as_str());
                v
            };
            match style {
                0 => {
                    words.first().cloned().unwrap_or_default()
                        + &words.iter().skip(1).map(caps).collect::<String>()
                }
                1 => words.iter().map(caps).collect(),
                2 => words.join("_"),
                3 => words.join("-"),
                _ => words.join("_").to_uppercase(),
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}
fn time(text: &str, to_date: bool, millis: bool, utc: bool) -> Result<String> {
    let text = text.trim();
    if to_date {
        let value = text.parse::<f64>().map_err(|_| "请输入有效的时间戳。")?;
        let seconds = if millis { value / 1000.0 } else { value };
        if !(-62_135_596_800.0..=253_402_300_799.0).contains(&seconds) {
            return Err("时间戳超出范围。");
        }
        let dt = DateTime::from_timestamp_millis((seconds * 1000.).round() as i64)
            .ok_or("时间戳超出范围。")?;
        Ok(if utc {
            dt.to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        } else {
            dt.with_timezone(&Local)
                .to_rfc3339_opts(chrono::SecondsFormat::Millis, true)
        })
    } else {
        let dt = if let Ok(value) = DateTime::parse_from_rfc3339(text) {
            value.to_utc()
        } else {
            let naive = NaiveDateTime::parse_from_str(text, "%Y-%m-%d %H:%M:%S%.f")
                .or_else(|_| NaiveDateTime::parse_from_str(text, "%Y-%m-%dT%H:%M:%S%.f"))
                .or_else(|_| {
                    chrono::NaiveDate::parse_from_str(text, "%Y-%m-%d")
                        .map(|d| d.and_hms_opt(0, 0, 0).unwrap())
                })
                .map_err(|_| "请输入 ISO 8601 日期或 年-月-日 时:分:秒。")?;
            if utc {
                naive.and_utc()
            } else {
                Local
                    .from_local_datetime(&naive)
                    .single()
                    .ok_or("此本地时间无效或有歧义，请指定时区。")?
                    .to_utc()
            }
        };
        Ok(if millis {
            dt.timestamp_millis()
        } else {
            dt.timestamp()
        }
        .to_string())
    }
}
fn url_text(text: &str, decode: bool) -> Result<String> {
    if decode {
        let b = text.as_bytes();
        if (0..b.len()).any(|i| {
            b[i] == b'%'
                && (b.get(i + 1).is_none_or(|b| !b.is_ascii_hexdigit())
                    || b.get(i + 2).is_none_or(|b| !b.is_ascii_hexdigit()))
        }) {
            return Err("URL 百分号编码无效。");
        }
        percent_encoding::percent_decode_str(text)
            .decode_utf8()
            .map(|v| v.into_owned())
            .map_err(|_| "URL 编码不是 UTF-8。")
    } else {
        const SET: &percent_encoding::AsciiSet = &percent_encoding::NON_ALPHANUMERIC
            .remove(b'-')
            .remove(b'.')
            .remove(b'_')
            .remove(b'~');
        Ok(percent_encoding::utf8_percent_encode(text, SET).to_string())
    }
}
pub(crate) fn qr_image(text: &str) -> Result<image::GrayImage> {
    if text.is_empty() || text.len() > 2000 {
        return Err("二维码内容应为 1 至 2000 个 UTF-8 字节。");
    }
    let qr = QrCode::with_error_correction_level(text.as_bytes(), EcLevel::M)
        .map_err(|_| "二维码内容过长。")?;
    let side = qr.width();
    let size = ((side + 8) * 8) as u32;
    Ok(image::GrayImage::from_fn(size, size, |x, y| {
        let (x, y) = ((x / 8) as usize, (y / 8) as usize);
        let black = (4..side + 4).contains(&x)
            && (4..side + 4).contains(&y)
            && qr[(x - 4, y - 4)] == Color::Dark;
        image::Luma([if black { 0 } else { 255 }])
    }))
}
pub(crate) fn read_image(path: &Path) -> Result<image::DynamicImage> {
    let mut data = Vec::new();
    std::fs::File::open(path)
        .map_err(|_| "无法读取图片。")?
        .take(20_000_001)
        .read_to_end(&mut data)
        .map_err(|_| "无法读取图片。")?;
    if data.len() > 20_000_000 {
        return Err("图片应小于 20 MB。");
    }
    let format = image::guess_format(&data).map_err(|_| "请选择 PNG、JPEG 或 WebP 图片。")?;
    if !matches!(
        format,
        image::ImageFormat::Jpeg | image::ImageFormat::Png | image::ImageFormat::WebP
    ) {
        return Err("请选择 PNG、JPEG 或 WebP 图片。");
    }
    let reader = || image::ImageReader::with_format(Cursor::new(&data), format);
    let (w, h) = reader()
        .into_dimensions()
        .map_err(|_| "无法读取图片尺寸。")?;
    if w == 0 || h == 0 || u64::from(w) * u64::from(h) > 36_000_000 || w > 12000 || h > 12000 {
        return Err("图片应小于 3600 万像素，长边不超过 12000 像素。");
    }
    match format {
        image::ImageFormat::Png
            if image::codecs::png::PngDecoder::new(Cursor::new(&data))
                .map_err(|_| "PNG 无效。")?
                .is_apng()
                .map_err(|_| "PNG 无效。")? =>
        {
            return Err("请选择静态图片。");
        }
        image::ImageFormat::WebP
            if image::codecs::webp::WebPDecoder::new(Cursor::new(&data))
                .map_err(|_| "WebP 无效。")?
                .has_animation() =>
        {
            return Err("请选择静态图片。");
        }
        _ => {}
    }
    reader().decode().map_err(|_| "无法解码图片。")
}
impl DeveloperTools {
    pub fn show(&mut self, ui: &mut Ui) {
        let compact = super::in_capsule(ui);
        let tools = |ui: &mut Ui, selected: &mut Tool| {
            for (tool, label) in TOOLS {
                ui.selectable_value(selected, tool, label);
            }
        };
        if compact && ui.available_width() < 480.0 {
            let current = TOOLS
                .iter()
                .find(|(tool, _)| *tool == self.tool)
                .map_or("工具", |(_, label)| *label);
            egui::ComboBox::from_id_salt("developer-tool")
                .selected_text(current)
                .show_ui(ui, |ui| tools(ui, &mut self.tool));
        } else {
            ui.horizontal_wrapped(|ui| tools(ui, &mut self.tool));
        }
        if self.tool == Tool::Image {
            #[cfg(windows)]
            self.images.show(ui);
            return;
        }
        let i = self.tool as usize;
        ui.horizontal_wrapped(|ui| match self.tool {
            Tool::Json => {
                for (j, label) in ["格式化", "压缩", "校验"].into_iter().enumerate() {
                    ui.selectable_value(&mut self.json, j as u8, label);
                }
            }
            Tool::Time => {
                ui.selectable_value(&mut self.to_date, true, "时间戳 → 日期");
                ui.selectable_value(&mut self.to_date, false, "日期 → 时间戳");
                ui.checkbox(&mut self.millis, "毫秒");
                ui.checkbox(&mut self.utc, "UTC");
                if ui.button("现在").clicked() {
                    let now = Utc::now();
                    self.drafts[i].input = if self.to_date {
                        if self.millis {
                            now.timestamp_millis()
                        } else {
                            now.timestamp()
                        }
                        .to_string()
                    } else if self.utc {
                        now.format("%Y-%m-%d %H:%M:%S").to_string()
                    } else {
                        now.with_timezone(&Local)
                            .format("%Y-%m-%d %H:%M:%S")
                            .to_string()
                    };
                }
            }
            Tool::Uuid => {
                ui.label("数量");
                ui.add(egui::DragValue::new(&mut self.count).range(1..=1000));
                ui.checkbox(&mut self.uppercase, "大写");
            }
            Tool::Name => {
                for (j, s) in [
                    "camelCase",
                    "PascalCase",
                    "snake_case",
                    "kebab-case",
                    "CONSTANT_CASE",
                ]
                .into_iter()
                .enumerate()
                {
                    ui.selectable_value(&mut self.style, j, s);
                }
            }
            Tool::Base64 | Tool::Url => {
                let v = if self.tool == Tool::Base64 {
                    &mut self.b64_decode
                } else {
                    &mut self.url_decode
                };
                ui.selectable_value(v, false, "编码");
                ui.selectable_value(v, true, "解码");
            }
            _ => {}
        });
        let d = &mut self.drafts[i];
        if self.tool != Tool::Uuid
            && ui
                .add(
                    egui::TextEdit::multiline(&mut d.input)
                        .desired_rows(if compact { 2 } else { 3 })
                        .desired_width(f32::INFINITY)
                        .hint_text("输入或粘贴内容")
                        .char_limit(1_048_576),
                )
                .changed()
        {
            d.message = None;
            d.output.clear();
            if self.tool == Tool::Qr {
                self.qr = None;
                self.texture = None;
            }
        }
        ui.horizontal_wrapped(|ui| {
            if ui
                .button(if matches!(self.tool, Tool::Qr | Tool::Uuid) {
                    "生成"
                } else {
                    "执行"
                })
                .clicked()
            {
                d.message = None;
                let text = d.input.as_str();
                let result = if text.len() > 1_048_576 {
                    Err("输入内容超过 1 MiB。")
                } else {
                    match self.tool {
                        Tool::Json => serde_json::from_str::<serde_json::Value>(text)
                            .and_then(|v| {
                                if self.json == 1 {
                                    serde_json::to_string(&v)
                                } else {
                                    serde_json::to_string_pretty(&v)
                                }
                            })
                            .inspect(|_| {
                                if self.json == 2 {
                                    d.message = Some("JSON 格式有效。");
                                }
                            })
                            .map_err(|_| "JSON 格式无效，请检查引号、逗号和括号。"),
                        Tool::Uuid => Ok((0..self.count)
                            .map(|_| {
                                let id = uuid::Uuid::new_v4().to_string();
                                if self.uppercase {
                                    id.to_uppercase()
                                } else {
                                    id
                                }
                            })
                            .collect::<Vec<_>>()
                            .join("\n")),
                        Tool::Time => time(text, self.to_date, self.millis, self.utc),
                        Tool::Name if text.trim().is_empty() => Err("请输入变量名称。"),
                        Tool::Name => Ok(name(text, self.style)),
                        Tool::Url => url_text(text, self.url_decode),
                        Tool::Base64 if self.b64_decode => STANDARD
                            .decode(text.split_whitespace().collect::<String>())
                            .ok()
                            .and_then(|b| String::from_utf8(b).ok())
                            .ok_or("请输入有效的 Base64 UTF-8 文本。"),
                        Tool::Base64 => Ok(STANDARD.encode(text)),
                        Tool::Qr => qr_image(text).map(|img| {
                            self.texture = None;
                            self.qr = Some(img);
                            String::new()
                        }),
                        Tool::Image => unreachable!(),
                    }
                };
                match result {
                    Ok(output) => d.output = output,
                    Err(e) => {
                        d.message = Some(e);
                        d.output.clear();
                        self.texture = None;
                        self.qr = None;
                    }
                }
            }
            if ui.button("清空").clicked() {
                *d = Draft::default();
                self.qr = None;
                self.texture = None;
            }
            if !d.output.is_empty() && ui.button("复制结果").clicked() {
                ui.ctx().copy_text(d.output.clone());
            }
        });
        if let Some(msg) = d.message {
            ui.label(msg);
        }
        if !d.output.is_empty() {
            ui.add(
                egui::TextEdit::multiline(&mut d.output)
                    .desired_width(f32::INFINITY)
                    .desired_rows(if compact { 2 } else { 4 })
                    .code_editor(),
            );
        }
        if self.tool == Tool::Qr
            && let Some(img) = &self.qr
        {
            let texture = self.texture.get_or_insert_with(|| {
                ui.ctx().load_texture(
                    "developer-qr",
                    egui::ColorImage::from_gray(
                        [img.width() as usize, img.height() as usize],
                        img.as_raw(),
                    ),
                    egui::TextureOptions::NEAREST,
                )
            });
            let size = ui.available_width().min(192.);
            ui.image((texture.id(), egui::vec2(size, size)));
            #[cfg(windows)]
            images::qr_actions(ui, img, &mut self.drafts[i].message);
        }
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn conversion_fixtures() {
        assert_eq!(
            name("HTTPServerURL\nhello world", 2),
            "http_server_url\nhello_world"
        );
        assert_eq!(url_text("a+b ~", false).unwrap(), "a%2Bb%20~");
        assert!(url_text("%GG", true).is_err());
        assert_eq!(
            time("1970-01-01 00:00:01", false, true, true).unwrap(),
            "1000"
        );
    }
}

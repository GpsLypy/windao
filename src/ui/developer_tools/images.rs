use super::{Result, read_image};
use crate::windows::{FileKind, open_file, save_file};
use eframe::egui::Ui;
use image::{DynamicImage, ImageFormat, codecs::jpeg::JpegEncoder};
use std::{
    io::{Cursor, Write},
    path::{Path, PathBuf},
    sync::mpsc,
};
struct Output {
    bytes: Vec<u8>,
    source_bytes: u64,
    ext: &'static str,
    dimensions: (u32, u32),
}
pub(super) struct Images {
    path: Option<PathBuf>,
    quality: u8,
    max: u32,
    png: bool,
    pending: Option<mpsc::Receiver<Result<Output>>>,
    output: Option<Output>,
    error: Option<&'static str>,
}
impl Default for Images {
    fn default() -> Self {
        Self {
            path: None,
            quality: 82,
            max: 0,
            png: false,
            pending: None,
            output: None,
            error: None,
        }
    }
}
fn compress(source: &Path, png: bool, quality: u8, max: u32) -> Result<Output> {
    let source_bytes = std::fs::metadata(source)
        .map_err(|_| "无法读取原图。")?
        .len();
    let mut image = read_image(source)?;
    if max > 0 && (image.width() > max || image.height() > max) {
        image = image.thumbnail(max, max);
    }
    let dimensions = (image.width(), image.height());
    let mut bytes = Vec::new();
    if png {
        image
            .write_to(&mut Cursor::new(&mut bytes), ImageFormat::Png)
            .map_err(|_| "PNG 编码失败。")?;
    } else {
        let mut rgb = image.to_rgba8();
        for pixel in rgb.pixels_mut() {
            let a = u32::from(pixel[3]);
            for c in &mut pixel.0[..3] {
                *c = ((u32::from(*c) * a + 255 * (255 - a) + 127) / 255) as u8;
            }
        }
        JpegEncoder::new_with_quality(&mut bytes, quality)
            .encode_image(&DynamicImage::ImageRgba8(rgb).into_rgb8())
            .map_err(|_| "JPEG 编码失败。")?;
    }
    Ok(Output {
        bytes,
        source_bytes,
        ext: if png { "png" } else { "jpg" },
        dimensions,
    })
}
fn save_new(bytes: &[u8], suggested: &str) -> Result<bool> {
    let Some(path) = save_file(FileKind::AllFiles, suggested).map_err(|_| "无法打开保存窗口。")?
    else {
        return Ok(false);
    };
    let mut file = tempfile::NamedTempFile::new_in(path.parent().ok_or("请选择文件夹。")?)
        .map_err(|_| "无法创建输出文件。")?;
    file.write_all(bytes)
        .and_then(|()| file.as_file().sync_all())
        .map_err(|_| "写入输出文件失败。")?;
    file.persist_noclobber(path)
        .map_err(|_| "无法保存，请选择一个新的文件名。")?;
    Ok(true)
}
impl Images {
    pub fn show(&mut self, ui: &mut Ui) {
        if let Some(rx) = &self.pending {
            match rx.try_recv() {
                Ok(res) => {
                    self.pending = None;
                    match res {
                        Ok(out) => self.output = Some(out),
                        Err(e) => self.error = Some(e),
                    }
                }
                Err(mpsc::TryRecvError::Empty) => {
                    ui.spinner();
                    ui.label("正在压缩图片…");
                    ui.ctx()
                        .request_repaint_after(std::time::Duration::from_millis(120));
                    return;
                }
                Err(_) => {
                    self.pending = None;
                    self.error = Some("压缩任务中断。");
                }
            }
        }
        if ui.button("选择 PNG、JPEG 或 WebP 图片").clicked() {
            match open_file(FileKind::AllFiles) {
                Ok(Some(p)) => {
                    self.path = Some(p);
                    self.output = None;
                    self.error = None;
                }
                Ok(None) => {}
                Err(_) => self.error = Some("无法打开文件窗口。"),
            }
        }
        if let Some(path) = &self.path {
            ui.label(path.file_name().unwrap_or_default().to_string_lossy());
        }
        let settings = (self.png, self.quality, self.max);
        ui.horizontal_wrapped(|ui| {
            ui.selectable_value(&mut self.png, false, "JPEG");
            ui.selectable_value(&mut self.png, true, "PNG");
            if !self.png {
                ui.add(eframe::egui::Slider::new(&mut self.quality, 1..=100).text("质量"));
            }
            ui.add(
                eframe::egui::DragValue::new(&mut self.max)
                    .range(0..=12000)
                    .prefix("最长边 ")
                    .suffix(" px"),
            )
            .on_hover_text("0 保留原图尺寸");
        });
        if settings != (self.png, self.quality, self.max) {
            self.output = None;
        }
        if let Some(path) = self.path.clone()
            && ui.button("本地压缩").clicked()
        {
            let (png, quality, max) = (self.png, self.quality, self.max);
            let (tx, rx) = mpsc::channel();
            self.pending = Some(rx);
            self.output = None;
            self.error = None;
            let ctx = ui.ctx().clone();
            std::thread::spawn(move || {
                let _ = tx.send(compress(&path, png, quality, max));
                ctx.request_repaint();
            });
        }
        if let Some(out) = &self.output {
            ui.label(format!(
                "{} × {} · {:.1} KB → {:.1} KB",
                out.dimensions.0,
                out.dimensions.1,
                out.source_bytes as f64 / 1024.,
                out.bytes.len() as f64 / 1024.
            ));
            if ui.button("另存图片").clicked() {
                self.error = match save_new(&out.bytes, &format!("compressed.{}", out.ext)) {
                    Ok(true) => Some("图片已保存。"),
                    Ok(false) => None,
                    Err(e) => Some(e),
                };
            }
        }
        if let Some(msg) = self.error {
            ui.label(msg);
        }
    }
}
pub(super) fn qr_actions(ui: &mut Ui, img: &image::GrayImage, message: &mut Option<&'static str>) {
    ui.horizontal_wrapped(|ui| {
        if ui.button("复制二维码").clicked() {
            let rgba = DynamicImage::ImageLuma8(img.clone()).into_rgba8();
            *message = Some(
                if arboard::Clipboard::new()
                    .and_then(|mut c| {
                        c.set_image(arboard::ImageData {
                            width: img.width() as usize,
                            height: img.height() as usize,
                            bytes: rgba.as_raw().as_slice().into(),
                        })
                    })
                    .is_ok()
                {
                    "二维码已复制。"
                } else {
                    "无法复制二维码。"
                },
            );
        }
        if ui.button("保存 PNG").clicked() {
            let mut out = Cursor::new(Vec::new());
            *message = match img.write_to(&mut out, ImageFormat::Png) {
                Ok(()) => match save_new(out.get_ref(), "qr-code.png") {
                    Ok(true) => Some("二维码已保存。"),
                    Ok(false) => None,
                    Err(e) => Some(e),
                },
                Err(_) => Some("PNG 编码失败。"),
            };
        }
    });
}

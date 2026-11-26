use std::sync::mpsc::{Receiver, TryRecvError, channel};

use eframe::egui::{self, Ui};

use crate::workspace::Store;

mod chat;
mod translation;

#[derive(Default)]
pub(super) struct WorkbenchTools {
    chat: chat::Chat,
    translation: translation::Translation,
}

impl WorkbenchTools {
    pub(super) fn show_ai(&mut self, ui: &mut Ui, store: &mut Store) {
        self.chat.show(ui, store);
    }

    pub(super) fn show_translation(&mut self, ui: &mut Ui) {
        self.translation.show(ui);
    }
}

type Result<T> = std::result::Result<T, String>;

struct Job<T>(Receiver<Result<T>>);

impl<T: Send + 'static> Job<T> {
    fn start(ctx: &egui::Context, work: impl FnOnce() -> Result<T> + Send + 'static) -> Self {
        let ctx = ctx.clone();
        let (send, receive) = channel();
        std::thread::spawn(move || {
            let _ = send.send(work());
            ctx.request_repaint();
        });
        Self(receive)
    }

    fn poll(&self) -> Option<Result<T>> {
        match self.0.try_recv() {
            Ok(result) => Some(result),
            Err(TryRecvError::Empty) => None,
            Err(TryRecvError::Disconnected) => Some(Err("后台任务中断，请重试。".into())),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn both_pages_render_offscreen_without_starting_a_request() {
        let ctx = egui::Context::default();
        let mut tools = WorkbenchTools::default();
        let directory = tempfile::tempdir().unwrap();
        let mut store = Store::open(directory.path().to_owned()).unwrap();
        let ai = ctx.run_ui(Default::default(), |ui| tools.show_ai(ui, &mut store));
        let translation = ctx.run_ui(Default::default(), |ui| tools.show_translation(ui));
        for (output, labels) in [
            (ai, ["AI 工作台", "智能助理", "发送"]),
            (translation, ["本机翻译", "互换", "翻译"]),
        ] {
            let text: Vec<_> = output
                .shapes
                .iter()
                .filter_map(|shape| match &shape.shape {
                    egui::Shape::Text(text) => Some(text.galley.job.text.as_str()),
                    _ => None,
                })
                .collect();
            for label in labels {
                assert!(text.contains(&label), "missing {label}: {text:?}");
            }
        }
    }
}

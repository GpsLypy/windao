use eframe::egui::{self, ViewportBuilder, ViewportCommand, ViewportId, WindowLevel};

use super::Desktop;
use crate::{
    ui::{
        Page,
        desktop_todo::{self, BubbleAction, DesktopTodo},
    },
    windows,
    workspace::DesktopTodoBounds,
};

#[derive(Default)]
pub(super) struct TodoWindow {
    content: DesktopTodo,
    focus: bool,
    initial_size: Option<[f32; 2]>,
    last_bounds: Option<DesktopTodoBounds>,
    bubble_initialized: bool,
    bubble_position: Option<[i32; 2]>,
}

impl Desktop {
    pub(super) fn open_desktop_todo(&mut self, ctx: &egui::Context) {
        let settings = self.store.state().settings.desktop_todo;
        if (!settings.open || !settings.bubble_visible)
            && let Err(error) = self.store.update(|state| {
                state.settings.desktop_todo.open = true;
                state.settings.desktop_todo.bubble_visible = true;
                Ok(())
            })
        {
            windows::show_error(&error);
            return;
        }
        self.desktop_todo.focus = true;
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn save_todo_window(&mut self, close: bool) -> Result<(), String> {
        let previous = self.store.state().settings.desktop_todo;
        let mut next = previous;
        next.bounds = self.desktop_todo.last_bounds.or(previous.bounds);
        next.bubble_position = self
            .desktop_todo
            .bubble_position
            .or(previous.bubble_position);
        if close {
            next.open = false;
        }
        if next == previous {
            return Ok(());
        }
        self.store.update(|state| {
            state.settings.desktop_todo = next;
            Ok(())
        })
    }

    fn draw_todo_bubble(&mut self, ctx: &egui::Context) {
        let settings = self.store.state().settings.desktop_todo;
        if !settings.bubble_visible {
            self.desktop_todo.bubble_initialized = false;
            return;
        }

        const DIAMETER: f32 = 56.0;
        let placement = (!self.desktop_todo.bubble_initialized)
            .then(|| {
                self.shell.as_ref()?.desktop_todo_bubble_position(
                    self.desktop_todo
                        .bubble_position
                        .or(settings.bubble_position),
                    DIAMETER,
                )
            })
            .flatten();
        self.desktop_todo.bubble_initialized = true;
        let today = chrono::Local::now().date_naive();
        let pending = self
            .store
            .state()
            .tasks
            .iter()
            .filter(|task| task.is_actionable_on(today))
            .count();
        let appearance = self.store.state().settings.appearance;
        let (close, action) = ctx.show_viewport_immediate(
            ViewportId::from_hash_of("windao-desktop-todo-bubble"),
            ViewportBuilder::default()
                .with_title("桌面待办悬浮球")
                .with_icon(self.icon.clone())
                .with_inner_size([DIAMETER, DIAMETER])
                .with_decorations(false)
                .with_transparent(true)
                .with_taskbar(false)
                .with_resizable(false)
                .with_active(false)
                .with_window_level(WindowLevel::AlwaysOnTop),
            |ui, _| {
                let ctx = ui.ctx();
                let scale = ctx.pixels_per_point();
                if let Some([x, y]) = placement {
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(
                        x as f32 / scale,
                        y as f32 / scale,
                    )));
                } else if let Some(position) =
                    ctx.input(|input| input.viewport().outer_rect.map(|rect| rect.min))
                    && position.is_finite()
                {
                    self.desktop_todo.bubble_position = Some([
                        (position.x * scale).round() as i32,
                        (position.y * scale).round() as i32,
                    ]);
                }
                if ctx.input(|input| input.viewport().close_requested()) {
                    return (true, None);
                }
                let action = desktop_todo::bubble(
                    ui,
                    pending,
                    settings.open,
                    desktop_todo::todo_theme(ui, appearance),
                );
                if action == Some(BubbleAction::Drag) {
                    ctx.send_viewport_cmd(ViewportCommand::StartDrag);
                }
                (false, action)
            },
        );
        let result = if close {
            self.store.update(|state| {
                state.settings.desktop_todo.bubble_visible = false;
                Ok(())
            })
        } else if action == Some(BubbleAction::Toggle) && settings.open {
            self.save_todo_window(true)
        } else if action == Some(BubbleAction::Toggle) {
            self.open_desktop_todo(ctx);
            Ok(())
        } else {
            return;
        };
        if let Err(error) = result {
            windows::show_error(&error);
        }
        ctx.request_repaint_of(ViewportId::ROOT);
    }

    pub(super) fn draw_desktop_todo(&mut self, ctx: &egui::Context) {
        self.draw_todo_bubble(ctx);
        let settings = self.store.state().settings.desktop_todo;
        if !settings.open {
            self.desktop_todo.initial_size = None;
            self.desktop_todo.last_bounds = None;
            return;
        }
        let opening = self.desktop_todo.initial_size.is_none();
        let placement = opening
            .then(|| self.shell.as_ref()?.desktop_todo_placement(settings.bounds))
            .flatten();
        let size = *self
            .desktop_todo
            .initial_size
            .get_or_insert_with(|| placement.map_or([400.0, 530.0], |(_, size)| size));
        let focus = std::mem::take(&mut self.desktop_todo.focus);
        let (close, open_editor) = ctx.show_viewport_immediate(
            ViewportId::from_hash_of("windao-desktop-todo"),
            ViewportBuilder::default()
                .with_title("桌面待办")
                .with_icon(self.icon.clone())
                .with_inner_size(size)
                .with_min_inner_size([300.0, 240.0])
                .with_maximize_button(false)
                .with_active(false)
                .with_window_level(if settings.pinned {
                    WindowLevel::AlwaysOnTop
                } else {
                    WindowLevel::Normal
                }),
            |ui, _| {
                let ctx = ui.ctx();
                let scale = ctx.pixels_per_point();
                if let Some(([x, y], _)) = placement {
                    ctx.send_viewport_cmd(ViewportCommand::OuterPosition(egui::pos2(
                        x as f32 / scale,
                        y as f32 / scale,
                    )));
                } else if let Some(bounds) = ctx.input(|input| {
                    let viewport = input.viewport();
                    if viewport.minimized == Some(true) || viewport.maximized == Some(true) {
                        return None;
                    }
                    let outer = viewport.outer_rect?;
                    let inner = viewport.inner_rect?;
                    (inner.width() >= 290.0 && inner.height() >= 230.0 && outer.min.is_finite())
                        .then_some(DesktopTodoBounds {
                            position: [
                                (outer.left() * scale).round() as i32,
                                (outer.top() * scale).round() as i32,
                            ],
                            size: [inner.width().round() as u32, inner.height().round() as u32],
                        })
                }) {
                    self.desktop_todo.last_bounds = Some(bounds);
                }
                if focus {
                    ctx.send_viewport_cmd(ViewportCommand::Minimized(false));
                    ctx.send_viewport_cmd(ViewportCommand::Focus);
                }
                if ctx.input(|input| input.viewport().close_requested()) {
                    return (true, false);
                }
                (false, self.desktop_todo.content.show(ui, &mut self.store))
            },
        );
        if close {
            match self.save_todo_window(true) {
                Ok(()) => {
                    self.desktop_todo.initial_size = None;
                    self.desktop_todo.last_bounds = None;
                }
                Err(error) => self.desktop_todo.content.error = Some(error),
            }
        }
        if open_editor {
            self.open_workbench(Page::Tasks);
        }
        if open_editor
            || close
            || settings.pinned != self.store.state().settings.desktop_todo.pinned
        {
            ctx.request_repaint_of(ViewportId::ROOT);
        }
    }
}

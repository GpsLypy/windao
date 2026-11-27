use eframe::egui::{self, Ui};

#[derive(Clone, Copy, Default, PartialEq)]
enum Operation {
    #[default]
    Add,
    Subtract,
    Multiply,
    Divide,
}

impl Operation {
    const ALL: [(Self, &'static str); 4] = [
        (Self::Add, "+"),
        (Self::Subtract, "−"),
        (Self::Multiply, "×"),
        (Self::Divide, "÷"),
    ];

    fn calculate(self, left: &str, right: &str) -> Result<f64, &'static str> {
        let (Ok(left), Ok(right)) = (left.trim().parse::<f64>(), right.trim().parse::<f64>())
        else {
            return Err("请输入两个数字。");
        };
        if !left.is_finite() || !right.is_finite() {
            return Err("请输入有效数字。");
        }
        let result = match self {
            Self::Add => left + right,
            Self::Subtract => left - right,
            Self::Multiply => left * right,
            Self::Divide if right == 0.0 => return Err("除数不能为零。"),
            Self::Divide => left / right,
        };
        if !result.is_finite() {
            return Err("结果超出范围。");
        }
        // Remove common floating-point display artifacts while retaining significant digits.
        Ok(format!("{result:.14e}").parse().unwrap_or(result))
    }
}

#[derive(Default)]
pub(super) struct Calculator {
    left: String,
    right: String,
    operation: Operation,
    result: Option<Result<f64, &'static str>>,
}

impl Calculator {
    pub(super) fn show(&mut self, ui: &mut Ui) {
        super::card(ui, |ui| {
            let first = super::field(ui, "第一个数字", &mut self.left, "例如 12.5", 40);
            ui.horizontal(|ui| {
                for (operation, label) in Operation::ALL {
                    if ui
                        .selectable_label(self.operation == operation, label)
                        .clicked()
                    {
                        self.operation = operation;
                        self.result = None;
                    }
                }
            });
            let second = super::field(ui, "第二个数字", &mut self.right, "例如 3", 40);
            if first.changed() || second.changed() {
                self.result = None;
            }
            ui.horizontal(|ui| {
                if super::primary(ui, "计算").clicked()
                    || ((first.lost_focus() || second.lost_focus())
                        && ui.input(|input| input.key_pressed(egui::Key::Enter)))
                {
                    self.result = Some(self.operation.calculate(&self.left, &self.right));
                }
                if ui.button("清空").clicked() {
                    *self = Self::default();
                }
            });
            match self.result {
                Some(Ok(value)) => {
                    let value = value.to_string();
                    ui.horizontal_wrapped(|ui| {
                        ui.monospace(&value);
                        if ui.button("复制结果").clicked() {
                            ui.ctx().copy_text(value);
                        }
                    });
                }
                Some(Err(error)) => {
                    ui.colored_label(ui.visuals().error_fg_color, error);
                }
                None => {}
            }
        });
    }
}

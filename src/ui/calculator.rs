use eframe::egui::{self, Button, RichText, Ui, vec2};

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
    entry: String,
    pending: Option<(String, Operation)>,
    expression: String,
    fresh: bool,
    error: Option<&'static str>,
}

impl Calculator {
    fn display(&self) -> &str {
        if self.entry.is_empty() {
            "0"
        } else {
            &self.entry
        }
    }

    fn evaluate(&mut self) {
        if let Some((left, operation)) = self.pending.take() {
            let operator = Operation::ALL
                .iter()
                .find(|(op, _)| *op == operation)
                .unwrap()
                .1;
            self.expression = format!("{left} {operator} {} =", self.display());
            match operation.calculate(&left, self.display()) {
                Ok(value) => self.entry = value.to_string(),
                Err(error) => self.error = Some(error),
            }
            self.fresh = true;
        }
    }

    fn press(&mut self, key: &str) {
        if key == "AC" {
            *self = Self::default();
            return;
        }
        let digit = key.len() == 1 && key.as_bytes()[0].is_ascii_digit();
        if self.error.is_some() {
            if !digit && key != "." {
                return;
            }
            *self = Self::default();
        }
        if let Some(&(operation, label)) = Operation::ALL.iter().find(|(_, label)| *label == key) {
            if !self.fresh {
                self.evaluate();
                if self.error.is_some() {
                    return;
                }
            }
            let left = self.display().to_owned();
            self.expression = format!("{left} {label}");
            self.pending = Some((left, operation));
            self.fresh = true;
        } else if digit || key == "." {
            if self.fresh {
                self.entry.clear();
                if self.pending.is_none() {
                    self.expression.clear();
                }
                self.fresh = false;
            }
            if key == "." && !self.entry.contains('.') && self.entry.len() < 17 {
                if self.entry.is_empty() {
                    self.entry.push('0');
                }
                self.entry.push('.');
            } else if digit && self.entry.len() < 17 {
                if self.entry == "-0" {
                    self.entry.truncate(1);
                } else if self.entry == "0" {
                    self.entry.clear();
                }
                self.entry.push_str(key);
            }
        } else {
            match key {
                "=" => self.evaluate(),
                "⌫" if !self.fresh => {
                    self.entry.pop();
                }
                "+/−" | "%" => {
                    if self.fresh && self.pending.is_some() {
                        self.entry = if key == "+/−" { "-0" } else { "0" }.into();
                        self.fresh = false;
                        return;
                    }
                    if let Ok(value) = self.display().parse::<f64>() {
                        let value = if key == "%" { value / 100.0 } else { -value };
                        self.entry = value.to_string();
                        self.fresh = false;
                    }
                }
                _ => {}
            }
        }
    }

    pub(super) fn show(&mut self, ui: &mut Ui) {
        if ui.is_enabled() {
            self.keyboard(ui);
        }
        let compact = super::in_capsule(ui);
        let wide = ui.available_width() >= if compact { 210.0 } else { 440.0 };
        if wide {
            ui.columns(2, |columns| {
                self.result(&mut columns[0]);
                self.keypad(&mut columns[1]);
            });
        } else {
            self.result(ui);
            ui.add_space(5.0);
            self.keypad(ui);
        }
    }

    fn keyboard(&mut self, ui: &Ui) {
        if ui
            .ctx()
            .input_mut(|input| input.consume_key(egui::Modifiers::NONE, egui::Key::Enter))
        {
            self.press("=");
        }
        let events = ui.input(|input| input.events.clone());
        for event in events {
            match event {
                egui::Event::Text(text) => {
                    for character in text.chars() {
                        let key = match character {
                            '-' => "−",
                            '*' => "×",
                            '/' => "÷",
                            '0'..='9' | '+' | '=' | '.' | '%' => {
                                self.press(&character.to_string());
                                continue;
                            }
                            _ => continue,
                        };
                        self.press(key);
                    }
                }
                egui::Event::Key {
                    key,
                    pressed: true,
                    modifiers,
                    ..
                } if !modifiers.command && !modifiers.alt => match key {
                    egui::Key::Backspace => self.press("⌫"),
                    egui::Key::Delete => self.press("AC"),
                    _ => {}
                },
                _ => {}
            }
        }
    }

    fn result(&mut self, ui: &mut Ui) {
        if !self.expression.is_empty() {
            ui.add(egui::Label::new(RichText::new(&self.expression).monospace().weak()).truncate())
                .on_hover_text(&self.expression);
        }
        let compact = super::in_capsule(ui);
        ui.add(
            egui::Label::new(
                RichText::new(if self.error.is_some() {
                    "错误"
                } else {
                    self.display()
                })
                .monospace()
                .size(if compact { 28.0 } else { 34.0 }),
            )
            .truncate(),
        )
        .on_hover_text(self.display());
        if let Some(error) = self.error {
            ui.colored_label(ui.visuals().error_fg_color, error);
        }
        ui.horizontal(|ui| {
            if ui.button("⌫").on_hover_text("退格").clicked() {
                self.press("⌫");
            }
            if ui
                .add_enabled(self.error.is_none(), Button::new("复制结果"))
                .clicked()
            {
                ui.ctx().copy_text(self.display().to_string());
            }
        });
    }

    fn keypad(&mut self, ui: &mut Ui) {
        let compact = super::in_capsule(ui);
        let height = if compact { 25.0 } else { 31.0 };
        if compact {
            ui.spacing_mut().item_spacing = vec2(4.0, 3.0);
            ui.spacing_mut().button_padding.x = 1.0;
            ui.spacing_mut().interact_size.y = 25.0;
        }
        let gap = ui.spacing().item_spacing.x;
        let width = ((ui.available_width() - gap * 3.0) / 4.0)
            .clamp(if compact { 18.0 } else { 24.0 }, 68.0);
        for row in [
            ["AC", "+/−", "%", "÷"],
            ["7", "8", "9", "×"],
            ["4", "5", "6", "−"],
            ["1", "2", "3", "+"],
            ["0", "", ".", "="],
        ] {
            ui.horizontal(|ui| {
                for key in row.into_iter().filter(|key| !key.is_empty()) {
                    let text_size = if compact && width < 30.0 { 13.0 } else { 16.0 };
                    let mut button =
                        Button::new(RichText::new(key).size(text_size)).corner_radius(7);
                    if key == "=" {
                        let color = ui.visuals().selection.bg_fill;
                        button = button.fill(color);
                    }
                    let span = if key == "0" { width * 2.0 + gap } else { width };
                    if ui.add_sized(vec2(span, height), button).clicked() {
                        self.press(key);
                    }
                }
            });
        }
    }
}

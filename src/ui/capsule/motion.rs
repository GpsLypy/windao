#[derive(Default)]
pub(super) struct Transition {
    position: f32,
    velocity: f32,
    target: f32,
    last_frame: Option<f64>,
}

impl Transition {
    pub(super) fn update(&mut self, now: f64, expanded: bool, instant: bool) -> f32 {
        let target = if expanded { 1.0 } else { 0.0 };
        let previous = self.last_frame.replace(now);
        if instant || previous.is_none() {
            self.target = target;
            self.position = target;
            self.velocity = 0.0;
            return self.position;
        }

        if !self.settled() {
            // A critically damped spring sampled at the actual frame time. Retargeting
            // preserves both position and velocity, even when the cursor reverses direction.
            const FREQUENCY: f32 = 28.0;
            let elapsed = (now - previous.unwrap()).clamp(0.0, 1.0) as f32;
            let offset = self.position - self.target;
            let rate = self.velocity + FREQUENCY * offset;
            let decay = (-FREQUENCY * elapsed).exp();
            self.position = self.target + (offset + rate * elapsed) * decay;
            self.velocity = (self.velocity - FREQUENCY * rate * elapsed) * decay;
            if self.position <= 0.0 || self.position >= 1.0 {
                self.position = self.position.clamp(0.0, 1.0);
                self.velocity = 0.0;
            }
            if (self.position - self.target).abs() < 0.002 && self.velocity.abs() < 0.06 {
                self.position = self.target;
                self.velocity = 0.0;
            }
        }
        self.target = target;
        self.position
    }

    pub(super) fn settled(&self) -> bool {
        self.position == self.target && self.velocity == 0.0
    }
}

pub(super) fn panel_opacity(progress: f32) -> f32 {
    let t = ((progress - 0.10) / 0.52).clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

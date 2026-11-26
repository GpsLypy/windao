use windows_sys::Win32::{
    Globalization::{GetLocaleInfoW, LOCALE_SLOCALIZEDDISPLAYNAME},
    System::Power::{GetSystemPowerStatus, SYSTEM_POWER_STATUS},
    UI::{
        Input::KeyboardAndMouse::{GetKeyState, GetKeyboardLayout, VK_CAPITAL},
        WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId},
    },
};

#[derive(Clone, Default, PartialEq, Eq)]
pub(crate) struct DeviceStatus {
    pub power: String,
    pub battery: Option<u8>,
    pub caps: bool,
    pub language: Option<String>,
}

impl DeviceStatus {
    pub fn read() -> Self {
        let mut status = SYSTEM_POWER_STATUS::default();
        let known = unsafe { GetSystemPowerStatus(&mut status) } != 0;
        let (power, battery) = if known {
            power_reading(
                status.ACLineStatus,
                status.BatteryFlag,
                status.BatteryLifePercent,
            )
        } else {
            ("未知", None)
        };
        let thread =
            unsafe { GetWindowThreadProcessId(GetForegroundWindow(), std::ptr::null_mut()) };
        let language = (thread != 0)
            .then(|| {
                let handle = unsafe { GetKeyboardLayout(thread) } as usize;
                let mut buffer = [0; 128];
                let length = unsafe {
                    GetLocaleInfoW(
                        (handle & 0xffff) as u32,
                        LOCALE_SLOCALIZEDDISPLAYNAME,
                        buffer.as_mut_ptr(),
                        buffer.len() as i32,
                    )
                };
                (handle != 0 && length > 1)
                    .then(|| String::from_utf16_lossy(&buffer[..length as usize - 1]))
            })
            .flatten();
        Self {
            power: power.to_owned(),
            battery,
            caps: unsafe { GetKeyState(i32::from(VK_CAPITAL)) } & 1 != 0,
            language,
        }
    }
}

#[derive(Default)]
pub(crate) struct CapsuleMetrics {
    sampler: crate::system_metrics::Sampler,
    sampled_at: Option<std::time::Instant>,
    caption: String,
}

impl CapsuleMetrics {
    pub fn poll(&mut self, ctx: &eframe::egui::Context) -> String {
        let now = std::time::Instant::now();
        let interval = std::time::Duration::from_secs(3);
        if self
            .sampled_at
            .is_none_or(|at| now.duration_since(at) >= interval)
        {
            let sample = self.sampler.poll(now);
            let mut values = Vec::new();
            if let Some(cpu) = sample.cpu_percent {
                values.push(format!("CPU {cpu:.0}%"));
            }
            if let Some(mem) = sample.memory {
                values.push(format!("内存 {:.0}%", mem.percent()));
            }
            let device = DeviceStatus::read();
            if let Some(battery) = device.battery {
                values.push(format!("电量 {battery}%"));
            }
            self.caption = values.join(" · ");
            self.sampled_at = Some(now);
        }
        ctx.request_repaint_after(
            interval.saturating_sub(now.duration_since(self.sampled_at.unwrap_or(now))),
        );
        self.caption.clone()
    }
}

fn power_reading(line: u8, flags: u8, percent: u8) -> (&'static str, Option<u8>) {
    let present = flags != 128;
    let state = match (line, flags) {
        (1, flag) if flag != 255 && flag & 8 != 0 => "充电中",
        (1, _) => "已连接电源",
        (0, _) => "使用电池",
        _ => "未知",
    };
    (state, (present && percent <= 100).then_some(percent))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn power_reading_handles_missing_battery_and_unknown_sentinels() {
        assert_eq!(power_reading(1, 8, 30), ("充电中", Some(30)));
        assert_eq!(power_reading(1, 128, 255), ("已连接电源", None));
        assert_eq!(power_reading(255, 255, 255), ("未知", None));
        assert_eq!(power_reading(0, 4, 0), ("使用电池", Some(0)));
    }
}

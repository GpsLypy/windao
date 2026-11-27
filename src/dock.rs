use serde::{Deserialize, Serialize};
use std::{
    collections::HashSet,
    time::{Duration, Instant},
};

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum DockEdge {
    #[default]
    Top,
    Left,
    Right,
    Bottom,
}

impl DockEdge {
    pub fn is_horizontal(self) -> bool {
        matches!(self, Self::Top | Self::Bottom)
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapsuleShape {
    #[default]
    Pill,
    Notch,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapsuleSkin {
    #[default]
    Classic,
    Cream,
    GlassLight,
    GlassDark,
    Cloud,
}

impl CapsuleSkin {
    pub const ALL: [(Self, &'static str); 5] = [
        (Self::Classic, "经典黑"),
        (Self::Cream, "奶油"),
        (Self::GlassLight, "浅色玻璃"),
        (Self::GlassDark, "深色玻璃"),
        (Self::Cloud, "悬浮云"),
    ];
    pub fn glass(self) -> bool {
        matches!(self, Self::GlassLight | Self::GlassDark)
    }
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum CapsulePage {
    Today,
    Calendar,
    Tasks,
    Music,
    Focus,
    Notes,
    Files,
    Clipboard,
    Calculator,
    Capture,
    AiWorkbench,
    Market,
    Translation,
    Developer,
    Performance,
    AiUsage,
    Authenticator,
    News,
}

impl CapsulePage {
    const ALL: [Self; 18] = [
        Self::Today,
        Self::Calendar,
        Self::Tasks,
        Self::Music,
        Self::Focus,
        Self::Notes,
        Self::Files,
        Self::Clipboard,
        Self::Calculator,
        Self::Capture,
        Self::AiWorkbench,
        Self::Market,
        Self::Translation,
        Self::Developer,
        Self::Performance,
        Self::AiUsage,
        Self::Authenticator,
        Self::News,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Today => "今天",
            Self::Calendar => "日历",
            Self::Tasks => "待办",
            Self::Music => "音乐",
            Self::Focus => "专注",
            Self::Notes => "便笺",
            Self::Files => "文件",
            Self::Clipboard => "剪贴板",
            Self::Calculator => "计算器",
            Self::Capture => "截屏与录屏",
            Self::AiWorkbench => "AI 工作台",
            Self::Market => "行情",
            Self::Translation => "翻译",
            Self::Developer => "开发工具",
            Self::Performance => "系统监控",
            Self::AiUsage => "AI 用量",
            Self::Authenticator => "身份验证器",
            Self::News => "资讯",
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PageSetting {
    pub page: CapsulePage,
    pub enabled: bool,
}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub enum Resident {
    Companion,
    Ai,
    Clock,
    Alerts,
    Weather,
    Metrics,
}

impl Resident {
    pub const ALL: [Self; 6] = [
        Self::Companion,
        Self::Ai,
        Self::Clock,
        Self::Alerts,
        Self::Weather,
        Self::Metrics,
    ];
    pub fn label(self) -> &'static str {
        match self {
            Self::Companion => "猫咪伙伴",
            Self::Ai => "AI 用量与额度",
            Self::Clock => "时钟与日期",
            Self::Alerts => "消息角标",
            Self::Weather => "天气",
            Self::Metrics => "系统指标",
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(default, rename_all = "camelCase")]
pub struct DockSettings {
    pub edge: DockEdge,
    pub scale: u16,
    pub offset: i16,
    pub oled_shift: bool,
    pub minimal: bool,
    pub clock_24h: bool,
    pub top_lyrics: bool,
    pub translucent: bool,
    pub skin: CapsuleSkin,
    pub shape: CapsuleShape,
    pub companion: bool,
    pub exclude_from_capture: bool,
    pub avoid_browsers: bool,
    pub hide_fullscreen: bool,
    pub avoid_apps: Vec<String>,
    #[serde(deserialize_with = "deserialize_pages")]
    pub pages: Vec<PageSetting>,
    pub resident_order: [Resident; 6],
}

fn deserialize_pages<'de, D: serde::Deserializer<'de>>(
    deserializer: D,
) -> Result<Vec<PageSetting>, D::Error> {
    let mut pages = Vec::<PageSetting>::deserialize(deserializer)?;
    // The original six entries are present in every valid pre-upgrade workspace.
    if pages.len() == 6
        && CapsulePage::ALL[..6]
            .iter()
            .all(|page| pages.iter().any(|entry| entry.page == *page))
    {
        pages.extend(CapsulePage::ALL[6..].iter().map(|&page| PageSetting {
            page,
            enabled: page != CapsulePage::News,
        }));
    }
    Ok(pages)
}

impl Default for DockSettings {
    fn default() -> Self {
        Self {
            edge: DockEdge::Top,
            scale: 100,
            offset: 0,
            oled_shift: false,
            minimal: false,
            clock_24h: true,
            top_lyrics: false,
            translucent: false,
            skin: CapsuleSkin::Classic,
            shape: CapsuleShape::Pill,
            companion: false,
            exclude_from_capture: false,
            avoid_browsers: true,
            hide_fullscreen: true,
            avoid_apps: Vec::new(),
            resident_order: Resident::ALL,
            pages: CapsulePage::ALL
                .map(|page| PageSetting {
                    page,
                    enabled: page != CapsulePage::News,
                })
                .into(),
        }
    }
}

impl DockSettings {
    pub fn factor(&self) -> f32 {
        f32::from(self.scale) / 100.0
    }

    #[cfg(windows)]
    pub fn expanded_size(&self) -> [f32; 2] {
        let size = match self.edge {
            DockEdge::Top | DockEdge::Bottom => [680.0, 318.0],
            DockEdge::Left | DockEdge::Right => [390.0, 496.0],
        };
        size.map(|dimension| dimension * self.factor())
    }

    pub fn validate(&self) -> Result<(), String> {
        let unique: HashSet<_> = self.pages.iter().map(|entry| entry.page).collect();
        if !(80..=140).contains(&self.scale)
            || !(-100..=100).contains(&self.offset)
            || self.resident_order.iter().collect::<HashSet<_>>().len() != Resident::ALL.len()
            || unique.len() != CapsulePage::ALL.len()
            || self.pages.len() != CapsulePage::ALL.len()
            || !self.pages.iter().any(|entry| entry.enabled)
        {
            return Err("胶囊外观或页面设置无效。".into());
        }
        let apps: HashSet<_> = self
            .avoid_apps
            .iter()
            .map(|name| name.to_lowercase())
            .collect();
        if self.avoid_apps.len() > 20
            || self.avoid_apps.len() != apps.len()
            || self
                .avoid_apps
                .iter()
                .any(|name| executable_name(name) != Some(name.as_str()))
        {
            return Err("胶囊避让应用设置无效。".into());
        }
        Ok(())
    }
}

pub fn executable_name(value: &str) -> Option<&str> {
    let name = value.trim().rsplit(['/', '\\']).next()?;
    (name.len() > 4
        && name.encode_utf16().count() <= 255
        && name
            .get(name.len().saturating_sub(4)..)?
            .eq_ignore_ascii_case(".exe")
        && !name
            .chars()
            .any(|ch| ch.is_control() || matches!(ch, ':' | '*' | '?' | '"' | '<' | '>' | '|')))
    .then_some(name)
}

pub fn place(
    [left, top, right, bottom]: [i32; 4],
    width: i32,
    height: i32,
    edge: DockEdge,
    offset: i16,
    shift: i32,
) -> [i32; 4] {
    let width = width.clamp(1, (right - left).max(1));
    let height = height.clamp(1, (bottom - top).max(1));
    let bias = i64::from(offset.clamp(-100, 100) + 100);
    let along = |start: i32, end: i32, size: i32| {
        let span = end - start - size;
        let delta = (i64::from(span) * bias + 100) / 200;
        start + (delta as i32).saturating_add(shift).clamp(0, span.max(0))
    };
    let (x, y) = match edge {
        DockEdge::Top => (along(left, right, width), top),
        DockEdge::Bottom => (along(left, right, width), bottom - height),
        DockEdge::Left => (left, along(top, bottom, height)),
        DockEdge::Right => (right - width, along(top, bottom, height)),
    };
    [x, y, x + width, y + height]
}

#[cfg(any(windows, test))]
pub fn edge_from_pointer(
    [left, top, right, bottom]: [i32; 4],
    [x, y]: [i32; 2],
    [width, height]: [i32; 2],
) -> (DockEdge, i16) {
    let edge = [
        (DockEdge::Top, y.abs_diff(top)),
        (DockEdge::Bottom, y.abs_diff(bottom)),
        (DockEdge::Left, x.abs_diff(left)),
        (DockEdge::Right, x.abs_diff(right)),
    ]
    .into_iter()
    .min_by_key(|(_, distance)| *distance)
    .unwrap()
    .0;
    let (position, start, end, size) = if edge.is_horizontal() {
        (x, left, right, width)
    } else {
        (y, top, bottom, height)
    };
    let span = end - start;
    let size = size.clamp(1, span.max(1));
    let available = span - size;
    let offset = if available > 0 {
        let along = (position - start - size / 2).clamp(0, available);
        ((200 * i64::from(along) + i64::from(available) / 2) / i64::from(available)) as i16 - 100
    } else {
        0
    };
    (edge, offset)
}

#[derive(Default)]
pub struct PixelShift {
    phase: usize,
    next_step: Option<Instant>,
}

impl PixelShift {
    pub fn update(&mut self, now: Instant, enabled: bool, idle: bool) -> (i32, Option<Instant>) {
        const OFFSETS: [i32; 12] = [0, 1, 2, 3, 2, 1, 0, -1, -2, -3, -2, -1];
        const INTERVAL: Duration = Duration::from_secs(30);
        if !enabled {
            *self = Self::default();
            return (0, None);
        }
        if idle {
            let next = self.next_step.get_or_insert(now + INTERVAL);
            if now >= *next {
                self.phase = (self.phase + 1) % OFFSETS.len();
                *next = now + INTERVAL;
            }
        } else {
            self.next_step = None;
        }
        (OFFSETS[self.phase], self.next_step)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn placement_keeps_all_four_edges_inside_offset_monitor_work_area() {
        let work = [-1680, -180, -80, 700];
        assert_eq!(
            place(work, 100, 30, DockEdge::Top, 100, 3),
            [-180, -180, -80, -150]
        );
        assert_eq!(
            place(work, 100, 30, DockEdge::Bottom, -50, -3),
            [-1308, 670, -1208, 700]
        );
        assert_eq!(
            place(work, 30, 100, DockEdge::Left, -100, -3),
            [-1680, -180, -1650, -80]
        );
        assert_eq!(
            place(work, 30, 100, DockEdge::Right, 0, 3),
            [-110, 213, -80, 313]
        );
        assert_eq!(place(work, 3000, 2000, DockEdge::Right, 100, 0), work);
    }

    #[test]
    fn oled_shift_pauses_during_interaction_without_jumping() {
        let mut shift = PixelShift::default();
        let start = Instant::now();
        let step = Duration::from_secs(30);
        assert_eq!(shift.update(start, true, true), (0, Some(start + step)));
        assert_eq!(shift.update(start + step, true, true).0, 1);
        assert_eq!(shift.update(start + step * 3, true, false), (1, None));
        assert_eq!(shift.update(start + step * 4, true, true).0, 1);
        assert_eq!(shift.update(start + step * 5, true, true).0, 2);
        assert_eq!(shift.update(start + step * 5, false, true), (0, None));
    }

    #[test]
    fn old_settings_get_valid_defaults() {
        let settings: DockSettings = serde_json::from_str("{}").unwrap();
        assert!(settings.validate().is_ok());
        assert_eq!(settings.edge, DockEdge::Top);
        assert_eq!(settings.resident_order, Resident::ALL);
        assert!(!settings.oled_shift);
        assert!(!settings.top_lyrics);
        assert!(!settings.exclude_from_capture);
    }

    #[test]
    fn existing_page_order_and_visibility_survive_the_panel_upgrade() {
        let original = serde_json::json!([
            {"page": "notes", "enabled": true},
            {"page": "calendar", "enabled": false},
            {"page": "today", "enabled": true},
            {"page": "tasks", "enabled": true},
            {"page": "music", "enabled": false},
            {"page": "focus", "enabled": true}
        ]);
        let upgraded: DockSettings = serde_json::from_value(serde_json::json!({
            "pages": original
        }))
        .unwrap();
        assert!(upgraded.validate().is_ok());
        assert_eq!(
            serde_json::to_value(&upgraded.pages[..6]).unwrap(),
            original
        );
        assert!(
            upgraded
                .pages
                .iter()
                .any(|p| p.page == CapsulePage::AiUsage && p.enabled)
        );
        assert!(
            upgraded
                .pages
                .iter()
                .any(|p| p.page == CapsulePage::News && !p.enabled)
        );
        assert_eq!(
            serde_json::from_value::<DockSettings>(serde_json::to_value(&upgraded).unwrap())
                .unwrap(),
            upgraded
        );
    }

    #[test]
    fn bottom_edge_round_trips_through_settings() {
        let settings = DockSettings {
            edge: DockEdge::Bottom,
            ..DockSettings::default()
        };
        let json = serde_json::to_value(&settings).unwrap();
        assert_eq!(json["edge"], "bottom");
        assert_eq!(
            serde_json::from_value::<DockSettings>(json).unwrap(),
            settings
        );
    }

    #[test]
    fn dropped_capsule_snaps_to_pointer_edge_on_an_offset_display() {
        let work = [-1600, -180, -80, 680];
        let (edge, offset) = edge_from_pointer(work, [-1000, 720], [100, 30]);
        assert_eq!(edge, DockEdge::Bottom);
        let [left, top, right, bottom] = place(work, 100, 30, edge, offset, 0);
        assert!(((left + right) / 2 + 1000).abs() <= 4);
        assert_eq!([top, bottom], [650, 680]);
        assert_eq!(
            edge_from_pointer(work, [-1590, 650], [30, 100]),
            (DockEdge::Left, 100)
        );
    }
}

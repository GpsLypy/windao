use super::{CapsuleOptions, CapsuleState, Rect, Store};
use crate::dock::Resident;

pub(super) fn layout(
    rect: Rect,
    store: &Store,
    state: &CapsuleState,
    options: &CapsuleOptions<'_>,
) -> [Option<Rect>; 6] {
    let mut slots = [None; 6];
    if !options.dock.is_horizontal() || options.minimal {
        let mut clock = rect;
        if !options.minimal && state.alert_count > 0 {
            let mut badge = rect;
            clock.min.y += 17. * options.scale;
            badge.max.y = clock.top();
            slots[Resident::Alerts as usize] = Some(badge);
        }
        slots[Resident::Clock as usize] = Some(clock);
        return slots;
    }
    let prefs = &store.state().settings;
    let width = |resident| match resident {
        Resident::Companion if prefs.dock.companion => 24.,
        Resident::Ai if state.ai_caption.is_some() => 100.,
        Resident::Alerts if state.alert_count > 0 => 18.,
        Resident::Weather if state.weather_caption.is_some() => 46.,
        Resident::Metrics if prefs.capsule_device_metrics => 194.,
        _ => 0.,
    };
    let extras = Resident::ALL.map(width).into_iter().sum::<f32>() * options.scale;
    let allowed = (rect.width() - 80. * options.scale).max(0.);
    let factor = if extras > allowed {
        allowed / extras
    } else {
        1.
    };
    let mut x = rect.left();
    for resident in prefs.dock.resident_order {
        let w = if resident == Resident::Clock {
            rect.width() - extras * factor
        } else {
            width(resident) * options.scale * factor
        };
        if w <= 0. {
            continue;
        }
        let mut area = rect;
        area.min.x = x;
        x = (x + w).min(rect.right());
        area.max.x = x;
        slots[resident as usize] = Some(area);
    }
    slots
}

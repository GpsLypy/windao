//! Read a known player's public top-level window caption when media metadata is incomplete.
#[derive(Clone, Debug, Eq, PartialEq)]
pub(super) struct Caption {
    pub source: String,
    pub title: String,
    pub artist: String,
}

fn brand(exe: &str) -> Option<&'static str> {
    match exe {
        "cloudmusic.exe" => Some("网易云音乐"),
        "qqmusic.exe" => Some("QQ音乐"),
        "kugou.exe" => Some("酷狗音乐"),
        "sodamusic.exe" => Some("汽水音乐"),
        _ => None,
    }
}

#[cfg(windows)]
pub(super) fn is_netease_source(source: &str) -> bool {
    let source = source.to_ascii_lowercase();
    source.contains("cloudmusic") || source.contains("netease") || source.contains("网易云音乐")
}

fn parse(exe: &str, caption: &str, known_title: Option<&str>) -> Option<Caption> {
    let brand = brand(exe)?;
    let raw = caption.trim();
    let body = raw.strip_suffix(&format!(" - {brand}")).unwrap_or(raw);
    let (a, b) = body.split_once(" - ")?;
    let (a, b) = (a.trim(), b.trim());
    if a.is_empty() || b.is_empty() || b.contains(" - ") || a == brand || b == brand {
        return None;
    }
    let (title, artist) = match known_title.filter(|v| !v.is_empty()) {
        Some(known) if a.eq_ignore_ascii_case(known) => (known, b),
        Some(known) if b.eq_ignore_ascii_case(known) => (known, a),
        Some(_) => return None,
        None if exe == "kugou.exe" && raw != body => (b, a),
        // Other versions vary in field order; keep the visible text verbatim.
        None if raw != body || exe == "cloudmusic.exe" => (body, ""),
        None => return None,
    };
    Some(Caption {
        source: exe.into(),
        title: title.into(),
        artist: artist.into(),
    })
}

#[cfg(windows)]
pub(super) fn read(source: Option<(&str, &str)>) -> Option<Caption> {
    use std::collections::HashMap;
    use windows_sys::Win32::{
        Foundation::{HWND, LPARAM},
        UI::WindowsAndMessaging::{
            EnumWindows, GetWindowTextW, GetWindowThreadProcessId, IsWindowVisible,
        },
    };
    struct Search<'a> {
        source: Option<(&'a str, &'a str)>,
        owners: HashMap<u32, Option<String>>,
        rows: Vec<Caption>,
    }
    unsafe extern "system" fn visit(hwnd: HWND, data: LPARAM) -> i32 {
        let search = unsafe { &mut *(data as *mut Search<'_>) };
        if unsafe { IsWindowVisible(hwnd) } == 0 {
            return 1;
        }
        let mut pid = 0;
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut pid);
        }
        let owner = search.owners.entry(pid).or_insert_with(|| {
            let name = crate::windows::executable_name(pid)?.to_ascii_lowercase();
            brand(&name).map(|_| name)
        });
        let Some(exe) = owner else { return 1 };
        if let Some((source, _)) = search.source {
            let app = source.to_ascii_lowercase();
            let name = exe.trim_end_matches(".exe");
            if !app.contains(name)
                && !app.contains(brand(exe).unwrap_or(""))
                && !(exe == "cloudmusic.exe" && is_netease_source(&app))
            {
                return 1;
            }
        }
        let mut buf = [0; 512];
        let size = unsafe { GetWindowTextW(hwnd, buf.as_mut_ptr(), buf.len() as i32) };
        if size > 0
            && (size as usize) < buf.len() - 1
            && let Some(row) = parse(
                exe,
                &String::from_utf16_lossy(&buf[..size as usize]),
                search.source.map(|(_, title)| title),
            )
            && !search.rows.contains(&row)
        {
            search.rows.push(row);
        }
        (search.rows.len() < 2) as i32
    }
    let mut search = Search {
        source,
        owners: HashMap::new(),
        rows: Vec::new(),
    };
    unsafe {
        EnumWindows(Some(visit), (&mut search as *mut Search<'_>) as isize);
    }
    if search.rows.len() == 1 {
        search.rows.pop()
    } else {
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn keep_unknown_field_order_and_require_unambiguous_captions() {
        assert_eq!(
            parse("kugou.exe", "甲 - 乙 - 酷狗音乐", None)
                .unwrap()
                .title,
            "乙"
        );
        for (exe, brand) in [
            ("qqmusic.exe", "QQ音乐"),
            ("sodamusic.exe", "汽水音乐"),
            ("cloudmusic.exe", "网易云音乐"),
        ] {
            let row = parse(exe, &format!("甲 - 乙 - {brand}"), None).unwrap();
            assert_eq!(row.title, "甲 - 乙");
            assert!(row.artist.is_empty());
            assert_eq!(parse(exe, "甲 - 乙", Some("乙")).unwrap().artist, "甲");
            assert!(parse(exe, "甲 - 乙", Some("丙")).is_none());
            assert!(parse(exe, brand, None).is_none());
        }
        assert_eq!(
            parse("cloudmusic.exe", "甲 - 乙", None).unwrap().title,
            "甲 - 乙"
        );
        assert!(parse("qqmusic.exe", "甲 - 乙", None).is_none());
        assert!(parse("browser.exe", "甲 - 乙 - 酷狗音乐", None).is_none());
    }
}

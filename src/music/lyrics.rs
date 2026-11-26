use std::time::Duration;

const MAX_BYTES: usize = 256 * 1024;
const MAX_LINES: usize = 4096;
const MAX_LINE_BYTES: usize = 2048;

#[derive(Debug)]
pub(super) struct Lyrics(Vec<(u64, String)>);

impl Lyrics {
    pub(super) fn parse(raw: &str) -> Result<Self, String> {
        if raw.len() > MAX_BYTES {
            return Err("歌词超过 256 KiB。".into());
        }
        let raw = raw.trim_start_matches('\u{feff}');
        let offset = raw
            .lines()
            .filter_map(|line| line.trim().strip_prefix("[offset:")?.strip_suffix(']'))
            .filter_map(|value| value.trim().parse::<i64>().ok())
            .next_back()
            .unwrap_or(0);
        let mut lines = Vec::new();
        for mut body in raw.lines().map(str::trim) {
            let mut stamps = Vec::new();
            while let Some((tag, rest)) = body.strip_prefix('[').and_then(|s| s.split_once(']')) {
                let Some(ms) = timestamp(tag) else { break };
                let adjusted = ms.saturating_add_signed(offset);
                stamps.push(adjusted);
                body = rest;
            }
            if stamps.is_empty() {
                continue;
            }
            let text = body.trim();
            if text.len() > MAX_LINE_BYTES || lines.len() + stamps.len() > MAX_LINES {
                return Err("歌词的行数或行长度超过上限。".into());
            }
            lines.extend(stamps.into_iter().map(|at| (at, text.to_owned())));
        }
        // Keep the first lyric at duplicate timestamps so a translation does not replace it.
        lines.sort_by_key(|(time, _)| *time);
        lines.dedup_by_key(|(time, _)| *time);
        if !lines.iter().any(|(_, text)| !text.is_empty()) {
            return Err("没有找到带时间戳的歌词。".into());
        }
        Ok(Self(lines))
    }

    pub(super) fn at(&self, position: f64) -> Option<&str> {
        Some(self.0[self.active_index(position)?].1.as_str())
    }

    #[cfg(windows)]
    pub(super) fn lines(&self) -> &[(u64, String)] {
        &self.0
    }

    pub(super) fn active_index(&self, position: f64) -> Option<usize> {
        let index = self.next_index(position).checked_sub(1)?;
        (!self.0[index].1.is_empty()).then_some(index)
    }

    pub(super) fn next_update(&self, position: f64, rate: f64) -> Option<Duration> {
        let (next, _) = self.0.get(self.next_index(position))?;
        if !rate.is_finite() || rate <= 0.0 {
            return None;
        }
        let seconds = ((*next as f64 / 1000.0 - position) / rate).max(0.001);
        Duration::try_from_secs_f64(seconds).ok()
    }

    fn next_index(&self, position: f64) -> usize {
        let elapsed = (position * 1000.0) as u64;
        self.0.partition_point(|(at, _)| *at <= elapsed)
    }
}

fn timestamp(tag: &str) -> Option<u64> {
    let (minutes, time) = tag.split_once(':')?;
    let minutes = minutes.parse::<u64>().ok()?;
    let (seconds, fraction) = time.split_once('.').unwrap_or((time, ""));
    let seconds = seconds
        .parse::<u64>()
        .ok()
        .filter(|seconds| *seconds < 60)?;
    let ms = match fraction.len() {
        0 => 0,
        digits @ 1..=3 if fraction.bytes().all(|digit| digit.is_ascii_digit()) => {
            fraction.parse::<u64>().ok()? * 10_u64.pow(3 - digits as u32)
        }
        _ => return None,
    };
    minutes
        .checked_mul(60_000)?
        .checked_add(seconds * 1000 + ms)
}

#[cfg(windows)]
pub(super) fn read(path: &std::path::Path) -> Result<String, String> {
    use std::{fs::File, io::Read};
    let mut bytes = Vec::new();
    File::open(path)
        .and_then(|file| file.take(MAX_BYTES as u64 + 1).read_to_end(&mut bytes))
        .map_err(|error| format!("无法读取歌词文件（{error}）。"))?;
    if bytes.len() > MAX_BYTES {
        return Err("歌词超过 256 KiB。".into());
    }
    if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let little_endian = bytes[0] == 0xff;
        let body = &bytes[2..];
        let (units, remainder) = body.as_chunks::<2>();
        if !remainder.is_empty() {
            return Err("歌词文件的 UTF-16 编码不完整。".into());
        }
        let units: Vec<_> = units
            .iter()
            .map(|&pair| {
                if little_endian {
                    u16::from_le_bytes(pair)
                } else {
                    u16::from_be_bytes(pair)
                }
            })
            .collect();
        return String::from_utf16(&units).map_err(|_| "歌词文件的 UTF-16 编码无效。".into());
    }
    String::from_utf8(bytes).map_err(|_| "请选择 UTF-8 或带 BOM 的 UTF-16 LRC 文件。".into())
}

#[cfg(windows)]
pub(super) fn lookup(track: &super::Snapshot) -> Result<String, String> {
    let duration = track
        .duration
        .filter(|duration| duration.is_finite() && *duration > 0.0)
        .ok_or("播放器尚未提供曲目时长。")?;
    let mut url = url::Url::parse("https://lrclib.net/api/get").expect("LRCLIB endpoint");
    url.query_pairs_mut()
        .append_pair("track_name", track.title.trim())
        .append_pair("artist_name", track.artist.trim())
        .append_pair("album_name", track.album.trim())
        .append_pair("duration", &duration.round().to_string());
    let (status, response) =
        crate::calendar_sync::get_json(&url).map_err(|_| "无法连接 LRCLIB，请检查网络后重试。")?;
    match status {
        200 => select_matching_lyrics(
            &response,
            &track.title,
            &track.artist,
            &track.album,
            duration,
        ),
        404 => Err("LRCLIB 暂无这首曲目的同步歌词。".into()),
        429 | 503 => Err("LRCLIB 当前繁忙，请稍后重试。".into()),
        _ => Err(format!("LRCLIB 查询失败（HTTP {status}）。")),
    }
}

fn select_matching_lyrics(
    response: &str,
    title: &str,
    artist: &str,
    album: &str,
    duration: f64,
) -> Result<String, String> {
    #[derive(serde::Deserialize)]
    #[serde(rename_all = "camelCase")]
    struct Track {
        track_name: String,
        artist_name: String,
        album_name: String,
        duration: f64,
        synced_lyrics: Option<String>,
    }

    let found: Track = serde_json::from_str(response).map_err(|_| "LRCLIB 返回的歌词格式有误。")?;
    let same = |left: &str, right: &str| left.trim().to_lowercase() == right.trim().to_lowercase();
    if !same(&found.track_name, title)
        || !same(&found.artist_name, artist)
        || (!album.trim().is_empty() && !same(&found.album_name, album))
        || (found.duration - duration).abs() > 3.0
    {
        return Err("没有找到与当前曲目版本匹配的歌词。".into());
    }
    found
        .synced_lyrics
        .filter(|raw| !raw.trim().is_empty())
        .ok_or_else(|| "LRCLIB 暂无这首曲目的同步歌词。".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lrc_uses_offsets_and_clears_during_timed_instrumental_gaps() {
        let lyrics = Lyrics::parse(
            "\u{feff}[ar:test]\n[00:01.10][00:03.5]第一行\n[00:02]\n[00:03.500]译文\n[offset:200]",
        )
        .unwrap();
        assert_eq!(lyrics.at(1.29), None);
        assert_eq!(lyrics.at(1.3), Some("第一行"));
        assert_eq!(lyrics.at(2.2), None);
        assert_eq!(lyrics.at(3.75), Some("第一行"));
        assert_eq!(lyrics.active_index(1.3), Some(0));
        assert_eq!(lyrics.active_index(2.2), None);
        assert_eq!(lyrics.active_index(3.75), Some(2));
        assert_eq!(lyrics.active_index(1.5), Some(0));
        assert_eq!(lyrics.next_update(2.2, 1.5), Some(Duration::from_secs(1)));
        assert_eq!(lyrics.next_update(4.0, 1.0), None);
        assert!(Lyrics::parse("[00:01]\n没有时间戳").is_err());
    }

    #[test]
    fn online_lyrics_require_matching_recording() {
        let row = serde_json::json!({
            "trackName": "Track",
            "artistName": "Artist",
            "albumName": "Live Sessions",
            "duration": 180,
            "syncedLyrics": "[00:01.00]第一行"
        })
        .to_string();
        assert!(select_matching_lyrics(&row, "Track", "Artist", "Live Sessions", 179.8).is_ok());
        assert!(select_matching_lyrics(&row, "Track", "Artist", "Studio Sessions", 180.0).is_err());
        assert!(select_matching_lyrics(&row, "Track", "Artist", "Live Sessions", 174.0).is_err());
    }
}

use std::{
    fs,
    io::{self, Read, Seek, SeekFrom},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};
use uuid::Uuid;

const MAX_ENTRIES: usize = 24_000;
const MAX_FILES: usize = 384;
const PER_FILE: usize = 12 << 20;
const TOTAL_BYTES: usize = 96 << 20;
const LINE_LIMIT: usize = 2 << 20;

pub(super) struct Logs {
    files: Vec<(SystemTime, PathBuf)>,
    since: i64,
    remaining: usize,
    pub found: bool,
    pub partial: bool,
}

#[derive(Clone, Copy)]
enum Location {
    Year,
    Month,
    Day,
    Rollouts,
    Projects,
    Sessions,
    SubagentDirectory,
    Subagents,
}

fn plain(meta: &fs::Metadata) -> bool {
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        if meta.file_attributes() & 0x400 != 0 {
            return false;
        }
    }
    !meta.is_symlink()
}

fn session_id(name: &str) -> bool {
    Uuid::parse_str(name).is_ok()
}
fn rollout(name: &str) -> bool {
    name.strip_prefix("rollout-")
        .and_then(|s| s.strip_suffix(".jsonl"))
        .and_then(|s| s.get(s.len().checked_sub(36)?..))
        .is_some_and(session_id)
}

impl Logs {
    pub fn codex(home: &Path, since: i64) -> Self {
        let mut result = Self::new(since);
        result.scan_root(home, "sessions", Location::Year);
        result.scan_root(home, "archived_sessions", Location::Rollouts);
        result
    }
    pub fn claude(home: &Path, since: i64) -> Self {
        let mut result = Self::new(since);
        result.scan_root(home, "projects", Location::Projects);
        result
    }
    fn new(since: i64) -> Self {
        Self {
            files: Vec::new(),
            since,
            remaining: MAX_ENTRIES,
            found: false,
            partial: false,
        }
    }
    fn scan_root(&mut self, home: &Path, name: &str, location: Location) {
        match fs::symlink_metadata(home) {
            Ok(m) if m.is_dir() && plain(&m) => self.walk(&home.join(name), location),
            Ok(_) => self.partial = true,
            Err(e) => self.partial |= e.kind() != io::ErrorKind::NotFound,
        }
    }
    fn walk(&mut self, dir: &Path, location: Location) {
        match fs::symlink_metadata(dir) {
            Ok(m) if !plain(&m) || !m.is_dir() => return,
            Err(e) => {
                self.partial |= e.kind() != io::ErrorKind::NotFound;
                return;
            }
            _ => {}
        }
        let Ok(entries) = fs::read_dir(dir) else {
            self.partial = true;
            return;
        };
        self.found = true;
        for entry in entries {
            if self.remaining == 0 {
                self.partial = true;
                break;
            }
            self.remaining -= 1;
            let Ok(entry) = entry else {
                self.partial = true;
                continue;
            };
            let name = entry.file_name();
            let Some(name) = name.to_str() else {
                continue;
            };
            let Ok(ft) = entry.file_type() else {
                self.partial = true;
                continue;
            };
            if ft.is_dir() {
                let numeric = |len| name.len() == len && name.bytes().all(|b| b.is_ascii_digit());
                let next = match location {
                    Location::Year if numeric(4) => Location::Month,
                    Location::Month if numeric(2) => Location::Day,
                    Location::Day if numeric(2) => Location::Rollouts,
                    Location::Projects => Location::Sessions,
                    Location::Sessions if session_id(name) => Location::SubagentDirectory,
                    Location::SubagentDirectory if name == "subagents" => Location::Subagents,
                    _ => continue,
                };
                self.walk(&entry.path(), next);
                continue;
            }
            let accepts = match location {
                Location::Rollouts => rollout(name),
                Location::Sessions => name.strip_suffix(".jsonl").is_some_and(session_id),
                Location::Subagents => name.starts_with("agent-") && name.ends_with(".jsonl"),
                _ => false,
            };
            if ft.is_file() && accepts {
                let Ok(m) = entry.metadata() else {
                    self.partial = true;
                    continue;
                };
                if !plain(&m) {
                    continue;
                }
                let Ok(modified) = m.modified() else {
                    self.partial = true;
                    continue;
                };
                if modified
                    .duration_since(UNIX_EPOCH)
                    .is_ok_and(|d| d.as_secs() as i64 >= self.since)
                {
                    self.files.push((modified, entry.path()));
                }
            }
        }
    }
    pub fn lines(&mut self, mut consume: impl FnMut(&Path, &[u8], bool)) {
        self.files.sort_unstable_by(|a, b| b.cmp(a));
        self.partial |= self.files.len() > MAX_FILES;
        let mut remaining = TOTAL_BYTES;
        let mut bytes = Vec::new();
        for (_, path) in self.files.iter().take(MAX_FILES) {
            if remaining == 0 {
                self.partial = true;
                break;
            }
            let read = (|| -> io::Result<()> {
                let mut file = fs::File::open(path)?;
                let len = file.metadata()?.len();
                let count = len.min(PER_FILE.min(remaining) as u64);
                let truncated = len > count;
                self.partial |= truncated;
                file.seek(SeekFrom::Start(len - count))?;
                bytes.clear();
                file.take(count).read_to_end(&mut bytes)?;
                remaining -= bytes.len();
                for (index, line) in bytes.split(|&b| b == b'\n').enumerate() {
                    if (truncated && index == 0) || line.is_empty() {
                        continue;
                    }
                    if line.len() > LINE_LIMIT {
                        self.partial = true;
                        continue;
                    }
                    consume(path, line, truncated);
                }
                Ok(())
            })();
            self.partial |= read.is_err();
        }
    }
}

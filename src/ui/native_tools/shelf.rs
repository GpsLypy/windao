use std::{
    collections::HashSet,
    ffi::OsString,
    fs::{File, OpenOptions},
    io::{self, Read, Write},
    os::windows::{
        ffi::{OsStrExt, OsStringExt},
        fs::OpenOptionsExt,
        io::AsRawHandle,
    },
    path::{Component, Path, PathBuf, Prefix},
};

use serde::{Deserialize, Serialize};
use tempfile::NamedTempFile;
use uuid::Uuid;
use windows_sys::Win32::{
    Storage::FileSystem::{
        FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_INFO, FILE_NAME_NORMALIZED, FILE_READ_ATTRIBUTES,
        FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, FILE_TYPE_DISK, FileIdInfo,
        GetDriveTypeW, GetFileInformationByHandleEx, GetFileType, GetFinalPathNameByHandleW,
        VOLUME_NAME_DOS,
    },
    System::WindowsProgramming::{DRIVE_CDROM, DRIVE_FIXED, DRIVE_RAMDISK, DRIVE_REMOVABLE},
};

pub(super) const CAPACITY: usize = 200;
const MAX_DOCUMENT_BYTES: u64 = 32 * 1024 * 1024;
const FILENAME: &str = "file-shelf.json";

#[derive(Clone, Deserialize, Eq, PartialEq, Serialize)]
struct Identity {
    volume: u64,
    file: [u8; 16],
}

#[derive(Clone, Deserialize, Serialize)]
pub(super) struct Entry {
    pub(super) id: Uuid,
    pub(super) path: PathBuf,
    pub(super) directory: bool,
    target: PathBuf,
    identity: Identity,
}

impl Entry {
    pub(super) fn name(&self) -> String {
        self.path
            .file_name()
            .unwrap_or(self.path.as_os_str())
            .to_string_lossy()
            .into_owned()
    }

    fn verified(&self, allow_delete: bool) -> io::Result<File> {
        let item = LocalItem::read(&self.path, allow_delete).map_err(|_| unavailable())?;
        if item.target != self.target
            || item.identity != self.identity
            || item.directory != self.directory
        {
            return Err(unavailable());
        }
        Ok(item.handle)
    }
}

pub(super) struct Shelf {
    document: PathBuf,
    pub(super) entries: Vec<Entry>,
    pub(super) unavailable: HashSet<Uuid>,
}

impl Shelf {
    pub(super) fn load(directory: &Path) -> io::Result<Self> {
        let document = directory.join(FILENAME);
        let entries = read_document(&document).map_err(|cause| {
            io::Error::other(format!("无法读取暂存记录，原记录已保留（{cause}）"))
        })?;
        let mut shelf = Self {
            document,
            entries,
            unavailable: HashSet::new(),
        };
        shelf.refresh();
        Ok(shelf)
    }

    pub(super) fn refresh(&mut self) {
        self.unavailable = self
            .entries
            .iter()
            .filter(|entry| entry.verified(true).is_err())
            .map(|entry| entry.id)
            .collect();
    }

    pub(super) fn add(&mut self, paths: impl IntoIterator<Item = PathBuf>) -> io::Result<()> {
        let mut next = self.entries.clone();
        let mut refreshed = HashSet::new();
        for (index, path) in paths.into_iter().enumerate() {
            if index >= CAPACITY {
                return Err(capacity_reached());
            }
            let item = LocalItem::read(&path, true)?;
            let existing = next.iter().position(|entry| entry.target == item.target);
            let id = existing.map_or_else(Uuid::new_v4, |index| next[index].id);
            let entry = Entry {
                id,
                path,
                target: item.target,
                directory: item.directory,
                identity: item.identity,
            };
            if let Some(index) = existing {
                next[index] = entry;
            } else if next.len() < CAPACITY {
                next.push(entry);
            } else {
                return Err(capacity_reached());
            }
            refreshed.insert(id);
        }
        self.write(next)?;
        self.unavailable.retain(|id| !refreshed.contains(id));
        Ok(())
    }

    pub(super) fn remove(&mut self, ids: &HashSet<Uuid>) -> io::Result<()> {
        let next = self
            .entries
            .iter()
            .filter(|entry| !ids.contains(&entry.id))
            .cloned()
            .collect();
        self.write(next)?;
        self.unavailable.retain(|id| !ids.contains(id));
        Ok(())
    }

    pub(super) fn clear(&mut self) -> io::Result<()> {
        self.write(Vec::new())?;
        self.unavailable.clear();
        Ok(())
    }

    pub(super) fn action(
        &mut self,
        id: Uuid,
        action: impl FnOnce(&Path) -> io::Result<()>,
    ) -> io::Result<()> {
        let entry = self
            .entries
            .iter()
            .find(|entry| entry.id == id)
            .ok_or_else(unavailable)?;
        let _handle = entry.verified(false).inspect_err(|_| {
            self.unavailable.insert(id);
        })?;
        action(&entry.path)
    }

    pub(super) fn copy_paths(&mut self, ids: &HashSet<Uuid>) -> io::Result<String> {
        let mut paths = Vec::with_capacity(ids.len());
        for entry in self.entries.iter().filter(|entry| ids.contains(&entry.id)) {
            entry.verified(true).inspect_err(|_| {
                self.unavailable.insert(entry.id);
            })?;
            paths.push(entry.path.to_string_lossy().into_owned());
        }
        Ok(paths.join("\r\n"))
    }

    fn write(&mut self, next: Vec<Entry>) -> io::Result<()> {
        let directory = self
            .document
            .parent()
            .ok_or_else(|| io::Error::other("缺少暂存记录目录"))?;
        let mut temp = NamedTempFile::new_in(directory)?;
        serde_json::to_writer(&mut temp, &next).map_err(io::Error::other)?;
        temp.write_all(b"\n")?;
        if temp.as_file().metadata()?.len() > MAX_DOCUMENT_BYTES {
            return Err(io::Error::other("暂存记录超过 32 MiB"));
        }
        temp.as_file().sync_all()?;
        temp.persist(&self.document).map_err(|error| error.error)?;
        self.entries = next;
        Ok(())
    }
}

fn read_document(document: &Path) -> io::Result<Vec<Entry>> {
    let file = match File::open(document) {
        Ok(file) => file,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(error) => return Err(error),
    };
    let mut bytes = Vec::new();
    file.take(MAX_DOCUMENT_BYTES + 1).read_to_end(&mut bytes)?;
    if bytes.len() as u64 > MAX_DOCUMENT_BYTES {
        return Err(io::Error::other("暂存记录超过 32 MiB"));
    }
    let entries: Vec<Entry> = serde_json::from_slice(&bytes).map_err(io::Error::other)?;
    if entries.len() > CAPACITY {
        return Err(capacity_reached());
    }
    let mut ids = HashSet::new();
    let mut targets = HashSet::new();
    for entry in &entries {
        disk_letter(&entry.path)?;
        disk_letter(&entry.target)?;
        if !ids.insert(entry.id) || !targets.insert(&entry.target) {
            return Err(io::Error::other("暂存记录包含重复项目"));
        }
    }
    Ok(entries)
}

struct LocalItem {
    handle: File,
    target: PathBuf,
    directory: bool,
    identity: Identity,
}

impl LocalItem {
    fn read(path: &Path, allow_delete: bool) -> io::Result<Self> {
        require_local_disk(path)?;
        let mut share = FILE_SHARE_READ | FILE_SHARE_WRITE;
        if allow_delete {
            share |= FILE_SHARE_DELETE;
        }
        let handle = OpenOptions::new()
            .access_mode(FILE_READ_ATTRIBUTES)
            .share_mode(share)
            .custom_flags(FILE_FLAG_BACKUP_SEMANTICS)
            .open(path)?;
        if unsafe { GetFileType(handle.as_raw_handle()) } != FILE_TYPE_DISK {
            return Err(local_path_required());
        }
        let target = final_path(&handle)?;
        require_local_disk(&target)?;
        let metadata = handle.metadata()?;
        if !metadata.is_file() && !metadata.is_dir() {
            return Err(local_path_required());
        }
        let mut info = FILE_ID_INFO::default();
        let result = unsafe {
            GetFileInformationByHandleEx(
                handle.as_raw_handle(),
                FileIdInfo,
                (&mut info as *mut FILE_ID_INFO).cast(),
                size_of::<FILE_ID_INFO>() as u32,
            )
        };
        if result == 0 {
            return Err(io::Error::last_os_error());
        }
        if info.FileId.Identifier == [0; 16] {
            return Err(io::Error::other("文件系统未提供可验证的文件标识"));
        }
        Ok(Self {
            handle,
            target,
            directory: metadata.is_dir(),
            identity: Identity {
                volume: info.VolumeSerialNumber,
                file: info.FileId.Identifier,
            },
        })
    }
}

fn final_path(handle: &File) -> io::Result<PathBuf> {
    let mut buffer = [0u16; 512];
    let resolve = |buffer: &mut [u16]| unsafe {
        GetFinalPathNameByHandleW(
            handle.as_raw_handle(),
            buffer.as_mut_ptr(),
            buffer.len() as u32,
            FILE_NAME_NORMALIZED | VOLUME_NAME_DOS,
        ) as usize
    };
    match resolve(&mut buffer) {
        0 => Err(io::Error::last_os_error()),
        written if written < buffer.len() => Ok(wide_path(&buffer[..written])),
        needed => {
            let mut buffer = vec![0; needed];
            match resolve(&mut buffer) {
                0 => Err(io::Error::last_os_error()),
                written if written < buffer.len() => Ok(wide_path(&buffer[..written])),
                _ => Err(io::Error::other("文件路径已改变，请重新添加")),
            }
        }
    }
}

fn require_local_disk(path: &Path) -> io::Result<()> {
    let letter = disk_letter(path)?;
    let root = [letter.into(), b':'.into(), b'\\'.into(), 0];
    let drive = unsafe { GetDriveTypeW(root.as_ptr()) };
    if matches!(
        drive,
        DRIVE_FIXED | DRIVE_REMOVABLE | DRIVE_CDROM | DRIVE_RAMDISK
    ) {
        Ok(())
    } else {
        Err(local_path_required())
    }
}

fn disk_letter(path: &Path) -> io::Result<u8> {
    let mut parts = path.components();
    let Some(Component::Prefix(prefix)) = parts.next() else {
        return Err(local_path_required());
    };
    if !matches!(parts.next(), Some(Component::RootDir)) {
        return Err(local_path_required());
    }
    let (Prefix::Disk(letter) | Prefix::VerbatimDisk(letter)) = prefix.kind() else {
        return Err(local_path_required());
    };
    if parts.any(|component| match component {
        Component::Normal(segment) => segment
            .encode_wide()
            .any(|unit| matches!(unit, 0..=31 | 58)),
        _ => true,
    }) {
        return Err(local_path_required());
    }
    Ok(letter)
}

fn wide_path(wide: &[u16]) -> PathBuf {
    PathBuf::from(OsString::from_wide(wide))
}

fn unavailable() -> io::Error {
    io::Error::new(
        io::ErrorKind::NotFound,
        "文件已移动、替换或暂不可用，请重新添加",
    )
}

fn capacity_reached() -> io::Error {
    io::Error::other("最多暂存 200 个文件与文件夹，请先移除一些项目")
}

fn local_path_required() -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidInput,
        "请选择本机磁盘中的文件或文件夹",
    )
}

#[cfg(test)]
mod tests {
    use std::fs;

    use super::*;

    #[test]
    fn retains_references_but_never_opens_a_replacement() {
        let directory = tempfile::tempdir().unwrap();
        let path = directory.path().join("日程.txt");
        let moved = directory.path().join("moved.txt");
        fs::write(&path, b"original").unwrap();

        let mut shelf = Shelf::load(directory.path()).unwrap();
        shelf.add([path.clone()]).unwrap();
        let mut shelf = Shelf::load(directory.path()).unwrap();
        let id = shelf.entries[0].id;
        shelf
            .action(id, |path| {
                assert_eq!(fs::read(path)?, b"original");
                Ok(())
            })
            .unwrap();

        fs::rename(&path, &moved).unwrap();
        fs::write(&path, b"replacement").unwrap();
        assert!(shelf.action(id, |_| panic!("opened replacement")).is_err());
        assert!(shelf.unavailable.contains(&id));

        shelf.add([path.clone()]).unwrap();
        assert_eq!(shelf.entries[0].id, id);
        assert!(shelf.unavailable.is_empty());
        shelf.clear().unwrap();
        assert!(Shelf::load(directory.path()).unwrap().entries.is_empty());
        assert_eq!(fs::read(path).unwrap(), b"replacement");
        assert_eq!(fs::read(moved).unwrap(), b"original");
    }

    #[test]
    fn accepts_only_absolute_paths_with_a_drive_letter() {
        for path in [r"C:\资料\日程.txt", r"\\?\d:\资料", r"c:/materials"] {
            assert!(disk_letter(Path::new(path)).is_ok(), "{path}");
        }
        for path in [
            r"C:relative.txt",
            r"C:\data.txt:hidden",
            r"C:\data\..\file.txt",
            r"\rooted.txt",
            r"\\server\share\file.txt",
            r"\\?\UNC\server\share\file.txt",
            r"\\?\GLOBALROOT\Device\HarddiskVolume1\file.txt",
            r"\\.\C:\file.txt",
        ] {
            assert!(disk_letter(Path::new(path)).is_err(), "{path}");
        }
    }
}

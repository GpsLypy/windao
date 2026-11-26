use super::{Account, Key, Summary};
use serde::{Deserialize, Serialize};
use std::{fs::File, path::Path};
use uuid::Uuid;

#[derive(Default, Serialize, Deserialize, Clone)]
pub struct Mailboxes {
    #[serde(default)]
    pub sync_accounts: Vec<Uuid>,
    #[serde(default)]
    pub reveal_seconds: u16,
    pub accounts: Vec<Account>,
    pub marks: Vec<Mark>,
}
#[derive(Clone, Serialize, Deserialize)]
pub struct Mark {
    pub account: Uuid,
    pub summary: Summary,
    pub trash: bool,
    pub unread: bool,
}

impl Mailboxes {
    pub fn open(dir: &Path) -> Result<Self, String> {
        let path = dir.join("mailboxes.json");
        match File::open(path) {
            Ok(file) => {
                if file.metadata().map_err(|_| "无法读取邮箱设置。")?.len() > 8 * 1024 * 1024
                {
                    return Err("邮箱设置文件过大。".into());
                }
                serde_json::from_reader(file).map_err(|_| "邮箱设置文件损坏。".into())
            }
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Self::default()),
            Err(_) => Err("无法读取邮箱设置。".into()),
        }
    }
    pub fn save(&self, dir: &Path) -> Result<(), String> {
        let mut tmp = tempfile::NamedTempFile::new_in(dir).map_err(|_| "无法创建邮箱设置文件。")?;
        serde_json::to_writer(&mut tmp, self).map_err(|_| "无法保存邮箱设置。")?;
        if tmp
            .as_file()
            .metadata()
            .map_err(|_| "无法读取设置大小。")?
            .len()
            > 8 * 1024 * 1024
        {
            return Err("邮箱设置已满，请移除不再使用的账户。".into());
        }
        tmp.as_file().sync_all().map_err(|_| "保存邮箱设置失败。")?;
        tmp.persist(dir.join("mailboxes.json"))
            .map_err(|_| "替换邮箱设置失败。")?;
        Ok(())
    }
    pub fn mark(&self, id: Uuid, key: Key) -> Option<&Mark> {
        self.marks
            .iter()
            .find(|r| r.account == id && r.summary.key == key)
    }
    pub fn set_mark(&mut self, id: Uuid, row: &Summary, unread: bool, trash: bool) {
        self.marks
            .retain(|r| !(r.account == id && r.summary.key == row.key));
        self.marks.push(Mark {
            account: id,
            summary: row.clone(),
            unread,
            trash,
        })
    }
}

use super::{Provider, Reading, Result};
use serde::{Deserialize, Serialize};
use std::{
    fs,
    io::{Read, Write},
    path::{Path, PathBuf},
};
use uuid::Uuid;

#[derive(Serialize, Deserialize, Clone)]
pub(crate) struct Profile {
    pub id: Uuid,
    pub provider: Provider,
    pub name: String,
    pub color: usize,
    pub endpoint: String,
    #[serde(default)]
    pub balance_field: String,
    #[serde(default)]
    pub unit: String,
    pub report: Option<Reading>,
    pub observed: Option<i64>,
    pub file: Option<PathBuf>,
}
#[derive(Default, Serialize, Deserialize, Clone)]
pub(crate) struct Profiles {
    pub accounts: Vec<Profile>,
    pub pinned: Option<Uuid>,
}

const INDEX: &str = "ai-accounts.json";
impl Profiles {
    pub fn open(dir: &Path) -> Result<Self> {
        let file = match fs::File::open(dir.join(INDEX)) {
            Ok(f) => f,
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Self::default()),
            Err(_) => return Err("读取 AI 账户列表失败。"),
        };
        let mut bytes = Vec::new();
        file.take(1_048_577)
            .read_to_end(&mut bytes)
            .map_err(|_| "读取 AI 账户列表失败。")?;
        if bytes.len() > 1_048_576 {
            return Err("AI 账户列表过大。");
        }
        let result: Self = serde_json::from_slice(&bytes).map_err(|_| "AI 账户列表格式无效。")?;
        if result.accounts.len() > 32 {
            return Err("AI 账户数量超过上限。");
        }
        Ok(result)
    }
    pub fn save(&self, dir: &Path) -> Result<()> {
        let save = || -> std::io::Result<()> {
            let mut file = tempfile::NamedTempFile::new_in(dir)?;
            serde_json::to_writer(&mut file, self)?;
            file.flush()?;
            file.as_file().sync_all()?;
            file.persist(dir.join(INDEX))?;
            Ok(())
        };
        save().map_err(|_| "保存 AI 账户列表失败，请检查存储空间。")
    }
    pub fn add(
        &mut self,
        provider: Provider,
        name: &str,
        color: usize,
        endpoint: &str,
    ) -> Result<Uuid> {
        if self.accounts.len() >= 32 {
            return Err("最多可添加 32 个账户。");
        }
        let name = name.trim();
        if name.chars().count() > 36 || name.chars().any(char::is_control) {
            return Err("账户名称最多 36 字。");
        }
        let endpoint = if provider == Provider::Relay {
            super::relay::base(endpoint)?.to_string()
        } else {
            String::new()
        };
        let id = Uuid::new_v4();
        self.accounts.push(Profile {
            id,
            provider,
            name: if name.is_empty() {
                provider.title().into()
            } else {
                name.into()
            },
            color,
            endpoint,
            balance_field: String::new(),
            unit: String::new(),
            report: None,
            observed: None,
            file: None,
        });
        Ok(id)
    }
    pub fn pinned(&self) -> Option<&Profile> {
        self.accounts.iter().find(|p| Some(p.id) == self.pinned)
    }
}
impl Profile {
    #[cfg(windows)]
    pub fn directory(&self, dir: &Path) -> PathBuf {
        dir.join("ai-credentials").join(self.id.to_string())
    }
    pub fn caption(&self) -> Option<String> {
        let v = match self.report.as_ref()? {
            Reading::Money { balances, .. } => {
                let b = balances.first()?;
                format!("{} {}", b.available, b.currency)
            }
            Reading::Plan(rows) => {
                let q = rows.first()?;
                if q.resets_at
                    .is_some_and(|t| t <= chrono::Utc::now().timestamp())
                {
                    return None;
                }
                format!("余 {:.0}%", 100. - q.used_percent)
            }
            Reading::Units { summary, .. } => summary.clone(),
        };
        let stale = self
            .observed
            .is_none_or(|n| chrono::Utc::now().timestamp().saturating_sub(n) > 3600);
        Some(format!(
            "{}{} {}",
            if stale { "上次 " } else { "" },
            self.name.chars().take(6).collect::<String>(),
            v
        ))
    }
}
pub(crate) fn read_report(provider: Provider, path: &Path) -> Result<Reading> {
    if !path.is_absolute()
        || !path
            .extension()
            .is_some_and(|x| x.eq_ignore_ascii_case("json"))
    {
        return Err("请选择用量 JSON 文件。");
    }
    let file = fs::File::open(path).map_err(|_| "无法打开已选用量文件。")?;
    if !file
        .metadata()
        .map_err(|_| "无法读取用量文件信息。")?
        .is_file()
    {
        return Err("请选择普通 JSON 文件。");
    }
    let mut body = String::new();
    file.take(262145)
        .read_to_string(&mut body)
        .map_err(|_| "无法读取用量 JSON。")?;
    super::response(provider, 200, &body)
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn aliases_isolate_multiple_accounts_and_pin_survives_restart() {
        let dir = tempfile::tempdir().unwrap();
        let mut p = Profiles::default();
        let a = p.add(Provider::KimiCode, "个人", 0, "").unwrap();
        let b = p.add(Provider::KimiCode, "工作", 1, "").unwrap();
        assert_ne!(a, b);
        p.pinned = Some(b);
        let file = dir.path().join("report.json");
        fs::write(&file, r#"{"usages":{"limit_5h":{"used_ratio":0.25}}}"#).unwrap();
        p.accounts[1].report = Some(read_report(Provider::KimiCode, &file).unwrap());
        p.accounts[1].observed = Some(chrono::Utc::now().timestamp());
        p.save(dir.path()).unwrap();
        let p = Profiles::open(dir.path()).unwrap();
        assert_eq!(
            p.pinned().unwrap().caption().as_deref(),
            Some("工作 余 75%")
        );
        assert!(p.accounts[0].report.is_none());
    }
}

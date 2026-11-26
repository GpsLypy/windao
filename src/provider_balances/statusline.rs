//! Claude Code sends its documented status line JSON to this command on stdin.
use super::{Provider, Reading, response};
use std::{
    io::{Read, Write},
    path::Path,
};
use uuid::Uuid;

pub(crate) fn receive(
    input: impl Read,
    output: impl Write,
    dir: &Path,
    id: Uuid,
) -> Result<(), String> {
    let mut raw = String::new();
    input
        .take(262145)
        .read_to_string(&mut raw)
        .map_err(|_| "读取状态栏失败。")?;
    let mut output = output;
    if raw.len() > 262144 {
        return Err("状态栏输入过大。".into());
    }
    let v: serde_json::Value = serde_json::from_str(&raw).map_err(|_| "状态栏 JSON 无效。")?;
    let fields = &v["rate_limits"];
    let mut limits = serde_json::Map::new();
    for key in ["five_hour", "seven_day"] {
        let row = &fields[key];
        if let Some(used) = row["used_percentage"]
            .as_f64()
            .filter(|n| (0.0..=100.0).contains(n))
        {
            limits.insert(
                key.into(),
                serde_json::json!({"used_percentage":used,"resets_at":row["resets_at"].as_i64()}),
            );
        }
    }
    if limits.is_empty() {
        writeln!(output, "Claude Code").map_err(|_| "状态栏输出失败。")?;
        return Ok(());
    }
    let data = serde_json::json!({"rate_limits":limits});
    let Reading::Plan(quota) = response(Provider::ClaudeStatus, 200, &data.to_string())? else {
        return Err("状态栏用量格式无效。".into());
    };
    let config = super::profiles::Profiles::open(dir)?;
    if !config
        .accounts
        .iter()
        .any(|a| a.id == id && a.provider == Provider::ClaudeStatus)
    {
        return Err("此用量账户已移除。".into());
    }
    let readings = dir.join("ai-statusline");
    std::fs::create_dir_all(&readings).map_err(|_| "无法创建状态栏目录。")?;
    let mut tmp = tempfile::NamedTempFile::new_in(&readings).map_err(|_| "无法写入状态栏用量。")?;
    serde_json::to_writer(&mut tmp, &data).map_err(|_| "无法保存用量。")?;
    tmp.as_file().sync_all().map_err(|_| "无法保存用量。")?;
    tmp.persist(readings.join(format!("{id}.json")))
        .map_err(|_| "无法保存用量。")?;
    let text = quota
        .iter()
        .map(|q| format!("{} {:.0}%", q.label, q.used_percent))
        .collect::<Vec<_>>()
        .join(" · ");
    writeln!(output, "Claude Code · {text}").map_err(|_| "状态栏输出失败。".into())
}
#[cfg(windows)]
pub(crate) fn command(id: Uuid) -> Result<String, String> {
    use base64::{Engine, engine::general_purpose::STANDARD};
    let exe = std::env::current_exe().map_err(|_| "找不到程序路径。")?;
    let literal = exe
        .to_str()
        .ok_or("程序路径无法用于状态栏。")?
        .replace('\'', "''");
    // Keep the executable path out of the caller's shell syntax (Bash and PowerShell are supported).
    let script = format!(
        "[Console]::InputEncoding=[System.Text.UTF8Encoding]::new($false);$OutputEncoding=[Console]::InputEncoding;[Console]::In.ReadToEnd() | & '{literal}' --claude-statusline {id}"
    );
    let bytes: Vec<_> = script.encode_utf16().flat_map(u16::to_le_bytes).collect();
    Ok(format!(
        "powershell.exe -NoProfile -NonInteractive -EncodedCommand {}",
        STANDARD.encode(bytes)
    ))
}
#[cfg(windows)]
pub(crate) fn configure(id: Uuid, enable: bool) -> Result<(), String> {
    let config_dir = std::env::var_os("CLAUDE_CONFIG_DIR")
        .map(std::path::PathBuf::from)
        .or_else(|| std::env::home_dir().map(|d| d.join(".claude")))
        .filter(|p| p.is_absolute())
        .ok_or("找不到 Claude Code 配置目录。")?;
    let path = config_dir.join("settings.json");
    let old = match std::fs::read(&path) {
        Ok(bytes) if bytes.len() <= 2 * 1024 * 1024 => Some(bytes),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => None,
        _ => return Err("无法读取 Claude Code 设置。".into()),
    };
    let mut settings: serde_json::Value = match old.as_deref() {
        Some(bytes) => serde_json::from_slice(bytes).map_err(|_| "Claude 设置不是有效 JSON。")?,
        None => serde_json::json!({}),
    };
    let obj = settings.as_object_mut().ok_or("Claude 设置格式无效。")?;
    let cmd = command(id)?;
    let current = obj.get("statusLine").filter(|v| !v.is_null());
    if enable {
        if let Some(existing) = current {
            if existing["command"].as_str() == Some(&cmd) {
                return Ok(());
            }
            return Err("已有状态栏。可复制用量命令，在原状态栏脚本中传入同一份标准输入。".into());
        }
        obj.insert(
            "statusLine".into(),
            serde_json::json!({"type":"command","command":cmd}),
        );
    } else if current.is_some_and(|c| c["command"].as_str() == Some(&cmd)) {
        obj.remove("statusLine");
    } else {
        return Ok(());
    }
    std::fs::create_dir_all(&config_dir).map_err(|_| "无法创建 Claude 设置目录。")?;
    let mut file =
        tempfile::NamedTempFile::new_in(config_dir).map_err(|_| "无法写入 Claude 设置。")?;
    serde_json::to_writer_pretty(&mut file, &settings).map_err(|_| "无法保存 Claude 设置。")?;
    file.as_file()
        .sync_all()
        .map_err(|_| "无法保存 Claude 设置。")?;
    if std::fs::read(&path).ok() != old {
        return Err("Claude 设置刚刚发生变化，请重试。".into());
    }
    if old.is_none() {
        file.persist_noclobber(path)
    } else {
        file.persist(path)
    }
    .map_err(|_| "无法保存 Claude 设置。")?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn persists_only_published_limit_fields() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let mut ps = super::super::profiles::Profiles::default();
        let id = ps.add(Provider::ClaudeStatus, "主账号", 0, "").unwrap();
        ps.save(dir).unwrap();
        let mut out = vec![];
        receive(br#"{"session_id":"omit-this","workspace":{"current_dir":"private"},"rate_limits":{"five_hour":{"used_percentage":12,"resets_at":1880000000,"something":"private"}}}"#.as_slice(),&mut out,dir,id).unwrap();
        let file =
            std::fs::read_to_string(dir.join("ai-statusline").join(format!("{id}.json"))).unwrap();
        assert!(!file.contains("private") && !file.contains("omit-this"));
        assert!(String::from_utf8(out).unwrap().contains("12%"));
    }
}

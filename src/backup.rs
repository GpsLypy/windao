use super::{Result, Store, Workspace, read_json, workspace_tempfile};
use std::path::Path;

pub fn export(store: &Store, path: &Path) -> Result<()> {
    let mut backup = store.state().clone();
    for source in &mut backup.calendar_sources {
        source.protected_connection = None;
        source.error = None;
    }
    let temp =
        workspace_tempfile(path, &backup).map_err(|error| format!("无法写入备份（{error}）"))?;
    // A backup must never replace an active workspace or an existing backup.
    temp.persist_noclobber(path)
        .map(|_| ())
        .map_err(|error| format!("无法保存备份，请选择新文件名（{}）", error.error))
}

pub fn read(path: &Path) -> Result<Workspace> {
    let workspace =
        read_json::<Workspace>(path).map_err(|error| format!("无法读取备份（{error}）"))?;
    workspace.validate()?;
    Ok(workspace)
}

pub fn restore(store: &mut Store, mut backup: Workspace) -> Result<()> {
    backup.settings.display_device = store.state().settings.display_device.clone();
    super::calendar_sources::label_cached_sources(&mut backup.calendar_sources, &backup.events);
    for source in &mut backup.calendar_sources {
        source.protected_connection = store
            .state()
            .calendar_sources
            .iter()
            .find(|local| local.id == source.id && local.kind == source.kind)
            .and_then(|local| local.protected_connection.clone());
        source.error = None;
    }
    let elapsed = super::Schedule::from_workspace(&backup)
        .elapsed_keys(chrono::Utc::now().timestamp_millis());
    super::reminders::mark_delivered(&mut backup.delivered_reminder_ids, elapsed);
    store.update(move |workspace| {
        backup.revision = workspace.revision;
        *workspace = backup;
        Ok(())
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn backup_cannot_replace_a_file_and_recovery_keeps_local_display() {
        let folder = tempdir().unwrap();
        let path = folder.path().join("backup.json");
        let mut store = Store::open(folder.path().join("profile")).unwrap();
        store
            .update(|state| {
                state.notes = "备份之前".into();
                state.settings.display_device = Some("screen-a".into());
                Ok(())
            })
            .unwrap();
        export(&store, &path).unwrap();
        store
            .update(|state| {
                state.notes = "之后的内容".into();
                state.settings.display_device = Some("screen-b".into());
                Ok(())
            })
            .unwrap();
        assert!(export(&store, &path).is_err());

        let backup = read(&path).unwrap();
        restore(&mut store, backup).unwrap();
        assert_eq!(store.state().notes, "备份之前");
        assert_eq!(
            store.state().settings.display_device.as_deref(),
            Some("screen-b")
        );
    }
}

//! Native window + worker qualification using disposable fixture files only.
use super::*;
use std::time::Duration;

pub(in crate::desktop) fn check(handle: &rune::AppHandle<State>) -> Result<serde_json::Value> {
    let owner = crate::config::app_root()?;
    #[cfg(target_os = "linux")]
    {
        let data = PathBuf::from(
            std::env::var_os("XDG_DATA_HOME")
                .context("--trash-check requires isolated XDG_DATA_HOME")?,
        );
        anyhow::ensure!(
            data.is_absolute() && data.starts_with(&owner),
            "--trash-check XDG_DATA_HOME must be inside isolated RODTTY_HOME"
        );
    }
    #[cfg(not(target_os = "linux"))]
    anyhow::ensure!(
        std::env::var_os("RODTTY_DISPOSABLE_TRASH_ACCOUNT").as_deref()
            == Some(std::ffi::OsStr::new("1")),
        "--trash-check requires a disposable OS account and RODTTY_DISPOSABLE_TRASH_ACCOUNT=1 on this platform"
    );
    let fixture = tempfile::Builder::new().prefix("native-trash-").tempdir()?;
    let folder = fixture.path().join("work");
    std::fs::create_dir(&folder)?;
    let first = folder.join("first.txt");
    let second = folder.join("second.txt");
    let unrelated = fixture.path().join("other.txt");
    let saved = fixture.path().join("preserved.txt");
    for path in [&first, &second, &unrelated] {
        std::fs::write(path, "saved source")?;
    }
    let (tx, rx) = std::sync::mpsc::channel();
    let paths = [first.clone(), second.clone(), unrelated.clone()];
    handle.update(move |s, _| {
        let _ = tx.send(s.config.explorer.confirm_deletion);
        s.save_config(|c| c.explorer.confirm_deletion = true);
        s.workspace.tabs.clear();
        for path in paths {
            s.open_file(path);
        }
    });
    let original = rx.recv_timeout(Duration::from_secs(10))?;
    wait_app(handle, |s| {
        s.workspace.tabs.len() == 3 && !s.file_io_pending()
    })
    .context("Open native Trash fixtures")?;
    let root = fixture.path().to_owned();
    handle.update(move |s, _| {
        for tab in &mut s.workspace.tabs {
            tab.edit.replace_text(format!(
                "unsaved {}",
                tab.document.path.file_name().unwrap().to_string_lossy()
            ));
            tab.revision += 1;
        }
        s.workspace.layout.explorer = true;
        s.navigate(root);
    });
    let selected = folder.clone();
    wait_app(handle, move |s| {
        s.workspace.entries.iter().any(|e| e.path == selected)
    })
    .context("Scan native Trash folder")?;
    let selected = folder.clone();
    handle.update(move |s, tx| {
        s.workspace.tree.select(&selected.to_string_lossy());
        s.dispatch(AppAction::TrashFile, tx);
    });
    wait_app(handle, |s| {
        matches!(s.pending, Some(Pending::Removal(RemovalPrompt::Confirm)))
    })
    .context("Show native Trash confirmation")?;
    handle.update(|s, tx| s.cancel_removal(tx));
    wait_app(handle, |s| s.workspace.removal.is_none())
        .context("Cancel native Trash confirmation")?;
    anyhow::ensure!(folder.exists(), "cancel removed fixture source");
    handle.update(|s, tx| s.dispatch(AppAction::TrashFile, tx));
    wait_app(handle, |s| {
        matches!(s.pending, Some(Pending::Removal(RemovalPrompt::Confirm)))
    })
    .context("Show native Trash confirmation again")?;
    handle.update(|s, tx| s.advance_removal(false, tx));
    wait_app(handle, |s| {
        matches!(s.pending, Some(Pending::Removal(RemovalPrompt::Dirty(_))))
    })
    .context("Show dirty native file decision")?;
    let preserved = saved.clone();
    handle.update(move |s, tx| {
        let Some(Pending::Removal(RemovalPrompt::Dirty(id))) = s.pending else {
            return;
        };
        s.begin_removal_save_as(id, tx);
        s.workspace
            .operation_destination
            .replace_text(preserved.to_string_lossy());
        s.submit_removal_save_as(tx);
    });
    wait_app(handle, |s| {
        !s.workspace.operation_busy
            && matches!(s.pending, Some(Pending::Removal(RemovalPrompt::Dirty(_))))
    })
    .context("Save As then show next dirty native file")?;
    handle.update(|s, tx| {
        if let Some(Pending::Removal(RemovalPrompt::Dirty(id))) = s.pending {
            s.discard_removal_buffer(id, tx);
        }
    });
    wait_app(handle, |s| {
        !s.workspace.operation_busy && s.message.starts_with("Moved to Trash")
    })
    .context("Finish native host Trash removal")?;
    anyhow::ensure!(!folder.exists(), "Trash did not remove fixture folder");
    anyhow::ensure!(
        std::fs::read_to_string(&saved)?.starts_with("unsaved "),
        "Save As did not preserve fixture edits"
    );
    let (tx, rx) = std::sync::mpsc::channel();
    handle.update(move |s, _| {
        let intact = s
            .workspace
            .tabs
            .iter()
            .any(|t| t.document.path == unrelated && t.dirty())
            && s.workspace.tabs.len() == 2;
        let _ = tx.send(intact);
        s.workspace.tabs.clear();
        s.save_config(|c| c.explorer.confirm_deletion = original);
        s.persist_file_references();
    });
    anyhow::ensure!(
        rx.recv_timeout(Duration::from_secs(10))?,
        "unrelated editor work changed during deletion"
    );
    Ok(
        serde_json::json!({"cancel_retained_files_and_buffers":true,"dirty_folder_save_as_preserved":true,"discard_applied_after_success":true,"unrelated_dirty_buffer_preserved":true,"host_trash_used":true,"isolated_fixture_only":true}),
    )
}

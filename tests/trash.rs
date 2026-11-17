use rodtty::files::removal::{RemovalOutcome, RemovalPlan};
use std::fs;
#[cfg(target_os = "linux")]
use std::process::Command;

// Run native Trash in a subprocess with its own data home. The test process never
// changes global environment variables or reads the owner's Trash.
#[cfg(target_os = "linux")]
#[test]
fn host_trash_preserves_items_and_metadata_in_an_isolated_data_home() {
    let root = tempfile::tempdir().unwrap();
    let test = "host_trash_preserves_items_and_metadata_in_an_isolated_data_home";
    if std::env::var_os("RODTTY_TRASH_CHILD").is_none() {
        let output = Command::new(std::env::current_exe().unwrap())
            .args(["--exact", test, "--nocapture"])
            .env("RODTTY_TRASH_CHILD", root.path())
            .env("XDG_DATA_HOME", root.path().join("data"))
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}\n{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    let root = std::path::PathBuf::from(std::env::var_os("RODTTY_TRASH_CHILD").unwrap());
    let source = root.join("notes.txt");
    fs::write(&source, "recoverable work").unwrap();
    let plan = RemovalPlan::prepare(source.clone()).unwrap();
    assert!(matches!(
        plan.trash().unwrap(),
        RemovalOutcome::Removed {
            permanent: false,
            ..
        }
    ));
    assert!(!source.exists());
    let trash = root.join("data/Trash");
    let items: Vec<_> = fs::read_dir(trash.join("files"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    assert_eq!(items.len(), 1);
    assert_eq!(fs::read_to_string(&items[0]).unwrap(), "recoverable work");
    let info = fs::read_dir(trash.join("info"))
        .unwrap()
        .next()
        .unwrap()
        .unwrap()
        .path();
    let metadata = fs::read_to_string(info).unwrap();
    assert!(metadata.starts_with("[Trash Info]\nPath="), "{metadata}");
    assert!(metadata.contains("notes.txt\nDeletionDate="), "{metadata}");

    // Duplicate basenames get distinct recoverable items rather than replacement.
    fs::write(&source, "second recoverable version").unwrap();
    RemovalPlan::prepare(source).unwrap().trash().unwrap();
    let items: Vec<_> = fs::read_dir(trash.join("files"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .collect();
    let contents: Vec<_> = items
        .iter()
        .map(|p| fs::read_to_string(p).unwrap())
        .collect();
    assert!(contents.contains(&"recoverable work".to_string()));
    assert!(contents.contains(&"second recoverable version".to_string()));
    assert_eq!(contents.len(), 2);

    let folder = root.join("folder");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("read-only.txt"), "nested recoverable work").unwrap();
    use std::os::unix::fs::PermissionsExt;
    fs::set_permissions(
        folder.join("read-only.txt"),
        fs::Permissions::from_mode(0o400),
    )
    .unwrap();
    fs::set_permissions(&folder, fs::Permissions::from_mode(0o500)).unwrap();
    match RemovalPlan::prepare(folder.clone())
        .unwrap()
        .trash()
        .unwrap()
    {
        RemovalOutcome::Removed {
            permanent: false, ..
        } => {}
        RemovalOutcome::TrashFailed { error, .. } => {
            assert!(!error.is_empty());
            assert_eq!(
                fs::read_to_string(folder.join("read-only.txt")).unwrap(),
                "nested recoverable work"
            );
            // Some Unix filesystems require directory write permission to update
            // its parent link during a move. Failure must retain the source.
            fs::set_permissions(&folder, fs::Permissions::from_mode(0o700)).unwrap();
            assert!(matches!(
                RemovalPlan::prepare(folder.clone())
                    .unwrap()
                    .trash()
                    .unwrap(),
                RemovalOutcome::Removed {
                    permanent: false,
                    ..
                }
            ));
        }
        other => panic!("Trash must never permanently delete: {other:?}"),
    }
    assert!(!folder.exists());
    let recycled_folder = fs::read_dir(trash.join("files"))
        .unwrap()
        .map(|e| e.unwrap().path())
        .find(|p| p.is_dir())
        .unwrap();
    assert_eq!(
        fs::read_to_string(recycled_folder.join("read-only.txt")).unwrap(),
        "nested recoverable work"
    );
    assert!(matches!(
        fs::metadata(&recycled_folder).unwrap().permissions().mode() & 0o777,
        0o500 | 0o700
    ));
    // Only this fixture-owned directory needs permission restoration for TempDir cleanup.
    fs::set_permissions(&recycled_folder, fs::Permissions::from_mode(0o700)).unwrap();

    let referent = root.join("referent.txt");
    fs::write(&referent, "link target retained").unwrap();
    let link = root.join("link.txt");
    std::os::unix::fs::symlink(&referent, &link).unwrap();
    RemovalPlan::prepare(link.clone()).unwrap().trash().unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(
        fs::read_to_string(&referent).unwrap(),
        "link target retained"
    );
    assert_eq!(fs::read_dir(trash.join("files")).unwrap().count(), 4);
    assert_eq!(fs::read_dir(trash.join("info")).unwrap().count(), 4);
}

#[test]
fn changed_removal_version_retains_files_and_explicit_permanent_removal_checks_again() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("notes.txt");
    fs::write(&source, "first version").unwrap();
    let stale = RemovalPlan::prepare(source.clone()).unwrap();
    fs::write(&source, "external changes").unwrap();
    assert!(stale.trash().unwrap_err().to_string().contains("changed"));
    assert_eq!(fs::read_to_string(&source).unwrap(), "external changes");
    let stale = RemovalPlan::prepare(source.clone()).unwrap();
    fs::write(&source, "newer external changes").unwrap();
    assert!(stale.permanently_delete().is_err());
    assert_eq!(
        fs::read_to_string(&source).unwrap(),
        "newer external changes"
    );
    assert!(matches!(
        RemovalPlan::prepare(source.clone())
            .unwrap()
            .permanently_delete()
            .unwrap(),
        RemovalOutcome::Removed {
            permanent: true,
            ..
        }
    ));
    assert!(!source.exists());
}

#[cfg(unix)]
#[test]
fn permanently_removing_a_directory_link_retains_the_link_target() {
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("original");
    fs::create_dir(&folder).unwrap();
    fs::write(folder.join("notes.txt"), "retained").unwrap();
    let link = root.path().join("link");
    std::os::unix::fs::symlink(&folder, &link).unwrap();
    RemovalPlan::prepare(link.clone())
        .unwrap()
        .permanently_delete()
        .unwrap();
    assert!(fs::symlink_metadata(&link).is_err());
    assert_eq!(
        fs::read_to_string(folder.join("notes.txt")).unwrap(),
        "retained"
    );
}

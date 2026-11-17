use rodtty::files::operations::{
    Collision, CollisionChoice, FileOperation, OperationOutcome, OperationRequest,
};
use std::{fs, path::Path};

fn copy(source: &Path, target: &Path) -> OperationRequest {
    OperationRequest {
        operation: FileOperation::Copy {
            source: source.to_owned(),
        },
        target: target.to_owned(),
    }
}
fn collision(request: OperationRequest) -> Collision {
    match request.execute(None).unwrap() {
        OperationOutcome::Collision(collision) => collision,
        other => panic!("expected a collision, got {other:?}"),
    }
}

#[cfg(unix)]
#[test]
fn replacing_a_destination_link_replaces_the_link_without_touching_its_referent() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let target = root.path().join("link.txt");
    fs::write(&source, "source contents").unwrap();
    std::os::unix::fs::symlink(&source, &target).unwrap();
    let request = copy(&source, &target);
    let ticket = collision(request.clone());
    assert!(matches!(
        request
            .execute(Some((ticket, CollisionChoice::Replace)))
            .unwrap(),
        OperationOutcome::Completed { .. }
    ));
    assert!(!fs::symlink_metadata(&target).unwrap().is_symlink());
    assert_eq!(fs::read_to_string(&target).unwrap(), "source contents");
    assert_eq!(fs::read_to_string(&source).unwrap(), "source contents");
}

#[test]
fn a_changed_destination_requires_a_new_decision_and_cancel_preserves_its_new_contents() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let target = root.path().join("target.txt");
    fs::write(&source, "new source").unwrap();
    fs::write(&target, "first version").unwrap();
    let request = copy(&source, &target);
    let ticket = collision(request.clone());
    fs::write(&target, "another process edited this").unwrap();
    let renewed = match request
        .clone()
        .execute(Some((ticket, CollisionChoice::Replace)))
        .unwrap()
    {
        OperationOutcome::Collision(ticket) => ticket,
        other => panic!("destination changed: expected a new choice, got {other:?}"),
    };
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "another process edited this"
    );
    assert!(matches!(
        request
            .execute(Some((renewed, CollisionChoice::Cancel)))
            .unwrap(),
        OperationOutcome::Cancelled
    ));
    assert_eq!(
        fs::read_to_string(&target).unwrap(),
        "another process edited this"
    );
}

#[test]
fn copying_a_folder_keeps_both_or_replaces_the_whole_target_instead_of_merging() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("project.v2");
    fs::create_dir_all(source.join("nested")).unwrap();
    fs::write(source.join("nested/new.txt"), "nested source").unwrap();
    fs::create_dir(&target).unwrap();
    fs::write(target.join("old.txt"), "existing target").unwrap();
    fs::create_dir(root.path().join("project.v2 copy")).unwrap();
    let request = copy(&source, &target);
    let ticket = collision(request.clone());
    let copied = match request
        .clone()
        .execute(Some((ticket, CollisionChoice::KeepBoth)))
        .unwrap()
    {
        OperationOutcome::Completed { path, .. } => path,
        other => panic!("expected copy, got {other:?}"),
    };
    assert_eq!(copied, root.path().join("project.v2 copy 2"));
    assert_eq!(
        fs::read_to_string(copied.join("nested/new.txt")).unwrap(),
        "nested source"
    );
    assert_eq!(
        fs::read_to_string(target.join("old.txt")).unwrap(),
        "existing target"
    );
    let ticket = collision(request.clone());
    assert!(matches!(
        request
            .execute(Some((ticket, CollisionChoice::Replace)))
            .unwrap(),
        OperationOutcome::Completed { .. }
    ));
    assert_eq!(
        fs::read_to_string(target.join("nested/new.txt")).unwrap(),
        "nested source"
    );
    assert!(!target.join("old.txt").exists());
}

#[cfg(unix)]
#[test]
fn folder_copies_preserve_relative_dangling_and_cyclic_links_without_traversal() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("copy");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file.txt"), "original").unwrap();
    for (name, link) in [
        ("relative", "file.txt"),
        ("dangling", "missing.txt"),
        ("cycle", "."),
    ] {
        std::os::unix::fs::symlink(link, source.join(name)).unwrap();
    }
    assert!(matches!(
        copy(&source, &target).execute(None).unwrap(),
        OperationOutcome::Completed { .. }
    ));
    assert_eq!(
        fs::read_link(target.join("relative")).unwrap(),
        Path::new("file.txt")
    );
    assert_eq!(
        fs::read_link(target.join("dangling")).unwrap(),
        Path::new("missing.txt")
    );
    assert_eq!(fs::read_link(target.join("cycle")).unwrap(), Path::new("."));
    assert_eq!(
        fs::read_to_string(target.join("relative")).unwrap(),
        "original"
    );
}

#[cfg(unix)]
#[test]
fn a_copy_failure_keeps_the_existing_destination_and_cleans_prepared_items() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    fs::create_dir(&source).unwrap();
    fs::write(source.join("ordinary.txt"), "ordinary file").unwrap();
    use std::os::unix::fs::PermissionsExt;
    let create_read_only = |number| {
        let path = source.join(format!("read-only-{number}"));
        fs::create_dir(&path).unwrap();
        fs::write(path.join("file.txt"), "read-only nested file").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o555)).unwrap();
        path
    };
    let mut read_only_directories = vec![create_read_only(0)];
    let _socket = std::os::unix::net::UnixListener::bind(source.join("socket")).unwrap();
    // Cover insertion and reverse insertion enumeration, then verify the fixture
    // exercises cleanup after a completed subtree rather than failing immediately.
    let has_completed_subtree = || {
        fs::read_dir(&source)
            .unwrap()
            .map(|entry| entry.unwrap().file_name())
            .take_while(|name| name != "socket")
            .any(|name| name.to_string_lossy().starts_with("read-only-"))
    };
    if !has_completed_subtree() {
        read_only_directories.push(create_read_only(1));
    }
    assert!(
        has_completed_subtree(),
        "fixture needs a read-only directory before the socket"
    );
    let referent = root.path().join("referent");
    fs::create_dir(&referent).unwrap();
    fs::write(referent.join("keep.txt"), "unrelated contents").unwrap();
    fs::set_permissions(&referent, fs::Permissions::from_mode(0o555)).unwrap();
    // Include a symlink in every completed subtree so cleanup must not chmod its referent.
    for read_only in &read_only_directories {
        fs::set_permissions(read_only, fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink(&referent, read_only.join("external-link")).unwrap();
        fs::set_permissions(read_only, fs::Permissions::from_mode(0o555)).unwrap();
    }
    fs::create_dir(&target).unwrap();
    fs::write(target.join("precious.txt"), "keep this").unwrap();
    let request = copy(&source, &target);
    let ticket = collision(request.clone());
    assert!(
        request
            .execute(Some((ticket, CollisionChoice::Replace)))
            .unwrap_err()
            .to_string()
            .contains("special file")
    );
    assert_eq!(
        fs::read_to_string(target.join("precious.txt")).unwrap(),
        "keep this"
    );
    assert!(!target.join("ordinary.txt").exists());
    let mut names = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    for read_only in &read_only_directories {
        assert_eq!(
            fs::metadata(read_only).unwrap().permissions().mode() & 0o777,
            0o555
        );
        assert_eq!(
            fs::read_to_string(read_only.join("file.txt")).unwrap(),
            "read-only nested file"
        );
        fs::set_permissions(read_only, fs::Permissions::from_mode(0o755)).unwrap();
    }
    let referent_mode = fs::metadata(&referent).unwrap().permissions().mode() & 0o777;
    fs::set_permissions(&referent, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(referent_mode, 0o555);
    assert_eq!(
        fs::read_to_string(referent.join("keep.txt")).unwrap(),
        "unrelated contents"
    );
    assert_eq!(names, ["referent", "source", "target"]);
}

#[cfg(unix)]
#[test]
fn replacing_a_read_only_folder_cleans_its_backup_without_changing_published_permissions() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("target");
    let referent = root.path().join("referent");
    for directory in [&source, &target, &referent] {
        fs::create_dir_all(directory.join("nested")).unwrap();
        fs::write(directory.join("nested/file.txt"), "original contents").unwrap();
    }
    fs::write(source.join("nested/file.txt"), "replacement contents").unwrap();
    fs::write(target.join("old.txt"), "discard this backup").unwrap();
    std::os::unix::fs::symlink(&referent, target.join("external-link")).unwrap();
    for directory in [&source, &target, &referent] {
        fs::set_permissions(
            directory.join("nested/file.txt"),
            fs::Permissions::from_mode(0o444),
        )
        .unwrap();
        fs::set_permissions(directory.join("nested"), fs::Permissions::from_mode(0o555)).unwrap();
        fs::set_permissions(directory, fs::Permissions::from_mode(0o555)).unwrap();
    }
    // Moving a directory across parents requires its root to be writable on Unix;
    // its nested read-only tree must still be cleaned once it becomes the backup.
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
    let request = copy(&source, &target);
    let ticket = collision(request.clone());
    let result = request.execute(Some((ticket, CollisionChoice::Replace)));
    let mut names = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    // Capture modes before restoring fixture permissions for TempDir's own cleanup.
    let modes = [&source, &target, &referent].map(|directory| {
        let modes = [
            directory.to_path_buf(),
            directory.join("nested"),
            directory.join("nested/file.txt"),
        ]
        .map(|path| fs::metadata(path).unwrap().permissions().mode() & 0o777);
        fs::set_permissions(directory, fs::Permissions::from_mode(0o755)).unwrap();
        fs::set_permissions(directory.join("nested"), fs::Permissions::from_mode(0o755)).unwrap();
        modes
    });
    assert!(matches!(
        result.unwrap(),
        OperationOutcome::Completed { .. }
    ));
    assert_eq!(
        modes,
        [
            [0o555, 0o555, 0o444],
            [0o555, 0o555, 0o444],
            [0o555, 0o555, 0o444]
        ]
    );
    assert_eq!(
        fs::read_to_string(source.join("nested/file.txt")).unwrap(),
        "replacement contents"
    );
    assert_eq!(
        fs::read_to_string(target.join("nested/file.txt")).unwrap(),
        "replacement contents"
    );
    assert_eq!(
        fs::read_to_string(referent.join("nested/file.txt")).unwrap(),
        "original contents"
    );
    assert!(!target.join("old.txt").exists());
    assert_eq!(names, ["referent", "source", "target"]);
}

#[test]
fn replacing_an_ancestor_of_the_source_is_rejected_without_removing_the_source() {
    let root = tempfile::tempdir().unwrap();
    let target = root.path().join("parent");
    let source = target.join("source.txt");
    fs::create_dir(&target).unwrap();
    fs::write(&source, "keep original").unwrap();
    let request = copy(&source, &target);
    let ticket = collision(request.clone());
    assert!(
        request
            .execute(Some((ticket, CollisionChoice::Replace)))
            .is_err()
    );
    assert_eq!(fs::read_to_string(source).unwrap(), "keep original");
}

#[cfg(unix)]
#[test]
fn a_read_only_source_folder_can_be_copied_with_its_permissions_preserved() {
    use std::os::unix::fs::PermissionsExt;
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("copy");
    fs::create_dir_all(source.join("nested/deeper")).unwrap();
    fs::write(source.join("nested/deeper/file.txt"), "readable").unwrap();
    for relative in ["nested/deeper", "nested", ""] {
        fs::set_permissions(source.join(relative), fs::Permissions::from_mode(0o555)).unwrap();
    }
    let result = copy(&source, &target).execute(None);
    let source_modes = ["", "nested", "nested/deeper"].map(|relative| {
        let path = source.join(relative);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        mode
    });
    assert!(matches!(
        result.unwrap(),
        OperationOutcome::Completed { .. }
    ));
    let target_modes = ["", "nested", "nested/deeper"].map(|relative| {
        let path = target.join(relative);
        let mode = fs::metadata(&path).unwrap().permissions().mode() & 0o777;
        fs::set_permissions(path, fs::Permissions::from_mode(0o755)).unwrap();
        mode
    });
    assert_eq!(source_modes, [0o555; 3]);
    assert_eq!(target_modes, [0o555; 3]);
    for directory in [&source, &target] {
        assert_eq!(
            fs::read_to_string(directory.join("nested/deeper/file.txt")).unwrap(),
            "readable"
        );
    }
    let mut names = fs::read_dir(root.path())
        .unwrap()
        .map(|entry| entry.unwrap().file_name())
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names, ["copy", "source"]);
}

#[test]
fn copying_a_read_only_file_preserves_contents_and_permissions_without_changing_the_source() {
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source.txt");
    let target = root.path().join("copy.txt");
    fs::write(&source, "read-only source").unwrap();
    let original_permissions = fs::metadata(&source).unwrap().permissions();
    let mut read_only = original_permissions.clone();
    read_only.set_readonly(true);
    fs::set_permissions(&source, read_only).unwrap();
    let result = copy(&source, &target).execute(None);
    let source_permissions = fs::metadata(&source).unwrap().permissions();
    let target_permissions = fs::metadata(&target).map(|m| m.permissions());
    fs::set_permissions(&source, original_permissions.clone()).unwrap();
    if target_permissions.is_ok() {
        fs::set_permissions(&target, original_permissions).unwrap();
    }
    assert!(matches!(
        result.unwrap(),
        OperationOutcome::Completed { .. }
    ));
    assert!(source_permissions.readonly());
    assert!(target_permissions.unwrap().readonly());
    assert_eq!(fs::read_to_string(source).unwrap(), "read-only source");
    assert_eq!(fs::read_to_string(target).unwrap(), "read-only source");
}

#[test]
fn background_folder_copy_and_terminal_input_both_complete_without_sharing_the_ui_thread() {
    use rodtty::{
        config::Config,
        session::Session,
        work::{Event, Job, Worker},
    };
    use std::{
        sync::Arc,
        time::{Duration, Instant},
    };
    let root = tempfile::tempdir().unwrap();
    let source = root.path().join("source");
    let target = root.path().join("copy");
    fs::create_dir(&source).unwrap();
    for number in 0..512 {
        fs::write(source.join(format!("file-{number}.txt")), b"copied content").unwrap();
    }
    let session = Session::spawn(
        Path::new(env!("CARGO_BIN_EXE_rodtty-shell")),
        root.path(),
        root.path(),
        Config::default(),
        24,
        100,
        Arc::new(|| {}),
    )
    .unwrap();
    let worker = Worker::new(Arc::new(|| {}));
    worker
        .send(Job::FileOperation {
            id: 42,
            request: copy(&source, &target),
            decision: None,
        })
        .unwrap();
    session
        .send(b"printf 'operation-'; printf 'terminal-ready\\n'\r".to_vec())
        .unwrap();
    let start = Instant::now();
    let mut copied = false;
    let mut typed = false;
    while start.elapsed() < Duration::from_secs(15) {
        for event in worker.drain() {
            if let Event::FileOperation { id: 42, result } = event {
                assert!(matches!(
                    result.unwrap(),
                    OperationOutcome::Completed { .. }
                ));
                copied = true;
            }
        }
        typed |=
            session.with_screen(|screen| screen.contents().contains("operation-terminal-ready"));
        if copied && typed {
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    assert!(copied, "background copy did not finish");
    assert!(typed, "terminal did not accept input during the operation");
    assert_eq!(
        fs::read(target.join("file-511.txt")).unwrap(),
        b"copied content"
    );
}

#[test]
fn save_as_preserves_unsaved_text_outside_removal_and_requires_fresh_collision_consent() {
    use rodtty::files::Document;
    let root = tempfile::tempdir().unwrap();
    let folder = root.path().join("remove-me");
    fs::create_dir(&folder).unwrap();
    let source = folder.join("notes.txt");
    fs::write(&source, "saved source").unwrap();
    let target = root.path().join("preserved.txt");
    fs::write(&target, "existing destination").unwrap();
    let request = OperationRequest {
        operation: FileOperation::SaveAs {
            document: Document::open(&source).unwrap(),
            text: "unsaved work".into(),
            removing: folder.clone(),
            protected: Vec::new(),
        },
        target: target.clone(),
    };
    let ticket = collision(request.clone());
    fs::write(&target, "external edit").unwrap();
    let ticket = match request
        .clone()
        .execute(Some((ticket, CollisionChoice::Replace)))
        .unwrap()
    {
        OperationOutcome::Collision(ticket) => ticket,
        other => panic!("expected fresh collision decision: {other:?}"),
    };
    assert_eq!(fs::read_to_string(&target).unwrap(), "external edit");
    let saved = match request
        .execute(Some((ticket, CollisionChoice::KeepBoth)))
        .unwrap()
    {
        OperationOutcome::SavedAs(document) => document,
        other => panic!("expected saved work: {other:?}"),
    };
    assert_eq!(saved.path, root.path().join("preserved copy.txt"));
    assert_eq!(fs::read_to_string(saved.path).unwrap(), "unsaved work");
    assert_eq!(fs::read_to_string(source).unwrap(), "saved source");
    assert_eq!(fs::read_to_string(target).unwrap(), "external edit");
}

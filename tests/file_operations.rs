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
    let read_only = source.join("read-only");
    fs::create_dir(&read_only).unwrap();
    fs::write(read_only.join("file.txt"), "read-only nested file").unwrap();
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o555)).unwrap();
    let _socket = std::os::unix::net::UnixListener::bind(source.join("socket")).unwrap();
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
    fs::set_permissions(&read_only, fs::Permissions::from_mode(0o755)).unwrap();
    assert_eq!(names, ["source", "target"]);
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
    fs::create_dir(&source).unwrap();
    fs::write(source.join("file.txt"), "readable").unwrap();
    fs::set_permissions(&source, fs::Permissions::from_mode(0o555)).unwrap();
    let result = copy(&source, &target).execute(None);
    fs::set_permissions(&source, fs::Permissions::from_mode(0o755)).unwrap();
    assert!(matches!(
        result.unwrap(),
        OperationOutcome::Completed { .. }
    ));
    assert_eq!(
        fs::read_to_string(target.join("file.txt")).unwrap(),
        "readable"
    );
    assert_eq!(
        fs::metadata(&target).unwrap().permissions().mode() & 0o777,
        0o555
    );
    fs::set_permissions(&target, fs::Permissions::from_mode(0o755)).unwrap();
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

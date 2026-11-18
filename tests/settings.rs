use rodtty::{config::Config, work::Layout};

mod common;

#[test]
fn preferences_migrate_once_and_workspace_writes_leave_owner_json_untouched() {
    let root = common::tempdir().unwrap();
    std::fs::write(
        root.path().join("config.toml"),
        "history = false\nscrollback = 2000\ndisabled = ['cat']\n",
    )
    .unwrap();
    std::fs::write(
        root.path().join("workspace.toml"),
        "explorer = true\neditor_share = 0.65\n",
    )
    .unwrap();
    let mut config = Config::load(root.path()).unwrap();
    assert!(!config.history);
    assert_eq!(config.disabled, ["cat"]);
    let settings = root.path().join("settings.json");
    let original = std::fs::read_to_string(&settings).unwrap();
    let mut layout = Layout::load(root.path()).unwrap();
    assert!(layout.explorer);
    layout.explorer = false;
    layout.save(root.path()).unwrap();
    assert_eq!(std::fs::read_to_string(&settings).unwrap(), original);
    assert!(root.path().join("workspace-state.json").is_file());
    assert!(root.path().join("config.toml").is_file());
    assert!(root.path().join("workspace.toml").is_file());
    config.history = true;
    config.save(root.path()).unwrap();
    assert!(Config::load(root.path()).unwrap().history);
    assert!(!Layout::load(root.path()).unwrap().explorer);
}

#[test]
fn existing_destinations_win_and_unknown_preferences_survive_preference_saves() {
    let legacy = common::tempdir().unwrap();
    let root = common::tempdir().unwrap();
    std::fs::write(legacy.path().join("config.toml"), "history = false\n").unwrap();
    std::fs::write(
        legacy.path().join("startup.sh"),
        "export TEST_MIGRATED=yes\n",
    )
    .unwrap();
    std::fs::write(legacy.path().join("history"), "echo retained\n").unwrap();
    std::fs::write(legacy.path().join("workspace.toml"), "explorer = true\n").unwrap();
    std::fs::write(
        root.path().join("workspace-state.json"),
        r#"{"explorer":false}"#,
    )
    .unwrap();
    std::fs::write(
        root.path().join("settings.json"),
        r#"{"history":true,"future":{"enabled":true}}"#,
    )
    .unwrap();
    rodtty::config::migrate_storage(root.path(), legacy.path()).unwrap();
    let mut config = Config::load(root.path()).unwrap();
    assert!(config.history);
    assert!(!Layout::load(root.path()).unwrap().explorer);
    config.scrollback = 3000;
    config.save(root.path()).unwrap();
    rodtty::config::migrate_storage(root.path(), legacy.path()).unwrap();
    let json: serde_json::Value =
        serde_json::from_slice(&std::fs::read(root.path().join("settings.json")).unwrap()).unwrap();
    assert_eq!(json["future"]["enabled"], true);
    assert_eq!(json["scrollback"], 3000);
    assert_eq!(
        std::fs::read_to_string(root.path().join("history")).unwrap(),
        "echo retained\n"
    );
    assert!(legacy.path().join("history").is_file());
}

#[test]
fn migration_launch_reopens_the_old_host_and_new_hosts_also_hold_the_old_lock() {
    use rodtty::instance::Instance;
    let legacy = common::tempdir().unwrap();
    let root = common::tempdir().unwrap();
    let old_host = Instance::acquire(legacy.path()).unwrap().unwrap();
    assert!(
        Instance::acquire_owner(root.path(), legacy.path())
            .unwrap()
            .is_none()
    );
    assert!(legacy.path().join("open.request").is_file());
    drop(old_host);
    let new_host = Instance::acquire_owner(root.path(), legacy.path())
        .unwrap()
        .unwrap();
    let (tx, rx) = std::sync::mpsc::channel();
    let _watcher = new_host
        .watch(move || {
            let _ = tx.send(());
        })
        .unwrap();
    assert!(Instance::acquire(legacy.path()).unwrap().is_none());
    rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
    assert!(
        Instance::acquire_owner(root.path(), legacy.path())
            .unwrap()
            .is_none()
    );
    drop(new_host);
    assert!(
        Instance::acquire_owner(root.path(), legacy.path())
            .unwrap()
            .is_some()
    );
}

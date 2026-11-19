use rodtty::paths::Canonical as _;
use rodtty::{
    files::{Document, MAX_FILE_BYTES, SaveFailure, TextFile, list_directory},
    git::{self, Comparison},
};
use std::{fs, path::Path, process::Command};

mod common;

#[test]
fn utf8_bom_and_mixed_endings_survive_edits() {
    let root = common::tempdir().unwrap();
    let path = root.path().join("café.txt");
    fs::write(&path, b"\xef\xbb\xbfone\r\ntwo\nthree\r\n").unwrap();
    let mut document = Document::open(&path).unwrap();
    assert_eq!(document.saved.text, "one\ntwo\nthree\n");
    document.save("one\ntwo edited\nthree\n", false).unwrap();
    assert_eq!(
        fs::read(&path).unwrap(),
        b"\xef\xbb\xbfone\r\ntwo edited\nthree\r\n"
    );
}

#[test]
fn external_changes_and_deletion_require_explicit_overwrite() {
    let root = common::tempdir().unwrap();
    let path = root.path().join("file.txt");
    fs::write(&path, "original\n").unwrap();
    let mut document = Document::open(&path).unwrap();
    fs::write(&path, "external\n").unwrap();
    assert!(matches!(
        document.save("my edits\n", false),
        Err(SaveFailure::Conflict)
    ));
    assert_eq!(fs::read_to_string(&path).unwrap(), "external\n");
    document.save("my edits\n", true).unwrap();
    fs::remove_file(&path).unwrap();
    assert!(matches!(
        document.save("more edits\n", false),
        Err(SaveFailure::Conflict)
    ));
    assert!(!path.exists());
    document.save("more edits\n", true).unwrap();
    assert_eq!(fs::read_to_string(path).unwrap(), "more edits\n");
}

#[test]
fn unchanged_buffer_does_not_bypass_external_conflict() {
    let root = common::tempdir().unwrap();
    let path = root.path().join("file");
    fs::write(&path, "original").unwrap();
    let mut document = Document::open(&path).unwrap();
    fs::write(&path, "external").unwrap();
    assert!(matches!(
        document.save("original", false),
        Err(SaveFailure::Conflict)
    ));
    assert_eq!(fs::read_to_string(path).unwrap(), "external");
}

#[test]
fn rejects_binary_invalid_utf8_and_large_files() {
    assert!(TextFile::decode(vec![b'x', 0, b'y']).is_err());
    assert!(TextFile::decode(vec![0xff, 0xfe]).is_err());
    assert!(TextFile::decode(vec![b'x'; MAX_FILE_BYTES as usize + 1]).is_err());
}

#[test]
fn explorer_sorts_directories_first_and_can_show_hidden_files() {
    let root = common::tempdir().unwrap();
    fs::create_dir(root.path().join("z-directory")).unwrap();
    fs::write(root.path().join("alpha.txt"), "a").unwrap();
    fs::write(root.path().join(".hidden"), "h").unwrap();
    let (_, entries) = list_directory(root.path(), false).unwrap();
    assert_eq!(
        entries.iter().map(|e| e.name.as_str()).collect::<Vec<_>>(),
        vec!["z-directory", "alpha.txt"]
    );
    assert_eq!(list_directory(root.path(), true).unwrap().1.len(), 3);
}

#[cfg(unix)]
#[test]
fn save_preserves_executable_mode_and_symlink_target() {
    use std::os::unix::fs::{PermissionsExt, symlink};
    let root = common::tempdir().unwrap();
    let path = root.path().join("script.sh");
    let link = root.path().join("link.sh");
    fs::write(&path, "echo original\n").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o751)).unwrap();
    symlink(&path, &link).unwrap();
    let mut document = Document::open(&link).unwrap();
    document.save("echo edited\n", false).unwrap();
    assert!(link.symlink_metadata().unwrap().file_type().is_symlink());
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o751
    );
}

fn git_command(path: &Path, args: &[&str]) {
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args([
            "-c",
            "user.name=Rodtty Test",
            "-c",
            "user.email=rodtty@example.invalid",
            "-c",
            "commit.gpgsign=false",
            "-c",
            "core.autocrlf=false",
        ])
        .args(args)
        .output()
        .unwrap();
    assert!(
        output.status.success(),
        "{}",
        String::from_utf8_lossy(&output.stderr)
    );
}
fn repo() -> tempfile::TempDir {
    let root = common::tempdir().unwrap();
    git_command(root.path(), &["init", "-b", "main"]);
    root
}
fn commit(root: &Path) {
    git_command(root, &["add", "."]);
    git_command(root, &["commit", "-m", "fixture"]);
}

#[test]
fn comparisons_separate_buffer_disk_index_and_head_without_writing() {
    let root = repo();
    let path = root.path().join("space name.rs");
    fs::write(&path, "head\n").unwrap();
    commit(root.path());
    fs::write(&path, "index\n").unwrap();
    git_command(root.path(), &["add", "space name.rs"]);
    fs::write(&path, "disk\n").unwrap();
    let index = root.path().join(".git/index");
    let before = fs::read(&index).unwrap();
    let modified = fs::metadata(&index).unwrap().modified().unwrap();
    for (mode, old, new) in [
        (Comparison::Current, "head", "buffer"),
        (Comparison::Unstaged, "index", "disk"),
        (Comparison::Staged, "head", "index"),
    ] {
        let result = git::diff(&path, "buffer\n", mode).unwrap();
        assert!(
            result.patch.contains(&format!("-{old}\n")),
            "{}",
            result.patch
        );
        assert!(
            result.patch.contains(&format!("+{new}\n")),
            "{}",
            result.patch
        );
    }
    assert_eq!(fs::read(index.clone()).unwrap(), before);
    assert_eq!(fs::metadata(index).unwrap().modified().unwrap(), modified);
    assert_eq!(fs::read_to_string(path).unwrap(), "disk\n");
}

#[test]
fn untracked_files_in_new_directories_are_labelled_as_additions() {
    let root = repo();
    fs::create_dir(root.path().join("new-folder")).unwrap();
    let path = root.path().join("new-folder/café name.txt");
    fs::write(&path, "new text\n").unwrap();
    let status = git::inspect(root.path()).unwrap().unwrap();
    assert_eq!(status.badge(&path.canonical().unwrap()), "??");
    let result = git::diff(&path, "new text\n", Comparison::Current).unwrap();
    assert!(result.untracked);
    assert!(result.patch.contains("--- /dev/null"));
    assert!(result.patch.contains("+new text\n"));
}

#[test]
fn initial_staged_changes_work_before_the_first_commit() {
    let root = repo();
    let path = root.path().join("initial.txt");
    fs::write(&path, "initial\n").unwrap();
    git_command(root.path(), &["add", "initial.txt"]);
    let result = git::diff(&path, "initial\n", Comparison::Staged).unwrap();
    assert!(result.patch.contains("+initial\n"));
}

#[test]
fn staged_rename_uses_the_original_head_contents() {
    let root = repo();
    fs::write(root.path().join("old.txt"), "original\n").unwrap();
    commit(root.path());
    git_command(root.path(), &["mv", "old.txt", "new.txt"]);
    let path = root.path().join("new.txt");
    fs::write(&path, "edited\n").unwrap();
    let result = git::diff(&path, "edited\n", Comparison::Current).unwrap();
    assert!(result.patch.contains("-original\n"));
    assert!(result.patch.contains("+edited\n"));
}

#[test]
fn linked_worktree_is_resolved_from_the_opened_file() {
    let root = repo();
    fs::write(root.path().join("tracked.txt"), "original\n").unwrap();
    commit(root.path());
    let other = common::tempdir().unwrap();
    let worktree = other.path().join("linked");
    git_command(
        root.path(),
        &["worktree", "add", "-b", "other", worktree.to_str().unwrap()],
    );
    assert!(worktree.join(".git").is_file());
    let path = worktree.join("tracked.txt");
    fs::write(&path, "linked edits\n").unwrap();
    let status = git::inspect(&worktree).unwrap().unwrap();
    assert_eq!(status.root, worktree.canonical().unwrap());
    assert!(
        git::diff(&path, "linked edits\n", Comparison::Current)
            .unwrap()
            .patch
            .contains("+linked edits\n")
    );
}

#[test]
fn layout_restores_saved_files_and_pane_configuration() {
    let root = common::tempdir().unwrap();
    let mut layout = rodtty::work::Layout::default();
    layout.files.push(root.path().join("file.txt"));
    layout.explorer = true;
    layout.follow = false;
    layout.editor_share = 0.65;
    layout.save(root.path()).unwrap();
    let restored = rodtty::work::Layout::load(root.path()).unwrap();
    assert_eq!(restored.files, layout.files);
    assert!(restored.explorer);
    assert!(!restored.follow);
    assert_eq!(restored.editor_share, 0.65);
}

//! Git's messages in the owner's language. Kept in its own test binary
//! because it changes the process environment.

mod common;

#[test]
fn a_folder_outside_a_repository_is_not_a_git_error_in_any_language() {
    let root = common::tempdir().unwrap();
    // SAFETY: this binary runs only this test, so no other thread reads the
    // environment while it changes.
    unsafe {
        std::env::set_var("LC_ALL", "pt_BR.UTF-8");
        std::env::set_var("LANG", "pt_BR.UTF-8");
        std::env::set_var("GIT_CEILING_DIRECTORIES", root.path());
    }
    assert!(rodtty::git::inspect(root.path()).unwrap().is_none());
}

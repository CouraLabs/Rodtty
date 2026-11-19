use crate::desktop::workspace::*;
use crate::desktop::*;
use crate::{
    files::{Document, Entry, TextFile},
    git::Comparison,
    work::Worker,
};
use rune_kit::diff::DiffState;

pub(crate) fn demo(s: &mut State, diff: bool) {
    let path = PathBuf::from("/workspace/rodtty/src/main.rs");
    let saved =
        TextFile::decode(b"fn main() {\n    println!(\"Hello, terminal\");\n}\n".to_vec()).unwrap();
    let document = Document {
        path: path.clone(),
        saved,
        existed: true,
    };
    let edit = TextEdit::new(
        "fn main() {\n    // Terminal, files, and code in one workspace.\n    println!(\"Hello from Rodtty\");\n}\n",
    );
    let old = document.saved.text.clone();
    let new = edit.text.clone();
    s.workspace.tabs.push(Tab {
        id: 1,
        preview: false,
        document,
        edit,
        mode: if diff {
            View::Diff(Comparison::Current)
        } else {
            View::Edit
        },
        external: None,
        disk_error: None,
        dismissed: false,
        saving: false,
        save_token: 0,
        revision: 0,
        baseline: 0,
        after_save: None,
        autosave: Default::default(),
    });
    s.workspace.next_id = 2;
    s.workspace.set_terminal_focus(false);
    s.workspace.layout.explorer = true;
    s.workspace.layout.editor = true;
    s.workspace.directory = Some(PathBuf::from("/workspace/rodtty"));
    s.workspace.entries = vec![
        Entry {
            path: PathBuf::from("/workspace/rodtty/src"),
            name: "src".into(),
            directory: true,
            missing: false,
        },
        Entry {
            path: PathBuf::from("/workspace/rodtty/Cargo.toml"),
            name: "Cargo.toml".into(),
            directory: false,
            missing: false,
        },
        Entry {
            path,
            name: "main.rs".into(),
            directory: false,
            missing: false,
        },
    ];
    s.workspace.layout.expanded_folders = vec![PathBuf::from("/workspace/rodtty/src")];
    s.workspace.tree_listed = s.workspace.layout.expanded_folders.clone();
    s.rebuild_tree();
    if diff {
        s.workspace.diff = DiffState::new(vec![rune_kit::diff::FileDiff::from_texts(
            "src/main.rs",
            &old,
            &new,
        )]);
        s.workspace.diff_patch = "preview".into();
        s.workspace.diff_label = "Editor ↔ HEAD · preview includes unsaved changes".into();
    }
}

/// Developer render scene: inspect a changed file from the real repository passed via --cwd.
/// This uses the same read-only Git boundary as the interactive workspace.
pub(crate) fn real_diff_demo(s: &mut State) -> anyhow::Result<()> {
    let directory = s.directory.clone();
    let repo = crate::git::inspect(&directory)?
        .ok_or_else(|| anyhow::anyhow!("--cwd must name a Git repository"))?;
    let path = repo
        .files
        .keys()
        .find(|path| path.is_file())
        .cloned()
        .ok_or_else(|| anyhow::anyhow!("The preview repository has no changed text file"))?;
    let document = Document::open(&path)?;
    let result = crate::git::diff(&path, &document.saved.text, Comparison::Current)?;
    s.workspace.tabs = vec![Tab {
        id: 1,
        preview: false,
        edit: TextEdit::new(&document.saved.text),
        document,
        mode: View::Diff(Comparison::Current),
        external: None,
        disk_error: None,
        dismissed: false,
        saving: false,
        save_token: 0,
        revision: 0,
        baseline: 0,
        after_save: None,
        autosave: Default::default(),
    }];
    s.workspace.next_id = 2;
    s.workspace.set_terminal_focus(false);
    s.workspace.layout.explorer = true;
    s.workspace.layout.editor = true;
    s.workspace.directory = Some(directory.clone());
    s.workspace.entries = crate::files::list_directory(&directory, false)?.1;
    s.workspace.repo = Some(repo);
    s.rebuild_tree();
    s.workspace.diff = DiffState::new(rune_kit::diff::parse(&result.patch));
    s.workspace.diff_label = result.label;
    s.workspace.diff_patch = result.patch;
    s.workspace.worker = Some(Worker::new(s.wake.clone()));
    Ok(())
}

/// Developer render scene for recognizable artwork, selected rows, and dirty editor tabs.
pub(crate) fn icons_demo(s: &mut State) {
    demo(s, false);
    let root = PathBuf::from("/workspace/icons");
    s.workspace.directory = Some(root.clone());
    s.workspace.layout.terminal = false;
    s.workspace.entries = [
        ("alpine", true),
        ("alpine/Dockerfile", false),
        ("src", true),
        ("src/index.ts", false),
        ("unknown-folder", true),
        ("Dockerfile", false),
        ("package.json", false),
        ("main.rs", false),
        ("types.d.ts", false),
        ("mystery.rodttyunknown", false),
    ]
    .into_iter()
    .map(|(name, directory)| Entry {
        path: root.join(name),
        name: PathBuf::from(name)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .into_owned(),
        directory,
        missing: false,
    })
    .collect();
    s.workspace.layout.expanded_folders = vec![root.join("alpine"), root.join("src")];
    s.workspace.tree_listed = s.workspace.layout.expanded_folders.clone();
    s.workspace.tabs.clear();
    for (index, name) in [
        "main.rs",
        "client/index.ts",
        "server/index.ts",
        "Dockerfile",
        "mystery.rodttyunknown",
    ]
    .into_iter()
    .enumerate()
    {
        let saved = TextFile::decode(b"// Saved file contents\n".to_vec()).unwrap();
        let edit = TextEdit::new(if index == 0 {
            "// Unsaved changes remain visible beside the Rust artwork\n"
        } else {
            &saved.text
        });
        s.workspace.tabs.push(Tab {
            id: index as u64 + 1,
            preview: false,
            document: Document {
                path: root.join(name),
                saved,
                existed: true,
            },
            edit,
            mode: View::Edit,
            external: None,
            disk_error: None,
            dismissed: false,
            saving: false,
            save_token: 0,
            revision: u64::from(index == 0),
            baseline: 0,
            after_save: None,
            autosave: Default::default(),
        });
    }
    s.workspace.active = 0;
    s.workspace.next_id = 6;
    s.rebuild_tree();
    s.workspace
        .tree
        .select(&root.join("main.rs").to_string_lossy());
}

/// Tooling-only scene: inspect a nested tree of the actual selected host folder.
pub(crate) fn host_tree_demo(s: &mut State) {
    let root = s.directory.clone();
    s.workspace.layout.explorer = true;
    s.workspace.layout.expanded_folders = vec![root.join("src"), root.join("src/desktop")];
    s.workspace.layout.hidden = s.config.explorer.show_hidden;
    s.workspace.layout.follow = s.config.explorer.follow_terminal;
    let listing = (|| -> anyhow::Result<_> {
        let repo = crate::git::inspect(&root)?;
        let ignored = if s.config.explorer.respect_ignore {
            repo.as_ref()
                .map(crate::git::ignored)
                .transpose()?
                .unwrap_or_default()
        } else {
            Vec::new()
        };
        let listing = crate::files::list_tree(
            &root,
            &s.workspace.layout.expanded_folders,
            s.config.explorer.show_hidden,
            &ignored,
            None,
        )?;
        Ok((listing, repo))
    })();
    match listing {
        Ok((listing, repo)) => {
            s.workspace.directory = Some(listing.root.clone());
            s.workspace.entries = listing.entries;
            s.workspace.tree_errors = listing.errors;
            s.workspace.tree_listed = listing.listed;
            s.workspace.repo = repo;
            s.rebuild_tree();
            s.workspace
                .tree
                .select(&root.join("src/desktop/workspace.rs").to_string_lossy());
        }
        Err(error) => s.workspace.explorer_error = Some(format!("{error:#}")),
    }
}

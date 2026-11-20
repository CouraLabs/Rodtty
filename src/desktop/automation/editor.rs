//! Editor, search and Git tools, through the editor's own open, edit and
//! version-protected save paths and the read-only Git provider.
use super::*;
use crate::automation::{arg_bool, arg_opt_str, arg_str, arg_u64};

impl State {
    pub(crate) fn editor_target(&self, id: u64) -> String {
        format!("ed_{}_{id}", self.automation.instance)
    }

    /// Whether the client may use `path`, by the workspace it belongs to.
    pub(crate) fn authorize_path(
        &self,
        grant: &ClientGrant,
        path: &Path,
    ) -> Result<PathBuf, ToolError> {
        use crate::paths::Canonical as _;
        if !path.is_absolute() {
            return Err(ToolError::invalid("Paths must be absolute"));
        }
        // A file that doesn't exist yet is judged by its folder. Symlinks
        // resolve first, so a link can't reach outside an authorized root.
        let resolved = path
            .canonical()
            .or_else(|_| {
                let parent = path
                    .parent()
                    .ok_or_else(|| std::io::Error::other("no parent"))?;
                Ok::<_, std::io::Error>(
                    parent
                        .canonical()?
                        .join(path.file_name().unwrap_or_default()),
                )
            })
            .map_err(|e| {
                ToolError::new(ErrorCode::UnknownTarget, format!("{}: {e}", path.display()))
            })?;
        let folder = if resolved.is_dir() {
            resolved.clone()
        } else {
            resolved.parent().map(Path::to_owned).unwrap_or_default()
        };
        let (workspace, _) = self.workspace_of(&folder);
        // An unregistered folder is its own workspace; a grant for a
        // registered root also covers everything below it.
        let registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
        let allowed = grant.allows_workspace(&workspace)
            || registry
                .workspaces
                .iter()
                .any(|w| resolved.starts_with(&w.root) && grant.allows_workspace(&w.id))
            || {
                let (active, active_root) = self.active_workspace();
                resolved.starts_with(&active_root) && grant.allows_workspace(&active)
            };
        if !allowed {
            return Err(ToolError::new(
                ErrorCode::WorkspaceDenied,
                "That path is outside this client's workspaces",
            ));
        }
        Ok(resolved)
    }

    fn editor_index(&self, grant: &ClientGrant, target: &str) -> Result<usize, ToolError> {
        let unknown = || ToolError::new(ErrorCode::UnknownTarget, format!("No editor {target}"));
        let (instance, id) = target
            .strip_prefix("ed_")
            .and_then(|r| r.rsplit_once('_'))
            .ok_or_else(unknown)?;
        if instance != self.automation.instance {
            return Err(ToolError::new(
                ErrorCode::UnknownTarget,
                "That editor ID is from an earlier Rodtty run",
            ));
        }
        let id: u64 = id.parse().map_err(|_| unknown())?;
        let index = self
            .workspace
            .tabs
            .iter()
            .position(|t| t.id == id)
            .ok_or_else(|| {
                ToolError::new(
                    ErrorCode::ClosedTarget,
                    format!("Editor {target} is closed"),
                )
            })?;
        self.authorize_path(grant, &self.workspace.tabs[index].document.path)?;
        Ok(index)
    }

    fn editor_row(&self, index: usize) -> Value {
        let tab = &self.workspace.tabs[index];
        json!({
            "id": self.editor_target(tab.id),
            "path": tab.document.path,
            "revision": tab.revision,
            "dirty": tab.dirty(),
            "saving": tab.saving,
            "changed_on_disk": tab.external.is_some(),
            "active": index == self.workspace.active && !self.settings && !self.search_tab,
        })
    }

    pub(crate) fn auto_editor_list(&mut self, grant: &ClientGrant) -> ToolResult {
        let rows: Vec<Value> = (0..self.workspace.tabs.len())
            .filter(|i| {
                self.authorize_path(grant, &self.workspace.tabs[*i].document.path)
                    .is_ok()
            })
            .map(|i| self.editor_row(i))
            .collect();
        Ok(json!({"editors": rows}))
    }

    pub(crate) fn auto_editor_open(
        &mut self,
        grant: &ClientGrant,
        args: &Value,
        tx: &mut Tx,
    ) -> ToolResult {
        let path = self.authorize_path(grant, Path::new(arg_str(args, "path")?))?;
        if !path.is_file() {
            return Err(ToolError::new(
                ErrorCode::UnknownTarget,
                format!("{} is not a file", path.display()),
            ));
        }
        let line = arg_u64(args, "line", 0)? as usize;
        let column = arg_u64(args, "column", 1)?.max(1) as usize;
        if line > 0 {
            self.open_file_at(
                path.clone(),
                false,
                workspace::SourceLocation {
                    line,
                    column,
                    length: 0,
                },
            );
            self.apply_pending_location(tx);
        } else {
            self.open_file(path.clone());
        }
        let index = self
            .workspace
            .tabs
            .iter()
            .position(|t| t.document.path == path);
        Ok(
            json!({"path": path, "editor": index.map(|i| self.editor_row(i)), "opening": index.is_none()}),
        )
    }

    pub(crate) fn auto_editor_read(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let index = self.editor_index(grant, arg_str(args, "editor")?)?;
        let tab = &self.workspace.tabs[index];
        let mut row = self.editor_row(index);
        row["text"] = json!(tab.edit.text);
        if arg_bool(args, "include_saved", false)? {
            row["saved_text"] = json!(tab.document.saved.text);
        }
        if let Some(external) = &tab.external {
            row["disk_text"] = json!(external.text);
        }
        Ok(row)
    }

    pub(crate) fn auto_editor_edit(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let index = self.editor_index(grant, arg_str(args, "editor")?)?;
        let expected = arg_u64(args, "expected_revision", u64::MAX)?;
        let tab = &mut self.workspace.tabs[index];
        if tab.revision != expected {
            return Err(ToolError::new(
                ErrorCode::Conflict,
                "The buffer changed since that revision; read it again",
            )
            .with_data(json!({"revision": tab.revision})));
        }
        if tab.saving {
            return Err(ToolError::new(
                ErrorCode::Conflict,
                "The buffer is being saved",
            ));
        }
        let range = match (arg_opt_str(args, "find")?, arg_opt_str(args, "text")?) {
            (Some(find), None) => {
                let mut matches = tab.edit.text.match_indices(find);
                let (start, _) = matches.next().ok_or_else(|| {
                    ToolError::new(ErrorCode::Conflict, "`find` text is not in the buffer")
                })?;
                if matches.next().is_some() {
                    return Err(ToolError::new(
                        ErrorCode::Conflict,
                        "`find` text occurs more than once; give more context",
                    ));
                }
                start..start + find.len()
            }
            (None, Some(_)) => 0..tab.edit.text.len(),
            _ => return Err(ToolError::invalid("Give either find with replace, or text")),
        };
        let replacement = match arg_opt_str(args, "text")? {
            Some(text) => text.to_owned(),
            None => arg_str(args, "replace")?.to_owned(),
        };
        // Through the editor's own edit history, so the owner can undo it.
        tab.edit.set_selection(range.start, range.end);
        tab.edit.insert(&replacement);
        tab.edit.break_history_group();
        let id = tab.id;
        self.changed_editor(id);
        self.invalidate_diff(false);
        Ok(self.editor_row(index))
    }

    fn auto_editor_begin_save(
        &mut self,
        grant: &ClientGrant,
        target: &str,
        expected: u64,
    ) -> Result<u64, ToolError> {
        let index = self.editor_index(grant, target)?;
        let tab = &self.workspace.tabs[index];
        if tab.revision != expected {
            return Err(ToolError::new(
                ErrorCode::Conflict,
                "The buffer changed since that revision; read it again",
            )
            .with_data(json!({"revision": tab.revision})));
        }
        let id = tab.id;
        self.save_file(id, false, None);
        Ok(id)
    }
}

pub(super) fn save(host: &Host, grant: &ClientGrant, args: &Value) -> ToolResult {
    let target = arg_str(args, "editor")?.to_owned();
    let expected = arg_u64(args, "expected_revision", u64::MAX)?;
    let g = grant.clone();
    let t = target.clone();
    let id = on_ui(&host.handle, move |s, _| {
        s.auto_editor_begin_save(&g, &t, expected)
    })??;
    let deadline = Instant::now() + Duration::from_secs(20);
    loop {
        std::thread::sleep(Duration::from_millis(25));
        let g = grant.clone();
        let t = target.clone();
        let done = on_ui(&host.handle, move |s, _| -> Option<ToolResult> {
            let index = match s.editor_index(&g, &t) {
                Ok(index) => index,
                Err(error) => return Some(Err(error)),
            };
            let tab = &s.workspace.tabs[index];
            if tab.saving {
                return None;
            }
            let conflict = matches!(s.pending, Some(Pending::File(workspace::FilePrompt::Conflict { id: p, .. })) if p == id)
                || s.workspace
                    .deferred_save_conflicts
                    .iter()
                    .any(|c| c.id == id);
            if conflict {
                return Some(Err(ToolError::new(
                    ErrorCode::Conflict,
                    "The file changed on disk. Nothing was written: the buffer keeps your edits and the disk keeps the other version. The owner decides in Rodtty.",
                )));
            }
            if tab.dirty() {
                return Some(Err(ToolError::new(ErrorCode::Internal, s.message.clone())));
            }
            Some(Ok(s.editor_row(index)))
        })?;
        if let Some(result) = done {
            return result.map(|mut row| {
                row["saved"] = json!(true);
                row
            });
        }
        if Instant::now() > deadline {
            return Err(ToolError::new(
                ErrorCode::Timeout,
                "The save did not finish in time",
            ));
        }
    }
}

pub(super) fn search(
    host: &Host,
    grant: &ClientGrant,
    args: &Value,
    cancel: &AtomicBool,
) -> ToolResult {
    let text = arg_str(args, "query")?.to_owned();
    let max = arg_u64(args, "max", 100)?.clamp(1, 500) as usize;
    let g = grant.clone();
    let workspace = arg_opt_str(args, "workspace")?.map(str::to_owned);
    let root = on_ui(&host.handle, move |s, _| -> Result<PathBuf, ToolError> {
        let root = match workspace {
            Some(id) => {
                let registry = crate::workspaces::Registry::load(&s.root).unwrap_or_default();
                match registry.get(&id) {
                    Some(w) => w.root.clone(),
                    None if s.active_workspace().0 == id => s.active_workspace().1,
                    None => {
                        return Err(ToolError::new(
                            ErrorCode::UnknownTarget,
                            "No such workspace",
                        ));
                    }
                }
            }
            None => s.active_workspace().1,
        };
        s.authorize_path(&g, &root)
    })??;
    let query = crate::files::search::Query {
        ignored: crate::files::search::ignored_paths(&root).unwrap_or_default(),
        root: root.clone(),
        text,
        hidden: false,
        limit: max,
    };
    let mut files = Vec::new();
    let summary = crate::files::search::search(
        &query,
        || cancel.load(Ordering::Relaxed),
        |found| {
            files.push(json!({
            "path": found.path,
            "relative": found.relative,
            "name_match": found.name.is_some(),
            "lines": found.lines.iter().map(|l| json!({"line": l.line, "column": l.column, "text": l.snippet.text})).collect::<Vec<_>>(),
        }));
        },
    );
    Ok(
        json!({"root": root, "files": files, "rows": summary.rows, "truncated": summary.truncated, "cancelled": summary.cancelled}),
    )
}

fn repository_root(host: &Host, grant: &ClientGrant, args: &Value) -> Result<PathBuf, ToolError> {
    let g = grant.clone();
    let workspace = arg_opt_str(args, "workspace")?.map(str::to_owned);
    on_ui(&host.handle, move |s, _| -> Result<PathBuf, ToolError> {
        let root = match workspace {
            Some(id) => crate::workspaces::Registry::load(&s.root)
                .unwrap_or_default()
                .get(&id)
                .map(|w| w.root.clone())
                .or_else(|| (s.active_workspace().0 == id).then(|| s.active_workspace().1))
                .ok_or_else(|| ToolError::new(ErrorCode::UnknownTarget, "No such workspace"))?,
            None => s.active_workspace().1,
        };
        s.authorize_path(&g, &root)
    })?
}

pub(super) fn git_changes(host: &Host, grant: &ClientGrant, args: &Value) -> ToolResult {
    let root = repository_root(host, grant, args)?;
    let changes = crate::git::changes(&root)
        .map_err(|e| ToolError::new(ErrorCode::Internal, format!("{e:#}")))?;
    Ok(match changes {
        None => {
            json!({"root": root, "repository": null, "message": "This folder is outside a Git repository"})
        }
        Some(changes) => json!(changes),
    })
}

pub(super) fn git_diff(host: &Host, grant: &ClientGrant, args: &Value) -> ToolResult {
    let path = PathBuf::from(arg_str(args, "path")?);
    let staged = arg_bool(args, "staged", false)?;
    let show = arg_bool(args, "show", false)?;
    let g = grant.clone();
    let p = path.clone();
    let path = on_ui(&host.handle, move |s, _| s.authorize_path(&g, &p))??;
    let comparison = if staged {
        crate::git::Comparison::Staged
    } else {
        crate::git::Comparison::Unstaged
    };
    let diff = crate::git::diff(&path, "", comparison)
        .map_err(|e| ToolError::new(ErrorCode::Internal, format!("{e:#}")))?;
    if show {
        if !grant.scopes.contains(&Scope::Workspace) {
            return Err(ToolError::new(
                ErrorCode::ScopeDenied,
                "Showing a diff in Rodtty needs the workspace scope",
            ));
        }
        let p = path.clone();
        on_ui(&host.handle, move |s, tx| s.show_diff(p, comparison, tx))?;
    }
    Ok(json!({
        "path": path,
        "comparison": if staged { "staged" } else { "unstaged" },
        "label": diff.label,
        "changed": diff.changed,
        "untracked": diff.untracked,
        "patch": diff.patch,
        "shown": show,
    }))
}

pub(super) fn services(host: &Host, grant: &ClientGrant, cancel: &AtomicBool) -> ToolResult {
    let g = grant.clone();
    // Shell process IDs of the sessions this client may see, by terminal ID.
    let shells = on_ui(&host.handle, move |s, _| -> Vec<(u32, String)> {
        let allowed: Vec<usize> = (0..s.sessions.len())
            .filter(|i| g.allows_workspace(&s.session_owner(*i).workspace))
            .collect();
        allowed
            .into_iter()
            .filter_map(|i| {
                s.sessions[i]
                    .processes()
                    .0
                    .map(|pid| (pid, s.terminal_target(&s.sessions[i])))
            })
            .collect()
    })?;
    let listeners = crate::services::list(cancel).map_err(|e| {
        ToolError::new(
            ErrorCode::Unavailable,
            format!("Listener inspection is unavailable: {e}"),
        )
    })?;
    let parents = crate::services::parents();
    let pids: Vec<u32> = shells.iter().map(|(p, _)| *p).collect();
    let rows: Vec<Value> = listeners
        .iter()
        .map(|l| {
            let owner = match (l.pid, &parents) {
                (Some(pid), Some(parents)) => match crate::services::owner(pid, &pids, parents) {
                    Some(shell) => shells.iter().find(|(p, _)| *p == shell).map(|(_, t)| json!(t)).unwrap_or(json!("unknown")),
                    None => json!("external"),
                },
                _ => json!("unknown"),
            };
            json!({"port": l.port, "address": l.address, "pid": l.pid, "process": l.process, "owner": owner, "loopback": l.loopback_reachable(), "url_if_http": l.loopback_reachable().then(|| l.local_url("http"))})
        })
        .collect();
    Ok(json!({"listeners": rows}))
}

pub(super) fn environment(
    host: &Host,
    grant: &ClientGrant,
    args: &Value,
    cancel: &AtomicBool,
) -> ToolResult {
    let root = repository_root(host, grant, args)?;
    let rows = crate::environment::check(&root, crate::environment::PROBE_TIMEOUT, cancel);
    Ok(
        json!({"root": root, "environment": "Rodtty's own PATH; terminals with custom shell changes may differ", "rows": rows}),
    )
}

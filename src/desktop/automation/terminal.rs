//! Terminal tools: sessions an agent creates, reads, types into and waits on,
//! through each session's own PTY.
use super::*;
use crate::{
    automation::{arg_bool, arg_opt_str, arg_str, arg_u64, keys},
    session::{CommandOutcome, CommandRecord, TerminalState},
};

/// The most an output read returns at once.
const MAX_READ: usize = 256 * 1024;

pub(crate) fn outcome_name(outcome: CommandOutcome) -> &'static str {
    match outcome {
        CommandOutcome::Running => "running",
        CommandOutcome::Succeeded => "succeeded",
        CommandOutcome::Failed => "failed",
        CommandOutcome::Interrupted => "interrupted",
        CommandOutcome::Unknown => "unknown",
    }
}

pub(crate) fn command_json(command: &CommandRecord, retained_from: u64) -> Value {
    json!({
        "id": command.id,
        "directory": command.directory,
        "outcome": outcome_name(command.outcome()),
        "exit_status": command.exit,
        "elapsed_ms": command.elapsed().as_millis() as u64,
        "output_cursor": command.output_start,
        "output_end": command.output_end,
        "output_complete": command.output_start >= retained_from,
    })
}

/// A session's state as a waiter off the UI thread reads it.
pub(crate) struct Probe {
    pub target: String,
    pub state: Arc<Mutex<TerminalState>>,
}

impl State {
    /// The index of the session `target` names, if this client may use it.
    pub(crate) fn find_terminal(
        &mut self,
        grant: &ClientGrant,
        target: &str,
    ) -> Result<usize, ToolError> {
        let unknown = || ToolError::new(ErrorCode::UnknownTarget, format!("No terminal {target}"));
        let rest = target.strip_prefix("term_").ok_or_else(unknown)?;
        let (instance, id) = rest.rsplit_once('_').ok_or_else(unknown)?;
        if instance != self.automation.instance {
            return Err(ToolError::new(
                ErrorCode::UnknownTarget,
                "That terminal ID is from an earlier Rodtty run; call terminal_list",
            ));
        }
        let id: usize = id.parse().map_err(|_| unknown())?;
        let Some(index) = self.sessions.iter().position(|s| s.id() == id) else {
            return Err(if crate::session::issued(id) {
                ToolError::new(
                    ErrorCode::ClosedTarget,
                    format!("Terminal {target} has closed"),
                )
            } else {
                unknown()
            });
        };
        let owner = self.session_owner(index);
        grant.check(None, Some(&owner.workspace))?;
        Ok(index)
    }

    fn terminal_row(&mut self, index: usize) -> Value {
        let owner = self.session_owner(index);
        let session = &self.sessions[index];
        let target = self.terminal_target(session);
        let (rows, cols) = session.size();
        let (shell_pid, foreground_pid) = session.processes();
        let state = session.state.lock().unwrap_or_else(|e| e.into_inner());
        let exit = state.exit;
        let cursor = state.output.end();
        let busy = exit.is_none() && state.parser.callbacks().command_running();
        drop(state);
        let running = exit.is_none() && session.has_running_commands();
        json!({
            "id": target,
            "title": session.tab_title(),
            "cwd": session.cwd(),
            "workspace": owner.workspace,
            "created_by": if owner.client.is_some() { "agent" } else { "owner" },
            "visible": index == self.active,
            "state": match (exit, busy || running) {
                (Some(_), _) => "exited",
                (None, true) => "busy",
                (None, false) => "idle",
            },
            "exit_status": exit,
            "rows": rows,
            "cols": cols,
            "shell_pid": shell_pid,
            "foreground_pid": foreground_pid,
            "output_cursor": cursor,
            "taken_over": self.automation.takeovers.contains(&target),
        })
    }

    pub(crate) fn auto_terminal_list(&mut self, grant: &ClientGrant) -> ToolResult {
        let mut rows = Vec::new();
        for index in 0..self.sessions.len() {
            let owner = self.session_owner(index);
            if grant.allows_workspace(&owner.workspace) {
                rows.push(self.terminal_row(index));
            }
        }
        Ok(json!({"terminals": rows, "cursor": self.automation.shared.hub.cursor()}))
    }

    /// Start a session for an agent in an authorized workspace, without
    /// changing the owner's visible tab unless `show` is set.
    pub(crate) fn spawn_agent_session(
        &mut self,
        grant: &ClientGrant,
        args: &Value,
    ) -> Result<usize, ToolError> {
        let (workspace, root) = match arg_opt_str(args, "workspace")? {
            Some(id) => {
                let registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
                let (active, active_root) = self.active_workspace();
                match registry.get(id) {
                    Some(w) => (w.id.clone(), w.root.clone()),
                    None if id == active => (active, active_root),
                    None => {
                        return Err(ToolError::new(
                            ErrorCode::UnknownTarget,
                            format!("No workspace {id}"),
                        ));
                    }
                }
            }
            None => self.active_workspace(),
        };
        let cwd = match arg_opt_str(args, "cwd")? {
            Some(cwd) => {
                let cwd = PathBuf::from(cwd);
                if !cwd.is_absolute() {
                    return Err(ToolError::invalid("`cwd` must be an absolute path"));
                }
                cwd
            }
            None => root.clone(),
        };
        // The directory decides the workspace: a cwd outside the named one
        // must itself be authorized.
        let (cwd_workspace, _) = {
            use crate::paths::Canonical as _;
            let canonical = cwd
                .canonical()
                .map_err(|e| ToolError::invalid(format!("Can't open {}: {e}", cwd.display())))?;
            let explicit = arg_opt_str(args, "workspace")?.is_some();
            let (specific, specific_root) = self.workspace_of(&canonical);
            let registered = crate::workspaces::Registry::load(&self.root)
                .unwrap_or_default()
                .get(&specific)
                .is_some();
            // The most specific registered workspace owns the session,
            // unless the client named the workspace it belongs in.
            if canonical.starts_with(&root) && (explicit || !registered) {
                (workspace.clone(), root.clone())
            } else {
                (specific, specific_root)
            }
        };
        grant.check(None, Some(&cwd_workspace))?;
        let index = self.spawn_session(cwd).map_err(|message| {
            ToolError::new(
                ErrorCode::Internal,
                if message.is_empty() {
                    "Fix Rodtty's settings before starting a session".to_owned()
                } else {
                    message
                },
            )
        })?;
        let id = self.sessions[index].id();
        self.automation.owners.insert(
            id,
            SessionOwner {
                workspace: cwd_workspace,
                client: Some(grant.id.clone()),
            },
        );
        if arg_bool(args, "show", false)? {
            self.active = index;
            self.workspace.note_session_switch();
            self.selection = None;
        }
        let target = self.terminal_target(&self.sessions[index]);
        self.emit(
            "session",
            Some(&target),
            json!({"state": "created", "client": grant.id}),
        );
        Ok(index)
    }

    pub(crate) fn auto_terminal_create(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let index = self.spawn_agent_session(grant, args)?;
        Ok(self.terminal_row(index))
    }

    pub(crate) fn auto_terminal_screen(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let index = self.find_terminal(grant, arg_str(args, "terminal")?)?;
        let row = self.terminal_row(index);
        let session = &self.sessions[index];
        let state = session.state.lock().unwrap_or_else(|e| e.into_inner());
        let screen = state.parser.screen();
        let (rows, cols) = screen.size();
        let lines: Vec<String> = screen.rows(0, cols).collect();
        let (cursor_row, cursor_col) = screen.cursor_position();
        let retained = state.output.start();
        let commands: Vec<Value> = state
            .parser
            .callbacks()
            .commands
            .iter()
            .rev()
            .take(5)
            .map(|c| command_json(c, retained))
            .collect();
        Ok(json!({
            "terminal": row["id"],
            "rows": rows,
            "cols": cols,
            "lines": lines,
            "cursor": {"row": cursor_row, "col": cursor_col, "visible": !screen.hide_cursor()},
            "revision": state.generation,
            "title": row["title"],
            "cwd": row["cwd"],
            "state": row["state"],
            "exit_status": state.exit,
            "alternate_screen": screen.alternate_screen(),
            "bracketed_paste": screen.bracketed_paste(),
            "application_cursor": screen.application_cursor(),
            "mouse_reporting": screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None,
            "output_cursor": state.output.end(),
            "recent_commands": commands,
            "taken_over": row["taken_over"],
        }))
    }

    pub(crate) fn auto_terminal_read(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let index = self.find_terminal(grant, arg_str(args, "terminal")?)?;
        let max = (arg_u64(args, "max_bytes", 65_536)? as usize).clamp(1, MAX_READ);
        let state = self.sessions[index]
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner());
        let cursor = match args.get("cursor") {
            Some(Value::Number(n)) => n
                .as_u64()
                .ok_or_else(|| ToolError::invalid("`cursor` must be a non-negative integer"))?,
            _ => state.output.end().saturating_sub(max as u64),
        };
        let (from, bytes, next) = state.output.read(cursor, max);
        Ok(json!({
            "text": crate::ansi::strip(&bytes),
            "cursor": from,
            "next_cursor": next,
            "truncated": from > cursor,
            "more": next < state.output.end(),
            "retained_from": state.output.start(),
            "exit_status": state.exit,
        }))
    }

    /// Check that agent input may reach `target`, and note the client as
    /// its current agent.
    pub(crate) fn agent_input(&mut self, client: &str, target: &str) -> Result<(), ToolError> {
        if self.automation.takeovers.contains(target) {
            return Err(ToolError::new(
                ErrorCode::OwnerTakeover,
                "The owner took over this target. Agent input resumes after the owner returns it.",
            ));
        }
        self.automation
            .agent_focus
            .insert(target.to_owned(), client.to_owned());
        Ok(())
    }

    fn agent_send(
        &mut self,
        grant: &ClientGrant,
        target: &str,
        bytes: Vec<u8>,
    ) -> Result<usize, ToolError> {
        let index = self.find_terminal(grant, target)?;
        self.agent_input(&grant.id, target)?;
        let session = &self.sessions[index];
        if session
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exit
            .is_some()
        {
            return Err(ToolError::new(
                ErrorCode::ClosedTarget,
                "The session has ended",
            ));
        }
        session
            .send(bytes)
            .map_err(|e| ToolError::new(ErrorCode::ClosedTarget, format!("{e:#}")))?;
        Ok(index)
    }

    pub(crate) fn auto_terminal_text(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let target = arg_str(args, "terminal")?;
        let text = arg_str(args, "text")?;
        if text.len() > 65_536 {
            return Err(ToolError::new(
                ErrorCode::Limit,
                "Text is limited to 64 KiB per call",
            ));
        }
        let index = self.find_terminal(grant, target)?;
        self.agent_input(&grant.id, target)?;
        let session = &self.sessions[index];
        let bracketed = session.with_screen(vt100::Screen::bracketed_paste);
        let multiline = text.contains('\n') || text.contains('\r');
        // A single line types as is. Multiline text is one paste: bracketed
        // when the application asked, so it arrives without submitting.
        let result = if multiline || bracketed {
            session.paste(text)
        } else {
            session.send(text.as_bytes().to_vec())
        };
        result.map_err(|e| ToolError::new(ErrorCode::ClosedTarget, format!("{e:#}")))?;
        let cursor = session
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .output
            .end();
        self.emit(
            "input",
            Some(target),
            json!({"kind": "text", "bytes": text.len(), "client": grant.id}),
        );
        Ok(
            json!({"terminal": target, "delivered": "text", "bracketed_paste": bracketed && (multiline || bracketed), "multiline": multiline, "output_cursor": cursor}),
        )
    }

    pub(crate) fn auto_terminal_keys(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let target = arg_str(args, "terminal")?;
        let names = args
            .get("keys")
            .and_then(Value::as_array)
            .ok_or_else(|| ToolError::invalid("`keys` must be an array of key names"))?;
        let index = self.find_terminal(grant, target)?;
        let application_cursor =
            self.sessions[index].with_screen(vt100::Screen::application_cursor);
        let mut bytes = Vec::new();
        for name in names {
            let name = name
                .as_str()
                .ok_or_else(|| ToolError::invalid("Each key must be a string"))?;
            let chord = keys::parse(name)?;
            bytes.extend(
                input::terminal_bytes(&chord, application_cursor).ok_or_else(|| {
                    ToolError::new(
                        ErrorCode::Unsupported,
                        format!("{name} has no terminal encoding"),
                    )
                })?,
            );
        }
        let index = self.agent_send(grant, target, bytes)?;
        let cursor = self.sessions[index]
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .output
            .end();
        self.emit(
            "input",
            Some(target),
            json!({"kind": "keys", "count": names.len(), "client": grant.id}),
        );
        Ok(json!({"terminal": target, "pressed": names, "output_cursor": cursor}))
    }

    pub(crate) fn auto_terminal_interrupt(
        &mut self,
        grant: &ClientGrant,
        args: &Value,
    ) -> ToolResult {
        let target = arg_str(args, "terminal")?;
        self.agent_send(grant, target, vec![0x03])?;
        self.emit(
            "input",
            Some(target),
            json!({"kind": "interrupt", "client": grant.id}),
        );
        Ok(json!({"terminal": target, "interrupted": true}))
    }

    pub(crate) fn auto_terminal_close(
        &mut self,
        grant: &ClientGrant,
        args: &Value,
        tx: &mut Tx,
    ) -> ToolResult {
        let target = arg_str(args, "terminal")?.to_owned();
        let index = self.find_terminal(grant, &target)?;
        let session = &self.sessions[index];
        let id = session.id();
        let state = session.state.clone();
        let confirm = match self.config.behavior.terminal_tab_close {
            crate::config::TerminalClosePolicy::ConfirmRunning => session.has_running_commands(),
            crate::config::TerminalClosePolicy::AlwaysConfirm => true,
            crate::config::TerminalClosePolicy::CloseImmediately => false,
        };
        if confirm {
            if self.pending.is_some() {
                return Err(ToolError::new(
                    ErrorCode::Conflict,
                    "The owner is answering another prompt; try again later",
                ));
            }
            // The owner decides, in the same prompt the tab's close button opens.
            self.pending = Some(Pending::Tab(id));
            if let Some(handle) = &self.handle {
                handle.show();
            }
            self.emit(
                "approval",
                Some(&target),
                json!({"kind": "close_terminal", "state": "waiting_for_owner"}),
            );
            return Err(ToolError::new(
                ErrorCode::WaitingForOwner,
                "The session has a running command; Rodtty asked the owner to confirm closing it",
            ));
        }
        self.pending = Some(Pending::Tab(id));
        self.confirm(tx);
        self.automation.shared.bump(&target);
        self.automation.agent_focus.remove(&target);
        self.emit(
            "session",
            Some(&target),
            json!({"state": "closed", "client": grant.id}),
        );
        let exit = state.lock().unwrap_or_else(|e| e.into_inner()).exit;
        Ok(json!({"terminal": target, "closed": true, "exit_status": exit}))
    }

    pub(crate) fn auto_terminal_probe(
        &mut self,
        grant: &ClientGrant,
        target: &str,
    ) -> Result<Probe, ToolError> {
        let index = self.find_terminal(grant, target)?;
        Ok(Probe {
            target: target.to_owned(),
            state: self.sessions[index].state.clone(),
        })
    }

    pub(crate) fn auto_terminal_capture(
        &mut self,
        grant: &ClientGrant,
        target: &str,
    ) -> Result<crate::desktop::capture::TerminalShot, ToolError> {
        grant.check(Some(Scope::Capture), None)?;
        let index = self.find_terminal(grant, target)?;
        Ok(crate::desktop::capture::TerminalShot {
            target: target.to_owned(),
            screen: self.sessions[index].screen(),
            palette: crate::desktop::capture::Palette::from_state(self),
        })
    }
}

impl State {
    /// A finished run of `session`'s command `id`, for comparison.
    pub(crate) fn command_run(session: &Session, id: u64) -> Option<crate::compare::Run> {
        let command = session
            .commands()
            .into_iter()
            .find(|c| c.id == id && c.ended.is_some())?;
        let (text, complete) = session.command_output(id)?;
        Some(crate::compare::Run {
            command: id,
            outcome: outcome_name(command.outcome()).into(),
            exit_status: command.exit,
            elapsed_ms: command.elapsed().as_millis() as u64,
            complete,
            text,
        })
    }

    pub(crate) fn auto_terminal_compare(
        &mut self,
        grant: &ClientGrant,
        args: &Value,
    ) -> ToolResult {
        let index = self.find_terminal(grant, arg_str(args, "terminal")?)?;
        let session = &self.sessions[index];
        let finished: Vec<u64> = session
            .commands()
            .iter()
            .filter(|c| c.ended.is_some())
            .map(|c| c.id)
            .collect();
        let pick = |key: &str, from_end: usize| -> Result<u64, ToolError> {
            match args.get(key).and_then(Value::as_u64) {
                Some(id) => Ok(id),
                None => finished.iter().rev().nth(from_end).copied().ok_or_else(|| {
                    ToolError::new(ErrorCode::UnknownTarget, "Fewer than two finished commands")
                }),
            }
        };
        let (before, after) = (pick("before", 1)?, pick("after", 0)?);
        let run = |id| {
            Self::command_run(session, id).ok_or_else(|| {
                ToolError::new(
                    ErrorCode::UnknownTarget,
                    format!("Command {id} is not a finished, retained run"),
                )
            })
        };
        Ok(json!(crate::compare::compare(run(before)?, run(after)?)))
    }
}

/// POSIX single quotes, as Brush reads them on every platform.
pub(crate) fn quote(word: &str) -> String {
    if !word.is_empty()
        && word
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_./=:,+@%".contains(&b))
    {
        return word.to_owned();
    }
    format!("'{}'", word.replace('\'', r"'\''"))
}

/// Wait off the UI thread until `done` holds for the session's state.
fn poll(
    probe: &Probe,
    deadline: Instant,
    cancel: &AtomicBool,
    mut done: impl FnMut(&TerminalState) -> Option<Value>,
) -> ToolResult {
    loop {
        {
            let state = probe.state.lock().unwrap_or_else(|e| e.into_inner());
            if let Some(value) = done(&state) {
                return Ok(value);
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ToolError::new(
                ErrorCode::Cancelled,
                "The wait was cancelled; the program keeps running",
            ));
        }
        if Instant::now() >= deadline {
            let state = probe.state.lock().unwrap_or_else(|e| e.into_inner());
            return Err(ToolError::new(
                ErrorCode::Timeout,
                "The condition did not happen before timeout_ms",
            )
            .with_data(json!({
                "revision": state.generation,
                "output_cursor": state.output.end(),
                "exit_status": state.exit,
                "screen": state.parser.screen().contents(),
            })));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(super) fn wait(
    host: &Host,
    grant: &ClientGrant,
    args: &Value,
    cancel: &AtomicBool,
) -> ToolResult {
    let target = arg_str(args, "terminal")?.to_owned();
    let timeout = Duration::from_millis(arg_u64(args, "timeout_ms", 30_000)?.min(600_000));
    let contains = arg_opt_str(args, "contains")?.map(str::to_owned);
    let screen_contains = arg_opt_str(args, "screen_contains")?.map(str::to_owned);
    let regex = match arg_opt_str(args, "regex")? {
        Some(pattern) => Some(
            regex_lite::Regex::new(pattern)
                .map_err(|e| ToolError::invalid(format!("Invalid regex: {e}")))?,
        ),
        None => None,
    };
    let changed_since = args.get("changed_since_revision").and_then(Value::as_u64);
    let idle = arg_bool(args, "idle", false)?;
    let exit = arg_bool(args, "exit", false)?;
    if contains.is_none()
        && regex.is_none()
        && screen_contains.is_none()
        && changed_since.is_none()
        && !idle
        && !exit
    {
        return Err(ToolError::invalid(
            "Name a condition: contains, regex, screen_contains, changed_since_revision, idle or exit",
        ));
    }
    let g = grant.clone();
    let t = target.clone();
    let probe = on_ui(&host.handle, move |s, _| s.auto_terminal_probe(&g, &t))??;
    let start_cursor = match args.get("cursor") {
        Some(Value::Number(n)) => n.as_u64().unwrap_or(0),
        _ => probe
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .output
            .end(),
    };
    let (job, job_cancel) = host
        .shared
        .hub
        .start_job("terminal_wait", &grant.id, Some(&target));
    let either = CancelEither(cancel, &job_cancel);
    let result = poll(&probe, Instant::now() + timeout, &either, |state| {
        let ended = state.exit.is_some();
        let (from, bytes, next) = state.output.read(start_cursor, usize::MAX);
        let text = || crate::ansi::strip(&bytes);
        let mut matched: Option<&str> = None;
        let mut found = None;
        if let Some(needle) = &contains
            && text().contains(needle.as_str())
        {
            matched = Some("contains");
        }
        if matched.is_none()
            && let Some(regex) = &regex
            && let Some(m) = regex.find(&text())
        {
            found = Some(m.as_str().to_owned());
            matched = Some("regex");
        }
        if matched.is_none()
            && let Some(needle) = &screen_contains
            && state.parser.screen().contents().contains(needle.as_str())
        {
            matched = Some("screen_contains");
        }
        if matched.is_none() && changed_since.is_some_and(|r| state.generation > r) {
            matched = Some("screen_changed");
        }
        if matched.is_none()
            && idle
            && !state.parser.callbacks().command_running()
            && state
                .parser
                .callbacks()
                .commands
                .back()
                .is_some_and(|c| c.ended.is_some())
        {
            matched = Some("idle");
        }
        if matched.is_none() && (exit || ended) && ended {
            matched = Some("exited");
        }
        let matched = matched?;
        let last = state
            .parser
            .callbacks()
            .commands
            .back()
            .map(|c| command_json(c, state.output.start()));
        Some(json!({
            "terminal": probe.target,
            "matched": matched,
            "match": found,
            "cursor": from,
            "next_cursor": next,
            "truncated": from > start_cursor,
            "revision": state.generation,
            "exit_status": state.exit,
            "last_command": last,
        }))
    });
    super::job_finished(&host.shared, &job, result.clone());
    result.map(|mut v| {
        v["job"] = json!(job);
        v
    })
}

/// Cancelled when either flag is set: the request's own, or its job's.
struct CancelEither<'a>(&'a AtomicBool, &'a AtomicBool);
impl std::ops::Deref for CancelEither<'_> {
    type Target = AtomicBool;
    fn deref(&self) -> &AtomicBool {
        if self.1.load(Ordering::Relaxed) {
            self.1
        } else {
            self.0
        }
    }
}

/// Open a terminal and run a program in it once its shell is ready.
pub(super) fn launch(host: &Host, grant: &ClientGrant, args: &Value) -> ToolResult {
    let executable = arg_str(args, "executable")?.to_owned();
    if executable.is_empty() || executable.contains(['\n', '\r', '\0']) {
        return Err(ToolError::invalid(
            "`executable` must be a single program name or path",
        ));
    }
    let arguments: Vec<String> = match args.get("args") {
        None | Some(Value::Null) => Vec::new(),
        Some(Value::Array(items)) => items
            .iter()
            .map(|a| {
                a.as_str()
                    .map(str::to_owned)
                    .ok_or_else(|| ToolError::invalid("`args` must be strings"))
            })
            .collect::<Result<_, _>>()?,
        Some(_) => return Err(ToolError::invalid("`args` must be an array of strings")),
    };
    if arguments.iter().any(|a| a.contains('\0')) {
        return Err(ToolError::invalid("Arguments can't contain NUL"));
    }
    let title = arg_opt_str(args, "title")?.map(str::to_owned);
    let g = grant.clone();
    let a = args.clone();
    let (target, probe) = on_ui(&host.handle, move |s, _| {
        let index = s.spawn_agent_session(&g, &a)?;
        let target = s.terminal_target(&s.sessions[index]);
        Ok::<_, ToolError>((
            target.clone(),
            Probe {
                target,
                state: s.sessions[index].state.clone(),
            },
        ))
    })??;
    let (job, job_cancel) = host
        .shared
        .hub
        .start_job("terminal_launch", &grant.id, Some(&target));
    // The shell reports its first prompt with an activity marker; type the
    // command once it is reading input.
    let ready = poll(
        &probe,
        Instant::now() + Duration::from_secs(15),
        &AtomicBool::new(false),
        |state| {
            (!state.parser.callbacks().command_running() || state.exit.is_some())
                .then(|| json!(state.exit))
        },
    );
    if let Err(error) = ready {
        super::job_finished(&host.shared, &job, Err(error.clone()));
        return Err(error);
    }
    let mut line = String::new();
    if let Some(title) = &title {
        line.push_str(&format!("printf '\\033]0;%s\\007' {}; ", quote(title)));
    }
    line.push_str(&quote(&executable));
    for argument in &arguments {
        line.push(' ');
        line.push_str(&quote(argument));
    }
    line.push('\r');
    let commands_before = probe
        .state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .parser
        .callbacks()
        .commands
        .back()
        .map_or(0, |c| c.id);
    let g = grant.clone();
    let t = target.clone();
    on_ui(&host.handle, move |s, _| -> Result<(), ToolError> {
        let index = s.find_terminal(&g, &t)?;
        s.sessions[index]
            .send(line.into_bytes())
            .map_err(|e| ToolError::new(ErrorCode::ClosedTarget, format!("{e:#}")))?;
        s.automation.agent_focus.insert(t.clone(), g.id.clone());
        Ok(())
    })??;
    // Wait briefly for the program to start so its process ID is known.
    let _ = poll(
        &probe,
        Instant::now() + Duration::from_secs(3),
        &AtomicBool::new(false),
        |state| {
            state
                .parser
                .callbacks()
                .commands
                .back()
                .filter(|c| c.id > commands_before)
                .map(|_| json!(true))
        },
    );
    let g = grant.clone();
    let t = target.clone();
    let row = on_ui(&host.handle, move |s, _| -> ToolResult {
        let index = s.find_terminal(&g, &t)?;
        Ok(s.terminal_row(index))
    })??;
    // The job follows the program until it returns to the prompt or the session ends.
    let shared = host.shared.clone();
    let job_id = job.clone();
    let watch = Probe {
        target: target.clone(),
        state: probe.state.clone(),
    };
    std::thread::spawn(move || {
        let result = poll(
            &watch,
            Instant::now() + Duration::from_secs(24 * 3600),
            &job_cancel,
            |state| {
                let command = state
                    .parser
                    .callbacks()
                    .commands
                    .iter()
                    .find(|c| c.id > commands_before);
                match command {
                    Some(c) if c.ended.is_some() => Some(command_json(c, state.output.start())),
                    _ if state.exit.is_some() => {
                        Some(json!({"session_exit_status": state.exit, "outcome": "unknown"}))
                    }
                    _ => None,
                }
            },
        );
        if let Ok(value) = &result {
            shared
                .hub
                .emit("command", Some(&watch.target), value.clone());
        }
        super::job_finished(&shared, &job_id, result);
    });
    let command: Vec<String> = std::iter::once(executable).chain(arguments).collect();
    Ok(json!({
        "terminal": target,
        "job": job,
        "command": command,
        "shell_pid": row["shell_pid"],
        "foreground_pid": row["foreground_pid"],
        "cwd": row["cwd"],
        "workspace": row["workspace"],
    }))
}

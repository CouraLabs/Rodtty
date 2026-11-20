//! The desktop end of automation: the local channel's handler, owner
//! controls (approve, pause, revoke, take over), and the tools, which run on
//! the UI thread against `State` with the same methods the UI uses.
//!
//! Agent input goes straight to the targeted PTY or browser. Nothing here
//! injects operating-system input, moves the physical pointer, reads or
//! replaces the clipboard, or activates the window unless a tool asks to.
use super::*;
use crate::automation::{
    ErrorCode, ToolError, ToolResult,
    channel::{self, Handler},
    events::{Begin, Hub, JobState},
    grants::{ClientGrant, Grants, Scope, Status},
    tools,
};
use serde_json::{Value, json};
use std::{
    collections::{HashMap, HashSet},
    path::Path,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

mod editor;
pub(crate) mod input;
mod terminal;
#[cfg(test)]
mod tests;

pub(crate) use input::VirtualInput;

/// Who opened a session and which workspace it belongs to, fixed when
/// automation first sees it.
#[derive(Clone, Debug)]
pub(crate) struct SessionOwner {
    pub workspace: String,
    pub root: PathBuf,
    /// The client that created it, or `None` for the owner's own sessions.
    pub client: Option<String>,
}

/// State shared with the channel's threads.
pub(crate) struct Shared {
    pub hub: Hub,
    /// Bumped per target when queued agent input must stop: owner takeover,
    /// pause, revocation, disconnect or closure. A typing sequence checks it
    /// between keys.
    epochs: Mutex<HashMap<String, u64>>,
}

impl Shared {
    pub fn epoch(&self, target: &str) -> u64 {
        *self
            .epochs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(target)
            .unwrap_or(&0)
    }
    pub fn bump(&self, target: &str) {
        *self
            .epochs
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .entry(target.to_owned())
            .or_default() += 1;
    }
}

pub(crate) struct Automation {
    pub shared: Arc<Shared>,
    /// Distinguishes this run's target IDs from a previous run's.
    pub instance: String,
    pub server: Option<channel::Server>,
    pub owners: HashMap<usize, SessionOwner>,
    /// Targets the owner took over: agent input is refused until returned.
    pub takeovers: HashSet<String>,
    /// The client whose input a target last received.
    pub agent_focus: HashMap<String, String>,
    pub inputs: HashMap<(String, String), VirtualInput>,
    /// Grant statuses as last seen, to notice pauses and revocations made
    /// outside this process.
    statuses: HashMap<String, Status>,
    grants_seen: Option<std::time::SystemTime>,
    grants_poll: Instant,
    /// Pending client approvals, oldest first, for the footer indicator.
    pub pending: Vec<ClientGrant>,
    pub active_clients: usize,
}

impl Default for Automation {
    fn default() -> Self {
        Self {
            shared: Arc::new(Shared {
                hub: Hub::default(),
                epochs: Default::default(),
            }),
            instance: crate::automation::token()[..8].to_owned(),
            server: None,
            owners: Default::default(),
            takeovers: Default::default(),
            agent_focus: Default::default(),
            inputs: Default::default(),
            statuses: Default::default(),
            grants_seen: None,
            grants_poll: Instant::now(),
            pending: Vec::new(),
            active_clients: 0,
        }
    }
}

/// Features this build and instance offer, for capability discovery.
pub(crate) fn features() -> Vec<String> {
    #[allow(unused_mut)]
    let mut features = vec![
        "terminal".to_owned(),
        "workspaces".to_owned(),
        "editor".to_owned(),
        "git".to_owned(),
    ];
    #[cfg(feature = "browser")]
    if crate::desktop::browser::available() {
        features.push("browser".into());
        features.push("recording".into());
    }
    if crate::desktop::codex::available() {
        features.push("codex".into());
    }
    features
}

/// Run `f` on the UI thread and wait for its answer.
pub(crate) fn on_ui<R: Send + 'static>(
    handle: &rune::AppHandle<State>,
    f: impl FnOnce(&mut State, &mut Tx) -> R + Send + 'static,
) -> Result<R, ToolError> {
    let (sender, receiver) = mpsc::channel();
    handle.update(move |state, tx| {
        let _ = sender.send(f(state, tx));
    });
    receiver.recv_timeout(Duration::from_secs(15)).map_err(|_| {
        ToolError::new(
            ErrorCode::Timeout,
            "Rodtty's UI thread didn't answer in time",
        )
    })
}

/// The channel handler: authorizes each call against the owner's grants,
/// makes mutations retry-safe, and runs tools.
pub(crate) struct Host {
    pub handle: rune::AppHandle<State>,
    pub shared: Arc<Shared>,
    pub root: PathBuf,
    pub instance: String,
}

impl Host {
    fn grant(&self, client: &str) -> Result<ClientGrant, ToolError> {
        let grants = Grants::load(&self.root)
            .map_err(|e| ToolError::new(ErrorCode::Internal, format!("{e:#}")))?;
        grants.get(client).cloned().ok_or_else(|| {
            ToolError::new(
                ErrorCode::Revoked,
                "This client is no longer known; reconnect to request approval",
            )
        })
    }
}

impl Handler for Host {
    fn hello(&self, name: &str) -> ToolResult {
        let name: String = name.chars().filter(|c| !c.is_control()).take(64).collect();
        let mut grants = Grants::load(&self.root)
            .map_err(|e| ToolError::new(ErrorCode::Internal, format!("{e:#}")))?;
        let (grant, created) = grants.request(&name);
        if created {
            grants
                .save(&self.root)
                .map_err(|e| ToolError::new(ErrorCode::Internal, format!("{e:#}")))?;
            self.shared.hub.emit(
                "client",
                Some(&grant.id),
                json!({"status": "pending", "name": grant.name}),
            );
            self.handle.update(|s, _| s.automation_refresh_grants(true));
        }
        Ok(json!({
            "client": grant.id,
            "name": grant.name,
            "status": grant.status,
            "scopes": grant.scopes,
            "instance": self.instance,
            "features": features(),
        }))
    }

    fn call(&self, client: &str, name: &str, args: Value, cancel: Arc<AtomicBool>) -> ToolResult {
        let Some(tool) = tools::find(name) else {
            return Err(ToolError::new(
                ErrorCode::UnknownTool,
                format!("No tool named {name}"),
            ));
        };
        let grant = self.grant(client)?;
        if let Some(feature) = tool.feature
            && !features().iter().any(|f| f == feature)
        {
            return Err(ToolError::new(
                ErrorCode::Unavailable,
                format!("This Rodtty build or instance has no `{feature}` support"),
            ));
        }
        if tool.scope.is_some() {
            grant.check(tool.scope, None)?;
        }
        if !args.is_object() {
            return Err(ToolError::invalid("Arguments must be an object"));
        }
        let request = match args.get("request_id") {
            Some(Value::String(id)) if tool.mutating && !id.is_empty() && id.len() <= 200 => {
                let mut fingerprint = args.clone();
                fingerprint.as_object_mut().map(|o| o.remove("request_id"));
                let key = format!("{client}/{name}/{id}");
                match self.shared.hub.begin(&key, &fingerprint.to_string())? {
                    Begin::Done(result) => {
                        return result.map(|mut v| {
                            if let Some(o) = v.as_object_mut() {
                                o.insert("replayed".into(), json!(true));
                            }
                            v
                        });
                    }
                    Begin::New => Some(key),
                }
            }
            Some(Value::String(_)) if tool.mutating => {
                return Err(ToolError::invalid(
                    "`request_id` must be 1 to 200 characters",
                ));
            }
            Some(_) if tool.mutating => {
                return Err(ToolError::invalid("`request_id` must be a string"));
            }
            _ => None,
        };
        let result = self.run(&grant, name, args, cancel);
        if let Some(key) = request {
            self.shared.hub.finish(&key, &result);
        }
        result
    }

    fn disconnected(&self, client: &str) {
        let client = client.to_owned();
        self.handle
            .update(move |s, _| s.automation_release_client(&client, "disconnected"));
    }
}

impl Host {
    fn run(
        &self,
        grant: &ClientGrant,
        name: &str,
        args: Value,
        cancel: Arc<AtomicBool>,
    ) -> ToolResult {
        let g = grant.clone();
        macro_rules! ui {
            (|$s:ident, $tx:ident| $body:expr) => {{
                let g = g.clone();
                let args = args.clone();
                #[allow(unused_variables)]
                on_ui(&self.handle, move |$s: &mut State, $tx: &mut Tx| {
                    let _ = (&g, &args);
                    $body
                })?
            }};
        }
        match name {
            "rodtty_status" => {
                let ui = ui!(|s, tx| s.automation_status_summary());
                Ok(json!({
                    "connected": true,
                    "instance": self.instance,
                    "client": {"id": grant.id, "name": grant.name, "status": grant.status, "scopes": grant.scopes, "workspaces": grant.workspaces},
                    "features": features(),
                    "scopes": Scope::ALL.iter().map(|s| s.key()).collect::<Vec<_>>(),
                    "approval": match grant.status {
                        Status::Pending => "Waiting for the owner. In Rodtty run Automation: Approve Pending Client from the command palette, or `rodtty automation approve` in a shell.",
                        Status::Approved => "Approved",
                        Status::Paused => "Paused by the owner",
                        Status::Revoked => "Revoked by the owner",
                    },
                    "rodtty": ui,
                    "version": env!("CARGO_PKG_VERSION"),
                }))
            }
            "workspace_list" => ui!(|s, tx| s.auto_workspace_list(&g)),
            "workspace_switch" => ui!(|s, tx| s.auto_workspace_switch(&g, &args, tx)),
            "window_show" => {
                let focus = crate::automation::arg_bool(&args, "focus", false)?;
                if focus {
                    self.handle.open();
                } else {
                    self.handle.show();
                }
                Ok(json!({"shown": true, "focused": focus}))
            }
            "app_action" => ui!(|s, tx| s.auto_app_action(&args, tx)),
            "terminal_list" => ui!(|s, tx| s.auto_terminal_list(&g)),
            "terminal_create" => ui!(|s, tx| s.auto_terminal_create(&g, &args)),
            "terminal_launch" => terminal::launch(self, grant, &args),
            "terminal_split" => Err(ToolError::new(
                ErrorCode::Unavailable,
                "Split terminals are not available in this build",
            )),
            "terminal_screen" => ui!(|s, tx| s.auto_terminal_screen(&g, &args)),
            "terminal_read" => ui!(|s, tx| s.auto_terminal_read(&g, &args)),
            "terminal_wait" => terminal::wait(self, grant, &args, &cancel),
            "terminal_send_text" => ui!(|s, tx| s.auto_terminal_text(&g, &args)),
            "terminal_send_keys" => ui!(|s, tx| s.auto_terminal_keys(&g, &args)),
            "terminal_interrupt" => ui!(|s, tx| s.auto_terminal_interrupt(&g, &args)),
            "terminal_close" => ui!(|s, tx| s.auto_terminal_close(&g, &args, tx)),
            "input_state" => ui!(|s, tx| s.auto_input_state(&g, &args)),
            "input_key" => ui!(|s, tx| s.auto_input_key(&g, &args)),
            "input_type" => input::type_text(self, grant, &args, &cancel),
            "input_pointer" => input::pointer(self, grant, &args, &cancel),
            "input_release_all" => ui!(|s, tx| s.auto_input_release(&g, &args)),
            "events_read" => {
                let cursor = match args.get("cursor") {
                    Some(Value::Number(n)) => n.as_u64().ok_or_else(|| {
                        ToolError::invalid("`cursor` must be a non-negative integer")
                    })?,
                    _ => self.shared.hub.cursor(),
                };
                let max = crate::automation::arg_u64(&args, "max", 100)?.clamp(1, 500) as usize;
                let wait = crate::automation::arg_u64(&args, "wait_ms", 0)?.min(60_000);
                let mut read =
                    self.shared
                        .hub
                        .read(cursor, max, Duration::from_millis(wait), &cancel);
                // Events about targets outside this client's workspaces stay hidden.
                let visible = ui!(|s, tx| s.automation_visible_targets(&g));
                if let Some(events) = read.get_mut("events").and_then(Value::as_array_mut) {
                    events.retain(|e| match e.get("target").and_then(Value::as_str) {
                        Some(t) if t.starts_with("term_") || t.starts_with("brw_") => {
                            visible.contains(t)
                        }
                        Some(t) if t.starts_with("cl_") => t == grant.id,
                        Some(t) if t.starts_with("job_") => {
                            self.shared.hub.job(t).is_some_and(|j| j.client == grant.id)
                        }
                        _ => true,
                    });
                }
                Ok(read)
            }
            "job_status" => match crate::automation::arg_opt_str(&args, "job")? {
                Some(id) => self.own_job(grant, id).map(|j| json!(j)),
                None => Ok(json!({"jobs": self.shared.hub.jobs_for(&grant.id)})),
            },
            "job_wait" => {
                let id = crate::automation::arg_str(&args, "job")?;
                self.own_job(grant, id)?;
                let wait = crate::automation::arg_u64(&args, "timeout_ms", 30_000)?.min(600_000);
                let job = self
                    .shared
                    .hub
                    .wait_job(id, Duration::from_millis(wait), &cancel);
                if cancel.load(Ordering::Relaxed) {
                    return Err(ToolError::new(
                        ErrorCode::Cancelled,
                        "The wait was cancelled; the job continues",
                    ));
                }
                Ok(json!(job))
            }
            "job_cancel" => {
                let id = crate::automation::arg_str(&args, "job")?;
                self.own_job(grant, id)?;
                self.shared.hub.cancel_job(id);
                Ok(json!(self.shared.hub.job(id)))
            }
            "files_search" => editor::search(self, grant, &args, &cancel),
            "editor_list" => ui!(|s, tx| s.auto_editor_list(&g)),
            "editor_open" => ui!(|s, tx| s.auto_editor_open(&g, &args, tx)),
            "editor_read" => ui!(|s, tx| s.auto_editor_read(&g, &args)),
            "editor_edit" => ui!(|s, tx| s.auto_editor_edit(&g, &args)),
            "editor_save" => editor::save(self, grant, &args),
            "git_changes" => editor::git_changes(self, grant, &args),
            "git_diff" => editor::git_diff(self, grant, &args),
            "capture_screenshot" => self.capture(grant, &args),
            #[cfg(feature = "browser")]
            name if name.starts_with("browser_") || name.starts_with("recording_") => {
                crate::desktop::browser::automation::run(self, grant, name, &args, &cancel)
            }
            name if name.starts_with("codex_") => {
                crate::desktop::codex::run(self, grant, name, &args, &cancel)
            }
            _ => Err(ToolError::new(
                ErrorCode::Unavailable,
                format!("{name} is not available in this build"),
            )),
        }
    }

    fn own_job(
        &self,
        grant: &ClientGrant,
        id: &str,
    ) -> Result<crate::automation::events::Job, ToolError> {
        self.shared
            .hub
            .job(id)
            .filter(|j| j.client == grant.id)
            .ok_or_else(|| {
                ToolError::new(
                    ErrorCode::UnknownTarget,
                    format!("No job {id} for this client"),
                )
            })
    }

    fn capture(&self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let target = crate::automation::arg_str(args, "target")?.to_owned();
        let artifacts = self.root.join("artifacts");
        if target.starts_with("term_") {
            let g = grant.clone();
            let screen = on_ui(&self.handle, move |s, _| {
                s.auto_terminal_capture(&g, &target)
            })??;
            return crate::desktop::capture::terminal_png(&artifacts, screen);
        }
        #[cfg(feature = "browser")]
        if target.starts_with("brw_") {
            return crate::desktop::browser::automation::screenshot(self, grant, args);
        }
        if target == "window" {
            grant.check(Some(Scope::Capture), None)?;
            return crate::desktop::capture::window_png(self, &artifacts);
        }
        Err(ToolError::new(
            ErrorCode::UnknownTarget,
            format!("No capture target {target}"),
        ))
    }
}

impl State {
    /// Start listening for automation clients. Called once the app handle
    /// exists, for the owner instance only.
    pub(crate) fn start_automation(&mut self) {
        let Some(handle) = self.handle.clone() else {
            return;
        };
        let host = Arc::new(Host {
            handle,
            shared: self.automation.shared.clone(),
            root: self.root.clone(),
            instance: self.automation.instance.clone(),
        });
        match channel::serve(&self.root, host) {
            Ok(server) => self.automation.server = Some(server),
            Err(error) => eprintln!("rodtty: automation channel unavailable: {error}"),
        }
        self.automation_refresh_grants(true);
    }

    pub(crate) fn emit(&self, kind: &str, target: Option<&str>, data: Value) {
        self.automation.shared.hub.emit(kind, target, data);
    }

    pub(crate) fn terminal_target(&self, session: &Session) -> String {
        format!("term_{}_{}", self.automation.instance, session.id())
    }

    fn automation_status_summary(&self) -> Value {
        json!({
            "window_visible": self.handle.as_ref().is_some_and(|h| h.is_visible()),
            "sessions": self.sessions.len(),
            "editors": self.workspace.tabs.len(),
        })
    }

    /// The workspace a folder belongs to: the registered workspace containing
    /// it, or the folder itself as an unregistered project root.
    pub(crate) fn workspace_of(&self, folder: &Path) -> (String, PathBuf) {
        use crate::paths::Canonical as _;
        let registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
        if let Some(w) = registry.containing(folder) {
            return (w.id.clone(), w.root.clone());
        }
        let root = folder.canonical().unwrap_or_else(|_| folder.to_owned());
        (crate::workspaces::workspace_id(&root), root)
    }

    /// The active workspace: the one containing the explorer's folder.
    pub(crate) fn active_workspace(&self) -> (String, PathBuf) {
        let folder = self
            .workspace
            .directory
            .clone()
            .unwrap_or_else(|| self.directory.clone());
        self.workspace_of(&folder)
    }

    pub(crate) fn session_owner(&mut self, index: usize) -> SessionOwner {
        let id = self.sessions[index].id();
        if let Some(owner) = self.automation.owners.get(&id) {
            return owner.clone();
        }
        let (workspace, root) = self.workspace_of(&self.sessions[index].initial_cwd.clone());
        let owner = SessionOwner {
            workspace,
            root,
            client: None,
        };
        self.automation.owners.insert(id, owner.clone());
        owner
    }

    fn automation_visible_targets(&mut self, grant: &ClientGrant) -> HashSet<String> {
        let mut visible = HashSet::new();
        for index in 0..self.sessions.len() {
            let owner = self.session_owner(index);
            if grant.allows_workspace(&owner.workspace) {
                visible.insert(self.terminal_target(&self.sessions[index]));
            }
        }
        #[cfg(feature = "browser")]
        visible.extend(self.browser_targets_for(grant));
        visible
    }

    fn auto_workspace_list(&mut self, grant: &ClientGrant) -> ToolResult {
        let registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
        let (active, active_root) = self.active_workspace();
        let mut rows: Vec<Value> = registry
            .workspaces
            .iter()
            .filter(|w| grant.allows_workspace(&w.id))
            .map(|w| json!({"id": w.id, "name": registry.label(&w.id), "root": w.root, "active": w.id == active, "registered": true}))
            .collect();
        if registry.get(&active).is_none() && grant.allows_workspace(&active) {
            rows.push(json!({"id": active, "name": active_root.file_name().map(|n| n.to_string_lossy().into_owned()), "root": active_root, "active": true, "registered": false}));
        }
        Ok(json!({"workspaces": rows}))
    }

    fn auto_workspace_switch(
        &mut self,
        grant: &ClientGrant,
        args: &Value,
        tx: &mut Tx,
    ) -> ToolResult {
        let id = crate::automation::arg_str(args, "workspace")?;
        grant.check(None, Some(id))?;
        let registry = crate::workspaces::Registry::load(&self.root).unwrap_or_default();
        let workspace = registry.get(id).cloned().ok_or_else(|| {
            ToolError::new(
                ErrorCode::UnknownTarget,
                "No registered workspace with that ID",
            )
        })?;
        self.switch_workspace(&workspace.id, tx)
            .map_err(|e| ToolError::new(ErrorCode::Conflict, e))?;
        Ok(json!({"workspace": workspace.id, "root": workspace.root}))
    }

    fn auto_app_action(&mut self, args: &Value, tx: &mut Tx) -> ToolResult {
        let Some(id) = crate::automation::arg_opt_str(args, "command")? else {
            let commands: Vec<Value> = self
                .commands
                .iter()
                .filter(|c| !c.id.starts_with("automation-"))
                .map(|c| json!({"id": c.id, "title": c.title, "disabled": self.action_disabled(c.action)}))
                .collect();
            return Ok(json!({"commands": commands}));
        };
        // The owner's automation controls stay with the owner.
        if id.starts_with("automation-") || id == "quit" {
            return Err(ToolError::new(
                ErrorCode::ScopeDenied,
                "That action is reserved for the owner",
            ));
        }
        let command = self
            .commands
            .iter()
            .find(|c| c.id == id)
            .ok_or_else(|| ToolError::invalid(format!("No command {id}")))?;
        let action = command.action;
        if let Some(reason) = self.action_disabled(action) {
            return Err(ToolError::new(ErrorCode::Conflict, reason));
        }
        self.dispatch(action, tx);
        Ok(json!({"command": id, "message": self.message}))
    }

    /// Re-read the grants file when it changed (or `force`): refresh the
    /// pending indicator and stop paused or revoked clients' queued input.
    pub(crate) fn automation_refresh_grants(&mut self, force: bool) {
        let path = self.root.join(crate::automation::grants::FILE);
        let modified = std::fs::metadata(&path).and_then(|m| m.modified()).ok();
        if !force && modified == self.automation.grants_seen {
            return;
        }
        self.automation.grants_seen = modified;
        let grants = Grants::load(&self.root).unwrap_or_default();
        self.automation.pending = grants
            .clients
            .iter()
            .filter(|c| c.status == Status::Pending)
            .cloned()
            .collect();
        self.automation.active_clients = grants
            .clients
            .iter()
            .filter(|c| c.status == Status::Approved)
            .count();
        for client in &grants.clients {
            let before = self
                .automation
                .statuses
                .insert(client.id.clone(), client.status);
            if before.is_some_and(|b| b != client.status) {
                self.emit("client", Some(&client.id), json!({"status": client.status}));
                if matches!(client.status, Status::Paused | Status::Revoked) {
                    let reason = if client.status == Status::Paused {
                        "paused"
                    } else {
                        "revoked"
                    };
                    self.automation_release_client(&client.id, reason);
                }
            }
        }
    }

    pub(crate) fn automation_poll(&mut self, tx: &mut Tx) -> bool {
        let diff = self.apply_pending_diff(tx);
        if self.automation.grants_poll.elapsed() < Duration::from_millis(500) {
            return diff;
        }
        self.automation.grants_poll = Instant::now();
        let pending = self.automation.pending.len();
        self.automation_refresh_grants(false);
        let ended = self.automation_note_exits();
        diff || pending != self.automation.pending.len() || ended
    }

    /// Show `path`'s diff once its editor tab exists.
    pub(crate) fn show_diff(
        &mut self,
        path: PathBuf,
        comparison: crate::git::Comparison,
        tx: &mut Tx,
    ) {
        self.open_file(path.clone());
        self.pending_diff = Some((path, comparison));
        self.apply_pending_diff(tx);
    }

    fn apply_pending_diff(&mut self, tx: &mut Tx) -> bool {
        let Some((path, comparison)) = self.pending_diff.clone() else {
            return false;
        };
        let Some(index) = self
            .workspace
            .tabs
            .iter()
            .position(|t| t.document.path == path)
        else {
            return false;
        };
        self.pending_diff = None;
        self.activate_editor(index);
        self.dispatch(AppAction::Compare(comparison), tx);
        true
    }

    /// Report sessions that ended since the last poll.
    fn automation_note_exits(&mut self) -> bool {
        false
    }

    /// Cancel a client's jobs and queued input and release its held keys
    /// and buttons on every target.
    pub(crate) fn automation_release_client(&mut self, client: &str, reason: &str) {
        self.automation.shared.hub.cancel_client(client);
        let targets: Vec<String> = self
            .automation
            .inputs
            .keys()
            .filter(|(c, _)| c == client)
            .map(|(_, t)| t.clone())
            .collect();
        for target in targets {
            self.release_virtual_input(client, &target, reason);
        }
        let focused: Vec<String> = self
            .automation
            .agent_focus
            .iter()
            .filter(|(_, c)| *c == client)
            .map(|(t, _)| t.clone())
            .collect();
        for target in focused {
            self.automation.shared.bump(&target);
            self.automation.agent_focus.remove(&target);
        }
    }

    /// The owner's own input reached `target`. If an agent was driving it,
    /// the owner takes over: the agent's queued input stops, its held keys
    /// and buttons are released, and its input is refused until the owner
    /// returns the target. The owner's input itself is never dropped.
    pub(crate) fn owner_input(&mut self, target: &str) {
        let Some(client) = self.automation.agent_focus.remove(target) else {
            return;
        };
        self.automation.takeovers.insert(target.to_owned());
        self.release_virtual_input(&client, target, "owner_takeover");
        self.automation.shared.bump(target);
        self.emit(
            "input_ownership",
            Some(target),
            json!({"owner": "human", "client": client, "reason": "owner_takeover"}),
        );
        self.message = "You took over a target an agent was using. Automation: Return Target to Agent resumes it.".into();
    }

    /// The active terminal's target ID, for owner input routing.
    pub(crate) fn owner_terminal_input(&mut self) {
        if let Some(target) = self.active().map(|s| self.terminal_target(s)) {
            self.owner_input(&target);
        }
    }

    pub(crate) fn automation_take_over(&mut self) {
        if let Some(target) = self.active().map(|s| self.terminal_target(s)) {
            let client = self.automation.agent_focus.remove(&target);
            self.automation.takeovers.insert(target.clone());
            if let Some(client) = client {
                self.release_virtual_input(&client, &target, "owner_takeover");
            }
            self.automation.shared.bump(&target);
            self.emit(
                "input_ownership",
                Some(&target),
                json!({"owner": "human", "reason": "owner_takeover"}),
            );
            self.message = "Agent input to this terminal is paused until you return it.".into();
        }
    }

    pub(crate) fn automation_return_target(&mut self) {
        let targets: Vec<String> = match self.active().map(|s| self.terminal_target(s)) {
            Some(target) if self.automation.takeovers.contains(&target) => vec![target],
            _ => self.automation.takeovers.iter().cloned().collect(),
        };
        for target in &targets {
            self.automation.takeovers.remove(target);
            self.emit(
                "input_ownership",
                Some(target),
                json!({"owner": "agent", "reason": "owner_returned"}),
            );
        }
        self.message = if targets.is_empty() {
            "No target is taken over.".into()
        } else {
            format!("Returned {} target(s) to agents.", targets.len())
        };
    }

    /// Owner control: approve the oldest pending client for every scope on
    /// the active workspace.
    pub(crate) fn automation_approve_pending(&mut self) {
        let mut grants = Grants::load(&self.root).unwrap_or_default();
        let Some(pending) = grants
            .clients
            .iter()
            .find(|c| c.status == Status::Pending)
            .cloned()
        else {
            self.message = "No automation client is waiting for approval.".into();
            return;
        };
        let (workspace, root) = self.active_workspace();
        let _ = grants.approve(&pending.id, &Scope::ALL, &[workspace]);
        self.message = match grants.save(&self.root) {
            Ok(()) => format!("Approved {} for {}.", pending.name, root.display()),
            Err(error) => format!("Could not save the approval: {error:#}"),
        };
        self.automation_refresh_grants(true);
    }

    pub(crate) fn automation_set_all(&mut self, from: &[Status], to: Status) {
        let mut grants = Grants::load(&self.root).unwrap_or_default();
        let ids: Vec<String> = grants
            .clients
            .iter()
            .filter(|c| from.contains(&c.status))
            .map(|c| c.id.clone())
            .collect();
        for id in &ids {
            let _ = grants.set_status(id, to);
        }
        self.message = match grants.save(&self.root) {
            Ok(()) => format!("{} automation client(s) now {:?}.", ids.len(), to).to_lowercase(),
            Err(error) => format!("Could not save automation clients: {error:#}"),
        };
        self.automation_refresh_grants(true);
    }

    /// The footer's automation indicator, when there is anything to show.
    pub(crate) fn automation_indicator(&self) -> Option<String> {
        let recording = self.recording_indicator();
        let agents = !self.automation.agent_focus.is_empty();
        match (self.automation.pending.first(), recording, agents) {
            (_, Some(recording), _) => Some(recording),
            (Some(pending), _, _) => Some(format!("Automation: {} wants access", pending.name)),
            (None, None, true) => Some(format!(
                "Agent active on {} target(s)",
                self.automation.agent_focus.len()
            )),
            _ => None,
        }
    }

    #[cfg(not(feature = "browser"))]
    pub(crate) fn recording_indicator(&self) -> Option<String> {
        None
    }

    /// Make `id` the active workspace: save the current arrangement to the
    /// workspace being left and show the project's folder. Terminal sessions
    /// and unsaved buffers stay alive; they belong to their workspaces.
    pub(crate) fn switch_workspace(&mut self, id: &str, _tx: &mut Tx) -> Result<(), String> {
        let mut registry =
            crate::workspaces::Registry::load(&self.root).map_err(|e| format!("{e:#}"))?;
        let target = registry.get(id).cloned().ok_or("No such workspace")?;
        if !target.root.is_dir() {
            return Err(format!(
                "The project folder {} is missing",
                target.root.display()
            ));
        }
        let (current, _) = self.active_workspace();
        if current == target.id {
            return Ok(());
        }
        let layout = self.workspace.layout.clone();
        if let Some(leaving) = registry.get_mut(&current) {
            leaving.layout = Some(layout);
        }
        registry.save(&self.root).map_err(|e| format!("{e:#}"))?;
        let _ = crate::workspaces::save_selection(&self.root, Some(&target.id));
        self.navigate(target.root.clone());
        self.emit(
            "workspace",
            Some(&target.id),
            json!({"state": "active", "root": target.root}),
        );
        self.message = format!("Switched to {}.", registry.label(&target.id));
        Ok(())
    }
}

/// `rodtty automation …`: the owner's command-line controls.
pub(crate) fn cli(root: &Path, args: &[String]) -> anyhow::Result<()> {
    let mut grants = Grants::load(root)?;
    let usage = "usage: rodtty automation list | approve <client> [--scopes read,terminal,…|all] [--workspace <folder|id|all>]… | pause <client> | resume <client> | revoke <client> | forget <client>";
    match args.first().map(String::as_str) {
        None | Some("list") => {
            println!("{}", serde_json::to_string_pretty(&grants)?);
            return Ok(());
        }
        Some("approve") => {
            let client = args.get(1).ok_or_else(|| anyhow::anyhow!(usage))?;
            let mut scopes = Scope::ALL.to_vec();
            let mut workspaces = Vec::new();
            let mut rest = args[2..].iter();
            while let Some(flag) = rest.next() {
                let value = rest.next().ok_or_else(|| anyhow::anyhow!(usage))?;
                match flag.as_str() {
                    "--scopes" if value == "all" => scopes = Scope::ALL.to_vec(),
                    "--scopes" => {
                        scopes = value
                            .split(',')
                            .map(|s| {
                                Scope::parse(s.trim())
                                    .ok_or_else(|| anyhow::anyhow!("unknown scope {s}"))
                            })
                            .collect::<anyhow::Result<_>>()?
                    }
                    "--workspace" if value == "all" => {
                        workspaces.push(crate::automation::grants::ALL_WORKSPACES.to_owned())
                    }
                    "--workspace" if value.starts_with("ws_") => workspaces.push(value.clone()),
                    "--workspace" => {
                        let mut registry = crate::workspaces::Registry::load(root)?;
                        let workspace = registry.create("", Path::new(value))?;
                        registry.save(root)?;
                        workspaces.push(workspace.id);
                    }
                    _ => anyhow::bail!(usage),
                }
            }
            if workspaces.is_empty() {
                anyhow::bail!(
                    "name at least one --workspace (a project folder, a workspace ID, or all)"
                );
            }
            grants.approve(client, &scopes, &workspaces)?;
        }
        Some("pause") => grants.set_status(
            args.get(1).ok_or_else(|| anyhow::anyhow!(usage))?,
            Status::Paused,
        )?,
        Some("resume") => grants.set_status(
            args.get(1).ok_or_else(|| anyhow::anyhow!(usage))?,
            Status::Approved,
        )?,
        Some("revoke") => grants.set_status(
            args.get(1).ok_or_else(|| anyhow::anyhow!(usage))?,
            Status::Revoked,
        )?,
        Some("forget") => {
            grants.forget(args.get(1).ok_or_else(|| anyhow::anyhow!(usage))?);
        }
        Some(_) => anyhow::bail!(usage),
    }
    grants.save(root)?;
    println!("{}", serde_json::to_string_pretty(&grants)?);
    Ok(())
}

#[allow(dead_code)]
fn job_finished(shared: &Shared, id: &str, result: ToolResult) {
    match result {
        Ok(value) => shared
            .hub
            .update_job(id, JobState::Completed, Some(value), None),
        Err(error) if error.code == ErrorCode::Cancelled => {
            shared
                .hub
                .update_job(id, JobState::Cancelled, None, Some(error))
        }
        Err(error) => shared
            .hub
            .update_job(id, JobState::Failed, None, Some(error)),
    }
}

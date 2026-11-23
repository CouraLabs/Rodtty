//! The optional structured Codex adapter: Codex App Server threads that
//! automation clients start in authorized workspaces, whose approval
//! requests wait for the owner in Rodtty. A Codex TUI in a terminal keeps
//! working through the terminal tools either way.
use super::*;
use crate::automation::{
    ErrorCode, ToolError, ToolResult, arg_opt_str, arg_str, codex::AppServer, events::JobState,
    grants::ClientGrant,
};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    sync::{Arc, Mutex, OnceLock, atomic::AtomicBool},
    time::Duration,
};

struct Entry {
    server: Arc<AppServer>,
    codex_thread: String,
    client: String,
    workspace: String,
}

static THREADS: Mutex<Option<HashMap<String, Entry>>> = Mutex::new(None);

fn with_threads<R>(f: impl FnOnce(&mut HashMap<String, Entry>) -> R) -> R {
    let mut guard = THREADS.lock().unwrap_or_else(|e| e.into_inner());
    f(guard.get_or_insert_with(HashMap::new))
}

/// The App Server command: `RODTTY_CODEX_APP_SERVER` (a command line, for
/// protocol fixtures) or `codex app-server`.
fn command() -> Vec<String> {
    match std::env::var("RODTTY_CODEX_APP_SERVER") {
        Ok(line) if !line.trim().is_empty() => line.split_whitespace().map(str::to_owned).collect(),
        _ => vec!["codex".into(), "app-server".into()],
    }
}

/// Whether an App Server is installed: the command answers `--help`.
pub(crate) fn available() -> bool {
    static AVAILABLE: OnceLock<bool> = OnceLock::new();
    *AVAILABLE.get_or_init(|| {
        let command = command();
        if std::env::var("RODTTY_CODEX_APP_SERVER").is_ok() {
            return true;
        }
        std::process::Command::new(&command[0])
            .args(&command[1..])
            .arg("--help")
            .stdin(std::process::Stdio::null())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok_and(|s| s.success())
    })
}

fn entry<R>(
    grant: &ClientGrant,
    thread: &str,
    f: impl FnOnce(&Entry) -> R,
) -> Result<R, ToolError> {
    with_threads(|threads| match threads.get(thread) {
        Some(entry) if entry.client == grant.id => {
            grant.check(None, Some(&entry.workspace))?;
            Ok(f(entry))
        }
        Some(_) => Err(ToolError::new(
            ErrorCode::ScopeDenied,
            "That thread belongs to another client",
        )),
        None => Err(ToolError::new(
            ErrorCode::UnknownTarget,
            format!("No Codex thread {thread}"),
        )),
    })
}

fn failed(e: String) -> ToolError {
    ToolError::new(ErrorCode::Internal, e)
}

pub(crate) fn run(
    host: &automation::Host,
    grant: &ClientGrant,
    name: &str,
    args: &Value,
    cancel: &AtomicBool,
) -> ToolResult {
    match name {
        "codex_thread_start" => {
            let g = grant.clone();
            let cwd = arg_opt_str(args, "cwd")?.map(PathBuf::from);
            let (cwd, workspace) = on_ui_result(host, move |s| {
                let cwd = match cwd {
                    Some(cwd) => cwd,
                    None => s.active_workspace().1,
                };
                let cwd = s.authorize_path(&g, &cwd)?;
                let (workspace, _) = s.workspace_of(&cwd);
                Ok((cwd, workspace))
            })?;
            let server = AppServer::start(&command(), &cwd)
                .map_err(|e| ToolError::new(ErrorCode::Unavailable, e))?;
            let codex_thread = server
                .start_thread(&cwd, arg_opt_str(args, "thread")?)
                .map_err(failed)?;
            let id = format!("cdx_{}", crate::automation::token());
            let version = server
                .version
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .clone();
            host.shared.set_target_client(&id, &grant.id);
            with_threads(|t| {
                t.insert(
                    id.clone(),
                    Entry {
                        server,
                        codex_thread: codex_thread.clone(),
                        client: grant.id.clone(),
                        workspace: workspace.clone(),
                    },
                )
            });
            host.shared
                .hub
                .emit("codex", Some(&id), json!({"state": "thread_started"}));
            Ok(
                json!({"thread": id, "codex_thread_id": codex_thread, "cwd": cwd, "workspace": workspace, "server": version}),
            )
        }
        "codex_turn_start" => {
            let thread = arg_str(args, "thread")?.to_owned();
            let prompt = arg_str(args, "prompt")?.to_owned();
            let (server, codex_thread) = entry(grant, &thread, |e| {
                (e.server.clone(), e.codex_thread.clone())
            })?;
            let turn = server.start_turn(&codex_thread, &prompt).map_err(failed)?;
            let (job, job_cancel) =
                host.shared
                    .hub
                    .start_job("codex_turn", &grant.id, Some(&thread));
            let shared = host.shared.clone();
            let handle = host.handle.clone();
            let (job_id, turn_id, thread_id) = (job.clone(), turn.clone(), thread.clone());
            std::thread::spawn(move || {
                let mut announced = false;
                while let Some(state) =
                    server.wait_turn(&turn_id, Duration::from_secs(3600), &job_cancel)
                {
                    match state.status.as_str() {
                        "waitingForApproval" => {
                            if !announced {
                                announced = true;
                                shared.hub.update_job(
                                    &job_id,
                                    JobState::WaitingForOwner,
                                    Some(json!(state)),
                                    None,
                                );
                                shared.hub.emit("approval", Some(&thread_id), json!({"kind": "codex", "turn": turn_id, "summary": state.approval.as_ref().map(|a| a.summary.clone())}));
                                handle.update(|s, _| s.message = "Codex is waiting for your approval: Codex: Approve Pending Request or Decline.".into());
                            }
                            std::thread::sleep(Duration::from_millis(100));
                        }
                        "inProgress" => {
                            announced = false;
                            if job_cancel.load(std::sync::atomic::Ordering::Relaxed) {
                                break;
                            }
                        }
                        "completed" => {
                            shared.hub.update_job(
                                &job_id,
                                JobState::Completed,
                                Some(json!(state)),
                                None,
                            );
                            break;
                        }
                        _ => {
                            let error = ToolError::new(
                                if state.status == "interrupted" {
                                    ErrorCode::Cancelled
                                } else {
                                    ErrorCode::Internal
                                },
                                state.error.clone().unwrap_or(state.status.clone()),
                            );
                            shared.hub.update_job(
                                &job_id,
                                if state.status == "interrupted" {
                                    JobState::Cancelled
                                } else {
                                    JobState::Failed
                                },
                                Some(json!(state)),
                                Some(error),
                            );
                            break;
                        }
                    }
                }
                shared.hub.emit(
                    "codex",
                    Some(&thread_id),
                    json!({"state": "turn_finished", "turn": turn_id}),
                );
            });
            let _ = cancel;
            Ok(json!({"thread": thread, "turn": turn, "job": job}))
        }
        "codex_turn_status" => {
            let thread = arg_str(args, "thread")?.to_owned();
            let server = entry(grant, &thread, |e| e.server.clone())?;
            let turn = match arg_opt_str(args, "turn")? {
                Some(turn) => server.turn(turn),
                None => server.latest_turn(),
            };
            Ok(json!({"thread": thread, "turn": turn}))
        }
        "codex_turn_interrupt" => {
            let thread = arg_str(args, "thread")?.to_owned();
            let (server, codex_thread) = entry(grant, &thread, |e| {
                (e.server.clone(), e.codex_thread.clone())
            })?;
            let turn = match arg_opt_str(args, "turn")? {
                Some(turn) => turn.to_owned(),
                None => server.latest_turn().map(|t| t.id).ok_or_else(|| {
                    ToolError::new(ErrorCode::UnknownTarget, "No turn to interrupt")
                })?,
            };
            server.interrupt(&codex_thread, &turn).map_err(failed)?;
            Ok(json!({"thread": thread, "turn": turn, "interrupted": true}))
        }
        _ => Err(ToolError::new(ErrorCode::UnknownTool, name.to_owned())),
    }
}

fn on_ui_result<R: Send + 'static>(
    host: &automation::Host,
    f: impl FnOnce(&mut State) -> Result<R, ToolError> + Send + 'static,
) -> Result<R, ToolError> {
    automation::on_ui(&host.handle, move |s, _| f(s))?
}

/// Open approval requests across structured threads, for the owner.
pub(crate) fn pending_approvals() -> Vec<(String, String, String)> {
    with_threads(|threads| {
        threads
            .iter()
            .filter_map(|(id, e)| {
                let turn = e.server.latest_turn()?;
                let approval = turn.approval?;
                Some((id.clone(), turn.id, approval.summary))
            })
            .collect()
    })
}

impl State {
    /// The owner's decision on the oldest open Codex approval request.
    pub(crate) fn codex_decide(&mut self, decision: &str) {
        let Some((thread, turn, summary)) = pending_approvals().into_iter().next() else {
            self.message = "No Codex request is waiting for approval.".into();
            return;
        };
        let result = with_threads(|threads| {
            threads
                .get(&thread)
                .map(|e| e.server.decide(&turn, decision))
        });
        self.message = match result {
            Some(Ok(())) => format!("Codex request ({summary}): {decision}."),
            Some(Err(error)) => format!("Could not answer the Codex request: {error}"),
            None => "That Codex thread has ended.".into(),
        };
        self.emit(
            "approval",
            Some(&thread),
            json!({"kind": "codex", "turn": turn, "decision": decision}),
        );
    }
}

/// End every structured thread's App Server, for Quit.
pub(crate) fn shutdown() {
    with_threads(HashMap::clear);
}

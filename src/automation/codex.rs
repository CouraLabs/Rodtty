//! A client for the Codex App Server protocol (`codex app-server`): JSON-RPC
//! 2.0 over the server's stdio, one message per line.
//!
//! Rodtty starts one App Server process per structured thread, in the
//! thread's workspace folder, and leaves its sandbox and approval settings to
//! the owner's Codex configuration. Approval requests the server sends
//! (`item/commandExecution/requestApproval`, `item/fileChange/requestApproval`,
//! and the legacy `execCommandApproval`/`applyPatchApproval`) wait for the
//! owner's decision in Rodtty; Rodtty never answers them on its own.
//! Account and credential methods are never called, and nothing from the
//! server's environment is returned to automation clients.
//!
//! Tested against the protocol of Codex CLI 0.170 (`codex app-server
//! generate-json-schema`). An App Server thread is not the same conversation
//! as a Codex TUI running in a terminal.
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::Path,
    process::{Child, ChildStdin, Command, Stdio},
    sync::{Arc, Condvar, Mutex, mpsc},
    time::{Duration, Instant},
};

/// What a turn is doing, from protocol notifications.
#[derive(Clone, Debug, Default, serde::Serialize)]
pub struct TurnState {
    pub id: String,
    /// `inProgress`, `completed`, `interrupted`, `failed`, or
    /// `waitingForApproval` while an approval request is open.
    pub status: String,
    /// The agent's reply text so far.
    pub message: String,
    /// Item kinds started in this turn, in order (`agentMessage`, `commandExecution`, …).
    pub items: Vec<String>,
    pub error: Option<String>,
    pub approval: Option<Approval>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct Approval {
    /// The JSON-RPC request ID to answer.
    #[serde(skip)]
    pub request: Value,
    pub method: String,
    pub summary: String,
}

#[derive(Default)]
struct Inner {
    replies: HashMap<u64, Result<Value, String>>,
    turns: HashMap<String, TurnState>,
    exited: bool,
    log: Vec<String>,
}

pub struct AppServer {
    child: Mutex<Child>,
    stdin: Mutex<ChildStdin>,
    inner: Arc<(Mutex<Inner>, Condvar)>,
    next: Mutex<u64>,
    pub thread: Mutex<Option<String>>,
    pub version: Mutex<Option<String>>,
}

fn summarize(method: &str, params: &Value) -> String {
    let command = params.get("command").map(|c| match c {
        Value::Array(parts) => parts
            .iter()
            .filter_map(Value::as_str)
            .collect::<Vec<_>>()
            .join(" "),
        Value::String(s) => s.clone(),
        _ => String::new(),
    });
    match (method, command) {
        (_, Some(command)) if !command.is_empty() => format!("Run `{command}`"),
        (m, _) if m.contains("fileChange") || m == "applyPatchApproval" => {
            "Apply file changes".into()
        }
        (m, _) => m.to_owned(),
    }
}

impl AppServer {
    /// Start `program` (normally `codex app-server`) in `cwd` and initialize it.
    pub fn start(program: &[String], cwd: &Path) -> Result<Arc<Self>, String> {
        let (exe, args) = program.split_first().ok_or("no App Server command")?;
        let mut child = Command::new(exe)
            .args(args)
            .current_dir(cwd)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .map_err(|e| format!("Could not start the Codex App Server: {e}"))?;
        let stdin = child.stdin.take().ok_or("no stdin")?;
        let stdout = child.stdout.take().ok_or("no stdout")?;
        let inner: Arc<(Mutex<Inner>, Condvar)> = Default::default();
        let (requests, incoming) = mpsc::channel::<(Value, String, Value)>();
        {
            let inner = inner.clone();
            std::thread::spawn(move || {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    let Ok(message) = serde_json::from_str::<Value>(&line) else {
                        continue;
                    };
                    let (lock, signal) = &*inner;
                    let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                    let method = message
                        .get("method")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                    match (message.get("id"), method) {
                        // A reply to one of our requests.
                        (Some(id), None) => {
                            if let Some(id) = id.as_u64() {
                                let reply = match message.get("error") {
                                    Some(error) => Err(error
                                        .get("message")
                                        .and_then(Value::as_str)
                                        .unwrap_or("error")
                                        .to_owned()),
                                    None => {
                                        Ok(message.get("result").cloned().unwrap_or(Value::Null))
                                    }
                                };
                                state.replies.insert(id, reply);
                            }
                        }
                        // A request from the server: approvals wait for the owner.
                        (Some(id), Some(method)) => {
                            let params = message.get("params").cloned().unwrap_or(Value::Null);
                            let turn = params
                                .get("turnId")
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .or_else(|| {
                                    state
                                        .turns
                                        .iter()
                                        .find(|(_, t)| t.status == "inProgress")
                                        .map(|(k, _)| k.clone())
                                });
                            if method.contains("pproval") {
                                if let Some(turn) = turn.and_then(|t| state.turns.get_mut(&t)) {
                                    turn.approval = Some(Approval {
                                        request: id.clone(),
                                        method: method.clone(),
                                        summary: summarize(&method, &params),
                                    });
                                    turn.status = "waitingForApproval".into();
                                }
                            } else {
                                let _ = requests.send((id.clone(), method, params));
                            }
                        }
                        (None, Some(method)) => {
                            let params = message.get("params").cloned().unwrap_or(Value::Null);
                            let turn_id = params
                                .get("turnId")
                                .and_then(Value::as_str)
                                .or_else(|| params.pointer("/turn/id").and_then(Value::as_str))
                                .map(str::to_owned);
                            if let Some(turn_id) = turn_id {
                                let turn =
                                    state.turns.entry(turn_id.clone()).or_insert_with(|| {
                                        TurnState {
                                            id: turn_id,
                                            status: "inProgress".into(),
                                            ..Default::default()
                                        }
                                    });
                                match method.as_str() {
                                    "item/agentMessage/delta" => {
                                        if let Some(delta) =
                                            params.get("delta").and_then(Value::as_str)
                                        {
                                            turn.message.push_str(delta);
                                        }
                                    }
                                    "item/started" => {
                                        if let Some(kind) =
                                            params.pointer("/item/type").and_then(Value::as_str)
                                        {
                                            turn.items.push(kind.to_owned());
                                        }
                                    }
                                    "item/completed" => {
                                        if params.pointer("/item/type").and_then(Value::as_str)
                                            == Some("agentMessage")
                                            && let Some(text) =
                                                params.pointer("/item/text").and_then(Value::as_str)
                                        {
                                            turn.message = text.to_owned();
                                        }
                                    }
                                    "serverRequest/resolved" => {
                                        turn.approval = None;
                                        if turn.status == "waitingForApproval" {
                                            turn.status = "inProgress".into();
                                        }
                                    }
                                    "turn/completed" => {
                                        turn.status = params
                                            .pointer("/turn/status")
                                            .and_then(Value::as_str)
                                            .unwrap_or("completed")
                                            .to_owned();
                                        turn.approval = None;
                                        if let Some(error) = params
                                            .pointer("/turn/error/message")
                                            .and_then(Value::as_str)
                                        {
                                            turn.error = Some(error.to_owned());
                                        }
                                    }
                                    "error" => {
                                        turn.error = params
                                            .pointer("/error/message")
                                            .and_then(Value::as_str)
                                            .map(str::to_owned)
                                            .or(Some("error".into()));
                                    }
                                    _ => {}
                                }
                            }
                            state.log.push(method);
                            if state.log.len() > 200 {
                                state.log.remove(0);
                            }
                        }
                        _ => {}
                    }
                    drop(state);
                    signal.notify_all();
                }
                let (lock, signal) = &*inner;
                let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
                state.exited = true;
                for turn in state
                    .turns
                    .values_mut()
                    .filter(|t| t.status == "inProgress" || t.status == "waitingForApproval")
                {
                    turn.status = "failed".into();
                    turn.error = Some("The Codex App Server exited".into());
                }
                drop(state);
                signal.notify_all();
            });
        }
        let server = Arc::new(Self {
            child: Mutex::new(child),
            stdin: Mutex::new(stdin),
            inner,
            next: Mutex::new(0),
            thread: Mutex::new(None),
            version: Mutex::new(None),
        });
        // Other server requests (tool calls, input prompts) are declined:
        // the structured adapter offers no tools to the model.
        {
            let weak = Arc::downgrade(&server);
            std::thread::spawn(move || {
                for (id, method, _) in incoming {
                    let Some(server) = weak.upgrade() else { return };
                    let _ = server.write(&json!({"jsonrpc": "2.0", "id": id, "error": {"code": -32601, "message": format!("{method} is not supported by Rodtty")}}));
                }
            });
        }
        let init = server.request("initialize", json!({"clientInfo": {"name": "rodtty", "title": "Rodtty", "version": env!("CARGO_PKG_VERSION")}}), Duration::from_secs(20))?;
        server.write(&json!({"jsonrpc": "2.0", "method": "initialized"}))?;
        *server.version.lock().unwrap_or_else(|e| e.into_inner()) = init
            .get("userAgent")
            .and_then(Value::as_str)
            .map(str::to_owned);
        Ok(server)
    }

    fn write(&self, message: &Value) -> Result<(), String> {
        let mut stdin = self.stdin.lock().unwrap_or_else(|e| e.into_inner());
        let mut line = serde_json::to_vec(message).map_err(|e| e.to_string())?;
        line.push(b'\n');
        stdin
            .write_all(&line)
            .and_then(|_| stdin.flush())
            .map_err(|e| format!("The Codex App Server is not accepting requests: {e}"))
    }

    pub fn request(&self, method: &str, params: Value, timeout: Duration) -> Result<Value, String> {
        let id = {
            let mut next = self.next.lock().unwrap_or_else(|e| e.into_inner());
            *next += 1;
            *next
        };
        self.write(&json!({"jsonrpc": "2.0", "id": id, "method": method, "params": params}))?;
        let deadline = Instant::now() + timeout;
        let (lock, signal) = &*self.inner;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            if let Some(reply) = state.replies.remove(&id) {
                return reply;
            }
            if state.exited {
                return Err("The Codex App Server exited".into());
            }
            let left = deadline.saturating_duration_since(Instant::now());
            if left.is_zero() {
                return Err(format!("{method} timed out"));
            }
            state = signal
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    pub fn start_thread(&self, cwd: &Path, resume: Option<&str>) -> Result<String, String> {
        let result = match resume {
            Some(id) => self.request(
                "thread/resume",
                json!({"threadId": id, "cwd": cwd}),
                Duration::from_secs(60),
            )?,
            None => self.request("thread/start", json!({"cwd": cwd}), Duration::from_secs(60))?,
        };
        let id = result
            .pointer("/thread/id")
            .and_then(Value::as_str)
            .ok_or("thread/start returned no thread")?
            .to_owned();
        *self.thread.lock().unwrap_or_else(|e| e.into_inner()) = Some(id.clone());
        Ok(id)
    }

    pub fn start_turn(&self, thread: &str, prompt: &str) -> Result<String, String> {
        let result = self.request(
            "turn/start",
            json!({"threadId": thread, "input": [{"type": "text", "text": prompt}]}),
            Duration::from_secs(60),
        )?;
        let id = result
            .pointer("/turn/id")
            .and_then(Value::as_str)
            .ok_or("turn/start returned no turn")?
            .to_owned();
        let (lock, signal) = &*self.inner;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        state.turns.entry(id.clone()).or_insert_with(|| TurnState {
            id: id.clone(),
            status: "inProgress".into(),
            ..Default::default()
        });
        drop(state);
        signal.notify_all();
        Ok(id)
    }

    pub fn interrupt(&self, thread: &str, turn: &str) -> Result<(), String> {
        self.request(
            "turn/interrupt",
            json!({"threadId": thread, "turnId": turn}),
            Duration::from_secs(30),
        )
        .map(|_| ())
    }

    pub fn turn(&self, id: &str) -> Option<TurnState> {
        self.inner
            .0
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .turns
            .get(id)
            .cloned()
    }

    pub fn latest_turn(&self) -> Option<TurnState> {
        let state = self.inner.0.lock().unwrap_or_else(|e| e.into_inner());
        state.turns.values().max_by(|a, b| a.id.cmp(&b.id)).cloned()
    }

    /// Wait until the turn finishes, needs an approval, or `timeout` passes.
    pub fn wait_turn(
        &self,
        id: &str,
        timeout: Duration,
        cancel: &std::sync::atomic::AtomicBool,
    ) -> Option<TurnState> {
        let deadline = Instant::now() + timeout;
        let (lock, signal) = &*self.inner;
        let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
        loop {
            let turn = state.turns.get(id).cloned();
            let done = turn.as_ref().is_some_and(|t| t.status != "inProgress");
            if done
                || Instant::now() >= deadline
                || cancel.load(std::sync::atomic::Ordering::Relaxed)
            {
                return turn;
            }
            let left = deadline
                .saturating_duration_since(Instant::now())
                .min(Duration::from_millis(200));
            state = signal
                .wait_timeout(state, left)
                .unwrap_or_else(|e| e.into_inner())
                .0;
        }
    }

    /// Answer an open approval request with the owner's decision:
    /// `accept`, `acceptForSession`, `decline` or `cancel`.
    pub fn decide(&self, turn: &str, decision: &str) -> Result<(), String> {
        let request = {
            let (lock, _) = &*self.inner;
            let mut state = lock.lock().unwrap_or_else(|e| e.into_inner());
            let turn = state.turns.get_mut(turn).ok_or("no such turn")?;
            let approval = turn.approval.take().ok_or("no approval is waiting")?;
            turn.status = "inProgress".into();
            approval
        };
        // The legacy approval methods take `approved`/`denied` decisions.
        let decision = match (request.method.as_str(), decision) {
            ("execCommandApproval" | "applyPatchApproval", "accept") => "approved",
            ("execCommandApproval" | "applyPatchApproval", "acceptForSession") => {
                "approved_for_session"
            }
            ("execCommandApproval" | "applyPatchApproval", "decline") => "denied",
            ("execCommandApproval" | "applyPatchApproval", _) => "abort",
            (_, d) => d,
        };
        self.write(
            &json!({"jsonrpc": "2.0", "id": request.request, "result": {"decision": decision}}),
        )
    }
}

impl Drop for AppServer {
    fn drop(&mut self) {
        let mut child = self.child.lock().unwrap_or_else(|e| e.into_inner());
        let _ = child.kill();
        let _ = child.wait();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn fixture() -> Vec<String> {
        vec![
            "python3".into(),
            concat!(
                env!("CARGO_MANIFEST_DIR"),
                "/scripts/automation/fixtures/fake-codex-app-server.py"
            )
            .into(),
        ]
    }

    #[test]
    fn a_turn_reports_progress_completion_approval_interrupt_and_errors() {
        let dir = crate::test_support::tempdir().unwrap();
        let server = AppServer::start(&fixture(), dir.path()).unwrap();
        let thread = server.start_thread(dir.path(), None).unwrap();
        let never = AtomicBool::new(false);

        let turn = server.start_turn(&thread, "hello").unwrap();
        let done = server
            .wait_turn(&turn, Duration::from_secs(10), &never)
            .unwrap();
        assert_eq!(done.status, "completed");
        assert_eq!(done.message, "Echo: hello");
        assert_eq!(done.items, ["agentMessage"]);

        let turn = server.start_turn(&thread, "please APPROVE this").unwrap();
        let waiting = server
            .wait_turn(&turn, Duration::from_secs(10), &never)
            .unwrap();
        assert_eq!(waiting.status, "waitingForApproval");
        assert_eq!(
            waiting.approval.as_ref().unwrap().summary,
            "Run `touch approved.txt`"
        );
        server.decide(&turn, "accept").unwrap();
        let done = server
            .wait_turn(&turn, Duration::from_secs(10), &never)
            .unwrap();
        assert_eq!(done.status, "completed");
        assert_eq!(done.message, "Approved: accept");

        let turn = server.start_turn(&thread, "SLOW").unwrap();
        server.interrupt(&thread, &turn).unwrap();
        assert_eq!(
            server
                .wait_turn(&turn, Duration::from_secs(10), &never)
                .unwrap()
                .status,
            "interrupted"
        );

        let turn = server.start_turn(&thread, "FAIL").unwrap();
        let failed = server
            .wait_turn(&turn, Duration::from_secs(10), &never)
            .unwrap();
        assert_eq!(failed.status, "failed");
        assert_eq!(failed.error.as_deref(), Some("fixture failure"));
    }
}

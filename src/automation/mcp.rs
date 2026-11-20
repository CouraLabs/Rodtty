//! `rodtty mcp`: a Model Context Protocol server on stdio.
//!
//! Standard output carries only JSON-RPC messages, one per line, as the MCP
//! stdio transport requires; diagnostics go to standard error. Each tool call
//! is forwarded to the running desktop instance over the [`channel`]. The
//! server never starts a second owner instance and never replays a call: a
//! call in flight when the connection drops fails with `disconnected`, and
//! the next call reconnects.
use super::{ErrorCode, ToolError, channel::Client, tools};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, Write},
    path::PathBuf,
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicUsize, Ordering},
    },
    time::Duration,
};

pub const PROTOCOL_VERSIONS: &[&str] = &["2025-06-18", "2025-03-26", "2024-11-05"];
pub const MAX_CONCURRENT_CALLS: usize = 8;

struct Link {
    root: PathBuf,
    name: Mutex<Option<String>>,
    client: Mutex<Option<Arc<Client>>>,
    hello: Mutex<Option<Value>>,
    /// The feature list behind the last tools/list answer.
    listed: Mutex<Option<Vec<String>>>,
    initialized: AtomicBool,
    out: Mutex<std::io::Stdout>,
    calls: AtomicUsize,
    /// JSON-RPC request ID (as text) to (connection, host request ID).
    in_flight: Mutex<HashMap<String, (Arc<Client>, u64)>>,
}

impl Link {
    fn send(&self, message: &Value) {
        let mut out = self.out.lock().unwrap_or_else(|e| e.into_inner());
        let _ = serde_json::to_writer(&mut *out, message);
        let _ = out.write_all(b"\n");
        let _ = out.flush();
    }

    fn respond(&self, id: &Value, result: Value) {
        self.send(&json!({"jsonrpc": "2.0", "id": id, "result": result}));
    }

    fn fail(&self, id: &Value, code: i64, message: &str) {
        self.send(
            &json!({"jsonrpc": "2.0", "id": id, "error": {"code": code, "message": message}}),
        );
    }

    fn features(&self) -> Option<Vec<String>> {
        let hello = self.hello.lock().unwrap_or_else(|e| e.into_inner());
        let features = hello.as_ref()?.get("features")?.as_array()?;
        Some(
            features
                .iter()
                .filter_map(|f| f.as_str().map(str::to_owned))
                .collect(),
        )
    }

    /// The live connection, connecting and introducing this client if needed.
    fn connect(&self) -> Result<Arc<Client>, ToolError> {
        let mut slot = self.client.lock().unwrap_or_else(|e| e.into_inner());
        if let Some(client) = slot.as_ref().filter(|c| c.is_alive()) {
            return Ok(client.clone());
        }
        *slot = None;
        let client = Client::connect(&self.root).map_err(|error| {
            ToolError::new(
                ErrorCode::Disconnected,
                format!(
                    "Can't reach a running Rodtty at {} ({error}). Start Rodtty, then call again.",
                    self.root.display()
                ),
            )
        })?;
        let name = self
            .name
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .unwrap_or_else(|| "mcp-client".into());
        let hello = client.hello(&name)?;
        *self.hello.lock().unwrap_or_else(|e| e.into_inner()) = Some(hello);
        *slot = Some(client.clone());
        Ok(client)
    }

    fn connected(&self) -> bool {
        self.client
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .as_ref()
            .is_some_and(|c| c.is_alive())
    }

    /// Tell the MCP client to list tools again when what the instance
    /// offers changed since it last asked.
    fn notify_if_tools_changed(&self) {
        if !self.initialized.load(Ordering::Relaxed) {
            return;
        }
        let now = if self.connected() {
            self.features()
        } else {
            None
        };
        let listed = self
            .listed
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clone();
        if listed.is_some() && listed != now {
            *self.listed.lock().unwrap_or_else(|e| e.into_inner()) = now;
            self.send(&json!({"jsonrpc": "2.0", "method": "notifications/tools/list_changed"}));
        }
    }

    fn list_tools(&self) -> Value {
        let _ = self.connect();
        let features = if self.connected() {
            self.features()
        } else {
            None
        };
        *self.listed.lock().unwrap_or_else(|e| e.into_inner()) = features.clone();
        let tools: Vec<Value> = match &features {
            Some(features) => tools::available(features)
                .map(tools::Tool::describe)
                .collect(),
            // Disconnected: only the status tool, which explains how to connect.
            None => tools::TOOLS
                .iter()
                .take(1)
                .map(tools::Tool::describe)
                .collect(),
        };
        json!({"tools": tools})
    }

    fn call(&self, request: &Value, name: &str, args: Value) -> Value {
        let result = match self.connect() {
            Ok(client) => {
                let timeout = args
                    .get("timeout_ms")
                    .or_else(|| args.get("wait_ms"))
                    .and_then(Value::as_u64)
                    .unwrap_or(30_000)
                    .min(600_000);
                let (host_id, reply) =
                    client.send(json!({"op": "call", "tool": name, "args": args}));
                let key = request.to_string();
                self.in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(key.clone(), (client.clone(), host_id));
                let result = reply
                    .recv_timeout(Duration::from_millis(timeout) + Duration::from_secs(60))
                    .unwrap_or_else(|_| {
                        Err(ToolError::new(
                            ErrorCode::Timeout,
                            "Rodtty didn't answer in time",
                        ))
                    });
                self.in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&key);
                result
            }
            Err(error) if name == "rodtty_status" => Ok(json!({
                "connected": false,
                "app_root": self.root,
                "error": error,
                "diagnostics": [
                    "Start the Rodtty desktop app for this owner (the same RODTTY_HOME).",
                    "The channel is a local socket in the app root; no network port is used.",
                ],
            })),
            Err(error) => Err(error),
        };
        self.notify_if_tools_changed();
        tool_result(result)
    }

    fn cancel(&self, request: &Value) {
        let key = request.to_string();
        if let Some((client, host_id)) = self
            .in_flight
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .get(&key)
        {
            client.cancel(*host_id);
        }
    }
}

/// An MCP `CallToolResult`: structured content plus the same JSON as text.
pub fn tool_result(result: Result<Value, ToolError>) -> Value {
    let (value, error) = match result {
        Ok(value) => (value, false),
        Err(error) => (json!({"error": error}), true),
    };
    let text = serde_json::to_string_pretty(&value).unwrap_or_default();
    let structured = if value.is_object() {
        value
    } else {
        json!({"value": value})
    };
    json!({
        "content": [{"type": "text", "text": text}],
        "structuredContent": structured,
        "isError": error,
    })
}

/// Serve MCP on stdin and stdout until stdin closes.
pub fn serve(root: PathBuf, client_name: Option<String>) -> anyhow::Result<()> {
    let link = Arc::new(Link {
        root,
        name: Mutex::new(client_name.clone()),
        client: Mutex::new(None),
        hello: Mutex::new(None),
        listed: Mutex::new(None),
        initialized: AtomicBool::new(false),
        out: Mutex::new(std::io::stdout()),
        calls: AtomicUsize::new(0),
        in_flight: Default::default(),
    });
    let explicit_name = client_name.is_some();
    // Reconnect in the background so the client hears when tools appear.
    let watcher = Arc::downgrade(&link);
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_secs(2));
            let Some(link) = watcher.upgrade() else {
                return;
            };
            if link.initialized.load(Ordering::Relaxed) {
                if !link.connected() {
                    let _ = link.connect();
                }
                link.notify_if_tools_changed();
            }
        }
    });
    let stdin = std::io::stdin();
    for line in stdin.lock().lines() {
        let line = line?;
        if line.trim().is_empty() {
            continue;
        }
        let message: Value = match serde_json::from_str(&line) {
            Ok(message) => message,
            Err(_) => {
                link.fail(&Value::Null, -32700, "Parse error");
                continue;
            }
        };
        let id = message.get("id").cloned();
        let method = message
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = message.get("params").cloned().unwrap_or(json!({}));
        match (method, id) {
            ("initialize", Some(id)) => {
                if !explicit_name
                    && let Some(name) = params.pointer("/clientInfo/name").and_then(Value::as_str)
                {
                    *link.name.lock().unwrap_or_else(|e| e.into_inner()) = Some(name.to_owned());
                }
                let requested = params
                    .get("protocolVersion")
                    .and_then(Value::as_str)
                    .unwrap_or_default();
                let version = PROTOCOL_VERSIONS
                    .iter()
                    .find(|v| **v == requested)
                    .unwrap_or(&PROTOCOL_VERSIONS[0]);
                link.respond(&id, json!({
                    "protocolVersion": version,
                    "capabilities": {"tools": {"listChanged": true}},
                    "serverInfo": {"name": "rodtty", "title": "Rodtty", "version": env!("CARGO_PKG_VERSION")},
                    "instructions": "Rodtty automation. Call rodtty_status first: the owner must approve this client in Rodtty before other tools work. Target IDs are stable for the life of the Rodtty instance. Pass request_id to mutating tools to make retries safe."
                }));
            }
            ("notifications/initialized", None) => {
                link.initialized.store(true, Ordering::Relaxed);
            }
            ("notifications/cancelled", None) => {
                if let Some(request) = params.get("requestId") {
                    link.cancel(request);
                }
            }
            ("ping", Some(id)) => link.respond(&id, json!({})),
            ("tools/list", Some(id)) => {
                let tools = link.list_tools();
                link.respond(&id, tools);
            }
            ("tools/call", Some(id)) => {
                let name = params
                    .get("name")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                if tools::find(&name).is_none() {
                    link.fail(&id, -32602, &format!("Unknown tool: {name}"));
                    continue;
                }
                let args = params.get("arguments").cloned().unwrap_or(json!({}));
                if !args.is_object() {
                    link.fail(&id, -32602, "Tool arguments must be an object");
                    continue;
                }
                if link.calls.fetch_add(1, Ordering::SeqCst) >= MAX_CONCURRENT_CALLS {
                    link.calls.fetch_sub(1, Ordering::SeqCst);
                    link.respond(
                        &id,
                        tool_result(Err(ToolError::new(
                            ErrorCode::Limit,
                            "Too many concurrent tool calls",
                        ))),
                    );
                    continue;
                }
                let link = link.clone();
                std::thread::spawn(move || {
                    let result = link.call(&id, &name, args);
                    link.calls.fetch_sub(1, Ordering::SeqCst);
                    link.respond(&id, result);
                });
            }
            (_, Some(id)) => link.fail(&id, -32601, &format!("Method not found: {method}")),
            (_, None) => {}
        }
    }
    Ok(())
}

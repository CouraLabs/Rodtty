//! Local automation of a running Rodtty desktop instance.
//!
//! `rodtty mcp` speaks the Model Context Protocol over stdio and forwards tool
//! calls through an owner-scoped local channel ([`channel`]) to the desktop
//! instance, which runs them against its own state with the same actions the
//! UI uses. The owner approves, pauses and revokes clients ([`grants`]).
pub mod channel;
pub mod codex;
pub mod events;
pub mod grants;
pub mod keys;
pub mod mcp;
pub mod tools;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Why a tool call failed. Clients can branch on the code; the message is for people.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ErrorCode {
    InvalidParams,
    UnknownTool,
    /// The feature isn't built in or is turned off.
    Unavailable,
    /// The target existed but has closed or ended.
    ClosedTarget,
    /// No target has this ID in this instance.
    UnknownTarget,
    /// The owner hasn't approved this client yet.
    AuthorizationRequired,
    /// The owner paused this client.
    Paused,
    /// The owner revoked this client.
    Revoked,
    /// The grant doesn't include the scope this tool needs.
    ScopeDenied,
    /// The grant doesn't include the target's workspace.
    WorkspaceDenied,
    /// The request conflicts with current state: a stale revision, a reused
    /// request ID with different arguments, or changes on disk.
    Conflict,
    /// A coordinate or element reference no longer matches the target.
    Stale,
    Timeout,
    Cancelled,
    /// The desktop instance isn't reachable.
    Disconnected,
    /// The owner took over this target; agent input stays paused until the owner returns it.
    OwnerTakeover,
    /// The target can't receive this action.
    Unsupported,
    /// A bound on payloads, concurrency or artifacts was reached.
    Limit,
    /// The owner must decide before this can continue.
    WaitingForOwner,
    Internal,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolError {
    pub code: ErrorCode,
    pub message: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl ToolError {
    pub fn new(code: ErrorCode, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
            data: None,
        }
    }
    pub fn with_data(mut self, data: Value) -> Self {
        self.data = Some(data);
        self
    }
    pub fn invalid(message: impl Into<String>) -> Self {
        Self::new(ErrorCode::InvalidParams, message)
    }
}

impl std::fmt::Display for ToolError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{:?}: {}", self.code, self.message)
    }
}

pub type ToolResult = Result<Value, ToolError>;

/// A short random token for opaque IDs. Not a secret: the channel itself is
/// owner-only, and IDs only need to be unique and unguessable across restarts.
pub fn token() -> String {
    use std::sync::atomic::{AtomicU64, Ordering};
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_nanos() as u64);
    let mut x = nanos
        ^ (u64::from(std::process::id()) << 32)
        ^ COUNTER
            .fetch_add(1, Ordering::Relaxed)
            .wrapping_mul(0x9e37_79b9_7f4a_7c15);
    // splitmix64 finalizer
    x = (x ^ (x >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    x = (x ^ (x >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    x ^= x >> 31;
    format!("{x:016x}")
}

/// Read a required string argument.
pub fn arg_str<'a>(args: &'a Value, key: &str) -> Result<&'a str, ToolError> {
    args.get(key)
        .and_then(Value::as_str)
        .ok_or_else(|| ToolError::invalid(format!("`{key}` must be a string")))
}

pub fn arg_opt_str<'a>(args: &'a Value, key: &str) -> Result<Option<&'a str>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(s)) => Ok(Some(s)),
        Some(_) => Err(ToolError::invalid(format!("`{key}` must be a string"))),
    }
}

pub fn arg_u64(args: &Value, key: &str, default: u64) -> Result<u64, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_u64()
            .ok_or_else(|| ToolError::invalid(format!("`{key}` must be a non-negative integer"))),
    }
}

pub fn arg_f64(args: &Value, key: &str) -> Result<Option<f64>, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(None),
        Some(v) => v
            .as_f64()
            .map(Some)
            .ok_or_else(|| ToolError::invalid(format!("`{key}` must be a number"))),
    }
}

pub fn arg_bool(args: &Value, key: &str, default: bool) -> Result<bool, ToolError> {
    match args.get(key) {
        None | Some(Value::Null) => Ok(default),
        Some(v) => v
            .as_bool()
            .ok_or_else(|| ToolError::invalid(format!("`{key}` must be a boolean"))),
    }
}

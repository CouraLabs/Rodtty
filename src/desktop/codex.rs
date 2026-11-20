//! The optional structured Codex adapter (`codex app-server`). Not built yet:
//! it reports itself unavailable, and Codex stays usable as a terminal app.
use super::*;
use crate::automation::{ErrorCode, ToolError, ToolResult, grants::ClientGrant};
use serde_json::Value;
use std::sync::atomic::AtomicBool;

pub(crate) fn available() -> bool {
    false
}

pub(crate) fn run(_host: &automation::Host, _grant: &ClientGrant, name: &str, _args: &Value, _cancel: &AtomicBool) -> ToolResult {
    Err(ToolError::new(
        ErrorCode::Unavailable,
        format!("{name}: the structured Codex adapter is not available; launch Codex in a terminal with terminal_launch"),
    ))
}

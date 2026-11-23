# Automating Rodtty with MCP

`rodtty mcp` is a Model Context Protocol server on stdio. An MCP client (Codex, Claude Code, an IDE, or the TypeScript SDK) starts it as a subprocess; it forwards each tool call to the Rodtty desktop instance that is already running for the same owner. It never starts a second owner instance.

## Connect a client

Codex (`~/.codex/config.toml`):

```toml
[mcp_servers.rodtty]
command = "/Applications/Rodtty.app/Contents/MacOS/rodtty"
args = ["mcp"]
```

Any client that takes a JSON server list:

```json
{ "mcpServers": { "rodtty": { "command": "/Applications/Rodtty.app/Contents/MacOS/rodtty", "args": ["mcp"] } } }
```

`--client-name NAME` overrides the name the client reports in `initialize`. Set `RODTTY_HOME` only when the desktop instance uses a non-default app root.

## Approve the client

The first call from a new client creates a pending request; every tool except `rodtty_status` answers `authorization_required` until the owner approves it. The footer shows `Automation: <client> wants access`.

- In Rodtty: command palette → **Automation: Approve Pending Client** approves the oldest request for all scopes on the active workspace.
- In a shell: `rodtty automation list`, then `rodtty automation approve <client-id> --workspace <project folder|ws_…|all> [--scopes read,terminal,input,workspace,editor,browser,capture]`.

**Automation: Pause All Clients**, **Resume Paused Clients** and **Revoke All Clients** (or `rodtty automation pause|resume|revoke|forget <client>`) take effect on the next request. Pausing or revoking cancels the client's jobs and queued input and releases its held keys and buttons. Grants live in `<app root>/automation-clients.json` (mode 0600). The channel is owner-scoped, not a security boundary against other programs running as the same user: those could edit the grant file directly.

### Scopes

| Scope | Allows |
| --- | --- |
| `read` | Workspace, terminal, editor, Git and browser state; events and jobs |
| `terminal` | Create, launch, type into, interrupt and close terminals; Codex App Server threads |
| `input` | Virtual pointer and keyboard (`input_*`) |
| `workspace` | Switch workspaces, show the window, run view-only palette actions (`app_action`), show a diff |
| `editor` | Open, edit and save buffers |
| `browser` | Create, navigate and operate the client's own browser targets |
| `capture` | Screenshots and browser recordings |

A grant also names workspaces. A target outside them is invisible to the client: terminals by the workspace they started in, files by their resolved path (symlinks included), browser targets by the client that created them. The owner's own Browser view is never an agent target. `app_action` runs only actions that change what the window shows (toggling panes, find, search, the changes view, command navigation, filters and pane focus); `app_action` without a command lists them. Every other palette action is reserved for the owner, including remote access and pairing, paste, file operations, settings toggles, workspace removal, `automation-*`, `codex-*` and Quit.

## Tools

`tools/list` advertises only what the running instance supports: terminal-only builds omit `browser_*` and `recording_*`; `codex_*` appears when `codex app-server` is installed. When Rodtty isn't running, only `rodtty_status` is listed (it explains how to connect), and the server sends `notifications/tools/list_changed` once it connects.

- **Terminals**: `terminal_list`, `terminal_create`, `terminal_launch` (executable, arguments and directory, quoted for the shell; returns terminal, shell and foreground process IDs, and a job that ends with the program's exit status), `terminal_screen` (rows, columns, lines, cursor, revision, alternate screen, bracketed paste, recent command results), `terminal_read` (output after a byte cursor, with truncation), `terminal_wait` (text, regex, screen text, screen change, idle or exit), `terminal_send_text` (multiline text is one paste, bracketed when the app enabled it; never presses Enter), `terminal_send_keys` (`Enter`, `Ctrl+C`, `Shift+Tab`, `F5`…), `terminal_interrupt`, `terminal_close` (a running command asks the owner, following the tab-close policy, and returns `waiting_for_owner`).
- **Virtual input**: `input_state`, `input_key` (press, down, up; held modifiers apply to later keys), `input_type` (Unicode, optional delay), `input_pointer` (move, down, up, click, double_click, drag, wheel in viewport CSS pixels for browsers and cells for terminals, with `frame_revision` to reject stale coordinates), `input_release_all`.
- **Browser**: `browser_open`, `browser_navigate`, `browser_history`, `browser_status`, `browser_snapshot` (roles, accessible names, values, bounds and element references for the current document), `browser_click`, `browser_type`, `browser_press`, `browser_scroll`, `browser_wait`, `browser_close`.
- **Capture**: `capture_screenshot` (a browser viewport or terminal; returns path, dimensions, scale and source), `recording_start`, `recording_status`, `recording_stop`.
- **Workspace, editor, Git**: `workspace_list`, `workspace_switch`, `window_show`, `app_action`, `files_search`, `editor_list`, `editor_open`, `editor_read`, `editor_edit` (requires the revision you read), `editor_save` (version-protected; a disk change is a conflict and nothing is written), `git_changes`, `git_diff`.
- **Coordination**: `events_read`, `job_status`, `job_wait`, `job_cancel`.
- **Codex App Server**: `codex_thread_start`, `codex_turn_start`, `codex_turn_status`, `codex_turn_interrupt`.

Errors are tool results with `isError: true` and `structuredContent.error.code`: `invalid_params`, `unavailable`, `closed_target`, `unknown_target`, `authorization_required`, `paused`, `revoked`, `scope_denied`, `workspace_denied`, `conflict`, `stale`, `timeout`, `cancelled`, `disconnected`, `owner_takeover`, `unsupported`, `limit`, `waiting_for_owner`.

## IDs, retries, jobs and events

- Target IDs (`term_…`, `brw_…`, `ed_…`, `job_…`, `rec_…`, `art_…`, `cl_…`, `cdx_…`) are opaque and stable for the life of the desktop instance. An ID from an earlier run answers `unknown_target`; a closed target answers `closed_target`.
- Every mutating tool takes `request_id`. Repeating a call with the same `request_id` returns the first result with `replayed: true` instead of acting again; reusing it with different arguments is a `conflict`. Retries are safe across reconnects while the instance runs.
- Long operations return job IDs: launches, waits, browser navigation and Codex turns. States are `pending`, `running`, `completed`, `failed`, `cancelled` and `waiting_for_owner`. `job_cancel` stops only that operation: cancelling a wait leaves the program running.
- `events_read` returns ordered events (`session`, `command`, `input`, `input_ownership`, `job`, `browser`, `navigation`, `recording`, `approval`, `client`) after a cursor. The history keeps 2048 events; a reader whose cursor fell out of it gets `expired: true` and should resynchronize from `terminal_list` and `browser_list`.
- An MCP `notifications/cancelled` cancels the matching call in Rodtty.

## Reconnect and ownership

If the desktop instance stops or the channel drops, calls in flight fail with `disconnected` and are not retried; the next call reconnects. Nothing is replayed on reconnect: resend with the same `request_id` if unsure whether it ran.

Agent input reaches only the named target and never moves the physical pointer, types into the operating system, reads or writes the clipboard, or activates the window (`window_show` with `focus: true` is the explicit exception). When the owner types or pastes into a terminal an agent is driving, the owner takes over: the agent's queued input stops, its held keys and buttons are released, and further agent input gets `owner_takeover` until the owner runs **Automation: Return Target to Agent**. **Automation: Take Over Agent Target** does the same without typing. A client disconnect releases its held input too.

## Limits

64 KiB of text per `terminal_send_text`, 16 KiB per `input_type`, 256 KiB per `terminal_read`, 1 MiB of retained raw output per terminal, 8 concurrent calls per MCP server and 16 per channel connection, waits up to 10 minutes, recordings up to 600 s, 30 fps, 3840 px wide and 1 GiB. Artifacts go to `<app root>/artifacts`.

## Diagnostics

- `rodtty_status` reports the connection, this client's grant, the instance ID and features. Disconnected, it names the app root it looked in.
- The channel is `<app root>/automation.sock` (Unix, mode 0600) or a loopback port whose number and secret are in `<app root>/automation.endpoint` (Windows). No network interface is used, and it is separate from remote browser access.
- Protocol messages go only to standard output; `rodtty mcp` diagnostics go to standard error.

## Checks

`scripts/automation/` holds the checks that drive a running Rodtty through the official MCP TypeScript SDK (`npm install` there first): `terminal-check.mjs`, `browser-check.mjs`, `tls-check.mjs`, `e2e.mjs`, `codex-adapter-check.mjs`, `codex-smoke.mjs` and `codex-appserver-smoke.mjs`. Each takes `RODTTY` (the executable) and `RODTTY_HOME` (the running instance's app root). See [automation evidence](evidence/automation/README.md) for recorded runs.

### Interactive apps

An app sees exactly what a person typing would send. Codex treats an Enter that arrives within about a second of a paste as part of the paste; wait for the pasted text to settle (`terminal_wait` with `screen_contains`, or a short pause) before pressing Enter. `codex-smoke.mjs` shows the sequence.

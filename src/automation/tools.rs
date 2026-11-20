//! The tool catalog: names, scopes, required features and input schemas.
//! The desktop instance reports which features it has; `rodtty mcp` lists
//! only the tools whose feature is present.
use super::grants::Scope;
use serde_json::{Value, json};

pub struct Tool {
    pub name: &'static str,
    pub description: &'static str,
    /// `None` for the status tool, which works before approval.
    pub scope: Option<Scope>,
    /// A feature the running instance must report, such as `browser`.
    pub feature: Option<&'static str>,
    /// Whether the tool changes state and so takes a `request_id`.
    pub mutating: bool,
    pub properties: fn() -> Value,
    pub required: &'static [&'static str],
}

impl Tool {
    pub fn input_schema(&self) -> Value {
        let mut properties = (self.properties)();
        if self.mutating {
            properties["request_id"] = json!({
                "type": "string",
                "description": "Client-chosen ID that makes this call retry-safe: repeating it returns the first result instead of acting again."
            });
        }
        json!({
            "type": "object",
            "properties": properties,
            "required": self.required,
            "additionalProperties": false,
        })
    }

    pub fn describe(&self) -> Value {
        json!({
            "name": self.name,
            "description": self.description,
            "inputSchema": self.input_schema(),
        })
    }
}

fn terminal() -> Value {
    json!({"type": "string", "description": "Terminal target ID, from terminal_list"})
}
fn browser() -> Value {
    json!({"type": "string", "description": "Browser target ID, from browser_list or browser_open"})
}
fn none() -> Value {
    json!({})
}

macro_rules! props {
    ($($key:literal : $value:expr),* $(,)?) => {{
        fn build() -> Value {
            let mut map = serde_json::Map::new();
            $(map.insert($key.to_owned(), $value);)*
            Value::Object(map)
        }
        build
    }};
}

pub static TOOLS: &[Tool] = &[
    Tool {
        name: "rodtty_status",
        description: "Connection state, this client's grant (status, scopes, workspaces), the instance ID, and the features this Rodtty build supports. Works before the owner approves the client.",
        scope: None,
        feature: None,
        mutating: false,
        properties: none,
        required: &[],
    },
    Tool {
        name: "workspace_list",
        description: "Workspaces this client may access, with IDs, names, roots, and which one is active.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: none,
        required: &[],
    },
    Tool {
        name: "workspace_switch",
        description: "Switch Rodtty's active workspace. Live terminal sessions and unsaved edits stay attached to their workspaces.",
        scope: Some(Scope::Workspace),
        feature: Some("workspaces"),
        mutating: true,
        properties: props!("workspace": json!({"type":"string"})),
        required: &["workspace"],
    },
    Tool {
        name: "window_show",
        description: "Show the Rodtty window. Pass focus=true to also activate it; otherwise the owner's active application keeps focus.",
        scope: Some(Scope::Workspace),
        feature: None,
        mutating: false,
        properties: props!("focus": json!({"type":"boolean","default":false})),
        required: &[],
    },
    Tool {
        name: "app_action",
        description: "Run a Rodtty command-palette action by its command ID (the same action the palette and shortcuts run). Lists the available IDs when called without one.",
        scope: Some(Scope::Workspace),
        feature: None,
        mutating: true,
        properties: props!("command": json!({"type":"string","description":"Command ID such as toggle-explorer"})),
        required: &[],
    },
    Tool {
        name: "terminal_list",
        description: "Terminal sessions in authorized workspaces: ID, title, directory, workspace, busy state, size, exit status, and whether the owner has taken it over.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: none,
        required: &[],
    },
    Tool {
        name: "terminal_create",
        description: "Open a new terminal tab with a fresh shell. Does not switch the owner's visible tab unless show=true.",
        scope: Some(Scope::Terminal),
        feature: None,
        mutating: true,
        properties: props!(
            "workspace": json!({"type":"string","description":"Workspace ID; defaults to the active workspace"}),
            "cwd": json!({"type":"string","description":"Absolute working directory inside the workspace"}),
            "show": json!({"type":"boolean","default":false})
        ),
        required: &[],
    },
    Tool {
        name: "terminal_launch",
        description: "Open a new terminal tab and run an executable with arguments in it, quoted for the shell. Returns the terminal ID, the shell and foreground process IDs, and a job that completes when the program exits.",
        scope: Some(Scope::Terminal),
        feature: None,
        mutating: true,
        properties: props!(
            "workspace": json!({"type":"string"}),
            "cwd": json!({"type":"string","description":"Absolute working directory inside the workspace"}),
            "executable": json!({"type":"string"}),
            "args": json!({"type":"array","items":{"type":"string"},"default":[]}),
            "title": json!({"type":"string"}),
            "show": json!({"type":"boolean","default":false})
        ),
        required: &["executable"],
    },
    Tool {
        name: "terminal_split",
        description: "Split a terminal tab into two panes.",
        scope: Some(Scope::Terminal),
        feature: Some("split"),
        mutating: true,
        properties: props!("terminal": terminal(), "orientation": json!({"enum":["horizontal","vertical"]})),
        required: &["terminal"],
    },
    Tool {
        name: "terminal_screen",
        description: "The terminal's visible screen: rows, columns, text lines, cursor, title, directory, revision, alternate-screen and bracketed-paste state, and recent command results.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("terminal": terminal()),
        required: &["terminal"],
    },
    Tool {
        name: "terminal_read",
        description: "Output printed after a cursor, as plain text. Returns the next cursor and whether bytes before it were already evicted (truncated).",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!(
            "terminal": terminal(),
            "cursor": json!({"type":"integer","minimum":0,"description":"Omit to read the retained tail"}),
            "max_bytes": json!({"type":"integer","minimum":1,"maximum":262144,"default":65536})
        ),
        required: &["terminal"],
    },
    Tool {
        name: "terminal_wait",
        description: "Wait, bounded by timeout_ms, until output after a cursor contains text or matches a regex, the screen changes after a revision, the shell returns to its prompt, or the session exits. Cancelling the wait leaves the program running.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!(
            "terminal": terminal(),
            "contains": json!({"type":"string"}),
            "regex": json!({"type":"string"}),
            "cursor": json!({"type":"integer","minimum":0}),
            "screen_contains": json!({"type":"string","description":"Text that must appear on the visible screen"}),
            "changed_since_revision": json!({"type":"integer","minimum":0}),
            "idle": json!({"type":"boolean","description":"Complete when no command is running"}),
            "exit": json!({"type":"boolean","description":"Complete when the session ends"}),
            "timeout_ms": json!({"type":"integer","minimum":0,"maximum":600000,"default":30000})
        ),
        required: &["terminal"],
    },
    Tool {
        name: "terminal_send_text",
        description: "Type text into the terminal's PTY without pressing Enter. Multiline and Unicode text is delivered as one paste, bracketed when the application enabled bracketed paste. Use terminal_send_keys with Enter to submit.",
        scope: Some(Scope::Terminal),
        feature: None,
        mutating: true,
        properties: props!("terminal": terminal(), "text": json!({"type":"string","maxLength":65536})),
        required: &["terminal", "text"],
    },
    Tool {
        name: "terminal_send_keys",
        description: "Press keys in the terminal, in order: names like Enter, Tab, Escape, Up, Ctrl+C, Alt+F, F5. Encoded the way Rodtty encodes the owner's keys.",
        scope: Some(Scope::Terminal),
        feature: None,
        mutating: true,
        properties: props!("terminal": terminal(), "keys": json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":256})),
        required: &["terminal", "keys"],
    },
    Tool {
        name: "terminal_compare",
        description: "Compare two finished command runs in a terminal by their command IDs (from terminal_screen recent_commands; default: the last two). Returns each run's exit status and whether its output was complete, plus a unified diff with formatting normalized.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("terminal": terminal(), "before": json!({"type":"integer"}), "after": json!({"type":"integer"})),
        required: &["terminal"],
    },
    Tool {
        name: "terminal_interrupt",
        description: "Send an interrupt (Ctrl+C) to the terminal's foreground program.",
        scope: Some(Scope::Terminal),
        feature: None,
        mutating: true,
        properties: props!("terminal": terminal()),
        required: &["terminal"],
    },
    Tool {
        name: "terminal_close",
        description: "Close a terminal tab. A session with a running command asks the owner, following Rodtty's close policy, and returns waiting_for_owner.",
        scope: Some(Scope::Terminal),
        feature: None,
        mutating: true,
        properties: props!("terminal": terminal()),
        required: &["terminal"],
    },
    Tool {
        name: "input_state",
        description: "This client's virtual input state on a target: pointer position, held buttons and modifiers, and whether the owner has taken the target over.",
        scope: Some(Scope::Input),
        feature: None,
        mutating: false,
        properties: props!("target": json!({"type":"string"})),
        required: &["target"],
    },
    Tool {
        name: "input_key",
        description: "Virtual keyboard: press, hold (down) or release (up) a key or modifier on a terminal or browser target. Held modifiers apply to later keys until released. Input goes straight to the target, never through the operating system.",
        scope: Some(Scope::Input),
        feature: None,
        mutating: true,
        properties: props!(
            "target": json!({"type":"string"}),
            "key": json!({"type":"string","description":"Key or chord, e.g. Enter, Ctrl+C, Shift"}),
            "action": json!({"enum":["press","down","up"],"default":"press"})
        ),
        required: &["target", "key"],
    },
    Tool {
        name: "input_type",
        description: "Virtual keyboard: type Unicode text key by key into a target, optionally with a delay between keys. Pause, revoke or owner takeover cancels the rest.",
        scope: Some(Scope::Input),
        feature: None,
        mutating: true,
        properties: props!(
            "target": json!({"type":"string"}),
            "text": json!({"type":"string","maxLength":16384}),
            "delay_ms": json!({"type":"integer","minimum":0,"maximum":1000,"default":0})
        ),
        required: &["target", "text"],
    },
    Tool {
        name: "input_pointer",
        description: "Virtual pointer on a target, in viewport-local CSS pixels (browser) or cells (terminal): move, down, up, click, double_click, drag, or wheel. Pass the frame revision you observed; a stale one is rejected.",
        scope: Some(Scope::Input),
        feature: None,
        mutating: true,
        properties: props!(
            "target": json!({"type":"string"}),
            "action": json!({"enum":["move","down","up","click","double_click","drag","wheel"]}),
            "x": json!({"type":"number"}),
            "y": json!({"type":"number"}),
            "to_x": json!({"type":"number"}),
            "to_y": json!({"type":"number"}),
            "button": json!({"enum":["left","middle","right"],"default":"left"}),
            "delta_x": json!({"type":"number"}),
            "delta_y": json!({"type":"number"}),
            "steps": json!({"type":"integer","minimum":1,"maximum":200,"default":1}),
            "frame_revision": json!({"type":"integer","minimum":0})
        ),
        required: &["target", "action"],
    },
    Tool {
        name: "input_release_all",
        description: "Release this client's held keys and buttons on a target and cancel its queued input.",
        scope: Some(Scope::Input),
        feature: None,
        mutating: false,
        properties: props!("target": json!({"type":"string"})),
        required: &["target"],
    },
    Tool {
        name: "events_read",
        description: "Ordered events after a cursor: sessions, command exits, output, browser navigation, recording and input ownership. If expired is true, history before the cursor was dropped: take a snapshot (terminal_list, browser_list) and continue from next_cursor.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!(
            "cursor": json!({"type":"integer","minimum":0,"description":"Omit to start from now"}),
            "max": json!({"type":"integer","minimum":1,"maximum":500,"default":100}),
            "wait_ms": json!({"type":"integer","minimum":0,"maximum":60000,"default":0})
        ),
        required: &[],
    },
    Tool {
        name: "job_status",
        description: "A job's state (pending, running, completed, failed, cancelled, waiting_for_owner) and result. Omit job to list this client's jobs.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("job": json!({"type":"string"})),
        required: &[],
    },
    Tool {
        name: "job_wait",
        description: "Wait, bounded by timeout_ms, for a job to finish.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("job": json!({"type":"string"}), "timeout_ms": json!({"type":"integer","minimum":0,"maximum":600000,"default":30000})),
        required: &["job"],
    },
    Tool {
        name: "job_cancel",
        description: "Cancel one job. A cancelled wait leaves its terminal program running.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("job": json!({"type":"string"})),
        required: &["job"],
    },
    Tool {
        name: "files_search",
        description: "Search file names and contents under an authorized workspace root, as Rodtty's Search does.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!(
            "query": json!({"type":"string"}),
            "workspace": json!({"type":"string"}),
            "max": json!({"type":"integer","minimum":1,"maximum":500,"default":100})
        ),
        required: &["query"],
    },
    Tool {
        name: "editor_list",
        description: "Open editor buffers: ID, path, revision, and whether each has unsaved changes.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: none,
        required: &[],
    },
    Tool {
        name: "editor_open",
        description: "Open a file in Rodtty's editor at a line and column, reusing an existing buffer (and its unsaved changes) when the file is already open.",
        scope: Some(Scope::Editor),
        feature: None,
        mutating: true,
        properties: props!(
            "path": json!({"type":"string"}),
            "line": json!({"type":"integer","minimum":1}),
            "column": json!({"type":"integer","minimum":1})
        ),
        required: &["path"],
    },
    Tool {
        name: "editor_read",
        description: "An editor buffer's current text (which may differ from the saved file), its revision, dirty state, and the saved file's text when requested.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("editor": json!({"type":"string"}), "include_saved": json!({"type":"boolean","default":false})),
        required: &["editor"],
    },
    Tool {
        name: "editor_edit",
        description: "Replace text in a buffer at the revision you read. A different current revision is rejected as a conflict. Edits are unsaved until editor_save.",
        scope: Some(Scope::Editor),
        feature: None,
        mutating: true,
        properties: props!(
            "editor": json!({"type":"string"}),
            "expected_revision": json!({"type":"integer","minimum":0}),
            "find": json!({"type":"string","description":"Exact text to replace; must occur once"}),
            "replace": json!({"type":"string"}),
            "text": json!({"type":"string","description":"Replace the whole buffer instead"})
        ),
        required: &["editor", "expected_revision"],
    },
    Tool {
        name: "editor_save",
        description: "Save a buffer at the revision you read, with Rodtty's version-protected save: a file changed on disk is reported as a conflict and both versions are kept.",
        scope: Some(Scope::Editor),
        feature: None,
        mutating: true,
        properties: props!("editor": json!({"type":"string"}), "expected_revision": json!({"type":"integer","minimum":0})),
        required: &["editor", "expected_revision"],
    },
    Tool {
        name: "git_changes",
        description: "Staged, unstaged and untracked changes in a workspace's repository or worktree. Read-only.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("workspace": json!({"type":"string"})),
        required: &[],
    },
    Tool {
        name: "git_diff",
        description: "A read-only unified diff for one file: staged (index vs HEAD) or unstaged (working tree vs index). show=true opens it in Rodtty at its first hunk.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!(
            "path": json!({"type":"string"}),
            "staged": json!({"type":"boolean","default":false}),
            "show": json!({"type":"boolean","default":false})
        ),
        required: &["path"],
    },
    Tool {
        name: "tasks_list",
        description: "The owner-defined project tasks of a workspace: name, command and folder. Defined by the owner in Rodtty (Tasks: Edit Project Tasks); clients cannot create them.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("workspace": json!({"type":"string"})),
        required: &[],
    },
    Tool {
        name: "task_run",
        description: "Run an owner-defined task in its own titled terminal. If it is already running, returns that terminal instead of starting a copy. Follow it with terminal_wait (idle) and terminal_screen; cancel with terminal_interrupt.",
        scope: Some(Scope::Terminal),
        feature: None,
        mutating: true,
        properties: props!("name": json!({"type":"string"}), "workspace": json!({"type":"string"})),
        required: &["name"],
    },
    Tool {
        name: "services_list",
        description: "Local TCP listeners on this host: port, address, process, and the terminal running each one when the process tree verifies it (`owner`: terminal ID, `external`, or `unknown`). Read-only.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: none,
        required: &[],
    },
    Tool {
        name: "project_environment",
        description: "Compare the versions a workspace's .node-version, global.json and rust-toolchain(.toml) expect with node, dotnet and rustc on Rodtty's PATH. Manifests are parsed as data; only fixed version commands run, each with a timeout.",
        scope: Some(Scope::Read),
        feature: None,
        mutating: false,
        properties: props!("workspace": json!({"type":"string"})),
        required: &[],
    },
    Tool {
        name: "browser_list",
        description: "Embedded browser targets: ID, URL, title, loading state, whether the owner's Browser view shows it, and recording state.",
        scope: Some(Scope::Read),
        feature: Some("browser"),
        mutating: false,
        properties: none,
        required: &[],
    },
    Tool {
        name: "browser_open",
        description: "Create a background browser target for this client, optionally navigating it. It renders and takes input without being visible; show=true shows it in the Browser view.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!(
            "url": json!({"type":"string"}),
            "width": json!({"type":"integer","minimum":200,"maximum":3840,"default":1280}),
            "height": json!({"type":"integer","minimum":200,"maximum":2160,"default":800}),
            "show": json!({"type":"boolean","default":false})
        ),
        required: &[],
    },
    Tool {
        name: "browser_navigate",
        description: "Navigate a browser target to an HTTP(S) address. Bare local addresses like localhost:4200 use HTTP; other bare hosts use HTTPS. Returns a job that completes when loading ends.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!("browser": browser(), "url": json!({"type":"string"}), "wait": json!({"type":"boolean","default":true})),
        required: &["browser", "url"],
    },
    Tool {
        name: "browser_history",
        description: "Back, forward, reload or stop.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!("browser": browser(), "action": json!({"enum":["back","forward","reload","stop"]})),
        required: &["browser", "action"],
    },
    Tool {
        name: "browser_status",
        description: "URL, title, loading state, last navigation error, viewport size and scale, and frame and document revisions.",
        scope: Some(Scope::Read),
        feature: Some("browser"),
        mutating: false,
        properties: props!("browser": browser()),
        required: &["browser"],
    },
    Tool {
        name: "browser_snapshot",
        description: "A bounded page snapshot: URL, title, and interactive elements with role, accessible name, value, bounds in viewport CSS pixels, and a reference valid for this document revision.",
        scope: Some(Scope::Read),
        feature: Some("browser"),
        mutating: false,
        properties: props!("browser": browser(), "max_elements": json!({"type":"integer","minimum":1,"maximum":1000,"default":200})),
        required: &["browser"],
    },
    Tool {
        name: "browser_click",
        description: "Click an element by reference (from browser_snapshot) or at viewport coordinates, through the virtual pointer: it moves, presses and releases like a person.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!(
            "browser": browser(),
            "ref": json!({"type":"string"}),
            "x": json!({"type":"number"}),
            "y": json!({"type":"number"}),
            "button": json!({"enum":["left","middle","right"],"default":"left"}),
            "count": json!({"type":"integer","minimum":1,"maximum":3,"default":1})
        ),
        required: &["browser"],
    },
    Tool {
        name: "browser_type",
        description: "Type text through the virtual keyboard, after clicking the referenced element to focus it when ref is given.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!(
            "browser": browser(),
            "ref": json!({"type":"string"}),
            "text": json!({"type":"string","maxLength":16384}),
            "delay_ms": json!({"type":"integer","minimum":0,"maximum":1000,"default":0})
        ),
        required: &["browser", "text"],
    },
    Tool {
        name: "browser_press",
        description: "Press key chords in the page through the virtual keyboard, e.g. Enter, Tab, Ctrl+A.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!("browser": browser(), "keys": json!({"type":"array","items":{"type":"string"},"minItems":1,"maxItems":64})),
        required: &["browser", "keys"],
    },
    Tool {
        name: "browser_scroll",
        description: "Scroll with the virtual wheel at a point (default: viewport center), or scroll the referenced element into view.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!(
            "browser": browser(),
            "ref": json!({"type":"string"}),
            "x": json!({"type":"number"}),
            "y": json!({"type":"number"}),
            "delta_x": json!({"type":"number","default":0}),
            "delta_y": json!({"type":"number","default":0})
        ),
        required: &["browser"],
    },
    Tool {
        name: "browser_wait",
        description: "Wait, bounded by timeout_ms, until loading finishes, the URL contains text, page text contains text, or an element with a role or name appears.",
        scope: Some(Scope::Read),
        feature: Some("browser"),
        mutating: false,
        properties: props!(
            "browser": browser(),
            "url_contains": json!({"type":"string"}),
            "text": json!({"type":"string"}),
            "selector": json!({"type":"string"}),
            "loaded": json!({"type":"boolean"}),
            "timeout_ms": json!({"type":"integer","minimum":0,"maximum":120000,"default":15000})
        ),
        required: &["browser"],
    },
    Tool {
        name: "browser_close",
        description: "Close a browser target this client created, releasing its renderer. An active recording is finalized first.",
        scope: Some(Scope::Browser),
        feature: Some("browser"),
        mutating: true,
        properties: props!("browser": browser()),
        required: &["browser"],
    },
    Tool {
        name: "capture_screenshot",
        description: "A PNG of exactly one surface: a browser viewport (rendered by the browser, with the virtual cursor only when cursor=true), a terminal pane (its screen, rendered by Rodtty), or the Rodtty window. Saved in the artifacts folder; returns its path, dimensions, scale and source.",
        scope: Some(Scope::Capture),
        feature: None,
        mutating: true,
        properties: props!(
            "target": json!({"type":"string","description":"Browser or terminal ID, or `window`"}),
            "cursor": json!({"type":"boolean","default":false})
        ),
        required: &["target"],
    },
    Tool {
        name: "recording_start",
        description: "Record a browser target's own rendered frames to a video file, while it runs in the background or behind other windows. Never records the desktop. Rodtty shows a recording indicator with a Stop action.",
        scope: Some(Scope::Capture),
        feature: Some("recording"),
        mutating: true,
        properties: props!(
            "browser": browser(),
            "fps": json!({"type":"integer","minimum":1,"maximum":30,"default":15}),
            "max_seconds": json!({"type":"integer","minimum":1,"maximum":600,"default":120}),
            "max_width": json!({"type":"integer","minimum":160,"maximum":3840,"default":1920}),
            "cursor": json!({"type":"boolean","default":true})
        ),
        required: &["browser"],
    },
    Tool {
        name: "recording_status",
        description: "A recording's state, frames written and dropped, duration and size.",
        scope: Some(Scope::Capture),
        feature: Some("recording"),
        mutating: false,
        properties: props!("recording": json!({"type":"string"})),
        required: &["recording"],
    },
    Tool {
        name: "recording_stop",
        description: "Stop and finalize a recording. Returns the artifact path, format, duration, frame count, size and status.",
        scope: Some(Scope::Capture),
        feature: Some("recording"),
        mutating: true,
        properties: props!("recording": json!({"type":"string"})),
        required: &["recording"],
    },
    Tool {
        name: "codex_thread_start",
        description: "Start (or resume) a structured Codex App Server thread in an authorized workspace.",
        scope: Some(Scope::Terminal),
        feature: Some("codex"),
        mutating: true,
        properties: props!("workspace": json!({"type":"string"}), "cwd": json!({"type":"string"}), "thread": json!({"type":"string","description":"Resume this thread ID"})),
        required: &[],
    },
    Tool {
        name: "codex_turn_start",
        description: "Start a turn on a Codex thread with a prompt. Returns a job that follows the turn's progress and approvals.",
        scope: Some(Scope::Terminal),
        feature: Some("codex"),
        mutating: true,
        properties: props!("thread": json!({"type":"string"}), "prompt": json!({"type":"string"})),
        required: &["thread", "prompt"],
    },
    Tool {
        name: "codex_turn_status",
        description: "A Codex turn's progress items, pending approval request, and completion.",
        scope: Some(Scope::Read),
        feature: Some("codex"),
        mutating: false,
        properties: props!("thread": json!({"type":"string"}), "turn": json!({"type":"string"})),
        required: &["thread"],
    },
    Tool {
        name: "codex_turn_interrupt",
        description: "Interrupt a running Codex turn.",
        scope: Some(Scope::Terminal),
        feature: Some("codex"),
        mutating: true,
        properties: props!("thread": json!({"type":"string"}), "turn": json!({"type":"string"})),
        required: &["thread"],
    },
];

pub fn find(name: &str) -> Option<&'static Tool> {
    TOOLS.iter().find(|t| t.name == name)
}

/// The tools an instance with `features` offers.
pub fn available<'a>(features: &'a [String]) -> impl Iterator<Item = &'static Tool> + 'a {
    TOOLS
        .iter()
        .filter(move |t| t.feature.is_none_or(|f| features.iter().any(|x| x == f)))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_are_unique_and_schemas_list_their_required_properties() {
        let mut names: Vec<_> = TOOLS.iter().map(|t| t.name).collect();
        names.sort();
        names.dedup();
        assert_eq!(names.len(), TOOLS.len());
        for tool in TOOLS {
            let schema = tool.input_schema();
            for required in tool.required {
                assert!(
                    schema["properties"].get(*required).is_some(),
                    "{} {required}",
                    tool.name
                );
            }
        }
        let terminal_only: Vec<_> = available(&[]).map(|t| t.name).collect();
        assert!(terminal_only.contains(&"terminal_launch"));
        assert!(!terminal_only.contains(&"browser_open"));
    }
}

# Session action policies

Behavior settings saves `behavior.window_close`, `behavior.terminal_tab_close`, `behavior.new_terminal_directory`, and `behavior.specified_directory` in the owner JSON. Controls are searchable, apply immediately, and reset individually. Valid external JSON edits use the same live preferences. Unknown fields inside `behavior` survive UI saves.

Window close defaults to Keep sessions running. Ask each time offers Keep sessions running / Quit Rodtty / Cancel. Quit Rodtty uses the explicit Quit flow, which confirms running commands independently of the terminal-tab close policy. Dirty editor Save / Discard / Cancel is resolved before any applicable window or quit decision; a protected save must finish successfully before its continuation closes the window.

Terminal-tab close defaults to Confirm when commands are running, with Always confirm and Close immediately alternatives. Close buttons, middle clicks, context menus, workspace menus, shortcuts and command-palette actions use the same action boundary. An alive shell at its prompt is idle; a shell builtin such as `read`, a foreground host program, and a known background job are protected. An exited shell needs no running-command confirmation.

Each session augments its owner's PS0 and appends to PROMPT_COMMAND (including indexed arrays). Invisible OSC command boundaries avoid matching visible prompt text. Direct shell job snapshots retain Brush's internal async jobs, which disappear from a subshell's job registry. Unix foreground process groups and PID liveness supplement those snapshots. Brush jobs without a representative PID remain conservatively protected until the next authoritative prompt refresh, for example after `wait`. Asynchronous completion alone does not clear this protection. ConPTY also conservatively retains known background jobs until the next prompt. Native Windows/macOS behavior remains pending runtime verification.

New tabs default to the active terminal's directory. Alternatives use the chosen explorer root or a specified absolute folder. Invalid/missing folders display an explanation without creating a replacement session. The first terminal still uses the explicit startup directory, launcher directory or home fallback. Running sessions keep their existing command selection, history configuration and startup environment; new preferences apply to new sessions.

## Evidence

The application runtime exercises 36 terminal close combinations: three policies × idle/running shell × close button, middle click, context menu, shortcut, workspace menu and command palette. Additional cases cover all window choices, dirty Cancel/Discard precedence, immediate-tab-close independence from explicit/window Quit confirmation, searchable persistence/reset/external edits, all directory choices and unchanged first-session startup.

Real PTY coverage exercises idle prompts, `read`, bundled and explicit host commands, interrupts, internal and external background syntax followed by `wait`, exited shells, preserved owner PS0/PROMPT_COMMAND arrays, history, cwd tracking and unchanged per-session configuration snapshots.

Native checks passed on Linux x86-64 for [Wayland](evidence/session-policy-wayland.json) and actual [Xlib](evidence/session-policy-xlib.json). These exercise native Ask close/cancel, Ask Keep and second-launch reconnection, running-command confirmation for explicit and window-policy Quit, all three terminal-tab policies, three new-tab directory choices, old/new command snapshots, dirty Save followed by close/reopen, background output while hidden and the same original session after relaunch. The harness finishes through the explicit application Quit action and the checked process exits successfully. Windows/macOS native GUI checks remain pending.

Reproduce with an isolated owner root under the Cargo target directory:

```sh
mkdir -p "$CARGO_TARGET_DIR/tmp"
export TMPDIR="$CARGO_TARGET_DIR/tmp"
export GIT_CEILING_DIRECTORIES="$TMPDIR"
cargo test --offline --locked --all-targets -- --test-threads=1 --include-ignored
RODTTY_HOME="$CARGO_TARGET_DIR/native-owner" "$CARGO_TARGET_DIR/debug/rodtty" \
  --session-policy-check "$CARGO_TARGET_DIR/session-policy.json"
```

For Xlib, remove `WAYLAND_DISPLAY` and `WAYLAND_SOCKET` from the process environment. The report's actual native raw-handle backend identifies the exercised platform; an environment label alone is insufficient.

The labelled Behavior scene can be rendered with `--scene behavior --snap <path> --size 1200x850` or `640x360`. The Settings list scrolls at short heights, and search narrows it to each preference group.

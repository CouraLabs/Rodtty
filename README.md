# Rodtty

A Rust and Rune desktop terminal for Windows, macOS, and Linux, with a consistent Bash-style shell environment and configurable uutils commands.

The desktop development build includes the terminal, file explorer, UTF-8 editor, and read-only Git diffs. Current validation is recorded in [the desktop implementation report](docs/desktop-implementation.md); the earlier [terminal feasibility report](docs/feasibility-report.md) records the initial milestone. Windows and macOS native runtime checks remain pending. Browser/PWA remote access follows in a later phase.

## Build and run

Use Rust 1.100 or later and a native development toolchain. Linux needs Fontconfig and xkbcommon development libraries plus a Vulkan-capable driver. Windows uses the MSVC Rust target and Visual Studio C++ build tools. macOS needs the Xcode command-line tools.

```sh
cargo build --locked --bins
cargo run --locked --bin rodtty
```

Build all binaries first: `rodtty-shell` must sit beside the desktop executable. `rodtty-probe` exercises the same PTY backend without the desktop UI:

```sh
cargo run --locked --bin rodtty-probe
cargo run --locked --bin rodtty -- --cwd /path/to/project
```

Rune and Brush are pinned to Git revisions; the Rune dependency patches are carried in the root manifest. `Cargo.lock` records the full dependency graph.

## Desktop behavior

- Terminal tabs run independent Brush sessions with native PTYs and a terminal-state parser.
- Commands on the inherited host PATH remain available. Bash-style syntax is the target; complete Bash script compatibility is not promised.
- The searchable Settings editor tab covers Appearance, Behavior, Keyboard Shortcuts, Terminal, Editor, Files, and Bundled Commands, with individual reset controls. Bundled-command, history, and scrollback changes apply to new sessions; appearance and applicable behavior changes apply live.
- Enabled utilities run through native launchers as external processes, supporting concurrent pipelines. Disabled names report an error instead of falling back to a host utility in the managed environment.
- The terminal displays ANSI colors, Unicode, alternate-screen applications, and a cursor; supports resizing, selection, clipboard paste/copy, and scrollback search.
- Window close defaults to **Keep sessions running**, with **Ask each time** and **Quit Rodtty** alternatives in Behavior settings. Relaunching Rodtty or using its tray entry reopens the same sessions and retained output. Terminal-tab close defaults to confirming commands in progress; **Always confirm** and **Close immediately** are alternatives. Explicit Quit, including the window-close Quit policy, confirms running commands. Dirty editors keep their Save / Discard / Cancel decision. A computer restart creates fresh sessions.

Select **Files** to open the explorer at the active terminal's directory. Expand folders in place and use the Up button or folder path field for explicit root changes. **Follow** reveals the terminal directory inside the chosen root and offers a root-change action outside it. **Hidden** includes hidden files, **Ignore** respects Git ignore rules, and Git status badges show changed/untracked entries.

The compact header shows the active host folder, **Files**, **Terminal**, **Editor**, Settings, and the workspace menu (**···**). The menu contains the Command Palette, terminal search, session ending, window closing, and Quit. Terminal and editor tabs have separate strips above their panes.

Single-click a file to preview it; double-click, Enter, or editing keeps it as a regular editor tab beside the terminal. Drag the thin pane dividers to resize the explorer and editor; the terminal occupies the larger initial share. Header controls hide or show panes. As the window narrows, the explorer collapses first; **Files** reveals it temporarily. Below 880 points, **Terminal** and **Editor** switch views, and **‹ Terminal** returns from any editor or Settings tab in one action. Open files, running sessions, and remembered pane widths remain intact. Widening restores requested pane visibility. Regular editor tabs, pane state, explorer root/expansions/selection, and terminal order/directories restore on reopening. Surviving sessions reattach; after Quit or restart, fresh shells open in their saved directories without replaying commands. Behavior also offers one fresh terminal. Missing paths explain the fallback; previews, draft text, and title overrides are excluded.

The editor provides syntax highlighting, undo/redo, find/replace, and save. It edits UTF-8 files up to 2 MiB, preserves BOMs and line endings, and checks for external changes approximately once a second. Clean buffers reload automatically; dirty buffers retain their edits and offer Reload or Keep edits. A conflicting save requires explicit Overwrite. Closing a dirty tab/window or quitting offers Save / Discard / Cancel.

Editor Git modes are read-only:

| Mode | Comparison |
| --- | --- |
| Current | Current editor buffer versus HEAD, including unsaved changes |
| Unstaged | Saved file versus Git's index |
| Staged | Git's index versus HEAD |

Untracked files appear as additions. **Auto** selects inline or split diffs based on pane width; **Inline** and **Split** override it. Arrow buttons jump between changes. Git operations use the host-installed `git`; a missing Git installation leaves normal editing available. Viewing a diff never saves files, stages changes, or updates the index.

| Shortcut | Action |
| --- | --- |
| Ctrl+Shift+T | New terminal tab |
| Ctrl+Shift+W | End current session using the terminal-tab close policy |
| Ctrl+Shift+Q | Quit, confirming commands in progress |
| Ctrl+Shift+F | Search retained scrollback |
| Ctrl+Shift+P / Cmd+Shift+P | Search and run workspace actions |
| Ctrl+Shift+C / Ctrl+Shift+V | Copy selection / paste |
| Shift+PageUp / Shift+PageDown | Scroll terminal output |
| Ctrl+C | Interrupt foreground command |
| Tab / Up / Ctrl+R | Shell completion / history / history search |
| Ctrl/Cmd+S in the editor | Save the active file |
| Ctrl/Cmd+F in the editor | Find/replace panel |
| Ctrl/Cmd+Z / Ctrl/Cmd+Shift+Z in the editor | Undo / redo |
| Ctrl/Cmd+W in the editor | Close file, with an unsaved-change prompt |

These are defaults. Settings → Keyboard Shortcuts searches actions, records keyboard or mouse bindings, shows conflicts, and offers unbind and per-action reset. Menus, tooltips, help, and palette labels show effective bindings. On macOS, Command replaces Ctrl+Shift for application shortcuts; Control continues to send terminal control characters. The mouse wheel scrolls retained output, and dragging selects visible terminal cells. Terminal Settings configures right-click as Context Menu (default) or Paste, plus Copy on Selection (default on), with separate resets. Copy/Paste follows the focused terminal or editor; Ctrl+C continues to interrupt terminal programs. Advanced mouse-reporting protocols are not yet implemented.

The Actions control also opens the command palette. Type to filter actions, use Up/Down to select, Enter to execute, and Escape or an outside click to dismiss. The palette shows current shortcuts and explains unavailable actions. Dismissing it returns focus to the previous workspace control; save, close, and quit actions retain their existing confirmation flows.

## Configuration

Owner preferences live at `~/.rodtty/settings.json` on every desktop platform. Settings opens beside the terminal and saves UI changes immediately; Open Settings JSON edits that actual file in the editor. Valid external edits apply automatically within the open app, with command/history/scrollback changes taking effect for new sessions. Invalid JSON keeps the last valid preferences active and reports the problem without replacing the file. Unrecognized preference fields are preserved when existing controls save.

Settings → Editor offers Explicit Save (the default), After Delay, and Editor Focus Loss, with a configurable delay from 100 to 60,000 milliseconds (default 1,000) and independent Reset autosave policy / Reset delay controls plus Reset Autosave for both. These choices persist in the `autosave` preferences object and apply immediately. Focus-loss saving follows leaving the file for the terminal, Settings, another file, or hiding the editor; inspecting its Git diff stays within that file. Autosave uses the same external-change protection as Save. Conflicts preserve edits and offer the usual resolution; cancelling a save, close, or removal decision suspends autosave for those edits until you edit again or change the policy.

Appearance defaults to Catppuccin with System brightness. Settings → Appearance separates a searchable family selector from Light / Dark / System, with live light/dark previews and independent resets. The nine families are Catppuccin (Latte/Mocha), Dracula (Alucard/Dracula), Tokyo Night (Light/Night), Nord, Gruvbox (medium), One Dark Pro (**Light is a labelled Rodtty adaptation**), Night Owl (Light Owl/Night Owl), Ayu, and Solarized. System follows host appearance changes; explicit Light or Dark stays fixed. Changes apply across controls, terminal defaults and ANSI colors, editor highlighting, and Git diffs without replacing running sessions. Extended indexed colors and truecolor requested by terminal programs remain unchanged. Palette source and MIT attribution are bundled in [assets/themes](assets/themes/README.md).

`~/.rodtty/workspace-state.json` separately stores regular editor tabs, terminal-tab order/last directories, pane state, and explorer root/expansions/selection; workspace activity does not rewrite preferences and unsaved drafts are not stored. Existing platform-directory `config.toml`, `workspace.toml`, optional `startup.sh`, and shared history migrate once while preserving originals and existing destinations. Launch coordination also retains the old owner lock so surviving sessions are reopened safely. Run `rodtty --paths` to locate these files. Set `RODTTY_HOME` to isolate all storage and coordination for a test instance. `RODTTY_SHELL` can select a companion executable explicitly.

Behavior preferences apply immediately from Settings or valid external JSON edits; each control has its own reset. Running sessions retain their command/history/startup snapshots. Shell activity markers distinguish an idle prompt from foreground commands and shell builtins, while preserving owner PS0 and PROMPT_COMMAND hooks. Known background jobs also require confirmation. Brush jobs without a PID remain conservatively protected until the next prompt refresh (for example, after `wait`); asynchronous completion alone does not clear that protection. See [session policy validation](docs/session-policy-validation.md).

With history enabled, commands append to the shared history file at each prompt. New sessions load saved history; sessions already running retain their own recall list. The pinned Brush version does not support Bash's `history -n` incremental import.

This partial example illustrates selected preferences; Settings writes the complete configuration.

```json
{
  "theme_family": "catppuccin",
  "appearance": "system",
  "disabled": ["rm"],
  "scrollback": 10000,
  "history": true,
  "explorer": { "follow_terminal": false, "show_hidden": false, "respect_ignore": true },
  "behavior": {
    "window_close": "keep_sessions_running",
    "terminal_tab_close": "confirm_running",
    "new_terminal_directory": "active",
    "specified_directory": ""
  }
}
```

New tabs inherit the active terminal directory by default. Behavior settings also offers the explorer root folder or a specified absolute folder; a missing folder reports an error without creating a replacement session. The first session still uses the explicit startup folder, launcher directory, or home fallback. New sessions begin with the host environment and Rodtty-specific startup file. Host Bash/Zsh startup files are loaded only when explicitly sourced. Startup-file PATH changes are followed by prepending the session's managed utility directory.

Command selection configures a convenience environment. Explicit host executable paths and arbitrary host programs remain available; it is not a sandbox. The exact limits of deliberate shell/environment overrides are recorded in the feasibility report.

The explorer keeps one chosen host folder as its root. Click folders to expand them;
use the arrow keys, Home/End, Page Up/Down, or type a name to select an item.
Single click previews a file in a temporary tab; double click or editing keeps it
open. Only a clean, idle preview can be reused. Enter, the context menu's Open,
and the palette's Open Selected Item keep the file through the same activation
boundary. Behavior in Settings offers live Single click keeps files open and its
reset; the JSON preference is `single_click_keeps_open` (default `false`). Regular
tabs restore from workspace state; temporary previews are omitted. Use the path
field or the command palette to choose a new root. Follow Terminal reveals its
current directory within that root and offers an explicit root change when it
moves outside. The explorer continues to browse host files while using SSH.
Hidden-file, Git-ignore and Follow Terminal preferences live in `settings.json`
and are also available under Files in Settings. Root and expanded folders live in
`workspace-state.json`; refresh retains expansion and selection. File and Git scans
run on the file worker, with loading and folder errors shown in the explorer.

Create File, Create Folder, Copy, Rename, Move, and Move to Trash share explorer context-menu and command-palette actions. Name collisions offer Replace, Keep Both, or Cancel. Successful rename/move remaps open descendant tabs, preserving edits and selection; failures retain the old paths and buffers. Pending operations defer autosave, which resumes against the committed destination.

Move to Trash is available in the explorer context menu and command palette.
Files settings provides a live, resettable confirmation preference
(`explorer.confirm_deletion`, default `true`). Permanent deletion always requires
an explicit decision after the host Trash fails. Unsaved files, including open
folder descendants, offer Save As, Discard, or Cancel. Save As must preserve work
outside the removed item; existing destinations require a fresh collision
choice. Files, editor tabs, and restored workspace paths change only after the
selected worker operation succeeds. Background file work keeps terminal input
available; owner decisions remain modal and affected editor work stays protected.

Linux draws the complete themed window frame and right-side controls. Empty header
space drags the window; double-click maximizes/restores it; all edges/corners resize.
Custom close follows the same unsaved-file and session lifecycle as menu closure.
See [Linux native checks](docs/linux-custom-frame.md), [the Windows adapter](docs/windows-custom-frame.md), and [the macOS adapter](docs/macos-custom-frame.md). Native Windows/macOS GUI execution remains pending; their all-target metadata checks do not establish runtime compatibility.

## Validation

```sh
cargo fmt --all -- --check
export CARGO_TARGET_DIR="$PWD/target/qualification"
export TMPDIR="$CARGO_TARGET_DIR/tmp"
export GIT_CEILING_DIRECTORIES="$TMPDIR"
mkdir -p "$TMPDIR"
cargo build --locked --bins
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets -- --test-threads=1 --include-ignored
```

The serial suite includes the real Vim check and requires an installed `vim`. The matching Git ceiling prevents isolated non-repository fixtures under `target/` from inheriting this repository. See [integrated qualification](docs/integrated-qualification.md) for the exact executed gates, visual matrix, and platform limits. CI defines native Linux, Windows, and macOS jobs. Those jobs have not run until the workflow is published and executed.

Render the workspace with labelled sample content (`preview` shows only the terminal, `workspace` includes the editor, `diff` includes Git-style changes, and `palette` shows action discovery; `file-collision` previews the explicit collision choices; `tree` reads nested folders from the actual host startup location):

```sh
target/debug/rodtty --scene workspace --snap target/workspace-preview.png --size 1440x860 --scale 1
```

Run the native window-close/relaunch check in a logged-in desktop session:

```sh
target/debug/rodtty --native-check target/native-check.json
```

This opens a window, runs a command, closes the window while output continues, relaunches the executable, verifies the same session, writes a JSON report, and exits. Use a separate `RODTTY_HOME` if another Rodtty instance is running. `--control-stdin` is an additional developer-only channel accepting `open`, `close`, `status`, and `quit` (using the normal confirmation flow).

`--frame-check <report.json>` is driven by the isolated KWin native input harness described in the custom-frame report. It exercises actual pointer resize/drag/controls, compositor/native state, dirty-close cancellation and the lifecycle below.

`--session-policy-check <report.json>` additionally exercises native Ask/Cancel/Keep/Quit policies, dirty Save/close/reopen, tab policies, all three new-tab directory modes, immutable command snapshots, background output and same-session relaunch.

`--trash-check <report.json>` additionally checks actual host Trash and the native
window/worker dirty-folder Save As, Discard and Cancel flow. Use a fresh
`RODTTY_HOME`, `TMPDIR` under the binary's target directory, and a matching
`GIT_CEILING_DIRECTORIES`. On Linux, set `XDG_DATA_HOME` to a directory inside that
isolated `RODTTY_HOME`; the check never reads the owner's Trash. Windows/macOS
checks require a disposable OS account and `RODTTY_DISPOSABLE_TRASH_ACCOUNT=1`.
Native Windows/macOS execution remains pending; metadata compilation alone does
not qualify their runtime behavior.

`--workspace-check <report.json>` additionally verifies native unsaved-file prompts and editor saving. `--font-check <report.json>` also discovers native fonts, changes UI and terminal families and UI, terminal and editor sizes live, checks the full-screen application sees the updated PTY dimensions, and verifies unsaved editor work and the terminal session survive. Run developer lifecycle checks with a fresh `RODTTY_HOME` so they use isolated sessions and test files.

Settings offers searchable installed UI and verified monospace terminal family selectors, keyboard navigation, previews, and independent family resets. The bundled Geist and Geist Mono defaults remain available offline; installed inventory is discovered per launch. Settings also provides independent UI, terminal and editor font sizes (defaults: 13px, 14px and 14px), with immediate application and reset. These are saved in the `fonts` object of `settings.json`. The editor follows the terminal family; unavailable saved families fall back to the bundled Geist faces.

## Design documents

- [Product scope](docs/product-scope.md)
- [Feasibility plan](docs/terminal-feasibility.md)
- [Feasibility report](docs/feasibility-report.md)
- [Desktop implementation report](docs/desktop-implementation.md)
- [Domain glossary](GLOSSARY.md)
- [Architecture decisions](docs/adr/)

Bundled Geist and Geist Mono are distributed under the [SIL Open Font License](assets/fonts/Geist-OFL.txt). The retained IBM Plex Mono asset has its own [font license](assets/fonts/IBMPlexMono-LICENSE.txt). Application code is MIT licensed.

Explorer and editor-tab artwork comes from [vscode-icons](https://github.com/vscode-icons/vscode-icons). Its artwork uses CC BY-SA 4.0, with branded icons retaining their respective copyright licenses; see the [artwork attribution and notices](assets/vscode-icons/NOTICE.md). The upstream source-code MIT license does not cover all artwork.

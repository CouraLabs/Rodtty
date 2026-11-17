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
- The searchable Settings editor tab enables/disables bundled utilities and configures history and scrollback, with individual reset controls. Changes apply to new sessions.
- Enabled utilities run through native launchers as external processes, supporting concurrent pipelines. Disabled names report an error instead of falling back to a host utility in the managed environment.
- The terminal displays ANSI colors, Unicode, alternate-screen applications, and a cursor; supports resizing, selection, clipboard paste/copy, and scrollback search.
- Closing the window keeps sessions running. Relaunching Rodtty or using its tray entry reopens the same instance. Ending a tab or selecting Quit requires confirmation. A computer restart creates fresh sessions.

Select **Files** to open the explorer at the active terminal's directory. Expand folders in place and use the Up button or folder path field for explicit root changes. **Follow** reveals the terminal directory inside the chosen root and offers a root-change action outside it. **Hidden** includes hidden files, **Ignore** respects Git ignore rules, and Git status badges show changed/untracked entries.

Double-click a file, or select it and press Enter, to open an editor tab beside the terminal. Drag the pane dividers to resize the explorer and editor. **Terminal**, **Editor**, and **Hide** control pane visibility. Saved file tabs, pane sizes, and explorer location are restored on reopening.

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
| Ctrl+Shift+W | End current session, with confirmation |
| Ctrl+Shift+Q | Quit, with confirmation |
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

On macOS, Command replaces Ctrl+Shift for application shortcuts; Control continues to send terminal control characters. The mouse wheel scrolls retained output, and dragging selects visible terminal cells. Advanced mouse-reporting protocols are not yet implemented.

The Actions control also opens the command palette. Type to filter actions, use Up/Down to select, Enter to execute, and Escape or an outside click to dismiss. The palette shows current shortcuts and explains unavailable actions. Dismissing it returns focus to the previous workspace control; save, close, and quit actions retain their existing confirmation flows.

## Configuration

Owner preferences live at `~/.rodtty/settings.json` on every desktop platform. Settings opens beside the terminal and saves UI changes immediately; Open Settings JSON edits that actual file in the editor. Valid external edits apply automatically within the open app, with command/history/scrollback changes taking effect for new sessions. Invalid JSON keeps the last valid preferences active and reports the problem without replacing the file. Unrecognized preference fields are preserved when existing controls save.

Appearance defaults to Catppuccin with System brightness. Settings → Appearance offers Light (Latte), Dark (Mocha), and System with live palette previews and independent resets. System follows host appearance changes; explicit Light or Dark stays fixed. Changes apply across controls, terminal defaults and ANSI colors, editor highlighting, and Git diffs without replacing running sessions. Extended indexed colors and truecolor requested by terminal programs remain unchanged. Palette source and MIT attribution are bundled in [assets/themes](assets/themes/README.md).

`~/.rodtty/workspace-state.json` separately stores pane layout and saved file paths; workspace activity does not rewrite preferences and unsaved drafts are not stored. Existing platform-directory `config.toml`, `workspace.toml`, optional `startup.sh`, and shared history migrate once while preserving originals and existing destinations. Launch coordination also retains the old owner lock so surviving sessions are reopened safely. Run `rodtty --paths` to locate these files. Set `RODTTY_HOME` to isolate all storage and coordination for a test instance. `RODTTY_SHELL` can select a companion executable explicitly.

With history enabled, commands append to the shared history file at each prompt. New sessions load saved history; sessions already running retain their own recall list. The pinned Brush version does not support Bash's `history -n` incremental import.

```json
{
  "theme_family": "catppuccin",
  "appearance": "system",
  "disabled": ["rm"],
  "scrollback": 10000,
  "history": true,
  "explorer": { "follow_terminal": false, "show_hidden": false, "respect_ignore": true }
}
```

New tabs inherit the active tab's directory but begin with the host environment and Rodtty-specific startup file. Host Bash/Zsh startup files are loaded only when explicitly sourced. Startup-file PATH changes are followed by prepending the session's managed utility directory.

Command selection configures a convenience environment. Explicit host executable paths and arbitrary host programs remain available; it is not a sandbox. The exact limits of deliberate shell/environment overrides are recorded in the feasibility report.

The explorer keeps one chosen host folder as its root. Click folders to expand them;
use the arrow keys, Home/End, Page Up/Down, or type a name to select an item.
Enter opens the selected file through the shared file-opening action. Use the path
field or the command palette to choose a new root. Follow Terminal reveals its
current directory within that root and offers an explicit root change when it
moves outside. The explorer continues to browse host files while using SSH.
Hidden-file, Git-ignore and Follow Terminal preferences live in `settings.json`
and are also available under Files in Settings. Root and expanded folders live in
`workspace-state.json`; refresh retains expansion and selection. File and Git scans
run on the file worker, with loading and folder errors shown in the explorer.

## Validation

```sh
cargo fmt --all -- --check
cargo clippy --locked --all-targets -- -D warnings
cargo test --locked --all-targets -- --test-threads=1
cargo test --locked --test terminal real_vim_accepts_keyboard_input_and_saves_a_file -- --ignored
```

The last check requires an installed `vim`. CI defines native Linux, Windows, and macOS jobs. Those jobs have not run until the workflow is published and executed.

Render the workspace with labelled sample content (`preview` shows only the terminal, `workspace` includes the editor, `diff` includes Git-style changes, and `palette` shows action discovery; `tree` reads nested folders from the actual host startup location):

```sh
target/debug/rodtty --scene workspace --snap target/workspace-preview.png --size 1440x860 --scale 1
```

Run the native window-close/relaunch check in a logged-in desktop session:

```sh
target/debug/rodtty --native-check target/native-check.json
```

This opens a window, runs a command, closes the window while output continues, relaunches the executable, verifies the same session, writes a JSON report, and exits. Use a separate `RODTTY_HOME` if another Rodtty instance is running. `--control-stdin` is an additional developer-only channel accepting `open`, `close`, `status`, and immediate `quit`.

`--workspace-check <report.json>` additionally verifies native unsaved-file prompts and editor saving. Run developer lifecycle checks with a fresh `RODTTY_HOME` so they use isolated sessions and test files.

## Design documents

- [Product scope](docs/product-scope.md)
- [Feasibility plan](docs/terminal-feasibility.md)
- [Feasibility report](docs/feasibility-report.md)
- [Desktop implementation report](docs/desktop-implementation.md)
- [Domain glossary](GLOSSARY.md)
- [Architecture decisions](docs/adr/)

Bundled IBM Plex Mono is distributed under its [font license](assets/fonts/IBMPlexMono-LICENSE.txt). Application code is MIT licensed.

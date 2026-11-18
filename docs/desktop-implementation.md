# Desktop implementation report

Validated on 2026-11-17, Linux x86-64, Rust/Cargo 1.100.0. The final redesign qualification, all 88 story mappings, exact source freshness, screenshots, and pending platform work are recorded in [integrated qualification](integrated-qualification.md). Earlier feasibility and per-ticket reports remain historical evidence.

## Delivered workspace

The native Rust/Rune desktop uses app-drawn themed chrome, independent terminal/editor tab strips, and resizable explorer/terminal/editor panes. Linux/Windows controls are on the right and macOS controls on the left. Empty header space drags and double-clicks maximize/restore; native edges/corners resize. Custom close uses the same dirty-file and session decisions as menus and shortcuts. See [Linux](linux-custom-frame.md), [Windows](windows-custom-frame.md), and [macOS](macos-custom-frame.md) adapters and their native qualification boundaries.

The explorer collapses first as space narrows. Below 880 points, terminal/editor become switchable views, with one-action return to the terminal from files or Settings. The minimum window is 640×360. Tabs, sessions, drafts, and remembered pane sizes survive these switches. Tabs support dragging, overflow scrolling, middle-click, context actions, full-title tooltips, file artwork, parent disambiguation, and dirty indicators. A running application's terminal title is authoritative; clearing it restores the terminal icon / Term / current last-folder label. Workspace state does not override titles.

Appearance separates twelve theme families from Light / Dark / System, defaulting to Catppuccin/System. All twenty-four appearances style chrome, controls, ANSI defaults, syntax, selection, focus, and Git diff roles immediately; program-selected extended/truecolor output remains intact. One Dark Pro Light is labelled as a Rodtty adaptation. [Palette sources and notices](../assets/themes/README.md) describe all pairs. Bundled Geist/Geist Mono plus searchable installed families, verified monospace terminal filtering, previews, fallback, and independent UI/terminal/editor sizes are live. The editor follows the terminal family. [Font notices](../assets/fonts/README.md) distinguish bundled coverage from host fallback.

The explorer keeps a stable root with expandable folders, keyboard/type navigation, offline file artwork, Git badges, hidden/ignore filters, and optional Follow Terminal. Following outside the root offers an explicit change. SSH leaves the explorer on host files. Single-click opens a clean reusable preview; editing, double-click, Enter, or explicit Open keeps it. Single-click retention is configurable. Create/copy/rename/move/Trash share context-menu and palette actions. Collisions offer Replace / Keep Both / Cancel; successful moves remap descendant editor and workspace paths while preserving drafts. Trash offers unsaved Save As / Discard / Cancel; Save As preserves work outside the removed subtree. Permanent fallback always requires explicit confirmation. Failed work retains files and buffers; background worker activity leaves terminal input available.

The UTF-8 editor retains syntax, undo/redo, find/replace, and version-protected asynchronous saves for files up to 2 MiB. BOM, mixed line endings, permissions, and external-change protection remain covered. Clean files reload; dirty files retain edits and offer conflict resolution. Dirty closure offers Save / Discard / Cancel before lifecycle policy. Explicit Save is default. Opt-in delay/focus-loss autosave uses the same protected boundary, defers behind operations and owner decisions, and resumes at a moved path without resurrecting a deleted source. Diff inspection stays in the file context and does not trigger focus-loss saving.

Host-installed Git supplies read-only Current (buffer↔HEAD), Unstaged (disk↔index), and Staged (index↔HEAD) views, including linked worktrees, untracked files, staged renames, and unborn repositories. Inline/split/Auto modes and hunk navigation remain available; Auto follows actual editor width. Inspection neither saves nor stages and uses no optional Git locks or external text-conversion drivers. Filesystem and Git work stay off the UI thread. See [ADR-0006](adr/0006-preserve-file-versions-and-inspect-host-git.md).

## Preferences and lifetime

Searchable Settings occupies a dedicated editor tab and includes Appearance, Behavior, Keyboard Shortcuts, Terminal, Editor, Files, and Bundled Commands. Individual resets (including separate autosave policy and delay), shortcut recording/conflicts/unbinding, and effective bindings in the palette/help are available. Right-click defaults to Context Menu with Paste as an alternative; copy-on-selection defaults on. Focused editor/Settings clipboard handling and terminal Ctrl+C interrupts remain intact.

Owner preferences save immediately at `~/.rodtty/settings.json`. Valid external edits apply live where supported; invalid edits retain the last valid configuration and explain the problem. Migration preserves originals, existing destinations, history/startup behavior, and owner-instance locks. Bundled-command/history/startup configuration retains per-new-session snapshots; appearance does not change running environments.

Separate `~/.rodtty/workspace-state.json` stores regular editor order/active file, terminal order/last directories/active tab, explorer root/expansions/selection, and pane sizes/visibility/focus. Periodic/lifecycle writes do not rewrite owner preferences. Previews, unsaved text, replayable commands, and title overrides are excluded. Missing saved paths explain fallback while the remaining workspace opens.

Window close defaults to Keep sessions running, with Ask each time and Quit alternatives. Terminal close offers Confirm running / Always confirm / Close immediately; explicit Quit retains running-command confirmation. Reopening reattaches surviving sessions with exact processes and output. After Quit/restart, saved tabs create fresh shells in last directories without replaying commands. One fresh terminal is an independent live/resettable startup choice; first launch retains the startup-directory contract.

## Executed qualification

The final serial suite passed **179 tests, zero failed or ignored**: 121 application/runtime, 20 file operations, 12 files/Git, 3 lifecycle/storage, 20 PTY (including real Vim), and 3 isolated host Trash. Formatting, strict application Clippy, binary builds, backend all-target checks, and Windows/Apple desktop all-target metadata checks passed. [Gate evidence](evidence/t23/gates.json) lists actual commands, test names, hashes, and source boundaries.

Final feature-source Linux Wayland and actual Xlib native checks passed for eight-direction resizing, drag, minimize/maximize/restore, dirty close, installed fonts/fullscreen PTY resizing, session policies, isolated Trash, background output, same-session relaunch, and live/cold restoration. [Native reports](integrated-qualification.md#native-evidence) record observed backends rather than requested environment labels. Clipboard uses the exact earlier guarded T14 reports, explicitly labelled with their source freshness; its final empty-selection integration guard is covered by the current runtime suite.

The [visual matrix](integrated-qualification.md#visual-evidence) covers all eighteen appearances at 1440×900 and 640×360, representative terminal/tree/selected editor/Settings/palette and real Git versions/layouts. Files, index bytes, and Git status remained unchanged. Live System Light→Dark→Light is exercised against the existing open Rune runtime at both sizes; native mode cycling separately retains the same session. These are distinct checks.

Use the [README validation commands](../README.md#validation). Set `TMPDIR` and `GIT_CEILING_DIRECTORIES` to the same target-local directory; build binaries before focused Session tests. Native profiles belong on the binary's filesystem so utility launchers hard-link rather than copy binaries:

```sh
export TMPDIR="$PWD/target/native-tmp"
export GIT_CEILING_DIRECTORIES="$TMPDIR"
mkdir -p "$TMPDIR"
RODTTY_HOME="$PWD/target/native-owner" target/debug/rodtty --workspace-check /tmp/workspace-check.json
```

Restoration uses the same isolated owner root for `--restoration-check /absolute/live.json`, then `--restoration-check /absolute/cold.json --restoration-cold /absolute/live.json`. Both exit through explicit Quit. Linux Trash additionally requires an isolated `XDG_DATA_HOME`; Windows/macOS Trash checks require a disposable account marker. See the qualification report for exact reproduction.

## Remaining platform work

Windows/macOS native GUI linking/execution, controls, installed fonts, clipboard, Trash, and lifecycle remain pending under the accepted Linux-first authorization. Metadata checks exercise real platform cfg adapters and test targets but establish no runtime compatibility. Other Linux compositors and a positive case-insensitive-volume alias run remain unexecuted. CI is prepared but has not been published/executed by this work. Broader IME/terminal conformance, installers, crash draft recovery, remote access, graphical Git writes, language servers, and large/other-encoding files remain outside this delivery.

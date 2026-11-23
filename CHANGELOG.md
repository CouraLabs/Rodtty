# Changelog

All notable changes to Rodtty are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow publishes the section whose heading matches the tagged version as the GitHub Release notes.

## [Unreleased]

## [0.2.1] - 2026-11-23

### Security

- `app_action` runs only actions that change what the window shows. An automation client could previously enable remote access and receive a pairing code, paste the clipboard, toggle settings, or start file operations. `app_action` also no longer returns a footer message the action didn't write.
- `events_read` no longer sends workspace switches, other clients' Codex approval summaries, or recording statuses to clients that weren't granted them.
- The remote access page shows session titles and folders as text. A terminal title could previously inject HTML into the page.

### Fixed

- `events_read` shows a terminal's events after it closes, including its `closed` event.
- `rodtty mcp` sends `notifications/tools/list_changed` when Rodtty starts after the client first listed tools.
- `terminal_screen`, `terminal_wait` with `screen_contains`, and terminal screenshots read the live screen while the owner is scrolled back.
- `terminal_wait` with `idle` completes on a freshly started shell.
- An agent's `terminal_close` no longer replaces a prompt the owner is answering.
- `terminal_launch` quotes arguments containing `=` or `%`.
- Codex threads report and interrupt their newest turn. Turns were ordered by ID as text, so `turn-9` came after `turn-10`.
- Jumping from a Filter Output result finds indented lines and lines with tabs.
- Characters whose UTF-8 encoding contains byte 0x9B, such as "ś" and "些", are no longer dropped from excerpts, run comparisons and command output.
- A corrupt draft file no longer crashes draft recovery.
- The environment check no longer times out on a program that prints more than 64 KiB.
- `rodtty open` rejects pipes and devices with exit status 4 instead of stalling the file worker.
- A non-ASCII line in `lsof` output no longer crashes the services list.
- Folders outside a Git repository no longer show a Git error when Git's messages are in another language.
- Pairing a device no longer replaces a paired-devices file that can't be read.
- The tab preview scenes render when the temporary folder is behind a symlink, such as macOS `/var`.

## [0.2.0] - 2026-11-20

### Added

- Split terminals: two independent sessions per tab, side by side or stacked, with a divider, focus switching and saved layouts.
- Previous Command and Next Command, and opt-in notifications for long commands that finish in the background.
- A Changes view listing staged, unstaged and untracked files, opening each one's diff.
- Remote session handoff over Tailscale with one-time pairing codes, a control policy and revocation. Off by default.
- Cmd-click (Ctrl-click) or Open Reference opens printed `path:line[:column]` references in the editor.
- Recovery of unsaved editor edits after an unexpected exit.
- Send to Terminal for editor selections and diff hunks, with an editable preview.
- Filter Terminal Output, with substring and regular-expression modes.
- A local agent status interface shown on terminal tabs.
- Copy as Markdown opens an editable preview with redaction.
- Pin a command's output and compare it with a rerun.

### Fixed

- A shell activity marker split across two PTY reads no longer shifts the next command's recorded output.

## [0.1.1] - 2026-11-20

### Added

- Mouse reporting for terminal applications that request it (xterm modes 9, 1000, 1002 and 1003, in default, UTF-8 and SGR encodings): clicks, drags, motion and the wheel reach the application. Shift keeps Rodtty's own selection and scrollback.
- The wheel scrolls alternate-screen applications that don't request mouse input, such as `less` and `man`, by sending arrow keys.

### Fixed

- The wheel and trackpad scroll the terminal's scrollback in the system's scroll direction, including macOS natural scrolling. They scrolled the opposite way.
- A full terminal draws in about a millisecond instead of about 17, so output and scrolling keep up with the display's refresh rate. Adjacent ASCII cells of one style now share one text run instead of one text element per cell.

## [0.1.0] - 2026-11-19

First public release. Windows and macOS native runtime checks are still pending; see [the desktop implementation report](docs/desktop-implementation.md).

### Added

- Desktop terminal for Windows, macOS, and Linux with tabs, each running an independent Brush (Bash-style) session on a native PTY.
- ANSI colors, Unicode, alternate-screen applications, resizing, selection, clipboard copy and paste, and scrollback search.
- Bundled uutils commands that run as native launchers, with per-command enable and disable settings.
- File explorer that follows the terminal directory, with hidden-file and Git-ignore filters, Git status badges, file search by name and content, and native Trash.
- UTF-8 editor with syntax highlighting, undo and redo, find and replace, external-change detection, conflict-safe saving, and configurable autosave.
- Read-only Git diffs with hunk navigation.
- Settings editor with Appearance, General, Shortcuts, and Commands tabs, live themes, and prompt styles.
- Session persistence: closing the window keeps sessions running by default, and relaunching reopens them with their output.
- Native accessibility support and reduced-motion-aware transitions.
- Installers: a universal macOS app, Windows MSI installers for x64 and ARM64, and Linux archives for x86_64 and ARM64, distributed through GitHub Releases, Homebrew, and winget.

[Unreleased]: https://github.com/CouraLabs/Rodtty/compare/v0.2.1...HEAD
[0.2.1]: https://github.com/CouraLabs/Rodtty/compare/v0.2.0...v0.2.1
[0.2.0]: https://github.com/CouraLabs/Rodtty/compare/v0.1.1...v0.2.0
[0.1.1]: https://github.com/CouraLabs/Rodtty/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/CouraLabs/Rodtty/releases/tag/v0.1.0

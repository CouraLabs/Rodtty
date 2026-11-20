# Changelog

All notable changes to Rodtty are recorded here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and versions follow [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The release workflow publishes the section whose heading matches the tagged version as the GitHub Release notes.

## [Unreleased]

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

[Unreleased]: https://github.com/CouraLabs/Rodtty/compare/v0.1.1...HEAD
[0.1.1]: https://github.com/CouraLabs/Rodtty/compare/v0.1.0...v0.1.1
[0.1.0]: https://github.com/CouraLabs/Rodtty/releases/tag/v0.1.0

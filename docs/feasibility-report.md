# Terminal feasibility report

Validation date: 2026-11-16. Tested host: Linux x86-64, kernel 7.3.4-1-cachyos, Rust/Cargo 1.100.0.

## Result

The prototype builds and runs as a native Rune terminal on this Linux host. Sixteen local tests pass, including a real Vim session, independent Brush sessions, command selection, concurrent utility pipelines, completion, history search, Unicode paste, resizing, interrupts, and normal foreground-child termination. Native window-close/relaunch checks pass under both Wayland and X11.

The cross-platform feasibility milestone is **not yet complete**. Windows and macOS shell/backend code has been cross-checked, but native execution and full GUI validation on those systems remain pending. The owner has subsequently authorized proceeding with the desktop explorer/editor/Git implementation while those machines are unavailable. This changes implementation sequencing, not the compatibility claims supported by this report.

## Components

| Component | Selected for this prototype |
| --- | --- |
| Desktop UI | Rune, revision `eb5ae6c9f77b1a0051d8e1e5ee936ebea086bb90`, with its required dependency patches |
| Shell | Brush, revision `fbd1eb36158384bbd5ad542445a5324bebefce78`; still a cross-platform candidate |
| Utilities | uutils 0.12.0 through Brush's feature-selected provider registry |
| PTY | portable-pty 0.9.0: Unix PTYs / Windows ConPTY |
| Terminal state | vt100 0.16.2; Rune draws its visible cells and translates keyboard input |
| Window reopening | Rune's retained application lifetime plus a per-owner file lock and local filesystem notification |

Enabled utilities execute as real external processes, avoiding the serialization/process-group limitations documented in Brush's experimental bundled builtin shims. See [ADR-0005](adr/0005-execute-bundled-utilities-as-native-processes.md).

## Evidence

| Check | Result |
| --- | --- |
| Formatting, strict Clippy, native build | Passed locally |
| Unit and integration tests, including explicitly enabled Vim check | 16 passed |
| Wayland window closure, output while closed, second-launch reopening, same session | [Passed](evidence/linux-wayland.json) |
| X11 window closure, output while closed, second-launch reopening, same session | [Passed](evidence/linux-x11.json) |
| 100,000 generated output lines through a live PTY | [Passed](evidence/linux-output-probe.json): approximately 506 ms including startup and final snapshot, approximately 71 MiB peak probe RSS |
| Bounded scrollback | Passed with 100 retained lines after 500 generated lines |
| Windows x86-64 MSVC shell/backend and test compilation | Passed with `--no-default-features --all-targets` |
| Apple Silicon macOS shell/backend and test compilation | Passed with `--no-default-features --all-targets` |
| Full Windows GUI cross-check from Linux | Stopped in a dependency build script: Microsoft's `lib.exe` is unavailable; no Windows GUI compatibility conclusion |
| Windows/macOS native builds and backend execution | Pending; CI workflow prepared but not published/run |
| Windows/macOS native window lifecycle and OS clipboard interaction | Pending on logged-in native desktops |

The output measurement is a single debug-build observation, not a performance guarantee. Rendering and keyboard handling read visible terminal state without cloning the entire scrollback. The [renderer preview](assets/terminal-preview.png) uses labelled sample content; the native lifecycle reports and PTY tests provide the execution evidence.

## Initial utility matrix

This matrix records exercised behavior on Linux, not complete GNU compatibility. All listed names are included in the prototype and configurable per session. Native Windows/macOS behavior remains to be validated.

| Utility | Exercised behavior |
| --- | --- |
| `cat` | Files, stdin, multi-stage pipeline; disabled-name handling |
| `cp` | Copy a file |
| `env` | Bundled version dispatch |
| `head` | First line with `-n 1` |
| `ls` | `-1`, `--version`, precedence over host executable |
| `mkdir` | Create a directory |
| `mv` | Move/rename a file |
| `rm` | Remove a file |
| `seq` | Generate numeric sequences, including 100,000 lines |
| `sleep` | Zero delay, delayed output, foreground interruption |
| `sort` | Sort text from stdin |
| `tail` | Last line with `-n 1` |
| `touch` | Create an empty file |
| `wc` | Count lines with `-l` |

## Boundaries and remaining work

- Command settings are snapshots for new sessions. Tests cover bare invocation, file scripts, `command`, `exec`, pipelines, substitutions, cached lookup, startup-file PATH changes, and explicit host paths. Shell built-ins are separate. Deliberately replacing the runtime environment, invoking a different interpreter, or using system executables directly is outside this convenience configuration; it is not a sandbox.
- Bash-style syntax is exercised; full Bash script/dotfile parity is not claimed. Brush itself labels Windows support experimental. [Brush](https://brush.sh/)
- The renderer handles the tested terminal modes and applications; advanced mouse reporting, comprehensive terminal conformance, and full job-control/signal matrices remain unvalidated.
- Unicode text is exercised, but shell/startup configuration paths must currently be valid Unicode.
- Shared saved history, Up-history, Ctrl+R search, and disabling history pass. Clipboard paste payloads reach the shell; platform clipboard copy/selection and IME still need broader native interaction checks.
- The explorer, editor, Git diff UI, remote clients, tunnel integrations, and installers remain outside this prototype, as agreed.

The [validation plan](terminal-feasibility.md) remains the gate for adopting these components across all supported targets. The [CI workflow](../.github/workflows/feasibility.yml) builds native targets and runs backend tests; logged-in desktop checks are still required for platform window behavior.

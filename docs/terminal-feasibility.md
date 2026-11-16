# Minimal terminal feasibility milestone

Validate the highest-risk desktop requirements before building the complete explorer and editor. Product requirements are authoritative in `docs/product-scope.md`; accepted architectural decisions are recorded in `docs/adr/`.

## Scope

Build a minimal Rust and Rune terminal with session switching, candidate shell integration, a small uutils command configuration, and enough controls to exercise session lifetime. Brush is a candidate, not a dependency whose suitability is already established. Select and validate the PTY and terminal-state components during this milestone.

The explorer, editor, Git diff interface, browser/PWA clients, remote listeners, tunnel integrations, and release installers are outside this milestone.

## Validation gates

### Interactive shell

- Exercise quoting, variables, pipelines, redirection, conditional command chains, globbing, working-directory changes, and exit status.
- Demonstrate independent current directories and environments across two concurrent sessions.
- Verify startup-directory selection, Rodtty-specific startup configuration, completion, history, and history search.
- Keep shell execution and PTY I/O from blocking the Rune interface.

### Command configuration

- Establish a small initial supported command set through per-command tests on each target system.
- Verify enabled bundled utilities take precedence over same-named host executables.
- Verify a disabled utility's bare name reports disabled, including representative pipelines, substitutions, scripts, `command`, `exec`, and cached lookups. Exercise startup-file PATH changes so the promised behavior is tested rather than inferred.
- Verify explicit host-program paths remain usable. Keep shell-built-in behavior distinct from bundled external utilities.
- Verify settings changes affect new sessions while existing sessions retain their command selection.

### Terminal behavior

- Run a real full-screen terminal application in addition to deterministic test fixtures.
- Exercise alternate-screen transitions, colors, resizing, Unicode, keyboard input, clipboard paste, interrupts, child-process exit, and return to the prompt.
- Demonstrate selection, bounded scrollback, scrollback search, and history configuration.
- Measure output handling and ensure configured scrollback remains bounded during sustained output.

### Session lifetime

- Close all desktop windows while a command continues producing output.
- Reopen the application and reattach to the same running session with its current terminal state and retained scrollback.
- Validate closing a terminal tab and explicitly quitting, including confirmation for running commands.
- Verify restarting the application after a true host-process exit creates fresh sessions rather than implying that terminated processes survived.

### Platform coverage

- Validate native execution on Windows, macOS, and Linux. Include X11 and Wayland window-lifetime behavior where supported.
- Record tested operating-system versions, architectures, toolchains, dependency revisions, and known limitations.
- Separate automated checks, manual interactive evidence, and checks not yet run. A successful Linux build is not evidence of Windows or macOS compatibility.

## Deliverables and completion

Deliver runnable prototype code, repeatable checks, a tested utility matrix, and a feasibility report identifying passed and failed gates. Pin the selected Rune revision and record the dependency choices actually validated. Any unverified platform or failed gate remains explicitly incomplete.

Advance to the complete desktop interface only when these requirements have evidence on the supported targets. Revisit the shell or terminal components if validation contradicts the agreed product behavior; do not silently reduce the compatibility promise.

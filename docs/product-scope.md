# Rodtty product scope

This document records agreed product behavior. [Integrated qualification](integrated-qualification.md) maps all 88 redesign stories to executed evidence and explicit pending native checks. The terminal, explorer, UTF-8 editor, and read-only Git diff implementation are recorded in `docs/desktop-implementation.md`. Native Windows/macOS validation remains pending; the initial terminal evidence is retained in `docs/feasibility-report.md`.

## Desktop first release

- Native Rust and Rune applications for Windows, macOS, and Linux.
- The terminal is the main screen and supports full interactive terminal applications.
- App-drawn themed chrome provides native drag, edge/corner resize, minimize, maximize/restore, and lifecycle-aware close. Windows/Linux controls are on the right; macOS controls are on the left. Linux Wayland and Xlib checks are executed; Windows/macOS native execution remains pending.
- Nine light/dark theme pairs plus live System appearance, defaulting to Catppuccin/System. One Dark Pro Light is explicitly a Rodtty adaptation. Bundle Geist and Geist Mono; provide searchable installed UI and verified monospace terminal families, previews, fallback, and independent UI/terminal/editor sizes. The editor follows the terminal family.
- Separate draggable/scrollable terminal and editor tab strips, context menus, middle-click closure, and full-title tooltips. Application-supplied terminal titles are authoritative; clearing restores the terminal-icon / Term / last-folder fallback. No manual title override is stored.
- A hierarchical, keyboard-navigable explorer with offline file artwork, Git badges, hidden/ignore filters, stable root, and optional Follow Terminal. Single-click previews are reused only when clean and idle; editing or explicit Open keeps a regular tab. Single-click retention is configurable.
- Create, copy, rename, move, and Trash actions share context-menu/palette behavior. Collisions offer Replace / Keep Both / Cancel. Successful moves preserve descendant buffers and remap paths; deletion offers Save As outside the removed subtree / Discard / Cancel for unsaved work. Permanent fallback always requires an explicit decision.
- Searchable Settings covers Appearance, Behavior, Keyboard Shortcuts, Terminal, Editor, Files, and Bundled Commands, with individual resets. Owner preferences live at `~/.rodtty/settings.json`; valid external edits apply live where supported, invalid edits retain the last valid settings, and migration preserves originals and existing destinations. Workspace JSON remains separate.
- Configurable shortcut recording/conflicts/unbinding/reset and a command palette show effective bindings. Terminal right-click offers context menu or paste; copy-on-selection defaults on. Platform copy/paste preserves Ctrl+C interrupts.
- Explicit Save is default; opt-in delayed/focus-loss autosave uses the same version-protected asynchronous save boundary. Pending file operations and owner decisions defer autosave; successful path changes direct later saves to the new path.
- Multiple terminal tabs, each with its own session, current directory, environment, and processes.
- Desktop-window close defaults to keeping terminal sessions running and accessible on reopening; Behavior settings also offers Ask each time and Quit Rodtty. Closing a terminal tab ends its session, with Confirm when commands are running (default), Always confirm and Close immediately policies. Quit Rodtty ends all sessions and retains confirmation for running commands, including when selected as the window-close policy. Computer restarts begin fresh sessions. See ADR-0004.
- Bash-style command syntax through the pinned Brush implementation, validated on Linux; complete Bash script compatibility is not promised. See ADR-0003.
- New tabs inherit the active terminal's directory by default, with explorer-folder and specified-absolute-folder alternatives. They start with the host environment plus Rodtty-specific startup configuration. Use an explicitly supplied directory for the first session, otherwise the launcher's directory, falling back to home. Other shell configuration files are sourced only when explicitly requested.
- Reopening a kept-running host retains the same terminal sessions, processes and recent output. After Quit or restart, restore terminal-tab order and last directories using fresh shells without replaying commands. Behavior settings also offers one fresh terminal using the new-tab directory choice; first launch retains the startup-directory contract. Regular editor tabs, explorer root/expansions/selection and pane layout restore from separate workspace JSON; the explorer itself starts hidden on every launch, so the terminal comes first. Missing paths show an explanation and leave the rest of the workspace accessible; temporary previews, application-title overrides and unsaved drafts are excluded.
- Provide command/path completion, saved command history, history search, text selection, clipboard copy/paste, and searchable scrollback. History is shared per owner and can be disabled. Scrollback has a configurable size limit.
- A tested, configurable set of bundled uutils commands. See ADR-0002 for precedence and disabled-command behavior.
- Command availability is configured per owner in a settings screen. Changes apply to new sessions, while existing sessions keep their starting selection.
- Inherit the host PATH so installed programs can run with their native behavior and platform requirements.
- Work directly with the native host filesystem, including slash-friendly native paths such as `C:/Users/Rodrigo` on Windows and `/home/rodrigo` on Linux. The explorer continues browsing host files when the terminal runs SSH; remote filesystem browsing is a separate future feature.
- Initialize the explorer from the active terminal's current directory when first opened. Navigate independently by default; provide an optional Follow Terminal mode.
- Open files from the explorer into a lightweight editor with tabs, syntax highlighting, undo/redo, find/replace, and save. Keep the terminal readily accessible.
- Use a collapsible explorer on the left, the terminal as the main view, and an editor pane beside the terminal when opening files. Terminal and editor tab sets are separate. Panes are resizable and hideable.
- Edit UTF-8 text while preserving existing line endings. Detect external changes: reload clean files automatically and ask before replacing unsaved edits. Explain binary-file limitations instead of opening binary data as editable text.
- Offer Save / Discard / Cancel for unsaved editor changes before closing an edited file, closing the desktop window, or quitting. Restore saved editor tabs and pane layout on reopening. The selected window-close policy applies only after the editor decision is resolved.
- Provide read-only Git integration through editor file diffs and explorer change indicators. Staging, commits, branch management, push, and pull are performed through the terminal in the first release.
- Default Git diffs compare current editor contents against the last commit, including clearly labelled unsaved changes. Also offer unstaged comparisons (saved file versus index) and staged comparisons (index versus HEAD). Show untracked files as additions labelled untracked. Viewing a diff never saves or stages a file.
- Present Git diffs side by side when space permits and offer inline mode for narrower windows. Highlight added/deleted lines and support navigation between changes.
- Use Git installed on the host. Show a setup message when Git is unavailable and keep the terminal and editor usable. The first release does not bundle Git.

## First implementation milestone

Validate a minimal terminal before building the complete explorer/editor interface. The feasibility build must demonstrate:

- Bash-style syntax, independent sessions, command/path completion, and history with the candidate shell.
- Bundled-command precedence and disabled-command handling in the presence of same-named host executables.
- Interactive terminal applications, resizing, Unicode, clipboard paste, and interrupts.
- Sessions surviving desktop-window closure and being available on reopening.
- Compatibility validation on Windows, macOS, and Linux.

Revisit a dependency that fails these checks. Use validation results to establish the exact supported utility list. See `docs/terminal-feasibility.md` for the validation plan and `docs/feasibility-report.md` for evidence from the implementation. The owner has authorized implementing the explorer, editor, and read-only Git integration using Linux validation while Windows/macOS machines are unavailable. Native cross-platform validation remains pending before claiming release compatibility; it no longer blocks this implementation phase.

## Later delivery

- Browser and installable PWA remote clients; no native mobile application.
- Single-owner access under the Rodtty host's operating-system account.
- Configurable LAN, Tailscale, and Cloudflare connection methods.
- Remote sessions survive browser disconnection and allow reconnection to the same session and recent output while the host remains running.
- Terminal split panes.
- Editor language servers, debugging, and extensions.
- Git write operations through the graphical interface.
- Automatic recovery of unsaved editor drafts after a crash.
- Remote filesystem browsing, broader text encodings, and specialized large-file editing.

## Open decisions

- Release utility support and platform compatibility beyond the Linux development qualification.
- Supported operating-system versions and architectures, following technical validation.
- Remote tunnel provisioning, authentication, and concurrent-client behavior before the remote phase.

The remaining technical choices are outputs of the feasibility milestone. Remote-specific design questions are deferred to the remote phase rather than assumed settled.

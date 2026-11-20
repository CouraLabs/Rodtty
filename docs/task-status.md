# Backlog tasks 01–31: implementation and evidence

Status as of 2026-11-20. "Verified" means the named test or check ran and passed on this date, on macOS 27.2.1 arm64 unless stated. Unit and integration tests: `cargo test --tests` (281 library tests, 60 integration tests, all passing), `cargo fmt --check` and `cargo clippy --all-targets` (no warnings), `cargo clippy --features browser --lib` (no warnings). MCP checks are listed in [automation evidence](evidence/automation/README.md).

| Task | Implementation | Evidence | Status |
| --- | --- | --- | --- |
| 01 Named project workspaces | `src/workspaces.rs`, `src/desktop/overview.rs` | `workspace_*` and isolation tests in `src/desktop/automation/tests.rs`; e2e workspace grant | Verified |
| 02 Split terminals | `src/desktop/split.rs`, layout `splits` in `src/work.rs` | `split_panes_keep_their_own_sizes_input_close_policy_and_restore_both_orientations` | Verified in tests; live window reopen relies on retained State (ADR 0004) |
| 03 Command results and notifications | `src/session.rs` command records, `src/desktop/commands.rs` | `command_results_*`, `previous_and_next_command_scroll_to_each_prompt_and_background_long_commands_notify_once`, `split_markers_are_held_until_complete` | Verified; native notification delivery not observed on screen |
| 04 Repository changes panel | `src/git.rs` `changes`, `src/desktop/changes.rs` | `the_changes_view_lists_a_linked_worktree_opens_diffs_keeps_drafts_and_follows_root_switches` | Verified |
| 05 Remote session handoff | `src/remote.rs`, `src/desktop/remote.rs`, [remote access](remote-access.md) | `remote_devices_pair_attach_read_type_resize_follow_control_and_lose_access_when_revoked_or_quit` (HTTP over loopback), `pairing_codes_are_one_time_and_devices_expire_or_revoke` | Partial: phone pairing over Tailscale is blocked (Tailscale is not installed on the test machine and no phone was available) |
| 06 Clickable file references | `src/references.rs`, `src/desktop/terminal/references.rs` | `the_same_relative_reference_printed_in_two_directories_opens_each_ones_file`, wrapped multibyte test | Verified on macOS; Windows drive paths covered by parser tests only |
| 07 Project tasks | `src/tasks.rs`, `src/desktop/tasks.rs` | `tasks-check.mjs` | Verified |
| 08 Running sessions overview | `src/desktop/overview.rs` | `idle_and_busy_sessions_with_one_title_are_distinct_and_selection_follows_identity` | Verified |
| 09 Editor draft recovery | `src/drafts.rs`, `src/desktop/drafts.rs` | `draft_recovery_survives_a_killed_process_and_resolves_conflicts_saves_and_discards` (kills a child process mid-edit) | Verified |
| 10 Send context to a terminal | `src/desktop/send.rs` | `send_to_terminal_previews_an_unsaved_multibyte_selection_and_a_staged_hunk_for_the_chosen_session_only` | Verified |
| 11 Open in Rodtty from scripts | `src/open.rs` | `open-check.mjs`, open tests | Verified with a running instance on macOS |
| 12 Multiline paste confirmation | `src/desktop/state.rs` | `multiline_paste_waits_for_confirmation_and_never_reaches_another_session` | Verified |
| 13 Local development services | `src/services.rs` | `services-check.mjs` | Verified |
| 14 Project environment check | `src/environment.rs` | environment tests | Verified |
| 15 Live log filters | `src/desktop/terminal/filter.rs` | `filtering_a_live_session_lists_incoming_matches_jumps_and_restores_the_terminal`, flood test | Verified |
| 16 Agent status integration | `src/session.rs` agent status, [agent status](agent-status.md), `scripts/agent-status/` | `agent_status_follows_its_own_session_rejects_bad_reports_expires_and_leaves_close_protection_alone` | Verified |
| 17 Terminal keyboard passthrough | `src/desktop/shortcuts.rs` | passthrough tests | Verified on macOS; Windows key routing not run |
| 18 Share a terminal excerpt | `src/excerpt.rs`, `src/desktop/excerpt.rs` | `markdown_excerpts_preview_redact_snapshot_and_copy_only_what_is_approved` | Verified |
| 19 Compare two runs | `src/compare.rs`, `src/desktop/runs.rs` | `a_pinned_failing_run_compares_with_its_rerun_and_clearing_leaves_the_terminal_intact`, `two_runs_compare_with_their_exit_statuses` | Verified |
| 20 Directory jump history | `src/desktop/folders.rs` | recent folders test | Verified |
| 21 Embedded CEF browser | `src/desktop/browser*`, [browser](browser.md) | `browser-check.mjs`, `tls-check.mjs`, e2e | Verified on macOS arm64 dev bundle; Windows and Linux CEF not run |
| 22 Desktop release verification | release v0.1.1 | See below | Partial |
| 23 Local MCP server | `src/automation/`, [automation](automation.md) | `terminal-check.mjs`, e2e | Verified |
| 24 Terminal and interactive app control | `src/desktop/automation/terminal.rs` | e2e steps 1–2, `terminal-check.mjs` | Verified |
| 25 Virtual mouse and keyboard | `src/desktop/automation/input.rs` | `agent_text_and_keys_reach_only_the_named_pty_and_owner_takeover_stops_them` (held keys released), `browser-check.mjs` (physical pointer unchanged) | Verified |
| 26 Embedded browser automation | `src/desktop/browser/automation.rs` | `browser-check.mjs`, e2e step 3 | Verified |
| 27 Screenshots and browser video | `src/desktop/capture.rs`, `src/desktop/browser/record.rs` | e2e: PNG screenshot and 48-frame AVI read by `ffprobe`; duration limit check | Verified |
| 28 Workspace, editor, Git and project tools | `src/desktop/automation/editor.rs` | e2e step 4 | Verified |
| 29 Jobs, events and agent handoff | `src/automation/events.rs` | e2e step 5 (duplicate requests, reconnect, stale IDs) | Verified |
| 30 Structured Codex adapter | `src/automation/codex.rs`, `src/desktop/codex.rs` | `codex-adapter-check.mjs` against the protocol fixture | Verified with the fixture; a live Codex account run was not repeated |
| 31 End-to-end orchestration | `scripts/automation/e2e.mjs` | [e2e-macos-arm64.json](evidence/automation/e2e-macos-arm64.json): 19 checks passed while the owner used WhatsApp in front | Verified on macOS arm64 dev bundle; packaged (signed) build blocked, see task 22 |

## Task 22: release evidence and blocked checks

Verified on 2026-11-20:

- Release `v0.1.1` (published 2026-11-20T03:37:27Z) lists the six advertised assets and `SHA256SUMS.txt`. `shasum -a 256 -c` passed for `Rodtty-0.1.1-macos-universal.zip` and `rodtty-0.1.1-linux-x86_64.tar.gz`.
- The macOS universal app reports `CFBundleShortVersionString` 0.1.1 and contains `x86_64 arm64` slices (`lipo -archs`).

Blocked or not run:

- **macOS signing and notarization.** The released app is ad-hoc signed (`codesign -dv`: `Signature=adhoc`) and `spctl -a -vv` rejects it. A Developer ID certificate and notarization credentials are needed; without them a clean-machine Gatekeeper install can't pass.
- **Windows native checks** (MSI install, launch, window close and reopen, x64 and ARM64): no Windows machine was available.
- **Linux native runtime** on this date: no Linux machine; earlier Linux evidence remains in `docs/evidence/`.
- **Homebrew and winget clean installs**: not run, because they would install over the owner's machine and account; they need a clean account or VM.
- **Remote phone pairing (task 05)**: needs Tailscale on the host and a phone in the same tailnet.

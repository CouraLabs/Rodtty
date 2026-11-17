# Final redesign review resolution

The independent review compared `589e25e69dac4005fb65adf2d1a8872b680f893a...d1019baffce844eca79b3e6f27608577402387b4`. The original [Standards report](reviews/ui-redesign-standards.md) and [Spec report](reviews/ui-redesign-spec.md) are preserved separately, each under 400 words with its original file/line references. They found no documented-standard violation, two P3 baseline judgements, and two P2 spec defects.

All four findings are addressed by `b5565f3a1ea9d2ee751315385fe4855ce61df25a`, based on the actual reviewed integration. The original reports are historical findings; this document records their resolutions rather than replacing them with a new review.

## Standards resolutions

- **P3 possible Duplicated Code:** `src/desktop/workspace.rs:287` now builds the workspace snapshot once. File-operation persistence still publishes `Job::Layout` (`:280`); periodic/lifecycle persistence retains its pending-load/shutdown guards, change detection and direct save (`:758`). Regular files, active file, terminal order/directories/focus, explorer state and settings separation retain their existing JSON behavior.
- **P3 possible Data Clumps:** one `Option<RestoredPanes>` (`src/desktop/workspace.rs:65`, `:114`) contains required editor visibility, terminal focus and explorer reveal values. It is initialized together (`:320`) and consumed after pending opens finish (`:1185`). Hidden-editor/all-files-missing terminal-focus fallback and explicit-empty/compact explorer behavior remain unchanged.

Existing real-worker/runtime tests cover deletion/Save As, folder moves and protected autosave; restoration tests cover missing paths, clean disk buffers, widths/focus, explicit empty editor, compact explorer and unchanged settings bytes. Native live/cold checks additionally verify regular editor files and pane visibility/focus/widths, with previews omitted after cold start.

## Spec resolutions

- **P2 command confirmation:** `src/profile.rs:127` emits private OSC 777 markers tagged to the owning session's unique private profile path; `src/session.rs:50` accepts only that session's tag. Public OSC 133 A/C and other sessions' markers cannot clear its command activity. Owner PS0, PROMPT_COMMAND arrays, history/cwd hooks, job snapshots, foreground fallback, conservative unknown-PID jobs and exact application titles are preserved. The tag separates integrations; it is **not a security boundary** against programs able to read the profile.
- **P2 symlink preview retention:** `src/desktop/workspace.rs:1116` promotes the existing canonical document on explicit Open/double-click. Its ID, buffer and focus are retained. Asynchronous latest-activation filtering and dirty/saving replacement protections are unchanged. The actual pointer-double-click regression verifies the regular label, same identity/text, editor focus, canonical persisted path and survival when another file is previewed.

Both supplied real-PTY and real-FS/Rune probes failed before their fixes and passed afterward. A second PTY case covers another session's tagged markers with public OSC 133 C and an ST terminator. The extended native policy harness emits public A/C before blocked `read` and verifies default tab, explicit Quit and native window-policy Quit confirmations, including cancellation.

## Executed validation

The [gate ledger](evidence/review-fixes/gates.json) records exact commands, source/binary/log hashes, all test names, red regressions and target-local storage. The full serial include-ignored gate passed **182 tests, 0 failed, 0 ignored**: 122 runtime, 20 operations, 12 files/Git, 3 lifecycle/storage, 22 real PTY including Vim and 3 isolated native Trash. All binaries, strict all-target Clippy, backend all-target check, Windows MSVC/Apple Silicon all-target metadata, formatting and diff checks passed. Cargo builds/tests ran strictly sequentially; real execution was escalated upfront. Existing upstream Rune warnings remain dependency warnings.

| Affected native boundary | Wayland | Actual Xlib on KWin/Xwayland |
| --- | --- | --- |
| Session policies/public-marker confirmations | [passed](evidence/review-fixes/review-fixes-wayland-session-policy.json) | [passed](evidence/review-fixes/review-fixes-xlib-session-policy.json) |
| Same live sessions/editor/panes on reopen | [passed](evidence/review-fixes/review-fixes-wayland-live.json) | [passed](evidence/review-fixes/review-fixes-xlib-live.json) |
| Fresh cold shells/order/cwd/regular editor/panes/no preview or stale title | [passed](evidence/review-fixes/review-fixes-wayland-cold.json) | [passed](evidence/review-fixes/review-fixes-xlib-cold.json) |

All six exited successfully and report their observed backend. Focus was reserved and released. No clipboard/pointer probes, owner clipboard/history inspection or broad frame/font reruns were performed. The [396-frame qualification](integrated-qualification.md#visual-evidence), other unchanged native reports and guarded clipboard evidence retain their original source attribution. Affected [individual story mappings](evidence/t23/story-coverage.json) link this supplemental evidence separately. Licenses, dependency pins and reproducible vendor patches are unchanged.

Native Windows/macOS GUI/link/runtime and other Linux compositors remain pending; metadata checks do not establish native compatibility. Positive case-insensitive-volume alias execution remains pending. Brush jobs lacking representative PIDs remain conservatively protected until an authoritative prompt refresh. The existing UTF-8/2 MiB editor scope and lack of crash draft recovery are unchanged.

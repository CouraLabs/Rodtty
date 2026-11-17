# Integrated redesign qualification

T23 / [issue #24](https://github.com/CouraLabs/Rodtty/issues/24), parent [#1](https://github.com/CouraLabs/Rodtty/issues/1). Executed 2026-11-17 on Linux x86-64/KWin. The verified complete feature integration is `e36d9a8c66b84b5a6c54a4cc8bb6bc5810297feb`. T23 adds qualification tests, a rendering harness, documentation, and evidence. A tooling-only Tree scene correction is committed at `4305b87dbf374e8e87d293d543a6e05dfcad8750`; normal interactive worker behavior is unchanged. Independent autosave policy/delay reset controls were added at `78b0e8d7310d4ee18810b7193d209124f7304bb4`; final code/test qualification runs at `6f248bce93ef5aedc00aca9e3a18630f7200591d`. Native and unaffected image evidence retains its actual earlier feature-source attribution. Earlier per-ticket reports retain their original chronology.

The current serial suite passes **179 tests, 0 failed, 0 ignored**: 121 application/runtime, 20 operations, 12 files/Git, 3 lifecycle/storage, 20 real PTY including Vim, and 3 isolated host Trash. [Gate evidence](evidence/t23/gates.json) records exact commands, actual test names, source freshness, log hashes, and target-local storage. Desktop binaries, strict application Clippy, backend all-target checks, Windows MSVC and Apple Silicon desktop all-target metadata, formatting, and diff checks passed. Existing vendored Rune warnings remain dependency warnings.

## Cross-feature execution

The existing approved Rune runtime/rendered UI, real temporary filesystem/Git/PTY, and native window boundaries were used. No replacement test framework or internal mock layer was introduced.

- The Settings audit reproduced missing independent autosave resets. Separate policy and delay controls now preserve the other preference, while grouped Reset Autosave remains available. The existing Rune test verifies live controls and exact persisted JSON values.
- A new rendered-runtime flow edits preview A, previews B without replacing A, moves A's parent folder while its autosave deadline expires, and verifies only the remapped descendant is saved. Both After Delay and Editor Focus Loss preserve B's clean preview and never recreate the old folder. Existing real-worker tests additionally cover rename failure, external conflicts, Trash Cancel/Discard/Save As, collision decisions, late generations, and protected file formats.
- All nine live System families now exercise Light→Dark→Light in the same open runtime at **640×360 and 1440×900**, alongside existing explicit Light/Dark isolation, independent resets, ANSI/truecolor/selection, syntax, and icon checks. Native cycling of all nine family preferences through Light/Dark/System separately proves the same running session survives; it does not claim the host's global OS theme was changed.
- The close-entry matrix covers buttons, middle click, context menu, default shortcuts, **remapped F9**, menu, and palette with its current F9 label, for idle/running PTYs and every terminal-close policy. Existing tests preserve dirty Save/Discard/Cancel before window/Quit policy, protected-save failure, palette/recorder modal routing, fullscreen keys, and Ctrl+C interrupts.
- Installed-font native checks use Adwaita Sans and Adwaita Mono on this launch (713 discovered families), independent sizes, actual fullscreen PTY dimensions after resize, retained draft, background output, and exact same session. Runtime checks additionally verify Unicode/wide/combining selection, cursor/cell origins, hit testing, and display scale. The larger-font minimum screenshots support visual usability; their labelled sample terminal is not PTY-dimension evidence.
- Native live/cold restoration checks preserve exact live identities/output and then produce fresh shell PIDs with saved tab order/directories after Quit, no old command replay and no stale title override. Current runtime/PTY tests retain OSC set/change/clear, independent environments, one-fresh/missing-path behavior, workspace/settings separation, editor restoration, and new-session command snapshots.

Independent visual review confirmed one developer-scene mismatch: `ignored/` remained visible while Ignore was checked. A real Git/rendered-runtime test failed before the fix, then passed with production `git::inspect` / `git::ignored` providers, the current Hidden/Ignore preferences, matching checkbox labels and unchanged index bytes. Only the 36 affected Tree frames were refreshed at `4305b87`; unaffected frames retain their original per-frame source and binary hashes. No normal interactive-worker regression was confirmed. Two initial qualification assertions needed fixture corrections: restoring the host-dark test state before asserting reset colors, and using the recorder's canonical `F9` preference spelling when asserting its displayed label. Render recipes were adapted to current control IDs and actual viewport bounds; minimum Files must be revealed explicitly and wide Auto split needs sufficient editor width.

## Visual evidence

The [396-cell matrix](evidence/t23/appearance-matrix.json) records each family, Light/Dark appearance, viewport, scene, frame SHA-256, and retained contact sheet. All eighteen appearances were captured at **1440×900 and 640×360**, with terminal, nested host tree, selected dirty editor, searchable/focused Settings, scrolled appearance controls, palette, and real Current/Unstaged/Staged/explicit Split/Inline Git views. Wide Current hides the terminal to demonstrate genuine Auto split; default three-pane wide comparisons can correctly stay inline because the editor itself is narrower. Minimum Auto is inline, with return-to-terminal and explicit layout controls reachable.

Each contact sheet has nine rows (families) and Light/Dark columns. They and representative full-resolution images were visually inspected for control clipping, selection/focus, icons, syntax/diff contrast, and pane switching. Minimum Settings/tree content scrolls within its pane; controls below the fold are reached through normal scrolling. Root independently inspected minimum editor/palette and changed-font images plus representative real Git captures. Full raw PNGs remain local under `target/t23/render-evidence`; the tracked sheets and hashes provide manageable review artifacts.

| Surface | Wide | Minimum |
| --- | --- | --- |
| Terminal sample | [all 18](assets/t23/contact-1440-terminal.jpg) | [all 18](assets/t23/contact-640-terminal.jpg) |
| Nested host tree/icons | [all 18](assets/t23/contact-1440-tree.jpg) | [all 18](assets/t23/contact-640-tree.jpg) |
| Selected dirty editor sample | [all 18](assets/t23/contact-1440-editor-selection.jpg) | [all 18](assets/t23/contact-640-editor-selection.jpg) |
| Search/keyboard focus in Settings | [all 18](assets/t23/contact-1440-settings.jpg) | [all 18](assets/t23/contact-640-settings.jpg), [scrolled](assets/t23/contact-640-settings-appearance.jpg) |
| Palette | [all 18](assets/t23/contact-1440-palette.jpg) | [all 18](assets/t23/contact-640-palette.jpg) |
| Real Current / Auto | [all 18 split](assets/t23/contact-1440-real-current.jpg) | [all 18 inline](assets/t23/contact-640-real-current.jpg) |
| Real Unstaged | [all 18](assets/t23/contact-1440-real-unstaged.jpg) | [all 18](assets/t23/contact-640-real-unstaged.jpg) |
| Real Staged | [all 18](assets/t23/contact-1440-real-staged.jpg) | [all 18](assets/t23/contact-640-real-staged.jpg) |
| Explicit Split / Inline | [split](assets/t23/contact-1440-real-split.jpg), [inline](assets/t23/contact-1440-real-inline.jpg) | [split](assets/t23/contact-640-real-split.jpg), [inline](assets/t23/contact-640-real-inline.jpg) |

The real repository fixture has distinct HEAD/index/disk text, nested and duplicate basenames, hidden/ignored files, and an untracked addition. The actual Git provider reads these versions; sample terminal text in the same developer scene remains labelled. [Before/after file hashes, index hash and Git status](evidence/t23/appearance-matrix.json) are identical across the entire capture. Inspector commands use `GIT_OPTIONAL_LOCKS=0`. Current uses the unchanged saved buffer in these frames; the runtime suite separately edits an unsaved buffer with focus-loss autosave enabled and verifies buffer/disk/index/HEAD stay distinct and unchanged by inspection. Linked-worktree, unborn, staged-rename and hunk-navigation cases remain in the executed suite.

Representative minimum changed-font [terminal](assets/t23/font-minimum-terminal.png), [selected editor](assets/t23/font-minimum-editor.png), [UI picker](assets/t23/font-minimum-ui-picker.png), and [monospace picker](assets/t23/font-minimum-terminal-picker.png) use UI15/terminal20/editor18 and installed families. Their [hashes/settings](evidence/t23/supplemental-fonts.json) are separate from the default-font matrix. [Scrolled appearance controls](assets/t23/appearance-minimum-controls.png) remain accessible at that size. The final independent autosave [policy](assets/t23/autosave-minimum-policy.png) and [delay/reset](assets/t23/autosave-minimum-delay.png) controls were separately captured at 640×360/UI15; [their ledger](evidence/t23/supplemental-autosave.json) records source `78b0e8d` and binary/image hashes.

Reproduce with built binaries, Pillow, fresh target-local owner storage, and a matching `TMPDIR`/Git ceiling:

```sh
python3 scripts/qualification/render-redesign.py \
  --binary "$CARGO_TARGET_DIR/debug/rodtty" \
  --storage "$CARGO_TARGET_DIR/render-qualification" \
  --output "$CARGO_TARGET_DIR/render-evidence"
```

`--resume` reuses only this harness's own fixture and unchanged binary; `--resume --only-tree` explicitly refreshes Tree after a tooling-only fix and preserves the original provenance of every other frame. File/index/status checks still apply. Rendering is offscreen and does not exercise native OS controls.

## Native evidence

All entries below observed the actual backend and used isolated target-local owner roots. The frame harness reserves desktop focus/pointer and restores them; remaining probes use the existing application lifecycle harness. Linux Trash uses an isolated `XDG_DATA_HOME`, never the owner's Trash. These T23 probes did not read/write the host clipboard. They ran at the verified feature integration `e36d9a8`, before T23's tooling-only Tree correction and independent autosave reset controls; native font/save/lifecycle/clipboard implementation paths were unchanged by those deltas.

| Boundary | Wayland | Actual Xlib on KWin/Xwayland |
| --- | --- | --- |
| Drag/eight resizes/minimize/maximize/restore/dirty close + lifecycle | [passed](evidence/t23/t23-frame-wayland.json) | [passed](evidence/t23/t23-frame-x11.json) |
| Fonts/fullscreen PTY resize/draft/background/relaunch | [passed](evidence/t23/t23-wayland-font.json) | [passed](evidence/t23/t23-xlib-font.json) |
| Session policies/new-directory modes/command snapshots | [passed](evidence/t23/t23-wayland-session-policy.json) | [passed](evidence/t23/t23-xlib-session-policy.json) |
| Actual isolated Trash/dirty-folder decisions | [passed](evidence/t23/t23-wayland-trash.json) | [passed](evidence/t23/t23-xlib-trash.json) |
| Same live sessions/output | [passed](evidence/t23/t23-wayland-live.json) | [passed](evidence/t23/t23-xlib-live.json) |
| Cold fresh shells/order/cwd/no replay | [passed](evidence/t23/t23-wayland-cold.json) | [passed](evidence/t23/t23-xlib-cold.json) |
| Independent native Unicode Copy and different external Paste through real PTY | [inherited T14 passed](evidence/t23/t14-clipboard-wayland-inherited.json) | [inherited T14 passed](evidence/t23/t14-clipboard-xlib-inherited.json) |

Clipboard native probes preceded the worker's independent Autosave `4addc076` merge and the final narrow empty-selection guard at `e36d9a8`; their original reports do not embed the native binary revision, so the delivered worker commit is labelled separately and is not presented as a probe-source SHA. Nonempty Unicode Copy/Paste code is unchanged and current runtime/PTY tests include that guard. Exact two original Wayland MIME forms were restored after provider cleanup. Xlib had no initial selection owner and returned to that state; arbitrary nonempty X selection restoration was not implemented/tested, and no compositor cross-bridge claim is made. Only fixture outcomes, MIME counts, and restoration booleans are public; no owner payloads, payload digests, history identities, or recovery paths are retained. Final cleanup restored originals and stopped helpers. These guarded writes were not repeated solely for rendering/documentation.

Wayland winit minimization is unknown (`null`); the independent compositor acknowledges it. KWin fractional-scale geometry and actual Xlib are executed evidence; other compositors remain pending. Native Windows/macOS GUI/link/runtime, clipboard, fonts, Trash, chrome, and lifecycle remain **pending**. [Windows](windows-custom-frame.md) and [macOS](macos-custom-frame.md) adapters and prepared native harnesses are present; all-target metadata exercises their real cfg code and does not establish compatibility. A real positive case-insensitive-volume alias run also remains pending.

## Parent-story coverage

[All 88 individual requirements](evidence/t23/story-coverage.json) map to actual executed test names, visual cells, and applicable native reports. This compact grouping is a navigation aid; native evidence covers the boundaries named above, with Windows/macOS pending throughout.

| Stories | Delivered behavior | Executed boundary |
| --- | --- | --- |
| 1–3 | Native terminal-first workspace | Final Linux native + runtime/PTY |
| 4–7 | App-drawn native chrome | Final Linux native + runtime/PTY |
| 8–10 | Header and responsive panes | Current Rune/runtime + real file/Git/PTY suite |
| 11–13 | Independent draggable tabs and closure | Current Rune/runtime + real file/Git/PTY suite |
| 14–17 | Authoritative titles and fallback | Final Linux native + runtime/PTY |
| 18–24 | Eighteen appearances and live System | Final Linux native + runtime/PTY |
| 25–30 | Bundled and installed fonts | Final Linux native + runtime/PTY |
| 31 | Focus, compact controls and tooltips | Current Rune/runtime + real file/Git/PTY suite |
| 32–36 | Hierarchical tree, filters and offline icons | Current Rune/runtime + real file/Git/PTY suite |
| 37–43 | Create/copy/move/Trash and collision decisions | Final Linux native + runtime/PTY |
| 44–47 | Stable host root and Follow Terminal | Current Rune/runtime + real file/Git/PTY suite |
| 48–51 | Preview retention and disambiguated tabs | Current Rune/runtime + real file/Git/PTY suite |
| 52–54 | Editor and preserved file versions | Current Rune/runtime + real file/Git/PTY suite |
| 55–57 | Read-only real Git versions and layouts | Current Rune/runtime + real file/Git/PTY suite |
| 58–65 | Settings, individual resets, JSON and migration | Current Rune/runtime + real file/Git/PTY suite |
| 66–71 | Session close and new-tab policies | Final Linux native + runtime/PTY |
| 72–73 | Protected opt-in autosave | Current Rune/runtime + real file/Git/PTY suite |
| 74–76 | Focused clipboard and terminal interrupts | Current runtime/PTY + inherited guarded native clipboard |
| 77–79 | Searchable bindings and palette | Current Rune/runtime + real file/Git/PTY suite |
| 80–85 | Workspace restoration and session identity | Final Linux native + runtime/PTY |
| 86 | Terminal grid, selection and fullscreen resize | Final Linux native + runtime/PTY |
| 87 | Independent environments and command snapshots | Final Linux native + runtime/PTY |
| 88 | Responsive real worker and PTY boundaries | Final Linux native + runtime/PTY |

## Provenance and remaining limits

Theme source blob/tree distinctions, Ayu v7.3.2, explicit light adaptations, full notices, Geist/OFL, retained IBM Plex license, and vscode-icons artwork attribution are preserved. The [icon notice](../assets/vscode-icons/NOTICE.md) distinguishes CC BY-SA artwork/brand rights from MIT association code and notes the adaptive generic derivative. Offline mappings/artwork remain bundled; the importer is development-only.

All four owned [Rune](../vendor/rune/UPSTREAM.md), [Rune Kit](../vendor/rune-kit/RODTTY-PATCH.md), [VTE](../vendor/vte/UPSTREAM.md), and [Trash](../vendor/trash/RODTTY-PATCH.md) patches reverse-apply cleanly to their owned source. [Pinned-original verification](evidence/t23/provenance.json) independently applies each patch to the actual cached Git revision/crate archive and reproduces the owned touched files, with the documented Rune Kit unchanged test-block CRLF→LF import normalization. Their pins, reproducible delta files, upstream notices, and Cargo patches remain intact. Raw copied upstream trees were not treated as new authored code.

The development build retains the existing UTF-8/2 MiB limit and has no crash draft recovery. Broad IME/terminal conformance, installers, remote browser/PWA access, graphical Git writes, and editor language servers remain later work. Final independent two-axis code review follows this qualification; these artifacts do not substitute for that review.

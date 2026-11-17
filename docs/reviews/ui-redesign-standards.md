# Standards review

Reviewed `git diff 589e25e69dac4005fb65adf2d1a8872b680f893a...HEAD` at pinned HEAD `d1019baffce844eca79b3e6f27608577402387b4`.

**Documented-standard violations: 0 identified.** Sources: AGENTS.md, docs/agents/{issue-tracker,triage-labels,domain}.md, GLOSSARY.md, all eight ADRs, README Testing, and owned dependency/asset provenance constraints. No tooling-enforced style findings.

**Baseline judgements: 2; neither is a hard violation.**

1. **[P3] Possible Duplicated Code — src/desktop/workspace.rs:278–300 and :756–775.** Both persistence routes independently rebuild the same workspace snapshot, including `layout.files = …filter(|t| !t.preview)…`, `layout.active_file = …filter(|t| !t.preview)…`, terminal directories/order/focus, and explorer selection/visibility. A restoration-field change now requires updating both copies, risking different state after deletion/Save As versus periodic/window-close persistence. Extract one snapshot-update helper and retain the callers’ distinct guards and publication behavior.

2. **[P3] Possible Data Clumps — src/desktop/workspace.rs:108–110, :314–317, :1194–1201.** The fields `restore_editor: Option<bool>`, `restore_focus: Option<bool>`, and `restore_explorer: Option<bool>` are set together, consumed together, and use `restore_editor` as the group’s presence test while the other members require fallback values. Bundle them in one optional restoration record with nonoptional fields so initialization and consumption stay atomic. Preserve the current restoration timing and fallback behavior.

Coverage includes authored application/worker paths and supporting test hunks, configuration/action models, owned Rune/Rune Kit/VTE/Trash manifests and patch/source hunks, asset resolver/importer/notices, and native/qualification scripts. Copied upstream originals and generated artwork/font bytes were excluded as agreed. Existing passing gates were accepted; no native GUI, clipboard, owner-data probes, or source/index mutations were performed. Native Windows/macOS execution remains outside this review’s evidence.

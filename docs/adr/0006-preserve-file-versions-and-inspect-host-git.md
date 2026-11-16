# Preserve file versions and inspect host Git

File loading, external-change scans, saving, and Git inspection run on a worker outside the Rune UI thread so they do not block terminal input. Each editor tab keeps its saved disk version separately from the editable buffer. Saves compare disk contents with that version before replacing the file, require explicit consent on conflict, preserve file permissions/BOMs/line endings, and use a temporary file in the same directory for replacement. Stale scan or save results do not overwrite newer editor text.

Git comparisons use the host-installed Git CLI to discover the opened file's repository and read HEAD, index, and status data, including linked worktrees. They run without optional Git locks or external diff/text-conversion drivers. The UI computes read-only comparisons independently of save/stage operations. This keeps Git optional and avoids bundling another Git implementation, at the cost of subprocess overhead and a host Git installation requirement for diff features.

The owner authorized continuing desktop development with Linux evidence while Windows and macOS machines are unavailable. That changes development sequencing; it does not establish native compatibility on those platforms.

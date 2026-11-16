# Execute bundled utilities as native processes

Rodtty's initial utility provider is Brush's feature-selected uutils registry, but enabled utilities execute through native launcher files as ordinary external processes. The launchers are hard links to `rodtty-shell`, with a copy fallback, and dispatch by their invoked name. Each session owns a snapshot of its launchers and disabled-command selection. Disabled names additionally receive Brush builtin guards; shell built-ins remain separate from bundled utilities.

Brush's experimental bundled-command builtin shim waits for a child to finish before returning from builtin execution. Its pinned source documents pipeline serialization and process-group propagation limitations. Using that shim for enabled utilities would undermine concurrent pipelines and foreground interrupts. Native external dispatch preserves the shell's existing pipeline and process-group path, demonstrated locally by a 100,000-line pipeline, full-screen applications, and interrupt tests. Revisit this arrangement if Brush supplies a validated native bundled-dispatch path.

Source: [pinned Brush bundled-command implementation](https://github.com/reubeno/brush/blob/fbd1eb36158384bbd5ad542445a5324bebefce78/brush-shell/src/bundled.rs).

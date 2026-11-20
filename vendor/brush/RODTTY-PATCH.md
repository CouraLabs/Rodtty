# Rodtty's copy of Brush

Source: [reubeno/brush](https://github.com/reubeno/brush) at
`fbd1eb36158384bbd5ad542445a5324bebefce78`. Brush is MIT licensed; its notice
is kept in `LICENSE`.

Rodtty builds its shell (`rodtty-shell`) on these crates, so they live here as
part of Rodtty and are fixed and extended in place.

Vendored crates: `brush-shell`, `brush-core`, `brush-parser`, `brush-builtins`,
`brush-interactive`, `brush-coreutils-builtins`, `brush-winnow-args-adapter`,
`brush-experimental-builtins`, `brush-clap-adapter`, `brush-test-harness`.
The upstream `brush` facade crate, `brush-bundled-tests`, `fuzz`, `xtask`,
benchmarks, e2e tests and docs are not included, and the workspace member list
in `Cargo.toml` names only the vendored crates.

The directory remains its own Cargo workspace, so Brush keeps its manifests,
lints and tests. Run Brush's tests with `cargo test` from this directory.

## Changes from upstream

Every change to Brush's source is listed here, newest last.

- `brush-interactive`, minimal input backend: a line ending in CR LF (how a
  Windows console ends lines) is read as ending in LF, so the CR doesn't join
  the command's last word. Before, `cd dir` on Windows looked for `dir\r` and
  failed with "os error 123".
- `brush-interactive`, VT backend: `StdTerminal` switches the Windows console
  to raw mode (VT input sequences, no line input, echo or Ctrl-C processing),
  reports its width, and waits for key events, so the VT line editor works on
  Windows. `brush-shell`'s build script offers the VT backend on Windows.
  Adds a Windows-only `windows-sys` dependency to `brush-interactive`.

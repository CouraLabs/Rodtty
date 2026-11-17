# VTE provenance and local patch

This directory vendors the complete published `vte` 0.15.0 crate from
<https://crates.io/crates/vte/0.15.0>, corresponding to upstream commit
`3b3da71c34cc1256c7e20981cf03f8eb95e08ffc` in
<https://github.com/alacritty/vte>. The package version and its MIT/Apache-2.0
licenses are unchanged. Cargo's local `.cargo-ok` cache marker is omitted.

Rodtty patches `Parser::advance_osc_string` in `src/lib.rs`: after the first
delimiter, OSC 0/1/2 (icon/window titles) and OSC 7 (working directory) retain
semicolons as bytes of their single string payload. This prevents the existing
16-parameter limit from discarding valid title/directory text. Other OSC commands
retain upstream parameter splitting and limits. The existing `std` raw buffer
and bounded `no_std` raw buffer behavior are unchanged.

The application regression coverage is in `tests/terminal.rs`, through real PTY
output and the public terminal session title/working-directory interfaces.

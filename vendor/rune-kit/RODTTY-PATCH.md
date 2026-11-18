# Owned Rune Kit patch

Source: anomalyco/rune at eb5ae6c9f77b1a0051d8e1e5ee936ebea086bb90,
crates/rune-kit. Upstream MIT notice is retained in LICENSE.

Only production-source differences, recorded in `rodtty.patch`:

- Kit optional syntax and selection colors; neutral defaults them to None;
  SyntaxTheme::from_kit uses the optional app theme. This keeps editor,
  embedded code widgets and read-only DiffView on one palette.
- Additional glyphs used by Rodtty's header band and explorer controls.
- DiffView draws permanent `+` and `−` sign markers beside line numbers, hidden
  from assistive technology because each row's label already names the change.
- TreeStyle::selected_mark draws an accent bar on the selected tree row.

The standalone manifest pins upstream sibling crates to the same revision.
T06 typography and diff sizing remain unchanged.

The imported `src/syntax.rs` upstream test block uses LF locally where the pinned
original uses CRLF. The recorded production patch applies to the exact original;
normalizing that unchanged test block's line endings then reproduces the owned
file byte-for-byte. This import normalization changes no Rust statements.

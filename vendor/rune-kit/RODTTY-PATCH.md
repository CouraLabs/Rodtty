# Owned Rune Kit patch

Source: anomalyco/rune at eb5ae6c9f77b1a0051d8e1e5ee936ebea086bb90,
crates/rune-kit. Upstream MIT notice is retained in LICENSE.

Only production-source differences: Kit optional syntax and selection colors;
neutral defaults them to None; SyntaxTheme::from_kit uses the optional app theme.
The standalone manifest pins upstream sibling crates to the same revision.
This keeps editor, embedded code widgets and read-only DiffView on one palette.
T06 typography and diff sizing remain unchanged.

The imported `src/syntax.rs` upstream test block uses LF locally where the pinned
original uses CRLF. The recorded production patch applies to the exact original;
normalizing that unchanged test block's line endings then reproduces the owned
file byte-for-byte. This import normalization changes no Rust statements.

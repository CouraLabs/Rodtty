# Owned Rune Kit patch

Source: anomalyco/rune at eb5ae6c9f77b1a0051d8e1e5ee936ebea086bb90,
crates/rune-kit. Upstream MIT notice is retained in LICENSE.

Only production-source differences: Kit optional syntax and selection colors;
neutral defaults them to None; SyntaxTheme::from_kit uses the optional app theme.
The standalone manifest pins upstream sibling crates to the same revision.
This keeps editor, embedded code widgets and read-only DiffView on one palette.
T06 typography and diff sizing remain unchanged.

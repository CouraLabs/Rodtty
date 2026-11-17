# Owned Rune platform bridge

Upstream: https://github.com/anomalyco/rune

Revision: `eb5ae6c9f77b1a0051d8e1e5ee936ebea086bb90` (MIT; see LICENSE).

This is only `crates/rune` source, required build assets, embedded-font notices,
precompiled platform shaders, its build script, and a standalone Cargo manifest.
Examples, integration-test executables, artifacts and the rest of the upstream
repository are omitted. Source-local upstream test modules remain with the source.
The main application test suite exercises the owned bridge at runtime/native seams.

The manifest concretizes the upstream 2024 edition and MIT license. Upstream sibling
path dependencies and tiny-skia-path resolve from the same pinned Git revision;
no upstream cache is edited. Existing root winit/wgpu-hal/parley_engine pins remain.
The root Git-source patch applies to rune-kit as well as Rodtty so there is one
Rune runtime/type identity. `cargo tree -i rune` verifies this.

Owned behavior changes are deliberately narrow:

- `WindowHandle::with_winit_window`: borrowed callback with an explicit UI-thread
  contract, for native state/operation access. It preserves winit's Option/unknown
  state on Wayland instead of deriving maximization from unavailable placement.
- `WindowOptions::custom_frame_resize_border`: opt-in logical hit band for a
  borderless resizable Linux window. Native pressed events initiate winit resize
  before runtime/deferred shell handling, preserving the actual Wayland seat and
  pressed-button serial. Eight directions and matching native cursors are supplied.
  Maximized/fullscreen/fixed/decorated windows bypass the band; app zoom does not
  alter its physical window-point size. The app draws/pads the corresponding band.
- Remove only the blanket borderless minimize rejection. Visibility, panel/launcher
  and enabled-button guards continue to apply, as does the canonical lifecycle.

The resize integration is Linux-only. winit's macOS drag_resize_window is unsupported
and requires a separate AppKit adapter. Windows/macOS retain native decorations
until their adapters opt in. No native runtime evidence is claimed for those hosts.

`rodtty.patch` records the delta to the pinned Rune crate, including standalone
manifest changes. Regenerate it against the revision above when changing the bridge;
never patch the Cargo Git checkout. Upstream pre-existing compiler/clippy warnings
are dependency warnings, not application warnings.

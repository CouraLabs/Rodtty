# macOS window frame

macOS keeps its native frame. The traffic lights, window movement, resizing and
zoom are the system's own, so they match the owner's accent, hover reveals and
Option-click behaviors, and AppKit answers `is_zoomed` without a style-mask
round trip. Rodtty draws a full-size content view under a transparent titlebar
(`App::titlebar(Titlebar::new(44.0))`), keeps its header clear of the lights
with `Cx::chrome_insets().leading`, and marks the band `.window_drag()`.

[Windows](windows-custom-frame.md) and Linux keep the app-drawn frame: neither
has a native frame Rodtty can style, so they draw the six-point border, the
header and the minimize/maximize/close controls at the trailing edge.
See [ADR-0009](adr/0009-keep-native-macos-window-frame.md) for why macOS
diverges, and [ADR-0007](adr/0007-draw-themed-desktop-window-chrome.md) for the
frame the other two still use.


## Validation status

Implementation and platform-independent Rune runtime tests are complete on Linux.
Apple Silicon `cargo check --target aarch64-apple-darwin --all-targets` checks the
actual AppKit adapter, desktop, and tests. For metadata checking on Linux, set
`CARGO_FEATURE_PURE=1` to select blake3's portable Rust path; no Apple SDK or native
linking/runtime result is implied. The pinned dependency has existing warnings.

**Native macOS GUI execution has now been run** on Apple Silicon: the traffic
lights render, the header band drags and double-clicks the window, the header's
icon buttons take their presses, and the window's place on the desktop is saved
and reopened. Resize-by-edge, minimize, and the session-lifecycle pass remain
unclaimed. The runtime
suite exercises the actual left-control layout at 640×360 and 2× scale, header
child exclusion, header drag/double-click actions, Settings access, and a dirty
custom-close/Cancel that preserves the draft.

## Native macOS checks

On a macOS GUI host with Xcode command-line tools, build Rodtty and grant the
invoking terminal Accessibility permission for synthetic Quartz input. The
harness queries only its launched PID; it uses an isolated RODTTY_HOME. Keep
TMPDIR on the binary's filesystem so session profiles can hard-link executables.

```sh
cargo build --locked --bins
mkdir -p "$PWD/target/native-mac-tmp"
TMPDIR="$PWD/target/native-mac-tmp" \
GIT_CEILING_DIRECTORIES="$PWD/target/native-mac-tmp" \
python3 scripts/native/check-macos-frame.py \
  --binary "$PWD/target/debug/rodtty" \
  --report "$PWD/target/macos-frame.json"
```

The Python harness compiles its Swift AppKit/Quartz input helper on the Mac,
clicks the actual custom controls, and independently observes native geometry.
It checks maximize/restore geometry, inactive maximized resize band, header
movement/double-click, all eight resize directions and fixed opposite edges,
minimum-size clamping, native miniaturization/restore, dirty-close cancellation,
then runs Rodtty's existing save/closed-window-output/same-session-relaunch check.
Failures remain failures in the report; pending checks are not marked passed.
The harness itself has not been compiled or executed on macOS yet.

Before qualifying the platform, also perform and record these manual checks:

- Inspect whole-window light/dark screenshots at wide and minimum sizes; confirm
  the native traffic lights are present and the header's icons keep their
  hover/focus states clear of them.
- Repeat at Retina and non-Retina scales, after changing display scale, and across
  monitors with negative origins. Resize outward beyond the original NSView and
  back inward; release outside it and confirm subsequent clicks work.
- Set System Settings → Desktop & Dock → titlebar double-click to Minimize, rerun
  the harness, and confirm double-click still only maximizes/restores. Repeat with
  None/Zoom. Leave the owner's preference as found. (Only the traffic lights and
  the band above the header now follow that preference; the header band does not.)
- Enter native full screen and verify edge/corner drags do not resize; leave full
  screen and verify resizing and restore geometry. Check focus loss mid-gesture.
- Click Files, Terminal, Editor, Settings and menu controls; verify clicks do not
  start window movement, including keyboard focus at minimum width.
- Exercise all Close Window policies and unsaved Save/Discard/Cancel, then confirm
  surviving terminal output and same-session reattachment. Test Dock restoration,
  native keyboard/window-menu behavior, and multiple window activations.

Record hardware, macOS version, display/scaling configuration, JSON report and
screenshots alongside any eventual qualification result. Linux checks and Apple
metadata compilation do not substitute for these native results.

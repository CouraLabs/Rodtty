# macOS app-drawn window frame

macOS opts into the same themed six-point frame and header as Linux. The left
controls are Close, Minimize, and Maximize/Restore, with the same button focus,
hover, help, and canonical Close Window action as the rest of Rodtty. The native
window is undecorated: no standard title bar, traffic lights, or visible OS border.
[Windows](windows-custom-frame.md) uses the shared winit resize adapter and right
controls; native Windows execution also remains pending.

The [owned Rune bridge](../vendor/rune/UPSTREAM.md) starts an AppKit resize gesture
in the actual pressed event. It captures the NSWindow frame, global NSEvent pointer,
and native minimum/maximum frame size on the main thread. Subsequent winit
mouseDragged events update NSWindow with setFrame:display:, including while the
pointer is outside NSView; mouseUp applies the final location and releases the
gesture. Focus loss cancels tracking. No unsupported winit drag_resize_window call
is made on macOS. All eight edge/corner directions preserve their opposite edge
when clamped. AppKit's bottom-left origin is handled directly in global points,
independent of Retina scale and app zoom. Maximized/fullscreen windows bypass the
resize band. Header drag, minimize, and maximize/restore retain winit's native
AppKit operations. App-frame double-click explicitly toggles maximize once,
regardless of the OS titlebar double-click preference.

The pinned winit NSView implementation forwards mouseDragged outside its bounds
while a button is held and sends a final mouse-motion event before mouseUp. The
bridge uses that real event path rather than installing a second input monitor.

## Validation status

Implementation and platform-independent Rune runtime tests are complete on Linux.
Apple Silicon `cargo check --target aarch64-apple-darwin --all-targets` checks the
actual AppKit adapter, desktop, and tests. For metadata checking on Linux, set
`CARGO_FEATURE_PURE=1` to select blake3's portable Rust path; no Apple SDK or native
linking/runtime result is implied. The pinned dependency has existing warnings.

**Native macOS GUI execution remains pending.** No native geometry, resize,
miniaturization, visual, or macOS session-lifecycle pass is claimed. The runtime
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
  no native traffic lights, title strip or standard frame, and visible focus/hover.
- Repeat at Retina and non-Retina scales, after changing display scale, and across
  monitors with negative origins. Resize outward beyond the original NSView and
  back inward; release outside it and confirm subsequent clicks work.
- Set System Settings → Desktop & Dock → titlebar double-click to Minimize, rerun
  the harness, and confirm double-click still only maximizes/restores. Repeat with
  None/Zoom. Leave the owner's preference as found.
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

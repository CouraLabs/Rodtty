# Linux app-drawn window frame

Linux uses an undecorated native Rune/winit window. Rodtty draws its six-point
resize band, thin themed border, header and right-side minimize/maximize/close
controls. All surfaces consume the resolved appearance. Header children keep their
click/focus behavior; bare header space supports drag and double-click maximize or
restore. Restored geometry belongs to the window manager. Custom close dispatches
the canonical Close Window action, preserving unsaved editor decisions and terminal
session lifetime.

The [owned Rune bridge](../vendor/rune/UPSTREAM.md) adds only a UI-thread native
window borrow and opt-in Linux frame hit band, and removes the blanket borderless
minimize guard. Resize runs synchronously inside the native pressed event so
winit supplies Wayland's real seat/button serial. It bypasses maximized/fullscreen
windows and fixed/decorated windows. Native resize cursors track all eight
edge/corner directions. The band uses logical window points independently of app
zoom, and winit continues to enforce the minimum window size.

Windows/macOS keep their native decorations until their separate adapters opt in
to the shared app frame. winit resize is unsupported on macOS; no Mac resize
implementation or Windows/Mac native runtime validation is claimed here.

## Native validation

Run `scripts/native/check-linux-frame.py` inside a KWin desktop. It launches an
isolated RODTTY_HOME, selects only that exact process PID, reads native/compositor
geometry, drives real pointer input, restores prior focus/pointer where exposed,
then verifies dirty-close cancellation, saving, background terminal output and
same-session relaunch. It does not inspect unrelated window titles.

```sh
python3 scripts/native/check-linux-frame.py \
  --binary "$PWD/target/debug/rodtty" --backend wayland \
  --fake-input-xml /path/to/plasma-wayland-protocols/src/protocols/fake-input.xml \
  --report /tmp/rodtty-frame-wayland.json --screenshot /tmp/rodtty-frame-wayland.png
```

Use `--backend x11` for Xlib. The harness unsets WAYLAND_DISPLAY and WAYLAND_SOCKET
for the launched app and checks the observed raw-handle backend. On a Wayland
KWin desktop, supply fake-input.xml for Xlib as well: this drives the actual Xlib
window through compositor input. Without that option, X11 input uses libXtst;
Xwayland XTest virtual-pointer behavior did not reach the controls in this desktop.
The helper compiles using `cc`, `wayland-scanner`, libwayland-client and the KDE
protocol supplied by the caller. The protocol is used only for test input, never
by Rodtty's resize implementation. Python dbus and GLib bindings are also needed.

Wayland's winit minimized state is unknown (`null`). The harness independently
observes compositor minimization and restores through compositor activation, as
the taskbar does; it never treats a request as a native acknowledgement. Wayland
has no xdg request for undoing minimization. KWin's noBorder rule flag may be false
for a CSD window: zero frame/client decoration extents plus native decorated=false
are the observed decoration checks. Restored geometry allows half a physical
pixel for fractional-scale floating-point reporting.

[Wayland evidence](evidence/custom-frame-wayland.json) and
[Xlib evidence](evidence/custom-frame-x11.json) record actual native runs. Both
cover all eight edge/corner resizes, header movement/double-click, maximize/restore
geometry, minimize, the actual custom close button and unsaved Cancel, file saving,
closed-window output, and relaunch of the same session. Whole-window screenshots
show [Wayland](assets/linux-custom-frame-wayland.png) and
[Xlib with its unsaved prompt](assets/linux-custom-frame-x11-prompt.png).

The runtime suite separately checks header-child exclusion, control ordering and
full bounds at the 640×360 minimum, Settings access, and preserving an unsaved
editor after custom close/Cancel. Native Linux evidence was collected under KWin
with Wayland at 150% scale and actual Xlib; other compositors have not been run.

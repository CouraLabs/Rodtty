# Windows app-drawn window frame

Windows opts into the themed six-point resize band, header and right-side
Minimize, Maximize/Restore and Close controls. These use the shared application
button hover, keyboard focus, help and resolved theme. Close dispatches the
canonical Close Window action, preserving unsaved-file decisions and configured
terminal-session lifetime.

The [owned Rune bridge](../vendor/rune/UPSTREAM.md) sets decorations=false and
suppresses winit's undecorated-shadow path only for app-frame windows. In the
pinned winit, that shadow path adds a visible one-pixel native border in
WM_NCCALCSIZE. Native sizing, minimize/maximize and AeroSnap styles remain owned
by winit. Rodtty does not install Rune's `.titlebar()` convenience frame, strip
WS_SIZEBOX, or draw duplicate OS controls. Ordinary windows retain their defaults.

Native pointer-down immediately invokes winit drag_resize_window for each of the
eight edge/corner directions; the matching cursor is updated from the same band.
Both GPU and Windows CPU-rendered window creation initialize it. The band uses
logical window points independently of app zoom and bypasses maximized,
fullscreen, decorated and fixed-size windows. Native sizing enforces the minimum.
Bare header space uses winit drag_window and double-click maximize/restore;
header children opt out of dragging. Restore geometry remains managed by Windows.
Minimize uses the ordinary native operation, including on undecorated windows.

## Validation status

The actual Windows desktop, owned Rune adapter and all test targets pass
`CARGO_FEATURE_PURE=1 cargo check --offline --locked --target x86_64-pc-windows-msvc --all-targets`
on Linux with the installed MSVC Rust target. PURE selects blake3's portable Rust
path for metadata checking. This requires no Windows SDK/native linking and is
not a Windows executable or native GUI result. Existing upstream warnings remain.

Linux Rune runtime tests exercise the shared right-control layout, complete
control bounds at 640×360, child exclusion, Settings access, header Drag/DoubleClick,
and dirty custom-close/Cancel preserving the draft. Serial filesystem/Git/PTY
integration, strict clippy, formatting, desktop build and backend checks pass.

**Native Windows execution is pending.** Windows frame visibility, native input,
DPI/focus behavior, minimize/maximize/restore, AeroSnap, visual qualification and
session lifecycle have not been executed on Windows hardware. The prepared
harness has passed Python syntax and CLI checks on Linux only.

## Native Windows checks

On an unlocked interactive Windows desktop with Rust, Git and Python installed,
build the binaries and run the native harness from PowerShell. Use the same
integrity level for Python and Rodtty so SendInput can reach the isolated process.
Keep fixtures on the binaries' filesystem for session executable hard links.

```powershell
cargo build --locked --bins
$frameTmp = Join-Path $PWD 'target/native-windows-tmp'
New-Item -ItemType Directory -Force $frameTmp | Out-Null
$env:TMPDIR = $frameTmp
$env:TEMP = $frameTmp
$env:TMP = $frameTmp
$env:GIT_CEILING_DIRECTORIES = $frameTmp
python scripts/native/check-windows-frame.py `
  --binary "$PWD/target/debug/rodtty.exe" `
  --report "$PWD/target/windows-frame.json"
```

The harness targets only its launched PID and isolated RODTTY_HOME. Win32 and
DWM independently report client, outer and visible geometry; SendInput drives
actual controls and pointer gestures. It checks no native decoration extents,
retained native sizing style, maximize/restore geometry, inactive maximized resize
band, header movement/double-click, child non-drag, all eight resize directions
and opposite edges, minimum-size clamping, native minimization/taskbar-style
restore, and dirty-close cancellation. It then runs the existing save,
closed-window output and same-session relaunch checks. Failures stay failures in
the JSON report. Native execution must produce a report before claiming a pass.

Complete these additional manual checks and record screenshots/results:

- Inspect light/dark whole-window screenshots at wide and minimum sizes, with
  focus and hover on the custom controls; check for native title strips/borders.
- Repeat at 100%, 150% and 200% display scaling, across mixed-DPI monitors and
  negative screen coordinates. Move between displays and repeat control/resize
  hit tests, minimum-size constraints and maximize/restore geometry.
- Snap using Win+Arrow and header drag to screen edges, unsnap by dragging the
  header, and maximize on displays with taskbars in different positions. Confirm
  the window fits the work area and restores normally. Native Windows 11 snap
  layout flyouts on custom maximize hover are not part of this implementation.
- Verify native keyboard/window-menu restoration and taskbar activation. Exercise
  fullscreen and focus loss during movement/sizing. Check Files, Terminal,
  Editor, Settings and menu clicks never start a native window drag.
- Exercise every Close Window policy and dirty Save/Discard/Cancel; confirm
  background terminal output and same-session reattachment after reopening.

Record Windows version, hardware, display/DPI setup, JSON reports and screenshots.
Linux native evidence and Windows metadata compilation do not replace these runs.

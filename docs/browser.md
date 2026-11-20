# Embedded browser (CEF)

The Browser is built with the `browser` Cargo feature, using the Rust [`cef`](https://docs.rs/cef) crate and its matching Chromium Embedded Framework build. Builds without the feature, or without the CEF runtime beside the executable, are terminal-only: they omit the Open Browser icon and palette actions and advertise no browser tools.

## Pinned versions

| Component | Version | Source |
| --- | --- | --- |
| `cef` crate | `=156.7.0+156.0.31` (pinned in `Cargo.toml`) | crates.io, [tauri-apps/cef-rs](https://github.com/tauri-apps/cef-rs) |
| CEF binary distribution | `156.0.31+ge603d97+chromium-156.0.8078.100`, "minimal" | `cef-builds.spotifycdn.com`, downloaded by `export-cef-dir` with its SHA-1 verified |

The crate's version names the CEF build it binds; update both together:

1. Bump `cef` in `Cargo.toml` to the new `=X.Y.Z+A.B.C`.
2. `cargo run -p export-cef-dir -- --force ~/.local/share/cef` from a cef-rs checkout at the same tag (it verifies the archive hash), and point `CEF_PATH` at it.
3. Build with `CEF_PATH=… cargo build --features browser` (needs CMake and Ninja for the CEF wrapper library), run `scripts/browser/dev-app.sh`, and rerun the checks below.

## Architecture

- **Rendering:** windowless (off-screen) rendering. CEF paints BGRA frames into memory; Rodtty shows them through a Rune `NativeSurface` and keeps the latest frame for screenshots and recordings. This was chosen over a native child view because the same frames serve the owner view, background agent targets, screenshots and video, and because input can then go to the browser alone. A browser target renders whether or not it is shown and whether or not Rodtty is the active application.
- **Input:** CEF's browser-host input API (`send_mouse_*_event`, `send_key_event`) into one renderer. No operating-system input, global hooks or clipboard use.
- **Process model:** the main `rodtty` executable never runs as a CEF subprocess. Renderer, GPU and utility processes use `rodtty-browser-helper` (on macOS inside the `Rodtty Helper*.app` bundles, entering Chromium's sandbox first), so startup, the owner instance lock and terminal sessions don't see them.
- **Lifecycle:** CEF initializes on the first browser open. Its message loop runs on Rodtty's UI thread through CEF's external pump. Closing the Browser view releases the owner's browser; Quit closes every browser, shuts CEF down and removes the ephemeral profile. If initialization fails or a renderer exits, the Browser shows a recoverable error (reload) and terminals and editors are unaffected.
- **macOS:** CEF requires the shared `NSApplication` to implement `CefAppProtocol`; Rodtty installs a subclass before the window system starts, only when the runtime is present.

## Isolation

- An in-memory profile per process (`root_cache_path` under `<app root>/browser/profile-<pid>`, removed on Quit), and a separate request context per target, so agent browsing and the owner's browsing don't share cookies or storage. No history, cookies or last URL persist.
- Chromium's sandbox stays on. Certificate errors cancel the request and show in the error state; they are never bypassed.
- Top-level navigation is limited to `http`, `https` and `about:blank`, including redirects. Popups and new windows, downloads and JavaScript dialogs are blocked with a status. Pages have no bridge to Rodtty.
- No remote debugging port. Automation uses DevTools protocol messages sent in-process to read page state. CEF logging is disabled, so page addresses stay out of diagnostics.

## Addresses

`localhost:4200`, `127.0.0.1:8080`, `[::1]:3000` and `*.localhost` open over HTTP; other bare hosts use HTTPS; `http://` and `https://` are kept. Other schemes, spaces and empty input are rejected. There is no web search.

## Recordings

Browser video comes only from the target's own frames: the encoder thread samples the latest frame at the chosen rate (1–30 fps), repeats the previous frame when nothing new painted, and drops sampling ticks instead of queueing when it falls behind (reported as `dropped`). Output is Motion JPEG in AVI (`.avi`), written to `.avi.partial` and renamed when final; failures remove the partial file. Limits: duration (default 120 s, max 600 s), width (default 1920 px) and 1 GiB. Closing the browser or quitting finalizes the file. A resize keeps the first frame size and scales into it. The footer and the Browser toolbar show `● REC` with a Stop action. Play the files with VLC, mpv, ffplay or any ffmpeg-based player; QuickTime Player does not play Motion JPEG AVI.

## Packaging

- **macOS:** `scripts/package/macos-cef.sh <Rodtty.app> <bin dir>` adds `Chromium Embedded Framework.framework` and the helper apps; `scripts/browser/dev-app.sh` builds an ad-hoc-signed development bundle at `target/browser/Rodtty.app`. Release signing must sign each helper app, the framework, `rodtty-shell`, `rodtty`, then the app, with the hardened runtime; notarization of a browser-enabled package has not been run yet.
- **Windows and Linux:** the code paths exist (`libcef.dll`/`libcef.so` and `rodtty-browser-helper` beside `rodtty`) but no browser-enabled package has been built or qualified. Ship terminal-only packages there until it is.

## Licenses

CEF is BSD-3-Clause; Chromium bundles many third-party licenses, listed in `CREDITS.html` of the CEF distribution. A browser-enabled package must include that file (and the `cef` crate's MIT/Apache-2.0 notice) with Rodtty's own license.

## Checks

With the dev bundle running for an app root (`RODTTY_HOME`), from `scripts/automation`:

- `browser-check.mjs`: redirect, form fill by accessible name, virtual click and wheel, stale references, a screenshot, a playable recording checked with `ffprobe`, and the owner's frontmost app and pointer before and after.
- `tls-check.mjs`: a self-signed HTTPS fixture is rejected with a certificate error.
- `e2e.mjs`: the whole orchestration, including renderer kill and recovery and the recording duration limit.

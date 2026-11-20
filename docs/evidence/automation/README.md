# Automation evidence

Recorded on 2026-11-20 on macOS 27.2.1 (build 26B131), Apple silicon (arm64), against the browser-enabled development bundle built by `CEF_PATH=~/.local/share/cef scripts/browser/dev-app.sh` from this repository (CEF 156.0.31, ad-hoc signed). The owner was using the machine during the runs: WhatsApp stayed frontmost and the owner moved the physical pointer.

| Check | Command (from `scripts/automation`, with `RODTTY=…/target/browser/Rodtty.app/Contents/MacOS/rodtty`) | Result |
| --- | --- | --- |
| End-to-end orchestration (task 31) | `RODTTY_HOME=/tmp/rt-e2e node e2e.mjs e2e-evidence.json` | Passed, 19 checks: [e2e-macos-arm64.json](e2e-macos-arm64.json) |
| Terminals, input, IDs, revocation | `node terminal-check.mjs` | Passed |
| Browser form fill, virtual pointer and wheel, screenshot, video, physical pointer unchanged | `node browser-check.mjs` | Passed |
| Self-signed TLS rejected | `node tls-check.mjs` | Passed (`ERR_CERT_AUTHORITY_INVALID`) |
| Local services ownership | `node services-check.mjs` | Passed |
| Project tasks | `node tasks-check.mjs` | Passed |
| `rodtty open` with a running instance, concurrent opens | `rodtty open "/tmp/open-proj/a dir/fïle one.txt:2"`, then `node open-check.mjs` | Passed |
| Structured Codex adapter | instance started with `RODTTY_CODEX_APP_SERVER="python3 fixtures/fake-codex-app-server.py" --control-stdin`, then `RODTTY_CONTROL=<fifo> node codex-adapter-check.mjs` | Passed |

The e2e run started a dev service and an interactive app in separate terminals, submitted a two-line prompt, opened the dev page in a background CEF target, filled and submitted its form with the virtual pointer and keyboard while recording, and saved a screenshot ([e2e-browser-result.png](e2e-browser-result.png)) and a 48-frame, 3.2 s MJPEG AVI that `ffprobe` reads. It then edited a source file at its observed revision, saved it, and opened the diff for review.

Owner takeover and stuck-key release are covered by `cargo test --lib agent_text_and_keys_reach_only_the_named_pty_and_owner_takeover_stops_them` and the input tests in `src/desktop/automation/tests.rs`.

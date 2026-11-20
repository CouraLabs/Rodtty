// Task 31: the end-to-end orchestration run through a real MCP client.
//
// In a fixture Git project: start a dev service in one terminal, an
// interactive TUI in another, submit a multiline prompt, open the dev page in
// a background CEF target, fill and submit its form with the virtual pointer
// and keyboard while recording, save a screenshot, edit a source file at the
// observed revision, save it, and open its diff for review. Then exercise
// failure cases: duplicate requests, reconnect without replay, stale IDs,
// recording duration limit, renderer crash and recovery, and revocation.
// The owner's frontmost app and pointer are sampled throughout.
//
//   RODTTY=<Rodtty.app>/Contents/MacOS/rodtty RODTTY_HOME=<app root> node e2e.mjs [evidence.json]
import { execFileSync } from "node:child_process";
import { existsSync, mkdtempSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { connect, check, sleep } from "./client.mjs";

const rodtty = process.env.RODTTY;
const home = process.env.RODTTY_HOME;
const here = dirname(fileURLToPath(import.meta.url));
const env = { ...process.env, RODTTY_HOME: home };
const evidence = { started: new Date().toISOString(), checks: [], desktop: [] };
const ok = (condition, message) => {
  evidence.checks.push({ message, passed: !!condition });
  check(condition, message);
};
const desktop = (label) => {
  const state = JSON.parse(execFileSync("python3", [join(here, "fixtures", "desktop-state.py")], { encoding: "utf8" }));
  state.front = state.front.split(" ASN")[0];
  evidence.desktop.push({ label, ...state });
  return state;
};

// A fixture Git project with a source file the agent will edit.
const project = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-e2e-")));
const git = (...args) => execFileSync("git", ["-C", project, ...args], { stdio: "ignore" });
git("init", "-q");
writeFileSync(join(project, "greeting.txt"), "Hello from the fixture.\nStatus: draft\n");
git("add", ".");
git("-c", "user.email=e2e@example.com", "-c", "user.name=e2e", "commit", "-qm", "fixture");

const start = desktop("before");
let mcp = await connect({ rodtty, home, name: `e2e-${process.pid}` });
let status = await mcp.call("rodtty_status");
execFileSync(rodtty, ["automation", "approve", status.client.id, "--workspace", project], { env, stdio: "ignore" });
status = await mcp.call("rodtty_status");
ok(status.client.status === "approved", `client ${status.client.id} approved for ${project}`);
evidence.instance = status.instance;
evidence.features = status.features;

console.log("1. dev service and interactive app in separate terminals");
const service = await mcp.call("terminal_launch", { cwd: project, executable: "node", args: [join(here, "fixtures", "web.mjs"), "0"], title: "dev service", request_id: "e2e-service" });
const up = await mcp.call("terminal_wait", { terminal: service.terminal, regex: "listening on (\\d+)", cursor: 0, timeout_ms: 20000 });
const port = up.match.match(/\d+/)[0];
const app = await mcp.call("terminal_launch", { cwd: project, executable: "python3", args: [join(here, "fixtures", "tui.py")], title: "interactive app", request_id: "e2e-app" });
await mcp.call("terminal_wait", { terminal: app.terminal, screen_contains: "PROMPT>", timeout_ms: 20000 });
ok(service.terminal !== app.terminal, "service and app run in separate terminals");

console.log("2. multiline prompt, then Enter");
await mcp.call("terminal_send_text", { terminal: app.terminal, text: "Please review the form.\nThen report back.", request_id: "e2e-prompt" });
await mcp.call("terminal_send_keys", { terminal: app.terminal, keys: ["Enter"], request_id: "e2e-enter" });
const submitted = await mcp.call("terminal_wait", { terminal: app.terminal, screen_contains: "SUBMITTED 1", timeout_ms: 10000 });
const appScreen = await mcp.call("terminal_screen", { terminal: app.terminal });
ok(appScreen.lines.some((l) => l.includes("Then report back.")), "the interactive app received and submitted the two-line prompt");
const terminalShot = await mcp.call("capture_screenshot", { target: app.terminal, request_id: "e2e-term-shot" });
evidence.terminal_screenshot = terminalShot.path;

console.log("3. browser workflow while the owner keeps using their app");
const { browser } = await mcp.call("browser_open", { width: 1100, height: 760, request_id: "e2e-browser" });
const recording = await mcp.call("recording_start", { browser, fps: 15, max_seconds: 120, request_id: "e2e-rec" });
await mcp.call("browser_navigate", { browser, url: `localhost:${port}/redirect` });
const during = desktop("during browser workflow");
let snap = await mcp.call("browser_snapshot", { browser });
const find = (role, name) => snap.elements.find((e) => e.role === role && e.name === name);
await mcp.call("browser_type", { browser, ref: find("textbox", "Your name").ref, text: "E2E Agent", delay_ms: 25 });
await mcp.call("browser_type", { browser, ref: find("textbox", "Message").ref, text: "Filled with virtual input", delay_ms: 10 });
await mcp.call("browser_scroll", { browser, delta_y: 500 });
await sleep(400);
await mcp.call("browser_scroll", { browser, delta_y: -500 });
await sleep(300);
await mcp.call("browser_click", { browser, ref: find("button", "Send").ref });
await mcp.call("browser_wait", { browser, text: "Thanks, E2E Agent!", timeout_ms: 15000 });
await sleep(1000);
const browserShot = await mcp.call("capture_screenshot", { target: browser, cursor: true, request_id: "e2e-browser-shot" });
const video = await mcp.call("recording_stop", { recording: recording.recording, request_id: "e2e-rec-stop" });
const probe = JSON.parse(execFileSync("ffprobe", ["-v", "error", "-count_frames", "-show_entries", "stream=codec_name,width,height,nb_read_frames:format=duration", "-of", "json", video.path], { encoding: "utf8" }));
ok(video.state === "completed" && Number(probe.streams[0].nb_read_frames) === video.frames, `playable video ${video.path}: ${video.frames} frames, ${probe.format.duration}s`);
ok(existsSync(browserShot.path), `browser screenshot ${browserShot.path}`);
evidence.video = { path: video.path, frames: video.frames, duration_ms: video.duration_ms, ffprobe: probe };
evidence.browser_screenshot = browserShot.path;

console.log("4. edit the source at the observed revision and open its diff");
const opened = await mcp.call("editor_open", { path: join(project, "greeting.txt"), line: 2, column: 1, request_id: "e2e-open" });
let editors = { editors: [] };
for (let i = 0; i < 50 && !editors.editors.length; i++) {
  editors = await mcp.call("editor_list");
  await sleep(100);
}
const editor = editors.editors.find((e) => e.path.endsWith("greeting.txt"));
const buffer = await mcp.call("editor_read", { editor: editor.id });
const staleEdit = await mcp.call("editor_edit", { editor: editor.id, expected_revision: buffer.revision + 7, find: "draft", replace: "x" }, { allowError: true });
ok(staleEdit.error?.code === "conflict", "an edit against a stale revision is rejected");
const edited = await mcp.call("editor_edit", { editor: editor.id, expected_revision: buffer.revision, find: "Status: draft", replace: "Status: reviewed by the agent", request_id: "e2e-edit" });
const saved = await mcp.call("editor_save", { editor: editor.id, expected_revision: edited.revision, request_id: "e2e-save" });
ok(saved.saved && !saved.dirty, "saved through the version-protected save");
const diff = await mcp.call("git_diff", { path: join(project, "greeting.txt"), show: true });
ok(diff.patch.includes("+Status: reviewed by the agent"), "the worktree diff shows the change and opens for review");
const { workspaces } = await mcp.call("workspace_list");
const workspace = workspaces.find((w) => w.root === project);
const changes = await mcp.call("git_changes", { workspace: workspace.id });
ok(changes.unstaged.some((c) => c.relative === "greeting.txt"), "Git changes list the edited file as unstaged");
evidence.diff = diff.patch;
void opened;

console.log("5. failure cases");
const dupe = await mcp.call("terminal_launch", { cwd: project, executable: "node", args: [join(here, "fixtures", "web.mjs"), "0"], title: "dev service", request_id: "e2e-service" });
ok(dupe.terminal === service.terminal && dupe.replayed, "a duplicate launch returns the first operation");
const before = (await mcp.call("terminal_list")).terminals.length;
await mcp.close();
mcp = await connect({ rodtty, home, name: `e2e-${process.pid}` });
const after = (await mcp.call("terminal_list")).terminals.length;
ok(after === before, `reconnecting replays nothing (${after} terminals before and after)`);
const reconnectScreen = await mcp.call("terminal_screen", { terminal: app.terminal });
ok(!reconnectScreen.lines.some((l) => l.includes("SUBMITTED 2")), "the prompt was submitted once across the reconnect");
const staleId = await mcp.call("terminal_screen", { terminal: "term_00000000_1" }, { allowError: true });
ok(staleId.error?.code === "unknown_target", "a terminal ID from another run is unknown");
const limited = await mcp.call("recording_start", { browser, fps: 10, max_seconds: 2, request_id: "e2e-limit" });
await sleep(3500);
const limitStatus = await mcp.call("recording_status", { recording: limited.recording });
ok(limitStatus.state === "stopped_at_limit" && limitStatus.frames === 20 && existsSync(limitStatus.path), `the 2 s limit stopped and finalized the recording (${limitStatus.frames} frames)`);
const crashTarget = await mcp.call("browser_status", { browser });
try {
  execFileSync("pkill", ["-f", "Rodtty Helper --type=renderer"]);
} catch {}
await sleep(1500);
const crashed = await mcp.call("browser_status", { browser });
ok(crashed.crashed, `a renderer exit is reported: ${crashed.crashed}`);
await mcp.call("browser_history", { browser, action: "reload" });
const recovered = await mcp.call("browser_wait", { browser, text: "Thanks", loaded: true, timeout_ms: 15000 }, { allowError: true });
ok(recovered.matched, "reload recovers the page after the renderer exit");
void crashTarget;
const end = desktop("after");
ok(end.front === start.front && during.front === start.front, `the owner's frontmost app stayed ${start.front}`);
// Rodtty sends no operating-system pointer events, so any movement here is the owner's own.
console.log(`  pointer ${start.pointer} -> ${during.pointer} -> ${end.pointer} (owner-controlled)`);

console.log("6. revocation, then clean up as the owner");
execFileSync(rodtty, ["automation", "revoke", status.client.id], { env, stdio: "ignore" });
const revoked = await mcp.call("terminal_interrupt", { terminal: service.terminal }, { allowError: true });
ok(revoked.error?.code === "revoked", "a revoked client cannot send input");
const aliveAfterRevoke = await mcp.call("rodtty_status");
ok(aliveAfterRevoke.client.status === "revoked", "status still reports the revocation");
execFileSync(rodtty, ["automation", "resume", status.client.id], { env, stdio: "ignore" });
await mcp.call("browser_close", { browser });
await mcp.call("terminal_send_keys", { terminal: app.terminal, keys: ["q"] });
await mcp.call("terminal_interrupt", { terminal: service.terminal });
await mcp.call("terminal_wait", { terminal: service.terminal, idle: true, timeout_ms: 10000 });
for (const t of [app.terminal, service.terminal]) await mcp.call("terminal_close", { terminal: t });
execFileSync(rodtty, ["automation", "forget", status.client.id], { env, stdio: "ignore" });
await mcp.close();
evidence.finished = new Date().toISOString();
evidence.passed = evidence.checks.every((c) => c.passed);
if (process.argv[2]) writeFileSync(process.argv[2], JSON.stringify(evidence, null, 2));
console.log("e2e passed");

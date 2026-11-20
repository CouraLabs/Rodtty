// Terminal automation through a real MCP client against a running Rodtty:
// discovery, approval, workspace isolation, launch, multiline prompt into an
// interactive TUI fixture, Enter as a separate key, waits, retries,
// takeover-free input routing and screenshots.
//
//   RODTTY=target/debug/rodtty RODTTY_HOME=/tmp/rt node terminal-check.mjs
// Rodtty must already be running with the same RODTTY_HOME.
import { execFileSync } from "node:child_process";
import { mkdtempSync, writeFileSync, existsSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join, dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { connect, check, sleep } from "./client.mjs";

const rodtty = process.env.RODTTY ?? "target/debug/rodtty";
const home = process.env.RODTTY_HOME;
if (!home) throw new Error("set RODTTY_HOME to the running instance's app root");
const here = dirname(fileURLToPath(import.meta.url));
const fixture = join(here, "fixtures", "tui.py");

const project = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-project-")));
const other = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-other-")));
writeFileSync(join(project, "README.md"), "fixture project\n");

console.log("connect and discover");
const mcp = await connect({ rodtty, home, name: `terminal-check-${process.pid}` });
const { tools } = await mcp.client.listTools();
const names = tools.map((t) => t.name);
check(names.includes("terminal_launch") && names.includes("terminal_wait"), `discovered ${names.length} tools`);
let status = await mcp.call("rodtty_status");
check(status.connected === true, `connected to instance ${status.instance}`);
const unapproved = await mcp.call("terminal_list", {}, { allowError: true });
check(unapproved.error?.code === "authorization_required", "an unapproved client cannot read terminals");

console.log("owner approves for one workspace");
execFileSync(rodtty, ["automation", "approve", status.client.id, "--workspace", project], {
  env: { ...process.env, RODTTY_HOME: home },
  stdio: "ignore",
});
status = await mcp.call("rodtty_status");
check(status.client.status === "approved", "approved");
const denied = await mcp.call("terminal_create", { cwd: other }, { allowError: true });
check(denied.error?.code === "workspace_denied", "approval for one workspace does not expose another");

console.log("launch a service and an interactive app in separate terminals");
const service = await mcp.call("terminal_launch", {
  cwd: project,
  executable: "python3",
  args: ["-m", "http.server", "0", "--bind", "127.0.0.1"],
  request_id: `svc-${process.pid}`,
});
const again = await mcp.call("terminal_launch", {
  cwd: project,
  executable: "python3",
  args: ["-m", "http.server", "0", "--bind", "127.0.0.1"],
  request_id: `svc-${process.pid}`,
});
check(again.terminal === service.terminal && again.replayed === true, "a duplicate launch request creates one operation");
const served = await mcp.call("terminal_wait", { terminal: service.terminal, regex: "port (\\d+)", cursor: 0, timeout_ms: 20000 });
const port = Number(served.match.match(/\d+/)[0]);
check(port > 0, `service listening on ${port}`);

const app = await mcp.call("terminal_launch", {
  cwd: project,
  executable: "python3",
  args: [fixture],
  title: "tui-fixture",
  request_id: `tui-${process.pid}`,
});
await mcp.call("terminal_wait", { terminal: app.terminal, screen_contains: "PROMPT>", timeout_ms: 20000 });
let screen = await mcp.call("terminal_screen", { terminal: app.terminal });
check(screen.alternate_screen === true && screen.bracketed_paste === true, "the TUI fixture owns the alternate screen with bracketed paste");

console.log("type a multiline prompt, then press Enter separately");
const prompt = "first line ünïcødé\nsecond line ✓";
await mcp.call("terminal_send_text", { terminal: app.terminal, text: prompt, request_id: `text-${process.pid}` });
await sleep(300);
screen = await mcp.call("terminal_screen", { terminal: app.terminal });
check(!screen.lines.join("\n").includes("SUBMITTED"), "text alone does not submit");
const revision = screen.revision;
await mcp.call("terminal_send_keys", { terminal: app.terminal, keys: ["Enter"], request_id: `enter-${process.pid}` });
await mcp.call("terminal_send_keys", { terminal: app.terminal, keys: ["Enter"], request_id: `enter-${process.pid}` });
const submitted = await mcp.call("terminal_wait", { terminal: app.terminal, screen_contains: "SUBMITTED 1", timeout_ms: 10000 });
check(submitted.revision > revision, "Enter submitted the prompt once (retry with the same request_id is a no-op)");
screen = await mcp.call("terminal_screen", { terminal: app.terminal });
const text = screen.lines.join("\n");
check(text.includes("first line ünïcødé") && text.includes("second line ✓"), "the app received both lines and the Unicode intact");
check(!text.includes("SUBMITTED 2"), "exactly one submission");

console.log("input stays on the named PTY");
const other_screen = await mcp.call("terminal_screen", { terminal: service.terminal });
check(!other_screen.lines.join("\n").includes("first line"), "the service terminal received none of the prompt");

console.log("wait cancellation leaves the program running");
const controller = new AbortController();
const pending = mcp.call("terminal_wait", { terminal: app.terminal, contains: "never-printed", timeout_ms: 60000 }, { signal: controller.signal }).catch((e) => e);
await sleep(500);
controller.abort();
await pending;
await sleep(300);
const list = await mcp.call("terminal_list");
const row = list.terminals.find((t) => t.id === app.terminal);
check(row && row.state === "busy", "the TUI is still running after its wait was cancelled");

console.log("capture the terminal");
const shot = await mcp.call("capture_screenshot", { target: app.terminal, request_id: `shot-${process.pid}` });
check(existsSync(shot.path) && shot.width > 0, `terminal screenshot ${shot.width}x${shot.height} at ${shot.path}`);

console.log("quit the app and close both terminals");
await mcp.call("terminal_send_keys", { terminal: app.terminal, keys: ["q"] });
const done = await mcp.call("job_wait", { job: app.job, timeout_ms: 15000 });
check(done.state === "completed" && done.result.exit_status === 0, `launch job completed with exit ${done.result?.exit_status}`);
await mcp.call("terminal_interrupt", { terminal: service.terminal });
await mcp.call("terminal_wait", { terminal: service.terminal, idle: true, timeout_ms: 10000 });
for (const t of [app.terminal, service.terminal]) {
  const closed = await mcp.call("terminal_close", { terminal: t, request_id: `close-${t}` });
  check(closed.closed, `closed ${t}`);
}
const stale = await mcp.call("terminal_screen", { terminal: app.terminal }, { allowError: true });
check(stale.error?.code === "closed_target", "a closed terminal ID reports closed_target");

console.log("revocation stops access");
execFileSync(rodtty, ["automation", "revoke", status.client.id], { env: { ...process.env, RODTTY_HOME: home }, stdio: "ignore" });
const revoked = await mcp.call("terminal_list", {}, { allowError: true });
check(revoked.error?.code === "revoked", "a revoked client cannot read state");
await mcp.close();
console.log("terminal-check passed");

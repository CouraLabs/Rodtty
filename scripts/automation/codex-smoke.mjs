// Installed Codex smoke check: launch the Codex TUI in a Rodtty PTY through
// MCP, read its first screen, submit a two-line prompt (text, then Enter as
// a separate key) and observe the reply. Codex runs read-only with no
// approvals in an empty temporary folder, and the prompt asks for one word.
// Uses the owner's existing Codex login; prints screens, never credentials.
//   RODTTY=… RODTTY_HOME=… node codex-smoke.mjs
import { execFileSync } from "node:child_process";
import { mkdtempSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, check, sleep } from "./client.mjs";

const dir = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-codex-")));
execFileSync("git", ["-C", dir, "init", "-q"]);
const mcp = await connect({ rodtty: process.env.RODTTY, home: process.env.RODTTY_HOME, name: `codex-smoke-${process.pid}` });
const status = await mcp.call("rodtty_status");
execFileSync(process.env.RODTTY, ["automation", "approve", status.client.id, "--workspace", dir], { env: process.env, stdio: "ignore" });
const screen = async (t) => (await mcp.call("terminal_screen", { terminal: t })).lines.join("\n");

const codex = await mcp.call("terminal_launch", {
  cwd: dir,
  executable: "codex",
  args: ["--sandbox", "read-only", "--ask-for-approval", "never"],
  request_id: `codex-${process.pid}`,
});
// Codex may first ask whether to trust the folder; accept for this temp folder.
let first = "";
for (let i = 0; i < 60; i++) {
  await sleep(500);
  first = await screen(codex.terminal);
  if (/trust/i.test(first) && /folder|directory/i.test(first)) {
    await mcp.call("terminal_send_keys", { terminal: codex.terminal, keys: ["Enter"] });
    await sleep(1500);
    continue;
  }
  if (/\? for shortcuts/i.test(first)) break;
}
await sleep(4000); // let Codex finish starting before typing
console.log("--- initial screen ---\n" + first.split("\n").filter((l) => l.trim()).slice(-12).join("\n"));
check(first.trim().length > 0, "read Codex's initial TUI from its PTY");
await mcp.call("terminal_send_text", { terminal: codex.terminal, text: "This is an automated smoke test.\nReply with exactly the word RODTTY_OK and nothing else; do not run commands." });
// Codex treats an Enter that closely follows a paste as part of it.
await sleep(3000);
await mcp.call("terminal_send_keys", { terminal: codex.terminal, keys: ["Enter"], request_id: `codex-enter-${process.pid}` });
const reply = await mcp.call("terminal_wait", { terminal: codex.terminal, screen_contains: "• RODTTY_OK", timeout_ms: 180000 }, { allowError: true, timeout: 200000 });
const last = await screen(codex.terminal);
console.log("--- after Enter ---\n" + last.split("\n").filter((l) => l.trim()).slice(-14).join("\n"));
console.log(JSON.stringify(reply.error ?? reply.matched)); check(!reply.error, "Codex answered the submitted prompt");
await mcp.call("terminal_send_keys", { terminal: codex.terminal, keys: ["Ctrl+C"] });
await sleep(500);
await mcp.call("terminal_send_keys", { terminal: codex.terminal, keys: ["Ctrl+C"] });
await mcp.call("terminal_wait", { terminal: codex.terminal, idle: true, timeout_ms: 20000 }, { allowError: true });
await mcp.call("terminal_close", { terminal: codex.terminal }, { allowError: true });
execFileSync(process.env.RODTTY, ["automation", "forget", status.client.id], { env: process.env, stdio: "ignore" });
await mcp.close();
console.log("codex-smoke passed");

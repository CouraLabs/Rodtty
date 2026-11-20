// Installed Codex App Server smoke: start a structured thread in an empty
// temporary folder and run one turn that asks for a single word. Uses the
// owner's Codex configuration and login; prints no credentials.
//   RODTTY=… RODTTY_HOME=… node codex-appserver-smoke.mjs
import { execFileSync } from "node:child_process";
import { mkdtempSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, check } from "./client.mjs";

const dir = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-cdx-real-")));
execFileSync("git", ["-C", dir, "init", "-q"]);
const mcp = await connect({ rodtty: process.env.RODTTY, home: process.env.RODTTY_HOME, name: `cdx-real-${process.pid}` });
const status = await mcp.call("rodtty_status");
if (!status.features.includes("codex")) {
  console.log("skipped: this machine has no Codex App Server; the terminal tools still run Codex as a TUI");
  process.exit(0);
}
execFileSync(process.env.RODTTY, ["automation", "approve", status.client.id, "--workspace", dir], { env: process.env, stdio: "ignore" });
const started = await mcp.call("codex_thread_start", { cwd: dir });
console.log(`thread ${started.thread} (codex ${started.codex_thread_id}, ${started.server})`);
const turn = await mcp.call("codex_turn_start", { thread: started.thread, prompt: "Automated smoke test: reply with exactly RODTTY_OK and do not run any commands." });
const job = await mcp.call("job_wait", { job: turn.job, timeout_ms: 180000 }, { timeout: 200000 });
console.log(JSON.stringify({ state: job.state, status: job.result?.status, message: job.result?.message, items: job.result?.items }));
check(job.state === "completed" && /RODTTY_OK/.test(job.result.message), "the installed App Server completed a turn with the expected reply");
execFileSync(process.env.RODTTY, ["automation", "forget", status.client.id], { env: process.env, stdio: "ignore" });
await mcp.close();
console.log("codex-appserver-smoke passed");

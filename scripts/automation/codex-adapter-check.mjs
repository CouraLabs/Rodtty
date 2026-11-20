// The structured Codex adapter through MCP against the App Server protocol
// fixture. Rodtty must run with
//   RODTTY_CODEX_APP_SERVER="python3 scripts/automation/fixtures/fake-codex-app-server.py"
// and --control-stdin, whose stdin is the FIFO at $RODTTY_CONTROL; the owner's
// approval is given through that developer channel, never by the client.
//   RODTTY=… RODTTY_HOME=… RODTTY_CONTROL=/tmp/fifo node codex-adapter-check.mjs
import { execFileSync } from "node:child_process";
import { appendFileSync, mkdtempSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, check, sleep } from "./client.mjs";

const dir = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-cdx-")));
const mcp = await connect({ rodtty: process.env.RODTTY, home: process.env.RODTTY_HOME, name: `cdx-${process.pid}` });
const status = await mcp.call("rodtty_status");
check(status.features.includes("codex"), "the codex capability is advertised");
execFileSync(process.env.RODTTY, ["automation", "approve", status.client.id, "--workspace", dir], { env: process.env, stdio: "ignore" });

const { thread } = await mcp.call("codex_thread_start", { cwd: dir, request_id: "cdx-thread" });
const first = await mcp.call("codex_turn_start", { thread, prompt: "hello", request_id: "cdx-turn-1" });
const retry = await mcp.call("codex_turn_start", { thread, prompt: "hello", request_id: "cdx-turn-1" });
check(retry.turn === first.turn && retry.replayed, "retrying a turn with the same request_id submits once");
let job = await mcp.call("job_wait", { job: first.job, timeout_ms: 10000 });
check(job.state === "completed" && job.result.message === "Echo: hello", "progress and completion come from protocol state");

const self = await mcp.call("app_action", { command: "codex-approve" }, { allowError: true });
check(self.error?.code === "scope_denied", "a client cannot answer approvals itself");
const approval = await mcp.call("codex_turn_start", { thread, prompt: "APPROVE please", request_id: "cdx-turn-2" });
for (let i = 0; i < 50; i++) {
  job = await mcp.call("job_status", { job: approval.job });
  if (job.state === "waiting_for_owner") break;
  await sleep(100);
}
check(job.state === "waiting_for_owner" && job.result.approval.summary === "Run `touch approved.txt`", "an approval request shows as waiting for the owner");
appendFileSync(process.env.RODTTY_CONTROL, "action codex-approve\n");
job = await mcp.call("job_wait", { job: approval.job, timeout_ms: 10000 });
check(job.state === "completed" && job.result.message === "Approved: accept", "the turn resumes after the owner's decision");

const slow = await mcp.call("codex_turn_start", { thread, prompt: "SLOW", request_id: "cdx-turn-3" });
await mcp.call("codex_turn_interrupt", { thread, turn: slow.turn });
job = await mcp.call("job_wait", { job: slow.job, timeout_ms: 10000 });
check(job.state === "cancelled", "interrupt ends the turn as interrupted");
const fail = await mcp.call("codex_turn_start", { thread, prompt: "FAIL", request_id: "cdx-turn-4" });
job = await mcp.call("job_wait", { job: fail.job, timeout_ms: 10000 });
check(job.state === "failed" && job.error.message === "fixture failure", "a failed turn reports its error");
execFileSync(process.env.RODTTY, ["automation", "forget", status.client.id], { env: process.env, stdio: "ignore" });
await mcp.close();
console.log("codex-adapter-check passed");

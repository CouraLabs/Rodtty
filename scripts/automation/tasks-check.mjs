// Project tasks: identical names in two projects run their own commands and
// folders; a running task is focused rather than started twice; an invalid
// task leaves the others usable. Task files are written here as the owner
// would in Rodtty (Tasks: Edit Project Tasks).
//   RODTTY=… RODTTY_HOME=… node tasks-check.mjs
import { execFileSync } from "node:child_process";
import { mkdirSync, mkdtempSync, realpathSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, check } from "./client.mjs";

const home = process.env.RODTTY_HOME;
const a = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-task-a-")));
const b = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-task-b-")));
mkdirSync(join(a, "sub"));
const mcp = await connect({ rodtty: process.env.RODTTY, home, name: `tasks-${process.pid}` });
const status = await mcp.call("rodtty_status");
execFileSync(process.env.RODTTY, ["automation", "approve", status.client.id, "--workspace", a, "--workspace", b], { env: process.env, stdio: "ignore" });
const { workspaces } = await mcp.call("workspace_list");
const wa = workspaces.find((w) => w.root === a).id;
const wb = workspaces.find((w) => w.root === b).id;
mkdirSync(join(home, "tasks"), { recursive: true });
writeFileSync(join(home, "tasks", `${wa}.json`), JSON.stringify({ tasks: [
  { name: "Test", command: "printf 'A_TEST in %s\\n' \"$PWD\"", cwd: "sub" },
  { name: "Watch", command: "printf 'WATCHING\\n'; sleep 30" },
  { name: "Broken", command: "true", cwd: "missing-folder" },
] }));
writeFileSync(join(home, "tasks", `${wb}.json`), JSON.stringify({ tasks: [{ name: "Test", command: "printf 'B_TEST in %s\\n' \"$PWD\"" }] }));
const listed = await mcp.call("tasks_list", { workspace: wa });
check(listed.tasks.length === 3, "the owner's tasks for project A are listed");
const runA = await mcp.call("task_run", { workspace: wa, name: "Test", request_id: "ta" });
const runB = await mcp.call("task_run", { workspace: wb, name: "Test", request_id: "tb" });
const outA = await mcp.call("terminal_wait", { terminal: runA.terminal, contains: "A_TEST in /", cursor: 0, timeout_ms: 15000 });
const outB = await mcp.call("terminal_wait", { terminal: runB.terminal, contains: "B_TEST in /", cursor: 0, timeout_ms: 15000 });
const screenA = (await mcp.call("terminal_read", { terminal: runA.terminal, cursor: 0 })).text.replace(/\n/g, "");
const screenB = (await mcp.call("terminal_read", { terminal: runB.terminal, cursor: 0 })).text.replace(/\n/g, "");
check(screenA.includes(`A_TEST in ${join(a, "sub")}`) && screenB.includes(`B_TEST in ${b}`), "same-named tasks run their own command in their own folder");
const watch = await mcp.call("task_run", { workspace: wa, name: "Watch", request_id: "tw1" });
await mcp.call("terminal_wait", { terminal: watch.terminal, regex: "(?m)^WATCHING", cursor: 0, timeout_ms: 15000 });
const again = await mcp.call("task_run", { workspace: wa, name: "Watch", request_id: "tw2" });
check(again.terminal === watch.terminal && again.already_running, "running a watcher again returns the running instance");
const broken = await mcp.call("task_run", { workspace: wa, name: "Broken" }, { allowError: true });
check(broken.error?.code === "conflict" && /does not exist/.test(broken.error.message), "an invalid task folder is reported");
const still = await mcp.call("task_run", { workspace: wb, name: "Test", request_id: "tb2" });
check(!still.error, "other tasks stay usable");
for (const t of [runA.terminal, runB.terminal, watch.terminal, still.terminal]) {
  await mcp.call("terminal_interrupt", { terminal: t });
}
for (const t of new Set([runA.terminal, runB.terminal, watch.terminal, still.terminal])) {
  await mcp.call("terminal_wait", { terminal: t, idle: true, timeout_ms: 10000 }, { allowError: true });
  await mcp.call("terminal_close", { terminal: t }, { allowError: true });
}
execFileSync(process.env.RODTTY, ["automation", "forget", status.client.id], { env: process.env, stdio: "ignore" });
await mcp.close();
void outA; void outB;
console.log("tasks-check passed");

// Local services: two HTTP servers in Rodtty terminals and one outside
// Rodtty. Only the two are attributed to terminals; a raw TCP listener is
// listed without assuming HTTP.
//   RODTTY=… RODTTY_HOME=… node services-check.mjs
import { execFileSync, spawn } from "node:child_process";
import { mkdtempSync, realpathSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, check } from "./client.mjs";

const dir = realpathSync(mkdtempSync(join(tmpdir(), "rodtty-svc-")));
const outside = spawn("python3", ["-u", "-m", "http.server", "0", "--bind", "127.0.0.1"], { cwd: dir, stdio: ["ignore", "pipe", "ignore"] });
const outsidePort = await new Promise((r) => outside.stdout.on("data", (d) => { const m = String(d).match(/port (\d+)/); if (m) r(Number(m[1])); }));
const raw = spawn("python3", ["-c", "import socket,time;s=socket.socket();s.bind(('127.0.0.1',0));s.listen();print(s.getsockname()[1],flush=True);time.sleep(60)"], { stdio: ["ignore", "pipe", "ignore"] });
const rawPort = await new Promise((r) => raw.stdout.on("data", (d) => r(Number(String(d).trim()))));
const mcp = await connect({ rodtty: process.env.RODTTY, home: process.env.RODTTY_HOME, name: `svc-${process.pid}` });
const status = await mcp.call("rodtty_status");
execFileSync(process.env.RODTTY, ["automation", "approve", status.client.id, "--workspace", dir], { env: process.env, stdio: "ignore" });
const ports = {};
for (const name of ["a", "b"]) {
  const t = await mcp.call("terminal_launch", { cwd: dir, executable: "python3", args: ["-m", "http.server", "0", "--bind", "127.0.0.1"] });
  const up = await mcp.call("terminal_wait", { terminal: t.terminal, regex: "port (\\d+)", cursor: 0, timeout_ms: 15000 });
  ports[name] = { terminal: t.terminal, port: Number(up.match.match(/\d+/)[0]) };
}
const { listeners } = await mcp.call("services_list");
const row = (port) => listeners.find((l) => l.port === port);
check(row(ports.a.port)?.owner === ports.a.terminal && row(ports.b.port)?.owner === ports.b.terminal, "both managed servers are attributed to their own terminals");
check(row(outsidePort)?.owner === "external", "the server started outside Rodtty is external");
check(row(rawPort) && row(rawPort).owner === "external", `a raw TCP listener (port ${rawPort}) is listed too`);
for (const p of Object.values(ports)) {
  await mcp.call("terminal_interrupt", { terminal: p.terminal });
  await mcp.call("terminal_wait", { terminal: p.terminal, idle: true, timeout_ms: 10000 });
  await mcp.call("terminal_close", { terminal: p.terminal });
}
outside.kill();
raw.kill();
execFileSync(process.env.RODTTY, ["automation", "forget", status.client.id], { env: process.env, stdio: "ignore" });
await mcp.close();
console.log("services-check passed");

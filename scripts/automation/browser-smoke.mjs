// Quick browser smoke: open a background target on a URL and screenshot it.
//   RODTTY=… RODTTY_HOME=… node browser-smoke.mjs <url>
import { execFileSync } from "node:child_process";
import { connect } from "./client.mjs";

const rodtty = process.env.RODTTY;
const home = process.env.RODTTY_HOME;
const mcp = await connect({ rodtty, home, name: `smoke-${process.pid}` });
const status = await mcp.call("rodtty_status");
console.log("features", status.features);
execFileSync(rodtty, ["automation", "approve", status.client.id, "--workspace", "all"], { env: { ...process.env, RODTTY_HOME: home }, stdio: "ignore" });
const opened = await mcp.call("browser_open", { url: process.argv[2] ?? "https://example.com", width: 1000, height: 700 });
console.log(JSON.stringify(opened, null, 1));
const shot = await mcp.call("capture_screenshot", { target: opened.browser });
console.log(shot.path, shot.width, shot.height);
await mcp.close();

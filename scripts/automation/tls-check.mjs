// A self-signed HTTPS fixture: the embedded browser must refuse it and
// report a certificate error, never bypass validation.
//   RODTTY=… RODTTY_HOME=… node tls-check.mjs
import { execFileSync } from "node:child_process";
import { createServer } from "node:https";
import { mkdtempSync, readFileSync } from "node:fs";
import { tmpdir } from "node:os";
import { join } from "node:path";
import { connect, check } from "./client.mjs";

const dir = mkdtempSync(join(tmpdir(), "rodtty-tls-"));
execFileSync("openssl", ["req", "-x509", "-newkey", "rsa:2048", "-nodes", "-subj", "/CN=localhost", "-days", "1",
  "-keyout", join(dir, "key.pem"), "-out", join(dir, "cert.pem")], { stdio: "ignore" });
const server = createServer({ key: readFileSync(join(dir, "key.pem")), cert: readFileSync(join(dir, "cert.pem")) }, (_, res) => res.end("<h1>should never render</h1>"));
await new Promise((r) => server.listen(0, "127.0.0.1", r));
const port = server.address().port;

const mcp = await connect({ rodtty: process.env.RODTTY, home: process.env.RODTTY_HOME, name: `tls-${process.pid}` });
const status = await mcp.call("rodtty_status");
execFileSync(process.env.RODTTY, ["automation", "approve", status.client.id, "--workspace", "all"], { env: process.env, stdio: "ignore" });
const { browser } = await mcp.call("browser_open", { width: 800, height: 600 });
await mcp.call("browser_navigate", { browser, url: `https://localhost:${port}/` }, { allowError: true });
const page = await mcp.call("browser_status", { browser });
console.log(JSON.stringify({ url: page.url, error: page.error }));
check(/certificate|CERT/i.test(page.error ?? ""), "the self-signed certificate was rejected with a certificate error");
const snap = await mcp.call("browser_snapshot", { browser });
check(!snap.text.includes("should never render"), "the untrusted page content never loaded");
await mcp.call("browser_close", { browser });
execFileSync(process.env.RODTTY, ["automation", "forget", status.client.id], { env: process.env, stdio: "ignore" });
await mcp.close();
server.close();
console.log("tls-check passed");

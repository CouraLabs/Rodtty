// rodtty open: one tab per file, unsaved edits kept, concurrent opens never start extra owner instances.
// Setup: mkdir -p "/tmp/open-proj/a dir" && printf "line1\nline2\n" > "/tmp/open-proj/a dir/fïle one.txt"; then
// RODTTY_HOME=… rodtty open "/tmp/open-proj/a dir/fïle one.txt:2" (cold start), then run this with RODTTY and RODTTY_HOME.
import { connect, check, sleep } from "./client.mjs";
import { execFileSync, spawn } from "node:child_process";
const R = process.env.RODTTY, env = { ...process.env };
const mcp = await connect({ rodtty: R, home: process.env.RODTTY_HOME, name: "open-check" });
const s = await mcp.call("rodtty_status");
execFileSync(R, ["automation","approve",s.client.id,"--workspace","/tmp/open-proj"], { env });
let eds = (await mcp.call("editor_list")).editors.filter(e => e.path.endsWith("fïle one.txt"));
check(eds.length === 1, "the opened file has one editor tab");
const read = await mcp.call("editor_read", { editor: eds[0].id });
await mcp.call("editor_edit", { editor: eds[0].id, expected_revision: read.revision, find: "line1", replace: "EDITED" });
const runs = [0,1,2].map(() => new Promise(r => spawn(R, ["open", "/tmp/open-proj/a dir/fïle one.txt:1"], { env, stdio: "ignore" }).on("exit", r)));
const codes = await Promise.all(runs);
check(codes.every(c => c === 0), `three concurrent opens exited ${codes}`);
eds = (await mcp.call("editor_list")).editors.filter(e => e.path.endsWith("fïle one.txt"));
check(eds.length === 1 && eds[0].dirty, "still one tab, and its unsaved edit is kept");
const text = (await mcp.call("editor_read", { editor: eds[0].id })).text;
check(text.startsWith("EDITED"), "the buffer was not reloaded from disk");
execFileSync(R, ["automation","forget",s.client.id], { env });
await mcp.close();

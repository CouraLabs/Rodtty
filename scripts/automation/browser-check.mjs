// Browser automation through a real MCP client: a background CEF target
// navigates a redirect, fills a form found by accessible name with the
// virtual keyboard, submits with a virtual click, scrolls, and is recorded
// to video and screenshotted, while the owner's frontmost app and pointer
// stay where they were.
//   RODTTY=<Rodtty.app>/Contents/MacOS/rodtty RODTTY_HOME=… node browser-check.mjs
import { execFileSync } from "node:child_process";
import { existsSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";
import { connect, check, sleep } from "./client.mjs";

const rodtty = process.env.RODTTY;
const home = process.env.RODTTY_HOME;
const here = dirname(fileURLToPath(import.meta.url));
const desktop = () => JSON.parse(execFileSync("python3", [join(here, "fixtures", "desktop-state.py")], { encoding: "utf8" }));
const before = desktop();
console.log("owner desktop before:", JSON.stringify(before));

const mcp = await connect({ rodtty, home, name: `browser-check-${process.pid}` });
const status = await mcp.call("rodtty_status");
check(status.features.includes("browser") && status.features.includes("recording"), "browser and recording advertised");
execFileSync(rodtty, ["automation", "approve", status.client.id, "--workspace", here], { env: { ...process.env, RODTTY_HOME: home }, stdio: "ignore" });

console.log("start the web fixture in a terminal");
const web = await mcp.call("terminal_launch", { cwd: here, executable: "node", args: [join(here, "fixtures", "web.mjs"), "0"], request_id: `web-${process.pid}` });
const up = await mcp.call("terminal_wait", { terminal: web.terminal, regex: "listening on (\\d+)", cursor: 0, timeout_ms: 20000 });
const base = `localhost:${up.match.match(/\d+/)[0]}`;

console.log("open a background browser target and record it");
const opened = await mcp.call("browser_open", { width: 1000, height: 700, request_id: `open-${process.pid}` });
const browser = opened.browser;
await mcp.call("browser_navigate", { browser, url: base });
const rec = await mcp.call("recording_start", { browser, fps: 15, max_seconds: 60, request_id: `rec-${process.pid}` });
const recAgain = await mcp.call("recording_start", { browser, fps: 15, max_seconds: 60, request_id: `rec-${process.pid}` });
check(recAgain.recording === rec.recording, "a retried recording start returns the same recording");

const nav = await mcp.call("browser_navigate", { browser, url: `${base}/redirect` });
check(nav.url.endsWith("/form") && nav.title === "Contact form", `followed the redirect to ${nav.url}`);
const snap = await mcp.call("browser_snapshot", { browser });
const field = snap.elements.find((e) => e.role === "textbox" && e.name === "Your name");
const message = snap.elements.find((e) => e.name === "Message");
const send = snap.elements.find((e) => e.role === "button" && e.name === "Send");
check(field && message && send, "found the name field, message box and Send button by accessible name");

await mcp.call("browser_type", { browser, ref: field.ref, text: "Zoë Agent", delay_ms: 30 });
await mcp.call("browser_type", { browser, ref: message.ref, text: "Typed by the virtual keyboard: ação ✓", delay_ms: 10 });
let snap2 = await mcp.call("browser_snapshot", { browser });
check(snap2.elements.find((e) => e.role === "textbox" && e.name === "Your name")?.value === "Zoë Agent", "the page observed the typed name, accents intact");

await mcp.call("browser_scroll", { browser, delta_y: 600 });
await sleep(300);
const scrolled = await mcp.call("browser_snapshot", { browser, max_elements: 1 });
check(scrolled.scroll.y > 0, `virtual wheel scrolled the page to y=${scrolled.scroll.y}`);
await mcp.call("browser_scroll", { browser, delta_y: -600 });
await sleep(300);

const stale = await mcp.call("browser_click", { browser, ref: "e999-0" }, { allowError: true });
check(stale.error?.code === "stale", "an unknown element reference is rejected as stale");

const shot = await mcp.call("capture_screenshot", { target: browser, cursor: true });
check(existsSync(shot.path) && shot.width === 1000 && shot.height === 700, `browser screenshot ${shot.width}x${shot.height} from ${shot.source.renderer}`);

await mcp.call("browser_click", { browser, ref: send.ref });
const done = await mcp.call("browser_wait", { browser, text: "Thanks, Zoë Agent!", timeout_ms: 15000 });
console.log("wait:", JSON.stringify(done)); const after_submit = await mcp.call("browser_status", { browser }); check(after_submit.url.includes("/result"), "the virtual click submitted the form and the result page arrived");
const staleAfterNav = await mcp.call("browser_click", { browser, ref: send.ref }, { allowError: true });
check(staleAfterNav.error?.code === "stale", "references from the previous document are stale after navigation");
const resultShot = await mcp.call("capture_screenshot", { target: browser });
await sleep(1500);

const blocked = await mcp.call("browser_navigate", { browser, url: "file:///etc/hosts" }, { allowError: true });
check(blocked.error?.code === "unsupported", "a file: address is rejected");

const stopped = await mcp.call("recording_stop", { recording: rec.recording, request_id: `stop-${process.pid}` });
check(stopped.state === "completed" && existsSync(stopped.path), `video finalized: ${stopped.frames} frames, ${stopped.duration_ms} ms, ${stopped.bytes} bytes`);
const probe = JSON.parse(execFileSync("ffprobe", ["-v", "error", "-count_frames", "-show_entries", "stream=codec_name,width,height,nb_read_frames:format=duration", "-of", "json", stopped.path], { encoding: "utf8" }));
const stream = probe.streams[0];
check(stream.codec_name === "mjpeg" && Number(stream.nb_read_frames) === stopped.frames, `an independent player decodes ${stream.nb_read_frames} ${stream.codec_name} frames, ${probe.format.duration}s`);

const after = desktop();
console.log("owner desktop after:", JSON.stringify(after));
check(after.front === before.front, `the frontmost app stayed ${before.front}`);
check(after.pointer[0] === before.pointer[0] && after.pointer[1] === before.pointer[1], "the physical pointer did not move");

await mcp.call("browser_close", { browser, request_id: `close-${process.pid}` });
const closed = await mcp.call("browser_status", { browser }, { allowError: true });
check(closed.error?.code === "closed_target", "a closed browser reports closed_target");
await mcp.call("terminal_interrupt", { terminal: web.terminal });
await mcp.call("terminal_wait", { terminal: web.terminal, idle: true, timeout_ms: 10000 });
await mcp.call("terminal_close", { terminal: web.terminal });
await mcp.close();
console.log(JSON.stringify({ video: stopped.path, screenshot: shot.path, result: resultShot.path }));
console.log("browser-check passed");

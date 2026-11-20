// A deterministic local web fixture: a redirect, a labelled form, a result
// page, a tall scrollable section and a CSS animation. No dependencies.
//   node web.mjs [port]   (0 picks a free port; prints "listening on <port>")
import { createServer } from "node:http";

const page = (title, body) => `<!doctype html><html><head><meta charset="utf-8"><title>${title}</title>
<style>
 body { font: 16px -apple-system, system-ui, sans-serif; margin: 24px; background: #f6f7fb; color: #1d2330; }
 label { display: block; margin: 12px 0 4px; font-weight: 600; }
 input, textarea { font: inherit; padding: 6px 8px; width: 320px; border: 1px solid #99a; border-radius: 6px; }
 button { font: inherit; margin-top: 16px; padding: 8px 18px; border-radius: 6px; border: 0; background: #3157d5; color: white; }
 .spinner { width: 28px; height: 28px; border: 4px solid #c5cbe0; border-top-color: #3157d5; border-radius: 50%; animation: spin 0.8s linear infinite; display: inline-block; vertical-align: middle; }
 @keyframes spin { to { transform: rotate(360deg); } }
 .tall { height: 1400px; background: linear-gradient(#f6f7fb, #c9d3f5); margin-top: 24px; padding: 12px; }
</style></head><body>${body}</body></html>`;

const escape = (s) => s.replace(/[&<>"']/g, (c) => ({ "&": "&amp;", "<": "&lt;", ">": "&gt;", '"': "&quot;", "'": "&#39;" })[c]);

const server = createServer((req, res) => {
  const url = new URL(req.url, "http://localhost");
  const send = (status, html, headers = {}) => {
    res.writeHead(status, { "content-type": "text/html; charset=utf-8", ...headers });
    res.end(html);
  };
  if (url.pathname === "/") {
    return send(200, page("Fixture home", `<h1>Fixture home</h1><p><span class="spinner"></span> Working…</p><a href="/redirect">Open the form</a>`));
  }
  if (url.pathname === "/redirect") {
    res.writeHead(302, { location: "/form" });
    return res.end();
  }
  if (url.pathname === "/form") {
    return send(200, page("Contact form", `<h1>Contact form</h1><span class="spinner"></span>
<form method="get" action="/result">
  <label for="name">Your name</label><input id="name" name="name" autocomplete="off">
  <label for="message">Message</label><textarea id="message" name="message" rows="3"></textarea>
  <div><button type="submit">Send</button></div>
</form>
<div class="tall"><h2>Scroll target</h2><p>The bottom of the page.</p><a id="bottom" href="/">Back home</a></div>`));
  }
  if (url.pathname === "/result") {
    const name = escape(url.searchParams.get("name") ?? "");
    const message = escape(url.searchParams.get("message") ?? "");
    return send(200, page("Thanks", `<h1>Thanks, ${name}!</h1><p id="echo">Message: ${message}</p><span class="spinner"></span>`));
  }
  send(404, page("Not found", "<h1>Not found</h1>"));
});

server.listen(Number(process.argv[2] ?? 0), "127.0.0.1", () => {
  console.log(`listening on ${server.address().port}`);
});

//! Remote session handoff: a paired device's browser lists the host's
//! terminal sessions, attaches to one, reads recent output, types and
//! resizes it. The first release uses one transport, Tailscale: Rodtty
//! listens only on this machine's Tailscale address, so traffic is carried
//! inside the tailnet's WireGuard encryption and never on a LAN or public
//! interface. Access is off by default and ends with Quit.
//!
//! Pairing: the owner asks Rodtty for a one-time eight-character code that
//! expires after five minutes; the device posts it once and receives a
//! random device token (256 bits from the OS) that expires after 30 days.
//! Devices are listed in `<app root>/remote-devices.json` (mode 0600); the
//! owner disconnects and revokes them from Rodtty. Pairing codes, tokens
//! and terminal output never go to logs or diagnostics.
//!
//! Control policy: any paired device may view an attached session; only
//! the session's controller types and resizes. A device takes control
//! explicitly; the owner typing on the desktop takes it back for the
//! desktop, and the device must take control again to type.
use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use std::io::{BufRead, BufReader, Read, Write};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub const PORT: u16 = 7681;
const PAIRING_SECONDS: u64 = 300;
const DEVICE_DAYS: u64 = 30;
const MAX_REQUEST: usize = 64 * 1024;
const MAX_CONNECTIONS: usize = 16;
/// Output returned per read, at most.
pub const READ_BYTES: usize = 64 * 1024;
const LONG_POLL: Duration = Duration::from_secs(20);

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

/// `bytes` random bytes from the operating system, as hex.
pub fn secret(bytes: usize) -> Result<String> {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no OS randomness: {e}"))?;
    Ok(buf.iter().map(|b| format!("{b:02x}")).collect())
}

#[derive(Clone, Debug, Serialize, Deserialize, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
    pub token: String,
    pub paired_at: u64,
    pub expires_at: u64,
}

#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Devices {
    pub devices: Vec<Device>,
}

impl Devices {
    fn path(root: &Path) -> PathBuf {
        root.join("remote-devices.json")
    }
    pub fn load(root: &Path) -> Self {
        std::fs::read(Self::path(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }
    pub fn save(&self, root: &Path) -> Result<()> {
        std::fs::create_dir_all(root)?;
        let mut file = tempfile::NamedTempFile::new_in(root)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            file.as_file()
                .set_permissions(std::fs::Permissions::from_mode(0o600))?;
        }
        file.write_all(&serde_json::to_vec_pretty(self)?)?;
        file.persist(Self::path(root))?;
        Ok(())
    }
    /// The unexpired device presenting `token`, compared in constant time.
    pub fn authenticate(&self, token: &str) -> Option<&Device> {
        let now = now();
        self.devices
            .iter()
            .find(|d| d.expires_at > now && constant_eq(d.token.as_bytes(), token.as_bytes()))
    }
}

fn constant_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

/// One-time pairing codes and the devices they create.
#[derive(Default)]
pub struct Pairing {
    code: Option<(String, u64)>,
}

impl Pairing {
    /// A fresh one-time code, replacing any earlier one.
    pub fn new_code(&mut self) -> Result<String> {
        const ALPHABET: &[u8] = b"ABCDEFGHJKLMNPQRSTUVWXYZ23456789";
        let mut buf = [0u8; 8];
        getrandom::fill(&mut buf).map_err(|e| anyhow::anyhow!("no OS randomness: {e}"))?;
        let code: String = buf
            .iter()
            .map(|b| ALPHABET[usize::from(*b) % ALPHABET.len()] as char)
            .collect();
        self.code = Some((code.clone(), now() + PAIRING_SECONDS));
        Ok(code)
    }
    /// Spend `code` for a new device. A wrong guess also spends the code.
    pub fn redeem(&mut self, root: &Path, code: &str, name: &str) -> Result<Device, &'static str> {
        let Some((expected, expires)) = self.code.take() else {
            return Err("no pairing code is active");
        };
        if now() >= expires {
            return Err("the pairing code expired");
        }
        if !constant_eq(expected.as_bytes(), code.trim().to_uppercase().as_bytes()) {
            return Err("the pairing code is wrong; ask Rodtty for a new one");
        }
        let device = Device {
            id: format!("dev_{}", secret(6).map_err(|_| "no OS randomness")?),
            name: name.chars().filter(|c| !c.is_control()).take(60).collect(),
            token: secret(32).map_err(|_| "no OS randomness")?,
            paired_at: now(),
            expires_at: now() + DEVICE_DAYS * 86_400,
        };
        let mut devices = Devices::load(root);
        devices.devices.push(device.clone());
        devices
            .save(root)
            .map_err(|_| "could not save the device")?;
        Ok(device)
    }
}

/// What the host is asked, on its UI thread.
#[derive(Clone, Debug)]
pub enum Call {
    List,
    Read {
        session: usize,
        cursor: u64,
    },
    Control {
        session: usize,
        device: String,
    },
    Input {
        session: usize,
        device: String,
        text: String,
    },
    Resize {
        session: usize,
        device: String,
        rows: u16,
        cols: u16,
    },
    Pair {
        code: String,
        name: String,
    },
    Attached {
        device: String,
        session: Option<usize>,
    },
}

pub type Reply = std::result::Result<Value, (u16, String)>;
pub type Dispatch = Arc<dyn Fn(Call) -> Reply + Send + Sync>;

/// A running listener. Dropping or stopping it closes the port.
pub struct Server {
    pub addr: SocketAddr,
    stop: Arc<AtomicBool>,
}

impl Server {
    pub fn stop(&self) {
        self.stop.store(true, Ordering::Release);
        // Wake the accept loop.
        let _ = TcpStream::connect_timeout(&self.addr, Duration::from_millis(200));
    }
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop();
    }
}

/// This machine's Tailscale IPv4 address, from the Tailscale CLI.
pub fn tailscale_address() -> Result<IpAddr> {
    let candidates = [
        "tailscale",
        "/Applications/Tailscale.app/Contents/MacOS/Tailscale",
        "/usr/bin/tailscale",
        "/usr/local/bin/tailscale",
    ];
    for cli in candidates {
        if let Ok(out) = std::process::Command::new(cli)
            .args(["ip", "-4"])
            .stderr(std::process::Stdio::null())
            .output()
            && out.status.success()
            && let Some(ip) = String::from_utf8_lossy(&out.stdout)
                .lines()
                .find_map(|l| l.trim().parse::<IpAddr>().ok())
        {
            return Ok(ip);
        }
    }
    anyhow::bail!("Tailscale isn't running on this machine; remote access needs it")
}

/// Where to listen: the Tailscale address, or loopback when the developer
/// override `RODTTY_REMOTE_LOOPBACK=1` is set (local checks only).
pub fn bind_address() -> Result<SocketAddr> {
    if std::env::var_os("RODTTY_REMOTE_LOOPBACK").is_some_and(|v| v == "1") {
        return Ok(SocketAddr::from(([127, 0, 0, 1], 0)));
    }
    Ok(SocketAddr::new(tailscale_address()?, PORT))
}

pub fn start(addr: SocketAddr, dispatch: Dispatch) -> Result<Server> {
    let listener = TcpListener::bind(addr).with_context(|| format!("listen on {addr}"))?;
    let addr = listener.local_addr()?;
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    let open = Arc::new(AtomicUsize::new(0));
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            if flag.load(Ordering::Acquire) {
                break;
            }
            let Ok(stream) = stream else { continue };
            if open.load(Ordering::Acquire) >= MAX_CONNECTIONS {
                let _ = respond(&stream, 503, "application/json", br#"{"error":"busy"}"#);
                continue;
            }
            open.fetch_add(1, Ordering::AcqRel);
            let (dispatch, open, flag) = (dispatch.clone(), open.clone(), flag.clone());
            std::thread::spawn(move || {
                let _ = stream.set_read_timeout(Some(Duration::from_secs(10)));
                let _ = handle(&stream, &dispatch, &flag);
                open.fetch_sub(1, Ordering::AcqRel);
            });
        }
    });
    Ok(Server { addr, stop })
}

struct Request {
    method: String,
    path: String,
    query: String,
    token: Option<String>,
    body: Vec<u8>,
}

fn read_request(stream: &TcpStream) -> Result<Request> {
    let mut reader = BufReader::new(stream.take(MAX_REQUEST as u64));
    let mut line = String::new();
    reader.read_line(&mut line)?;
    let mut parts = line.split_whitespace();
    let method = parts.next().context("no method")?.to_owned();
    let target = parts.next().context("no target")?.to_owned();
    let (path, query) = target.split_once('?').unwrap_or((&target, ""));
    let (path, query) = (path.to_owned(), query.to_owned());
    let mut length = 0;
    let mut token = None;
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header)? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if let Some((name, value)) = header.split_once(':') {
            let value = value.trim();
            match name.to_ascii_lowercase().as_str() {
                "content-length" => length = value.parse().unwrap_or(0),
                "authorization" => {
                    token = value.strip_prefix("Bearer ").map(str::to_owned);
                }
                _ => {}
            }
        }
    }
    anyhow::ensure!(length <= MAX_REQUEST, "request too large");
    let mut body = vec![0; length];
    reader.read_exact(&mut body)?;
    Ok(Request {
        method,
        path,
        query,
        token,
        body,
    })
}

fn respond(mut stream: &TcpStream, status: u16, kind: &str, body: &[u8]) -> std::io::Result<()> {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        401 => "Unauthorized",
        403 => "Forbidden",
        404 => "Not Found",
        409 => "Conflict",
        503 => "Service Unavailable",
        _ => "Error",
    };
    write!(
        stream,
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nX-Content-Type-Options: nosniff\r\nContent-Security-Policy: default-src 'self'; script-src 'unsafe-inline'; style-src 'unsafe-inline'\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

fn query_value<'a>(query: &'a str, key: &str) -> Option<&'a str> {
    query
        .split('&')
        .filter_map(|p| p.split_once('='))
        .find(|(k, _)| *k == key)
        .map(|(_, v)| v)
}

fn handle(stream: &TcpStream, dispatch: &Dispatch, stop: &AtomicBool) -> Result<()> {
    let request = read_request(stream)?;
    let json_reply = |reply: Reply| -> std::io::Result<()> {
        match reply {
            Ok(value) => respond(
                stream,
                200,
                "application/json",
                value.to_string().as_bytes(),
            ),
            Err((status, message)) => respond(
                stream,
                status,
                "application/json",
                json!({ "error": message }).to_string().as_bytes(),
            ),
        }
    };
    if request.method == "GET" && (request.path == "/" || request.path == "/index.html") {
        respond(stream, 200, "text/html; charset=utf-8", PAGE.as_bytes())?;
        return Ok(());
    }
    let body: Value = serde_json::from_slice(&request.body).unwrap_or(Value::Null);
    if request.method == "POST" && request.path == "/api/pair" {
        let code = body["code"].as_str().unwrap_or("").to_owned();
        let name = body["name"].as_str().unwrap_or("Device").to_owned();
        json_reply(dispatch(Call::Pair { code, name }))?;
        return Ok(());
    }
    // Everything else needs a paired, unexpired, unrevoked device.
    let Some(device) = request.token.as_deref().and_then(|t| {
        dispatch(Call::Attached {
            device: t.to_owned(),
            session: None,
        })
        .ok()
        .and_then(|v| v["device"].as_str().map(str::to_owned))
    }) else {
        json_reply(Err((401, "pair this device first".into())))?;
        return Ok(());
    };
    let segments: Vec<&str> = request.path.trim_matches('/').split('/').collect();
    let reply = match (request.method.as_str(), segments.as_slice()) {
        ("GET", ["api", "sessions"]) => dispatch(Call::List),
        ("GET", ["api", "sessions", id, "output"]) => {
            let session: usize = id
                .parse()
                .map_err(|_| (400, "bad session".to_owned()))
                .unwrap_or(0);
            let cursor = query_value(&request.query, "cursor")
                .and_then(|c| c.parse().ok())
                .unwrap_or(0);
            let _ = dispatch(Call::Attached {
                device: request.token.clone().unwrap_or_default(),
                session: Some(session),
            });
            // Long poll: answer as soon as output past the cursor exists.
            let deadline = std::time::Instant::now() + LONG_POLL;
            loop {
                let reply = dispatch(Call::Read { session, cursor });
                let more = reply
                    .as_ref()
                    .is_ok_and(|v| v["next"].as_u64() == Some(cursor) && cursor != 0);
                if !more || std::time::Instant::now() >= deadline || stop.load(Ordering::Acquire) {
                    break reply;
                }
                std::thread::sleep(Duration::from_millis(100));
            }
        }
        ("POST", ["api", "sessions", id, action]) => {
            let session: usize = id.parse().unwrap_or(0);
            match *action {
                "control" => dispatch(Call::Control {
                    session,
                    device: device.clone(),
                }),
                "input" => dispatch(Call::Input {
                    session,
                    device: device.clone(),
                    text: body["text"].as_str().unwrap_or("").to_owned(),
                }),
                "resize" => dispatch(Call::Resize {
                    session,
                    device: device.clone(),
                    rows: body["rows"].as_u64().unwrap_or(24).clamp(2, 500) as u16,
                    cols: body["cols"].as_u64().unwrap_or(80).clamp(10, 1000) as u16,
                }),
                _ => Err((404, "unknown action".into())),
            }
        }
        _ => Err((404, "not found".into())),
    };
    json_reply(reply)?;
    Ok(())
}

/// Turn a dispatch that answers on another thread into a blocking one.
pub fn channel_dispatch() -> (Dispatch, mpsc::Receiver<(Call, mpsc::Sender<Reply>)>) {
    let (tx, rx) = mpsc::channel::<(Call, mpsc::Sender<Reply>)>();
    let tx = Mutex::new(tx);
    let dispatch: Dispatch = Arc::new(move |call| {
        let (reply_tx, reply_rx) = mpsc::channel();
        if tx.lock().map(|t| t.send((call, reply_tx))).is_err() {
            return Err((503, "Rodtty is stopping".into()));
        }
        reply_rx
            .recv_timeout(Duration::from_secs(15))
            .unwrap_or(Err((503, "Rodtty didn't answer".into())))
    });
    (dispatch, rx)
}

/// The browser client: pair, list sessions, attach, type and resize. A
/// plain page with no external resources, served from the host itself.
pub const PAGE: &str = r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Rodtty</title>
<style>
body{font:15px system-ui,sans-serif;margin:0;background:#111;color:#eee}
header{padding:10px;background:#222;display:flex;gap:8px;align-items:center;flex-wrap:wrap}
button,input,select{font:inherit;padding:6px 10px}
pre{margin:0;padding:8px;font:13px ui-monospace,monospace;white-space:pre-wrap;word-break:break-all;height:calc(100vh - 150px);overflow:auto}
#send{display:flex;gap:6px;padding:8px;background:#222}#text{flex:1}
</style></head><body>
<header><strong>Rodtty</strong><span id="status"></span>
<select id="sessions" aria-label="Session"></select>
<button id="attach">Attach</button><button id="control">Take control</button></header>
<div id="pair" hidden><p style="padding:10px">Enter the pairing code Rodtty shows (Remote: Pair Device).</p>
<p style="padding:0 10px"><input id="code" autocomplete="one-time-code" aria-label="Pairing code"> <button id="pairbtn">Pair</button></p></div>
<pre id="out" role="log" aria-live="polite"></pre>
<form id="send"><input id="text" aria-label="Type into the session" autocomplete="off" autocapitalize="off">
<button>Send</button><button type="button" id="enter">⏎</button><button type="button" id="ctrlc">Ctrl+C</button></form>
<script>
const $=id=>document.getElementById(id);let token=localStorage.getItem("rodtty-token"),session=null,cursor=0,gen=0;
const api=async(path,opts={})=>{const r=await fetch(path,{...opts,headers:{"Content-Type":"application/json",...(token?{Authorization:"Bearer "+token}:{})}});const j=await r.json().catch(()=>({}));if(r.status==401){localStorage.removeItem("rodtty-token");token=null;show();}if(!r.ok)throw new Error(j.error||r.status);return j};
const say=t=>$("status").textContent=t;
async function show(){$("pair").hidden=!!token;if(!token)return;try{const j=await api("/api/sessions");$("sessions").innerHTML=j.sessions.map(s=>`<option value="${s.id}">${s.title} — ${s.directory}</option>`).join("");say(j.sessions.length+" session(s)")}catch(e){say(e.message)}}
$("pairbtn").onclick=async()=>{try{const j=await api("/api/pair",{method:"POST",body:JSON.stringify({code:$("code").value,name:navigator.userAgent.slice(0,60)})});token=j.token;localStorage.setItem("rodtty-token",token);show()}catch(e){say(e.message)}};
const strip=s=>s.replace(/\x1b\[[0-9;?]*[ -\/]*[@-~]|\x1b\][^\x07\x1b]*(\x07|\x1b\\)|\x1b[()][0-9A-Za-z]|\x1b./g,"").replace(/\r\n/g,"\n").replace(/\r/g,"");
async function poll(g){while(g==gen){try{const j=await api(`/api/sessions/${session}/output?cursor=${cursor}`);if(j.ended){say("The session ended");return}if(j.truncated)$("out").textContent+="\n[earlier output is no longer retained]\n";$("out").textContent=( $("out").textContent+strip(atob(j.data)) ).slice(-200000);cursor=j.next;$("out").scrollTop=1e9}catch(e){say(e.message+" — reconnecting");await new Promise(r=>setTimeout(r,2000))}}}
$("attach").onclick=()=>{session=$("sessions").value;cursor=0;$("out").textContent="";gen++;poll(gen);resize()};
$("control").onclick=async()=>{try{await api(`/api/sessions/${session}/control`,{method:"POST",body:"{}"});say("You have control")}catch(e){say(e.message)}};
const input=async t=>{try{await api(`/api/sessions/${session}/input`,{method:"POST",body:JSON.stringify({text:t})})}catch(e){say(e.message)}};
$("send").onsubmit=e=>{e.preventDefault();input($("text").value);$("text").value=""};
$("enter").onclick=()=>input("\r");$("ctrlc").onclick=()=>input("\x03");
async function resize(){if(!session)return;const cols=Math.max(20,Math.floor($("out").clientWidth/7.8)),rows=Math.max(5,Math.floor($("out").clientHeight/16));try{await api(`/api/sessions/${session}/resize`,{method:"POST",body:JSON.stringify({rows,cols})})}catch(e){}}
addEventListener("resize",resize);show();
</script></body></html>"#;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pairing_codes_are_one_time_and_devices_expire_or_revoke() {
        let root = crate::test_support::tempdir().unwrap();
        let mut pairing = Pairing::default();
        let code = pairing.new_code().unwrap();
        assert_eq!(code.len(), 8);
        assert!(pairing.redeem(root.path(), "WRONG000", "phone").is_err());
        assert!(
            pairing.redeem(root.path(), &code, "phone").is_err(),
            "a wrong guess spent the code"
        );
        let code = pairing.new_code().unwrap();
        let device = pairing
            .redeem(root.path(), &code.to_lowercase(), "phone")
            .unwrap();
        assert_eq!(device.token.len(), 64);
        assert!(
            pairing.redeem(root.path(), &code, "again").is_err(),
            "codes are one-time"
        );
        let mut devices = Devices::load(root.path());
        assert!(devices.authenticate(&device.token).is_some());
        assert!(devices.authenticate("nope").is_none());
        devices.devices[0].expires_at = 1;
        assert!(devices.authenticate(&device.token).is_none(), "expired");
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(root.path().join("remote-devices.json"))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
    }
}

//! The owner-scoped local channel between `rodtty mcp` and the desktop instance.
//!
//! Unix uses a socket at `<app root>/automation.sock` with mode 0600 inside the
//! owner's app root. Windows uses a loopback TCP port whose number and a random
//! secret are written to `<app root>/automation.endpoint` in the owner's
//! profile; a connection must present the secret first. Neither listens on a
//! network interface.
//!
//! Messages are JSON, one per line. Requests carry an `id`; each reply names
//! the request it answers, so replies to concurrent requests may arrive in
//! any order.
use super::{ErrorCode, ToolError, ToolResult};
use serde_json::{Value, json};
use std::{
    collections::HashMap,
    io::{BufRead, BufReader, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering},
        mpsc,
    },
    time::Duration,
};

pub const PROTOCOL: u64 = 1;
/// The largest message either side accepts.
pub const MAX_MESSAGE: usize = 4 << 20;
pub const MAX_CONNECTIONS: usize = 16;
pub const MAX_IN_FLIGHT: usize = 16;

#[cfg(unix)]
type Stream = std::os::unix::net::UnixStream;
#[cfg(not(unix))]
type Stream = std::net::TcpStream;

pub fn socket_path(root: &Path) -> PathBuf {
    root.join("automation.sock")
}

pub fn endpoint_path(root: &Path) -> PathBuf {
    root.join("automation.endpoint")
}

/// What the desktop instance does with requests.
pub trait Handler: Send + Sync + 'static {
    /// Identify a connecting client by name; returns its grant summary.
    fn hello(&self, client_name: &str) -> ToolResult;
    fn call(&self, client: &str, tool: &str, args: Value, cancel: Arc<AtomicBool>) -> ToolResult;
    /// The client's connection closed: cancel its queued input.
    fn disconnected(&self, client: &str);
    /// `rodtty open`: open a file for the owner. Needs no client grant: it
    /// can only open an existing file in the owner's own editor.
    fn open(&self, _request: Value) -> ToolResult {
        Err(ToolError::new(
            ErrorCode::Unavailable,
            "opening files is not supported",
        ))
    }
}

/// Keeps the listener running; dropping it stops accepting and removes the endpoint.
pub struct Server {
    path: PathBuf,
    stop: Arc<AtomicBool>,
}

impl Drop for Server {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        let _ = std::fs::remove_file(&self.path);
    }
}

fn read_line(reader: &mut impl BufRead, line: &mut Vec<u8>) -> std::io::Result<bool> {
    line.clear();
    let read = <&mut _ as std::io::Read>::take(&mut *reader, MAX_MESSAGE as u64 + 1)
        .read_until(b'\n', line)?;
    if read == 0 {
        return Ok(false);
    }
    if line.len() > MAX_MESSAGE {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "message too large",
        ));
    }
    Ok(true)
}

fn write_message(writer: &Mutex<Stream>, message: &Value) -> std::io::Result<()> {
    let mut bytes = serde_json::to_vec(message)?;
    bytes.push(b'\n');
    let mut writer = writer.lock().unwrap_or_else(|e| e.into_inner());
    writer.write_all(&bytes)?;
    writer.flush()
}

/// Start listening for automation clients.
pub fn serve(root: &Path, handler: Arc<dyn Handler>) -> std::io::Result<Server> {
    std::fs::create_dir_all(root)?;
    let stop = Arc::new(AtomicBool::new(false));
    #[cfg(unix)]
    let (listener, path) = {
        use std::os::unix::fs::PermissionsExt;
        let path = socket_path(root);
        // The instance lock guarantees no other owner instance; a socket
        // file left behind is from a host that stopped.
        let _ = std::fs::remove_file(&path);
        let listener = std::os::unix::net::UnixListener::bind(&path)?;
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o600))?;
        (listener, path)
    };
    #[cfg(not(unix))]
    let (listener, path, secret) = {
        let listener = std::net::TcpListener::bind(("127.0.0.1", 0))?;
        let secret = format!("{}{}", super::token(), super::token());
        let path = endpoint_path(root);
        std::fs::write(
            &path,
            serde_json::to_vec(&json!({"port": listener.local_addr()?.port(), "secret": secret}))?,
        )?;
        (listener, path, secret)
    };
    let connections = Arc::new(AtomicUsize::new(0));
    let accept_stop = stop.clone();
    std::thread::Builder::new()
        .name("rodtty-automation".into())
        .spawn(move || {
            for stream in listener.incoming() {
                if accept_stop.load(Ordering::Relaxed) {
                    break;
                }
                let Ok(stream) = stream else { continue };
                if connections.load(Ordering::Relaxed) >= MAX_CONNECTIONS {
                    continue;
                }
                connections.fetch_add(1, Ordering::Relaxed);
                let handler = handler.clone();
                let connections = connections.clone();
                #[cfg(not(unix))]
                let secret = secret.clone();
                std::thread::spawn(move || {
                    #[cfg(not(unix))]
                    let _ = connection(stream, handler, Some(&secret));
                    #[cfg(unix)]
                    let _ = connection(stream, handler, None);
                    connections.fetch_sub(1, Ordering::Relaxed);
                });
            }
        })?;
    Ok(Server { path, stop })
}

fn connection(
    stream: Stream,
    handler: Arc<dyn Handler>,
    secret: Option<&str>,
) -> std::io::Result<()> {
    let writer = Arc::new(Mutex::new(stream.try_clone()?));
    let mut reader = BufReader::new(stream);
    let mut line = Vec::new();
    if let Some(secret) = secret {
        if !read_line(&mut reader, &mut line)? || line.trim_ascii() != secret.as_bytes() {
            return Ok(());
        }
    }
    let mut client: Option<String> = None;
    let in_flight: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>> = Default::default();
    while read_line(&mut reader, &mut line)? {
        let Ok(message) = serde_json::from_slice::<Value>(&line) else {
            write_message(
                &writer,
                &json!({"id": null, "error": ToolError::invalid("malformed message")}),
            )?;
            continue;
        };
        let id = message.get("id").and_then(Value::as_u64).unwrap_or(0);
        match message.get("op").and_then(Value::as_str) {
            Some("hello") => {
                let name = message
                    .get("client")
                    .and_then(Value::as_str)
                    .unwrap_or("unnamed");
                let reply = handler.hello(name);
                if let Ok(value) = &reply {
                    client = value
                        .get("client")
                        .and_then(Value::as_str)
                        .map(str::to_owned);
                }
                write_message(&writer, &reply_message(id, reply))?;
            }
            Some("open") => {
                let reply = handler.open(message.get("request").cloned().unwrap_or(Value::Null));
                write_message(&writer, &reply_message(id, reply))?;
            }
            Some("cancel") => {
                if let Some(target) = message.get("target").and_then(Value::as_u64)
                    && let Some(flag) = in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .get(&target)
                {
                    flag.store(true, Ordering::Relaxed);
                }
            }
            Some("call") => {
                let Some(client) = client.clone() else {
                    write_message(
                        &writer,
                        &reply_message(
                            id,
                            Err(ToolError::new(
                                ErrorCode::AuthorizationRequired,
                                "Say hello first",
                            )),
                        ),
                    )?;
                    continue;
                };
                if in_flight.lock().unwrap_or_else(|e| e.into_inner()).len() >= MAX_IN_FLIGHT {
                    write_message(
                        &writer,
                        &reply_message(
                            id,
                            Err(ToolError::new(
                                ErrorCode::Limit,
                                "Too many concurrent requests",
                            )),
                        ),
                    )?;
                    continue;
                }
                let tool = message
                    .get("tool")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned();
                let args = message.get("args").cloned().unwrap_or(json!({}));
                let cancel = Arc::new(AtomicBool::new(false));
                in_flight
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .insert(id, cancel.clone());
                let handler = handler.clone();
                let writer = writer.clone();
                let in_flight = in_flight.clone();
                std::thread::spawn(move || {
                    let reply = handler.call(&client, &tool, args, cancel);
                    in_flight
                        .lock()
                        .unwrap_or_else(|e| e.into_inner())
                        .remove(&id);
                    let _ = write_message(&writer, &reply_message(id, reply));
                });
            }
            _ => write_message(
                &writer,
                &reply_message(id, Err(ToolError::invalid("unknown op"))),
            )?,
        }
    }
    for flag in in_flight.lock().unwrap_or_else(|e| e.into_inner()).values() {
        flag.store(true, Ordering::Relaxed);
    }
    if let Some(client) = client {
        handler.disconnected(&client);
    }
    Ok(())
}

fn reply_message(id: u64, reply: ToolResult) -> Value {
    match reply {
        Ok(result) => json!({"id": id, "result": result}),
        Err(error) => json!({"id": id, "error": error}),
    }
}

/// A connection to the desktop instance.
pub struct Client {
    writer: Mutex<Stream>,
    pending: Mutex<HashMap<u64, mpsc::Sender<ToolResult>>>,
    next: AtomicU64,
    alive: AtomicBool,
}

fn disconnected() -> ToolError {
    ToolError::new(
        ErrorCode::Disconnected,
        "Rodtty isn't running, or its automation channel closed. Start Rodtty; the next call reconnects. Calls that were in flight were not retried.",
    )
}

impl Drop for Client {
    fn drop(&mut self) {
        // The reader thread holds a clone of the stream; shut it down so the
        // host sees the client leave.
        let _ = self
            .writer
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .shutdown(std::net::Shutdown::Both);
    }
}

impl Client {
    pub fn connect(root: &Path) -> std::io::Result<Arc<Self>> {
        #[cfg(unix)]
        let stream = Stream::connect(socket_path(root))?;
        #[cfg(not(unix))]
        let stream = {
            let endpoint: Value = serde_json::from_slice(&std::fs::read(endpoint_path(root))?)?;
            let port = endpoint["port"].as_u64().unwrap_or(0) as u16;
            let mut stream = Stream::connect(("127.0.0.1", port))?;
            stream.write_all(endpoint["secret"].as_str().unwrap_or_default().as_bytes())?;
            stream.write_all(b"\n")?;
            stream
        };
        let reader = stream.try_clone()?;
        let client = Arc::new(Self {
            writer: Mutex::new(stream),
            pending: Default::default(),
            next: AtomicU64::new(1),
            alive: AtomicBool::new(true),
        });
        let weak = Arc::downgrade(&client);
        std::thread::spawn(move || {
            let mut reader = BufReader::new(reader);
            let mut line = Vec::new();
            while let Ok(true) = read_line(&mut reader, &mut line) {
                let Some(client) = weak.upgrade() else { return };
                let Ok(message) = serde_json::from_slice::<Value>(&line) else {
                    continue;
                };
                let Some(id) = message.get("id").and_then(Value::as_u64) else {
                    continue;
                };
                let reply = match message.get("error") {
                    Some(error) => {
                        Err(serde_json::from_value(error.clone()).unwrap_or_else(|_| {
                            ToolError::new(ErrorCode::Internal, error.to_string())
                        }))
                    }
                    None => Ok(message.get("result").cloned().unwrap_or(Value::Null)),
                };
                if let Some(sender) = client
                    .pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .remove(&id)
                {
                    let _ = sender.send(reply);
                }
            }
            if let Some(client) = weak.upgrade() {
                client.alive.store(false, Ordering::Relaxed);
                for (_, sender) in client
                    .pending
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .drain()
                {
                    let _ = sender.send(Err(disconnected()));
                }
            }
        });
        Ok(client)
    }

    pub fn is_alive(&self) -> bool {
        self.alive.load(Ordering::Relaxed)
    }

    /// Send `message` (without its `id`); returns the request ID and a
    /// receiver for the reply.
    pub fn send(&self, mut message: Value) -> (u64, mpsc::Receiver<ToolResult>) {
        let id = self.next.fetch_add(1, Ordering::Relaxed);
        message["id"] = json!(id);
        let (sender, receiver) = mpsc::channel();
        if !self.is_alive() {
            let _ = sender.send(Err(disconnected()));
            return (id, receiver);
        }
        self.pending
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .insert(id, sender.clone());
        if write_message(&self.writer, &message).is_err() {
            self.alive.store(false, Ordering::Relaxed);
            self.pending
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .remove(&id);
            let _ = sender.send(Err(disconnected()));
        }
        (id, receiver)
    }

    pub fn request(&self, message: Value, timeout: Duration) -> ToolResult {
        let (_, receiver) = self.send(message);
        receiver.recv_timeout(timeout).unwrap_or_else(|_| {
            Err(ToolError::new(
                ErrorCode::Timeout,
                "Rodtty didn't answer in time",
            ))
        })
    }

    pub fn cancel(&self, id: u64) {
        let _ = write_message(&self.writer, &json!({"op": "cancel", "target": id}));
    }

    pub fn hello(&self, name: &str) -> ToolResult {
        self.request(
            json!({"op": "hello", "client": name, "protocol": PROTOCOL}),
            Duration::from_secs(10),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo(Mutex<Vec<String>>);
    impl Handler for Echo {
        fn hello(&self, name: &str) -> ToolResult {
            Ok(json!({"client": format!("cl_{name}")}))
        }
        fn call(
            &self,
            client: &str,
            tool: &str,
            args: Value,
            cancel: Arc<AtomicBool>,
        ) -> ToolResult {
            if tool == "slow" {
                while !cancel.load(Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(5));
                }
                return Err(ToolError::new(ErrorCode::Cancelled, "cancelled"));
            }
            Ok(json!({"client": client, "tool": tool, "args": args}))
        }
        fn disconnected(&self, client: &str) {
            self.0.lock().unwrap().push(client.to_owned());
        }
    }

    #[test]
    fn calls_reach_the_handler_and_cancellation_stops_only_one() {
        let root = crate::test_support::tempdir().unwrap();
        let echo = Arc::new(Echo(Mutex::new(Vec::new())));
        let _server = serve(root.path(), echo.clone()).unwrap();
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let mode = std::fs::metadata(socket_path(root.path()))
                .unwrap()
                .permissions()
                .mode();
            assert_eq!(mode & 0o777, 0o600);
        }
        let client = Client::connect(root.path()).unwrap();
        assert_eq!(
            client
                .request(json!({"op":"call","tool":"x"}), Duration::from_secs(5))
                .unwrap_err()
                .code,
            ErrorCode::AuthorizationRequired
        );
        assert_eq!(client.hello("codex").unwrap()["client"], "cl_codex");
        let (slow, slow_reply) = client.send(json!({"op":"call","tool":"slow"}));
        let fast = client
            .request(
                json!({"op":"call","tool":"fast","args":{"a":1}}),
                Duration::from_secs(5),
            )
            .unwrap();
        assert_eq!(fast["args"]["a"], 1);
        assert_eq!(fast["client"], "cl_codex");
        client.cancel(slow);
        assert_eq!(
            slow_reply
                .recv_timeout(Duration::from_secs(5))
                .unwrap()
                .unwrap_err()
                .code,
            ErrorCode::Cancelled
        );
        drop(client);
        let deadline = std::time::Instant::now() + Duration::from_secs(5);
        while echo.0.lock().unwrap().is_empty() && std::time::Instant::now() < deadline {
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(echo.0.lock().unwrap().as_slice(), ["cl_codex"]);
    }
}

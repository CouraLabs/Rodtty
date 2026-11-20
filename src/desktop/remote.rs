//! Remote session handoff in the window: the owner enables access, pairs
//! devices with one-time codes, sees attached devices in the footer, and
//! disconnects and revokes them. Remote calls run here, on the UI thread,
//! against the same sessions the window shows.
use super::*;
use crate::remote::{self, Call, Devices, Reply};
use serde_json::json;
use std::collections::HashMap;
use std::time::{Duration, Instant};

/// A device counts as attached while it polled within this long.
const ATTACHED_FOR: Duration = Duration::from_secs(45);

#[derive(Default)]
pub(crate) struct RemoteState {
    pub server: Option<remote::Server>,
    pub pairing: remote::Pairing,
    /// The device controlling each session, by session identity. Absent:
    /// the desktop has control.
    pub controllers: HashMap<usize, String>,
    /// Each device's last poll and the session it reads.
    pub attached: HashMap<String, (Instant, Option<usize>)>,
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b = [
            chunk[0],
            *chunk.get(1).unwrap_or(&0),
            *chunk.get(2).unwrap_or(&0),
        ];
        let n = (u32::from(b[0]) << 16) | (u32::from(b[1]) << 8) | u32::from(b[2]);
        for (i, shift) in [18, 12, 6, 0].into_iter().enumerate() {
            if i <= chunk.len() {
                out.push(T[((n >> shift) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

impl State {
    /// Start listening on the Tailscale address and remember the choice.
    pub(crate) fn enable_remote(&mut self) {
        if self.remote.server.is_some() {
            self.message = "Remote access is on.".into();
            return;
        }
        match self.start_remote() {
            Ok(addr) => {
                self.save_config(|c| c.behavior.remote_access = true);
                self.message = format!(
                    "Remote access is on at http://{addr}/ (inside your tailnet). Use Remote: Pair Device to add a device."
                );
            }
            Err(e) => self.message = format!("Remote access is off: {e:#}"),
        }
    }

    pub(crate) fn start_remote(&mut self) -> anyhow::Result<std::net::SocketAddr> {
        let handle = self
            .handle
            .clone()
            .ok_or_else(|| anyhow::anyhow!("the window isn't running"))?;
        let handle = std::sync::Mutex::new(handle);
        let dispatch: remote::Dispatch = Arc::new(move |call| {
            let handle = handle
                .lock()
                .map_err(|_| (503, "stopping".to_owned()))?
                .clone();
            automation::on_ui(&handle, move |s, _| s.remote_call(call))
                .unwrap_or(Err((503, "Rodtty didn't answer".into())))
        });
        let server = remote::start(remote::bind_address()?, dispatch)?;
        let addr = server.addr;
        self.remote.server = Some(server);
        Ok(addr)
    }

    /// Stop listening; paired devices stay paired.
    pub(crate) fn disable_remote(&mut self) {
        self.remote.server = None;
        self.remote.attached.clear();
        self.remote.controllers.clear();
        self.save_config(|c| c.behavior.remote_access = false);
        self.message = "Remote access is off.".into();
    }

    pub(crate) fn pair_remote_device(&mut self) {
        if self.remote.server.is_none() {
            self.enable_remote();
            if self.remote.server.is_none() {
                return;
            }
        }
        let addr = self.remote.server.as_ref().map(|s| s.addr);
        self.message = match (self.remote.pairing.new_code(), addr) {
            (Ok(code), Some(addr)) => format!(
                "Open http://{addr}/ on the device and enter {code}. The code works once, for 5 minutes."
            ),
            (Err(e), _) => format!("Could not make a pairing code: {e:#}"),
            _ => String::new(),
        };
    }

    /// Disconnect every device and revoke its pairing.
    pub(crate) fn revoke_remote_devices(&mut self) {
        let count = Devices::load(&self.root).devices.len();
        self.message = match Devices::default().save(&self.root) {
            Ok(()) => format!("Revoked {count} paired device(s); they must pair again."),
            Err(e) => format!("Could not revoke devices: {e:#}"),
        };
        self.remote.attached.clear();
        self.remote.controllers.clear();
    }

    /// The owner typed on the desktop: the desktop takes control back.
    pub(crate) fn owner_takes_remote_control(&mut self) {
        if let Some(id) = self.active().map(Session::id) {
            self.remote.controllers.remove(&id);
        }
    }

    pub(crate) fn remote_indicator(&self) -> Option<String> {
        self.remote.server.as_ref()?;
        let n = self
            .remote
            .attached
            .values()
            .filter(|(at, _)| at.elapsed() < ATTACHED_FOR)
            .count();
        Some(match n {
            0 => "Remote: on".into(),
            n => format!("Remote: {n} device(s) attached"),
        })
    }

    pub(crate) fn remote_call(&mut self, call: Call) -> Reply {
        let session = |s: &State, id: usize| {
            s.sessions
                .iter()
                .position(|x| x.id() == id)
                .ok_or((404, "that session has ended".to_owned()))
        };
        match call {
            Call::Pair { code, name } => self
                .remote
                .pairing
                .redeem(&self.root, &code, &name)
                .map(|d| {
                    self.message = format!("Paired {}.", d.name);
                    json!({"device": d.id, "token": d.token, "expires_at": d.expires_at})
                })
                .map_err(|e| (403, e.to_owned())),
            Call::Attached { device, session } => {
                let devices = Devices::load(&self.root);
                let device = devices
                    .authenticate(&device)
                    .ok_or((401, "unknown or revoked device".to_owned()))?;
                let entry = self
                    .remote
                    .attached
                    .entry(device.id.clone())
                    .or_insert((Instant::now(), None));
                entry.0 = Instant::now();
                if session.is_some() {
                    entry.1 = session;
                }
                Ok(json!({"device": device.id}))
            }
            Call::List => Ok(json!({
                "sessions": self.sessions.iter().map(|s| json!({
                    "id": s.id(),
                    "title": s.tab_title(),
                    "directory": window::tilde_path(&s.cwd()),
                    "running": s.has_running_commands(),
                    "controller": self.remote.controllers.get(&s.id()),
                })).collect::<Vec<_>>()
            })),
            Call::Read {
                session: id,
                cursor,
            } => {
                let index = session(self, id)?;
                let s = &self.sessions[index];
                let state = s.state.lock().unwrap_or_else(|e| e.into_inner());
                let ended = state.exit.is_some();
                // A first read starts at the recent output.
                let start = if cursor == 0 {
                    state.output.end().saturating_sub(remote::READ_BYTES as u64)
                } else {
                    cursor
                };
                let (from, data, next) = state.output.read(start, remote::READ_BYTES);
                Ok(json!({
                    "data": base64(&data),
                    "next": next,
                    "truncated": cursor != 0 && from > cursor,
                    "ended": ended && data.is_empty(),
                }))
            }
            Call::Control {
                session: id,
                device,
            } => {
                session(self, id)?;
                self.remote.controllers.insert(id, device);
                Ok(json!({"control": true}))
            }
            Call::Input {
                session: id,
                device,
                text,
            } => {
                let index = session(self, id)?;
                if self.remote.controllers.get(&id) != Some(&device) {
                    return Err((409, "take control of the session first".into()));
                }
                self.sessions[index]
                    .send(text.into_bytes())
                    .map_err(|e| (500, e.to_string()))?;
                Ok(json!({"sent": true}))
            }
            Call::Resize {
                session: id,
                device,
                rows,
                cols,
            } => {
                let index = session(self, id)?;
                if self.remote.controllers.get(&id) != Some(&device) {
                    return Err((409, "take control of the session first".into()));
                }
                self.sessions[index]
                    .resize(rows, cols)
                    .map_err(|e| (500, e.to_string()))?;
                Ok(json!({"rows": rows, "cols": cols}))
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_standard_alphabet() {
        assert_eq!(base64(b""), "");
        assert_eq!(base64(b"f"), "Zg==");
        assert_eq!(base64(b"fo"), "Zm8=");
        assert_eq!(base64(b"foobar"), "Zm9vYmFy");
    }
}

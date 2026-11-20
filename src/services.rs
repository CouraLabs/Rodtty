//! Local development services: TCP listeners on this host, and which
//! terminal session (if any) runs each one.
//!
//! Listing uses `lsof -nP -iTCP -sTCP:LISTEN` on macOS and Linux and
//! `netstat -ano -p TCP` on Windows, with a timeout, off the UI thread.
//! Ownership is verified by walking the listener's parent processes up to a
//! session's shell; anything unverifiable is reported as unknown, never
//! guessed. Nothing is terminated or forwarded.
use serde::Serialize;
use std::{
    collections::BTreeMap, path::Path, process::Command, sync::atomic::AtomicBool, time::Duration,
};

#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize)]
pub struct Listener {
    pub port: u16,
    pub address: String,
    pub pid: Option<u32>,
    pub process: Option<String>,
}

impl Listener {
    /// Whether the listener is reachable on loopback.
    pub fn loopback_reachable(&self) -> bool {
        matches!(
            self.address.as_str(),
            "127.0.0.1" | "::1" | "[::1]" | "*" | "0.0.0.0" | "::" | "[::]"
        ) || self.address.starts_with("127.")
    }
    /// A local URL for this listener; the caller decides whether it speaks HTTP.
    pub fn local_url(&self, scheme: &str) -> String {
        let host = if self.address.contains(':') && self.address != "*" {
            "[::1]"
        } else {
            "127.0.0.1"
        };
        format!("{scheme}://{host}:{}", self.port)
    }
}

/// Parse `lsof -F pcn` output: `p<pid>`, `c<command>`, `n<address:port>` records.
pub fn parse_lsof(text: &str) -> Vec<Listener> {
    let mut out = Vec::new();
    let (mut pid, mut command) = (None, None);
    for line in text.lines() {
        let (tag, value) = line.split_at(line.len().min(1));
        match tag {
            "p" => {
                pid = value.parse().ok();
                command = None;
            }
            "c" => command = Some(value.to_owned()),
            "n" => {
                if let Some((address, port)) = value.rsplit_once(':')
                    && let Ok(port) = port.parse()
                {
                    out.push(Listener {
                        port,
                        address: address.trim_matches(['[', ']']).to_owned(),
                        pid,
                        process: command.clone(),
                    });
                }
            }
            _ => {}
        }
    }
    out.sort();
    out.dedup();
    out
}

/// Parse Windows `netstat -ano -p TCP` rows in the LISTENING state.
pub fn parse_netstat(text: &str) -> Vec<Listener> {
    let mut out: Vec<Listener> = text
        .lines()
        .filter_map(|line| {
            let fields: Vec<&str> = line.split_whitespace().collect();
            if fields.len() < 5
                || !fields[0].eq_ignore_ascii_case("TCP")
                || !fields[3].eq_ignore_ascii_case("LISTENING")
            {
                return None;
            }
            let (address, port) = fields[1].rsplit_once(':')?;
            Some(Listener {
                port: port.parse().ok()?,
                address: address.trim_matches(['[', ']']).to_owned(),
                pid: fields[4].parse().ok(),
                process: None,
            })
        })
        .collect();
    out.sort();
    out.dedup();
    out
}

/// The listeners on this host.
pub fn list(cancel: &AtomicBool) -> Result<Vec<Listener>, String> {
    let dir = std::env::temp_dir();
    #[cfg(windows)]
    {
        let (_, out) = crate::environment::probe(
            "netstat",
            &["-ano", "-p", "TCP"],
            &dir,
            Duration::from_secs(5),
            cancel,
        )?;
        Ok(parse_netstat(&out))
    }
    #[cfg(not(windows))]
    {
        let (_, out) = crate::environment::probe(
            "lsof",
            &["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"],
            &dir,
            Duration::from_secs(5),
            cancel,
        )
        .or_else(|e| {
            if Path::new("/usr/sbin/lsof").exists() {
                crate::environment::probe(
                    "/usr/sbin/lsof",
                    &["-nP", "-iTCP", "-sTCP:LISTEN", "-F", "pcn"],
                    &dir,
                    Duration::from_secs(5),
                    cancel,
                )
            } else {
                Err(e)
            }
        })?;
        Ok(parse_lsof(&out))
    }
}

/// Parent process IDs for every process, from `ps` (Unix).
pub fn parents() -> Option<BTreeMap<u32, u32>> {
    #[cfg(unix)]
    {
        let out = Command::new("ps")
            .args(["-axo", "pid=,ppid="])
            .output()
            .ok()?;
        let text = String::from_utf8_lossy(&out.stdout);
        Some(
            text.lines()
                .filter_map(|l| {
                    let mut f = l.split_whitespace();
                    Some((f.next()?.parse().ok()?, f.next()?.parse().ok()?))
                })
                .collect(),
        )
    }
    #[cfg(not(unix))]
    None
}

/// The shell (from `shells`) that `pid` descends from, if the process tree shows one.
pub fn owner(pid: u32, shells: &[u32], parents: &BTreeMap<u32, u32>) -> Option<u32> {
    let mut current = pid;
    for _ in 0..64 {
        if shells.contains(&current) {
            return Some(current);
        }
        current = *parents.get(&current)?;
        if current <= 1 {
            return None;
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn listeners_parse_from_lsof_and_netstat() {
        let lsof = "p100\ncnode\nn127.0.0.1:4200\nn[::1]:4200\np200\ncpython3\nn*:8000\n";
        let rows = parse_lsof(lsof);
        assert_eq!(rows.len(), 3);
        assert_eq!(rows[0].port, 4200);
        assert!(
            rows.iter()
                .any(|r| r.address == "::1" && r.local_url("http") == "http://[::1]:4200")
        );
        assert!(rows.iter().all(Listener::loopback_reachable));
        let netstat = "  TCP    0.0.0.0:135   0.0.0.0:0   LISTENING   912\n  TCP    [::1]:5432   [::]:0   LISTENING   4021\n  TCP 10.0.0.2:5000 1.2.3.4:443 ESTABLISHED 77\n";
        let rows = parse_netstat(netstat);
        assert_eq!(rows.iter().map(|r| r.port).collect::<Vec<_>>(), [135, 5432]);
    }

    #[test]
    fn ownership_needs_a_path_to_a_session_shell() {
        let parents: BTreeMap<u32, u32> =
            [(30, 20), (20, 10), (10, 1), (40, 1)].into_iter().collect();
        assert_eq!(owner(30, &[10], &parents), Some(10));
        assert_eq!(owner(40, &[10], &parents), None);
        assert_eq!(owner(99, &[10], &parents), None);
    }
}

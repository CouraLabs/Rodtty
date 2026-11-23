//! Local development services: TCP listeners on this host, and which
//! terminal session (if any) runs each one.
//!
//! Listing uses `lsof -nP -iTCP -sTCP:LISTEN` on macOS and Linux and
//! `netstat -ano -p TCP` on Windows, with a timeout, off the UI thread.
//! Ownership is verified by walking the listener's parent processes up to a
//! session's shell; anything unverifiable is reported as unknown, never
//! guessed. Nothing is terminated or forwarded.
use serde::Serialize;
use std::{collections::BTreeMap, sync::atomic::AtomicBool, time::Duration};

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
        let Some((tag, value)) = line.split_at_checked(1) else {
            continue;
        };
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
            if std::path::Path::new("/usr/sbin/lsof").exists() {
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
        let out = std::process::Command::new("ps")
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
    fn lsof_lines_with_non_ascii_or_unknown_tags_are_skipped() {
        assert!(parse_lsof("é").is_empty());
        assert!(parse_lsof("\n\nf3\nn\nnnoport\nn127.0.0.1:http\n").is_empty());
        let rows = parse_lsof("p7\ncnodé\nn127.0.0.1:80\néx\nn127.0.0.1:81\n");
        assert_eq!(rows.iter().map(|r| r.port).collect::<Vec<_>>(), [80, 81]);
        assert!(rows.iter().all(|r| r.process.as_deref() == Some("nodé")));
    }

    #[test]
    fn netstat_rows_need_tcp_listening_and_a_numeric_port() {
        let text = "Proto Local Foreign State PID\n  UDP 0.0.0.0:53 *:* 1\n  TCP 0.0.0.0:x 0.0.0.0:0 LISTENING 2\n  TCP 0.0.0.0:22 0.0.0.0:0 listening notapid\n  TCP 0.0.0.0:23 0.0.0.0:0\n";
        let rows = parse_netstat(text);
        assert_eq!(rows.len(), 1);
        assert_eq!((rows[0].port, rows[0].pid), (22, None));
    }

    #[test]
    fn local_urls_use_the_loopback_family_of_the_listener() {
        let listener = |address: &str| Listener {
            port: 3000,
            address: address.to_owned(),
            pid: None,
            process: None,
        };
        assert_eq!(listener("*").local_url("http"), "http://127.0.0.1:3000");
        assert_eq!(
            listener("0.0.0.0").local_url("http"),
            "http://127.0.0.1:3000"
        );
        assert_eq!(listener("::").local_url("https"), "https://[::1]:3000");
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

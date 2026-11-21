//! Project environment check: the tool versions a project asks for, against
//! what this machine has.
//!
//! Manifests are parsed as data: `.node-version`, `global.json` and
//! `rust-toolchain.toml` (or `rust-toolchain`). Probes run fixed version
//! commands (`node --version`, `dotnet --list-sdks`, `rustc --version`) in
//! the project folder with Rodtty's own environment (`PATH` as Rodtty was
//! started with, not a terminal's custom shell changes), each with a timeout.
//! Nothing from the project is executed. Note that `rustc` is usually a
//! rustup proxy, which itself reads the project's toolchain file.
use serde::Serialize;
use std::{
    path::{Path, PathBuf},
    process::{Command, Stdio},
    sync::atomic::{AtomicBool, Ordering},
    time::Duration,
};
use wait_timeout::ChildExt;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum Outcome {
    Match,
    Mismatch,
    Missing,
    Unknown,
}

#[derive(Clone, Debug, PartialEq, Serialize)]
pub struct Row {
    pub source: PathBuf,
    pub tool: &'static str,
    /// The expected version or channel, as written.
    pub expected: String,
    pub executable: Option<PathBuf>,
    pub detected: Option<String>,
    pub outcome: Outcome,
    pub detail: String,
}

pub const PROBE_TIMEOUT: Duration = Duration::from_secs(5);

/// Run `program args` in `cwd` and return its standard output, or why not.
pub fn probe(
    program: &str,
    args: &[&str],
    cwd: &Path,
    timeout: Duration,
    cancel: &AtomicBool,
) -> Result<(PathBuf, String), String> {
    let executable = which(program).ok_or_else(|| format!("{program} is not on PATH"))?;
    let mut child = Command::new(&executable)
        .args(args)
        .current_dir(cwd)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| e.to_string())?;
    let mut waited = Duration::ZERO;
    let step = Duration::from_millis(50);
    loop {
        if let Some(_status) = child.wait_timeout(step).map_err(|e| e.to_string())? {
            break;
        }
        waited += step;
        if waited >= timeout || cancel.load(Ordering::Relaxed) {
            let _ = child.kill();
            let _ = child.wait();
            return Err(if waited >= timeout {
                format!("{program} did not answer within {}s", timeout.as_secs())
            } else {
                "cancelled".into()
            });
        }
    }
    let mut out = String::new();
    use std::io::Read;
    if let Some(mut stdout) = child.stdout.take() {
        let _ = stdout.read_to_string(&mut out);
    }
    Ok((executable, out))
}

pub fn which(program: &str) -> Option<PathBuf> {
    let path = std::env::var_os("PATH")?;
    std::env::split_paths(&path).find_map(|dir| {
        let candidate = dir.join(format!("{program}{}", std::env::consts::EXE_SUFFIX));
        candidate.is_file().then_some(candidate)
    })
}

fn version_numbers(text: &str) -> Vec<u64> {
    text.trim()
        .trim_start_matches('v')
        .split(['.', '-', '+'])
        .map_while(|p| p.parse().ok())
        .collect()
}

/// Whether `detected` satisfies a numeric prefix like `20`, `20.11` or `20.11.1`.
fn prefix_matches(expected: &str, detected: &str) -> Option<bool> {
    let want = version_numbers(expected);
    if want.is_empty()
        || expected
            .trim()
            .trim_start_matches('v')
            .contains(|c: char| !c.is_ascii_digit() && c != '.')
    {
        return None;
    }
    let have = version_numbers(detected);
    Some(have.len() >= want.len() && have[..want.len()] == want[..])
}

fn node(project: &Path, cancel: &AtomicBool, timeout: Duration, rows: &mut Vec<Row>) {
    let source = project.join(".node-version");
    let Ok(text) = std::fs::read_to_string(&source) else {
        return;
    };
    let expected = text.lines().next().unwrap_or("").trim().to_owned();
    let mut row = Row {
        source,
        tool: "node",
        expected: expected.clone(),
        executable: None,
        detected: None,
        outcome: Outcome::Unknown,
        detail: String::new(),
    };
    match probe("node", &["--version"], project, timeout, cancel) {
        Err(e) if e.ends_with("not on PATH") => {
            row.outcome = Outcome::Missing;
            row.detail = e;
        }
        Err(e) => row.detail = e,
        Ok((exe, out)) => {
            let detected = out.trim().to_owned();
            row.executable = Some(exe);
            row.outcome = match prefix_matches(&expected, &detected) {
                Some(true) => Outcome::Match,
                Some(false) => Outcome::Mismatch,
                None => {
                    row.detail =
                        format!("{expected:?} is an alias or range this check doesn't resolve");
                    Outcome::Unknown
                }
            };
            row.detected = Some(detected);
        }
    }
    rows.push(row);
}

/// .NET SDK selection by `global.json`: `version` with `rollForward`
/// (default `latestPatch`). Patch roll-forward stays within the feature band
/// (the hundreds of the patch number).
fn dotnet_matches(
    expected: &str,
    policy: &str,
    installed: &[String],
) -> Option<(bool, Option<String>)> {
    let want = version_numbers(expected);
    if want.len() < 3 {
        return None;
    }
    let band = |v: &[u64]| v[2] / 100;
    let candidates: Vec<(&String, Vec<u64>)> = installed
        .iter()
        .map(|s| (s, version_numbers(s)))
        .filter(|(_, v)| v.len() >= 3)
        .collect();
    let ok = |v: &[u64]| -> Option<bool> {
        Some(match policy {
            "disable" => v[..3] == want[..3],
            "patch" | "latestPatch" => {
                v[0] == want[0] && v[1] == want[1] && band(v) == band(&want) && v[2] >= want[2]
            }
            "feature" | "latestFeature" => v[0] == want[0] && v[1] == want[1] && v[2] >= want[2],
            "minor" | "latestMinor" => v[0] == want[0] && (v[1], v[2]) >= (want[1], want[2]),
            "major" | "latestMajor" => (v[0], v[1], v[2]) >= (want[0], want[1], want[2]),
            _ => return None,
        })
    };
    let mut found = None;
    for (text, v) in &candidates {
        match ok(v) {
            None => return None,
            Some(true) => found = Some((*text).clone()),
            Some(false) => {}
        }
    }
    Some((found.is_some(), found))
}

fn dotnet(project: &Path, cancel: &AtomicBool, timeout: Duration, rows: &mut Vec<Row>) {
    let source = project.join("global.json");
    let Ok(text) = std::fs::read_to_string(&source) else {
        return;
    };
    let mut row = Row {
        source,
        tool: "dotnet",
        expected: String::new(),
        executable: None,
        detected: None,
        outcome: Outcome::Unknown,
        detail: String::new(),
    };
    let parsed: Result<serde_json::Value, _> = serde_json::from_str(&text);
    let Some(sdk) = parsed.as_ref().ok().and_then(|v| v.get("sdk")) else {
        row.detail = match parsed {
            Err(e) => format!("invalid JSON: {e}"),
            Ok(_) => "no sdk section".into(),
        };
        rows.push(row);
        return;
    };
    let expected = sdk
        .get("version")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_owned();
    let policy = sdk
        .get("rollForward")
        .and_then(|v| v.as_str())
        .unwrap_or("latestPatch")
        .to_owned();
    row.expected = format!("{expected} (rollForward {policy})");
    match probe("dotnet", &["--list-sdks"], project, timeout, cancel) {
        Err(e) if e.ends_with("not on PATH") => {
            row.outcome = Outcome::Missing;
            row.detail = e;
        }
        Err(e) => row.detail = e,
        Ok((exe, out)) => {
            let installed: Vec<String> = out
                .lines()
                .filter_map(|l| l.split_whitespace().next())
                .map(str::to_owned)
                .collect();
            row.executable = Some(exe);
            match dotnet_matches(&expected, &policy, &installed) {
                Some((true, found)) => {
                    row.outcome = Outcome::Match;
                    row.detected = found;
                }
                Some((false, _)) => {
                    row.outcome = Outcome::Mismatch;
                    row.detected = Some(installed.join(", "));
                }
                None => row.detail = "unsupported version or rollForward policy".into(),
            }
        }
    }
    rows.push(row);
}

fn rust(project: &Path, cancel: &AtomicBool, timeout: Duration, rows: &mut Vec<Row>) {
    let (source, text) = match ["rust-toolchain.toml", "rust-toolchain"]
        .iter()
        .find_map(|name| {
            let path = project.join(name);
            std::fs::read_to_string(&path).ok().map(|t| (path, t))
        }) {
        Some(found) => found,
        None => return,
    };
    let mut row = Row {
        source: source.clone(),
        tool: "rustc",
        expected: String::new(),
        executable: None,
        detected: None,
        outcome: Outcome::Unknown,
        detail: String::new(),
    };
    let channel = if source.extension().is_some_and(|e| e == "toml") {
        match text.parse::<toml::Table>() {
            Ok(table) => table
                .get("toolchain")
                .and_then(|t| t.get("channel"))
                .and_then(|c| c.as_str())
                .map(str::to_owned),
            Err(e) => {
                row.detail = format!("invalid TOML: {e}");
                rows.push(row);
                return;
            }
        }
    } else {
        text.lines().next().map(|l| l.trim().to_owned())
    };
    let Some(channel) = channel.filter(|c| !c.is_empty()) else {
        row.detail = "no toolchain channel".into();
        rows.push(row);
        return;
    };
    row.expected = channel.clone();
    match probe("rustc", &["--version"], project, timeout, cancel) {
        Err(e) if e.ends_with("not on PATH") => {
            row.outcome = Outcome::Missing;
            row.detail = e;
        }
        Err(e) => row.detail = e,
        Ok((exe, out)) => {
            // `rustc 1.79.0 (129f3b996 2024-06-10)` or `rustc 1.81.0-nightly (…)`
            let detected = out.split_whitespace().nth(1).unwrap_or("").to_owned();
            row.executable = Some(exe);
            let kind = |v: &str| {
                if v.contains("nightly") {
                    "nightly"
                } else if v.contains("beta") {
                    "beta"
                } else {
                    "stable"
                }
            };
            row.outcome = match channel.as_str() {
                "stable" | "beta" | "nightly" => {
                    row.detail = "rolling channel: matched by channel kind".into();
                    if kind(&detected) == channel {
                        Outcome::Match
                    } else {
                        Outcome::Mismatch
                    }
                }
                c if c.starts_with("nightly-")
                    || c.starts_with("beta-")
                    || c.starts_with("stable-") =>
                {
                    row.detail = "dated channels are not compared".into();
                    Outcome::Unknown
                }
                c => match prefix_matches(c, &detected) {
                    Some(true) if kind(&detected) == "stable" => Outcome::Match,
                    Some(_) => Outcome::Mismatch,
                    None => Outcome::Unknown,
                },
            };
            row.detected = Some(detected);
        }
    }
    rows.push(row);
}

/// Check every supported manifest in `project`. One manifest's problem
/// never hides the others.
pub fn check(project: &Path, timeout: Duration, cancel: &AtomicBool) -> Vec<Row> {
    let mut rows = Vec::new();
    node(project, cancel, timeout, &mut rows);
    dotnet(project, cancel, timeout, &mut rows);
    rust(project, cancel, timeout, &mut rows);
    rows
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn version_policies() {
        assert_eq!(prefix_matches("20", "v20.11.1"), Some(true));
        assert_eq!(prefix_matches("20.11", "v20.12.0"), Some(false));
        assert_eq!(prefix_matches("lts/*", "v20.1.0"), None);
        let sdks = vec!["8.0.100".to_owned(), "8.0.204".to_owned()];
        assert_eq!(
            dotnet_matches("8.0.201", "latestPatch", &sdks),
            Some((true, Some("8.0.204".into())))
        );
        assert_eq!(
            dotnet_matches("8.0.110", "latestPatch", &sdks),
            Some((false, None))
        );
        assert!(dotnet_matches("8.0.110", "latestFeature", &sdks).unwrap().0);
        assert_eq!(dotnet_matches("8.0.110", "sideways", &sdks), None);
    }

    #[test]
    fn rows_cover_match_mismatch_missing_invalid_and_never_run_project_files() {
        let dir = crate::test_support::tempdir().unwrap();
        let project = dir.path();
        let never = AtomicBool::new(false);
        // A manifest that looks like a script, and a script beside it, must stay inert.
        std::fs::write(
            project.join("rust-toolchain.toml"),
            "[toolchain]\nchannel = \"stable\"\n",
        )
        .unwrap();
        std::fs::write(project.join("global.json"), "{ not json").unwrap();
        std::fs::write(project.join(".node-version"), "$(touch ran)\n").unwrap();
        std::fs::write(project.join("setup.sh"), "touch ran\n").unwrap();
        let rows = check(project, PROBE_TIMEOUT, &never);
        let by = |tool: &str| rows.iter().find(|r| r.tool == tool).unwrap().clone();
        assert_eq!(by("dotnet").outcome, Outcome::Unknown);
        assert!(by("dotnet").detail.starts_with("invalid JSON"));
        let node = by("node");
        assert!(
            matches!(node.outcome, Outcome::Unknown | Outcome::Missing),
            "{node:?}"
        );
        let rust = by("rustc");
        assert!(
            matches!(
                rust.outcome,
                Outcome::Match | Outcome::Mismatch | Outcome::Missing
            ),
            "{rust:?}"
        );
        assert!(
            !project.join("ran").exists(),
            "nothing from the project ran"
        );
    }

    #[cfg(unix)]
    #[test]
    fn a_hanging_probe_times_out() {
        let dir = crate::test_support::tempdir().unwrap();
        let never = AtomicBool::new(false);
        let error = probe(
            "sleep",
            &["30"],
            dir.path(),
            Duration::from_millis(300),
            &never,
        )
        .unwrap_err();
        assert!(error.contains("did not answer"), "{error}");
    }
}

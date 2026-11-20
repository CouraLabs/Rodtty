use crate::{
    config::Config,
    profile::{DISABLED_ENV, PreparedProfile},
};
use anyhow::{Context, Result, bail};
use portable_pty::{ChildKiller, CommandBuilder, MasterPty, PtySize, native_pty_system};
use std::{
    io::{Read, Write},
    path::{Path, PathBuf},
    sync::{
        Arc, Mutex,
        atomic::{AtomicUsize, Ordering},
        mpsc,
    },
    time::{Duration, Instant},
};

#[derive(Default)]
pub struct TerminalEvents {
    pub cwd: Option<PathBuf>,
    pub title: String,
    responses: Vec<Vec<u8>>,
    command_running: bool,
    activity_tag: String,
}

impl vt100::Callbacks for TerminalEvents {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }
    fn unhandled_osc(&mut self, _: &mut vt100::Screen, params: &[&[u8]]) {
        match params {
            // vt100 routes OSC 0/2 here when semicolons split the title into
            // additional parameters. They belong to the title, not its syntax.
            [b"0" | b"2", title @ ..] if !title.is_empty() => {
                self.title = String::from_utf8_lossy(&title.join(&b';')).into_owned();
            }
            [b"7", location @ ..] if !location.is_empty() => {
                let location = location.join(&b';');
                if let Some(path) = std::str::from_utf8(&location)
                    .ok()
                    .and_then(|p| p.strip_prefix("file://localhost"))
                {
                    let path = PathBuf::from(path);
                    if path.is_dir() {
                        self.cwd = Some(path);
                    }
                }
            }
            [b"777", b"rodtty", tag, phase]
                if !self.activity_tag.is_empty() && *tag == self.activity_tag.as_bytes() =>
            {
                match *phase {
                    b"C" => self.command_running = true,
                    b"A" => self.command_running = false,
                    _ => {}
                }
            }
            _ => {}
        }
    }
    fn unhandled_csi(
        &mut self,
        screen: &mut vt100::Screen,
        _: Option<u8>,
        _: Option<u8>,
        params: &[&[u16]],
        c: char,
    ) {
        if c == 'n' && params.first().and_then(|p| p.first()) == Some(&6) {
            let (row, col) = screen.cursor_position();
            self.responses
                .push(format!("\x1b[{};{}R", row + 1, col + 1).into_bytes());
        } else if c == 'c' {
            self.responses.push(b"\x1b[?1;2c".to_vec());
        }
    }
}

pub struct TerminalState {
    pub parser: vt100::Parser<TerminalEvents>,
    pub generation: u64,
    pub bytes: u64,
    pub exit: Option<u32>,
    pub error: Option<String>,
}

/// A terminal tab's label while its application has set no terminal title.
pub const FALLBACK_TAB_TITLE: &str = "Terminal";

static NEXT_SESSION_ID: AtomicUsize = AtomicUsize::new(1);

pub struct Session {
    id: usize,
    pub initial_cwd: PathBuf,
    pub config: Config,
    pub state: Arc<Mutex<TerminalState>>,
    input: Option<Arc<mpsc::SyncSender<Vec<u8>>>>,
    master: Box<dyn MasterPty + Send>,
    killer: Box<dyn ChildKiller + Send + Sync>,
    size: (u16, u16),
    shell_pid: Option<u32>,
    jobs: PathBuf,
    job_pids: PathBuf,
    profile: PathBuf,
    /// The shell executable's path. A Windows console titles itself with the
    /// program it runs, which names no application, so tabs ignore it.
    shell_title: String,
}

impl Session {
    pub fn spawn(
        shell: &Path,
        root: &Path,
        cwd: &Path,
        config: Config,
        rows: u16,
        cols: u16,
        wake: Arc<dyn Fn() + Send + Sync>,
    ) -> Result<Self> {
        let profile = PreparedProfile::new(shell, root, &config)?;
        let pair = native_pty_system().openpty(PtySize {
            rows,
            cols,
            pixel_width: 0,
            pixel_height: 0,
        })?;
        let mut command = CommandBuilder::new(shell);
        command.args([
            "--no-config",
            "--noprofile",
            "--input-backend",
            "vt",
            "--rcfile",
        ]);
        command.arg(&profile.rc);
        command.arg("-i");
        if !config.history {
            command.args(["+o", "history"]);
        }
        command.cwd(cwd);
        command.env("PATH", &profile.path);
        command.env(DISABLED_ENV, &profile.disabled);
        command.env("TERM", "xterm-256color");
        command.env("COLORTERM", "truecolor");
        command.env_remove("BASH_ENV");
        let mut child = pair
            .slave
            .spawn_command(command)
            .context("start Brush in PTY")?;
        drop(pair.slave);
        let shell_pid = child.process_id();
        let killer = child.clone_killer();
        let mut reader = pair.master.try_clone_reader()?;
        let mut writer = pair.master.take_writer()?;
        let (input, incoming) = mpsc::sync_channel::<Vec<u8>>(128);
        let input = Arc::new(input);
        let state = Arc::new(Mutex::new(TerminalState {
            parser: vt100::Parser::new_with_callbacks(
                rows,
                cols,
                config.scrollback,
                TerminalEvents {
                    command_running: true,
                    activity_tag: profile.activity_tag.clone(),
                    ..Default::default()
                },
            ),
            generation: 0,
            bytes: 0,
            exit: None,
            error: None,
        }));
        let write_state = state.clone();
        let write_wake = wake.clone();
        std::thread::spawn(move || {
            for bytes in incoming {
                if let Err(error) = writer.write_all(&bytes).and_then(|_| writer.flush()) {
                    if let Ok(mut state) = write_state.lock() {
                        state.error = Some(error.to_string());
                    }
                    write_wake();
                    break;
                }
            }
        });
        let read_state = state.clone();
        let read_wake = wake.clone();
        let replies = Arc::downgrade(&input);
        std::thread::spawn(move || {
            let mut buffer = [0; 16_384];
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let mut state = read_state.lock().unwrap_or_else(|e| e.into_inner());
                        state.parser.process(&buffer[..count]);
                        state.bytes += count as u64;
                        state.generation += 1;
                        let responses = std::mem::take(&mut state.parser.callbacks_mut().responses);
                        drop(state);
                        for response in responses {
                            if let Some(replies) = replies.upgrade() {
                                let _ = replies.try_send(response);
                            }
                        }
                        read_wake();
                    }
                    // Unix PTYs commonly return EIO after the slave closes.
                    Err(_) => break,
                }
            }
        });
        let jobs = profile.jobs.clone();
        let job_pids = profile.job_pids.clone();
        let profile_directory = profile.directory.path().to_owned();
        let exit_state = state.clone();
        std::thread::spawn(move || {
            // Windows cannot remove in-use launcher executables. Keep the
            // profile until the child has exited rather than until the view drops.
            let _profile = profile;
            let result = child.wait();
            let mut state = exit_state.lock().unwrap_or_else(|e| e.into_inner());
            match result {
                Ok(status) => state.exit = Some(status.exit_code()),
                Err(error) => state.error = Some(error.to_string()),
            }
            state.generation += 1;
            drop(state);
            wake();
        });
        Ok(Self {
            shell_title: shell.to_string_lossy().into_owned(),
            id: NEXT_SESSION_ID.fetch_add(1, Ordering::Relaxed),
            initial_cwd: cwd.to_owned(),
            config,
            state,
            input: Some(input),
            master: pair.master,
            killer,
            size: (rows, cols),
            shell_pid,
            jobs,
            job_pids,
            profile: profile_directory,
        })
    }

    /// Process-lifetime identity, independent of tab position and rendering.
    pub fn id(&self) -> usize {
        self.id
    }

    /// A live interactive shell at its prompt is idle. Shell markers cover
    /// builtins as well as external commands; the foreground group additionally
    /// protects interactive programs which own the terminal on Unix.
    pub fn has_running_commands(&self) -> bool {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if state.exit.is_some() {
            return false;
        }
        let events = state.parser.callbacks();
        if events.command_running {
            return true;
        }
        let Ok(jobs) = std::fs::read_to_string(&self.jobs) else {
            // A failed prompt job snapshot cannot prove an idle session safe.
            return true;
        };
        let pids: Vec<u32> = std::fs::read_to_string(&self.job_pids)
            .unwrap_or_default()
            .split_whitespace()
            .filter_map(|pid| pid.parse().ok())
            .collect();
        // Brush can run bundled commands as internal async jobs with no OS PID.
        // Retain their confirmation until the next authoritative prompt refresh.
        if jobs.lines().filter(|line| !line.is_empty()).count() > pids.len() {
            return true;
        }
        #[cfg(unix)]
        {
            use nix::{sys::signal::kill, unistd::Pid};
            if pids.iter().any(|pid| {
                matches!(
                    kill(Pid::from_raw(*pid as i32), None),
                    Ok(()) | Err(nix::errno::Errno::EPERM)
                )
            }) {
                return true;
            }
            self.master
                .process_group_leader()
                .is_some_and(|group| Some(group as u32) != self.shell_pid)
        }
        #[cfg(not(unix))]
        {
            // ConPTY has no foreground process group. A command or job the shell
            // started is a live child process of the shell.
            !jobs.is_empty() || self.shell_pid.is_none_or(has_child_process)
        }
    }

    /// Republish prompt preferences to this shell's private profile; the
    /// shell applies them when it next draws a prompt. An ended session has
    /// no prompt left to update.
    pub fn publish_prompt(&self, prompt: crate::prompt::PromptPreferences) -> Result<()> {
        if self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exit
            .is_some()
        {
            return Ok(());
        }
        prompt.publish(&self.profile)
    }

    pub fn send(&self, bytes: impl Into<Vec<u8>>) -> Result<()> {
        self.input
            .as_ref()
            .context("session closed")?
            .try_send(bytes.into())
            .context("terminal input queue is full or closed")
    }

    pub fn paste(&self, text: &str) -> Result<()> {
        let bracketed = self.with_screen(vt100::Screen::bracketed_paste);
        let text = text.replace('\x1b', "");
        if bracketed {
            self.send(format!("\x1b[200~{text}\x1b[201~").into_bytes())
        } else {
            self.send(text.replace("\r\n", "\n").replace('\n', "\r").into_bytes())
        }
    }

    pub fn resize(&mut self, rows: u16, cols: u16) -> Result<()> {
        let rows = rows.max(2);
        let cols = cols.max(10);
        if self.size != (rows, cols) {
            self.master.resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })?;
            self.state
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .parser
                .screen_mut()
                .set_size(rows, cols);
            self.size = (rows, cols);
        }
        Ok(())
    }

    pub fn screen(&self) -> vt100::Screen {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .parser
            .screen()
            .clone()
    }

    pub fn with_screen<R>(&self, read: impl FnOnce(&vt100::Screen) -> R) -> R {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        read(state.parser.screen())
    }

    pub fn with_screen_mut<R>(&self, change: impl FnOnce(&mut vt100::Screen) -> R) -> R {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        change(state.parser.screen_mut())
    }

    /// The terminal title the running application set, or `Terminal` when it
    /// set none. The tab's tooltip, not its label, shows the directory.
    pub fn tab_title(&self) -> String {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let title = &state.parser.callbacks().title;
        if title.is_empty() || *title == self.shell_title {
            FALLBACK_TAB_TITLE.into()
        } else {
            title.clone()
        }
    }

    pub fn cwd(&self) -> PathBuf {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .parser
            .callbacks()
            .cwd
            .clone()
            .unwrap_or_else(|| self.initial_cwd.clone())
    }

    pub fn scroll(&self, delta: isize) {
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let screen = state.parser.screen_mut();
        screen.set_scrollback(screen.scrollback().saturating_add_signed(delta));
    }

    pub fn find(&self, needle: &str) -> bool {
        if needle.is_empty() {
            return false;
        }
        let mut state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let screen = state.parser.screen_mut();
        let previous = screen.scrollback();
        screen.set_scrollback(usize::MAX);
        let maximum = screen.scrollback();
        for offset in (0..=maximum).rev() {
            screen.set_scrollback(offset);
            if screen.contents().contains(needle) {
                return true;
            }
        }
        screen.set_scrollback(previous);
        false
    }

    pub fn wait_for(&self, needle: &str, timeout: Duration) -> Result<()> {
        let start = Instant::now();
        while start.elapsed() < timeout {
            if self.with_screen(|screen| screen.contents().contains(needle)) {
                return Ok(());
            }
            std::thread::sleep(Duration::from_millis(20));
        }
        bail!(
            "timed out waiting for {needle:?}; screen:\n{}",
            self.with_screen(vt100::Screen::contents)
        )
    }
}

impl Drop for Session {
    fn drop(&mut self) {
        if self
            .state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .exit
            .is_none()
        {
            #[cfg(unix)]
            if let Some(group) = self.master.process_group_leader()
                && group > 0
                && group != nix::unistd::getpgrp().as_raw()
            {
                let group = nix::unistd::Pid::from_raw(group);
                let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGHUP);
                let _ = nix::sys::signal::killpg(group, nix::sys::signal::Signal::SIGCONT);
            }
            let _ = self.killer.kill();
        }
        self.input.take();
    }
}

/// Whether process `parent` has a live child process. When the process list
/// can't be read, the answer is yes: an unreadable state can't prove a
/// session idle.
#[cfg(windows)]
fn has_child_process(parent: u32) -> bool {
    use windows_sys::Win32::{
        Foundation::{CloseHandle, INVALID_HANDLE_VALUE},
        System::Diagnostics::ToolHelp::{
            CreateToolhelp32Snapshot, PROCESSENTRY32W, Process32FirstW, Process32NextW,
            TH32CS_SNAPPROCESS,
        },
    };
    // SAFETY: a process snapshot handle, closed below.
    let snapshot = unsafe { CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) };
    if snapshot == INVALID_HANDLE_VALUE {
        return true;
    }
    // SAFETY: an all-zero PROCESSENTRY32W is valid once its size is set.
    let mut entry: PROCESSENTRY32W = unsafe { std::mem::zeroed() };
    entry.dwSize = std::mem::size_of::<PROCESSENTRY32W>() as u32;
    let mut found = false;
    // SAFETY: `entry` outlives each call and records its own size.
    let mut more = unsafe { Process32FirstW(snapshot, &raw mut entry) } != 0;
    while more {
        if entry.th32ParentProcessID == parent && entry.th32ProcessID != parent {
            found = true;
            break;
        }
        // SAFETY: as above.
        more = unsafe { Process32NextW(snapshot, &raw mut entry) } != 0;
    }
    // SAFETY: closes the snapshot opened above.
    unsafe { CloseHandle(snapshot) };
    found
}

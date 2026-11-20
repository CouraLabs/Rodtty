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
    /// The output offset just past the bytes the parser is processing now.
    offset: u64,
    /// Commands this session's own shell reported, oldest first.
    pub commands: std::collections::VecDeque<CommandRecord>,
    next_command: u64,
    /// The secret an agent in this session presents with status updates.
    status_token: String,
    pub agent: Option<AgentReport>,
    /// Status updates refused (wrong token, stale, malformed, oversized).
    pub rejected_status: u64,
}

/// Agent status, version 1 of the local status interface.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AgentState {
    Working,
    Waiting,
    Finished,
    /// The last report expired.
    Unknown,
}

impl AgentState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Waiting => "waiting for input",
            Self::Finished => "finished",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct AgentReport {
    pub state: AgentState,
    pub sequence: u64,
    pub at: Instant,
    pub ttl: Duration,
    /// The command running when the agent reported, which it belongs to.
    pub command: Option<u64>,
}

impl AgentReport {
    /// The reported state, or Unknown once the report outlived its TTL.
    pub fn current(&self) -> AgentState {
        if self.at.elapsed() > self.ttl {
            AgentState::Unknown
        } else {
            self.state
        }
    }
}

/// Status reports default to this lifetime and may ask for up to an hour.
pub const AGENT_STATUS_TTL: Duration = Duration::from_secs(120);
const AGENT_STATUS_MAX_BYTES: usize = 256;

impl TerminalEvents {
    /// `ESC ] 777 ; rodtty-status ; 1 ; <token> ; <sequence> ; <state> [; <ttl>] BEL`
    fn agent_status(&mut self, params: &[&[u8]]) {
        let size: usize = params.iter().map(|p| p.len() + 1).sum();
        let parsed = (|| {
            if size > AGENT_STATUS_MAX_BYTES || self.status_token.is_empty() {
                return None;
            }
            let [version, token, sequence, state, rest @ ..] = params else {
                return None;
            };
            if *version != b"1" || rest.len() > 1 {
                return None;
            }
            let expected = self.status_token.as_bytes();
            if token.len() != expected.len()
                || token
                    .iter()
                    .zip(expected)
                    .fold(0u8, |a, (x, y)| a | (x ^ y))
                    != 0
            {
                return None;
            }
            let sequence: u64 = std::str::from_utf8(sequence).ok()?.parse().ok()?;
            if self.agent.is_some_and(|a| sequence <= a.sequence) {
                return None;
            }
            let state = match *state {
                b"working" => AgentState::Working,
                b"waiting" => AgentState::Waiting,
                b"finished" => AgentState::Finished,
                _ => return None,
            };
            let ttl = match rest.first() {
                Some(t) => {
                    let t: u64 = std::str::from_utf8(t).ok()?.parse().ok()?;
                    if !(1..=3600).contains(&t) {
                        return None;
                    }
                    Duration::from_secs(t)
                }
                None => AGENT_STATUS_TTL,
            };
            let command = self
                .commands
                .back()
                .filter(|c| c.ended.is_none())
                .map(|c| c.id);
            Some(AgentReport {
                state,
                sequence,
                at: Instant::now(),
                ttl,
                command,
            })
        })();
        match parsed {
            Some(report) => self.agent = Some(report),
            None => self.rejected_status += 1,
        }
    }
}

/// Commands kept per session. Their output ranges index the session's
/// [`OutputLog`], so older ones would point only at evicted bytes.
pub const COMMAND_HISTORY: usize = 512;

/// One command the session's shell ran, from its pre-command marker to the
/// prompt that followed it.
#[derive(Clone, Debug)]
pub struct CommandRecord {
    /// Unique within the session, increasing.
    pub id: u64,
    /// The directory the shell last reported before the command began.
    pub directory: Option<PathBuf>,
    pub started: Instant,
    pub started_at: std::time::SystemTime,
    pub ended: Option<Instant>,
    /// The shell's `$?` when the next prompt drew. `None` while running, or
    /// when the shell exited before reporting.
    pub exit: Option<i32>,
    /// The output range, as [`OutputLog`] offsets.
    pub output_start: u64,
    pub output_end: Option<u64>,
    /// The screen row that held the prompt and command line when the command
    /// began, for finding the command again in scrollback.
    pub prompt_line: Option<String>,
}

/// How a finished command ended, from its exit status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CommandOutcome {
    Running,
    Succeeded,
    Failed,
    /// Ended by SIGINT (status 130), as Ctrl+C reports.
    Interrupted,
    /// The shell exited before reporting a status.
    Unknown,
}

impl CommandRecord {
    pub fn outcome(&self) -> CommandOutcome {
        match (self.ended, self.exit) {
            (None, _) => CommandOutcome::Running,
            (Some(_), Some(0)) => CommandOutcome::Succeeded,
            (Some(_), Some(130)) => CommandOutcome::Interrupted,
            (Some(_), Some(_)) => CommandOutcome::Failed,
            (Some(_), None) => CommandOutcome::Unknown,
        }
    }
    pub fn elapsed(&self) -> Duration {
        self.ended.unwrap_or_else(Instant::now) - self.started
    }
}

impl TerminalEvents {
    /// Whether the shell reported a command running, between its pre-command
    /// marker and the next prompt.
    pub fn command_running(&self) -> bool {
        self.command_running
    }
    fn command_started(&mut self, prompt_line: Option<String>) {
        // A start with no prompt in between replaces nothing: nested
        // pre-command markers belong to the command already running.
        if self.commands.back().is_some_and(|c| c.ended.is_none()) {
            return;
        }
        if self.commands.len() == COMMAND_HISTORY {
            self.commands.pop_front();
        }
        self.next_command += 1;
        self.commands.push_back(CommandRecord {
            id: self.next_command,
            directory: self.cwd.clone(),
            started: Instant::now(),
            started_at: std::time::SystemTime::now(),
            ended: None,
            exit: None,
            output_start: self.offset,
            output_end: None,
            prompt_line,
        });
    }
    fn prompt_drawn(&mut self, exit: Option<i32>) {
        if let Some(command) = self.commands.back_mut()
            && command.ended.is_none()
        {
            // The command the agent reported from ended: the agent is gone.
            if self.agent.is_some_and(|a| a.command == Some(command.id)) {
                self.agent = None;
            }
            command.ended = Some(Instant::now());
            command.exit = exit;
            command.output_end = Some(self.offset);
        }
    }
    /// Close a command left running when the shell itself ended.
    fn shell_ended(&mut self) {
        self.prompt_drawn(None);
    }
}

impl vt100::Callbacks for TerminalEvents {
    fn set_window_title(&mut self, _: &mut vt100::Screen, title: &[u8]) {
        self.title = String::from_utf8_lossy(title).into_owned();
    }
    fn unhandled_osc(&mut self, screen: &mut vt100::Screen, params: &[&[u8]]) {
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
            [b"777", b"rodtty-status", rest @ ..] => self.agent_status(rest),
            [b"777", b"rodtty", tag, phase, status @ ..]
                if !self.activity_tag.is_empty() && *tag == self.activity_tag.as_bytes() =>
            {
                match *phase {
                    b"C" => {
                        self.command_running = true;
                        // The accepted command line is the row the cursor
                        // just left, or the one it is on mid-line.
                        let (row, col) = screen.cursor_position();
                        let row = if col == 0 {
                            row.checked_sub(1)
                        } else {
                            Some(row)
                        };
                        let viewed = screen.scrollback();
                        screen.set_scrollback(0);
                        let line = row
                            .filter(|_| !screen.alternate_screen())
                            .and_then(|row| screen.rows(0, screen.size().1).nth(usize::from(row)))
                            .map(|l| l.trim_end().to_owned())
                            .filter(|l| !l.is_empty());
                        screen.set_scrollback(viewed);
                        self.command_started(line);
                    }
                    b"A" => {
                        self.command_running = false;
                        let exit = status
                            .first()
                            .and_then(|s| std::str::from_utf8(s).ok())
                            .and_then(|s| s.parse().ok());
                        self.prompt_drawn(exit);
                    }
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
    /// The most recent raw output, for incremental reads by cursor.
    pub output: OutputLog,
}

/// A bounded window of a session's raw output. Cursors are byte offsets into
/// everything the session ever printed, so a reader can tell when the bytes it
/// asked for have already been evicted.
pub struct OutputLog {
    bytes: std::collections::VecDeque<u8>,
    /// The offset of `bytes[0]` in the session's whole output.
    start: u64,
    capacity: usize,
}

/// Raw output retained for incremental reads: one mebibyte per session.
pub const OUTPUT_LOG_BYTES: usize = 1 << 20;

impl OutputLog {
    pub fn new(capacity: usize) -> Self {
        Self {
            bytes: Default::default(),
            start: 0,
            capacity: capacity.max(1),
        }
    }
    pub fn push(&mut self, data: &[u8]) {
        let data = if data.len() > self.capacity {
            self.start += (data.len() - self.capacity) as u64;
            &data[data.len() - self.capacity..]
        } else {
            data
        };
        let overflow = (self.bytes.len() + data.len()).saturating_sub(self.capacity);
        self.bytes.drain(..overflow.min(self.bytes.len()));
        self.start += overflow as u64;
        self.bytes.extend(data);
    }
    /// The first retained offset.
    pub fn start(&self) -> u64 {
        self.start
    }
    /// The offset one past the last byte printed.
    pub fn end(&self) -> u64 {
        self.start + self.bytes.len() as u64
    }
    /// Up to `max` bytes from `cursor`, the cursor actually read from (later
    /// than `cursor` when part of the range was evicted), and the next cursor.
    pub fn read(&self, cursor: u64, max: usize) -> (u64, Vec<u8>, u64) {
        let from = cursor.clamp(self.start, self.end());
        let skip = (from - self.start) as usize;
        let data: Vec<u8> = self.bytes.iter().skip(skip).take(max).copied().collect();
        let next = from + data.len() as u64;
        (from, data, next)
    }
}

/// A terminal tab's label while its application has set no terminal title.
pub const FALLBACK_TAB_TITLE: &str = "Terminal";

static NEXT_SESSION_ID: AtomicUsize = AtomicUsize::new(1);

/// Whether this process ever gave a session `id`, so a missing one has ended
/// rather than never existed.
pub fn issued(id: usize) -> bool {
    id > 0 && id < NEXT_SESSION_ID.load(Ordering::Relaxed)
}

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
        // The local agent status interface: version and this session's secret.
        let status_token = crate::remote::secret(16).unwrap_or_default();
        command.env("RODTTY_AGENT_STATUS", "1");
        command.env("RODTTY_AGENT_STATUS_TOKEN", &status_token);
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
                    status_token,
                    ..Default::default()
                },
            ),
            generation: 0,
            bytes: 0,
            exit: None,
            error: None,
            output: OutputLog::new(OUTPUT_LOG_BYTES),
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
            // The start of an activity marker the last read cut off, held
            // until its end arrives so the marker's offset stays exact.
            let mut carry: Vec<u8> = Vec::new();
            loop {
                match reader.read(&mut buffer) {
                    Ok(0) => break,
                    Ok(count) => {
                        let mut state = read_state.lock().unwrap_or_else(|e| e.into_inner());
                        carry.extend_from_slice(&buffer[..count]);
                        let held = incomplete_marker(&carry).unwrap_or(carry.len());
                        let pending = carry.split_off(held);
                        let data = std::mem::replace(&mut carry, pending);
                        let count = data.len();
                        // Feed the parser up to the end of each activity
                        // marker separately, so a command's recorded output
                        // range starts and ends exactly at its markers.
                        let chunk = &data[..];
                        let mut from = 0;
                        for end in marker_ends(chunk).chain([count]) {
                            if end <= from {
                                continue;
                            }
                            let piece = &chunk[from..end];
                            state.output.push(piece);
                            let offset = state.output.end();
                            state.parser.callbacks_mut().offset = offset;
                            state.parser.process(piece);
                            from = end;
                        }
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
            state.parser.callbacks_mut().shell_ended();
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

    /// The agent status reported in this session, if any.
    pub fn agent_status(&self) -> Option<AgentState> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.parser.callbacks().agent.map(|a| a.current())
    }

    pub fn rejected_status_updates(&self) -> u64 {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.parser.callbacks().rejected_status
    }

    /// The commands this session's shell reported, oldest first.
    pub fn commands(&self) -> Vec<CommandRecord> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        state.parser.callbacks().commands.iter().cloned().collect()
    }

    /// A command's retained output without terminal control sequences, and
    /// whether all of it was still retained.
    pub fn command_output(&self, id: u64) -> Option<(String, bool)> {
        let state = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let command = state
            .parser
            .callbacks()
            .commands
            .iter()
            .find(|c| c.id == id)?;
        let end = command.output_end.unwrap_or_else(|| state.output.end());
        let max = usize::try_from(end.saturating_sub(command.output_start)).unwrap_or(usize::MAX);
        let (from, bytes, _) = state.output.read(command.output_start, max);
        let bytes = &bytes[..bytes
            .len()
            .min(usize::try_from(end - from.min(end)).unwrap_or(0))];
        Some((crate::ansi::strip(bytes), from == command.output_start))
    }

    /// Process-lifetime identity, independent of tab position and rendering.
    pub fn id(&self) -> usize {
        self.id
    }

    /// The PTY's rows and columns.
    pub fn size(&self) -> (u16, u16) {
        self.size
    }

    /// The shell's process ID, and the PTY's foreground process group leader
    /// where the platform reports one.
    pub fn processes(&self) -> (Option<u32>, Option<u32>) {
        #[cfg(unix)]
        let foreground = self
            .master
            .process_group_leader()
            .and_then(|pid| u32::try_from(pid).ok());
        #[cfg(not(unix))]
        let foreground = None;
        (self.shell_pid, foreground)
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

/// Offsets just past each complete `OSC 777;rodtty` marker in `chunk`.
/// Where an activity marker, or a prefix of one, starts at the end of
/// `chunk` without its terminator. Very long unterminated text is not held.
fn incomplete_marker(chunk: &[u8]) -> Option<usize> {
    const START: &[u8] = b"\x1b]777;rodtty;";
    const LIMIT: usize = 4096;
    let from = chunk.len().saturating_sub(LIMIT);
    let window = &chunk[from..];
    if let Some(at) = window.windows(START.len()).rposition(|w| w == START) {
        let body = &window[at + START.len()..];
        let ended = body.contains(&0x07) || body.windows(2).any(|w| w == b"\x1b\\");
        if !ended {
            return Some(from + at);
        }
    }
    // The chunk ends partway into a marker's opening bytes.
    (1..START.len().min(chunk.len() + 1))
        .rev()
        .find(|&n| chunk.ends_with(&START[..n]))
        .map(|n| chunk.len() - n)
}

#[cfg(test)]
#[test]
fn split_markers_are_held_until_complete() {
    assert_eq!(incomplete_marker(b"out\x1b]777;rod"), Some(3));
    assert_eq!(incomplete_marker(b"out\x1b]777;rodtty;tag;C"), Some(3));
    assert_eq!(incomplete_marker(b"out\x1b]777;rodtty;tag;C\x1b"), Some(3));
    assert_eq!(incomplete_marker(b"out\x1b]777;rodtty;tag;C\x07more"), None);
    assert_eq!(incomplete_marker(b"plain text"), None);
    assert_eq!(incomplete_marker(b"color \x1b[31m"), None);
}

fn marker_ends(chunk: &[u8]) -> impl Iterator<Item = usize> + '_ {
    const START: &[u8] = b"\x1b]777;rodtty;";
    let mut at = 0;
    std::iter::from_fn(move || {
        while at + START.len() <= chunk.len() {
            if chunk[at..].starts_with(START) {
                let body = at + START.len();
                let end = chunk[body..].iter().enumerate().find_map(|(i, b)| match b {
                    0x07 => Some(body + i + 1),
                    0x1b if chunk.get(body + i + 1) == Some(&b'\\') => Some(body + i + 2),
                    _ => None,
                });
                at = end.unwrap_or(chunk.len());
                if end.is_some() {
                    return end;
                }
            } else {
                at += 1;
            }
        }
        None
    })
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

//! The terminal a [`super::VtInputBackend`] edits on.

use std::io::{IsTerminal, Read};
use std::ops::{Deref, DerefMut};
use std::time::Duration;

/// What a [`super::VtInputBackend`] needs from its terminal, beyond the shell's
/// standard error, where it draws.
///
/// Hosts that can't control the terminal through the platform (e.g., a browser
/// page running the shell as WebAssembly) provide their own implementation.
pub trait Terminal: Send {
    /// Switches the terminal to raw mode for line editing: input arrives a key at
    /// a time, without being echoed or interpreted. Output is still translated as
    /// usual, so a newline (`\n`) moves to the start of the next line.
    fn enable_raw_mode(&mut self) -> std::io::Result<()>;

    /// Restores the mode the terminal was in before [`Self::enable_raw_mode`].
    fn disable_raw_mode(&mut self) -> std::io::Result<()>;

    /// Returns the terminal's width, in columns, if known.
    fn columns(&self) -> Option<usize>;

    /// Whether there's a terminal to edit on. If not, lines are read as they come,
    /// without editing. By default, whether standard input is a terminal.
    fn is_terminal(&self) -> bool {
        std::io::stdin().is_terminal()
    }

    /// Reads what the terminal sends as keys are pressed. By default, from
    /// standard input.
    ///
    /// # Arguments
    ///
    /// * `buf` - Where to put what's read.
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        std::io::stdin().read(buf)
    }

    /// Waits up to `timeout` for the terminal to send more; returns whether it did.
    /// This tells a key that sends several bytes, which may arrive separately, from
    /// keys pressed one after another. By default, it doesn't wait, and reports that
    /// nothing came.
    ///
    /// # Arguments
    ///
    /// * `timeout` - How long to wait.
    fn wait_for_input(&mut self, timeout: Duration) -> std::io::Result<bool> {
        let _ = timeout;
        Ok(false)
    }
}

/// A [`Terminal`] in raw mode, from [`Self::enable`] until [`Self::restore`]. If
/// it's dropped first (e.g., when reading a line is cancelled), the terminal's
/// mode is restored then, and any error in restoring it is ignored.
pub(crate) struct RawMode<'a, T: Terminal> {
    terminal: &'a mut T,
    restored: bool,
}

impl<'a, T: Terminal> RawMode<'a, T> {
    /// Switches `terminal` to raw mode.
    ///
    /// # Arguments
    ///
    /// * `terminal` - The terminal to switch.
    pub fn enable(terminal: &'a mut T) -> std::io::Result<Self> {
        terminal.enable_raw_mode()?;
        Ok(Self {
            terminal,
            restored: false,
        })
    }

    /// Restores the mode the terminal was in before.
    pub fn restore(mut self) -> std::io::Result<()> {
        self.terminal.disable_raw_mode()?;
        self.restored = true;
        Ok(())
    }
}

impl<T: Terminal> Deref for RawMode<'_, T> {
    type Target = T;

    fn deref(&self) -> &T {
        self.terminal
    }
}

impl<T: Terminal> DerefMut for RawMode<'_, T> {
    fn deref_mut(&mut self) -> &mut T {
        self.terminal
    }
}

impl<T: Terminal> Drop for RawMode<'_, T> {
    fn drop(&mut self) {
        if !self.restored {
            let _ = self.terminal.disable_raw_mode();
        }
    }
}

/// A [`Terminal`] for the process's own controlling terminal, controlled through
/// the platform's terminal APIs: termios on Unix, and the console API on Windows.
/// Elsewhere, [`Terminal::enable_raw_mode`] fails.
#[derive(Default)]
pub struct StdTerminal {
    #[cfg(unix)]
    mode: Option<brush_core::terminal::AutoModeGuard>,
    #[cfg(windows)]
    mode: Option<console::RawMode>,
}

impl Terminal for StdTerminal {
    #[cfg(unix)]
    fn enable_raw_mode(&mut self) -> std::io::Result<()> {
        // Restore any mode set before, so it isn't taken for the original one.
        self.mode = None;
        let mode = brush_core::terminal::AutoModeGuard::new(std::io::stdin().into())
            .map_err(std::io::Error::other)?;

        // Raw mode, as readline sets it, with Enter and Ctrl-J told apart, and a
        // read waiting for a byte; but with newlines output as CRLF pairs, as the
        // drawing expects.
        let settings = brush_core::terminal::Settings::builder()
            .raw_input(true)
            .output_nl_as_nlcr(true)
            .build();
        mode.apply_settings(&settings)
            .map_err(std::io::Error::other)?;

        self.mode = Some(mode);
        Ok(())
    }

    #[cfg(windows)]
    fn enable_raw_mode(&mut self) -> std::io::Result<()> {
        // Restore any mode set before, so it isn't taken for the original one.
        self.mode = None;
        self.mode = Some(console::RawMode::enable()?);
        Ok(())
    }

    #[cfg(not(any(unix, windows)))]
    fn enable_raw_mode(&mut self) -> std::io::Result<()> {
        // WASI has no raw mode. Fail, rather than edit with the terminal echoing keys.
        Err(std::io::Error::new(
            std::io::ErrorKind::Unsupported,
            "the terminal can't be switched to raw mode on this platform",
        ))
    }

    fn disable_raw_mode(&mut self) -> std::io::Result<()> {
        // Dropping the guard restores the original mode.
        #[cfg(any(unix, windows))]
        {
            self.mode = None;
        }
        Ok(())
    }

    #[cfg(not(windows))]
    fn columns(&self) -> Option<usize> {
        brush_core::sys::terminal::get_columns(&std::io::stdin().into())
    }

    #[cfg(windows)]
    fn columns(&self) -> Option<usize> {
        console::columns()
    }

    #[cfg(not(windows))]
    fn wait_for_input(&mut self, timeout: Duration) -> std::io::Result<bool> {
        match brush_core::sys::poll::poll_for_input(&std::io::stdin().into(), timeout) {
            // Where the platform can't wait for input, don't.
            Err(e) if e.kind() == std::io::ErrorKind::Unsupported => Ok(false),
            result => result,
        }
    }

    #[cfg(windows)]
    fn wait_for_input(&mut self, timeout: Duration) -> std::io::Result<bool> {
        console::wait_for_key(timeout)
    }
}

/// Raw mode on the Windows console: keys arrive as VT sequences, a key at a
/// time, unechoed, with Ctrl-C read as a key rather than raised as a signal.
#[cfg(windows)]
mod console {
    use std::os::windows::io::AsRawHandle;
    use std::time::{Duration, Instant};

    use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0, WAIT_TIMEOUT};
    use windows_sys::Win32::System::Console::{
        CONSOLE_SCREEN_BUFFER_INFO, ENABLE_ECHO_INPUT, ENABLE_LINE_INPUT, ENABLE_PROCESSED_INPUT,
        ENABLE_VIRTUAL_TERMINAL_INPUT, GetConsoleMode, GetConsoleScreenBufferInfo, INPUT_RECORD,
        KEY_EVENT, PeekConsoleInputW, ReadConsoleInputW, SetConsoleMode,
    };
    use windows_sys::Win32::System::Threading::WaitForSingleObject;

    fn input() -> HANDLE {
        std::io::stdin().as_raw_handle()
    }

    /// The console's input mode before raw mode, restored on drop.
    pub(super) struct RawMode {
        original: u32,
    }

    impl RawMode {
        pub(super) fn enable() -> std::io::Result<Self> {
            let mut original = 0;
            // SAFETY: `input()` is the process's console input handle and
            // `original` outlives the call.
            if unsafe { GetConsoleMode(input(), &raw mut original) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            let raw = (original
                & !(ENABLE_LINE_INPUT | ENABLE_ECHO_INPUT | ENABLE_PROCESSED_INPUT))
                | ENABLE_VIRTUAL_TERMINAL_INPUT;
            // SAFETY: as above; the mode is a plain bit mask.
            if unsafe { SetConsoleMode(input(), raw) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            Ok(Self { original })
        }
    }

    impl Drop for RawMode {
        fn drop(&mut self) {
            // SAFETY: restores the mode read from the same handle.
            let _ = unsafe { SetConsoleMode(input(), self.original) };
        }
    }

    /// The width of the console window, in columns.
    pub(super) fn columns() -> Option<usize> {
        // SAFETY: an all-zero CONSOLE_SCREEN_BUFFER_INFO is a valid value.
        let mut info: CONSOLE_SCREEN_BUFFER_INFO = unsafe { std::mem::zeroed() };
        let output = std::io::stdout().as_raw_handle();
        // SAFETY: `info` outlives the call.
        if unsafe { GetConsoleScreenBufferInfo(output, &raw mut info) } == 0 {
            return None;
        }
        usize::try_from(i32::from(info.srWindow.Right) - i32::from(info.srWindow.Left) + 1).ok()
    }

    /// Waits up to `timeout` for a key. The console's input handle is also
    /// signalled by focus, mouse and resize events, which a read skips; those
    /// are taken off the queue so they don't count as input.
    pub(super) fn wait_for_key(timeout: Duration) -> std::io::Result<bool> {
        let deadline = Instant::now() + timeout;
        loop {
            let left = deadline.saturating_duration_since(Instant::now());
            let millis = u32::try_from(left.as_millis()).unwrap_or(u32::MAX);
            // SAFETY: waits on the console input handle.
            match unsafe { WaitForSingleObject(input(), millis) } {
                WAIT_OBJECT_0 => {}
                WAIT_TIMEOUT => return Ok(false),
                _ => return Err(std::io::Error::last_os_error()),
            }
            // SAFETY: an all-zero INPUT_RECORD is a valid value.
            let mut record: INPUT_RECORD = unsafe { std::mem::zeroed() };
            let mut count = 0;
            // SAFETY: room for one record; `count` outlives the call.
            if unsafe { PeekConsoleInputW(input(), &raw mut record, 1, &raw mut count) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
            if count == 0 {
                continue;
            }
            // SAFETY: KEY_EVENT records carry a KeyEvent.
            if u32::from(record.EventType) == KEY_EVENT
                && unsafe { record.Event.KeyEvent.bKeyDown } != 0
            {
                return Ok(true);
            }
            // SAFETY: as for the peek; removes the record just seen.
            if unsafe { ReadConsoleInputW(input(), &raw mut record, 1, &raw mut count) } == 0 {
                return Err(std::io::Error::last_os_error());
            }
        }
    }
}

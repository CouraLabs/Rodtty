//! The embedded CEF browser (feature `browser`).
//!
//! Rendering is windowless (CEF off-screen rendering): each browser target
//! paints BGRA frames into memory, which Rodtty shows through a Rune
//! `NativeSurface` and keeps as the source for screenshots and recordings.
//! A target renders and takes input whether or not it is visible, and whether
//! or not Rodtty is the active application, so automation and capture never
//! need the desktop.
//!
//! Input is delivered with CEF's browser-host input API (mouse, wheel and key
//! events into one renderer), never as operating-system input.
//!
//! CEF starts on the first browser open, not at launch. Its helper processes
//! use the separate `rodtty-browser-helper` executable, so Rodtty's own
//! startup and instance coordination never see a CEF subprocess. Browsing is
//! ephemeral: an in-memory profile, nothing persisted across restarts, and
//! Chromium's sandbox and certificate checks stay on.
use super::*;
use cef::{
    BrowserSettings, CefString, ImplBrowser, ImplBrowserHost, ImplFrame, ImplRequest, WindowInfo, args::Args,
    rc::Rc as _, *,
};
// Rodtty's names win over the CEF glob import.
use super::{Appearance, El, Key, KeyEvent, Phase, State, TextEdit, Tx};
use cef::App;
use std::{
    cell::RefCell,
    collections::HashMap,
    path::{Path, PathBuf},
    sync::{
        Arc, Condvar, Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::{Duration, Instant},
};

pub(crate) mod address;
pub(crate) mod automation;
#[cfg(target_os = "macos")]
pub(crate) mod mac;
pub(crate) mod record;
mod view;
pub(crate) use view::*;

/// One painted frame, in physical pixels.
pub(crate) struct Frame {
    pub id: u64,
    pub width: u32,
    pub height: u32,
    pub bgra: Vec<u8>,
    pub scale: f32,
    pub at: Instant,
}

/// What the browser's handlers report, shared with the UI and automation.
#[derive(Default)]
pub(crate) struct Page {
    pub url: String,
    pub title: String,
    pub loading: bool,
    pub can_back: bool,
    pub can_forward: bool,
    pub error: Option<String>,
    /// Increases with every main-frame navigation; element references from
    /// an earlier document are stale.
    pub document: u64,
    pub frame: Option<Arc<Frame>>,
    pub frames: u64,
    /// Viewport in CSS pixels and the device scale factor.
    pub view: (u32, u32),
    pub scale: f32,
    pub closed: bool,
    pub crashed: Option<String>,
    pub blocked: Option<String>,
    /// DevTools protocol replies by message ID.
    pub replies: HashMap<i32, Result<serde_json::Value, String>>,
    /// The recording feed, while recording.
    pub feed: Option<record::Feed>,
    /// The agent's virtual pointer, in CSS pixels, for overlays.
    pub cursor: Option<(f32, f32)>,
}

#[derive(Clone)]
pub(crate) struct Shared {
    pub page: Arc<Mutex<Page>>,
    pub surface: rune::NativeSurface,
    pub wake: Arc<dyn Fn() + Send + Sync>,
}

impl Shared {
    pub(crate) fn lock(&self) -> std::sync::MutexGuard<'_, Page> {
        self.page.lock().unwrap_or_else(|e| e.into_inner())
    }
}

// --- Runtime -----------------------------------------------------------------

enum Runtime {
    NotStarted,
    Ready,
    Failed(String),
}

thread_local! {
    static RUNTIME: RefCell<Runtime> = const { RefCell::new(Runtime::NotStarted) };
}

/// When CEF last asked to have its message loop run.
static PUMP: Mutex<Option<Instant>> = Mutex::new(None);
static PUMP_SIGNAL: Condvar = Condvar::new();
static PUMP_STARTED: AtomicBool = AtomicBool::new(false);
/// Whether any browser is alive: then the loop also runs on a short tick, so
/// background targets keep painting.
static ACTIVE: AtomicBool = AtomicBool::new(false);

fn schedule_pump(delay_ms: i64) {
    let at = Instant::now() + Duration::from_millis(delay_ms.clamp(0, 1000) as u64);
    let mut next = PUMP.lock().unwrap_or_else(|e| e.into_inner());
    if next.is_none_or(|n| at < n) {
        *next = Some(at);
    }
    PUMP_SIGNAL.notify_all();
}

fn start_pump_thread(handle: rune::AppHandle<State>) {
    if PUMP_STARTED.swap(true, Ordering::SeqCst) {
        return;
    }
    std::thread::Builder::new()
        .name("rodtty-cef-pump".into())
        .spawn(move || {
            let mut next = PUMP.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                let tick = if ACTIVE.load(Ordering::Relaxed) { Duration::from_millis(16) } else { Duration::from_secs(1) };
                let due = next.unwrap_or_else(|| Instant::now() + tick);
                let now = Instant::now();
                if due > now {
                    next = PUMP_SIGNAL.wait_timeout(next, due - now).unwrap_or_else(|e| e.into_inner()).0;
                    if next.is_some_and(|n| n > Instant::now()) {
                        continue;
                    }
                }
                *next = None;
                drop(next);
                handle.update_if(|s, _| s.pump_browser());
                next = PUMP.lock().unwrap_or_else(|e| e.into_inner());
            }
        })
        .ok();
}

/// Where CEF's framework or library is, beside the running executable.
fn runtime_location() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no executable folder")?;
    #[cfg(target_os = "macos")]
    let path = dir.join("../Frameworks/Chromium Embedded Framework.framework/Chromium Embedded Framework");
    #[cfg(target_os = "windows")]
    let path = dir.join("libcef.dll");
    #[cfg(target_os = "linux")]
    let path = dir.join("libcef.so");
    if path.exists() {
        Ok(path)
    } else {
        Err("This Rodtty build has no browser runtime beside it. Use a package that includes the embedded browser.".into())
    }
}

/// Whether this build can offer the browser: compiled in and with its
/// runtime present. Initialization itself is deferred to the first open.
pub(crate) fn available() -> bool {
    runtime_location().is_ok() && RUNTIME.with_borrow(|r| !matches!(r, Runtime::Failed(_)))
}

fn helper_path() -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let dir = exe.parent().ok_or("no executable folder")?;
    #[cfg(target_os = "macos")]
    let helper = dir.join("../Frameworks/Rodtty Helper.app/Contents/MacOS/Rodtty Helper");
    #[cfg(target_os = "windows")]
    let helper = dir.join("rodtty-browser-helper.exe");
    #[cfg(target_os = "linux")]
    let helper = dir.join("rodtty-browser-helper");
    if helper.exists() { Ok(helper) } else { Err(format!("The browser helper is missing: {}", helper.display())) }
}

wrap_browser_process_handler! {
    struct ProcessHandler;
    impl BrowserProcessHandler {
        fn on_schedule_message_pump_work(&self, delay_ms: i64) {
            schedule_pump(delay_ms);
        }
    }
}

wrap_app! {
    struct BrowserApp;
    impl App {
        fn on_before_command_line_processing(&self, _process_type: Option<&CefString>, command_line: Option<&mut CommandLine>) {
            if let Some(command_line) = command_line {
                // Never touch the owner's keychain for Chromium's own storage.
                command_line.append_switch(Some(&"use-mock-keychain".into()));
                command_line.append_switch(Some(&"disable-background-networking".into()));
                command_line.append_switch(Some(&"no-first-run".into()));
            }
        }
        fn browser_process_handler(&self) -> Option<BrowserProcessHandler> {
            Some(ProcessHandler::new())
        }
    }
}

impl State {
    /// Start CEF if it hasn't started. Errors stay recoverable: the terminal
    /// and editor are unaffected and the next open can try again only after
    /// Rodtty restarts, since CEF can initialize once per process.
    pub(crate) fn ensure_browser_runtime(&mut self) -> Result<(), String> {
        if let Some(error) = RUNTIME.with_borrow(|r| match r {
            Runtime::Ready => Some(None),
            Runtime::Failed(e) => Some(Some(e.clone())),
            Runtime::NotStarted => None,
        }) {
            return error.map_or(Ok(()), Err);
        }
        let result = (|| -> Result<(), String> {
            let library = runtime_location()?;
            #[cfg(target_os = "macos")]
            {
                use std::os::unix::ffi::OsStrExt;
                let name = std::ffi::CString::new(library.as_os_str().as_bytes()).map_err(|e| e.to_string())?;
                if load_library(Some(unsafe { &*name.as_ptr() })) != 1 {
                    return Err("Could not load the browser runtime".into());
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = library;
            let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
            let cache = self.root.join("browser").join(format!("profile-{}", std::process::id()));
            let _ = std::fs::remove_dir_all(&cache);
            std::fs::create_dir_all(&cache).map_err(|e| e.to_string())?;
            let settings = Settings {
                windowless_rendering_enabled: 1,
                external_message_pump: 1,
                no_sandbox: 0,
                command_line_args_disabled: 1,
                browser_subprocess_path: CefString::from(helper_path()?.to_string_lossy().as_ref()),
                root_cache_path: CefString::from(cache.to_string_lossy().as_ref()),
                // An empty cache path is an in-memory profile.
                cache_path: CefString::from(""),
                persist_session_cookies: 0,
                log_severity: LogSeverity::DISABLE,
                ..Default::default()
            };
            let args = Args::new();
            let mut app = BrowserApp::new();
            if initialize(Some(args.as_main_args()), Some(&settings), Some(&mut app), std::ptr::null_mut()) != 1 {
                return Err("The browser runtime failed to start".into());
            }
            Ok(())
        })();
        RUNTIME.with_borrow_mut(|r| {
            *r = match &result {
                Ok(()) => Runtime::Ready,
                Err(e) => Runtime::Failed(e.clone()),
            }
        });
        if result.is_ok()
            && let Some(handle) = self.handle.clone()
        {
            start_pump_thread(handle);
        }
        result
    }

    /// Run CEF's message loop once, when it is running.
    pub(crate) fn pump_browser(&mut self) -> bool {
        if !RUNTIME.with_borrow(|r| matches!(r, Runtime::Ready)) {
            return false;
        }
        do_message_loop_work();
        ACTIVE.store(!self.browsers.targets.is_empty(), Ordering::Relaxed);
        self.browser_after_pump()
    }

    /// Close every browser and stop CEF, for Quit.
    pub(crate) fn shutdown_browsers(&mut self) {
        if !RUNTIME.with_borrow(|r| matches!(r, Runtime::Ready)) {
            return;
        }
        for target in &mut self.browsers.targets {
            if let Some(mut recording) = target.recording.take() {
                recording.stop("Rodtty quit", true);
            }
            if let Some(host) = target.browser.host() {
                host.close_browser(1);
            }
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        while Instant::now() < deadline && self.browsers.targets.iter().any(|t| !t.shared.lock().closed) {
            do_message_loop_work();
            std::thread::sleep(Duration::from_millis(10));
        }
        self.browsers.targets.clear();
        for _ in 0..20 {
            do_message_loop_work();
        }
        shutdown();
        RUNTIME.with_borrow_mut(|r| *r = Runtime::NotStarted);
        let _ = std::fs::remove_dir_all(self.root.join("browser").join(format!("profile-{}", std::process::id())));
    }
}

// --- Handlers ----------------------------------------------------------------

fn cef_text(value: Option<&CefString>) -> String {
    value.map(|v| v.to_string()).unwrap_or_default()
}

fn is_main(frame: &Option<&mut cef::Frame>) -> bool {
    frame.as_ref().is_some_and(|f| f.is_main() == 1)
}

wrap_render_handler! {
    struct Render { shared: Shared }
    impl RenderHandler {
        fn view_rect(&self, _browser: Option<&mut Browser>, rect: Option<&mut Rect>) {
            if let Some(rect) = rect {
                let (w, h) = self.shared.lock().view;
                rect.x = 0;
                rect.y = 0;
                rect.width = w.max(1) as i32;
                rect.height = h.max(1) as i32;
            }
        }
        fn screen_info(&self, _browser: Option<&mut Browser>, info: Option<&mut ScreenInfo>) -> ::std::os::raw::c_int {
            let Some(info) = info else { return 0 };
            let page = self.shared.lock();
            info.device_scale_factor = page.scale.max(1.0);
            let (w, h) = page.view;
            info.rect = Rect { x: 0, y: 0, width: w as i32, height: h as i32 };
            info.available_rect = info.rect.clone();
            1
        }
        fn on_paint(&self, _browser: Option<&mut Browser>, type_: PaintElementType, _dirty_rects: Option<&[Rect]>, buffer: *const u8, width: ::std::os::raw::c_int, height: ::std::os::raw::c_int) {
            if type_ != PaintElementType::VIEW || buffer.is_null() || width <= 0 || height <= 0 {
                return;
            }
            let (w, h) = (width as u32, height as u32);
            let bgra = unsafe { std::slice::from_raw_parts(buffer, (w * h * 4) as usize) }.to_vec();
            let mut page = self.shared.lock();
            page.frames += 1;
            let frame = Arc::new(Frame { id: page.frames, width: w, height: h, bgra, scale: page.scale.max(1.0), at: Instant::now() });
            let _ = self.shared.surface.push_bytes(
                rune::native_surface::ByteFrame {
                    width: w,
                    height: h,
                    format: rune::native_surface::Format::Bgra,
                    color: Default::default(),
                    planes: [frame.bgra.clone(), Vec::new()],
                },
                frame.id,
            );
            if let Some(feed) = &page.feed {
                *feed.latest.lock().unwrap_or_else(|e| e.into_inner()) = Some(frame.clone());
                let cursor = page.cursor.map(|(x, y)| (x * frame.scale, y * frame.scale));
                *feed.cursor.lock().unwrap_or_else(|e| e.into_inner()) = cursor;
            }
            page.frame = Some(frame);
            drop(page);
            (self.shared.wake)();
        }
    }
}

wrap_load_handler! {
    struct Load { shared: Shared }
    impl LoadHandler {
        fn on_loading_state_change(&self, _browser: Option<&mut Browser>, is_loading: ::std::os::raw::c_int, can_go_back: ::std::os::raw::c_int, can_go_forward: ::std::os::raw::c_int) {
            let mut page = self.shared.lock();
            page.loading = is_loading == 1;
            page.can_back = can_go_back == 1;
            page.can_forward = can_go_forward == 1;
            drop(page);
            (self.shared.wake)();
        }
        fn on_load_start(&self, _browser: Option<&mut Browser>, frame: Option<&mut cef::Frame>, _transition_type: TransitionType) {
            if is_main(&frame) {
                let mut page = self.shared.lock();
                page.document += 1;
                page.error = None;
            }
        }
        fn on_load_error(&self, _browser: Option<&mut Browser>, frame: Option<&mut cef::Frame>, error_code: Errorcode, error_text: Option<&CefString>, failed_url: Option<&CefString>) {
            let code = format!("{error_code:?}");
            // An aborted load is a navigation that something replaced.
            if !is_main(&frame) || code.contains("ABORTED") {
                return;
            }
            let mut page = self.shared.lock();
            page.error = Some(format!("{} ({})", cef_text(error_text), cef_text(failed_url)));
            drop(page);
            (self.shared.wake)();
        }
    }
}

wrap_display_handler! {
    struct Display { shared: Shared }
    impl DisplayHandler {
        fn on_address_change(&self, _browser: Option<&mut Browser>, frame: Option<&mut cef::Frame>, url: Option<&CefString>) {
            if is_main(&frame) {
                self.shared.lock().url = cef_text(url);
                (self.shared.wake)();
            }
        }
        fn on_title_change(&self, _browser: Option<&mut Browser>, title: Option<&CefString>) {
            self.shared.lock().title = cef_text(title);
            (self.shared.wake)();
        }
    }
}

wrap_request_handler! {
    struct Requests { shared: Shared }
    impl RequestHandler {
        fn on_before_browse(&self, _browser: Option<&mut Browser>, frame: Option<&mut cef::Frame>, request: Option<&mut Request>, _user_gesture: ::std::os::raw::c_int, _is_redirect: ::std::os::raw::c_int) -> ::std::os::raw::c_int {
            let url = request.map(|r| CefString::from(&r.url()).to_string()).unwrap_or_default();
            if is_main(&frame) && !address::allowed_navigation(&url) {
                let scheme = url.split(':').next().unwrap_or_default().to_owned();
                self.shared.lock().blocked = Some(format!("Blocked a {scheme}: navigation; only http and https pages open here"));
                (self.shared.wake)();
                return 1;
            }
            0
        }
        fn on_certificate_error(&self, _browser: Option<&mut Browser>, cert_error: Errorcode, request_url: Option<&CefString>, _ssl_info: Option<&mut Sslinfo>, _callback: Option<&mut Callback>) -> ::std::os::raw::c_int {
            // Returning 0 cancels the request: certificate errors are never bypassed.
            self.shared.lock().error = Some(format!("Certificate error {cert_error:?} for {}", cef_text(request_url)));
            (self.shared.wake)();
            0
        }
        fn on_render_process_terminated(&self, _browser: Option<&mut Browser>, status: TerminationStatus, _error_code: ::std::os::raw::c_int, _error_string: Option<&CefString>) {
            self.shared.lock().crashed = Some(format!("The page's renderer stopped ({status:?}). Reload to recover."));
            (self.shared.wake)();
        }
    }
}

wrap_life_span_handler! {
    struct Life { shared: Shared }
    impl LifeSpanHandler {
        fn on_before_popup(&self, _browser: Option<&mut Browser>, _frame: Option<&mut cef::Frame>, _popup_id: ::std::os::raw::c_int, target_url: Option<&CefString>, _target_frame_name: Option<&CefString>, _target_disposition: WindowOpenDisposition, _user_gesture: ::std::os::raw::c_int, _popup_features: Option<&PopupFeatures>, _window_info: Option<&mut WindowInfo>, _client: Option<&mut Option<Client>>, _settings: Option<&mut BrowserSettings>, _extra_info: Option<&mut Option<DictionaryValue>>, _no_javascript_access: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            let url = cef_text(target_url);
            let host = url.split('/').nth(2).unwrap_or("a page").to_owned();
            self.shared.lock().blocked = Some(format!("Blocked a new window for {host}"));
            (self.shared.wake)();
            1
        }
        fn on_before_close(&self, _browser: Option<&mut Browser>) {
            self.shared.lock().closed = true;
            (self.shared.wake)();
        }
    }
}

wrap_jsdialog_handler! {
    struct Dialogs { shared: Shared }
    impl JsdialogHandler {
        fn on_jsdialog(&self, _browser: Option<&mut Browser>, _origin_url: Option<&CefString>, _dialog_type: JsdialogType, _message_text: Option<&CefString>, _default_prompt_text: Option<&CefString>, _callback: Option<&mut JsdialogCallback>, suppress_message: Option<&mut ::std::os::raw::c_int>) -> ::std::os::raw::c_int {
            if let Some(suppress) = suppress_message {
                *suppress = 1;
            }
            self.shared.lock().blocked = Some("Suppressed a JavaScript dialog".into());
            0
        }
    }
}

wrap_download_handler! {
    struct Downloads { shared: Shared }
    impl DownloadHandler {
        fn can_download(&self, _browser: Option<&mut Browser>, _url: Option<&CefString>, _request_method: Option<&CefString>) -> ::std::os::raw::c_int {
            self.shared.lock().blocked = Some("Downloads are blocked in the embedded browser".into());
            0
        }
    }
}

wrap_client! {
    struct PageClient { shared: Shared }
    impl Client {
        fn render_handler(&self) -> Option<RenderHandler> { Some(Render::new(self.shared.clone())) }
        fn load_handler(&self) -> Option<LoadHandler> { Some(Load::new(self.shared.clone())) }
        fn display_handler(&self) -> Option<DisplayHandler> { Some(Display::new(self.shared.clone())) }
        fn request_handler(&self) -> Option<RequestHandler> { Some(Requests::new(self.shared.clone())) }
        fn life_span_handler(&self) -> Option<LifeSpanHandler> { Some(Life::new(self.shared.clone())) }
        fn jsdialog_handler(&self) -> Option<JsdialogHandler> { Some(Dialogs::new(self.shared.clone())) }
        fn download_handler(&self) -> Option<DownloadHandler> { Some(Downloads::new(self.shared.clone())) }
    }
}

wrap_dev_tools_message_observer! {
    struct DevTools { shared: Shared }
    impl DevToolsMessageObserver {
        fn on_dev_tools_method_result(&self, _browser: Option<&mut Browser>, message_id: ::std::os::raw::c_int, success: ::std::os::raw::c_int, result: Option<&[u8]>) {
            let parsed = result
                .and_then(|r| serde_json::from_slice::<serde_json::Value>(r).ok())
                .unwrap_or(serde_json::Value::Null);
            let reply = if success == 1 { Ok(parsed) } else { Err(parsed.to_string()) };
            self.shared.lock().replies.insert(message_id, reply);
        }
    }
}

// --- Targets -----------------------------------------------------------------

pub(crate) struct BrowserTarget {
    pub id: String,
    /// The client that created it; `None` for the owner's Browser view.
    pub client: Option<String>,
    pub browser: cef::Browser,
    pub shared: Shared,
    _devtools: Option<Registration>,
    pub recording: Option<record::Recording>,
    pub next_message: i32,
    /// Element references from the last snapshot: (document, index, fingerprint).
    pub refs: HashMap<String, (u64, usize, String)>,
    pub focused: bool,
}

#[derive(Default)]
pub(crate) struct Browsers {
    pub targets: Vec<BrowserTarget>,
    /// Whether the Browser view is open, and the target it shows.
    pub view_open: bool,
    pub shown: Option<String>,
    /// Whether the page (not the address field) has the owner's keyboard.
    pub page_focus: bool,
    pub address: Option<TextEdit>,
    pub status: Option<String>,
    /// The owner's view size in points, from layout.
    pub view_size: Option<(f32, f32)>,
    pub view_scale: f32,
    /// Last held owner mouse button, for drags.
    pub owner_button: Option<MouseButtonType>,
    /// IDs of browser targets that closed, so their IDs report closed.
    pub closed: std::collections::HashSet<String>,
    /// Stopped recordings whose encoder is still writing the file.
    pub finalizing: Vec<record::Recording>,
    pub finished_recordings: Vec<record::Status>,
}

pub(crate) const MOD_SHIFT: u32 = 1 << 1;
pub(crate) const MOD_CONTROL: u32 = 1 << 2;
pub(crate) const MOD_ALT: u32 = 1 << 3;
pub(crate) const MOD_LEFT: u32 = 1 << 4;
pub(crate) const MOD_MIDDLE: u32 = 1 << 5;
pub(crate) const MOD_RIGHT: u32 = 1 << 6;
pub(crate) const MOD_COMMAND: u32 = 1 << 7;

impl BrowserTarget {
    pub(crate) fn host(&self) -> Option<BrowserHost> {
        self.browser.host()
    }

    pub(crate) fn navigate(&self, url: &str) {
        if let Some(frame) = self.browser.main_frame() {
            frame.load_url(Some(&CefString::from(url)));
        }
    }

    pub(crate) fn resize(&self, width: u32, height: u32, scale: f32) {
        let mut page = self.shared.lock();
        if page.view == (width, height) && (page.scale - scale).abs() < f32::EPSILON {
            return;
        }
        let rescaled = (page.scale - scale).abs() >= f32::EPSILON;
        page.view = (width, height);
        page.scale = scale;
        drop(page);
        if let Some(host) = self.host() {
            if rescaled {
                host.notify_screen_info_changed();
            }
            host.was_resized();
        }
    }

    /// Send a DevTools protocol command; the reply arrives in `Page::replies`.
    pub(crate) fn devtools(&mut self, method: &str, params: serde_json::Value) -> Option<i32> {
        self.next_message += 1;
        let id = self.next_message;
        let message = serde_json::json!({"id": id, "method": method, "params": params}).to_string();
        let sent = self.host()?.send_dev_tools_message(Some(message.as_bytes()));
        (sent == 1).then_some(id)
    }

    pub(crate) fn mouse_move(&self, x: f32, y: f32, modifiers: u32) {
        if let Some(host) = self.host() {
            host.send_mouse_move_event(Some(&MouseEvent { x: x as i32, y: y as i32, modifiers }), 0);
        }
    }

    pub(crate) fn mouse_button(&self, x: f32, y: f32, button: MouseButtonType, up: bool, count: i32, modifiers: u32) {
        if let Some(host) = self.host() {
            if !up {
                host.set_focus(1);
            }
            host.send_mouse_click_event(Some(&MouseEvent { x: x as i32, y: y as i32, modifiers }), button, up as i32, count.max(1));
        }
    }

    pub(crate) fn wheel(&self, x: f32, y: f32, dx: f32, dy: f32, modifiers: u32) {
        if let Some(host) = self.host() {
            host.send_mouse_wheel_event(Some(&MouseEvent { x: x as i32, y: y as i32, modifiers }), dx as i32, dy as i32);
        }
    }

    pub(crate) fn key(&self, event: cef::KeyEvent) {
        if let Some(host) = self.host() {
            host.set_focus(1);
            host.send_key_event(Some(&event));
        }
    }
}

/// Key codes for a named key: (Windows virtual key, macOS virtual key, character).
pub(crate) fn key_codes(key: &crate::automation::keys::KeyName) -> Option<(i32, i32, Option<char>)> {
    use crate::automation::keys::KeyName::*;
    Some(match key {
        Enter => (0x0D, 0x24, Some('\r')),
        Tab => (0x09, 0x30, Some('\t')),
        Backspace => (0x08, 0x33, Some('\u{8}')),
        Escape => (0x1B, 0x35, Some('\u{1b}')),
        Space => (0x20, 0x31, Some(' ')),
        Left => (0x25, 0x7B, None),
        Up => (0x26, 0x7E, None),
        Right => (0x27, 0x7C, None),
        Down => (0x28, 0x7D, None),
        Delete => (0x2E, 0x75, None),
        Home => (0x24, 0x73, None),
        End => (0x23, 0x77, None),
        PageUp => (0x21, 0x74, None),
        PageDown => (0x22, 0x79, None),
        Insert => (0x2D, 0x72, None),
        Function(n) => (0x6F + i32::from(*n), [0x7A, 0x78, 0x63, 0x76, 0x60, 0x61, 0x62, 0x64, 0x65, 0x6D, 0x67, 0x6F].get(usize::from(*n) - 1).copied().unwrap_or(0), None),
        Char(c) => {
            let lower = c.to_ascii_lowercase();
            let mac = "asdfhgzxcv\0bqweryt123465=97-80]ou[ip\0lj'k;\\,/nm.".find(lower).map_or(-1, |i| i as i32);
            let vk = if lower.is_ascii_alphanumeric() { lower.to_ascii_uppercase() as i32 } else { 0 };
            (vk, mac, Some(*c))
        }
    })
}

/// Key events for one chord: key down, the typed character, key up.
pub(crate) fn key_events(chord: &crate::automation::keys::Chord, down: bool, up: bool) -> Vec<cef::KeyEvent> {
    let m = chord.mods;
    let mut modifiers = 0;
    if m.shift { modifiers |= MOD_SHIFT; }
    if m.ctrl { modifiers |= MOD_CONTROL; }
    if m.alt { modifiers |= MOD_ALT; }
    if m.meta { modifiers |= MOD_COMMAND; }
    let Some((vk, native, character)) = key_codes(&chord.key) else { return Vec::new() };
    let unicode = |c: char| -> u16 {
        let mut buf = [0u16; 2];
        c.encode_utf16(&mut buf);
        buf[0]
    };
    let base = cef::KeyEvent {
        size: std::mem::size_of::<sys::cef_key_event_t>(),
        modifiers,
        windows_key_code: vk,
        native_key_code: native,
        ..Default::default()
    };
    let mut events = Vec::new();
    let shortcut = m.ctrl || m.meta;
    if down {
        let character = character.map(|c| if m.shift { c.to_uppercase().next().unwrap_or(c) } else { c });
        events.push(cef::KeyEvent {
            type_: KeyEventType::RAWKEYDOWN,
            character: character.map_or(0, unicode),
            unmodified_character: character.map_or(0, unicode),
            ..base.clone()
        });
        if let Some(c) = character
            && !shortcut
            && (!c.is_control() || c == '\r')
        {
            events.push(cef::KeyEvent {
                type_: KeyEventType::CHAR,
                character: unicode(c),
                unmodified_character: unicode(c),
                ..base.clone()
            });
        }
    }
    if up {
        events.push(cef::KeyEvent { type_: KeyEventType::KEYUP, ..base });
    }
    events
}

/// Character events for text with no key of its own (accents, CJK, emoji).
pub(crate) fn char_events(c: char) -> Vec<cef::KeyEvent> {
    let mut buf = [0u16; 2];
    c.encode_utf16(&mut buf)
        .iter()
        .map(|unit| cef::KeyEvent {
            size: std::mem::size_of::<sys::cef_key_event_t>(),
            type_: KeyEventType::CHAR,
            character: *unit,
            unmodified_character: *unit,
            ..Default::default()
        })
        .collect()
}

/// Draw the virtual pointer (an outlined arrow) into RGB pixels.
pub(crate) fn draw_cursor_rgb(rgb: &mut [u8], width: u32, height: u32, x: f32, y: f32, channels: usize) {
    let size = 18i32;
    for dy in 0..size {
        for dx in 0..=dy / 2 + 1 {
            let (px, py) = (x as i32 + dx, y as i32 + dy);
            if px < 0 || py < 0 || px as u32 >= width || py as u32 >= height || dy > size - 4 && dx > 2 {
                continue;
            }
            let edge = dx == 0 || dx == dy / 2 + 1 || dy == size - 1;
            let color = if edge { [255, 255, 255] } else { [220, 30, 120] };
            let i = (py as usize * width as usize + px as usize) * channels;
            rgb[i..i + 3].copy_from_slice(&color);
        }
    }
}

impl State {
    /// Create a browser target. `client` is the automation client that owns
    /// it, or `None` for the owner's Browser view.
    pub(crate) fn create_browser(&mut self, client: Option<String>, url: Option<&str>, size: (u32, u32), scale: f32) -> Result<String, String> {
        self.ensure_browser_runtime()?;
        let id = format!("brw_{}_{}", self.automation.instance, crate::automation::token()[..8].to_owned());
        let handle = self.handle.clone();
        let wake: Arc<dyn Fn() + Send + Sync> = match handle {
            Some(handle) => Arc::new(move || handle.wake()),
            None => self.wake.clone(),
        };
        let shared = Shared {
            page: Arc::new(Mutex::new(Page { view: size, scale, url: url.unwrap_or("about:blank").to_owned(), ..Default::default() })),
            surface: rune::NativeSurface::new(),
            wake,
        };
        let window_info = WindowInfo { windowless_rendering_enabled: 1, ..Default::default() };
        let settings = BrowserSettings { windowless_frame_rate: 30, ..Default::default() };
        let mut client_handler = PageClient::new(shared.clone());
        // A dedicated in-memory context per target keeps agent browsing and
        // the owner's browsing apart.
        let mut context = request_context_create_context(Some(&RequestContextSettings::default()), None);
        let browser = browser_host_create_browser_sync(
            Some(&window_info),
            Some(&mut client_handler),
            Some(&CefString::from(url.unwrap_or("about:blank"))),
            Some(&settings),
            None,
            context.as_mut(),
        )
        .ok_or("The browser could not be created")?;
        let mut observer = DevTools::new(shared.clone());
        let registration = browser.host().and_then(|h| h.add_dev_tools_message_observer(Some(&mut observer)));
        self.browsers.targets.push(BrowserTarget {
            id: id.clone(),
            client,
            browser,
            shared,
            _devtools: registration,
            recording: None,
            next_message: 0,
            refs: HashMap::new(),
            focused: false,
        });
        ACTIVE.store(true, Ordering::Relaxed);
        schedule_pump(0);
        self.emit("browser", Some(&id), serde_json::json!({"state": "created"}));
        Ok(id)
    }

    pub(crate) fn browser_index(&self, id: &str) -> Option<usize> {
        self.browsers.targets.iter().position(|t| t.id == id)
    }

    pub(crate) fn close_browser_target(&mut self, id: &str) -> Option<record::Status> {
        let index = self.browser_index(id)?;
        let mut target = self.browsers.targets.remove(index);
        let finished = target.recording.take().map(|mut r| {
            let status = r.stop("browser closed", false);
            self.browsers.finalizing.push(r);
            status
        });
        if let Some(host) = target.host() {
            host.close_browser(1);
        }
        if self.browsers.shown.as_deref() == Some(id) {
            self.browsers.shown = None;
        }
        self.automation.shared.bump(id);
        self.automation.agent_focus.remove(id);
        self.emit("browser", Some(id), serde_json::json!({"state": "closed"}));
        finished
    }

    /// After a pump: emit navigation events and retire finished recordings.
    fn browser_after_pump(&mut self) -> bool {
        let mut changed = false;
        for target in &mut self.browsers.targets {
            if target.recording.as_ref().is_some_and(record::Recording::finished)
                && let Some(recording) = target.recording.take()
            {
                target.shared.lock().feed = None;
                let status = recording.status();
                self.automation.shared.hub.emit("recording", Some(&recording.id), serde_json::json!(status));
                self.browsers.finished_recordings.push(status);
                changed = true;
            }
        }
        let (done, pending): (Vec<_>, Vec<_>) = std::mem::take(&mut self.browsers.finalizing).into_iter().partition(record::Recording::finished);
        self.browsers.finalizing = pending;
        for recording in done {
            let status = recording.status();
            self.automation.shared.hub.emit("recording", Some(&recording.id), serde_json::json!(status));
            self.browsers.finished_recordings.push(status);
            changed = true;
        }
        changed
    }

    #[cfg(feature = "browser")]
    pub(crate) fn recording_indicator(&self) -> Option<String> {
        let recording = self.browsers.targets.iter().filter(|t| t.recording.is_some()).count();
        (recording > 0).then(|| format!("● Recording browser ({recording})"))
    }

    pub(crate) fn browser_targets_for(&self, grant: &crate::automation::grants::ClientGrant) -> Vec<String> {
        self.browsers
            .targets
            .iter()
            .filter(|t| t.client.as_deref() == Some(grant.id.as_str()))
            .map(|t| t.id.clone())
            .collect()
    }
}

/// The browser's artifacts folder.
pub(crate) fn artifacts(root: &Path) -> PathBuf {
    root.join("artifacts")
}

//! Browser tools for automation clients: targets, navigation, page
//! snapshots with element references, virtual-input actions, waits,
//! screenshots and recordings. Page content is returned as data; nothing a
//! page does reaches Rodtty's terminals, editor, files or settings.
use super::*;
use crate::automation::{
    ErrorCode, ToolError, ToolResult, arg_bool, arg_f64, arg_opt_str, arg_str, arg_u64,
    grants::ClientGrant,
};
use crate::desktop::automation::{Host, on_ui};
use serde_json::{Value, json};

/// Collects visible, interactive or labelled elements in document order.
/// The same walk resolves a reference later: index plus a fingerprint of
/// role and name, so a changed page reports a stale reference instead of
/// acting on a different element.
const WALK: &str = r#"
(() => {
  const selector = 'a[href],button,input,select,textarea,summary,[role],[contenteditable=""],[contenteditable="true"],[tabindex],h1,h2,h3,label,img[alt]';
  const implicit = (e) => {
    const t = e.tagName.toLowerCase();
    if (t === 'a') return 'link';
    if (t === 'button' || t === 'summary') return 'button';
    if (t === 'select') return 'combobox';
    if (t === 'textarea') return 'textbox';
    if (t === 'img') return 'img';
    if (t === 'label') return 'label';
    if (/^h[1-6]$/.test(t)) return 'heading';
    if (t === 'input') {
      const ty = (e.getAttribute('type') || 'text').toLowerCase();
      return ({checkbox:'checkbox', radio:'radio', submit:'button', button:'button', reset:'button', range:'slider', search:'searchbox', email:'textbox', password:'textbox', tel:'textbox', url:'textbox', number:'spinbutton', text:'textbox'})[ty] || 'textbox';
    }
    if (e.isContentEditable) return 'textbox';
    return 'generic';
  };
  const clean = (s) => (s || '').replace(/\s+/g, ' ').trim().slice(0, 120);
  const name = (e) => {
    const label = e.getAttribute('aria-label');
    if (label) return clean(label);
    const by = e.getAttribute('aria-labelledby');
    if (by) return clean(by.split(/\s+/).map(id => document.getElementById(id)?.innerText || '').join(' '));
    if (e.labels && e.labels.length) return clean([...e.labels].map(l => l.innerText).join(' '));
    if (e.tagName === 'INPUT' && ['submit','button','reset'].includes(e.type)) return clean(e.value);
    if (e.getAttribute('placeholder')) return clean(e.getAttribute('placeholder'));
    if (e.getAttribute('alt')) return clean(e.getAttribute('alt'));
    if (e.getAttribute('title')) return clean(e.getAttribute('title'));
    if (['INPUT','SELECT','TEXTAREA'].includes(e.tagName)) return clean(e.getAttribute('name'));
    return clean(e.innerText);
  };
  const out = [];
  for (const e of document.querySelectorAll(selector)) {
    const r = e.getBoundingClientRect();
    const s = getComputedStyle(e);
    if ((r.width === 0 && r.height === 0) || s.visibility === 'hidden' || s.display === 'none') continue;
    const role = e.getAttribute('role') || implicit(e);
    out.push({e, role, name: name(e), r});
  }
  return out;
})()
"#;

fn snapshot_script(max: usize) -> String {
    format!(
        r#"(() => {{
  const all = {WALK};
  const elements = all.slice(0, {max}).map((x, i) => {{
    const e = x.e;
    const value = (e.type === 'password') ? undefined : (('value' in e && typeof e.value === 'string') ? e.value.slice(0, 200) : undefined);
    return {{index: i, role: x.role, name: x.name, tag: e.tagName.toLowerCase(), type: e.getAttribute('type') || undefined,
      value, checked: ('checked' in e && (e.type === 'checkbox' || e.type === 'radio')) ? e.checked : undefined,
      disabled: e.disabled || undefined, focused: document.activeElement === e || undefined,
      bounds: {{x: Math.round(x.r.x), y: Math.round(x.r.y), width: Math.round(x.r.width), height: Math.round(x.r.height)}}}};
  }});
  return {{url: location.href, title: document.title, total: all.length, elements,
    text: (document.body ? document.body.innerText : '').slice(0, 4000),
    scroll: {{x: scrollX, y: scrollY, width: document.documentElement.scrollWidth, height: document.documentElement.scrollHeight}},
    viewport: {{width: innerWidth, height: innerHeight}}, ready: document.readyState}};
}})()"#
    )
}

fn locate_script(index: usize, scroll: bool) -> String {
    format!(
        r#"(() => {{
  const x = ({WALK})[{index}];
  if (!x) return null;
  {scroll}
  const r = x.e.getBoundingClientRect();
  return {{role: x.role, name: x.name, x: r.x + r.width / 2, y: r.y + r.height / 2, width: r.width, height: r.height}};
}})()"#,
        scroll = if scroll {
            "x.e.scrollIntoView({block: 'center', inline: 'center', behavior: 'instant'});"
        } else {
            ""
        }
    )
}

fn fingerprint(role: &str, name: &str) -> String {
    format!("{role}\u{1f}{name}")
}

fn closed(id: &str) -> ToolError {
    ToolError::new(ErrorCode::ClosedTarget, format!("Browser {id} is closed"))
}

impl State {
    /// A browser target this client may use.
    pub(crate) fn agent_browser(&self, grant: &ClientGrant, id: &str) -> Result<usize, ToolError> {
        match self.browser_index(id) {
            Some(index)
                if self.browsers.targets[index].client.as_deref() == Some(grant.id.as_str()) =>
            {
                if self.browsers.targets[index].shared.lock().closed {
                    return Err(closed(id));
                }
                Ok(index)
            }
            Some(_) => Err(ToolError::new(
                ErrorCode::ScopeDenied,
                "That browser belongs to the owner or another client",
            )),
            None if self.browsers.closed.contains(id) => Err(closed(id)),
            None if id.starts_with("brw_") && !id.contains(&self.automation.instance) => {
                Err(ToolError::new(
                    ErrorCode::UnknownTarget,
                    "That browser ID is from an earlier Rodtty run",
                ))
            }
            None => Err(ToolError::new(
                ErrorCode::UnknownTarget,
                format!("No browser {id}"),
            )),
        }
    }

    fn browser_status_json(&self, index: usize) -> Value {
        let target = &self.browsers.targets[index];
        let page = target.shared.lock();
        json!({
            "id": target.id,
            "url": page.url,
            "title": page.title,
            "loading": page.loading,
            "can_go_back": page.can_back,
            "can_go_forward": page.can_forward,
            "error": page.error,
            "crashed": page.crashed,
            "blocked": page.blocked,
            "document_revision": page.document,
            "frame_revision": frame_revision(&page),
            "frames_painted": page.frames,
            "viewport": {"width": page.view.0, "height": page.view.1, "scale": page.scale},
            "shown_in_browser_view": self.browsers.shown.as_deref() == Some(target.id.as_str()) && self.browsers.view_open,
            "recording": target.recording.as_ref().map(|r| r.id.clone()),
            "taken_over": self.automation.takeovers.contains(&target.id),
        })
    }

    pub(crate) fn browser_frame(&self, grant: &ClientGrant, id: &str) -> Result<u64, ToolError> {
        let index = self.agent_browser(grant, id)?;
        Ok(frame_revision(&self.browsers.targets[index].shared.lock()))
    }

    pub(crate) fn browser_release_input(
        &mut self,
        id: &str,
        input: &crate::desktop::automation::VirtualInput,
    ) {
        let Some(index) = self.browser_index(id) else {
            return;
        };
        let target = &self.browsers.targets[index];
        let (x, y) = input
            .pointer
            .map_or((0.0, 0.0), |(x, y)| (x as f32, y as f32));
        for button in &input.buttons {
            target.mouse_button(x, y, cef_button(button), true, 1, 0);
        }
        let m = input.modifiers;
        for (held, key) in [
            (m.shift, "Shift"),
            (m.ctrl, "Ctrl"),
            (m.alt, "Alt"),
            (m.meta, "Meta"),
        ] {
            if held {
                let _ = self.browser_modifier(id, key, false);
            }
        }
    }

    pub(crate) fn browser_modifier(
        &self,
        id: &str,
        key: &str,
        down: bool,
    ) -> Result<(), ToolError> {
        let index = self.browser_index(id).ok_or_else(|| closed(id))?;
        let (vk, native) = match key.to_ascii_lowercase().as_str() {
            "shift" => (0x10, 0x38),
            "ctrl" | "control" => (0x11, 0x3B),
            "alt" | "option" | "opt" => (0x12, 0x3A),
            _ => (0x5B, 0x37),
        };
        self.browsers.targets[index].key(cef::KeyEvent {
            size: std::mem::size_of::<sys::cef_key_event_t>(),
            type_: if down {
                KeyEventType::RAWKEYDOWN
            } else {
                KeyEventType::KEYUP
            },
            windows_key_code: vk,
            native_key_code: native,
            ..Default::default()
        });
        Ok(())
    }

    pub(crate) fn browser_key(
        &mut self,
        id: &str,
        chord: &crate::automation::keys::Chord,
        action: &str,
    ) -> ToolResult {
        let index = self.browser_index(id).ok_or_else(|| closed(id))?;
        let target = &self.browsers.targets[index];
        // Select All is an editing command in windowless macOS pages, where
        // the key alone does nothing.
        if cfg!(target_os = "macos")
            && (chord.mods.meta || chord.mods.ctrl)
            && chord.key == crate::automation::keys::KeyName::Char('a')
            && action == "press"
        {
            if let Some(frame) = target.browser.focused_frame() {
                frame.select_all();
            }
            return Ok(json!({"target": id, "sent": true, "command": "select_all"}));
        }
        let events = key_events(chord, action != "up", action != "down");
        if events.is_empty() {
            return Err(ToolError::new(
                ErrorCode::Unsupported,
                "That key has no browser encoding",
            ));
        }
        for event in events {
            target.key(event);
        }
        Ok(json!({"target": id, "sent": true}))
    }

    pub(crate) fn browser_char(
        &mut self,
        grant: &ClientGrant,
        id: &str,
        c: char,
    ) -> Result<(), ToolError> {
        let index = self.agent_browser(grant, id)?;
        let target = &self.browsers.targets[index];
        let named = match c {
            '\n' => Some(crate::automation::keys::KeyName::Enter),
            '\t' => Some(crate::automation::keys::KeyName::Tab),
            ' ' => Some(crate::automation::keys::KeyName::Space),
            c if c.is_ascii_alphanumeric() => Some(crate::automation::keys::KeyName::Char(c)),
            _ => None,
        };
        match named {
            Some(key) => {
                let shift = c.is_ascii_uppercase();
                let chord = crate::automation::keys::Chord {
                    key: if shift {
                        crate::automation::keys::KeyName::Char(c.to_ascii_lowercase())
                    } else {
                        key
                    },
                    mods: crate::automation::keys::Modifiers {
                        shift,
                        ..Default::default()
                    },
                };
                for event in key_events(&chord, true, true) {
                    target.key(event);
                }
            }
            None => {
                for event in char_events(c) {
                    target.key(event);
                }
            }
        }
        Ok(())
    }

    pub(crate) fn browser_pointer(
        &mut self,
        grant: &ClientGrant,
        id: &str,
        step: &crate::desktop::automation::input::PointerStep,
    ) -> Result<(), ToolError> {
        use crate::desktop::automation::input::StepKind;
        let index = self.agent_browser(grant, id)?;
        let target = &self.browsers.targets[index];
        let (w, h) = target.shared.lock().view;
        let (x, y) = (step.x as f32, step.y as f32);
        if x < 0.0 || y < 0.0 || x > w as f32 || y > h as f32 {
            return Err(ToolError::invalid(format!(
                "({x}, {y}) is outside the {w}x{h} viewport"
            )));
        }
        target.shared.lock().cursor = Some((x, y));
        let input = self
            .automation
            .inputs
            .get(&(grant.id.clone(), id.to_owned()));
        let mut modifiers = input.map_or(0, |i| {
            let m = i.modifiers;
            (if m.shift { MOD_SHIFT } else { 0 })
                | (if m.ctrl { MOD_CONTROL } else { 0 })
                | (if m.alt { MOD_ALT } else { 0 })
                | (if m.meta { MOD_COMMAND } else { 0 })
        });
        if let Some(input) = input {
            for b in &input.buttons {
                modifiers |= match b.as_str() {
                    "middle" => MOD_MIDDLE,
                    "right" => MOD_RIGHT,
                    _ => MOD_LEFT,
                };
            }
        }
        match step.kind {
            StepKind::Move => target.mouse_move(x, y, modifiers),
            StepKind::Down => target.mouse_button(
                x,
                y,
                cef_button(&step.button),
                false,
                step.click_count as i32,
                modifiers,
            ),
            StepKind::Up => target.mouse_button(
                x,
                y,
                cef_button(&step.button),
                true,
                step.click_count as i32,
                modifiers,
            ),
            StepKind::Wheel { dx, dy } => {
                target.mouse_move(x, y, modifiers);
                // CSS-pixel deltas; positive y scrolls content up, like a wheel down.
                target.wheel(x, y, -dx as f32, -dy as f32, modifiers);
            }
        }
        Ok(())
    }
}

fn cef_button(name: &str) -> MouseButtonType {
    match name {
        "middle" => MouseButtonType::MIDDLE,
        "right" => MouseButtonType::RIGHT,
        _ => MouseButtonType::LEFT,
    }
}

/// The viewport's identity for coordinate actions: its size, scale and document.
pub(crate) fn frame_revision(page: &Page) -> u64 {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    for v in [
        page.view.0 as u64,
        page.view.1 as u64,
        page.scale.to_bits() as u64,
        page.document,
    ] {
        hash ^= v;
        hash = hash.wrapping_mul(0x0100_0000_01b3);
    }
    hash >> 12
}

/// Evaluate a script in the page and return its value.
fn evaluate(
    host: &Host,
    grant: &ClientGrant,
    id: &str,
    expression: String,
    cancel: &AtomicBool,
) -> ToolResult {
    let g = grant.clone();
    let t = id.to_owned();
    let (shared, message) = on_ui(
        &host.handle,
        move |s, _| -> Result<(Shared, i32), ToolError> {
            let index = s.agent_browser(&g, &t)?;
            let target = &mut s.browsers.targets[index];
            let message = target
            .devtools("Runtime.evaluate", json!({"expression": expression, "returnByValue": true, "awaitPromise": true, "userGesture": false}))
            .ok_or_else(|| ToolError::new(ErrorCode::ClosedTarget, "The page is not accepting commands"))?;
            Ok((target.shared.clone(), message))
        },
    )??;
    super::schedule_pump(0);
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        if let Some(reply) = shared.lock().replies.remove(&message) {
            let reply = reply.map_err(|e| {
                ToolError::new(
                    ErrorCode::Internal,
                    format!("The page did not evaluate the request: {e}"),
                )
            })?;
            if let Some(exception) = reply.get("exceptionDetails") {
                return Err(ToolError::new(
                    ErrorCode::Internal,
                    format!(
                        "Page script failed: {}",
                        exception
                            .pointer("/exception/description")
                            .and_then(Value::as_str)
                            .unwrap_or("error")
                    ),
                ));
            }
            return Ok(reply
                .pointer("/result/value")
                .cloned()
                .unwrap_or(Value::Null));
        }
        if shared.lock().closed {
            return Err(closed(id));
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ToolError::new(ErrorCode::Cancelled, "Cancelled"));
        }
        if Instant::now() > deadline {
            return Err(ToolError::new(
                ErrorCode::Timeout,
                "The page did not answer in time",
            ));
        }
        std::thread::sleep(Duration::from_millis(10));
    }
}

/// Resolve an element reference to its center, scrolled into view.
fn locate(
    host: &Host,
    grant: &ClientGrant,
    id: &str,
    reference: &str,
    cancel: &AtomicBool,
) -> Result<(f64, f64), ToolError> {
    let g = grant.clone();
    let t = id.to_owned();
    let r = reference.to_owned();
    let (document, index, print, current) = on_ui(
        &host.handle,
        move |s, _| -> Result<(u64, usize, String, u64), ToolError> {
            let i = s.agent_browser(&g, &t)?;
            let target = &s.browsers.targets[i];
            let (document, index, print) = target.refs.get(&r).cloned().ok_or_else(|| {
                ToolError::new(
                    ErrorCode::Stale,
                    "Unknown element reference; take a new browser_snapshot",
                )
            })?;
            Ok((document, index, print, target.shared.lock().document))
        },
    )??;
    if document != current {
        return Err(ToolError::new(
            ErrorCode::Stale,
            "The page navigated since that snapshot; take a new browser_snapshot",
        ));
    }
    let found = evaluate(host, grant, id, locate_script(index, true), cancel)?;
    let role = found
        .get("role")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let name = found
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if found.is_null() || fingerprint(role, name) != print {
        return Err(ToolError::new(
            ErrorCode::Stale,
            "The page changed and that element is gone or different; take a new browser_snapshot",
        ));
    }
    // Let the scroll repaint before pointing at the new position.
    std::thread::sleep(Duration::from_millis(60));
    let found = evaluate(host, grant, id, locate_script(index, false), cancel)?;
    Ok((
        found["x"].as_f64().unwrap_or(0.0),
        found["y"].as_f64().unwrap_or(0.0),
    ))
}

fn wait_loaded(
    host: &Host,
    grant: &ClientGrant,
    id: &str,
    after_document: u64,
    timeout: Duration,
    cancel: &AtomicBool,
) -> ToolResult {
    let g = grant.clone();
    let t = id.to_owned();
    let shared = on_ui(&host.handle, move |s, _| {
        s.agent_browser(&g, &t)
            .map(|i| s.browsers.targets[i].shared.clone())
    })??;
    let deadline = Instant::now() + timeout;
    let started = Instant::now();
    loop {
        {
            let page = shared.lock();
            if page.closed {
                return Err(closed(id));
            }
            let navigated =
                page.document > after_document || started.elapsed() > Duration::from_millis(1500);
            if navigated && !page.loading && page.frame.is_some() {
                return Ok(
                    json!({"url": page.url, "title": page.title, "error": page.error, "blocked": page.blocked, "document_revision": page.document}),
                );
            }
            if page.blocked.is_some()
                && !page.loading
                && started.elapsed() > Duration::from_millis(300)
                && page.document <= after_document
            {
                return Err(ToolError::new(
                    ErrorCode::Unsupported,
                    page.blocked.clone().unwrap_or_default(),
                ));
            }
        }
        if cancel.load(Ordering::Relaxed) {
            return Err(ToolError::new(
                ErrorCode::Cancelled,
                "The wait was cancelled; the page keeps loading",
            ));
        }
        if Instant::now() > deadline {
            return Err(ToolError::new(
                ErrorCode::Timeout,
                "The page did not finish loading in time",
            ));
        }
        std::thread::sleep(Duration::from_millis(20));
    }
}

pub(crate) fn run(
    host: &Host,
    grant: &ClientGrant,
    name: &str,
    args: &Value,
    cancel: &AtomicBool,
) -> ToolResult {
    let id = || arg_str(args, "browser").map(str::to_owned);
    let g = grant.clone();
    match name {
        "browser_list" => on_ui(&host.handle, move |s, _| {
            let rows: Vec<Value> = (0..s.browsers.targets.len())
                .filter(|i| s.browsers.targets[*i].client.as_deref() == Some(g.id.as_str()))
                .map(|i| s.browser_status_json(i))
                .collect();
            json!({"browsers": rows})
        }),
        "browser_open" => {
            let url = match arg_opt_str(args, "url")? {
                Some(url) => Some(
                    address::normalize(url)
                        .map_err(|e| ToolError::new(ErrorCode::Unsupported, e))?,
                ),
                None => None,
            };
            let width = arg_u64(args, "width", 1280)?.clamp(200, 3840) as u32;
            let height = arg_u64(args, "height", 800)?.clamp(200, 2160) as u32;
            let scale = arg_f64(args, "scale")?.unwrap_or(1.0).clamp(1.0, 3.0) as f32;
            let show = arg_bool(args, "show", false)?;
            let u = url.clone();
            let target = on_ui(&host.handle, move |s, _| -> Result<String, ToolError> {
                let target = s
                    .create_browser(Some(g.id.clone()), u.as_deref(), (width, height), scale)
                    .map_err(|e| ToolError::new(ErrorCode::Unavailable, e))?;
                if show {
                    s.browsers.view_open = true;
                    s.browsers.shown = Some(target.clone());
                }
                Ok(target)
            })??;
            let loaded = if url.is_some() {
                Some(wait_loaded(
                    host,
                    grant,
                    &target,
                    0,
                    Duration::from_secs(20),
                    cancel,
                )?)
            } else {
                None
            };
            let t = target.clone();
            let status = on_ui(&host.handle, move |s, _| {
                s.browser_index(&t).map(|i| s.browser_status_json(i))
            })?;
            Ok(json!({"browser": target, "status": status, "loaded": loaded}))
        }
        "browser_navigate" => {
            let target = id()?;
            let url = address::normalize(arg_str(args, "url")?)
                .map_err(|e| ToolError::new(ErrorCode::Unsupported, e))?;
            let t = target.clone();
            let u = url.clone();
            let before = on_ui(&host.handle, move |s, _| -> Result<u64, ToolError> {
                let i = s.agent_browser(&g, &t)?;
                let target = &s.browsers.targets[i];
                target.shared.lock().blocked = None;
                let document = target.shared.lock().document;
                target.navigate(&u);
                Ok(document)
            })??;
            super::schedule_pump(0);
            let (job, job_cancel) =
                host.shared
                    .hub
                    .start_job("browser_navigate", &grant.id, Some(&target));
            if !arg_bool(args, "wait", true)? {
                let host_shared = host.shared.clone();
                let handle = host.handle.clone();
                let grant = grant.clone();
                let (target, job_id) = (target.clone(), job.clone());
                let root = host.root.clone();
                let instance = host.instance.clone();
                std::thread::spawn(move || {
                    let h = Host {
                        handle,
                        shared: host_shared.clone(),
                        root,
                        instance,
                    };
                    let result = wait_loaded(
                        &h,
                        &grant,
                        &target,
                        before,
                        Duration::from_secs(60),
                        &job_cancel,
                    );
                    finish_job(&host_shared, &job_id, result);
                });
                return Ok(json!({"browser": id()?, "url": url, "job": job}));
            }
            let result = wait_loaded(
                host,
                grant,
                &target,
                before,
                Duration::from_secs(30),
                cancel,
            );
            finish_job(&host.shared, &job, result.clone());
            if let Ok(value) = &result {
                host.shared
                    .hub
                    .emit("navigation", Some(&target), value.clone());
            }
            result.map(|mut v| {
                v["job"] = json!(job);
                v
            })
        }
        "browser_history" => {
            let target = id()?;
            let action = arg_str(args, "action")?.to_owned();
            on_ui(&host.handle, move |s, _| -> ToolResult {
                let i = s.agent_browser(&g, &target)?;
                let t = &s.browsers.targets[i];
                match action.as_str() {
                    "back" => t.browser.go_back(),
                    "forward" => t.browser.go_forward(),
                    "reload" => {
                        t.shared.lock().crashed = None;
                        t.browser.reload()
                    }
                    "stop" => t.browser.stop_load(),
                    _ => {
                        return Err(ToolError::invalid(
                            "`action` must be back, forward, reload or stop",
                        ));
                    }
                }
                Ok(json!({"browser": target, "action": action}))
            })?
        }
        "browser_status" => {
            let target = id()?;
            on_ui(&host.handle, move |s, _| {
                s.agent_browser(&g, &target)
                    .map(|i| s.browser_status_json(i))
            })?
        }
        "browser_snapshot" => {
            let target = id()?;
            let max = arg_u64(args, "max_elements", 200)?.clamp(1, 1000) as usize;
            let t = target.clone();
            let document = on_ui(&host.handle, move |s, _| {
                s.agent_browser(&g, &t)
                    .map(|i| s.browsers.targets[i].shared.lock().document)
            })??;
            let mut snapshot = evaluate(host, grant, &target, snapshot_script(max), cancel)?;
            let mut refs = Vec::new();
            if let Some(elements) = snapshot.get_mut("elements").and_then(Value::as_array_mut) {
                for element in elements {
                    let index = element["index"].as_u64().unwrap_or(0) as usize;
                    let reference = format!("e{document}-{index}");
                    let print = fingerprint(
                        element["role"].as_str().unwrap_or_default(),
                        element["name"].as_str().unwrap_or_default(),
                    );
                    refs.push((reference.clone(), (document, index, print)));
                    element["ref"] = json!(reference);
                }
            }
            let g = grant.clone();
            let t = target.clone();
            on_ui(&host.handle, move |s, _| {
                if let Ok(i) = s.agent_browser(&g, &t) {
                    // References stay valid across snapshots of one document.
                    let target = &mut s.browsers.targets[i];
                    target.refs.retain(|_, (doc, _, _)| *doc == document);
                    target.refs.extend(refs);
                }
            })?;
            snapshot["document_revision"] = json!(document);
            snapshot["browser"] = json!(target);
            Ok(snapshot)
        }
        "browser_click" => {
            let target = id()?;
            let (x, y) = match arg_opt_str(args, "ref")? {
                Some(reference) => locate(host, grant, &target, reference, cancel)?,
                None => (
                    arg_f64(args, "x")?
                        .ok_or_else(|| ToolError::invalid("Give ref, or x and y"))?,
                    arg_f64(args, "y")?
                        .ok_or_else(|| ToolError::invalid("Give ref, or x and y"))?,
                ),
            };
            let count = arg_u64(args, "count", 1)?.clamp(1, 3);
            let mut pointer_args = json!({"target": target, "action": if count >= 2 { "double_click" } else { "click" }, "x": x, "y": y, "steps": 8, "button": arg_opt_str(args, "button")?.unwrap_or("left")});
            if arg_opt_str(args, "ref")?.is_none()
                && let Some(frame) = args.get("frame_revision")
            {
                pointer_args["frame_revision"] = frame.clone();
            }
            crate::desktop::automation::input::pointer(host, grant, &pointer_args, cancel)?;
            Ok(json!({"browser": target, "clicked": {"x": x, "y": y}}))
        }
        "browser_type" => {
            let target = id()?;
            if let Some(reference) = arg_opt_str(args, "ref")? {
                let (x, y) = locate(host, grant, &target, reference, cancel)?;
                crate::desktop::automation::input::pointer(
                    host,
                    grant,
                    &json!({"target": target, "action": "click", "x": x, "y": y, "steps": 6}),
                    cancel,
                )?;
            }
            crate::desktop::automation::input::type_text(
                host,
                grant,
                &json!({"target": target, "text": arg_str(args, "text")?, "delay_ms": arg_u64(args, "delay_ms", 0)?}),
                cancel,
            )
        }
        "browser_press" => {
            let target = id()?;
            let keys: Vec<String> = args
                .get("keys")
                .and_then(Value::as_array)
                .ok_or_else(|| ToolError::invalid("`keys` must be an array"))?
                .iter()
                .map(|k| {
                    k.as_str()
                        .map(str::to_owned)
                        .ok_or_else(|| ToolError::invalid("Each key must be a string"))
                })
                .collect::<Result<_, _>>()?;
            for key in &keys {
                let chord = crate::automation::keys::parse(key)?;
                let g = grant.clone();
                let t = target.clone();
                on_ui(&host.handle, move |s, _| -> ToolResult {
                    s.agent_browser(&g, &t)?;
                    s.agent_input(&g.id, &t)?;
                    s.browser_key(&t, &chord, "press")
                })??;
            }
            Ok(json!({"browser": target, "pressed": keys}))
        }
        "browser_scroll" => {
            let target = id()?;
            if let Some(reference) = arg_opt_str(args, "ref")? {
                let (x, y) = locate(host, grant, &target, reference, cancel)?;
                return Ok(
                    json!({"browser": target, "scrolled_into_view": true, "center": {"x": x, "y": y}}),
                );
            }
            let t = target.clone();
            let (w, h) = on_ui(&host.handle, move |s, _| {
                s.agent_browser(&g, &t)
                    .map(|i| s.browsers.targets[i].shared.lock().view)
            })??;
            let x = arg_f64(args, "x")?.unwrap_or(f64::from(w) / 2.0);
            let y = arg_f64(args, "y")?.unwrap_or(f64::from(h) / 2.0);
            crate::desktop::automation::input::pointer(
                host,
                grant,
                &json!({"target": target, "action": "wheel", "x": x, "y": y, "delta_x": arg_f64(args, "delta_x")?.unwrap_or(0.0), "delta_y": arg_f64(args, "delta_y")?.unwrap_or(0.0)}),
                cancel,
            )
        }
        "browser_wait" => {
            let target = id()?;
            let timeout = Duration::from_millis(arg_u64(args, "timeout_ms", 15_000)?.min(120_000));
            let url = arg_opt_str(args, "url_contains")?.map(str::to_owned);
            let text = arg_opt_str(args, "text")?.map(str::to_owned);
            let selector = arg_opt_str(args, "selector")?.map(str::to_owned);
            let loaded = arg_bool(args, "loaded", false)?;
            if url.is_none() && text.is_none() && selector.is_none() && !loaded {
                return Err(ToolError::invalid(
                    "Name a condition: url_contains, text, selector or loaded",
                ));
            }
            let deadline = Instant::now() + timeout;
            loop {
                let t = target.clone();
                let g = grant.clone();
                let (page_url, loading) = on_ui(&host.handle, move |s, _| {
                    s.agent_browser(&g, &t).map(|i| {
                        let p = s.browsers.targets[i].shared.lock();
                        (p.url.clone(), p.loading)
                    })
                })??;
                let mut ok = (!loaded || !loading)
                    && url.as_ref().is_none_or(|u| page_url.contains(u.as_str()));
                if ok && (text.is_some() || selector.is_some()) {
                    let check = format!(
                        "(() => {{ const t = {}; const s = {}; if (t !== null && !(document.body && document.body.innerText.includes(t))) return false; if (s !== null && !document.querySelector(s)) return false; return true; }})()",
                        serde_json::to_string(&text).unwrap_or("null".into()),
                        serde_json::to_string(&selector).unwrap_or("null".into())
                    );
                    ok = evaluate(host, grant, &target, check, cancel)
                        .map(|v| v == json!(true))
                        .unwrap_or(false);
                }
                if ok {
                    return Ok(json!({"browser": target, "url": page_url, "matched": true}));
                }
                if cancel.load(Ordering::Relaxed) {
                    return Err(ToolError::new(
                        ErrorCode::Cancelled,
                        "The wait was cancelled",
                    ));
                }
                if Instant::now() > deadline {
                    return Err(ToolError::new(
                        ErrorCode::Timeout,
                        "The condition did not happen before timeout_ms",
                    )
                    .with_data(json!({"url": page_url})));
                }
                std::thread::sleep(Duration::from_millis(150));
            }
        }
        "browser_close" => {
            let target = id()?;
            on_ui(&host.handle, move |s, _| -> ToolResult {
                s.agent_browser(&g, &target)?;
                let recording = s.close_browser_target(&target);
                s.browsers.closed.insert(target.clone());
                Ok(json!({"browser": target, "closed": true, "recording": recording}))
            })?
        }
        "recording_start" => {
            let target = id()?;
            let limits = record::Limits {
                fps: arg_u64(args, "fps", 15)?.clamp(1, 30) as u32,
                max_seconds: arg_u64(args, "max_seconds", 120)?.clamp(1, 600) as u32,
                max_width: arg_u64(args, "max_width", 1920)?.clamp(160, 3840) as u32,
                max_bytes: 1 << 30,
                cursor: arg_bool(args, "cursor", true)?,
            };
            let artifacts = artifacts(&host.root);
            on_ui(&host.handle, move |s, _| -> ToolResult {
                let i = s.agent_browser(&g, &target)?;
                if let Some(existing) = &s.browsers.targets[i].recording {
                    return Err(ToolError::new(
                        ErrorCode::Conflict,
                        format!("Browser {target} is already recording ({})", existing.id),
                    ));
                }
                let (artifact, path) =
                    crate::desktop::capture::artifact_path(&artifacts, "browser", "avi")?;
                let id = format!("rec_{}", &artifact[4..]);
                let wake: Arc<dyn Fn() + Send + Sync> = match s.handle.clone() {
                    Some(h) => Arc::new(move || h.wake()),
                    None => s.wake.clone(),
                };
                let recording = record::Recording::start(
                    id.clone(),
                    target.clone(),
                    path.clone(),
                    limits,
                    wake,
                );
                let t = &mut s.browsers.targets[i];
                {
                    let mut page = t.shared.lock();
                    page.feed = Some(recording.feed.clone());
                    if let Some(frame) = page.frame.clone() {
                        *recording
                            .feed
                            .latest
                            .lock()
                            .unwrap_or_else(|e| e.into_inner()) = Some(frame);
                    }
                }
                if let Some(host) = t.host() {
                    host.invalidate(PaintElementType::VIEW);
                }
                t.recording = Some(recording);
                s.emit(
                    "recording",
                    Some(&id),
                    json!({"state": "recording", "browser": target}),
                );
                Ok(
                    json!({"recording": id, "browser": target, "path": path, "format": "avi (Motion JPEG)", "fps": limits.fps, "max_seconds": limits.max_seconds, "max_width": limits.max_width}),
                )
            })?
        }
        "recording_status" | "recording_stop" => {
            let recording = arg_str(args, "recording")?.to_owned();
            let stop = name == "recording_stop";
            let deadline = Instant::now() + Duration::from_secs(30);
            let mut stopped = false;
            loop {
                let r = recording.clone();
                let g2 = g.clone();
                let status = on_ui(
                    &host.handle,
                    move |s, _| -> Result<Option<record::Status>, ToolError> {
                        for t in &mut s.browsers.targets {
                            if t.recording.as_ref().is_some_and(|x| x.id == r) {
                                if t.client.as_deref() != Some(g2.id.as_str()) {
                                    return Err(ToolError::new(
                                        ErrorCode::ScopeDenied,
                                        "That recording belongs to another client",
                                    ));
                                }
                                if stop && !stopped {
                                    t.shared.lock().feed = None;
                                    let mut rec = t.recording.take().expect("present");
                                    rec.stop("stopped by the client", false);
                                    s.browsers.finalizing.push(rec);
                                    return Ok(None);
                                }
                                return Ok(Some(t.recording.as_ref().expect("present").status()));
                            }
                        }
                        if let Some(rec) = s.browsers.finalizing.iter().find(|x| x.id == r) {
                            return Ok(if stop { None } else { Some(rec.status()) });
                        }
                        s.browser_after_pump();
                        s.browsers
                            .finished_recordings
                            .iter()
                            .rev()
                            .find(|x| x.id == r)
                            .cloned()
                            .map(Some)
                            .ok_or_else(|| {
                                ToolError::new(
                                    ErrorCode::UnknownTarget,
                                    format!("No recording {r}"),
                                )
                            })
                    },
                )??;
                stopped |= stop;
                match status {
                    Some(status)
                        if !stop
                            || !matches!(
                                status.state,
                                record::RecordingState::Recording
                                    | record::RecordingState::Finalizing
                            ) =>
                    {
                        let mut value = json!(status);
                        value["artifact"] = json!(
                            status
                                .path
                                .as_ref()
                                .map(|_| format!("art_{}", &recording[4..]))
                        );
                        return Ok(value);
                    }
                    _ => {}
                }
                if Instant::now() > deadline {
                    return Err(ToolError::new(
                        ErrorCode::Timeout,
                        "The recording did not finalize in time",
                    ));
                }
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        _ => Err(ToolError::new(ErrorCode::UnknownTool, name.to_owned())),
    }
}

fn finish_job(shared: &crate::desktop::automation::Shared, id: &str, result: ToolResult) {
    use crate::automation::events::JobState;
    match result {
        Ok(v) => shared
            .hub
            .update_job(id, JobState::Completed, Some(v), None),
        Err(e) if e.code == ErrorCode::Cancelled => {
            shared
                .hub
                .update_job(id, JobState::Cancelled, None, Some(e))
        }
        Err(e) => shared.hub.update_job(id, JobState::Failed, None, Some(e)),
    }
}

/// A PNG of the browser's own latest frame: the page and, when asked, the
/// virtual pointer. Never the desktop.
pub(crate) fn screenshot(host: &Host, grant: &ClientGrant, args: &Value) -> ToolResult {
    let target = arg_str(args, "target")?.to_owned();
    let cursor = arg_bool(args, "cursor", false)?;
    let g = grant.clone();
    let t = target.clone();
    let (frame, pointer, view) = on_ui(&host.handle, move |s, _| -> Result<_, ToolError> {
        let i = s.agent_browser(&g, &t)?;
        let page = s.browsers.targets[i].shared.lock();
        Ok((page.frame.clone(), page.cursor, page.view))
    })??;
    let frame = frame
        .ok_or_else(|| ToolError::new(ErrorCode::Unsupported, "The page has not painted yet"))?;
    let mut rgba = Vec::with_capacity(frame.bgra.len());
    for p in frame.bgra.chunks_exact(4) {
        rgba.extend_from_slice(&[p[2], p[1], p[0], 255]);
    }
    if cursor && let Some((x, y)) = pointer {
        draw_cursor_rgb(
            &mut rgba,
            frame.width,
            frame.height,
            x * frame.scale,
            y * frame.scale,
            4,
        );
    }
    let (artifact, path) =
        crate::desktop::capture::artifact_path(&artifacts(&host.root), "browser", "png")?;
    crate::desktop::capture::write_png(&path, frame.width, frame.height, &rgba)?;
    Ok(json!({
        "artifact": artifact,
        "path": path,
        "format": "png",
        "width": frame.width,
        "height": frame.height,
        "scale": frame.scale,
        "viewport": {"width": view.0, "height": view.1},
        "cursor": cursor && pointer.is_some(),
        "source": {"kind": "browser", "target": target, "renderer": "CEF off-screen frame", "frame": frame.id},
        "timestamp_ms": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
    }))
}

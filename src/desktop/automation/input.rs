//! The agent's virtual pointer and keyboard. Each client has its own state
//! per target: pointer position, held buttons and modifiers. Events go
//! straight to the target (a PTY's mouse reporting and key encoding, or a
//! browser's input API), never through the operating system.
use super::*;
use crate::automation::{
    arg_f64, arg_opt_str, arg_str, arg_u64,
    keys::{self, Chord, KeyName, Modifiers},
};
use crate::desktop::terminal::mouse;

#[derive(Clone, Debug, Default)]
pub(crate) struct VirtualInput {
    pub pointer: Option<(f64, f64)>,
    pub buttons: Vec<String>,
    pub modifiers: Modifiers,
}

impl VirtualInput {
    fn json(&self) -> Value {
        let m = self.modifiers;
        json!({
            "pointer": self.pointer.map(|(x, y)| json!({"x": x, "y": y})),
            "buttons": self.buttons,
            "modifiers": {"shift": m.shift, "ctrl": m.ctrl, "alt": m.alt, "meta": m.meta},
        })
    }
}

/// A chord as the bytes Rodtty's terminal sends for the same key from a keyboard.
pub(crate) fn terminal_bytes(chord: &Chord, application_cursor: bool) -> Option<Vec<u8>> {
    let m = chord.mods;
    let mods = rune::Mods {
        shift: m.shift,
        alt: m.alt,
        ctrl: m.ctrl,
        cmd: false,
        logo: false,
    };
    if m.meta {
        return None;
    }
    let key = match &chord.key {
        KeyName::Char(c) => {
            let c = if m.shift && !m.ctrl { c.to_uppercase().next().unwrap_or(*c) } else { *c };
            rune::Key::Text(c.to_string())
        }
        KeyName::Space => rune::Key::Text(" ".into()),
        KeyName::Enter => rune::Key::Enter,
        KeyName::Backspace => rune::Key::Backspace,
        KeyName::Delete => rune::Key::Delete,
        KeyName::Escape => rune::Key::Escape,
        KeyName::Tab => rune::Key::Tab,
        KeyName::Up => rune::Key::Up,
        KeyName::Down => rune::Key::Down,
        KeyName::Left => rune::Key::Left,
        KeyName::Right => rune::Key::Right,
        KeyName::Home => rune::Key::Home,
        KeyName::End => rune::Key::End,
        KeyName::PageUp => rune::Key::PageUp,
        KeyName::PageDown => rune::Key::PageDown,
        KeyName::Insert => rune::Key::Insert,
        KeyName::Function(n) => rune::Key::Function(*n),
    };
    crate::desktop::encode_key(&rune::KeyEvent { key, mods }, application_cursor)
}

fn merge(a: Modifiers, b: Modifiers) -> Modifiers {
    Modifiers {
        shift: a.shift || b.shift,
        ctrl: a.ctrl || b.ctrl,
        alt: a.alt || b.alt,
        meta: a.meta || b.meta,
    }
}

fn set_modifier(mods: &mut Modifiers, key: &str, down: bool) -> bool {
    let Some(set) = keys::modifier(key) else { return false };
    let mut probe = Modifiers::default();
    set(&mut probe);
    if probe.shift { mods.shift = down; }
    if probe.ctrl { mods.ctrl = down; }
    if probe.alt { mods.alt = down; }
    if probe.meta { mods.meta = down; }
    true
}

/// A terminal's geometry as a frame revision: coordinates sent against a
/// different size can't be trusted to name the same cell.
pub(crate) fn terminal_frame(rows: u16, cols: u16) -> u64 {
    (u64::from(rows) << 16) | u64::from(cols)
}

impl State {
    fn input_entry(&mut self, client: &str, target: &str) -> &mut VirtualInput {
        self.automation.inputs.entry((client.to_owned(), target.to_owned())).or_default()
    }

    /// Drop a client's held keys and buttons on a target, telling the
    /// target they were released so nothing stays stuck.
    pub(crate) fn release_virtual_input(&mut self, client: &str, target: &str, reason: &str) {
        let Some(input) = self.automation.inputs.remove(&(client.to_owned(), target.to_owned())) else {
            return;
        };
        if target.starts_with("term_")
            && !input.buttons.is_empty()
            && let Some(session) = self.sessions.iter().find(|s| self.terminal_target(s) == target)
            && let Some((x, y)) = input.pointer
        {
            for button in &input.buttons {
                let button = parse_button(button).unwrap_or(mouse::Button::Left);
                if let Some(bytes) = session.with_screen(|screen| {
                    mouse::encode(screen, mouse::Action::Release(button), y as u16, x as u16, Default::default())
                }) {
                    let _ = session.send(bytes);
                }
            }
        }
        #[cfg(feature = "browser")]
        if target.starts_with("brw_") {
            self.browser_release_input(target, &input);
        }
        self.automation.shared.bump(target);
        self.emit("input_ownership", Some(target), json!({"released": true, "client": client, "reason": reason}));
    }

    pub(crate) fn auto_input_state(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let target = arg_str(args, "target")?;
        let frame = self.target_frame(grant, target)?;
        let input = self.input_entry(&grant.id, target).json();
        Ok(json!({
            "target": target,
            "input": input,
            "frame_revision": frame,
            "taken_over": self.automation.takeovers.contains(target),
            "agent": self.automation.agent_focus.get(target),
        }))
    }

    /// A target's current frame revision, checking the client may use it.
    fn target_frame(&mut self, grant: &ClientGrant, target: &str) -> Result<u64, ToolError> {
        if target.starts_with("term_") {
            let index = self.find_terminal(grant, target)?;
            let (rows, cols) = self.sessions[index].size();
            return Ok(terminal_frame(rows, cols));
        }
        #[cfg(feature = "browser")]
        if target.starts_with("brw_") {
            return self.browser_frame(grant, target);
        }
        Err(ToolError::new(ErrorCode::Unsupported, format!("{target} can't take virtual input")))
    }

    pub(crate) fn auto_input_release(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let target = arg_str(args, "target")?;
        self.target_frame(grant, target)?;
        self.release_virtual_input(&grant.id, target, "client_release");
        Ok(json!({"target": target, "released": true}))
    }

    pub(crate) fn auto_input_key(&mut self, grant: &ClientGrant, args: &Value) -> ToolResult {
        let target = arg_str(args, "target")?.to_owned();
        let key = arg_str(args, "key")?;
        let action = arg_opt_str(args, "action")?.unwrap_or("press");
        self.target_frame(grant, &target)?;
        self.agent_input(&grant.id, &target)?;
        let entry = self.input_entry(&grant.id, &target);
        match action {
            "down" | "up" if set_modifier(&mut entry.modifiers, key, action == "down") => {
                let modifiers = entry.json()["modifiers"].clone();
                #[cfg(feature = "browser")]
                if target.starts_with("brw_") {
                    self.browser_modifier(&target, key, action == "down")?;
                }
                return Ok(json!({"target": target, "modifiers": modifiers}));
            }
            "press" | "down" | "up" => {}
            _ => return Err(ToolError::invalid("`action` must be press, down or up")),
        }
        let held = entry.modifiers;
        let mut chord = keys::parse(key)?;
        chord.mods = merge(chord.mods, held);
        if target.starts_with("term_") {
            // A PTY has no key-up: a key sends its bytes when pressed.
            if action == "up" {
                return Ok(json!({"target": target, "sent": false}));
            }
            let index = self.find_terminal(grant, &target)?;
            let cursor = self.sessions[index].with_screen(vt100::Screen::application_cursor);
            let bytes = terminal_bytes(&chord, cursor)
                .ok_or_else(|| ToolError::new(ErrorCode::Unsupported, format!("{key} has no terminal encoding")))?;
            self.sessions[index]
                .send(bytes)
                .map_err(|e| ToolError::new(ErrorCode::ClosedTarget, format!("{e:#}")))?;
            return Ok(json!({"target": target, "sent": true}));
        }
        #[cfg(feature = "browser")]
        if target.starts_with("brw_") {
            return self.browser_key(&target, &chord, action);
        }
        Err(ToolError::new(ErrorCode::Unsupported, "This target takes no key input"))
    }

    /// Send one typed character to a target.
    pub(crate) fn agent_char(&mut self, grant: &ClientGrant, target: &str, c: char, epoch: u64) -> Result<(), ToolError> {
        if self.automation.shared.epoch(target) != epoch {
            return Err(ToolError::new(ErrorCode::Cancelled, "Typing stopped: the owner took over, paused, or the target changed"));
        }
        self.agent_input(&grant.id, target)?;
        if target.starts_with("term_") {
            let index = self.find_terminal(grant, target)?;
            let bytes = match c {
                '\n' => b"\r".to_vec(),
                c => c.to_string().into_bytes(),
            };
            return self.sessions[index]
                .send(bytes)
                .map_err(|e| ToolError::new(ErrorCode::ClosedTarget, format!("{e:#}")));
        }
        #[cfg(feature = "browser")]
        if target.starts_with("brw_") {
            return self.browser_char(grant, target, c);
        }
        Err(ToolError::new(ErrorCode::Unsupported, "This target takes no text input"))
    }

    /// One pointer event on a terminal, in zero-based cells.
    fn terminal_pointer(&mut self, grant: &ClientGrant, target: &str, action: mouse::Action, x: f64, y: f64) -> Result<bool, ToolError> {
        let index = self.find_terminal(grant, target)?;
        let (rows, cols) = self.sessions[index].size();
        if x < 0.0 || y < 0.0 || x >= f64::from(cols) || y >= f64::from(rows) {
            return Err(ToolError::invalid(format!("Cell ({x}, {y}) is outside the {cols}x{rows} terminal")));
        }
        let session = &self.sessions[index];
        let mods = {
            let m = self.automation.inputs.get(&(grant.id.clone(), target.to_owned())).map(|i| i.modifiers).unwrap_or_default();
            mouse::Modifiers { shift: m.shift, alt: m.alt, ctrl: m.ctrl }
        };
        let bytes = session.with_screen(|screen| {
            if screen.mouse_protocol_mode() != vt100::MouseProtocolMode::None {
                mouse::encode(screen, action, y as u16, x as u16, mods)
            } else if screen.alternate_screen() && matches!(action, mouse::Action::WheelUp | mouse::Action::WheelDown) {
                Some(mouse::wheel_arrow(screen, action == mouse::Action::WheelUp).to_vec())
            } else {
                None
            }
        });
        match bytes {
            Some(bytes) => {
                session.send(bytes).map_err(|e| ToolError::new(ErrorCode::ClosedTarget, format!("{e:#}")))?;
                Ok(true)
            }
            // Mouse modes report only some motion; anything else here means
            // the program asked for no pointer input at all.
            None if matches!(action, mouse::Action::Motion(_)) => Ok(false),
            None => Err(ToolError::new(
                ErrorCode::Unsupported,
                "This terminal program has not enabled mouse reporting, so it can't receive pointer input",
            )),
        }
    }

    /// One step of a pointer gesture on any target.
    pub(crate) fn agent_pointer_step(&mut self, grant: &ClientGrant, target: &str, step: &PointerStep, epoch: u64) -> Result<(), ToolError> {
        if self.automation.shared.epoch(target) != epoch {
            return Err(ToolError::new(ErrorCode::Cancelled, "The gesture stopped: the owner took over, paused, or the target changed"));
        }
        self.agent_input(&grant.id, target)?;
        let (x, y) = (step.x, step.y);
        let button_name = step.button.clone();
        {
            let entry = self.input_entry(&grant.id, target);
            entry.pointer = Some((x, y));
            match step.kind {
                StepKind::Down => {
                    if !entry.buttons.contains(&button_name) {
                        entry.buttons.push(button_name.clone());
                    }
                }
                StepKind::Up => entry.buttons.retain(|b| *b != button_name),
                _ => {}
            }
        }
        if target.starts_with("term_") {
            let button = parse_button(&step.button).unwrap_or(mouse::Button::Left);
            let held = self.automation.inputs.get(&(grant.id.clone(), target.to_owned())).and_then(|i| i.buttons.first().cloned()).and_then(|b| parse_button(&b));
            let action = match step.kind {
                StepKind::Move => mouse::Action::Motion(held),
                StepKind::Down => mouse::Action::Press(button),
                StepKind::Up => mouse::Action::Release(button),
                StepKind::Wheel { dy, .. } if dy < 0.0 => mouse::Action::WheelUp,
                StepKind::Wheel { .. } => mouse::Action::WheelDown,
            };
            self.terminal_pointer(grant, target, action, x, y)?;
            return Ok(());
        }
        #[cfg(feature = "browser")]
        if target.starts_with("brw_") {
            return self.browser_pointer(grant, target, step);
        }
        Err(ToolError::new(ErrorCode::Unsupported, "This target takes no pointer input"))
    }
}

pub(crate) fn parse_button(name: &str) -> Option<mouse::Button> {
    match name {
        "left" => Some(mouse::Button::Left),
        "middle" => Some(mouse::Button::Middle),
        "right" => Some(mouse::Button::Right),
        _ => None,
    }
}

#[derive(Clone, Debug)]
pub(crate) enum StepKind {
    Move,
    Down,
    Up,
    Wheel { dx: f64, dy: f64 },
}

#[derive(Clone, Debug)]
pub(crate) struct PointerStep {
    pub kind: StepKind,
    pub x: f64,
    pub y: f64,
    pub button: String,
    pub click_count: u32,
}

/// Type text one character at a time, stopping at once on takeover, pause,
/// revocation, cancellation or closure.
pub(super) fn type_text(host: &Host, grant: &ClientGrant, args: &Value, cancel: &AtomicBool) -> ToolResult {
    let target = arg_str(args, "target")?.to_owned();
    let text = arg_str(args, "text")?.to_owned();
    if text.len() > 16_384 {
        return Err(ToolError::new(ErrorCode::Limit, "Text is limited to 16 KiB per call"));
    }
    let delay = Duration::from_millis(arg_u64(args, "delay_ms", 0)?.min(1000));
    let epoch = host.shared.epoch(&target);
    let mut typed = 0;
    for c in text.chars() {
        if cancel.load(Ordering::Relaxed) {
            return Err(ToolError::new(ErrorCode::Cancelled, "Typing was cancelled").with_data(json!({"typed": typed})));
        }
        let g = grant.clone();
        let t = target.clone();
        on_ui(&host.handle, move |s, _| s.agent_char(&g, &t, c, epoch))?
            .map_err(|e| e.with_data(json!({"typed": typed})))?;
        typed += 1;
        if !delay.is_zero() {
            std::thread::sleep(delay);
        }
    }
    Ok(json!({"target": target, "typed": typed}))
}

/// Build a gesture's steps, then deliver them in order with the pointer
/// visibly progressing between points.
pub(super) fn pointer(host: &Host, grant: &ClientGrant, args: &Value, cancel: &AtomicBool) -> ToolResult {
    let target = arg_str(args, "target")?.to_owned();
    let action = arg_str(args, "action")?;
    let button = arg_opt_str(args, "button")?.unwrap_or("left").to_owned();
    if parse_button(&button).is_none() {
        return Err(ToolError::invalid("`button` must be left, middle or right"));
    }
    let g = grant.clone();
    let t = target.clone();
    let (frame, last) = on_ui(&host.handle, move |s, _| -> Result<(u64, Option<(f64, f64)>), ToolError> {
        let frame = s.target_frame(&g, &t)?;
        Ok((frame, s.automation.inputs.get(&(g.id.clone(), t.clone())).and_then(|i| i.pointer)))
    })??;
    if let Some(expected) = args.get("frame_revision").and_then(Value::as_u64)
        && expected != frame
    {
        return Err(ToolError::new(ErrorCode::Stale, "The target was resized, rescaled or changed since that frame; observe it again").with_data(json!({"frame_revision": frame})));
    }
    let x = arg_f64(args, "x")?;
    let y = arg_f64(args, "y")?;
    let at = match (x, y) {
        (Some(x), Some(y)) => (x, y),
        (None, None) => last.ok_or_else(|| ToolError::invalid("Give x and y; the pointer has no position yet"))?,
        _ => return Err(ToolError::invalid("Give both x and y")),
    };
    let steps_count = arg_u64(args, "steps", 1)?.clamp(1, 200) as usize;
    let path = |from: (f64, f64), to: (f64, f64), n: usize| -> Vec<(f64, f64)> {
        (1..=n).map(|i| {
            let t = i as f64 / n as f64;
            (from.0 + (to.0 - from.0) * t, from.1 + (to.1 - from.1) * t)
        }).collect()
    };
    let step = |kind: StepKind, (x, y): (f64, f64), count: u32| PointerStep { kind, x, y, button: button.clone(), click_count: count };
    let mut steps = Vec::new();
    let approach = |steps: &mut Vec<PointerStep>, to: (f64, f64)| {
        let from = last.unwrap_or(to);
        for p in path(from, to, if from == to { 1 } else { steps_count }) {
            steps.push(step(StepKind::Move, p, 0));
        }
    };
    match action {
        "move" => approach(&mut steps, at),
        "down" => steps.push(step(StepKind::Down, at, 1)),
        "up" => steps.push(step(StepKind::Up, at, 1)),
        "click" | "double_click" => {
            approach(&mut steps, at);
            let clicks = if action == "click" { 1 } else { 2 };
            for count in 1..=clicks {
                steps.push(step(StepKind::Down, at, count));
                steps.push(step(StepKind::Up, at, count));
            }
        }
        "drag" => {
            let to = match (arg_f64(args, "to_x")?, arg_f64(args, "to_y")?) {
                (Some(x), Some(y)) => (x, y),
                _ => return Err(ToolError::invalid("A drag needs to_x and to_y")),
            };
            approach(&mut steps, at);
            steps.push(step(StepKind::Down, at, 1));
            for p in path(at, to, steps_count.max(4)) {
                steps.push(step(StepKind::Move, p, 0));
            }
            steps.push(step(StepKind::Up, to, 1));
        }
        "wheel" => {
            let dx = arg_f64(args, "delta_x")?.unwrap_or(0.0);
            let dy = arg_f64(args, "delta_y")?.unwrap_or(0.0);
            approach(&mut steps, at);
            steps.push(step(StepKind::Wheel { dx, dy }, at, 0));
        }
        _ => return Err(ToolError::invalid("Unknown pointer action")),
    }
    let epoch = host.shared.epoch(&target);
    let total = steps.len();
    for (done, step) in steps.into_iter().enumerate() {
        let g = grant.clone();
        let t = target.clone();
        if cancel.load(Ordering::Relaxed) {
            // Never leave a button held by a cancelled gesture.
            let g2 = grant.clone();
            let _ = on_ui(&host.handle, move |s, _| s.release_virtual_input(&g2.id, &t, "cancelled"));
            return Err(ToolError::new(ErrorCode::Cancelled, "The gesture was cancelled; held buttons were released").with_data(json!({"steps_done": done})));
        }
        let result = on_ui(&host.handle, move |s, _| s.agent_pointer_step(&g, &t, &step, epoch))?;
        if let Err(error) = result {
            let g2 = grant.clone();
            let t2 = target.clone();
            let _ = on_ui(&host.handle, move |s, _| s.release_virtual_input(&g2.id, &t2, "gesture_failed"));
            return Err(error.with_data(json!({"steps_done": done})));
        }
        if total > 1 {
            std::thread::sleep(Duration::from_millis(8));
        }
    }
    Ok(json!({"target": target, "action": action, "at": {"x": at.0, "y": at.1}, "steps": total, "frame_revision": frame}))
}

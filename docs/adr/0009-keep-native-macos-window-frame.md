# Keep macOS traffic lights; draw control only on Linux and Windows

ADR-0007 has Rodtty draw its own title bar and window controls on every desktop
system. That holds on Linux and Windows, where neither has a frame Rodtty can
style. On macOS it does not: the app-drawn frame there shipped without its
three advertised behaviors.

An undecorated macOS window hands Rodtty movement, sizing and minimize. Three
defects followed from that, and each traces to the borderless frame rather than
to the code that draws it:

- **The window would not stay where it was put.** With the frame gone, the app
  had to answer "is the window zoomed?" itself. winit's `is_zoomed` cannot ask
  AppKit directly on an undecorated window, so it sets `NSWindowStyleMask` to
  `Titled | Resizable`, queries, and restores the previous mask. Rodtty asks
  once per frame, so AppKit re-derived the window's frame many times a second
  and the window returned to its remembered origin.
- **No traffic lights.** A borderless window has none, and Rodtty's own circles
  are not the system's: they do not match the owner's accent, hover reveals, or
  Option-click behaviors.
- **Presses went missing.** Moving the window calls
  `performWindowDragWithEvent:`, whose nested AppKit tracking loop can swallow
  the matching mouse-up. The macOS input path had no recovery for a press whose
  release never arrived, so the next click landed as an extension of the
  previous one.

macOS now keeps its native frame. The window stays decorated, so AppKit owns
movement, sizing and zoom, `is_zoomed` answers without a style-mask round trip,
and the traffic lights are the system's own. Rodtty draws a full-size content
view under a transparent titlebar
(`App::titlebar(Titlebar::new(44.0))`), keeps its header clear of the lights
with `Cx::chrome_insets().leading`, and marks the band `.window_drag()`.

Linux and Windows keep the app-drawn frame from ADR-0007, where it works and
where the alternative is an unstylable native frame.

## Consequences

- The two frames diverge: `frame::placement()` returns `Hosted` on macOS and
  `Trailing` elsewhere, and only `Trailing` draws window controls.
- `frame::borderless()` is false on macOS. The themed six-point border and the
  resize band exist only where Rodtty supplies the frame.
- macOS gains a titlebar Rodtty does not control the height of beyond requesting
  it: the lights sit where AppKit puts them for the requested band height.
- A native frame has a native title bar, so the app no longer owns the window's
  full vertical extent on macOS. The header starts below it.

Supersedes ADR-0007 on macOS only.

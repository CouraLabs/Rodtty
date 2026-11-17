//! Only native frame mechanics; appearance and lifecycle remain application owned.
use winit::window::{ResizeDirection, Window};

/// Window-local logical coordinates, independent of app zoom. Kept at the native
/// boundary so a Wayland request uses the original winit pointer seat and serial.
pub(super) fn resize_edge(window: &Window, border: f32, point: (f32, f32)) -> Option<ResizeDirection> {
    if !cfg!(target_os = "linux") || border <= 0.0 || window.is_decorated() || !window.is_resizable()
        || window.is_maximized() || window.fullscreen().is_some() { return None; }
    let size = window.inner_size().to_logical::<f32>(window.scale_factor());
    let (x, y) = point;
    if x < 0.0 || y < 0.0 || x >= size.width || y >= size.height { return None; }
    let (left, right, top, bottom) = (x < border, x >= size.width - border, y < border, y >= size.height - border);
    use ResizeDirection::*;
    match (left, right, top, bottom) {
        (true, _, true, _) => Some(NorthWest),
        (_, true, true, _) => Some(NorthEast),
        (true, _, _, true) => Some(SouthWest),
        (_, true, _, true) => Some(SouthEast),
        (true, _, _, _) => Some(West),
        (_, true, _, _) => Some(East),
        (_, _, true, _) => Some(North),
        (_, _, _, true) => Some(South),
        _ => None,
    }
}

pub(super) fn resize_cursor(edge: ResizeDirection) -> winit::window::CursorIcon {
    use ResizeDirection::*;
    use winit::window::CursorIcon;
    match edge {
        North | South => CursorIcon::NsResize,
        West | East => CursorIcon::EwResize,
        NorthWest | SouthEast => CursorIcon::NwseResize,
        NorthEast | SouthWest => CursorIcon::NeswResize,
    }
}

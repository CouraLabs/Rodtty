//! AppKit has no `drag_resize_window`. Track a border gesture on the UI thread.
//! winit's NSView forwards mouseDragged outside its bounds while a button is down,
//! and mouseUp sends the final move before release. Never derive screen deltas from
//! view-local points: the view itself moves while resizing the west/south edges.
use objc2_foundation::MainThreadMarker;
use objc2_app_kit::NSEvent;
use objc2_foundation::{NSPoint, NSRect, NSSize};
use winit::window::{ResizeDirection, Window};
use crate::platform::macos::ns_window;

pub(super) struct Gesture {
    edge: ResizeDirection,
    frame: NSRect,
    pointer: NSPoint,
    min: NSSize,
    max: NSSize,
}

impl Gesture {
    pub(super) fn begin(window: &Window, edge: ResizeDirection) -> Option<Self> {
        let _main = MainThreadMarker::new()?;
        let ns = ns_window(window)?;
        // SAFETY: AppKit main thread, live NSWindow. These values are global
        // bottom-left screen points, independent of Retina scale and app zoom.
        unsafe {
            Some(Self { edge, frame: ns.frame(), pointer: NSEvent::mouseLocation(),
                min: ns.minSize(), max: ns.maxSize() })
        }
    }

    pub(super) fn update(&self, window: &Window) {
        let Some(_main) = MainThreadMarker::new() else { return };
        let Some(ns) = ns_window(window) else { return };
        // SAFETY: global screen location on the AppKit main thread.
        let pointer = unsafe { NSEvent::mouseLocation() };
        let (dx, dy) = (pointer.x - self.pointer.x, pointer.y - self.pointer.y);
        use ResizeDirection::*;
        let west = matches!(self.edge, West | NorthWest | SouthWest);
        let east = matches!(self.edge, East | NorthEast | SouthEast);
        let north = matches!(self.edge, North | NorthWest | NorthEast);
        let south = matches!(self.edge, South | SouthWest | SouthEast);
        let mut frame = self.frame;
        if west || east {
            frame.size.width = (self.frame.size.width + if west { -dx } else { dx })
                .clamp(self.min.width.max(1.0), self.max.width.max(self.min.width).max(1.0));
            if west { frame.origin.x = self.frame.origin.x + self.frame.size.width - frame.size.width; }
        }
        if north || south {
            frame.size.height = (self.frame.size.height + if south { -dy } else { dy })
                .clamp(self.min.height.max(1.0), self.max.height.max(self.min.height).max(1.0));
            if south { frame.origin.y = self.frame.origin.y + self.frame.size.height - frame.size.height; }
        }
        // Keep the opposite edge fixed even at the minimum. AppKit's origin is
        // bottom-left: north grows upward; south moves the origin downward.
        // winit's view frame notification delivers the resulting surface resize.
        if ns.frame() != frame { ns.setFrame_display(frame, true); }
    }
}

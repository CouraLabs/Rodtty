//! CEF on macOS needs the shared `NSApplication` to implement
//! `CefAppProtocol`. Rodtty installs this subclass as the shared application
//! before winit asks for it; winit then swizzles `sendEvent:` on it as usual.
use cef::application_mac::{CefAppProtocol, CrAppControlProtocol, CrAppProtocol};
use objc2::{
    ClassType, DefinedClass, MainThreadMarker, define_class, msg_send,
    rc::Retained,
    runtime::{Bool, NSObjectProtocol},
};
use objc2_app_kit::{NSApplication, NSEvent};
use std::cell::Cell;

#[derive(Default)]
pub struct Ivars {
    handling_send_event: Cell<Bool>,
}

define_class!(
    #[unsafe(super(NSApplication))]
    #[ivars = Ivars]
    #[name = "RodttyApplication"]
    pub struct RodttyApplication;

    impl RodttyApplication {
        #[unsafe(method(sendEvent:))]
        unsafe fn send_event(&self, event: &NSEvent) {
            let was = self.ivars().handling_send_event.get();
            self.ivars().handling_send_event.set(Bool::YES);
            let _: () = unsafe { msg_send![super(self), sendEvent: event] };
            self.ivars().handling_send_event.set(was);
        }
    }

    unsafe impl NSObjectProtocol for RodttyApplication {}

    unsafe impl CrAppProtocol for RodttyApplication {
        #[unsafe(method(isHandlingSendEvent))]
        unsafe fn _is_handling_send_event(&self) -> Bool {
            self.ivars().handling_send_event.get()
        }
    }

    unsafe impl CrAppControlProtocol for RodttyApplication {
        #[unsafe(method(setHandlingSendEvent:))]
        unsafe fn _set_handling_send_event(&self, handling: Bool) {
            self.ivars().handling_send_event.set(handling);
        }
    }

    unsafe impl CefAppProtocol for RodttyApplication {}
);

/// Make `RodttyApplication` the shared application. Must run on the main
/// thread before anything else touches `NSApp`.
pub fn install() -> bool {
    let Some(_mtm) = MainThreadMarker::new() else {
        return false;
    };
    let app: Retained<NSApplication> =
        unsafe { msg_send![RodttyApplication::class(), sharedApplication] };
    app.isKindOfClass(RodttyApplication::class())
}

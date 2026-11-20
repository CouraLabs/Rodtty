//! `rodtty-browser-helper`: the CEF subprocess (renderer, GPU, network and
//! utility processes). Rodtty's main executable never runs as a CEF
//! subprocess, so its startup, instance lock and terminal sessions stay
//! independent of the browser. On macOS this binary lives in the `Rodtty
//! Helper*.app` bundles and enters Chromium's sandbox first.
use cef::{args::Args, *};

fn main() {
    let args = Args::new();

    #[cfg(target_os = "macos")]
    let _sandbox = {
        let mut sandbox = cef::sandbox::Sandbox::new();
        sandbox.initialize(args.as_main_args());
        sandbox
    };

    #[cfg(target_os = "macos")]
    let _loader = {
        let loader = library_loader::LibraryLoader::new(
            &std::env::current_exe().expect("helper path"),
            true,
        );
        if !loader.load() {
            std::process::exit(1);
        }
        loader
    };

    let _ = api_hash(sys::CEF_API_VERSION_LAST, 0);
    let code = execute_process(
        Some(args.as_main_args()),
        None::<&mut App>,
        std::ptr::null_mut(),
    );
    std::process::exit(code.max(0));
}

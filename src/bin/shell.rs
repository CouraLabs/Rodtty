use std::{collections::HashMap, ffi::OsString, path::Path};

fn disabled(args: Vec<OsString>) -> i32 {
    eprintln!(
        "rodtty: {}: disabled for this session",
        args[0].to_string_lossy()
    );
    126
}

fn main() {
    let utilities = brush_coreutils_builtins::bundled_commands();
    let args: Vec<OsString> = std::env::args_os().collect();
    let name = Path::new(&args[0])
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("");
    let disabled_names = rodtty::profile::disabled_from_env();
    // Native hard links execute as ordinary external programs. This avoids
    // Brush's experimental builtin shims serializing pipeline stages.
    if let Some(run) = utilities.get(name) {
        let code = if disabled_names.contains(name) {
            disabled(args)
        } else {
            run(args)
        };
        std::process::exit(code);
    }
    // Disabled builtins also guard ordinary bare-name lookups if PATH changes.
    // Enabled utilities remain external processes for pipeline/job semantics.
    let guards: HashMap<String, brush_shell::bundled::BundledFn> = disabled_names
        .into_iter()
        .map(|name| (name, disabled as brush_shell::bundled::BundledFn))
        .collect();
    brush_shell::bundled::install(guards);
    brush_shell::entry::run();
}

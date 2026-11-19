/// A file under `assets/`, embedded by its path from there.
#[allow(unused_macros)]
macro_rules! asset_bytes {
    ($path:expr) => {
        include_bytes!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $path))
    };
}
/// A text file under `assets/`, embedded by its path from there.
#[allow(unused_macros)]
macro_rules! asset_str {
    ($path:expr) => {
        include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/assets/", $path))
    };
}

pub mod config;
pub mod files;
pub mod git;
pub mod instance;
pub mod profile;
pub mod prompt;
pub mod session;
pub mod work;

#[cfg(feature = "desktop")]
pub mod desktop;

#[cfg(test)]
mod test_support;

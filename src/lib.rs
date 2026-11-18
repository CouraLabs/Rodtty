pub mod config;
pub mod files;
pub mod git;
pub mod instance;
pub mod profile;
pub mod session;
pub mod work;

#[cfg(feature = "desktop")]
pub mod desktop;

#[cfg(test)]
mod test_support;

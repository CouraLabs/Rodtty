//! Local, per-owner launch coordination. This does not expose a network listener.
use anyhow::{Context, Result};
use fs2::FileExt;
use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use std::{
    fs::{File, OpenOptions},
    path::{Path, PathBuf},
    time::{SystemTime, UNIX_EPOCH},
};

pub struct Instance {
    _lock: File,
    root: PathBuf,
}

impl Instance {
    pub fn acquire(root: &Path) -> Result<Option<Self>> {
        std::fs::create_dir_all(root)?;
        let lock = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(root.join("host.lock"))?;
        match lock.try_lock_exclusive() {
            Ok(()) => Ok(Some(Self {
                _lock: lock,
                root: root.to_owned(),
            })),
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                Self::request_open(root)?;
                Ok(None)
            }
            Err(error) => Err(error).context("lock Rodtty host instance"),
        }
    }

    fn request_open(root: &Path) -> Result<()> {
        let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        std::fs::write(
            root.join("open.request"),
            format!("{}-{id}", std::process::id()),
        )?;
        Ok(())
    }

    pub fn watch(&self, open: impl Fn() + Send + 'static) -> Result<RecommendedWatcher> {
        let request = self.root.join("open.request");
        let mut last = std::fs::read_to_string(&request).unwrap_or_default();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|event| event.kind.is_modify() || event.kind.is_create())
                    && let Ok(next) = std::fs::read_to_string(&request)
                    && !next.is_empty()
                    && next != last
                {
                    last = next;
                    open();
                }
            })?;
        watcher.watch(&self.root, RecursiveMode::NonRecursive)?;
        Ok(watcher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn another_launch_requests_reopen_without_owning_sessions() {
        let root = tempfile::tempdir().unwrap();
        let host = Instance::acquire(root.path()).unwrap().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let _watcher = host
            .watch(move || {
                let _ = tx.send(());
            })
            .unwrap();
        assert!(Instance::acquire(root.path()).unwrap().is_none());
        rx.recv_timeout(std::time::Duration::from_secs(5)).unwrap();
        drop(host);
        assert!(Instance::acquire(root.path()).unwrap().is_some());
    }
}

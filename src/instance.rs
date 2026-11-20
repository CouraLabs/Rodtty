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
    compatibility: Option<Box<Instance>>,
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
                compatibility: None,
            })),
            // Unix reports a held lock as WouldBlock; Windows reports
            // ERROR_LOCK_VIOLATION, which fs2 names `lock_contended_error`.
            Err(error)
                if error.kind() == std::io::ErrorKind::WouldBlock
                    || error.raw_os_error() == fs2::lock_contended_error().raw_os_error() =>
            {
                Self::request_open(root)?;
                Ok(None)
            }
            Err(error) => Err(error).context("lock Rodtty host instance"),
        }
    }

    /// Lock the legacy root first so an older live host remains authoritative.
    /// Holding it for our lifetime also makes older launchers request our window.
    pub fn acquire_owner(root: &Path, legacy: &Path) -> Result<Option<Self>> {
        if root == legacy {
            return Self::acquire(root);
        }
        let Some(compatibility) = Self::acquire(legacy)? else {
            return Ok(None);
        };
        let Some(mut instance) = Self::acquire(root)? else {
            return Ok(None);
        };
        instance.compatibility = Some(Box::new(compatibility));
        Ok(Some(instance))
    }

    fn request_open(root: &Path) -> Result<()> {
        use std::io::Write;
        let id = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();
        // Replace the request file instead of rewriting it in place: macOS's
        // FSEvents reports no event for a rewrite of a file that already
        // exists, so every launch after the first would go unheard. A rename
        // over it is reported everywhere, and the host never reads a partial
        // request.
        let mut request = tempfile::NamedTempFile::new_in(root)?;
        write!(request, "{}-{id}", std::process::id())?;
        request
            .persist(root.join("open.request"))
            .map_err(|error| error.error)?;
        Ok(())
    }

    pub fn watch(&self, open: impl Fn() + Send + 'static) -> Result<RecommendedWatcher> {
        let mut roots = vec![self.root.clone()];
        if let Some(compatibility) = &self.compatibility {
            roots.push(compatibility.root.clone());
        }
        let mut requests: Vec<_> = roots
            .iter()
            .map(|root| {
                let path = root.join("open.request");
                let last = std::fs::read_to_string(&path).unwrap_or_default();
                (path, last)
            })
            .collect();
        let mut watcher =
            notify::recommended_watcher(move |event: notify::Result<notify::Event>| {
                if event.is_ok_and(|event| event.kind.is_modify() || event.kind.is_create()) {
                    for (request, last) in &mut requests {
                        if let Ok(next) = std::fs::read_to_string(request)
                            && !next.is_empty()
                            && next != *last
                        {
                            *last = next;
                            open();
                        }
                    }
                }
            })?;
        for root in roots {
            watcher.watch(&root, RecursiveMode::NonRecursive)?;
        }
        Ok(watcher)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn another_launch_requests_reopen_without_owning_sessions() {
        let root = crate::test_support::tempdir().unwrap();
        let host = Instance::acquire(root.path()).unwrap().unwrap();
        let (tx, rx) = std::sync::mpsc::channel();
        let _watcher = host
            .watch(move || {
                let _ = tx.send(());
            })
            .unwrap();
        // macOS FSEvents can drop changes made while a new stream is still
        // registering, which a busy test host hits and a real second launch
        // never does. Relaunch until the host hears one request.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        loop {
            assert!(Instance::acquire(root.path()).unwrap().is_none());
            if rx
                .recv_timeout(std::time::Duration::from_millis(500))
                .is_ok()
            {
                break;
            }
            assert!(
                std::time::Instant::now() < deadline,
                "host never heard a reopen request"
            );
        }
        drop(host);
        // A test running in parallel may fork a PTY child while the lock is
        // open; the child holds the descriptor until it execs and closes it.
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
        while std::time::Instant::now() < deadline && Instance::acquire(root.path()).unwrap().is_none() {
            std::thread::sleep(std::time::Duration::from_millis(20));
        }
        assert!(Instance::acquire(root.path()).unwrap().is_some());
    }
}

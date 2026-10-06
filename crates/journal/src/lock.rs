//! The exclusive journal-writer lock (SPEC §9.1, §9.4). Held for the life of
//! the companion; a second writer cannot acquire it, even within one process,
//! because BSD `flock` locks belong to the open file description.

use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum LockError {
    /// Another live writer holds the lock.
    Held { path: PathBuf },
    Io { path: PathBuf, error: std::io::Error },
}

impl std::fmt::Display for LockError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Held { path } => write!(f, "writer lock {} is held by another process", path.display()),
            Self::Io { path, error } => write!(f, "writer lock {}: {error}", path.display()),
        }
    }
}

impl std::error::Error for LockError {}

#[derive(Debug)]
pub struct WriterLock {
    file: File,
    path: PathBuf,
}

impl WriterLock {
    /// Acquires `writer.lock` in `store_dir` without waiting, and records the
    /// holder's PID in it for diagnostics.
    pub fn acquire(store_dir: &Path) -> Result<Self, LockError> {
        let path = store_dir.join("writer.lock");
        let io = |error| LockError::Io { path: path.clone(), error };
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .mode(0o600)
            .open(&path)
            .map_err(io)?;
        // SAFETY: the descriptor belongs to `file`, which stays open for the
        // lifetime of the returned lock.
        let rc = unsafe { libc::flock(file.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) };
        if rc != 0 {
            let error = std::io::Error::last_os_error();
            return Err(if error.raw_os_error() == Some(libc::EWOULDBLOCK) {
                LockError::Held { path }
            } else {
                io(error)
            });
        }
        file.set_len(0).map_err(io)?;
        writeln!(file, "{}", std::process::id()).map_err(io)?;
        Ok(Self { file, path })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn is_held(&self) -> bool {
        self.file.as_raw_fd() >= 0
    }
}

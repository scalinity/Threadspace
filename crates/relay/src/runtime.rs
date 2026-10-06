//! The private runtime directory and sockets (SPEC §8.1): a short random
//! `/tmp/ts.<uid>.<random>/` created atomically with mode 0700, verified
//! without following symlinks, holding 0600 sockets whose paths fit
//! `sockaddr_un.sun_path[104]`.

use std::fs::{self, DirBuilder};
use std::io;
use std::os::unix::fs::{DirBuilderExt, FileTypeExt, MetadataExt, PermissionsExt};
use std::os::unix::net::UnixListener;
use std::path::{Path, PathBuf};

use crate::peer::current_euid;

const SUN_PATH_MAX: usize = 104;

fn verify_private_dir(path: &Path) -> io::Result<()> {
    let metadata = fs::symlink_metadata(path)?;
    let mode = metadata.mode() & 0o777;
    if !metadata.file_type().is_dir() || metadata.uid() != current_euid() || mode != 0o700 {
        return Err(io::Error::new(
            io::ErrorKind::PermissionDenied,
            format!(
                "{} is not a private directory owned by this user",
                path.display()
            ),
        ));
    }
    Ok(())
}

fn runtime_prefix() -> String {
    format!("ts.{}.", current_euid())
}

/// Creates a fresh private runtime directory under `/tmp`.
pub fn create_runtime_dir() -> io::Result<PathBuf> {
    for _ in 0..16 {
        // SAFETY: arc4random has no preconditions.
        let random = unsafe { libc::arc4random() };
        let path = PathBuf::from(format!("/tmp/{}{random:08x}", runtime_prefix()));
        match DirBuilder::new().mode(0o700).create(&path) {
            Ok(()) => {
                // Mode is filtered by umask at creation; set it explicitly, then verify.
                fs::set_permissions(&path, fs::Permissions::from_mode(0o700))?;
                verify_private_dir(&path)?;
                return Ok(path);
            }
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::AlreadyExists,
        "could not allocate a runtime directory",
    ))
}

/// Removes an earlier runtime directory of this user, if it is one.
pub fn remove_stale_runtime_dir(path: &Path) {
    let is_ours = path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| name.starts_with(&runtime_prefix()))
        && path.parent() == Some(Path::new("/tmp"))
        && verify_private_dir(path).is_ok();
    if is_ours {
        let _ = fs::remove_dir_all(path);
    }
}

/// Binds a 0600 Unix stream socket inside a verified private directory.
pub fn bind_private_socket(dir: &Path, name: &str) -> io::Result<(UnixListener, PathBuf)> {
    verify_private_dir(dir)?;
    let path = dir.join(name);
    if path.as_os_str().len() >= SUN_PATH_MAX {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "socket path exceeds sun_path",
        ));
    }
    if let Ok(existing) = fs::symlink_metadata(&path) {
        if !existing.file_type().is_socket() {
            return Err(io::Error::new(
                io::ErrorKind::AlreadyExists,
                "non-socket at socket path",
            ));
        }
        fs::remove_file(&path)?;
    }
    let listener = UnixListener::bind(&path)?;
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600))?;
    Ok((listener, path))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn creates_private_dir_and_socket() {
        let dir = create_runtime_dir().expect("dir");
        assert!(dir.as_os_str().len() < 40, "short path: {}", dir.display());
        let (_listener, socket) = bind_private_socket(&dir, "control.sock").expect("bind");
        let mode = fs::symlink_metadata(&socket).expect("stat").mode() & 0o777;
        assert_eq!(mode, 0o600);
        remove_stale_runtime_dir(&dir);
        assert!(!dir.exists());
    }

    #[test]
    fn refuses_foreign_or_loose_directories() {
        assert!(bind_private_socket(Path::new("/tmp"), "x.sock").is_err());
        remove_stale_runtime_dir(Path::new("/tmp"));
        assert!(Path::new("/tmp").exists());
    }
}

//! Terminal device identity (SPEC §4.12, §13.3). The kernel reports a
//! process's controlling device as `e_tdev`; a terminal application reports a
//! tab's `/dev/tty…` pathname. The two meet only through `stat(path).st_rdev`
//! of a character device. `st_dev` is the containing filesystem, not this
//! join, and the device is never opened or read.

use std::ffi::CString;
use std::mem::MaybeUninit;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TtyError {
    /// Only `/dev/` device paths are considered.
    NotDevicePath,
    Stat {
        errno: i32,
    },
    NotCharacterDevice,
}

impl TtyError {
    pub fn code(&self) -> &'static str {
        match self {
            Self::NotDevicePath => "TTY_NOT_DEVICE_PATH",
            Self::Stat { .. } => "TTY_STAT_FAILED",
            Self::NotCharacterDevice => "TTY_NOT_CHARACTER_DEVICE",
        }
    }
}

impl std::fmt::Display for TtyError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NotDevicePath => f.write_str("not a /dev/ path"),
            Self::Stat { errno } => write!(f, "stat failed (errno {errno})"),
            Self::NotCharacterDevice => f.write_str("not a character device"),
        }
    }
}

impl std::error::Error for TtyError {}

/// The normalized device number (`st_rdev` as the kernel's unsigned `dev_t`
/// bits) of the character device at `path`, for comparison with `e_tdev`.
pub fn character_device(path: &str) -> Result<u32, TtyError> {
    if !path.starts_with("/dev/") || path.contains("/../") || path.contains('\0') {
        return Err(TtyError::NotDevicePath);
    }
    let text = CString::new(path).map_err(|_| TtyError::NotDevicePath)?;
    let mut status = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: `text` is NUL-terminated and `status` is writable; `stat` does
    // not open the device.
    let rc = unsafe { libc::stat(text.as_ptr(), status.as_mut_ptr()) };
    if rc != 0 {
        return Err(TtyError::Stat {
            errno: std::io::Error::last_os_error().raw_os_error().unwrap_or(0),
        });
    }
    // SAFETY: `stat` succeeded and filled the structure.
    let status = unsafe { status.assume_init() };
    if status.st_mode & libc::S_IFMT != libc::S_IFCHR {
        return Err(TtyError::NotCharacterDevice);
    }
    Ok(status.st_rdev as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dev_null_is_a_character_device_distinct_from_its_filesystem() {
        let rdev = character_device("/dev/null").expect("/dev/null");
        // st_dev of /dev/null is the devfs mount; st_rdev is the device itself.
        let text = CString::new("/dev/null").expect("cstr");
        let mut status = MaybeUninit::<libc::stat>::zeroed();
        // SAFETY: as above.
        assert_eq!(unsafe { libc::stat(text.as_ptr(), status.as_mut_ptr()) }, 0);
        // SAFETY: stat succeeded.
        let status = unsafe { status.assume_init() };
        assert_ne!(rdev, status.st_dev as u32, "st_rdev must not be st_dev");
    }

    #[test]
    fn rejects_non_device_paths_and_non_character_files() {
        assert_eq!(character_device("ttys001"), Err(TtyError::NotDevicePath));
        assert_eq!(character_device("/etc/hosts"), Err(TtyError::NotDevicePath));
        assert_eq!(
            character_device("/dev/../etc/hosts"),
            Err(TtyError::NotDevicePath)
        );
        assert!(matches!(
            character_device("/dev/ttys-does-not-exist"),
            Err(TtyError::Stat { .. })
        ));
        assert_eq!(
            character_device("/dev/fd"),
            Err(TtyError::NotCharacterDevice)
        );
    }
}

//! Kernel process incarnation evidence (SPEC §4.2, §4.5). A ProcessKey is
//! endpoint + boot + PID + kernel birth; PID alone is never identity.

use std::ffi::CStr;
use std::mem::{MaybeUninit, size_of};
use std::path::PathBuf;

/// `e_tdev` value meaning "no controlling terminal".
const NODEV: u32 = u32::MAX;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProcessSample {
    pub pid: i32,
    pub ppid: i32,
    pub uid: u32,
    pub start_seconds: u64,
    pub start_microseconds: u32,
    /// Controlling-device number (`e_tdev`); `None` for no controlling terminal.
    pub controlling_device: Option<u32>,
    pub pgid: u32,
    pub tpgid: u32,
    pub comm: String,
}

impl ProcessSample {
    /// Same incarnation: identical PID and kernel birth instant.
    pub fn same_incarnation(&self, other: &ProcessSample) -> bool {
        self.pid == other.pid
            && self.start_seconds == other.start_seconds
            && self.start_microseconds == other.start_microseconds
    }
}

#[derive(Debug)]
pub enum ProcessError {
    /// The process does not exist or the kernel refused the read.
    Unavailable { pid: i32, errno: i32 },
    /// A short read: the structure was not returned at its exact length.
    ShortRead { pid: i32, bytes: i32 },
    Sysctl { name: &'static str, errno: i32 },
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable { pid, errno } => write!(f, "process {pid} unavailable (errno {errno})"),
            Self::ShortRead { pid, bytes } => write!(f, "process {pid}: short proc_pidinfo read ({bytes} bytes)"),
            Self::Sysctl { name, errno } => write!(f, "sysctl {name} failed (errno {errno})"),
        }
    }
}

impl std::error::Error for ProcessError {}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

/// Samples `proc_pidinfo(PROC_PIDTBSDINFO)`, requiring the exact structure
/// length; denied or short reads leave the evidence unavailable.
pub fn sample(pid: i32) -> Result<ProcessSample, ProcessError> {
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let expected = size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: the buffer is a properly aligned, zeroed `proc_bsdinfo` of the
    // size passed to the kernel.
    let bytes = unsafe {
        libc::proc_pidinfo(pid, libc::PROC_PIDTBSDINFO, 0, info.as_mut_ptr().cast(), expected)
    };
    if bytes <= 0 {
        return Err(ProcessError::Unavailable { pid, errno: errno() });
    }
    if bytes != expected {
        return Err(ProcessError::ShortRead { pid, bytes });
    }
    // SAFETY: the kernel filled exactly `expected` bytes.
    let info = unsafe { info.assume_init() };
    // SAFETY: `pbi_comm` is a NUL-terminated (or zero-padded) fixed array.
    let comm = unsafe { CStr::from_ptr(info.pbi_comm.as_ptr()) }.to_string_lossy().into_owned();
    Ok(ProcessSample {
        pid: info.pbi_pid as i32,
        ppid: info.pbi_ppid as i32,
        uid: info.pbi_uid,
        start_seconds: info.pbi_start_tvsec,
        start_microseconds: info.pbi_start_tvusec as u32,
        controlling_device: (info.e_tdev != NODEV).then_some(info.e_tdev),
        pgid: info.pbi_pgid,
        tpgid: info.e_tpgid,
        comm,
    })
}

/// The executable path the kernel reports for `pid`.
pub fn executable_path(pid: i32) -> Result<PathBuf, ProcessError> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is writable for the size passed.
    let length = unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return Err(ProcessError::Unavailable { pid, errno: errno() });
    }
    buffer.truncate(length as usize);
    Ok(PathBuf::from(String::from_utf8_lossy(&buffer).into_owned()))
}

/// The kernel boot-session UUID (`kern.bootsessionuuid`); ProcessKeys from
/// different boots never compare equal.
pub fn boot_session_id() -> Result<String, ProcessError> {
    const NAME: &CStr = c"kern.bootsessionuuid";
    let mut buffer = [0u8; 64];
    let mut length = buffer.len();
    // SAFETY: `buffer`/`length` describe a writable region; no new value is set.
    let rc = unsafe {
        libc::sysctlbyname(NAME.as_ptr(), buffer.as_mut_ptr().cast(), &mut length, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return Err(ProcessError::Sysctl { name: "kern.bootsessionuuid", errno: errno() });
    }
    let text = CStr::from_bytes_until_nul(&buffer[..length.min(buffer.len())])
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_else(|_| String::from_utf8_lossy(&buffer[..length]).into_owned());
    Ok(text)
}

/// macOS product version components from `kern.osproductversion`.
pub fn os_product_version() -> Result<(u32, u32, u32), ProcessError> {
    const NAME: &CStr = c"kern.osproductversion";
    let mut buffer = [0u8; 32];
    let mut length = buffer.len();
    // SAFETY: as in `boot_session_id`.
    let rc = unsafe {
        libc::sysctlbyname(NAME.as_ptr(), buffer.as_mut_ptr().cast(), &mut length, std::ptr::null_mut(), 0)
    };
    if rc != 0 {
        return Err(ProcessError::Sysctl { name: "kern.osproductversion", errno: errno() });
    }
    let text = CStr::from_bytes_until_nul(&buffer[..length.min(buffer.len())])
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut parts = text.split('.').map(|part| part.parse::<u32>().unwrap_or(0));
    Ok((parts.next().unwrap_or(0), parts.next().unwrap_or(0), parts.next().unwrap_or(0)))
}

/// The selected deployment floor (SPEC §18.9): macOS 26.0, enforced at runtime
/// as well as in bundle configuration.
pub fn meets_minimum_macos() -> bool {
    os_product_version().map(|(major, _, _)| major >= 26).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn samples_this_process() {
        let pid = std::process::id() as i32;
        let first = sample(pid).expect("sample self");
        let second = sample(pid).expect("resample self");
        assert_eq!(first.pid, pid);
        assert!(first.same_incarnation(&second));
        assert!(first.start_seconds > 0);
        assert!(executable_path(pid).expect("path").is_absolute());
    }

    #[test]
    fn missing_process_is_unavailable() {
        assert!(matches!(sample(i32::MAX - 1), Err(ProcessError::Unavailable { .. })));
    }

    #[test]
    fn boot_session_is_a_uuid() {
        let id = boot_session_id().expect("boot id");
        assert_eq!(id.len(), 36, "{id}");
    }

    #[test]
    fn target_mac_meets_floor() {
        assert!(meets_minimum_macos());
    }
}

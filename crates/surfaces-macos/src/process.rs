//! Kernel process incarnation evidence (SPEC §4.2, §4.5). A ProcessKey is
//! endpoint + boot + PID + kernel birth; PID alone is never identity, and the
//! executable is a separate axis because `exec` keeps both PID and birth.

use std::ffi::{CStr, CString};
use std::mem::{MaybeUninit, size_of};
use std::path::PathBuf;

/// `e_tdev` value meaning "no controlling terminal".
const NODEV: u32 = u32::MAX;
/// `pbi_status` for a stopped (suspended or traced) process.
pub const SSTOP: u32 = 4;
/// `pbi_status` for a zombie awaiting collection.
pub const SZOMB: u32 = 5;

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
    /// Foreground process group of the controlling terminal (`e_tpgid`).
    pub tpgid: u32,
    /// Kernel process state (`pbi_status`: SIDL/SRUN/SSLEEP/SSTOP/SZOMB).
    pub status: u32,
    pub comm: String,
}

impl ProcessSample {
    /// Same incarnation: identical PID and kernel birth instant.
    pub fn same_incarnation(&self, other: &ProcessSample) -> bool {
        self.pid == other.pid
            && self.start_seconds == other.start_seconds
            && self.start_microseconds == other.start_microseconds
    }

    /// The process group owns its controlling terminal's foreground.
    pub fn is_terminal_foreground(&self) -> bool {
        self.controlling_device.is_some() && self.tpgid != 0 && self.tpgid == self.pgid
    }

    pub fn is_stopped(&self) -> bool {
        self.status == SSTOP
    }

    /// Kernel birth as microseconds since the epoch, for ordering checks.
    pub fn birth_micros(&self) -> u128 {
        u128::from(self.start_seconds) * 1_000_000 + u128::from(self.start_microseconds)
    }
}

/// The running image: the kernel's executable path plus the file identity
/// found at that path. A changed path, name or file is a replaced executable.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ExecutableIdentity {
    pub path: String,
    /// `dev:ino` of the file at `path` when sampled; `None` if it was unlinked.
    pub file_id: Option<String>,
}

impl ExecutableIdentity {
    /// Canonical single-string form stored with a process incarnation.
    pub fn canonical(&self) -> String {
        match &self.file_id {
            Some(file_id) => format!("{}#{file_id}", self.path),
            None => format!("{}#unlinked", self.path),
        }
    }
}

/// One incarnation sample: kernel BSD info and the executable read inside a
/// birth-checked bracket, so both belong to the same process.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Incarnation {
    pub sample: ProcessSample,
    pub executable: ExecutableIdentity,
}

impl Incarnation {
    /// Same process incarnation running the same executable.
    pub fn same_process_and_image(&self, other: &Incarnation) -> bool {
        self.sample.same_incarnation(&other.sample) && self.executable == other.executable
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProcessError {
    /// No such process (`ESRCH`): it exited or never existed.
    Vanished {
        pid: i32,
    },
    /// The kernel refused the read (`EPERM`).
    Denied {
        pid: i32,
        errno: i32,
    },
    /// Any other failed read.
    Unavailable {
        pid: i32,
        errno: i32,
    },
    /// A short read: the structure was not returned at its exact length.
    ShortRead {
        pid: i32,
        bytes: i32,
    },
    /// The PID's incarnation changed between the reads of one sample.
    ChangedDuringCapture {
        pid: i32,
    },
    Sysctl {
        name: &'static str,
        errno: i32,
    },
}

impl ProcessError {
    /// Stable code for evidence records.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Vanished { .. } => "PROCESS_VANISHED",
            Self::Denied { .. } => "PROCESS_READ_DENIED",
            Self::Unavailable { .. } => "PROCESS_UNAVAILABLE",
            Self::ShortRead { .. } => "PROCESS_SHORT_READ",
            Self::ChangedDuringCapture { .. } => "PROCESS_CHANGED_DURING_CAPTURE",
            Self::Sysctl { .. } => "SYSCTL_FAILED",
        }
    }
}

impl std::fmt::Display for ProcessError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Vanished { pid } => write!(f, "process {pid} vanished"),
            Self::Denied { pid, errno } => write!(f, "process {pid} read denied (errno {errno})"),
            Self::Unavailable { pid, errno } => {
                write!(f, "process {pid} unavailable (errno {errno})")
            }
            Self::ShortRead { pid, bytes } => {
                write!(f, "process {pid}: short proc_pidinfo read ({bytes} bytes)")
            }
            Self::ChangedDuringCapture { pid } => {
                write!(f, "process {pid} changed incarnation during capture")
            }
            Self::Sysctl { name, errno } => write!(f, "sysctl {name} failed (errno {errno})"),
        }
    }
}

impl std::error::Error for ProcessError {}

fn errno() -> i32 {
    std::io::Error::last_os_error().raw_os_error().unwrap_or(0)
}

fn read_error(pid: i32, errno: i32) -> ProcessError {
    match errno {
        libc::ESRCH => ProcessError::Vanished { pid },
        libc::EPERM => ProcessError::Denied { pid, errno },
        _ => ProcessError::Unavailable { pid, errno },
    }
}

/// Samples `proc_pidinfo(PROC_PIDTBSDINFO)`, requiring the exact structure
/// length; denied or short reads leave the evidence unavailable.
pub fn sample(pid: i32) -> Result<ProcessSample, ProcessError> {
    if pid <= 0 {
        return Err(ProcessError::Vanished { pid });
    }
    let mut info = MaybeUninit::<libc::proc_bsdinfo>::zeroed();
    let expected = size_of::<libc::proc_bsdinfo>() as i32;
    // SAFETY: the buffer is a properly aligned, zeroed `proc_bsdinfo` of the
    // size passed to the kernel.
    let bytes = unsafe {
        libc::proc_pidinfo(
            pid,
            libc::PROC_PIDTBSDINFO,
            0,
            info.as_mut_ptr().cast(),
            expected,
        )
    };
    if bytes <= 0 {
        return Err(read_error(pid, errno()));
    }
    if bytes != expected {
        return Err(ProcessError::ShortRead { pid, bytes });
    }
    // SAFETY: the kernel filled exactly `expected` bytes.
    let info = unsafe { info.assume_init() };
    // SAFETY: `pbi_comm` is a NUL-terminated (or zero-padded) fixed array.
    let comm = unsafe { CStr::from_ptr(info.pbi_comm.as_ptr()) }
        .to_string_lossy()
        .into_owned();
    Ok(ProcessSample {
        pid: info.pbi_pid as i32,
        ppid: info.pbi_ppid as i32,
        uid: info.pbi_uid,
        start_seconds: info.pbi_start_tvsec,
        start_microseconds: info.pbi_start_tvusec as u32,
        controlling_device: (info.e_tdev != NODEV).then_some(info.e_tdev),
        pgid: info.pbi_pgid,
        tpgid: info.e_tpgid,
        status: info.pbi_status,
        comm,
    })
}

/// The executable path the kernel reports for `pid`.
pub fn executable_path(pid: i32) -> Result<PathBuf, ProcessError> {
    let mut buffer = vec![0u8; libc::PROC_PIDPATHINFO_MAXSIZE as usize];
    // SAFETY: the buffer is writable for the size passed.
    let length =
        unsafe { libc::proc_pidpath(pid, buffer.as_mut_ptr().cast(), buffer.len() as u32) };
    if length <= 0 {
        return Err(read_error(pid, errno()));
    }
    buffer.truncate(length as usize);
    Ok(PathBuf::from(String::from_utf8_lossy(&buffer).into_owned()))
}

/// Every live process (PID > 1) and its kernel executable path. Processes
/// whose path cannot be read are omitted.
pub fn executables() -> Result<Vec<(i32, PathBuf)>, ProcessError> {
    // SAFETY: a null buffer asks only for the current count.
    let estimate = unsafe { libc::proc_listallpids(std::ptr::null_mut(), 0) };
    if estimate <= 0 {
        return Err(ProcessError::Sysctl {
            name: "proc_listallpids",
            errno: errno(),
        });
    }
    let mut pids = vec![0i32; estimate as usize + 128];
    // SAFETY: the buffer is writable for the byte size passed.
    let count = unsafe {
        libc::proc_listallpids(
            pids.as_mut_ptr().cast(),
            (pids.len() * size_of::<i32>()) as i32,
        )
    };
    if count <= 0 {
        return Err(ProcessError::Sysctl {
            name: "proc_listallpids",
            errno: errno(),
        });
    }
    pids.truncate((count as usize).min(pids.len()));
    Ok(pids
        .into_iter()
        .filter(|pid| *pid > 1)
        .filter_map(|pid| executable_path(pid).ok().map(|path| (pid, path)))
        .collect())
}

/// Every live PID whose kernel executable path is exactly `path`. This is how
/// an application's process is found for incarnation sampling: LaunchServices'
/// running-application list can also attribute transient helper processes to
/// an application's bundle identifier, so it is not identity evidence.
pub fn pids_with_executable(path: &str) -> Result<Vec<i32>, ProcessError> {
    Ok(executables()?
        .into_iter()
        .filter(|(_, exe)| exe.as_os_str() == std::ffi::OsStr::new(path))
        .map(|(pid, _)| pid)
        .collect())
}

/// `dev:ino` of the file currently at `path`, or `None` if it cannot be stat'ed.
fn file_id(path: &str) -> Option<String> {
    let text = CString::new(path).ok()?;
    let mut status = MaybeUninit::<libc::stat>::zeroed();
    // SAFETY: `text` is NUL-terminated and `status` is writable.
    let rc = unsafe { libc::stat(text.as_ptr(), status.as_mut_ptr()) };
    if rc != 0 {
        return None;
    }
    // SAFETY: `stat` succeeded and filled the structure.
    let status = unsafe { status.assume_init() };
    Some(format!("{}:{}", status.st_dev as u32, status.st_ino))
}

pub fn executable_identity(pid: i32) -> Result<ExecutableIdentity, ProcessError> {
    let path = executable_path(pid)?.display().to_string();
    let file_id = file_id(&path);
    Ok(ExecutableIdentity { path, file_id })
}

/// `dev:ino` of the file currently at `path`, for comparison with a mapped image.
pub fn path_file_id(path: &str) -> Option<String> {
    file_id(path)
}

/// Kernel `struct proc_regioninfo` (`sys/proc_info.h`), absent from `libc`.
#[repr(C)]
#[derive(Clone, Copy)]
struct RegionInfo {
    protection: u32,
    max_protection: u32,
    inheritance: u32,
    flags: u32,
    offset: u64,
    behavior: u32,
    user_wired_count: u32,
    user_tag: u32,
    pages_resident: u32,
    pages_shared_now_private: u32,
    pages_swapped_out: u32,
    pages_dirtied: u32,
    ref_count: u32,
    shadow_depth: u32,
    share_mode: u32,
    private_pages_resident: u32,
    shared_pages_resident: u32,
    obj_id: u32,
    depth: u32,
    address: u64,
    size: u64,
}

/// Kernel `struct proc_regionwithpathinfo`.
#[repr(C)]
struct RegionWithPath {
    info: RegionInfo,
    vnode: libc::vnode_info_path,
}

const PROC_PIDREGIONPATHINFO: i32 = 8;
const MAX_REGIONS: usize = 4096;

/// `dev:ino` of the file the kernel has mapped as `pid`'s executable (the
/// region whose vnode path is the kernel's executable path), or `None` when
/// no mapped region names it.
///
/// [`executable_identity`] stats the path now, so a bundle replaced in place
/// while the process runs reports the new file. A running application keeps
/// the unlinked old image; this reads the vnode actually mapped.
pub fn mapped_image_file_id(pid: i32) -> Result<Option<String>, ProcessError> {
    if pid <= 0 {
        return Err(ProcessError::Vanished { pid });
    }
    let path = executable_path(pid)?.display().to_string();
    let expected = size_of::<RegionWithPath>() as i32;
    let mut address: u64 = 0;
    for _ in 0..MAX_REGIONS {
        let mut region = MaybeUninit::<RegionWithPath>::zeroed();
        // SAFETY: the buffer is a properly aligned, zeroed structure of the
        // size passed; the kernel returns the region at or after `address`.
        let bytes = unsafe {
            libc::proc_pidinfo(
                pid,
                PROC_PIDREGIONPATHINFO,
                address,
                region.as_mut_ptr().cast(),
                expected,
            )
        };
        if bytes <= 0 {
            let errno = errno();
            // EINVAL: no region at or after `address`.
            return if errno == libc::EINVAL {
                Ok(None)
            } else {
                Err(read_error(pid, errno))
            };
        }
        if bytes != expected {
            return Err(ProcessError::ShortRead { pid, bytes });
        }
        // SAFETY: the kernel filled exactly `expected` bytes.
        let region = unsafe { region.assume_init() };
        // SAFETY: `vip_path` is a NUL-terminated (or zero-padded) fixed array.
        let mapped = unsafe { CStr::from_ptr(region.vnode.vip_path.as_ptr().cast()) };
        let stat = region.vnode.vip_vi.vi_stat;
        if stat.vst_ino != 0 && mapped.to_bytes() == path.as_bytes() {
            return Ok(Some(format!("{}:{}", stat.vst_dev, stat.vst_ino)));
        }
        let next = region.info.address.saturating_add(region.info.size);
        if next <= address {
            return Ok(None);
        }
        address = next;
    }
    Ok(None)
}

/// Samples one incarnation: BSD info, executable, BSD info again. A changed
/// birth between the two reads means the PID was reused mid-capture.
pub fn sample_incarnation(pid: i32) -> Result<Incarnation, ProcessError> {
    let first = sample(pid)?;
    let executable = executable_identity(pid)?;
    let second = sample(pid)?;
    if !first.same_incarnation(&second) {
        return Err(ProcessError::ChangedDuringCapture { pid });
    }
    Ok(Incarnation {
        sample: second,
        executable,
    })
}

/// The kernel boot-session UUID (`kern.bootsessionuuid`); ProcessKeys from
/// different boots never compare equal.
pub fn boot_session_id() -> Result<String, ProcessError> {
    const NAME: &CStr = c"kern.bootsessionuuid";
    let mut buffer = [0u8; 64];
    let mut length = buffer.len();
    // SAFETY: `buffer`/`length` describe a writable region; no new value is set.
    let rc = unsafe {
        libc::sysctlbyname(
            NAME.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(ProcessError::Sysctl {
            name: "kern.bootsessionuuid",
            errno: errno(),
        });
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
        libc::sysctlbyname(
            NAME.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(ProcessError::Sysctl {
            name: "kern.osproductversion",
            errno: errno(),
        });
    }
    let text = CStr::from_bytes_until_nul(&buffer[..length.min(buffer.len())])
        .map(|value| value.to_string_lossy().into_owned())
        .unwrap_or_default();
    let mut parts = text.split('.').map(|part| part.parse::<u32>().unwrap_or(0));
    Ok((
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
        parts.next().unwrap_or(0),
    ))
}

/// The macOS build identifier from `kern.osversion` (for example `26B5091g`).
pub fn os_build_version() -> Result<String, ProcessError> {
    const NAME: &CStr = c"kern.osversion";
    let mut buffer = [0u8; 32];
    let mut length = buffer.len();
    // SAFETY: as in `boot_session_id`.
    let rc = unsafe {
        libc::sysctlbyname(
            NAME.as_ptr(),
            buffer.as_mut_ptr().cast(),
            &mut length,
            std::ptr::null_mut(),
            0,
        )
    };
    if rc != 0 {
        return Err(ProcessError::Sysctl {
            name: "kern.osversion",
            errno: errno(),
        });
    }
    Ok(
        CStr::from_bytes_until_nul(&buffer[..length.min(buffer.len())])
            .map(|value| value.to_string_lossy().into_owned())
            .unwrap_or_default(),
    )
}

/// The selected deployment floor (SPEC §18.9): macOS 26.0, enforced at runtime
/// as well as in bundle configuration.
pub fn meets_minimum_macos() -> bool {
    os_product_version()
        .map(|(major, _, _)| major >= 26)
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    #[test]
    fn samples_this_process() {
        let pid = std::process::id() as i32;
        let first = sample(pid).expect("sample self");
        let second = sample(pid).expect("resample self");
        assert_eq!(first.pid, pid);
        assert!(first.same_incarnation(&second));
        assert!(first.start_seconds > 0);
        assert!(executable_path(pid).expect("path").is_absolute());
        let incarnation = sample_incarnation(pid).expect("incarnation");
        assert!(incarnation.executable.file_id.is_some());
        assert!(incarnation.executable.canonical().contains('#'));
    }

    #[test]
    fn missing_process_is_vanished() {
        assert_eq!(
            sample(i32::MAX - 1),
            Err(ProcessError::Vanished { pid: i32::MAX - 1 })
        );
        assert!(matches!(sample(0), Err(ProcessError::Vanished { .. })));
    }

    #[test]
    fn launchd_has_no_controlling_terminal() {
        // PID 1 may be unreadable for an unprivileged caller; if readable it
        // must report NODEV as no controlling device rather than a number.
        if let Ok(launchd) = sample(1) {
            assert_eq!(launchd.controlling_device, None);
            assert!(!launchd.is_terminal_foreground());
        }
    }

    /// Native kernel evidence for SPEC §4.2: `exec` keeps the PID and kernel
    /// birth but changes the executable, so the image must be revalidated.
    #[test]
    fn exec_keeps_pid_and_birth_but_changes_the_executable() {
        let mut child = Command::new("/bin/sh")
            .arg("-c")
            .arg("read line; exec /bin/sleep 5")
            .stdin(Stdio::piped())
            .spawn()
            .expect("spawn");
        let pid = child.id() as i32;
        let before = sample_incarnation(pid).expect("before exec");
        assert!(before.executable.path.ends_with("/sh"), "{before:?}");
        {
            use std::io::Write;
            let mut stdin = child.stdin.take().expect("stdin");
            stdin.write_all(b"go\n").expect("release");
        }
        let deadline = Instant::now() + Duration::from_secs(3);
        let after = loop {
            let now = sample_incarnation(pid).expect("after exec");
            if now.executable.path.ends_with("/sleep") || Instant::now() > deadline {
                break now;
            }
            std::thread::sleep(Duration::from_millis(5));
        };
        let _ = child.kill();
        let _ = child.wait();
        assert!(
            before.sample.same_incarnation(&after.sample),
            "same PID and birth"
        );
        assert_ne!(before.executable, after.executable, "image replaced");
        assert!(!before.same_process_and_image(&after));
    }

    #[test]
    fn finds_this_process_by_exact_executable_path() {
        let pid = std::process::id() as i32;
        let path = executable_path(pid).expect("path").display().to_string();
        let found = pids_with_executable(&path).expect("scan");
        assert!(found.contains(&pid), "{found:?}");
        assert!(
            pids_with_executable("/nonexistent/executable")
                .expect("scan")
                .is_empty()
        );
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

    /// The kernel's path for `pid` once it runs `expected` (temporary
    /// directories resolve through `/private`).
    fn executable_path_eventually(pid: i32, expected: &std::path::Path) -> String {
        let real = std::fs::canonicalize(expected).expect("canonical path");
        let deadline = Instant::now() + Duration::from_secs(2);
        loop {
            if let Ok(path) = executable_path(pid)
                && path == real
            {
                return path.display().to_string();
            }
            assert!(Instant::now() < deadline, "child never ran {real:?}");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    /// An in-place replacement while running leaves the process on its old
    /// image: the path's file identity changes, the mapped image does not.
    #[test]
    fn mapped_image_survives_in_place_replacement_of_its_path() {
        let dir = std::env::temp_dir().join(format!("threadspace-image-{}", std::process::id()));
        std::fs::create_dir(&dir).expect("own temporary directory");
        let path = dir.join("sleeper");
        // Copied platform binaries are killed outside system paths; build an
        // ordinary ad-hoc-signed one instead.
        let build = |out: &std::path::Path| {
            let mut cc = Command::new("/usr/bin/cc")
                .args(["-x", "c", "-", "-o"])
                .arg(out)
                .stdin(Stdio::piped())
                .spawn()
                .expect("cc");
            {
                use std::io::Write;
                cc.stdin
                    .take()
                    .expect("stdin")
                    .write_all(b"#include <unistd.h>\nint main(void){sleep(5);return 0;}\n")
                    .expect("source");
            }
            assert!(cc.wait().expect("cc status").success(), "cc built");
        };
        build(&path);
        let mut child = Command::new(&path).spawn().expect("spawn copy");
        let pid = child.id() as i32;
        let path_text = executable_path_eventually(pid, &path);
        let deadline = Instant::now() + Duration::from_secs(2);
        let before = loop {
            match mapped_image_file_id(pid) {
                Ok(Some(id)) => break id,
                _ if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(5)),
                other => panic!("mapped image not found: {other:?}"),
            }
        };
        assert_eq!(Some(before.clone()), path_file_id(&path_text), "running the file at its path");
        let staged = dir.join("sleeper.new");
        build(&staged);
        std::fs::rename(&staged, &path).expect("replace in place");
        let after = mapped_image_file_id(pid).expect("mapped image read");
        let current = path_file_id(&path_text);
        let _ = child.kill();
        let _ = child.wait();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(after.as_deref(), Some(before.as_str()), "old image still mapped");
        assert_ne!(current.as_deref(), Some(before.as_str()), "path now names a new file");
        assert!(mapped_image_file_id(pid).is_err(), "reaped process has no image");
    }
}

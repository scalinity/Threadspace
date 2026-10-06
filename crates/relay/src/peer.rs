//! Unix socket peer credentials (SPEC §8.1): effective UID/GID from
//! `getpeereid` and the peer PID from `LOCAL_PEERPID`. They describe the live
//! direct peer only; a forwarded or replayed record does not inherit them.

use std::io;
use std::os::fd::AsRawFd;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PeerCredentials {
    pub euid: u32,
    pub egid: u32,
    pub pid: i32,
}

pub fn peer_credentials(socket: &impl AsRawFd) -> io::Result<PeerCredentials> {
    let fd = socket.as_raw_fd();
    let mut euid: libc::uid_t = 0;
    let mut egid: libc::gid_t = 0;
    // SAFETY: `fd` is a live socket and the out-pointers are valid.
    if unsafe { libc::getpeereid(fd, &mut euid, &mut egid) } != 0 {
        return Err(io::Error::last_os_error());
    }
    let mut pid: libc::pid_t = 0;
    let mut length = std::mem::size_of::<libc::pid_t>() as libc::socklen_t;
    // SAFETY: as above; `length` matches the `pid` buffer.
    let rc = unsafe {
        libc::getsockopt(fd, libc::SOL_LOCAL, libc::LOCAL_PEERPID, (&raw mut pid).cast(), &mut length)
    };
    if rc != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(PeerCredentials { euid, egid, pid })
}

/// This process's effective UID.
pub fn current_euid() -> u32 {
    // SAFETY: geteuid has no preconditions.
    unsafe { libc::geteuid() }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn reports_this_process_as_the_peer_of_a_pair() {
        let (left, _right) = UnixStream::pair().expect("pair");
        let credentials = peer_credentials(&left).expect("credentials");
        assert_eq!(credentials.euid, current_euid());
        assert_eq!(credentials.pid, std::process::id() as i32);
    }
}

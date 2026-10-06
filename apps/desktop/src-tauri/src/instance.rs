//! The UI single-instance lock and incumbent forwarding (SPEC §18.8). The
//! first UI process holds `ui-instance.lock` and serves a private activation
//! socket named by an owner-only locator. A second launch verifies the
//! incumbent's process incarnation and socket peer, forwards one validated
//! activation and exits. This never starts or depends on the companion, so it
//! works with observation disabled.

use std::fs::{self, File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::fd::AsRawFd;
use std::os::unix::fs::OpenOptionsExt;
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use serde::{Deserialize, Serialize};
use threadspace_relay::paths::home_dir;
use threadspace_relay::peer::{current_euid, peer_credentials};
use threadspace_relay::runtime::{bind_private_socket, create_runtime_dir};
use threadspace_surfaces_macos::process;

const ACTIVATE: &str = "ACTIVATE";
const ACCEPTED: &str = "OK";

#[derive(Debug, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct Locator {
    pid: u32,
    start_seconds: String,
    start_microseconds: u32,
    socket: String,
}

pub struct Incumbent {
    _lock: File,
    listener: UnixListener,
}

pub enum Claim {
    Incumbent(Incumbent),
    /// Another live UI took the activation; this process should exit 0.
    Forwarded,
    /// The lock is held but the incumbent could not be verified or reached.
    Failed(String),
}

fn instance_dir(app_identifier: &str) -> Option<PathBuf> {
    Some(
        home_dir()?
            .join("Library/Application Support")
            .join(app_identifier),
    )
}

fn own_locator(socket: &std::path::Path) -> Option<Locator> {
    let pid = std::process::id();
    let sample = process::sample(pid as i32).ok()?;
    Some(Locator {
        pid,
        start_seconds: sample.start_seconds.to_string(),
        start_microseconds: sample.start_microseconds,
        socket: socket.display().to_string(),
    })
}

fn forward(dir: &std::path::Path) -> Result<(), String> {
    let text = fs::read_to_string(dir.join("ui-instance.json")).map_err(|e| e.to_string())?;
    let locator: Locator = serde_json::from_str(&text).map_err(|e| e.to_string())?;
    let sample = process::sample(locator.pid as i32).map_err(|e| e.to_string())?;
    if sample.start_seconds.to_string() != locator.start_seconds
        || sample.start_microseconds != locator.start_microseconds
    {
        return Err("incumbent locator names a different process incarnation".into());
    }
    let mut stream = UnixStream::connect(&locator.socket).map_err(|e| e.to_string())?;
    stream
        .set_read_timeout(Some(Duration::from_secs(3)))
        .map_err(|e| e.to_string())?;
    let peer = peer_credentials(&stream).map_err(|e| e.to_string())?;
    if peer.euid != current_euid() || peer.pid != locator.pid as i32 {
        return Err("activation socket peer is not the verified incumbent".into());
    }
    stream
        .write_all(format!("{ACTIVATE}\n").as_bytes())
        .map_err(|e| e.to_string())?;
    let mut reply = String::new();
    BufReader::new(stream)
        .read_line(&mut reply)
        .map_err(|e| e.to_string())?;
    (reply.trim() == ACCEPTED)
        .then_some(())
        .ok_or_else(|| format!("incumbent answered {reply:?}"))
}

pub fn claim(app_identifier: &str) -> Claim {
    let Some(dir) = instance_dir(app_identifier) else {
        return Claim::Failed("no home directory".into());
    };
    if let Err(error) = fs::create_dir_all(&dir) {
        return Claim::Failed(error.to_string());
    }
    let lock = match OpenOptions::new()
        .read(true)
        .write(true)
        .create(true)
        .truncate(false)
        .mode(0o600)
        .open(dir.join("ui-instance.lock"))
    {
        Ok(file) => file,
        Err(error) => return Claim::Failed(error.to_string()),
    };
    // SAFETY: the descriptor belongs to `lock`, which the incumbent keeps open.
    if unsafe { libc::flock(lock.as_raw_fd(), libc::LOCK_EX | libc::LOCK_NB) } != 0 {
        return match forward(&dir) {
            Ok(()) => Claim::Forwarded,
            Err(error) => Claim::Failed(error),
        };
    }
    let bound =
        create_runtime_dir().and_then(|runtime| bind_private_socket(&runtime, "activate.sock"));
    let (listener, socket) = match bound {
        Ok(bound) => bound,
        Err(error) => return Claim::Failed(error.to_string()),
    };
    let Some(locator) = own_locator(&socket) else {
        return Claim::Failed("own process sample unavailable".into());
    };
    let temp = dir.join("ui-instance.json.tmp");
    let written = serde_json::to_vec(&locator)
        .map_err(|e| e.to_string())
        .and_then(|bytes| {
            OpenOptions::new()
                .write(true)
                .create(true)
                .truncate(true)
                .mode(0o600)
                .open(&temp)
                .and_then(|mut file| file.write_all(&bytes))
                .map_err(|e| e.to_string())
        })
        .and_then(|()| fs::rename(&temp, dir.join("ui-instance.json")).map_err(|e| e.to_string()));
    if let Err(error) = written {
        return Claim::Failed(error);
    }
    Claim::Incumbent(Incumbent {
        _lock: lock,
        listener,
    })
}

/// Serves activations for the life of the process.
pub fn serve(incumbent: Incumbent, on_activate: impl Fn() + Send + 'static) {
    let _ = thread::Builder::new()
        .name("ui-activation".into())
        .spawn(move || {
            let Incumbent { _lock, listener } = incumbent;
            for stream in listener.incoming().flatten() {
                let trusted =
                    peer_credentials(&stream).is_ok_and(|peer| peer.euid == current_euid());
                if !trusted {
                    continue;
                }
                let _ = stream.set_read_timeout(Some(Duration::from_secs(2)));
                let Ok(mut writer) = stream.try_clone() else {
                    continue;
                };
                let mut line = String::new();
                let read = BufReader::new(std::io::Read::take(stream, 256)).read_line(&mut line);
                if read.is_ok() && line.trim() == ACTIVATE {
                    on_activate();
                    let _ = writer.write_all(format!("{ACCEPTED}\n").as_bytes());
                }
            }
        });
}

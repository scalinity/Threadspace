//! The owner-only runtime locator in the companion's Application Support
//! directory (SPEC §8.1). It names the current socket, core generation and
//! companion incarnation and is replaced atomically on every start.

use std::fs::{self, OpenOptions};
use std::io::{self, Read, Write};
use std::os::unix::fs::{MetadataExt, OpenOptionsExt};
use std::path::Path;

use serde::{Deserialize, Serialize};
use threadspace_contracts::diagnostics::ProcessIdentity;

use crate::peer::current_euid;

pub const LOCATOR_SCHEMA: u32 = 1;
const LOCATOR_MAX_BYTES: u64 = 16 * 1024;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RuntimeLocator {
    pub schema: u32,
    pub bundle_identifier: String,
    pub runtime_dir: String,
    pub control_socket: String,
    pub core_generation: String,
    pub store_generation: String,
    pub companion: ProcessIdentity,
    pub written_at_ms: i64,
}

#[derive(Debug)]
pub enum LocatorError {
    Missing,
    NotPrivate(String),
    Io(io::Error),
    Malformed(String),
}

impl std::fmt::Display for LocatorError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Missing => f.write_str("no runtime locator; the companion has not started"),
            Self::NotPrivate(detail) => write!(f, "locator is not owner-only: {detail}"),
            Self::Io(error) => write!(f, "locator: {error}"),
            Self::Malformed(detail) => write!(f, "locator malformed: {detail}"),
        }
    }
}

impl std::error::Error for LocatorError {}

/// Writes the locator to a same-directory temporary file, syncs it, and
/// renames it into place.
pub fn write_atomic(path: &Path, locator: &RuntimeLocator) -> io::Result<()> {
    let directory = path.parent().ok_or_else(|| io::Error::other("locator has no parent"))?;
    let temporary = directory.join(format!(".runtime-locator.{}.tmp", std::process::id()));
    let _ = fs::remove_file(&temporary);
    let body = serde_json::to_vec_pretty(locator).map_err(io::Error::other)?;
    {
        let mut file = OpenOptions::new().write(true).create_new(true).mode(0o600).open(&temporary)?;
        file.write_all(&body)?;
        file.sync_all()?;
    }
    fs::rename(&temporary, path)?;
    fs::File::open(directory)?.sync_all()?;
    Ok(())
}

pub fn read(path: &Path) -> Result<RuntimeLocator, LocatorError> {
    let metadata = match fs::symlink_metadata(path) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Err(LocatorError::Missing),
        Err(error) => return Err(LocatorError::Io(error)),
    };
    if !metadata.file_type().is_file() {
        return Err(LocatorError::NotPrivate("not a regular file".into()));
    }
    if metadata.uid() != current_euid() || metadata.mode() & 0o077 != 0 {
        return Err(LocatorError::NotPrivate(format!("uid {} mode {:o}", metadata.uid(), metadata.mode() & 0o777)));
    }
    if metadata.len() > LOCATOR_MAX_BYTES {
        return Err(LocatorError::Malformed("too large".into()));
    }
    let mut body = Vec::new();
    fs::File::open(path).map_err(LocatorError::Io)?.read_to_end(&mut body).map_err(LocatorError::Io)?;
    let locator: RuntimeLocator =
        serde_json::from_slice(&body).map_err(|error| LocatorError::Malformed(error.to_string()))?;
    if locator.schema != LOCATOR_SCHEMA {
        return Err(LocatorError::Malformed(format!("schema {}", locator.schema)));
    }
    Ok(locator)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    fn sample() -> RuntimeLocator {
        RuntimeLocator {
            schema: LOCATOR_SCHEMA,
            bundle_identifier: "ai.scalinity.threadspace.dev.agent".into(),
            runtime_dir: "/tmp/ts.501.0".into(),
            control_socket: "/tmp/ts.501.0/control.sock".into(),
            core_generation: "core".into(),
            store_generation: "store".into(),
            companion: ProcessIdentity {
                pid: 1,
                boot_id: "boot".into(),
                start_seconds: "1".into(),
                start_microseconds: 0,
                executable_path: "/x".into(),
            },
            written_at_ms: 1,
        }
    }

    #[test]
    fn writes_owner_only_and_reads_back() {
        let dir = std::env::temp_dir().join(format!("ts-locator-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).expect("dir");
        let path = dir.join("runtime-locator.json");
        write_atomic(&path, &sample()).expect("write");
        assert_eq!(fs::metadata(&path).expect("stat").mode() & 0o777, 0o600);
        assert_eq!(read(&path).expect("read"), sample());

        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).expect("chmod");
        assert!(matches!(read(&path), Err(LocatorError::NotPrivate(_))));
        assert!(matches!(read(&dir.join("absent.json")), Err(LocatorError::Missing)));
        let _ = fs::remove_dir_all(dir);
    }
}

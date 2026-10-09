//! The Threadspace-owned integration directory and the file operations every
//! setup step uses (SPEC §19.2): same-directory temporary writes that are
//! synced and renamed into place, owner-only backups, the staged helper and
//! the content-addressed observer mod copy.
//!
//!   <owned>/bin/threadspace-hook          staged helper (0755)
//!   <owned>/observer/<sha256[..16]>/       mod copy with the owned captureArgv
//!   <owned>/session/{settings.json,activate.env}   session scope only
//!   <owned>/record.json                    the InstallRecord
//!   <owned>/backups/                       original settings bytes (0600, dir 0700)

use std::fs::{self, DirBuilder, File, OpenOptions};
use std::io::{self, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use super::ordered_json::Value;
use super::{InstallRecord, SetupError};

const MANIFEST: &str = ".claude-plugin/plugin.json";

pub struct Layout {
    pub root: PathBuf,
    pub bin: PathBuf,
    pub helper: PathBuf,
    pub observer: PathBuf,
    pub session: PathBuf,
    pub session_settings: PathBuf,
    pub session_env: PathBuf,
    pub record: PathBuf,
    pub backups: PathBuf,
}

impl Layout {
    pub fn new(root: &Path) -> Self {
        let session = root.join("session");
        Self {
            root: root.to_owned(),
            bin: root.join("bin"),
            helper: root.join("bin/threadspace-hook"),
            observer: root.join("observer"),
            session_settings: session.join("settings.json"),
            session_env: session.join("activate.env"),
            session,
            record: root.join("record.json"),
            backups: root.join("backups"),
        }
    }

    pub fn create(&self) -> Result<(), SetupError> {
        [&self.root, &self.bin, &self.observer, &self.backups]
            .into_iter()
            .try_for_each(|dir| create_private_dir(dir))
    }
}

/// Creates the directory (and missing parents) owner-only.
pub fn create_private_dir(dir: &Path) -> Result<(), SetupError> {
    DirBuilder::new()
        .recursive(true)
        .mode(0o700)
        .create(dir)
        .and_then(|()| fs::set_permissions(dir, fs::Permissions::from_mode(0o700)))
        .map_err(io_error(dir))
}

pub fn io_error(path: &Path) -> impl FnOnce(io::Error) -> SetupError + '_ {
    move |error| SetupError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    }
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// The file's bytes, or `None` when it does not exist.
pub fn read_optional(path: &Path) -> Result<Option<Vec<u8>>, SetupError> {
    match fs::read(path) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(io_error(path)(error)),
    }
}

fn write_synced(path: &Path, bytes: &[u8], mode: u32, create_new: bool) -> io::Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .create_new(create_new)
        .truncate(true)
        .mode(mode)
        .open(path)?;
    file.write_all(bytes)?;
    // Set explicitly: the creation mode is reduced by the umask.
    file.set_permissions(fs::Permissions::from_mode(mode))?;
    file.sync_all()
}

fn sync_dir(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

/// Writes to a same-directory temporary file, syncs it, renames it into
/// place and syncs the directory.
pub fn write_atomic(path: &Path, bytes: &[u8], mode: u32) -> Result<(), SetupError> {
    let fail = io_error(path);
    let (Some(dir), Some(name)) = (path.parent(), path.file_name()) else {
        return Err(fail(io::Error::other("no parent directory")));
    };
    let temporary = dir.join(format!(
        ".{}.{}.tmp",
        name.to_string_lossy(),
        std::process::id()
    ));
    let _ = fs::remove_file(&temporary);
    write_synced(&temporary, bytes, mode, true)
        .and_then(|()| fs::rename(&temporary, path))
        .and_then(|()| sync_dir(dir))
        .map_err(|error| {
            let _ = fs::remove_file(&temporary);
            fail(error)
        })
}

#[cfg(test)]
pub type RereadHook = Box<dyn FnOnce(&Path)>;

#[cfg(test)]
thread_local! {
    /// Runs once between a checked write's earlier read and its re-read, to
    /// race an owner edit against the write.
    pub static BEFORE_REREAD: std::cell::RefCell<Option<RereadHook>> =
        const { std::cell::RefCell::new(None) };
}

/// Replaces (or, with `None`, deletes) a provider file only while its
/// SHA-256 still equals `expected` (`None`: still absent). A symlinked file
/// is written through to its target; an existing file keeps its mode.
pub fn replace_checked(
    path: &Path,
    expected: Option<&str>,
    bytes: Option<&[u8]>,
) -> Result<(), SetupError> {
    let real = fs::canonicalize(path).unwrap_or_else(|_| path.to_owned());
    #[cfg(test)]
    if let Some(hook) = BEFORE_REREAD.with(|slot| slot.borrow_mut().take()) {
        hook(&real);
    }
    let current = read_optional(&real)?;
    if current.as_deref().map(sha256_hex).as_deref() != expected {
        return Err(SetupError::ChangedDuringInstall {
            path: path.to_owned(),
        });
    }
    match bytes {
        Some(bytes) => {
            let mode = fs::metadata(&real)
                .map(|metadata| metadata.permissions().mode() & 0o7777)
                .unwrap_or(0o600);
            write_atomic(&real, bytes, mode)
        }
        // Deletion removes the configured entry itself, never a link target.
        None => fs::remove_file(path)
            .and_then(|()| path.parent().map_or(Ok(()), sync_dir))
            .map_err(io_error(path)),
    }
}

/// Backups are content-addressed, so repeated installs over the same
/// original keep one copy.
pub fn backup_name(sha256: &str) -> String {
    format!("settings.{}.json", &sha256[..16])
}

pub fn read_record(path: &Path) -> Result<Option<InstallRecord>, SetupError> {
    read_optional(path)?
        .map(|bytes| {
            serde_json::from_slice(&bytes).map_err(|error| SetupError::InvalidRecord {
                path: path.to_owned(),
                reason: error.to_string(),
            })
        })
        .transpose()
}

pub fn write_json(path: &Path, value: &impl serde::Serialize) -> Result<(), SetupError> {
    let mut bytes = serde_json::to_vec_pretty(value).map_err(|error| SetupError::Io {
        path: path.to_owned(),
        message: error.to_string(),
    })?;
    bytes.push(b'\n');
    write_atomic(path, &bytes, 0o600)
}

/// Removes a file or directory tree; reports whether anything was there.
pub fn remove_owned(path: &Path) -> Result<bool, SetupError> {
    let result = match fs::symlink_metadata(path) {
        Ok(metadata) if metadata.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Ok(false),
        Err(error) => Err(error),
    };
    result.map(|()| true).map_err(io_error(path))
}

/// Removes every entry of `dir` except `keep`.
pub fn remove_others(dir: &Path, keep: &Path) -> Result<(), SetupError> {
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        let path = entry.map_err(io_error(dir))?.path();
        if path != keep {
            remove_owned(&path)?;
        }
    }
    Ok(())
}

/// The observer mod as it will be staged: the source tree without its tests
/// and type-checking files, with the manifest's `captureArgv` default set to
/// the owned helper.
pub struct ModCopy {
    /// SHA-256 over the staged files' paths and bytes.
    pub sha256: String,
    files: Vec<(String, Vec<u8>)>,
}

impl ModCopy {
    pub fn dir_name(&self) -> &str {
        &self.sha256[..16]
    }

    pub fn read(source: &Path, capture_argv: &[String]) -> Result<Self, SetupError> {
        if !source.is_dir() {
            return Err(SetupError::MissingSource {
                path: source.to_owned(),
            });
        }
        let invalid = |reason: String| SetupError::InvalidMod {
            path: source.to_owned(),
            reason,
        };
        let mut files = Vec::new();
        read_tree(source, "", true, &mut files)?;
        files.sort();
        let (_, manifest) = files
            .iter_mut()
            .find(|(path, _)| path == MANIFEST)
            .ok_or_else(|| invalid(format!("no {MANIFEST}")))?;
        let mut document = Value::parse(manifest).map_err(|error| invalid(error.to_string()))?;
        let capture = document
            .as_object_mut()
            .and_then(|root| root.get_mut("userConfig"))
            .and_then(Value::as_object_mut)
            .and_then(|config| config.get_mut("captureArgv"))
            .and_then(Value::as_object_mut)
            .ok_or_else(|| invalid("no userConfig.captureArgv in the manifest".into()))?;
        capture.insert(
            "default",
            Value::Array(capture_argv.iter().map(Value::string).collect()),
        );
        *manifest = document
            .to_pretty()
            .map_err(|error| invalid(error.to_string()))?;
        Ok(Self {
            sha256: tree_sha256(&files),
            files,
        })
    }

    /// Stages the copy at `target` by building it beside it and renaming it
    /// into place. An existing copy with the same contents is kept.
    pub fn stage(&self, target: &Path) -> Result<(), SetupError> {
        let fail = io_error(target);
        match staged_sha256(target)? {
            Some(existing) if existing == self.sha256 => return Ok(()),
            Some(_) => fs::remove_dir_all(target).map_err(io_error(target))?,
            None => {}
        }
        let Some(parent) = target.parent() else {
            return Err(fail(io::Error::other("no parent directory")));
        };
        let staging = parent.join(format!(".{}.{}.tmp", self.dir_name(), std::process::id()));
        remove_owned(&staging)?;
        let built = self.files.iter().try_for_each(|(relative, bytes)| {
            let path = staging.join(relative);
            path.parent()
                .map_or(Ok(()), fs::create_dir_all)
                .and_then(|()| write_synced(&path, bytes, 0o644, true))
        });
        built
            .and_then(|()| fs::rename(&staging, target))
            .and_then(|()| sync_dir(parent))
            .map_err(|error| {
                let _ = fs::remove_dir_all(&staging);
                fail(error)
            })
    }
}

/// The SHA-256 of an existing copy, as `ModCopy::sha256` computes it.
pub fn staged_sha256(dir: &Path) -> Result<Option<String>, SetupError> {
    if !dir.is_dir() {
        return Ok(None);
    }
    let mut files = Vec::new();
    read_tree(dir, "", false, &mut files)?;
    files.sort();
    Ok(Some(tree_sha256(&files)))
}

fn tree_sha256(files: &[(String, Vec<u8>)]) -> String {
    let mut hasher = Sha256::new();
    for (path, bytes) in files {
        hasher.update(path.as_bytes());
        hasher.update([0]);
        hasher.update((bytes.len() as u64).to_be_bytes());
        hasher.update(bytes);
    }
    hasher
        .finalize()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

/// Not part of the runtime mod: its tests and the type-checking files the
/// plugin tooling generates.
fn excluded(relative: &str) -> bool {
    relative == "tests"
        || relative == ".claude-plugin/types"
        || relative == "tsconfig.json"
        || relative.ends_with("/tsconfig.json")
}

fn read_tree(
    dir: &Path,
    prefix: &str,
    exclude: bool,
    files: &mut Vec<(String, Vec<u8>)>,
) -> Result<(), SetupError> {
    let invalid = |path: &Path, reason: &str| SetupError::InvalidMod {
        path: path.to_owned(),
        reason: reason.to_owned(),
    };
    for entry in fs::read_dir(dir).map_err(io_error(dir))? {
        let entry = entry.map_err(io_error(dir))?;
        let path = entry.path();
        let Some(name) = entry.file_name().to_str().map(str::to_owned) else {
            return Err(invalid(&path, "file name is not UTF-8"));
        };
        let relative = if prefix.is_empty() {
            name
        } else {
            format!("{prefix}/{name}")
        };
        if exclude && excluded(&relative) {
            continue;
        }
        let kind = entry.file_type().map_err(io_error(&path))?;
        if kind.is_dir() {
            read_tree(&path, &relative, exclude, files)?;
        } else if kind.is_file() {
            files.push((relative, fs::read(&path).map_err(io_error(&path))?));
        } else {
            return Err(invalid(
                &path,
                "only regular files and directories are copied",
            ));
        }
    }
    Ok(())
}

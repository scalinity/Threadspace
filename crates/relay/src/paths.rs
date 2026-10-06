//! Per-identity locations. Development and production use distinct bundle
//! identifiers, so their stores, locators, sockets and logs never overlap.

use std::ffi::CStr;
use std::path::PathBuf;

/// The companion's bundle identifier for an outer application identifier.
pub fn agent_identifier_for(app_identifier: &str) -> String {
    format!("{app_identifier}.agent")
}

/// Reverse-DNS characters only; rejects anything that could escape a directory.
pub fn valid_identifier(identifier: &str) -> bool {
    !identifier.is_empty()
        && identifier.len() <= 128
        && !identifier.starts_with('.')
        && !identifier.contains("..")
        && identifier
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'.' || byte == b'-')
}

/// The effective user's home directory from the password database, so it is
/// independent of the (possibly cleared) environment.
pub fn home_dir() -> Option<PathBuf> {
    let mut buffer = vec![0 as libc::c_char; 4096];
    let mut entry = std::mem::MaybeUninit::<libc::passwd>::uninit();
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    // SAFETY: all pointers reference live, correctly sized buffers.
    let rc = unsafe {
        libc::getpwuid_r(
            libc::geteuid(),
            entry.as_mut_ptr(),
            buffer.as_mut_ptr(),
            buffer.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() {
        return None;
    }
    // SAFETY: getpwuid_r succeeded, so `pw_dir` points into `buffer`.
    let dir = unsafe { CStr::from_ptr((*result).pw_dir) };
    Some(PathBuf::from(dir.to_string_lossy().into_owned()))
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentPaths {
    pub agent_identifier: String,
    pub store_dir: PathBuf,
    pub journal: PathBuf,
    pub locator: PathBuf,
    pub log_dir: PathBuf,
}

impl AgentPaths {
    pub fn for_agent(agent_identifier: &str) -> Option<Self> {
        if !valid_identifier(agent_identifier) {
            return None;
        }
        let home = home_dir()?;
        let store_dir = home
            .join("Library/Application Support")
            .join(agent_identifier);
        Some(Self {
            agent_identifier: agent_identifier.to_owned(),
            journal: store_dir.join("journal.sqlite3"),
            locator: store_dir.join("runtime-locator.json"),
            store_dir,
            log_dir: home.join("Library/Logs").join(agent_identifier),
        })
    }
}

/// Replaces the home directory prefix with `~` for diagnostics and evidence.
pub fn redact_home(path: &str) -> String {
    match home_dir() {
        Some(home) => {
            let home = home.to_string_lossy();
            path.strip_prefix(home.as_ref())
                .map_or_else(|| path.to_owned(), |rest| format!("~{rest}"))
        }
        None => path.to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn identifiers_are_validated() {
        assert!(valid_identifier("ai.scalinity.threadspace.dev.agent"));
        for bad in ["", "../x", "a/b", ".hidden", "a..b", "a b"] {
            assert!(!valid_identifier(bad), "{bad:?}");
        }
        assert_eq!(
            agent_identifier_for("ai.scalinity.threadspace"),
            "ai.scalinity.threadspace.agent"
        );
    }

    #[test]
    fn paths_live_under_the_users_library() {
        let paths = AgentPaths::for_agent("ai.scalinity.threadspace.dev.agent").expect("paths");
        assert!(
            paths
                .store_dir
                .ends_with("Library/Application Support/ai.scalinity.threadspace.dev.agent")
        );
        assert!(
            paths
                .log_dir
                .ends_with("Library/Logs/ai.scalinity.threadspace.dev.agent")
        );
        assert!(redact_home(&paths.journal.to_string_lossy()).starts_with("~/Library/"));
        assert!(AgentPaths::for_agent("../evil").is_none());
    }
}

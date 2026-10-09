//! Claude Code builds whose observer-mod semantics are qualified (D-0005 for
//! 2.1.291, D-0009 for 2.1.295). The version is the one the kernel reports
//! for the calling process's executable, never the mod's own claim: the
//! mod's `$.session.version()` is middleware-interceptable. Every other build
//! is LIMITED: its observations are kept at a lower evidence tier.

use std::path::{Component, Path};

/// A qualified observer build.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ObserverProfile {
    pub version: &'static str,
    /// The repository capability profile that records its evidence.
    pub profile_ref: &'static str,
}

const QUALIFIED: &[ObserverProfile] = &[
    ObserverProfile {
        version: "2.1.291",
        profile_ref: "docs/compatibility/claude-observer-2.1.291.json",
    },
    ObserverProfile {
        version: "2.1.295",
        profile_ref: "docs/compatibility/claude-observer-2.1.295.json",
    },
];

pub const QUALIFIED_OBSERVER_VERSIONS: &[&str] = &["2.1.291", "2.1.295"];

/// `Some` only for a qualified build.
pub fn observer_profile(version: &str) -> Option<ObserverProfile> {
    QUALIFIED
        .iter()
        .copied()
        .find(|profile| profile.version == version)
}

/// The version of a direct CLI executable from its path in the installer's
/// versions directory (`…/claude/versions/2.1.295`). Any other layout, such
/// as the desktop app's bundled `claude`, names no version.
pub fn version_from_executable(path: &str) -> Option<String> {
    let path = Path::new(path);
    if path
        .components()
        .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return None;
    }
    let version = path.file_name()?.to_str()?;
    let parent = path.parent()?;
    let is_versions_dir = parent.file_name()? == "versions"
        && parent
            .parent()
            .and_then(Path::file_name)
            .is_some_and(|name| name == "claude");
    let well_formed = !version.is_empty()
        && version
            .split('.')
            .all(|part| !part.is_empty() && part.bytes().all(|b| b.is_ascii_digit()));
    (is_versions_dir && well_formed).then(|| version.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_qualified_builds_have_a_profile() {
        assert_eq!(
            observer_profile("2.1.295").map(|p| p.version),
            Some("2.1.295")
        );
        assert_eq!(
            observer_profile("2.1.291").map(|p| p.version),
            Some("2.1.291")
        );
        for version in ["2.1.292", "2.1.294", "2.1.296", "", "2.1.295 "] {
            assert_eq!(observer_profile(version), None, "{version:?}");
        }
        for version in QUALIFIED_OBSERVER_VERSIONS {
            assert!(observer_profile(version).is_some());
        }
    }

    #[test]
    fn the_version_comes_from_the_versions_directory_layout() {
        assert_eq!(
            version_from_executable("/Users/u/.local/share/claude/versions/2.1.295").as_deref(),
            Some("2.1.295")
        );
        for path in [
            "/Users/u/.local/share/claude/ClaudeCode.app/Contents/MacOS/claude",
            "/Users/u/.local/share/claude/versions/../versions/2.1.295",
            "/Users/u/.local/share/claude/versions/2.1.295-beta",
            "/Users/u/.local/share/other/versions/2.1.295",
            "/Users/u/.local/bin/claude",
            "versions/2.1.295",
            "",
        ] {
            assert_eq!(version_from_executable(path), None, "{path}");
        }
    }
}

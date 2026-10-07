//! Provider capability profiles the reducer consults (SPEC §10). A profile is
//! pinned by reducer version, so a replay applies the same capabilities.

/// The capabilities that change canonical reduction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Capabilities {
    /// Native acceptance and original origin are observable (SPEC §7.3 (1)–(2)).
    pub accepted_input_provenance: bool,
    /// A positive original-order witness is qualified (SPEC §7.3 (3)). Without
    /// it, explicit "Mark handled" is the only resolution of output attention.
    pub automatic_human_followup_resolution: bool,
}

/// Claude Code 2.1.291 (D-0005): accepted-input provenance is present; no
/// positive original-submission order witness exists, so automatic
/// human-follow-up resolution is NOT_SUPPORTED.
pub const CLAUDE_NATIVE_OBSERVER: Capabilities = Capabilities {
    accepted_input_provenance: true,
    automatic_human_followup_resolution: false,
};

/// The synthetic qualification profile with a positive original-order
/// witness, used to exercise the auto-resolution rules themselves.
pub const SYNTHETIC_WITNESSED: Capabilities = Capabilities {
    accepted_input_provenance: true,
    automatic_human_followup_resolution: true,
};

/// Every other profile: nothing is assumed.
pub const LIMITED: Capabilities = Capabilities {
    accepted_input_provenance: false,
    automatic_human_followup_resolution: false,
};

/// Synthetic profiles whose name starts with this prefix carry the witness.
pub const SYNTHETIC_WITNESSED_PREFIX: &str = "witnessed";

pub fn capabilities(provider: &str, profile_ref: &str) -> Capabilities {
    match provider {
        "claude" => CLAUDE_NATIVE_OBSERVER,
        "synthetic" if profile_ref.starts_with(SYNTHETIC_WITNESSED_PREFIX) => SYNTHETIC_WITNESSED,
        "synthetic" => CLAUDE_NATIVE_OBSERVER,
        _ => LIMITED,
    }
}

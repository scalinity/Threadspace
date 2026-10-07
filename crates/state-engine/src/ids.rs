//! Canonical identifiers.
//!
//! Admission allocates the random identity of a newly seen native object
//! (SPEC §4.1) through an `Allocator`. The reducer never allocates: a record
//! it derives (an attention item, a wait episode, an outbox intent) gets a
//! name-based UUID hashed from the canonical inputs that define it, so exact
//! replay reproduces it without randomness.

use sha2::{Digest, Sha256};
use uuid::{Builder, Uuid};

const DERIVED_NAMESPACE: &[u8] = b"threadspace.derived.v1\0";

/// A name-based (RFC 9562 version 8) UUID from SHA-256 of `name`.
pub fn derived_id(name: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(DERIVED_NAMESPACE);
    hasher.update(name.as_bytes());
    let digest = hasher.finalize();
    let mut bytes = [0u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    Builder::from_custom_bytes(bytes)
        .into_uuid()
        .hyphenated()
        .to_string()
}

/// Supplies identities for newly seen native objects during admission.
pub trait Allocator {
    fn allocate(&mut self) -> String;
}

/// Production allocation: random version-4 UUIDs.
#[derive(Debug, Default)]
pub struct RandomAllocator;

impl Allocator for RandomAllocator {
    fn allocate(&mut self) -> String {
        Uuid::new_v4().hyphenated().to_string()
    }
}

/// Deterministic allocation for reproducible synthetic runs: version-4-shaped
/// UUIDs from a SplitMix64 stream, so the same seed and admission order
/// yield byte-identical journals.
#[derive(Debug, Clone)]
pub struct SeededAllocator {
    state: u64,
}

impl SeededAllocator {
    pub fn new(seed: u64) -> Self {
        Self { state: seed }
    }

    fn next_u64(&mut self) -> u64 {
        self.state = self.state.wrapping_add(0x9E37_79B9_7F4A_7C15);
        let mut z = self.state;
        z = (z ^ (z >> 30)).wrapping_mul(0xBF58_476D_1CE4_E5B9);
        z = (z ^ (z >> 27)).wrapping_mul(0x94D0_49BB_1331_11EB);
        z ^ (z >> 31)
    }
}

impl Allocator for SeededAllocator {
    fn allocate(&mut self) -> String {
        let mut bytes = [0u8; 16];
        bytes[..8].copy_from_slice(&self.next_u64().to_be_bytes());
        bytes[8..].copy_from_slice(&self.next_u64().to_be_bytes());
        Builder::from_random_bytes(bytes)
            .into_uuid()
            .hyphenated()
            .to_string()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn derived_ids_are_stable_version_8() {
        let a = derived_id("attention|s|t");
        assert_eq!(a, derived_id("attention|s|t"));
        assert_ne!(a, derived_id("attention|s|u"));
        assert_eq!(Uuid::parse_str(&a).expect("uuid").get_version_num(), 8);
    }

    #[test]
    fn seeded_allocation_is_reproducible_version_4() {
        let mut one = SeededAllocator::new(7);
        let mut two = SeededAllocator::new(7);
        let first = one.allocate();
        assert_eq!(first, two.allocate());
        assert_ne!(first, one.allocate());
        assert_eq!(Uuid::parse_str(&first).expect("uuid").get_version_num(), 4);
    }
}

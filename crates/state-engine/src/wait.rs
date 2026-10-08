//! The episode partition of an aggregate wait scope (D-0007 §4), shared by
//! the reducer, semantic equality and the synthetic invariant monitor.

use std::collections::{BTreeMap, BTreeSet};

use threadspace_contracts::canonical::causal::{CausalOrder, CausalPoint};
use threadspace_contracts::canonical::records::{WaitOwnerDecision, WaitScopeRecord};

use crate::causal::compare;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Episode {
    pub positives: BTreeSet<CausalPoint>,
    /// Holds positives without a causal point (always episode 0).
    pub unordered: bool,
    pub open: bool,
    /// A positive incomparable with a clear: never auto-cleared.
    pub uncertain: bool,
}

/// The episode a positive belongs to: the number of comparable clears
/// before it.
pub fn index_of(scope: &WaitScopeRecord, positive: &CausalPoint) -> u32 {
    let before = scope
        .clears
        .iter()
        .filter(|clear| compare(clear, positive) == CausalOrder::Before)
        .count();
    u32::try_from(before).unwrap_or(u32::MAX)
}

/// Episodes by index. A positive is cleared by a comparable clear at or
/// after it; one incomparable with any clear leaves its episode uncertain.
pub fn partition(scope: &WaitScopeRecord) -> BTreeMap<u32, Episode> {
    let mut episodes: BTreeMap<u32, Episode> = BTreeMap::new();
    for positive in &scope.positives {
        let cleared = scope.clears.iter().any(|clear| {
            matches!(compare(positive, clear), CausalOrder::Before | CausalOrder::Equal)
        });
        let incomparable = scope.unordered_clears > 0
            || scope
                .clears
                .iter()
                .any(|clear| compare(positive, clear) == CausalOrder::Incomparable);
        let episode = episodes.entry(index_of(scope, positive)).or_default();
        episode.positives.insert(positive.clone());
        if !cleared {
            if incomparable {
                episode.uncertain = true;
            } else {
                episode.open = true;
            }
        }
    }
    if scope.unordered_positives > 0 {
        let any_clear = !scope.clears.is_empty() || scope.unordered_clears > 0;
        let episode = episodes.entry(0).or_default();
        episode.unordered = true;
        if any_clear {
            episode.uncertain = true;
        } else {
            episode.open = true;
        }
    }
    episodes
}

/// An owner decision governs an episode holding evidence it was made on.
pub fn covers(decision: &WaitOwnerDecision, episode: &Episode) -> bool {
    (decision.unordered && episode.unordered) || !decision.positives.is_disjoint(&episode.positives)
}

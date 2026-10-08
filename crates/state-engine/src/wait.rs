//! The episode partition of an aggregate wait scope and the owner state its
//! decisions give each episode (D-0007 §4), shared by the reducer, semantic
//! equality and the synthetic invariant monitor.

use std::collections::{BTreeMap, BTreeSet};

use threadspace_contracts::canonical::causal::{CausalOrder, CausalPoint};
use threadspace_contracts::canonical::command::OwnerAction;
use threadspace_contracts::canonical::records::{WaitOwnerDecision, WaitScopeRecord};

use crate::causal::compare;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Episode {
    /// Every positive witness, natively ended or not.
    pub positives: BTreeSet<CausalPoint>,
    /// The positives no comparable clear ended (open or uncertain).
    pub active: BTreeSet<CausalPoint>,
    /// Positives without a causal point (always episode 0, never ended).
    pub unordered: u32,
    pub open: bool,
    /// A positive incomparable with a clear: never auto-cleared.
    pub uncertain: bool,
}

impl Episode {
    /// Holds evidence no clear ended.
    pub fn is_active(&self) -> bool {
        !self.active.is_empty() || self.unordered > 0
    }
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
            episode.active.insert(positive.clone());
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
        episode.unordered = scope.unordered_positives;
        if any_clear {
            episode.uncertain = true;
        } else {
            episode.open = true;
        }
    }
    episodes
}

/// One action's decisions as they apply to an episode's item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Applied<'a> {
    /// Ordered by command ID.
    pub decisions: Vec<&'a WaitOwnerDecision>,
    /// When the action came to cover the whole item: the latest of each
    /// witness's earliest covering decision.
    pub at_ms: i64,
    /// Snooze only: the earliest of each witness's latest snooze end.
    pub until_ms: Option<i64>,
}

/// An episode item's owner state, one entry per action.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct OwnerState<'a> {
    pub acknowledge: Option<Applied<'a>>,
    pub resolve: Option<Applied<'a>>,
    pub snooze: Option<Applied<'a>>,
}

impl OwnerState<'_> {
    /// Whether `decision` is one of those applying to the item.
    pub fn applies(&self, decision: &WaitOwnerDecision) -> bool {
        [&self.acknowledge, &self.resolve, &self.snooze]
            .into_iter()
            .flatten()
            .any(|applied| applied.decisions.iter().any(|d| d.command_id == decision.command_id))
    }
}

fn until(decision: &WaitOwnerDecision) -> Option<i64> {
    match decision.action {
        OwnerAction::Snooze { until_ms } => Some(until_ms),
        _ => None,
    }
}

/// How one action's decisions apply to an episode. A decision applies only
/// to the evidence it covered: while the episode holds active evidence, the
/// action applies only if its decisions together cover every active witness
/// (evidence none of them covered keeps the item actionable), and then each
/// decision covering an active witness applies. Once no evidence is active,
/// every decision made on the episode's evidence is kept on it as history.
fn applied<'a>(episode: &Episode, decisions: &[&'a WaitOwnerDecision]) -> Option<Applied<'a>> {
    if !episode.is_active() {
        let history: Vec<&WaitOwnerDecision> = decisions
            .iter()
            .copied()
            .filter(|d| !d.positives.is_disjoint(&episode.positives))
            .collect();
        let at_ms = history.iter().map(|d| d.at_ms).min()?;
        let until_ms = history.iter().filter_map(|d| until(d)).max();
        return Some(Applied { decisions: history, at_ms, until_ms });
    }
    let mut witnesses: Vec<Vec<&WaitOwnerDecision>> = episode
        .active
        .iter()
        .map(|point| decisions.iter().copied().filter(|d| d.positives.contains(point)).collect())
        .collect();
    if episode.unordered > 0 {
        witnesses.push(decisions.iter().copied().filter(|d| d.unordered >= episode.unordered).collect());
    }
    let mut at_ms = i64::MIN;
    let mut until_ms: Option<i64> = None;
    for covering in &witnesses {
        at_ms = at_ms.max(covering.iter().map(|d| d.at_ms).min()?);
        if let Some(end) = covering.iter().filter_map(|d| until(d)).max() {
            until_ms = Some(until_ms.map_or(end, |u| u.min(end)));
        }
    }
    let mut applying: Vec<&WaitOwnerDecision> = witnesses.into_iter().flatten().collect();
    applying.sort_by(|a, b| a.command_id.cmp(&b.command_id));
    applying.dedup_by(|a, b| a.command_id == b.command_id);
    Some(Applied { decisions: applying, at_ms, until_ms })
}

/// The owner state of an episode's item, from its scope's decisions.
pub fn owner_state<'a>(decisions: &'a [WaitOwnerDecision], episode: &Episode) -> OwnerState<'a> {
    let of = |kind: fn(&OwnerAction) -> bool| {
        let chosen: Vec<&WaitOwnerDecision> = decisions.iter().filter(|d| kind(&d.action)).collect();
        applied(episode, &chosen)
    };
    OwnerState {
        acknowledge: of(|a| matches!(a, OwnerAction::Acknowledge)),
        resolve: of(|a| matches!(a, OwnerAction::Resolve { .. })),
        snooze: of(|a| matches!(a, OwnerAction::Snooze { .. })),
    }
}

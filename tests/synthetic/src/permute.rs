//! Valid delivery permutations: seeded random linear extensions of a
//! scenario's delivery constraints. Only causally required relations are
//! constraints (an owner command after the item it targets, the owner's own
//! command order); everything a spool replay, a racing hook or a delayed
//! callback can reorder is free.

use crate::builder::{Scenario, Step};
use crate::rng::Rng;

/// A uniformly chosen available step at each position; deterministic in seed.
pub fn linear_extension(len: usize, constraints: &[(usize, usize)], seed: u64) -> Vec<usize> {
    let mut rng = Rng::new(seed);
    let mut blockers = vec![0usize; len];
    let mut successors: Vec<Vec<usize>> = vec![Vec::new(); len];
    for &(a, b) in constraints {
        if a < len && b < len && a != b {
            blockers[b] += 1;
            successors[a].push(b);
        }
    }
    let mut available: Vec<usize> = (0..len).filter(|&i| blockers[i] == 0).collect();
    let mut order = Vec::with_capacity(len);
    while !available.is_empty() {
        let pick = available.swap_remove(rng.below(available.len()));
        order.push(pick);
        for &next in &successors[pick] {
            blockers[next] -= 1;
            if blockers[next] == 0 {
                available.push(next);
            }
        }
        // Keep the candidate set in a canonical order so the choice depends
        // only on the seed, not on insertion history.
        available.sort_unstable();
    }
    order
}

/// A delivery order for `seed`: a linear extension of the scenario's
/// constraints, with 1–3 duplicate redeliveries inserted when the seed says
/// so (a retry of an observation can arrive at any time). Returns the order
/// and the number of duplicates injected.
pub fn order_for(scenario: &Scenario, seed: u64) -> (Vec<usize>, usize) {
    let mut order = linear_extension(scenario.steps.len(), &scenario.constraints, seed);
    let mut rng = Rng::new(seed ^ 0xD0D0_D0D0);
    let observations: Vec<usize> = (0..scenario.steps.len())
        .filter(|&i| matches!(scenario.steps[i], Step::Observe(_)))
        .collect();
    let mut injected = 0;
    if rng.chance(1, 2) && !observations.is_empty() {
        for _ in 0..=rng.below(3) {
            let step = observations[rng.below(observations.len())];
            let at = rng.below(order.len() + 1);
            order.insert(at, step);
            injected += 1;
        }
    }
    (order, injected)
}

/// True when `order` respects every constraint.
pub fn respects(order: &[usize], constraints: &[(usize, usize)]) -> bool {
    let mut position = vec![usize::MAX; order.len()];
    for (at, &step) in order.iter().enumerate() {
        if step < position.len() {
            position[step] = at;
        }
    }
    constraints
        .iter()
        .all(|&(a, b)| a >= order.len() || b >= order.len() || position[a] < position[b])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extensions_respect_constraints_and_vary_with_seed() {
        let constraints = [(0, 3), (1, 3), (3, 5)];
        let mut distinct = std::collections::BTreeSet::new();
        for seed in 0..200 {
            let order = linear_extension(6, &constraints, seed);
            assert_eq!(order.len(), 6);
            assert!(respects(&order, &constraints), "{order:?}");
            distinct.insert(order);
        }
        assert!(distinct.len() > 20, "only {} distinct orders", distinct.len());
        assert_eq!(linear_extension(6, &constraints, 9), linear_extension(6, &constraints, 9));
    }
}

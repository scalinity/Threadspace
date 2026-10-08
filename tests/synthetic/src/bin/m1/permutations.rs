//! Seeded valid partial-order permutations (MILESTONES M1 "Tests"): each
//! permutation is a linear extension of its scenario's delivery constraints,
//! half of them with duplicate redeliveries injected, admitted through the
//! pure path and compared with the scenario's reference semantics, its own
//! expectation and the per-step invariant monitor. Every 50th is also
//! admitted through the real SQLite journal.

use std::collections::BTreeMap;
use std::path::Path;
use std::time::Instant;

use serde_json::{Value, json};
use threadspace_synthetic::builder::{Scenario, Step};
use threadspace_synthetic::permute::{linear_extension, respects};
use threadspace_synthetic::rng::Rng;
use threadspace_synthetic::runner::{PureRunner, run};
use threadspace_synthetic::scenarios::catalog;
use threadspace_synthetic::sqlite::{SqliteRunner, TempStore};

use crate::evidence::Area;

/// The families MILESTONES M1 names for the permutation property tests.
pub const REQUIRED_FAMILIES: &[&str] = &[
    "output-input",
    "actor-events",
    "wait-events",
    "completion",
    "delayed-callbacks",
    "owner-commands",
    "snapshot-live",
    "activation-changes",
];
const EXTRA_FAMILIES: &[&str] = &["duplicates", "robustness"];
const EXTRA_PER_FAMILY: usize = 200;
const SQLITE_EVERY: usize = 50;

/// seed = family * 1_000_000 + scenario * 100_000 + index (documented scheme).
pub fn seed(family: usize, scenario: usize, index: usize) -> u64 {
    (family * 1_000_000 + scenario * 100_000 + index) as u64
}

/// A delivery order with 1–3 duplicate redeliveries inserted when the seed
/// says so (a retry of an observation can arrive at any time).
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

pub fn run_all(root: &Path, total: usize) -> Result<Value, String> {
    let area = Area::new(root, "permutations")?;
    let started = Instant::now();
    let scenarios = catalog();
    let mut by_family: BTreeMap<&str, Vec<(usize, &Scenario)>> = BTreeMap::new();
    for (index, scenario) in scenarios.iter().enumerate() {
        by_family.entry(scenario.family).or_default().push((index, scenario));
    }
    let per_required = total.div_ceil(REQUIRED_FAMILIES.len());
    let mut families = serde_json::Map::new();
    let mut failing = Vec::new();
    let mut grand_total = 0usize;
    let mut required_total = 0usize;
    let mut sqlite_checks = 0usize;
    let mut sequence = 0usize;
    for (family_index, family) in REQUIRED_FAMILIES.iter().chain(EXTRA_FAMILIES).enumerate() {
        let members = by_family.get(family).ok_or(format!("no scenarios in family {family}"))?;
        let budget = if REQUIRED_FAMILIES.contains(family) {
            per_required
        } else {
            EXTRA_PER_FAMILY
        };
        let mut family_count = 0usize;
        let mut duplicates = 0usize;
        let mut family_failures = 0usize;
        let mut seeds = Vec::new();
        for (member, (scenario_index, scenario)) in members.iter().enumerate() {
            let reference: Vec<usize> = (0..scenario.steps.len()).collect();
            let expected = run(scenario, &reference, &mut PureRunner::new(1)).semantic_hash;
            let share = budget / members.len() + usize::from(member < budget % members.len());
            let first = seed(family_index, *scenario_index, 0);
            seeds.push(json!({ "scenario": scenario.name, "from": first, "to": first + share as u64 - 1 }));
            for index in 0..share {
                let seed = seed(family_index, *scenario_index, index);
                let (order, injected) = order_for(scenario, seed);
                duplicates += injected;
                let mut problems = Vec::new();
                if !respects(&dedup_first(&order), &scenario.constraints) {
                    problems.push("order violates constraints".to_owned());
                }
                let mut runner = PureRunner::new(seed);
                let report = run(scenario, &order, &mut runner);
                if report.semantic_hash != expected {
                    problems.push("semantic hash differs from reference".to_owned());
                }
                if let Err(error) = (scenario.expect)(&runner.engine.state) {
                    problems.push(error);
                }
                problems.extend(report.violations);
                problems.extend(report.owner_failures);
                sequence += 1;
                if sequence.is_multiple_of(SQLITE_EVERY) {
                    sqlite_checks += 1;
                    let store = TempStore::new("perm");
                    let mut sqlite = SqliteRunner::open(store, seed).map_err(|e| e.to_string())?;
                    let sqlite_report = run(scenario, &order, &mut sqlite);
                    if sqlite_report.semantic_hash != expected {
                        problems.push("SQLite semantic hash differs".to_owned());
                    }
                    let digest = sqlite.journal.replay_digest().map_err(|e| e.to_string())?;
                    if digest.projection_sha256 != digest.tables_sha256 {
                        problems.push("SQLite tables differ from state".to_owned());
                    }
                }
                if !problems.is_empty() {
                    family_failures += 1;
                    failing.push(json!({
                        "family": family, "scenario": scenario.name, "seed": seed, "problems": problems,
                    }));
                }
                family_count += 1;
            }
        }
        grand_total += family_count;
        if REQUIRED_FAMILIES.contains(family) {
            required_total += family_count;
        }
        families.insert(
            (*family).to_owned(),
            json!({
                "scenarios": members.iter().map(|(_, s)| s.name.clone()).collect::<Vec<_>>(),
                "permutations": family_count,
                "duplicateDeliveriesInjected": duplicates,
                "failures": family_failures,
                "seedRanges": seeds,
            }),
        );
    }
    area.json("failing-seeds.json", &json!(failing))?;
    let summary = json!({
        "area": "permutations",
        "pass": failing.is_empty() && required_total >= total,
        "requestedRequiredPermutations": total,
        "requiredFamilyPermutations": required_total,
        "totalPermutations": grand_total,
        "sqliteCrossChecks": sqlite_checks,
        "failures": failing.len(),
        "seedScheme": "seed = familyIndex * 1000000 + scenarioIndex * 100000 + index; duplicates injected when Rng(seed ^ 0xD0D0D0D0) draws 1/2",
        "families": families,
        "durationMs": started.elapsed().as_millis() as u64,
    });
    area.json("summary.json", &summary)?;
    Ok(summary)
}

/// The order with later duplicate deliveries removed (constraints apply to
/// first deliveries; a retry can arrive any time).
fn dedup_first(order: &[usize]) -> Vec<usize> {
    let mut seen = std::collections::BTreeSet::new();
    order.iter().copied().filter(|step| seen.insert(*step)).collect()
}

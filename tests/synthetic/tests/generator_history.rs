//! What the duplicate-redelivery fix (ec8ef5b) changed in the permutation
//! campaign of candidate f7e9a6c, recomputed from that campaign's own seed
//! ranges (`evidence/M1/history/f7e9a6c/permutations/summary.json`): the
//! generator before the fix inserted a duplicate anywhere; the current one
//! inserts it after every step its original must follow. Writes the
//! comparison to `evidence/M1/remediation-2/generator-orders.json` when
//! `THREADSPACE_WRITE_GENERATOR_ORDERS=1`.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};
use threadspace_synthetic::builder::{Scenario, Step};
use threadspace_synthetic::permute::{linear_extension, order_for, respects};
use threadspace_synthetic::rng::Rng;
use threadspace_synthetic::scenarios::catalog;

/// `order_for` as it was before ec8ef5b.
fn order_before_fix(scenario: &Scenario, seed: u64) -> Vec<usize> {
    let mut order = linear_extension(scenario.steps.len(), &scenario.constraints, seed);
    let mut rng = Rng::new(seed ^ 0xD0D0_D0D0);
    let observations: Vec<usize> =
        (0..scenario.steps.len()).filter(|&i| matches!(scenario.steps[i], Step::Observe(_))).collect();
    if rng.chance(1, 2) && !observations.is_empty() {
        for _ in 0..=rng.below(3) {
            let step = observations[rng.below(observations.len())];
            let at = rng.below(order.len() + 1);
            order.insert(at, step);
        }
    }
    order
}

fn first_deliveries(order: &[usize]) -> Vec<usize> {
    let mut seen = BTreeSet::new();
    order.iter().copied().filter(|step| seen.insert(*step)).collect()
}

/// Steps delivered (first) before a step they must follow.
fn violated(order: &[usize], scenario: &Scenario) -> BTreeSet<String> {
    let first = first_deliveries(order);
    let at = |step: usize| first.iter().position(|&s| s == step);
    scenario
        .constraints
        .iter()
        .filter(|&&(a, b)| matches!((at(a), at(b)), (Some(x), Some(y)) if x > y))
        .map(|&(_, b)| scenario.steps[b].label() + &format!("#{b}"))
        .collect()
}

#[test]
fn the_fix_changed_37_orders_28_invalid_and_9_valid_and_made_none_invalid() {
    let text = std::fs::read_to_string(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../evidence/M1/history/f7e9a6c/permutations/summary.json"
    ))
    .expect("archived campaign");
    let summary: Value = serde_json::from_str(&text).expect("json");
    let scenarios: BTreeMap<String, Scenario> = catalog().into_iter().map(|s| (s.name.clone(), s)).collect();

    let (mut seeds, mut changed, mut invalid_before, mut valid_changed, mut invalid_now) = (0, 0, 0, 0, 0);
    let mut by_scenario: BTreeMap<String, (usize, usize, usize)> = BTreeMap::new();
    let mut causes: BTreeMap<String, usize> = BTreeMap::new();
    for family in summary["families"].as_object().expect("families").values() {
        for range in family["seedRanges"].as_array().expect("ranges") {
            let scenario = &scenarios[range["scenario"].as_str().expect("name")];
            for seed in range["from"].as_u64().expect("from")..=range["to"].as_u64().expect("to") {
                seeds += 1;
                let before = order_before_fix(scenario, seed);
                let (now, _) = order_for(scenario, seed);
                let before_valid = respects(&first_deliveries(&before), &scenario.constraints);
                if !respects(&first_deliveries(&now), &scenario.constraints) {
                    invalid_now += 1;
                }
                if before == now {
                    continue;
                }
                changed += 1;
                let row = by_scenario.entry(scenario.name.clone()).or_default();
                row.0 += 1;
                if before_valid {
                    valid_changed += 1;
                    row.2 += 1;
                } else {
                    invalid_before += 1;
                    row.1 += 1;
                    let steps: Vec<String> = violated(&before, scenario).into_iter().collect();
                    *causes.entry(steps.join(" + ")).or_default() += 1;
                }
            }
        }
    }
    let report = json!({
        "campaign": "evidence/M1/history/f7e9a6c/permutations/summary.json",
        "seeds": seeds,
        "changedOrders": changed,
        "invalidBeforeFix": invalid_before,
        "validButChanged": valid_changed,
        "invalidAfterFix": invalid_now,
        "byScenario": by_scenario.iter().map(|(name, (c, i, v))| json!({ "scenario": name, "changed": c, "invalidBeforeFix": i, "validButChanged": v })).collect::<Vec<_>>(),
        "invalidBeforeFixByViolatedStep": causes,
    });
    if std::env::var("THREADSPACE_WRITE_GENERATOR_ORDERS").as_deref() == Ok("1") {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../evidence/M1/remediation-2/generator-orders.json");
        std::fs::write(path, serde_json::to_string_pretty(&report).expect("json") + "\n").expect("write");
    }
    println!("{report:#}");
    assert_eq!(invalid_now, 0, "the current generator makes no invalid order");
    assert_eq!((changed, invalid_before, valid_changed), (37, 28, 9), "{report:#}");
}

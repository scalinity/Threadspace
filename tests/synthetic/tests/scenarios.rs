//! Every catalogued scenario, in its reference order and in seeded valid
//! permutations, through the pure admission path.

use threadspace_state_engine::hash::state_hash;
use threadspace_state_engine::semantic::semantic_hash;
use threadspace_synthetic::permute::linear_extension;
use threadspace_synthetic::runner::{PureRunner, replay, run};
use threadspace_synthetic::scenarios::catalog;

#[test]
fn every_scenario_meets_its_expectation_in_reference_order() {
    let mut failures = Vec::new();
    for scenario in catalog() {
        let order: Vec<usize> = (0..scenario.steps.len()).collect();
        let mut runner = PureRunner::new(1);
        let report = run(&scenario, &order, &mut runner);
        if let Err(error) = (scenario.expect)(&runner.engine.state) {
            failures.push(format!("{}: {error}", scenario.name));
        }
        for violation in &report.violations {
            failures.push(format!("{}: invariant: {violation}", scenario.name));
        }
        for failure in &report.owner_failures {
            failures.push(format!("{}: owner: {failure}", scenario.name));
        }
        let replayed = replay(&runner.entries);
        if state_hash(&replayed.state) != report.state_hash {
            failures.push(format!("{}: exact replay differs", scenario.name));
        }
    }
    assert!(failures.is_empty(), "{failures:#?}");
}

#[test]
fn seeded_permutations_converge_semantically() {
    let mut failures = Vec::new();
    for scenario in catalog() {
        let reference: Vec<usize> = (0..scenario.steps.len()).collect();
        let mut base = PureRunner::new(1);
        let expected = run(&scenario, &reference, &mut base).semantic_hash;
        for seed in 0..40u64 {
            let order = linear_extension(scenario.steps.len(), &scenario.constraints, seed);
            let mut runner = PureRunner::new(1000 + seed);
            let report = run(&scenario, &order, &mut runner);
            if report.semantic_hash != expected {
                failures.push(format!("{} seed {seed}: semantic hash differs", scenario.name));
            } else if let Err(error) = (scenario.expect)(&runner.engine.state) {
                failures.push(format!("{} seed {seed}: {error}", scenario.name));
            }
            for violation in report.violations {
                failures.push(format!("{} seed {seed}: {violation}", scenario.name));
            }
            assert_eq!(semantic_hash(&runner.engine.state), report.semantic_hash);
        }
    }
    assert!(failures.is_empty(), "{} failures: {:#?}", failures.len(), &failures[..failures.len().min(20)]);
}

/// Negative controls: the semantic projection must notice missing evidence
/// and differing native identity, or convergence would prove nothing.
#[test]
fn semantic_equality_detects_missing_or_different_evidence() {
    for scenario in catalog() {
        let full: Vec<usize> = (0..scenario.steps.len()).collect();
        let mut base = PureRunner::new(1);
        let expected = run(&scenario, &full, &mut base).semantic_hash;
        // Dropping every copy of some state-changing step must change the
        // hash (rejected input and duplicates legitimately change nothing).
        let mut changed = false;
        for drop in (0..scenario.steps.len()).rev() {
            let dropped = &scenario.steps[drop];
            let order: Vec<usize> = full
                .iter()
                .copied()
                .filter(|&i| scenario.steps[i] != *dropped)
                .collect();
            let mut runner = PureRunner::new(1);
            if run(&scenario, &order, &mut runner).semantic_hash != expected {
                changed = true;
                break;
            }
        }
        assert!(changed, "{}: no single dropped step changed the projection", scenario.name);
    }
}

#[test]
fn allocated_ids_differ_but_semantics_do_not() {
    let scenario = threadspace_synthetic::scenarios::normal_session();
    let order: Vec<usize> = (0..scenario.steps.len()).collect();
    let (mut a, mut b) = (PureRunner::new(1), PureRunner::new(2));
    let (ra, rb) = (run(&scenario, &order, &mut a), run(&scenario, &order, &mut b));
    assert_ne!(ra.state_hash, rb.state_hash, "different allocations give different exact states");
    assert_eq!(ra.semantic_hash, rb.semantic_hash, "and identical semantics");
}

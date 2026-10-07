//! Scoped causal comparison (SPEC §5.4). Points compare only within one
//! source, epoch and order domain, or through an explicit native predecessor
//! relation. Independent counters, epochs and wall time never order facts.

use threadspace_contracts::canonical::causal::{CausalOrder, CausalPoint};
use threadspace_contracts::cursor::parse_cursor;

fn same_domain(a: &CausalPoint, b: &CausalPoint) -> bool {
    a.source_id == b.source_id && a.source_epoch == b.source_epoch && a.order_domain == b.order_domain
}

fn follows(later: &CausalPoint, earlier: &CausalPoint) -> bool {
    earlier
        .native_key
        .as_ref()
        .is_some_and(|key| later.native_predecessor_keys.contains(key))
}

pub fn compare(a: &CausalPoint, b: &CausalPoint) -> CausalOrder {
    if follows(b, a) {
        return CausalOrder::Before;
    }
    if follows(a, b) {
        return CausalOrder::After;
    }
    if !same_domain(a, b) {
        return CausalOrder::Incomparable;
    }
    let (Some(x), Some(y)) = (
        a.sequence.as_deref().and_then(parse_cursor),
        b.sequence.as_deref().and_then(parse_cursor),
    ) else {
        return CausalOrder::Incomparable;
    };
    match x.cmp(&y) {
        std::cmp::Ordering::Less => CausalOrder::Before,
        std::cmp::Ordering::Greater => CausalOrder::After,
        std::cmp::Ordering::Equal => CausalOrder::Equal,
    }
}

/// True when `a` is known to precede `b`.
pub fn before(a: &CausalPoint, b: &CausalPoint) -> bool {
    compare(a, b) == CausalOrder::Before
}

#[cfg(test)]
mod tests {
    use super::*;

    fn point(epoch: &str, domain: &str, sequence: u64) -> CausalPoint {
        CausalPoint {
            source_id: "src".into(),
            source_epoch: epoch.into(),
            order_domain: domain.into(),
            sequence: Some(sequence.to_string()),
            native_key: None,
            native_predecessor_keys: Vec::new(),
        }
    }

    #[test]
    fn same_domain_orders_numerically_not_lexically() {
        assert_eq!(compare(&point("e", "d", 9), &point("e", "d", 10)), CausalOrder::Before);
        assert_eq!(compare(&point("e", "d", 10), &point("e", "d", 9)), CausalOrder::After);
        assert_eq!(compare(&point("e", "d", 4), &point("e", "d", 4)), CausalOrder::Equal);
    }

    #[test]
    fn other_epochs_and_domains_are_incomparable() {
        assert_eq!(compare(&point("e1", "d", 1), &point("e2", "d", 9)), CausalOrder::Incomparable);
        assert_eq!(compare(&point("e", "entry", 1), &point("e", "result", 9)), CausalOrder::Incomparable);
    }

    #[test]
    fn native_predecessor_relations_cross_domains() {
        let mut output = point("e1", "turn", 7);
        output.native_key = Some("turn-7-output".into());
        let mut input = point("e2", "prompt", 1);
        input.native_predecessor_keys = vec!["turn-7-output".into()];
        assert_eq!(compare(&output, &input), CausalOrder::Before);
        assert_eq!(compare(&input, &output), CausalOrder::After);
    }
}

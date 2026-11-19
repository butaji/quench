//! The supernode library the planner reads: the rules (`supernode_rules.rs`) and the list
//! `build.rs` generated from `crates/quench-bjit/supernodes.txt` (`build/supernodes.rs`). The
//! generated file, which `QUENCH_SUPERNODES` names, holds `FAMILIES` (the opcodes the list's labels
//! may name, with their row family) and `SUPERNODES` (the entries, each with its miner pattern text,
//! feeds, place sets and evidence). `build.rs` itself includes `supernode_rules.rs` alone, since the
//! list does not exist until it has run.

/// The rules read the library through their parent (`supernode_rules.rs`, `use super::library`).
use super::library;
#[path = "supernode_rules.rs"]
mod rules;
pub use rules::*;

include!(env!("QUENCH_SUPERNODES"));

#[cfg(test)]
mod tests {
    use crate::numeric::library::*;
    use super::*;
    use std::collections::BTreeSet;

    #[test]
    fn the_checked_in_list_parses_and_fits_its_budgets() {
        assert!(SUPERNODES.len() <= MAX_SUPERNODES);
        for supernode in SUPERNODES {
            let shape = parse(supernode.pattern, FAMILIES).expect(supernode.pattern);
            assert!(matches!(shape.keys[1], Key::Number(_)), "{}", supernode.pattern);
            assert!(!supernode.feeds.is_empty() && !supernode.evidence.is_empty());
        }
        assert!(instances(SUPERNODES, FAMILIES).len() <= MAX_VARIANTS, "{}", instances(SUPERNODES, FAMILIES).len());
    }

    #[test]
    fn miner_patterns_parse_to_keys_and_links() {
        let shape = parse("Binary(>>) ; Binary(&) [0>1]", FAMILIES).unwrap();
        assert_eq!(shape, Shape { keys: vec![Key::Number(SHR), Key::Number(AND)], links: vec![(0, 1)] });
        assert_eq!(parse("GetIndex ; NumericAdd(+) [0>1]", FAMILIES).unwrap().keys[0], Key::Heap(INDEX_ROW));
        assert_eq!(parse("Binary(<<) ; JumpBinaryFalse(>=) [0>1]", FAMILIES).unwrap().keys[1], Key::Number(GE));
    }

    #[test]
    fn patterns_the_library_cannot_build_are_refused() {
        for text in [
            "Binary(&) ; Binary(<) [0>1]",
            "JumpBinaryFalse(+) ; Binary(&) [0>1]",
            "Move ; NumericAdd(+) [0>1]",
            "NumericAdd(+) ; NumericAdd(+) []",
            "NumericAdd(+) ; NumericAdd(+) [1>0]",
            "NumericAdd(+) ; NumericAdd(+) ; NumericAdd(+) [0>1,1>2]",
            "NumericAdd(+)",
        ] {
            assert_eq!(parse(text, FAMILIES), None, "{text}");
        }
    }

    #[test]
    fn a_variant_keeps_the_value_between_rows_in_its_own_pin_and_the_hole_roles_apart() {
        let mut names = BTreeSet::new();
        for nodes in instances(SUPERNODES, FAMILIES) {
            let (Node::Number((_, dst, a0, b0, _)) | Node::Heap((_, _, dst, a0, b0))) = nodes[0];
            assert!((PIN..=LAST_INT_PIN).contains(&dst), "{nodes:?}");
            assert_eq!(roles(&nodes[0]) & roles(&nodes[1]), 0, "{nodes:?}");
            let Node::Number((_, second_dst, a, b, _)) = nodes[1] else { panic!("{nodes:?}") };
            assert!(![a0, b0, second_dst].contains(&dst), "{nodes:?}");
            assert_eq!([a, b].iter().filter(|place| **place == dst).count(), 1, "{nodes:?}");
            assert!(names.insert(super::name(&nodes)), "{nodes:?}");
        }
    }

    #[test]
    fn roles_name_the_hole_users() {
        assert_eq!(roles(&Node::Number((ADD, PIN, PIN + 1, IMM, true))), 1 << 2);
        assert_eq!(roles(&Node::Number((LT, MEM, PIN, PIN + 1, true))), 0);
        assert_eq!(roles(&Node::Heap((INDEX_ROW, false, PIN, MEM, PIN))), 1 << 1);
    }
}

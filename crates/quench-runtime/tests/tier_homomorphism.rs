//! Tier homomorphism, first oracle (TOD-2177, S1.14): every staged Number row, run on the
//! interpreter projection (`Concrete`) over synthetic operand states, must answer what an
//! independent reference says the ECMAScript operation answers. The rows are enumerated from the
//! same tables the native tiers consume (`number_*_rows!`), so a new operator is covered without a
//! new list here. The references are written from the spec in i64 and f64 arithmetic, not by
//! calling the code under test.
//!
//! The rows, states, frame and references are in `tier_homomorphism/shared.rs`, which the u-jit
//! comparison mounts too: `quench-runtime-engine`'s `emit/tier_homomorphism_tests.rs` runs the same
//! rows and states through `Emit` (Cranelift) and compares each with Concrete. The b-jit stencil run
//! needs a stencil build and is not compared yet. Design: `/mnt/project-files/v2-merged/s114/design.md`.

/// Each mount uses part of it: the Emit comparison reads operand representations, this test the references.
#[allow(dead_code)]
mod shared {
    include!("tier_homomorphism/shared.rs");
}

use shared::{cases, expected_row_count, reference, rows, run, same, seed, NumberWord, Outcome, Rng};

/// Counts of what the test covered, and every disagreement (all are reported, not the first).
#[derive(Default)]
struct Coverage {
    rows: usize,
    states: usize,
    conservative_leaves: usize,
    mismatches: Vec<String>,
}

/// Records a disagreement of Concrete's `actual` with the reference's `expected`. `may_leave` marks
/// a state where a leave is allowed to stand for the reference's value (`conservative_wrap_leave`).
fn check(coverage: &mut Coverage, label: &str, (expected, may_leave): (Outcome, bool), actual: Outcome,
    (left, right): (NumberWord, NumberWord)) {
    coverage.states += 1;
    if same(expected, actual) {
        return;
    }
    if may_leave && actual.is_none() && expected.is_some() {
        coverage.conservative_leaves += 1;
        return;
    }
    coverage.mismatches.push(format!("{label}: left {left:?} right {right:?}: reference {expected:?}, Concrete {actual:?}"));
}

#[test]
fn concrete_number_rows_match_the_ecmascript_reference() {
    let cases = cases();
    let mut coverage = Coverage::default();
    for row in rows(&mut Rng(seed()), cases) {
        coverage.rows += 1;
        for &(left, right) in &row.states {
            let expected = reference(row.operation, left, right);
            check(&mut coverage, &row.label, expected, run(left, right, row.operation), (left, right));
        }
    }
    assert_eq!(coverage.rows, expected_row_count(), "the enumerated row count changed");
    eprintln!("tier_homomorphism: {} rows, {} states (seed {:#x}, {cases} random per row), {} allowed conservative leaves",
        coverage.rows, coverage.states, seed(), coverage.conservative_leaves);
    assert!(coverage.mismatches.is_empty(), "{} disagreements, first {:?}", coverage.mismatches.len(),
        &coverage.mismatches[..coverage.mismatches.len().min(12)]);
}

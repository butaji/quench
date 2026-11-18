//! Auto-possessification: a greedy unit repeat whose next consuming instruction needs a unit
//! the repeat's test rejects can never use a unit it gives back, so it keeps no frame to give
//! units back. A static fact of the program, so every tier that runs it gains: `[\w.]+@`,
//! `(\w+)\s`, `\d{4}-`, `\w+(?=@)` backtrack nothing.
use super::{Inst, MatchSet, UnitTest, ASCII_UNITS};

/// Marks each greedy `UnitRepeat` whose follower rejects every unit it takes `atomic`.
pub(super) fn mark(instructions: &mut [Inst], sets: &[MatchSet]) {
    for pc in 0..instructions.len() {
        let Inst::UnitRepeat { test, greedy: true, atomic: false, .. } = instructions[pc] else { continue };
        if follower(&instructions[pc + 1..]).is_some_and(|next| disjoint(test, next, sets)) {
            if let Inst::UnitRepeat { atomic, .. } = &mut instructions[pc] {
                *atomic = true;
            }
        }
    }
}

/// The test the first unit after a repeat must pass, on the path a frame of the repeat
/// resumes: past the capture bookkeeping, which neither consumes nor fails, and into a
/// positive lookahead's body, which must match at that same position.
fn follower(rest: &[Inst]) -> Option<UnitTest> {
    rest.iter().find_map(|inst| match inst {
        Inst::CaptureOpen { .. } | Inst::CaptureClose { .. } | Inst::Look { negative: false, .. } => None,
        Inst::Unit(test) | Inst::UnitRepeat { test, min: 1.., .. } => Some(Some(*test)),
        _ => Some(None),
    })?
}

/// Whether no unit passes both tests.
fn disjoint(left: UnitTest, right: UnitTest, sets: &[MatchSet]) -> bool {
    match (left, right) {
        (UnitTest::Exact(left), UnitTest::Exact(right)) => left != right,
        (UnitTest::Exact(unit), UnitTest::Set(set)) | (UnitTest::Set(set), UnitTest::Exact(unit)) => !sets[set as usize].contains(unit),
        (UnitTest::Set(left), UnitTest::Set(right)) => {
            let (left, right) = (&sets[left as usize], &sets[right as usize]);
            let ascii = (0..ASCII_UNITS as u8).all(|unit| !(left.table.contains(unit) && right.table.contains(unit)));
            ascii && (left.beyond.ascii_only() || right.beyond.ascii_only())
        }
    }
}

//! Auto-possessification: a greedy unit repeat whose next consuming instruction needs a unit
//! the repeat's test rejects can never use a unit it gives back, so it keeps no frame to give
//! units back. A static fact of the program, so every tier that runs it gains: `[\w.]+@`,
//! `(\w+)\s`, `\d{4}-`, `\w+(?=@)` backtrack nothing.
use super::{Beyond, UNBOUNDED, ClassItem, ClassKind, Inst, MatchSet, UnitTest, ASCII_UNITS};

/// The test of the repeat a run begins with (past the openings of groups) where a failed run
/// from a start fails from each later start the repeat's units cover, and at their end: the
/// repeat is atomic, unbounded and takes at least one unit, so from a later start inside its
/// units it ends where it ended and the rest of the run, which reads positions and units
/// but no capture (no backreference), fails as it did; and at their end it takes no unit.
/// `None` where a search's runs do not begin at its start (an anchor or a prefix scan).
pub(super) fn restart_skip(instructions: &[Inst], scanned: bool) -> Option<UnitTest> {
    let references = instructions.iter().any(|inst| matches!(inst, Inst::Backreference { .. } | Inst::BackreferenceBack { .. }));
    if scanned || references {
        return None;
    }
    let first = instructions.iter().find(|inst| !matches!(inst, Inst::CaptureOpen { .. }))?;
    match *first {
        Inst::UnitRepeat { test, min, max: UNBOUNDED, atomic: true, .. } if min >= 1 => Some(test),
        _ => None,
    }
}

/// The leading atomic repeat's test and the literal byte that must follow it, where every
/// match begins with an unbounded run of units passing the test (at least one) and then that
/// byte, which the test refuses: a match starts only at the start of a run of the test's units
/// that ends just before an occurrence of the byte (`Program::inner_literal_start`).
pub(super) fn run_then_literal(instructions: &[Inst], sets: &[MatchSet], scanned: bool) -> Option<(UnitTest, u8)> {
    let test = restart_skip(instructions, scanned)?;
    let mut rest = instructions.iter().filter(|inst| !matches!(inst, Inst::CaptureOpen { .. } | Inst::CaptureClose { .. }));
    let _repeat = rest.next()?;
    let Inst::Unit(UnitTest::Exact(literal)) = *rest.next()? else { return None };
    let byte = u8::try_from(literal).ok().filter(u8::is_ascii)?;
    let refused = match test {
        UnitTest::Exact(unit) => unit != literal,
        UnitTest::Set(set) => !sets[set as usize].contains(literal),
    };
    refused.then_some((test, byte))
}

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

impl Beyond {
    /// Whether the test admits no unit past ASCII, by construction: the possessive pass
    /// (`possessive::disjoint`) then decides a pair of sets by their ASCII tables alone.
    pub(super) fn ascii_only(&self) -> bool {
        match self {
            Self::Never => true,
            Self::Either(either) => either.iter().all(Self::ascii_only),
            Self::Ranges { ranges, negative: false } => ranges.last().is_none_or(|&(_, max)| max < ASCII_UNITS),
            Self::Class { class, ignore_case, unicode } => !class.negative
                && matches!(class.kind, ClassKind::Union)
                && class.items.iter().all(|item| class_item_ascii_only(item, *ignore_case && *unicode)),
            _ => false,
        }
    }
}

/// Whether a class item admits no unit past ASCII: digits, and word units unless case-folded
/// in Unicode mode (where U+017F and U+212A fold to `s` and `k`).
fn class_item_ascii_only(item: &ClassItem, folded_unicode: bool) -> bool {
    use oxc::regular_expression::ast::CharacterClassEscapeKind as Escape;
    match item {
        ClassItem::Escape(Escape::D) => true,
        ClassItem::Escape(Escape::W) => !folded_unicode,
        _ => false,
    }
}

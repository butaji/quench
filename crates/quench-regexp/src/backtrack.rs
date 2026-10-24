//! A compiled backtracking matcher for the lowered pattern (`Expr`).
//!
//! The ECMAScript pattern semantics are a backtracking search: alternatives in order,
//! quantifiers greedy or lazy, captures reset per iteration and empty iterations rejected
//! once the minimum is met. `match_expr_with` states them as continuation-passing
//! recursion; this matcher runs the same steps as a program over registers, with an
//! explicit stack of choice points and register restores, so a match allocates nothing
//! once its workspace exists.
//!
//! Lookbehind and classes that can match a string of several units are not compiled; such
//! patterns keep the repository matcher. A search may also stop early when a step budget
//! is given (`Program::find`), so that a pattern which another exact backend matches in
//! linear time never backtracks without bound here.

use std::cell::RefCell;
use std::ops::Range;

use super::{
    canonical, canonical_preimages_of, class_is_single_unit, class_item_matches, class_matches, equal, is_line_terminator, is_word_mode, merge_flags,
    literal_finder, Assertion, Backreference, ClassExpr, ClassItem, ClassKind, Expr, Flags, LiteralFinder, Lookaround,
    Unit, ASCII_MAX,
};

/// A register holding no position: an unset capture bound.
const UNSET: usize = usize::MAX;

/// A quantifier bound without a maximum.
const UNBOUNDED: u32 = u32::MAX;

/// The code units an ASCII bitmap covers.
const ASCII_UNITS: u32 = ASCII_MAX as u32 + 1;

/// Steps a budgeted search may take per subject unit, plus a fixed allowance, before it
/// defers to the linear-time backend. Ordinary patterns take a few steps per unit at each
/// start position they try; exhausting the budget signals backtracking that grows faster
/// than the subject.
const BUDGET_STEPS_PER_UNIT: usize = 1024;
const BUDGET_BASE_STEPS: usize = 1 << 16;

/// The subject a program matches: one unit per position.
pub(crate) trait Text {
    fn len(&self) -> usize;
    fn unit(&self, index: usize) -> u32;
    /// The units as bytes, for a subject of ASCII units.
    fn bytes(&self) -> Option<&[u8]>;
}

impl Text for [u8] {
    #[inline(always)]
    fn len(&self) -> usize {
        <[u8]>::len(self)
    }

    #[inline(always)]
    fn unit(&self, index: usize) -> u32 {
        u32::from(self[index])
    }

    fn bytes(&self) -> Option<&[u8]> {
        Some(self)
    }
}

impl Text for [u16] {
    #[inline(always)]
    fn len(&self) -> usize {
        <[u16]>::len(self)
    }

    #[inline(always)]
    fn unit(&self, index: usize) -> u32 {
        u32::from(self[index])
    }

    fn bytes(&self) -> Option<&[u8]> {
        None
    }
}

impl Text for [Unit] {
    #[inline(always)]
    fn len(&self) -> usize {
        <[Unit]>::len(self)
    }

    #[inline(always)]
    fn unit(&self, index: usize) -> u32 {
        self[index].value
    }

    fn bytes(&self) -> Option<&[u8]> {
        None
    }
}

/// What one subject unit must be.
#[derive(Clone, Copy)]
enum UnitTest {
    Exact(u32),
    /// An index into `Program::sets`.
    Set(u32),
}

/// A set of units: exact for ASCII by bitmap, and by `beyond` for any other unit.
struct UnitSet {
    ascii: u128,
    beyond: Beyond,
}

/// The ASCII units of a mask as one flag per unit, derived for the sets a program matches
/// with (`MatchSet`): a test of an ASCII unit loads its flag instead
/// of shifting the 128-bit mask. The set algebra that builds those sets keeps only the mask.
type AsciiTable = [bool; ASCII_UNITS as usize];

fn ascii_table(ascii: u128) -> AsciiTable {
    std::array::from_fn(|unit| ascii >> unit & 1 != 0)
}

/// A set a program's instructions test units against: the set with its `AsciiTable`.
struct MatchSet {
    table: AsciiTable,
    beyond: Beyond,
}

impl From<UnitSet> for MatchSet {
    fn from(set: UnitSet) -> Self {
        Self {
            table: ascii_table(set.ascii),
            beyond: set.beyond,
        }
    }
}

impl MatchSet {
    #[inline(always)]
    fn contains(&self, value: u32) -> bool {
        if value < ASCII_UNITS {
            self.table[value as usize]
        } else {
            self.beyond.matches(value)
        }
    }
}

/// How a `UnitSet` decides a unit past ASCII.
#[derive(Clone)]
enum Beyond {
    Never,
    /// Either set's test.
    Either(Box<[Beyond; 2]>),
    Dot {
        dot_all: bool,
    },
    Literal {
        value: u32,
        unicode: bool,
    },
    Class {
        class: ClassExpr,
        ignore_case: bool,
        unicode: bool,
    },
    /// A class of characters and ranges matched exactly: its members as sorted, disjoint
    /// inclusive ranges, which `negative` complements.
    Ranges {
        ranges: Box<[(u32, u32)]>,
        negative: bool,
    },
}

/// `ranges` as sorted, disjoint inclusive ranges: overlapping and adjacent ones joined.
fn coalesced_ranges(mut ranges: Vec<(u32, u32)>) -> Box<[(u32, u32)]> {
    ranges.sort_unstable();
    let mut merged: Vec<(u32, u32)> = Vec::with_capacity(ranges.len());
    for (min, max) in ranges {
        match merged.last_mut() {
            Some(last) if min <= last.1.saturating_add(1) => last.1 = last.1.max(max),
            _ => merged.push((min, max)),
        }
    }
    merged.into_boxed_slice()
}

impl UnitSet {
    /// The set of the ASCII units in `ascii` and the units past ASCII `beyond` admits.
    fn of(ascii: u128, beyond: Beyond) -> Self {
        Self { ascii, beyond }
    }

    /// The set of both: their tests are moved, not copied, as a fold over an alternation's
    /// branches would otherwise copy the growing test once per branch.
    fn union(self, other: Self) -> Self {
        let beyond = match (self.beyond, other.beyond) {
            (Beyond::Never, beyond) | (beyond, Beyond::Never) => beyond,
            // Two sets of exact member ranges are one: a fold over an alternation of classes
            // tests one sorted range list rather than every branch's.
            (
                Beyond::Ranges { ranges: left, negative: false },
                Beyond::Ranges { ranges: right, negative: false },
            ) => Beyond::Ranges {
                ranges: coalesced_ranges(left.iter().chain(right.iter()).copied().collect()),
                negative: false,
            },
            (left, right) => Beyond::Either(Box::new([left, right])),
        };
        Self::of(self.ascii | other.ascii, beyond)
    }

    fn new(beyond: Beyond) -> Self {
        Self::of(beyond.ascii_mask(), beyond)
    }

    /// Whether no unit is in the set.
    fn is_empty(&self) -> bool {
        self.ascii == 0 && self.beyond.is_never()
    }
}

impl Beyond {
    /// The test `class` makes of a unit: a union of characters and ranges without case
    /// folding is its member ranges, derived once; any other class tests its items.
    fn class(class: &ClassExpr, ignore_case: bool, unicode: bool) -> Self {
        let mut ranges = Vec::with_capacity(class.items.len());
        let plain = !ignore_case
            && matches!(class.kind, ClassKind::Union)
            && class.items.iter().all(|item| match *item {
                ClassItem::Character(character) => {
                    ranges.push((character, character));
                    true
                }
                ClassItem::Range(min, max) => {
                    ranges.push((min, max));
                    true
                }
                _ => false,
            });
        if !plain {
            return Self::Class {
                class: class.clone(),
                ignore_case,
                unicode,
            };
        }
        Self::Ranges {
            ranges: coalesced_ranges(ranges),
            negative: class.negative,
        }
    }

    /// The ASCII units the test admits, one bit per unit: derived from the ranges where the
    /// test is exact ranges, and by testing each ASCII unit otherwise.
    fn ascii_mask(&self) -> u128 {
        match self {
            Self::Never => 0,
            Self::Either(either) => either.iter().fold(0, |bits, beyond| bits | beyond.ascii_mask()),
            Self::Ranges { ranges, negative } => {
                let members = ranges
                    .iter()
                    .take_while(|(min, _)| *min < ASCII_UNITS)
                    .fold(0u128, |bits, &(min, max)| bits | range_ascii_mask(min, max));
                if *negative { !members } else { members }
            }
            Self::Literal { value, unicode } => folded_ascii_mask(*value, *unicode),
            Self::Class { class, ignore_case, unicode } => class_ascii_mask(class, *ignore_case, *unicode),
            _ => (0..ASCII_UNITS)
                .filter(|value| self.matches(*value))
                .fold(0u128, |bits, value| bits | 1 << value),
        }
    }

    /// Whether the test rejects every unit by construction.
    fn is_never(&self) -> bool {
        match self {
            Self::Never => true,
            Self::Either(either) => either.iter().all(Self::is_never),
            _ => false,
        }
    }

    fn matches(&self, value: u32) -> bool {
        match self {
            Self::Never => false,
            Self::Either(either) => either.iter().any(|beyond| beyond.matches(value)),
            Self::Dot { dot_all } => *dot_all || !is_line_terminator(value),
            Self::Literal {
                value: literal,
                unicode,
            } => equal(*literal, value, true, *unicode),
            Self::Class {
                class,
                ignore_case,
                unicode,
            } => class_matches(class, value, *ignore_case, *unicode),
            Self::Ranges { ranges, negative } => {
                let after = ranges.partition_point(|(min, _)| *min <= value);
                let member = after != 0 && value <= ranges[after - 1].1;
                member != *negative
            }
        }
    }
}

/// The ASCII units of `min..=max`, one bit per unit.
fn range_ascii_mask(min: u32, max: u32) -> u128 {
    if min >= ASCII_UNITS || min > max {
        return 0;
    }
    let max = max.min(ASCII_UNITS - 1);
    (u128::MAX >> (ASCII_UNITS - 1 - max)) & (u128::MAX << min)
}

/// The ASCII units equal to `value` under case folding (`equal` ignoring case): those
/// folding to its canonical form, which are that form and its preimages.
fn folded_ascii_mask(value: u32, unicode: bool) -> u128 {
    let form = canonical(value, unicode);
    std::iter::once(form)
        .chain(canonical_preimages_of(form, unicode))
        .filter(|unit| *unit < ASCII_UNITS && canonical(*unit, unicode) == form)
        .fold(0u128, |bits, unit| bits | 1 << unit)
}

/// The ASCII units `class` admits (`class_matches`), combined from each item's as its kind
/// combines their tests.
fn class_ascii_mask(class: &ClassExpr, ignore_case: bool, unicode: bool) -> u128 {
    let mut items = class.items.iter().map(|item| class_item_ascii_mask(item, ignore_case, unicode));
    let members = match class.kind {
        ClassKind::Union => items.fold(0, |bits, item| bits | item),
        ClassKind::Intersection => items.fold(u128::MAX, |bits, item| bits & item),
        ClassKind::Subtraction => {
            let first = items.next().unwrap_or(0);
            items.fold(first, |bits, item| bits & !item)
        }
    };
    if class.negative { !members } else { members }
}

/// The ASCII units one class item admits (`class_item_matches`): exact for a character and,
/// without case folding, a range; by testing each ASCII unit otherwise.
fn class_item_ascii_mask(item: &ClassItem, ignore_case: bool, unicode: bool) -> u128 {
    match *item {
        ClassItem::Character(character) if !ignore_case => range_ascii_mask(character, character),
        ClassItem::Character(character) => folded_ascii_mask(character, unicode),
        ClassItem::Range(min, max) if !ignore_case => range_ascii_mask(min, max),
        _ => (0..ASCII_UNITS)
            .filter(|value| class_item_matches(item, *value, ignore_case, unicode))
            .fold(0u128, |bits, value| bits | 1 << value),
    }
}

/// An assertion with the flags that decide it.
#[derive(Clone, Copy)]
struct AssertionTest {
    assertion: Assertion,
    multiline: bool,
    /// `is_word_mode`'s case-folding argument.
    word_ignore_case: bool,
}

#[derive(Clone, Copy)]
enum Inst {
    Unit(UnitTest),
    /// `min..=max` units of a test, without captures and never empty, so its choices are
    /// only how many units it takes.
    UnitRepeat {
        test: UnitTest,
        min: u32,
        max: u32,
        greedy: bool,
    },
    Assert(AssertionTest),
    /// Continue at the next instruction; on backtracking, at `alternative`.
    Split {
        alternative: u32,
    },
    /// `Split` whose next instruction begins a body that can start only at the positions
    /// `Program::guards[guard]` admits: elsewhere the body fails at once, so the run goes
    /// straight to `alternative` and keeps no choice.
    GuardedSplit {
        alternative: u32,
        guard: u32,
    },
    Jump(u32),
    /// A group's start into its temporary register: with a restore entry where the group
    /// may run again on one path (`Compiler::repeated`), plainly elsewhere, as the group's
    /// own open then always writes the temporary again before its close reads it.
    CaptureOpen {
        temporary: u32,
        restores: bool,
    },
    CaptureClose {
        capture: u32,
        temporary: u32,
    },
    /// Unset the captures listed in `Program::lists[first..first + count]`.
    ResetCaptures {
        first: u32,
        count: u32,
    },
    LoopInit {
        counter: u32,
    },
    /// A quantifier's decision before an iteration of the body that follows it.
    LoopHead {
        counter: u32,
        min: u32,
        max: u32,
        greedy: bool,
        exit: u32,
    },
    /// The start of one iteration, kept for the empty check.
    LoopIterate {
        start: u32,
    },
    /// The end of the one iteration an optional atom (`max` of one, `min` of zero) makes:
    /// it fails having consumed nothing.
    EmptyCheck {
        start: u32,
    },
    LoopTail {
        counter: u32,
        start: u32,
        min: u32,
        head: u32,
    },
    /// A backreference to the first set capture of `Program::lists[first..first + count]`.
    Backreference {
        first: u32,
        count: u32,
        ignore_case: bool,
        unicode: bool,
    },
    /// A lookahead whose body follows, ending at `LookSucceed`; matching continues at `end`.
    Look {
        negative: bool,
        end: u32,
    },
    LookSucceed,
    Match,
}

/// A backtracking stack entry.
#[derive(Clone, Copy)]
enum Frame {
    Choice {
        pc: u32,
        position: usize,
    },
    Restore {
        register: u32,
        value: usize,
    },
    /// Both bounds of a capture, which every capture write sets together.
    RestoreCapture {
        capture: u32,
        start: usize,
        end: usize,
    },
    /// A greedy `UnitRepeat` that may give back units down to `floor`.
    Greedy {
        pc: u32,
        floor: usize,
        position: usize,
    },
    /// A lazy `UnitRepeat` that may take units up to `ceiling`.
    Lazy {
        pc: u32,
        test: UnitTest,
        ceiling: usize,
        position: usize,
    },
    /// The bottom of one run: a search, or a lookahead body.
    Barrier,
}

/// The positions a search may start a match at, through `last`.
#[derive(Clone, Copy)]
struct Search {
    last: usize,
}

/// The search ran past its step budget.
pub(crate) struct Exhausted;

/// A match: the start and end positions and each capture's positions.
pub(crate) struct Found {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) captures: Vec<Option<Range<usize>>>,
}

/// Registers and stack reused across searches.
#[derive(Default)]
struct Workspace {
    registers: Vec<usize>,
    stack: Vec<Frame>,
}

pub(crate) struct Program {
    instructions: Box<[Inst]>,
    sets: Box<[MatchSet]>,
    /// Capture index lists for `ResetCaptures` and `Backreference`.
    lists: Box<[u32]>,
    /// The starts of guarded bodies, for `GuardedSplit`.
    guards: Box<[MatchStarts]>,
    capture_count: usize,
    register_count: usize,
    /// Where matches can start; `None` where any position can.
    starts: Option<MatchStarts>,
    /// How an ASCII subject's scan finds the next of `starts`' units; derived from them once
    /// here, as the set algebra that builds `starts` never scans. Only their ASCII part
    /// matters there: an ASCII subject holds no other unit.
    scan: AsciiScan,
    /// The literal units every match begins with, searched for directly in ASCII subjects.
    prefix: Option<Prefix>,
    /// Where a run starts once the prefix scan has found its units: past the prefix's
    /// `Inst::Unit`s, at this pc and this many units on. `None` when no scan proves them,
    /// as an `anchored` search's single start, which no scan finds.
    entry: Option<u32>,
    /// Whether a match can only start at the subject's start (`Starts::only_at_start` with
    /// `at_start`); one with neither start can never match, which the run finds at once.
    anchored: bool,
    /// For an `anchored` pattern, a superset of the units its match consumes first at the
    /// subject's start (`anchored_first_units`), so a search whose first unit is outside it
    /// fails without a run; `None` where no such set is known.
    anchored_first: Option<MatchSet>,
    /// The length of `prefix` when it is the whole pattern and the pattern has no captures:
    /// an ASCII subject's match is then where the search finds it, with no run. A folded
    /// prefix holds this only in an ASCII subject, whose units have no other case forms
    /// (the Kelvin sign and long s are past ASCII); other subjects run the program.
    literal_length: Option<usize>,
}

thread_local! {
    /// The registers and stack of the running search. A search never starts another, so
    /// every program on a thread shares one workspace, sized by the largest search so far.
    static WORKSPACE: RefCell<Workspace> = RefCell::default();
}

/// The compiled form of a construct this matcher does not run.
struct Unsupported;

struct Compiler {
    instructions: Vec<Inst>,
    sets: Vec<MatchSet>,
    lists: Vec<u32>,
    guards: Vec<MatchStarts>,
    registers: u32,
    /// Bodies being compiled that one path may run more than once, or whose registers a
    /// caller keeps across their run: loops with more than one iteration and lookarounds.
    /// Outside them, a group runs at most once per path.
    repeated: u32,
}

impl Compiler {
    fn emit(&mut self, instruction: Inst) -> u32 {
        self.instructions.push(instruction);
        self.pc() - 1
    }

    fn pc(&self) -> u32 {
        self.instructions.len() as u32
    }

    fn register(&mut self) -> u32 {
        self.registers += 1;
        self.registers - 1
    }

    fn list(&mut self, items: impl IntoIterator<Item = usize>) -> Result<(u32, u32), Unsupported> {
        let first = self.lists.len() as u32;
        for item in items {
            self.lists
                .push(u32::try_from(item).map_err(|_| Unsupported)?);
        }
        Ok((first, self.lists.len() as u32 - first))
    }

    /// A split to `alternative` before a body `expr`, guarded by the body's starts when
    /// they are known.
    fn split(&mut self, alternative: u32, body: &Expr, flags: Flags) -> Inst {
        match starts(body, flags) {
            Some(guard) => {
                self.guards.push(guard.into());
                Inst::GuardedSplit {
                    alternative,
                    guard: self.guards.len() as u32 - 1,
                }
            }
            None => Inst::Split { alternative },
        }
    }

    fn set(&mut self, beyond: Beyond) -> UnitTest {
        self.sets.push(UnitSet::new(beyond).into());
        UnitTest::Set(self.sets.len() as u32 - 1)
    }

    /// The test of a single-unit expression, if `expr` is one.
    fn unit_test(&mut self, expr: &Expr, flags: Flags) -> Result<Option<UnitTest>, Unsupported> {
        Ok(Some(match expr {
            Expr::Literal(value) if !flags.ignore_case => UnitTest::Exact(*value),
            Expr::Literal(value) => self.set(Beyond::Literal {
                value: *value,
                unicode: flags.unicode,
            }),
            Expr::Dot => self.set(Beyond::Dot {
                dot_all: flags.dot_all,
            }),
            Expr::Class(class) => {
                if !class_is_single_unit(class) {
                    return Err(Unsupported);
                }
                self.set(Beyond::class(class, flags.ignore_case, flags.unicode))
            }
            _ => return Ok(None),
        }))
    }

    fn compile(&mut self, expr: &Expr, flags: Flags) -> Result<(), Unsupported> {
        if let Some(test) = self.unit_test(expr, flags)? {
            self.emit(Inst::Unit(test));
            return Ok(());
        }
        match expr {
            Expr::Sequence(parts) => {
                for part in parts {
                    self.compile(part, flags)?;
                }
            }
            Expr::Alternation(alternatives) => {
                let mut exits = Vec::new();
                for (index, alternative_expr) in alternatives.iter().enumerate() {
                    let last = index + 1 == alternatives.len();
                    let split = (!last).then(|| self.emit(Inst::Split { alternative: 0 }));
                    self.compile(alternative_expr, flags)?;
                    if let Some(split) = split {
                        exits.push(self.emit(Inst::Jump(0)));
                        let alternative = self.pc();
                        self.instructions[split as usize] =
                            self.split(alternative, alternative_expr, flags);
                    }
                }
                let end = self.pc();
                for exit in exits {
                    self.instructions[exit as usize] = Inst::Jump(end);
                }
            }
            Expr::Capture { index, body } => {
                let capture = u32::try_from(*index).map_err(|_| Unsupported)?;
                let temporary = self.register();
                self.emit(Inst::CaptureOpen {
                    temporary,
                    restores: self.repeated > 0,
                });
                self.compile(body, flags)?;
                self.emit(Inst::CaptureClose { capture, temporary });
            }
            Expr::Repeat {
                body,
                min,
                max,
                greedy,
                captures,
            } => {
                let min = u32::try_from(*min)
                    .ok()
                    .filter(|min| *min < UNBOUNDED)
                    .ok_or(Unsupported)?;
                let max = match max {
                    None => UNBOUNDED,
                    Some(max) => u32::try_from(*max)
                        .ok()
                        .filter(|max| *max < UNBOUNDED)
                        .ok_or(Unsupported)?,
                };
                // RepeatMatcher step 1: no iteration at all.
                if max == 0 {
                    return Ok(());
                }
                if captures.is_empty() {
                    if let Some(test) = self.unit_test(body, flags)? {
                        self.emit(Inst::UnitRepeat {
                            test,
                            min,
                            max,
                            greedy: *greedy,
                        });
                        return Ok(());
                    }
                }
                let (first, count) = self.list(captures.iter().copied())?;
                // RepeatMatcher with a maximum of one: one iteration, or none, in the
                // quantifier's order; once it is taken no further iteration is tried, so no
                // counter is kept.
                if min == 0 && max == 1 {
                    let start = self.register();
                    let split = self.emit(Inst::Split { alternative: 0 });
                    let skip = (!*greedy).then(|| self.emit(Inst::Jump(0)));
                    let body_start = self.pc();
                    self.emit(Inst::LoopIterate { start });
                    // Outside repeated bodies the iteration's captures run only here, once per
                    // path, so they are still unset and need no reset.
                    if count > 0 && self.repeated > 0 {
                        self.emit(Inst::ResetCaptures { first, count });
                    }
                    self.compile(body, flags)?;
                    self.emit(Inst::EmptyCheck { start });
                    let exit = self.pc();
                    match skip {
                        // Greedy: the iteration first, the exit on backtracking.
                        None => self.instructions[split as usize] = self.split(exit, body, flags),
                        // Lazy: the exit first, the iteration on backtracking.
                        Some(skip) => {
                            self.instructions[split as usize] = Inst::Split {
                                alternative: body_start,
                            };
                            self.instructions[skip as usize] = Inst::Jump(exit);
                        }
                    }
                    return Ok(());
                }
                let (counter, start) = (self.register(), self.register());
                self.emit(Inst::LoopInit { counter });
                let head = self.emit(Inst::LoopHead {
                    counter,
                    min,
                    max,
                    greedy: *greedy,
                    exit: 0,
                });
                self.emit(Inst::LoopIterate { start });
                if count > 0 {
                    self.emit(Inst::ResetCaptures { first, count });
                }
                self.repeated += 1;
                self.compile(body, flags)?;
                self.repeated -= 1;
                self.emit(Inst::LoopTail {
                    counter,
                    start,
                    min,
                    head,
                });
                let exit = self.pc();
                self.instructions[head as usize] = Inst::LoopHead {
                    counter,
                    min,
                    max,
                    greedy: *greedy,
                    exit,
                };
            }
            Expr::Assertion(assertion) => {
                self.emit(Inst::Assert(AssertionTest {
                    assertion: *assertion,
                    multiline: flags.multiline,
                    word_ignore_case: flags.ignore_case && flags.unicode,
                }));
            }
            Expr::Lookaround { kind, body } => {
                let negative = match kind {
                    Lookaround::Ahead => false,
                    Lookaround::NegativeAhead => true,
                    Lookaround::Behind | Lookaround::NegativeBehind => return Err(Unsupported),
                };
                let look = self.emit(Inst::Look { negative, end: 0 });
                self.repeated += 1;
                self.compile(body, flags)?;
                self.repeated -= 1;
                self.emit(Inst::LookSucceed);
                let end = self.pc();
                self.instructions[look as usize] = Inst::Look { negative, end };
            }
            Expr::Backreference(reference) => {
                // Backreferences count captures from one; the registers from zero.
                let (first, count) = match reference {
                    Backreference::Index(index) => {
                        self.list(index.checked_sub(1).ok_or(Unsupported))?
                    }
                    Backreference::Named(indices) => self.list(
                        indices
                            .iter()
                            .map(|index| index.checked_sub(1).ok_or(Unsupported))
                            .collect::<Result<Vec<_>, _>>()?,
                    )?,
                };
                self.emit(Inst::Backreference {
                    first,
                    count,
                    ignore_case: flags.ignore_case,
                    unicode: flags.unicode,
                });
            }
            Expr::Mode { body, flags: local } => {
                self.compile(body, merge_flags(flags, *local))?;
            }
            Expr::Literal(_) | Expr::Dot | Expr::Class(_) => {
                unreachable!("single-unit expressions compile as unit tests")
            }
        }
        Ok(())
    }
}

/// The bit in which an ASCII letter's two cases differ.
const ASCII_CASE_BIT: u8 = 0x20;

/// One unit of a literal prefix in an ASCII subject: the bytes that are `folded` once
/// `case_bit` is set in them. A letter matched without case has `ASCII_CASE_BIT`; a unit
/// with one ASCII form has none, so only `folded` itself matches.
#[derive(Clone, Copy, PartialEq, Eq)]
struct PrefixUnit {
    folded: u8,
    case_bit: u8,
}

impl PrefixUnit {
    /// The unit whose ASCII forms are the members of `ascii`: one byte, or a letter's two
    /// cases. `None` for any other set, which no single folded byte describes.
    fn of(ascii: u128) -> Option<Self> {
        let low = ascii.trailing_zeros() as u8;
        match ascii.count_ones() {
            1 => Some(Self { folded: low, case_bit: 0 }),
            2 if ascii == Self::case_pair(low) => Some(Self { folded: low | ASCII_CASE_BIT, case_bit: ASCII_CASE_BIT }),
            _ => None,
        }
    }

    /// `low` and the unit `ASCII_CASE_BIT` above it, one bit per unit.
    fn case_pair(low: u8) -> u128 {
        1u128 << low | 1u128 << (low | ASCII_CASE_BIT)
    }

    #[inline(always)]
    fn admits(self, unit: u8) -> bool {
        unit | self.case_bit == self.folded
    }

    /// The bytes it admits, one bit per unit.
    fn ascii(self) -> u128 {
        1u128 << self.folded | 1u128 << (self.folded & !self.case_bit)
    }
}

/// The literal units every match begins with, searched for directly in an ASCII subject:
/// its bytes when they match exactly, else their folded forms (`PrefixUnit`).
enum Prefix {
    Exact(LiteralFinder),
    /// A prefix matched without case: a scan for its `anchor`th unit, a caseless one where
    /// it has one, then a check of the others.
    Folded {
        units: Box<[PrefixUnit]>,
        anchor: usize,
        scan: StartBytes,
    },
}

impl Prefix {
    fn of(units: &[PrefixUnit]) -> Option<Self> {
        if units.is_empty() {
            return None;
        }
        if units.iter().all(|unit| unit.case_bit == 0) {
            return Some(Self::Exact(literal_finder(&units.iter().map(|unit| unit.folded).collect::<Vec<_>>())));
        }
        let anchor = units.iter().position(|unit| unit.case_bit == 0).unwrap_or(0);
        Some(Self::Folded {
            units: units.into(),
            anchor,
            scan: StartBytes::of(units[anchor].ascii())?,
        })
    }

    /// The first position from `from` through `last` where `bytes` holds the prefix.
    #[inline(always)]
    fn find(&self, bytes: &[u8], from: usize, last: usize) -> Option<usize> {
        match self {
            Self::Exact(finder) => {
                // A start after `last` is no answer, so the scan stops where such a match begins.
                let end = last.saturating_add(finder.needle().len()).min(bytes.len());
                bytes.get(from..end).and_then(|window| finder.find(window)).map(|offset| from + offset)
            }
            Self::Folded { units, anchor, scan } => Self::find_folded(units, *anchor, *scan, bytes, from, last),
        }
    }

    #[inline(never)]
    fn find_folded(
        units: &[PrefixUnit],
        anchor: usize,
        scan: StartBytes,
        bytes: &[u8],
        mut from: usize,
        last: usize,
    ) -> Option<usize> {
        while from <= last {
            let window = from + anchor;
            let start = window + scan.find(bytes.get(window..)?)? - anchor;
            if start > last {
                return None;
            }
            let candidate = bytes.get(start..start + units.len())?;
            if candidate.iter().zip(units).all(|(unit, prefix)| prefix.admits(*unit)) {
                return Some(start);
            }
            from = start + 1;
        }
        None
    }
}

/// The ASCII units every match of `expr` begins with (`PrefixUnit`), whether matched
/// exactly or without case. Every unit compiles to one `Inst::Unit`, so a run whose start
/// the prefix scan found may begin after them (`Program::entry`).
fn literal_prefix(expr: &Expr, flags: Flags, prefix: &mut Vec<PrefixUnit>) -> bool {
    match expr {
        Expr::Literal(value) if !flags.ignore_case && *value < ASCII_UNITS => {
            prefix.push(PrefixUnit { folded: *value as u8, case_bit: 0 });
            true
        }
        Expr::Literal(value) if *value < ASCII_UNITS => {
            // An ASCII subject holds only the literal's ASCII forms (`Beyond::ascii_mask`).
            let forms = Beyond::Literal { value: *value, unicode: flags.unicode }.ascii_mask();
            let Some(unit) = PrefixUnit::of(forms) else {
                return false;
            };
            prefix.push(unit);
            true
        }
        Expr::Sequence(parts) => {
            for part in parts {
                if !literal_prefix(part, flags, prefix) {
                    return false;
                }
            }
            true
        }
        _ => false,
    }
}

/// Where a match can start: at the subject's start when `at_start`, at its end when
/// `at_end`, and wherever the unit there is in `units`. `only_at_start` is derived: no unit
/// and not the end admits a start, so the subject's start is the sole candidate and a search
/// need not scan for another.
struct Starts {
    at_start: bool,
    at_end: bool,
    units: UnitSet,
    only_at_start: bool,
}

/// The most start units a scan finds with `memchr`, `memchr2` or `memchr3`.
const START_SCAN_BYTES: usize = 3;
/// The most runs of consecutive ASCII start units a scan tests a block of units against at
/// once (`StartRanges`); a set of more runs is scanned unit by unit.
const START_SCAN_RANGES: usize = 4;

/// The scan of an ASCII subject for its next start unit (`Program::scan`).
#[derive(Clone, Copy)]
enum AsciiScan {
    /// No ASCII unit starts a match.
    Nothing,
    Bytes(StartBytes),
    Ranges(StartRanges),
    /// Unit by unit, through `MatchSet::table`.
    Table,
}

impl AsciiScan {
    /// The scan for the ASCII units in `ascii`.
    fn of(ascii: u128) -> Self {
        if ascii == 0 {
            return Self::Nothing;
        }
        if let Some(bytes) = StartBytes::of(ascii) {
            return Self::Bytes(bytes);
        }
        StartRanges::of(ascii).map_or(Self::Table, Self::Ranges)
    }

    /// The offset in `haystack`, an ASCII subject's units, of its first unit in the scan's
    /// set, which `admits` tests unit by unit.
    #[inline(always)]
    fn find(self, haystack: &[u8], admits: impl Fn(u8) -> bool) -> Option<usize> {
        match self {
            Self::Nothing => None,
            Self::Bytes(bytes) => bytes.find(haystack),
            Self::Ranges(ranges) => ranges.find(haystack, admits),
            Self::Table => haystack.iter().position(|unit| admits(*unit)),
        }
    }
}

/// Up to `START_SCAN_RANGES` runs `low..=high` of consecutive ASCII start units, which a scan
/// tests a block of units against with one comparison pair per run.
#[derive(Clone, Copy)]
struct StartRanges {
    low: [u8; START_SCAN_RANGES],
    high: [u8; START_SCAN_RANGES],
    count: usize,
}

/// The units a block scan tests at once: one SSE2 register.
const SCAN_BLOCK: usize = 16;

impl StartRanges {
    fn of(ascii: u128) -> Option<Self> {
        let mut ranges = Self { low: [0; START_SCAN_RANGES], high: [0; START_SCAN_RANGES], count: 0 };
        // Each step takes the lowest run of set bits, then clears it: adding the run's
        // lowest bit carries through the whole run.
        let mut rest = ascii;
        while rest != 0 {
            if ranges.count == START_SCAN_RANGES {
                return None;
            }
            let low = rest.trailing_zeros();
            let length = (rest >> low).trailing_ones();
            ranges.low[ranges.count] = low as u8;
            ranges.high[ranges.count] = (low + length - 1) as u8;
            ranges.count += 1;
            rest &= rest.wrapping_add(rest & rest.wrapping_neg());
        }
        Some(ranges)
    }

    /// The offset of `haystack`'s first unit in a range, `table` deciding the units after the
    /// last whole block.
    fn find(self, haystack: &[u8], admits: impl Fn(u8) -> bool) -> Option<usize> {
        let admits = |unit: &u8| admits(*unit);
        // A near start, as between words, is found before the block scan's set-up pays.
        let near = haystack.len().min(SCAN_BLOCK);
        if let Some(found) = haystack[..near].iter().position(admits) {
            return Some(found);
        }
        // The block scan below is x86-64-only; arm64 uses the tail scan and never
        // advances this offset.
        #[allow(unused_mut)]
        let mut offset = near;
        #[cfg(target_arch = "x86_64")]
        // SAFETY: SSE2 is part of the x86-64 baseline, and each load reads a whole block
        // inside `haystack`.
        unsafe {
            use std::arch::x86_64::{
                __m128i, _mm_cmpeq_epi8, _mm_loadu_si128, _mm_max_epu8, _mm_min_epu8, _mm_movemask_epi8,
                _mm_or_si128, _mm_set1_epi8, _mm_setzero_si128,
            };
            let bounds: [(__m128i, __m128i); START_SCAN_RANGES] = std::array::from_fn(|range| {
                (_mm_set1_epi8(self.low[range] as i8), _mm_set1_epi8(self.high[range] as i8))
            });
            while offset + SCAN_BLOCK <= haystack.len() {
                let block = _mm_loadu_si128(haystack.as_ptr().add(offset).cast());
                let mut hits = _mm_setzero_si128();
                for (low, high) in &bounds[..self.count] {
                    // A unit is in `low..=high` exactly when clamping it there leaves it.
                    let clamped = _mm_min_epu8(_mm_max_epu8(block, *low), *high);
                    hits = _mm_or_si128(hits, _mm_cmpeq_epi8(clamped, block));
                }
                let mask = _mm_movemask_epi8(hits) as u32;
                if mask != 0 {
                    return Some(offset + mask.trailing_zeros() as usize);
                }
                offset += SCAN_BLOCK;
            }
        }
        haystack[offset..].iter().position(admits).map(|found| offset + found)
    }
}

/// One to `START_SCAN_BYTES` bytes a match can start with.
#[derive(Clone, Copy)]
struct StartBytes {
    bytes: [u8; START_SCAN_BYTES],
    count: usize,
}

impl StartBytes {
    fn of(ascii: u128) -> Option<Self> {
        let count = ascii.count_ones() as usize;
        if count == 0 || count > START_SCAN_BYTES {
            return None;
        }
        let mut bytes = [0; START_SCAN_BYTES];
        for (slot, unit) in bytes.iter_mut().zip((0..ASCII_UNITS).filter(|unit| ascii >> unit & 1 != 0)) {
            *slot = unit as u8;
        }
        Some(Self { bytes, count })
    }

    /// The offset in `haystack` of its first byte among these.
    #[inline(always)]
    fn find(self, haystack: &[u8]) -> Option<usize> {
        let [first, second, third] = self.bytes;
        match self.count {
            1 => memchr::memchr(first, haystack),
            2 => memchr::memchr2(first, second, haystack),
            _ => memchr::memchr3(first, second, third, haystack),
        }
    }
}

impl Starts {
    fn new(at_start: bool, at_end: bool, units: UnitSet) -> Self {
        let only_at_start = units.is_empty() && !at_end;
        Self {
            at_start,
            at_end,
            units,
            only_at_start,
        }
    }

    /// Only the position `assertion` admits as a whole match's start, for an assertion that
    /// holds at one end of the subject alone: `^` and `$` outside multiline mode, `\A`, `\z`.
    fn of_anchor(assertion: Assertion, flags: Flags) -> Option<Self> {
        let never = || UnitSet::new(Beyond::Never);
        match assertion {
            Assertion::Start if !flags.multiline => Some(Self::new(true, false, never())),
            Assertion::StartBuffer => Some(Self::new(true, false, never())),
            Assertion::End if !flags.multiline => Some(Self::new(false, true, never())),
            Assertion::EndBuffer => Some(Self::new(false, true, never())),
            _ => None,
        }
    }

    /// No position: the starts of an expression that never consumes a unit.
    fn none() -> Self {
        Self::new(false, false, UnitSet::new(Beyond::Never))
    }

    fn union(self, other: Self) -> Self {
        Self::new(
            self.at_start || other.at_start,
            self.at_end || other.at_end,
            self.units.union(other.units),
        )
    }
}

/// `Starts` a program tests positions against: its units as a `MatchSet`.
struct MatchStarts {
    at_start: bool,
    at_end: bool,
    units: MatchSet,
}

impl From<Starts> for MatchStarts {
    fn from(starts: Starts) -> Self {
        Self {
            at_start: starts.at_start,
            at_end: starts.at_end,
            units: starts.units.into(),
        }
    }
}

impl MatchStarts {
    #[inline(always)]
    fn admits<T: Text + ?Sized>(&self, text: &T, position: usize) -> bool {
        (self.at_start && position == 0)
            || if position < text.len() {
                self.units.contains(text.unit(position))
            } else {
                self.at_end && position == text.len()
            }
    }
}

/// A superset of the positions where a match of `expr` can start, or `None` where any
/// position may start one, as for an expression that can match empty anywhere.
fn starts(expr: &Expr, flags: Flags) -> Option<Starts> {
    match expr {
        Expr::Sequence(parts) => sequence_starts(parts, flags),
        Expr::Alternation(alternatives) => {
            let mut alternatives = alternatives.iter();
            let first = starts(alternatives.next()?, flags)?;
            alternatives.try_fold(first, |all, alternative| {
                Some(all.union(starts(alternative, flags)?))
            })
        }
        Expr::Capture { body, .. } => starts(body, flags),
        Expr::Mode { body, flags: local } => starts(body, merge_flags(flags, *local)),
        Expr::Assertion(assertion) => Starts::of_anchor(*assertion, flags),
        expr if !nullable(expr) => consuming_starts(expr, flags),
        _ => None,
    }
}

/// A superset of the positions where a match of `expr` that consumes a unit can start, or
/// `None` where any position may; an expression that never consumes starts nowhere.
fn consuming_starts(expr: &Expr, flags: Flags) -> Option<Starts> {
    let units = |beyond| Some(Starts::new(false, false, UnitSet::new(beyond)));
    match expr {
        Expr::Literal(value) if !flags.ignore_case => {
            let value = *value;
            Some(Starts::new(
                false,
                false,
                UnitSet::of(
                    if value < ASCII_UNITS { 1 << value } else { 0 },
                    if value < ASCII_UNITS {
                        Beyond::Never
                    } else {
                        Beyond::Literal {
                            value,
                            unicode: flags.unicode,
                        }
                    },
                ),
            ))
        }
        Expr::Literal(value) => units(Beyond::Literal {
            value: *value,
            unicode: flags.unicode,
        }),
        Expr::Dot => units(Beyond::Dot {
            dot_all: flags.dot_all,
        }),
        Expr::Class(class) if class_is_single_unit(class) => {
            units(Beyond::class(class, flags.ignore_case, flags.unicode))
        }
        Expr::Sequence(parts) => {
            sequence_consuming_starts(parts, flags).map(|(found, _)| found.unwrap_or_else(Starts::none))
        }
        Expr::Alternation(alternatives) => alternatives.iter().try_fold(Starts::none(), |all, alternative| {
            Some(all.union(consuming_starts(alternative, flags)?))
        }),
        Expr::Capture { body, .. } | Expr::Repeat { body, .. } => consuming_starts(body, flags),
        Expr::Mode { body, flags: local } => consuming_starts(body, merge_flags(flags, *local)),
        Expr::Assertion(_) | Expr::Lookaround { .. } => Some(Starts::none()),
        _ => None,
    }
}

/// `starts` of a sequence: a match starts where its first part that consumes a unit does,
/// or where an anchor met after only empty parts holds; one whose parts can all match
/// empty without an anchor can start anywhere.
fn sequence_starts(parts: &[Expr], flags: Flags) -> Option<Starts> {
    match sequence_consuming_starts(parts, flags)? {
        (found, true) => found,
        (_, false) => None,
    }
}

/// The starts of a sequence's consuming matches: the union of each part's
/// (`consuming_starts`) up to its first part that cannot match empty, or up to an anchor
/// met after only empty parts, which adds its own position. Zero-width tests other than
/// anchors (word boundaries, lookarounds) leave the candidates to the parts that follow.
/// The flag tells whether the walk stopped there, rather than running past every part,
/// as a sequence that can match empty anywhere does.
fn sequence_consuming_starts(parts: &[Expr], flags: Flags) -> Option<(Option<Starts>, bool)> {
    let mut found: Option<Starts> = None;
    let add = |found: &mut Option<Starts>, more: Starts| {
        *found = Some(match found.take() {
            Some(all) => all.union(more),
            None => more,
        });
    };
    for part in parts {
        if let Expr::Assertion(assertion) = part {
            if let Some(anchor) = Starts::of_anchor(*assertion, flags) {
                add(&mut found, anchor);
                return Some((found, true));
            }
            continue;
        }
        if matches!(part, Expr::Lookaround { .. }) {
            continue;
        }
        add(&mut found, consuming_starts(part, flags)?);
        if !nullable(part) {
            return Some((found, true));
        }
    }
    Some((found, false))
}

/// The units a match of a pattern anchored at the subject's start can consume first: the
/// starts (`sequence_consuming_starts`) of the parts after its leading anchor, when they
/// stop at a part that must consume a unit and admit neither end of the subject.
fn anchored_first_units(expr: &Expr, flags: Flags) -> Option<UnitSet> {
    let Expr::Sequence(parts) = expr else {
        return None;
    };
    let (Expr::Assertion(anchor), rest) = parts.split_first()? else {
        return None;
    };
    if !Starts::of_anchor(*anchor, flags).is_some_and(|starts| starts.at_start) {
        return None;
    }
    match sequence_consuming_starts(rest, flags)? {
        (Some(starts), true) if !starts.at_start && !starts.at_end => Some(starts.units),
        _ => None,
    }
}

/// Whether `expr` can match the empty string.
fn nullable(expr: &Expr) -> bool {
    match expr {
        Expr::Literal(_) | Expr::Dot | Expr::Class(_) => false,
        Expr::Sequence(parts) => parts.iter().all(nullable),
        Expr::Alternation(alternatives) => alternatives.iter().any(nullable),
        Expr::Capture { body, .. } | Expr::Mode { body, .. } => nullable(body),
        Expr::Repeat { body, min, .. } => *min == 0 || nullable(body),
        Expr::Assertion(_) | Expr::Lookaround { .. } | Expr::Backreference(_) => true,
    }
}

impl Program {
    /// The program for `expr`, or `None` when it uses a construct this matcher does not run.
    pub(crate) fn compile(expr: &Expr, flags: Flags, capture_count: usize) -> Option<Self> {
        let mut compiler = Compiler {
            instructions: Vec::new(),
            sets: Vec::new(),
            lists: Vec::new(),
            guards: Vec::new(),
            registers: u32::try_from(2 * capture_count).ok()?,
            repeated: 0,
        };
        compiler.compile(expr, flags).ok()?;
        compiler.emit(Inst::Match);
        let mut prefix = Vec::new();
        let whole_literal = literal_prefix(expr, flags, &mut prefix);
        let starts = starts(expr, flags);
        let anchored = starts.as_ref().is_some_and(|starts| starts.only_at_start);
        let entry = u32::try_from(prefix.len()).ok().filter(|length| {
            !anchored
                && *length > 0
                && compiler.instructions[..prefix.len()].iter().all(|instruction| matches!(instruction, Inst::Unit(_)))
        });
        Some(Self {
            instructions: compiler.instructions.into_boxed_slice(),
            sets: compiler.sets.into_boxed_slice(),
            lists: compiler.lists.into_boxed_slice(),
            guards: compiler.guards.into_boxed_slice(),
            capture_count,
            register_count: compiler.registers as usize,
            anchored,
            anchored_first: anchored
                .then(|| anchored_first_units(expr, flags))
                .flatten()
                .map(MatchSet::from),
            scan: starts.as_ref().map_or(AsciiScan::Nothing, |starts| AsciiScan::of(starts.units.ascii)),
            starts: starts.map(MatchStarts::from),
            literal_length: (whole_literal && capture_count == 0 && !prefix.is_empty()).then_some(prefix.len()),
            prefix: Prefix::of(&prefix),
            entry,
        })
    }

    /// Whether a match can only start at the subject's start (`Starts::only_at_start`), so a
    /// search tries that one position and scans for nothing.
    /// Whether a search from `start` finds no match because of the anchor alone: an
    /// anchored pattern begins only at the subject's start, and only with one of its
    /// `anchored_first` units (`first`, the subject's unit there, `None` past its end), as
    /// `find` rejects before a run.
    pub(crate) fn anchor_excludes(&self, start: usize, first: Option<u32>) -> bool {
        self.anchored
            && (start != 0
                || self
                    .anchored_first
                    .as_ref()
                    .is_some_and(|units| first.is_none_or(|first| !units.contains(first))))
    }

    pub(crate) fn anchored_at_start(&self) -> bool {
        self.anchored
    }

    /// Whether every ASCII search starts by finding `literal` as the program's prefix
    /// (`next_start`).
    pub(crate) fn searches_prefix(&self, literal: &[u8]) -> bool {
        matches!(&self.prefix, Some(Prefix::Exact(prefix)) if prefix.needle() == literal)
    }

    /// The step budget of a search over `length` units, when a linear-time backend can
    /// answer it instead.
    pub(crate) fn budget(length: usize) -> usize {
        BUDGET_BASE_STEPS.saturating_add(length.saturating_mul(BUDGET_STEPS_PER_UNIT))
    }

    #[inline(always)]
    fn test(&self, test: UnitTest, value: u32) -> bool {
        match test {
            UnitTest::Exact(expected) => value == expected,
            UnitTest::Set(set) => self.sets[set as usize].contains(value),
        }
    }

    /// The end of the run of units from `from` to at most `to` that pass `test`. An ASCII
    /// subject tests each byte by the set's table, resolved once for the run.
    fn run_end<T: Text + ?Sized>(&self, test: UnitTest, text: &T, from: usize, to: usize) -> usize {
        let taken = match (test, text.bytes()) {
            (UnitTest::Set(set), Some(bytes)) => {
                let table = &self.sets[set as usize].table;
                bytes[from..to]
                    .iter()
                    .position(|unit| !table[usize::from(*unit & ASCII_MAX as u8)])
            }
            (UnitTest::Exact(expected), Some(bytes)) => {
                bytes[from..to].iter().position(|unit| u32::from(*unit) != expected)
            }
            (test, None) => (from..to).position(|index| !self.test(test, text.unit(index))),
        };
        taken.map_or(to, |taken| from + taken)
    }

    /// The first match at a position from `first`, trying only `first` when `sticky`.
    pub(crate) fn find<T: Text + ?Sized>(
        &self,
        text: &T,
        first: usize,
        sticky: bool,
        budget: Option<usize>,
    ) -> Result<Option<Found>, Exhausted> {
        self.search(text, first, sticky, budget, |registers| {
            registers[..2 * self.capture_count]
                .chunks_exact(2)
                .map(|bounds| (bounds[0] != UNSET).then(|| bounds[0]..bounds[1]))
                .collect()
        })
    }

    /// `find`'s match bounds alone: its captures are never collected.
    pub(crate) fn find_bounds<T: Text + ?Sized>(
        &self,
        text: &T,
        first: usize,
        sticky: bool,
        budget: Option<usize>,
    ) -> Result<Option<Range<usize>>, Exhausted> {
        Ok(self
            .search(text, first, sticky, budget, |_| Vec::new())?
            .map(|found| found.start..found.end))
    }

    /// `find`, with the captures of a match built from its registers by `captures`.
    #[inline(always)]
    fn search<T: Text + ?Sized>(
        &self,
        text: &T,
        first: usize,
        sticky: bool,
        budget: Option<usize>,
        captures: impl FnOnce(&[usize]) -> Vec<Option<Range<usize>>>,
    ) -> Result<Option<Found>, Exhausted> {
        if let (Some(length), Some(_)) = (self.literal_length, text.bytes()) {
            let last = if sticky { first } else { text.len() };
            return Ok(self.next_start(text, first, last).map(|start| Found {
                start,
                end: start + length,
                captures: Vec::new(),
            }));
        }
        let search = Search {
            last: if sticky { first } else { text.len() },
        };
        let Some(start) = self.next_start(text, first, search.last) else {
            return Ok(None);
        };
        // An anchored search has the one start, which a first unit outside the anchored
        // pattern's first units fails before a run.
        if let Some(units) = &self.anchored_first {
            if start >= text.len() || !units.contains(text.unit(start)) {
                return Ok(None);
            }
        }
        WORKSPACE.with_borrow_mut(|workspace| {
            let Workspace { registers, stack } = &mut *workspace;
            // A failed run restores every register it set, so one reset serves every start.
            registers.clear();
            registers.resize(self.register_count, UNSET);
            let mut steps = budget.unwrap_or(usize::MAX);
            stack.clear();
            stack.push(Frame::Barrier);
            let (pc, position) = self.entry(text, start);
            let at = (pc, start, position);
            let found = if budget.is_some() {
                self.run::<T, true>(text, at, Some(search), registers, stack, &mut steps)?
            } else {
                self.run::<T, false>(text, at, Some(search), registers, stack, &mut steps)?
            };
            let Some((start, end)) = found else {
                return Ok(None);
            };
            let captures = captures(registers);
            Ok(Some(Found {
                start,
                end,
                captures,
            }))
        })
    }

    /// The pc and position a run from the search start `start` (`next_start`) begins at:
    /// past the prefix the scan of an ASCII subject has matched there (`entry`).
    #[inline(always)]
    fn entry<T: Text + ?Sized>(&self, text: &T, start: usize) -> (u32, usize) {
        match (self.entry, text.bytes()) {
            (Some(pc), Some(_)) => (pc, start + pc as usize),
            _ => (0, start),
        }
    }

    /// The first position from `from` through `last` where a match can start.
    #[inline(always)]
    fn next_start<T: Text + ?Sized>(&self, text: &T, from: usize, last: usize) -> Option<usize> {
        if from > last {
            return None;
        }
        // An anchored pattern starts at the subject's start or nowhere, whatever its prefix.
        if self.anchored {
            return (from == 0).then_some(0);
        }
        if let (Some(prefix), Some(bytes)) = (&self.prefix, text.bytes()) {
            return prefix.find(bytes, from, last);
        }
        match &self.starts {
            None => Some(from),
            Some(starts) if let Some(bytes) = text.bytes() => {
                // A start right here, as in a run of matches, needs no scan; a start at the
                // subject's start is found here or first in the scan.
                if bytes.get(from).is_some_and(|unit| starts.units.table[usize::from(*unit & ASCII_MAX as u8)]) {
                    return Some(from);
                }
                // A one-position search, as a sticky one, has no further unit to scan for.
                if from == last {
                    let edge =
                        (starts.at_start && from == 0) || (starts.at_end && from == bytes.len());
                    return edge.then_some(from);
                }
                Self::scan_start_bytes(starts, self.scan, bytes, from, last)
            }
            Some(starts) => (from..=last).find(|position| starts.admits(text, *position)),
        }
    }

    /// `next_start` by a `memchr` scan of an ASCII subject for one to `START_SCAN_BYTES`
    /// start units, out of line so `run`, whose restarts call it, keeps its registers.
    #[inline(never)]
    fn scan_start_bytes(starts: &MatchStarts, scan: AsciiScan, bytes: &[u8], from: usize, last: usize) -> Option<usize> {
        // The positions admitted in order: the subject's start, a start byte, its end.
        if starts.at_start && from == 0 {
            return Some(0);
        }
        let found = bytes
            .get(from..)
            .and_then(|rest| scan.find(rest, |unit| starts.units.table[usize::from(unit & ASCII_MAX as u8)]))
            .map(|offset| from + offset)
            .or_else(|| starts.at_end.then_some(bytes.len()))?;
        (found <= last).then_some(found)
    }

    /// Runs from `pc` at `position` until a `Match` or `LookSucceed`, whose position it
    /// returns, or until backtracking reaches the run's barrier.
    #[allow(clippy::too_many_lines)]
    /// `BUDGETED` runs count each step against `steps`; an unbudgeted search counts nothing.
    fn run<T: Text + ?Sized, const BUDGETED: bool>(
        &self,
        text: &T,
        (mut pc, mut start, mut position): (u32, usize, usize),
        search: Option<Search>,
        registers: &mut [usize],
        stack: &mut Vec<Frame>,
        steps: &mut usize,
    ) -> Result<Option<(usize, usize)>, Exhausted> {
        let length = text.len();
        // The steps left, held here and written back where the run returns or recurses.
        let mut left = *steps;
        let set = |registers: &mut [usize], stack: &mut Vec<Frame>, register: u32, value| {
            let slot = &mut registers[register as usize];
            stack.push(Frame::Restore {
                register,
                value: *slot,
            });
            *slot = value;
        };
        let set_capture = |registers: &mut [usize], stack: &mut Vec<Frame>, capture: u32, start, end| {
            let bound = 2 * capture as usize;
            stack.push(Frame::RestoreCapture {
                capture,
                start: registers[bound],
                end: registers[bound + 1],
            });
            registers[bound] = start;
            registers[bound + 1] = end;
        };
        loop {
            if BUDGETED {
                left = left.checked_sub(1).ok_or(Exhausted)?;
            }
            let advanced = match self.instructions[pc as usize] {
                Inst::Unit(test) => {
                    if position < length && self.test(test, text.unit(position)) {
                        position += 1;
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::UnitRepeat {
                    test,
                    min,
                    max,
                    greedy,
                } => {
                    let ceiling = if max == UNBOUNDED {
                        length
                    } else {
                        position.saturating_add(max as usize).min(length)
                    };
                    let floor = position + min as usize;
                    if floor > length {
                        false
                    } else if greedy {
                        let end = self.run_end(test, text, position, ceiling);
                        if end < floor {
                            false
                        } else {
                            if end > floor {
                                stack.push(Frame::Greedy {
                                    pc: pc + 1,
                                    floor,
                                    position: end,
                                });
                            }
                            position = end;
                            pc += 1;
                            true
                        }
                    } else {
                        let end = self.run_end(test, text, position, floor);
                        if end < floor {
                            false
                        } else {
                            if end < ceiling {
                                stack.push(Frame::Lazy {
                                    pc: pc + 1,
                                    test,
                                    ceiling,
                                    position: end,
                                });
                            }
                            position = end;
                            pc += 1;
                            true
                        }
                    }
                }
                Inst::Assert(test) => {
                    if assertion_holds(test, text, position) {
                        pc += 1;
                        true
                    } else {
                        false
                    }
                }
                Inst::Split { alternative } => {
                    stack.push(Frame::Choice {
                        pc: alternative,
                        position,
                    });
                    pc += 1;
                    true
                }
                Inst::GuardedSplit { alternative, guard } => {
                    if self.guards[guard as usize].admits(text, position) {
                        stack.push(Frame::Choice {
                            pc: alternative,
                            position,
                        });
                        pc += 1;
                    } else {
                        pc = alternative;
                    }
                    true
                }
                Inst::Jump(target) => {
                    pc = target;
                    true
                }
                Inst::CaptureOpen { temporary, restores } => {
                    if restores {
                        set(registers, stack, temporary, position);
                    } else {
                        registers[temporary as usize] = position;
                    }
                    pc += 1;
                    true
                }
                Inst::CaptureClose { capture, temporary } => {
                    let start = registers[temporary as usize];
                    set_capture(registers, stack, capture, start, position);
                    pc += 1;
                    true
                }
                Inst::ResetCaptures { first, count } => {
                    for &capture in &self.lists[first as usize..(first + count) as usize] {
                        if registers[2 * capture as usize] != UNSET {
                            set_capture(registers, stack, capture, UNSET, UNSET);
                        }
                    }
                    pc += 1;
                    true
                }
                Inst::LoopInit { counter } => {
                    set(registers, stack, counter, 0);
                    pc += 1;
                    true
                }
                Inst::LoopHead {
                    counter,
                    min,
                    max,
                    greedy,
                    exit,
                } => {
                    let count = registers[counter as usize];
                    if count < min as usize {
                        pc += 1;
                    } else if max != UNBOUNDED && count >= max as usize {
                        pc = exit;
                    } else if greedy {
                        stack.push(Frame::Choice { pc: exit, position });
                        pc += 1;
                    } else {
                        stack.push(Frame::Choice {
                            pc: pc + 1,
                            position,
                        });
                        pc = exit;
                    }
                    true
                }
                Inst::LoopIterate { start } => {
                    set(registers, stack, start, position);
                    pc += 1;
                    true
                }
                Inst::EmptyCheck { start } => {
                    if position == registers[start as usize] {
                        false
                    } else {
                        pc += 1;
                        true
                    }
                }
                Inst::LoopTail {
                    counter,
                    start,
                    min,
                    head,
                } => {
                    let count = registers[counter as usize];
                    // RepeatMatcher's continuation: once the minimum is met, an iteration
                    // that consumed nothing fails.
                    if count >= min as usize && position == registers[start as usize] {
                        false
                    } else {
                        set(registers, stack, counter, count + 1);
                        pc = head;
                        true
                    }
                }
                Inst::Backreference {
                    first,
                    count,
                    ignore_case,
                    unicode,
                } => {
                    let captured = self.lists[first as usize..(first + count) as usize]
                        .iter()
                        .map(|capture| 2 * *capture as usize)
                        .find(|bound| registers[*bound] != UNSET)
                        .map(|bound| registers[bound]..registers[bound + 1]);
                    match captured {
                        None => {
                            pc += 1;
                            true
                        }
                        Some(range) => {
                            let width = range.end - range.start;
                            let matches = position + width <= length
                                && range.clone().zip(position..).all(|(expected, actual)| {
                                    equal(
                                        text.unit(expected),
                                        text.unit(actual),
                                        ignore_case,
                                        unicode,
                                    )
                                });
                            if matches {
                                position += width;
                                pc += 1;
                            }
                            matches
                        }
                    }
                }
                Inst::Look { negative, end } => {
                    let base = stack.len();
                    stack.push(Frame::Barrier);
                    *steps = left;
                    let found = self.run::<T, BUDGETED>(text, (pc + 1, position, position), None, registers, stack, steps)?;
                    left = *steps;
                    match (found.is_some(), negative) {
                        (true, false) => {
                            // A lookahead is atomic: its choices go, the restores of its
                            // captures stay for backtracking past it.
                            let mut kept = base;
                            for index in base + 1..stack.len() {
                                if let Frame::Restore { .. } | Frame::RestoreCapture { .. } = stack[index] {
                                    stack[kept] = stack[index];
                                    kept += 1;
                                }
                            }
                            stack.truncate(kept);
                            pc = end;
                            true
                        }
                        (true, true) => {
                            while stack.len() > base {
                                if let Some(frame) = stack.pop() {
                                    restore(registers, frame);
                                }
                            }
                            false
                        }
                        (false, false) => false,
                        (false, true) => {
                            pc = end;
                            true
                        }
                    }
                }
                Inst::LookSucceed | Inst::Match => {
                    *steps = left;
                    return Ok(Some((start, position)));
                }
            };
            if advanced {
                continue;
            }
            // Backtrack to the latest choice of this run.
            loop {
                if BUDGETED {
                    left = left.checked_sub(1).ok_or(Exhausted)?;
                }
                match stack.pop() {
                    None | Some(Frame::Barrier) => {
                        // A search moves on to its next start; a run of a body fails.
                        let next =
                            search.and_then(|search| self.next_start(text, start + 1, search.last));
                        let Some(next) = next else {
                            *steps = left;
                            return Ok(None);
                        };
                        stack.push(Frame::Barrier);
                        start = next;
                        (pc, position) = self.entry(text, next);
                        break;
                    }
                    Some(frame @ (Frame::Restore { .. } | Frame::RestoreCapture { .. })) => {
                        restore(registers, frame);
                    }
                    Some(Frame::Choice {
                        pc: resume,
                        position: at,
                    }) => {
                        pc = resume;
                        position = at;
                        break;
                    }
                    Some(Frame::Greedy {
                        pc: resume,
                        floor,
                        position: at,
                    }) => {
                        let at = at - 1;
                        if at > floor {
                            stack.push(Frame::Greedy {
                                pc: resume,
                                floor,
                                position: at,
                            });
                        }
                        pc = resume;
                        position = at;
                        break;
                    }
                    Some(Frame::Lazy {
                        pc: resume,
                        test,
                        ceiling,
                        position: at,
                    }) => {
                        if !self.test(test, text.unit(at)) {
                            continue;
                        }
                        let at = at + 1;
                        if at < ceiling {
                            stack.push(Frame::Lazy {
                                pc: resume,
                                test,
                                ceiling,
                                position: at,
                            });
                        }
                        pc = resume;
                        position = at;
                        break;
                    }
                }
            }
        }
    }
}

/// The registers a restore entry puts back; any other entry restores nothing.
fn restore(registers: &mut [usize], frame: Frame) {
    match frame {
        Frame::Restore { register, value } => registers[register as usize] = value,
        Frame::RestoreCapture { capture, start, end } => {
            let bound = 2 * capture as usize;
            registers[bound] = start;
            registers[bound + 1] = end;
        }
        _ => {}
    }
}

fn assertion_holds<T: Text + ?Sized>(test: AssertionTest, text: &T, position: usize) -> bool {
    let length = text.len();
    let terminator_at = |index: usize| index < length && is_line_terminator(text.unit(index));
    match test.assertion {
        Assertion::Start => position == 0 || (test.multiline && terminator_at(position - 1)),
        Assertion::End => position == length || (test.multiline && terminator_at(position)),
        Assertion::StartBuffer => position == 0,
        Assertion::EndBuffer => position == length,
        Assertion::EndBufferOptionalNewline => {
            position == length
                || (position + 1 == length && terminator_at(position))
                || (position + 2 == length
                    && text.unit(position) == u32::from(b'\r')
                    && text.unit(position + 1) == u32::from(b'\n'))
        }
        Assertion::Boundary | Assertion::NonBoundary => {
            let word = |index: usize| is_word_mode(text.unit(index), test.word_ignore_case);
            let left = position > 0 && word(position - 1);
            let right = position < length && word(position);
            matches!(test.assertion, Assertion::Boundary) == (left != right)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::super::{units_from_str, Flags, Regex};
    use super::{Beyond, ASCII_UNITS};

    /// A ranges test's derived ASCII mask: the units its per-unit test admits.
    #[test]
    fn ranges_ascii_mask_matches_each_unit() {
        let cases: [&[(u32, u32)]; 5] = [
            &[],
            &[(0, 0)],
            &[(0x30, 0x39), (0x41, 0x5a), (0x61, 0x7a)],
            &[(0x7f, 0x7f), (0x80, 0x10ffff)],
            &[(0x20, 0x2000)],
        ];
        for ranges in cases {
            for negative in [false, true] {
                let beyond = Beyond::Ranges {
                    ranges: ranges.into(),
                    negative,
                };
                let each = (0..ASCII_UNITS)
                    .filter(|value| beyond.matches(*value))
                    .fold(0u128, |bits, value| bits | 1 << value);
                assert_eq!(beyond.ascii_mask(), each, "{ranges:?} negative={negative}");
            }
        }
    }

    /// A case-folded literal's derived ASCII mask: the units its per-unit test admits, for
    /// literals that fold with ASCII letters (the Kelvin sign, long s) and others.
    #[test]
    fn literal_ascii_mask_matches_each_unit() {
        const LITERALS_CHECKED: u32 = 0x2200;
        for value in 0..LITERALS_CHECKED {
            for unicode in [false, true] {
                let beyond = Beyond::Literal { value, unicode };
                let each = (0..ASCII_UNITS)
                    .filter(|unit| beyond.matches(*unit))
                    .fold(0u128, |bits, unit| bits | 1 << unit);
                assert_eq!(beyond.ascii_mask(), each, "{value:#x} unicode={unicode}");
            }
        }
    }

    /// A class test's derived ASCII mask: the units its per-unit test admits, for each class
    /// kind, negation and flag set, over characters (some folding with ASCII letters),
    /// ranges, escapes and a nested class.
    #[test]
    fn class_ascii_mask_matches_each_unit() {
        use super::super::{ClassExpr, ClassItem, ClassKind};
        use oxc::regular_expression::ast::CharacterClassEscapeKind as Escape;
        let items = || {
            vec![
                ClassItem::Character(u32::from('k')),
                ClassItem::Character(0x212a),
                ClassItem::Character(0x17f),
                ClassItem::Range(u32::from('A'), u32::from('F')),
                ClassItem::Range(0x60, 0x2000),
                ClassItem::Escape(Escape::D),
                ClassItem::Escape(Escape::NegativeW),
                ClassItem::Escape(Escape::S),
                ClassItem::Nested(ClassExpr {
                    negative: true,
                    kind: ClassKind::Union,
                    items: vec![ClassItem::Range(u32::from('0'), u32::from('z'))],
                }),
            ]
        };
        for kind in [ClassKind::Union, ClassKind::Intersection, ClassKind::Subtraction] {
            for take in 0..=items().len() {
                for negative in [false, true] {
                    for ignore_case in [false, true] {
                        for unicode in [false, true] {
                            let class = ClassExpr { negative, kind, items: items().into_iter().skip(take % 3).take(take).collect() };
                            let beyond = Beyond::Class { class, ignore_case, unicode };
                            let each = (0..ASCII_UNITS)
                                .filter(|unit| beyond.matches(*unit))
                                .fold(0u128, |bits, unit| bits | 1 << unit);
                            assert_eq!(beyond.ascii_mask(), each, "take={take} negative={negative} i={ignore_case} u={unicode}");
                        }
                    }
                }
            }
        }
    }

    /// Every pattern, flag set and subject: the compiled program's match against the
    /// repository matcher's, from each start position.
    #[test]
    fn matches_the_repository_matcher() {
        let patterns = [
            r"^ba",
            r"(((\w+):\/\/)([^\/:]*)(:(\d+))?)?([^#?]*)(\?([^#]*))?(#(.*))?",
            r"^\s*|\s*$",
            r"\bQBZPbageby_cynprubyqre\b",
            r"^[\s\xa0]+|[\s\xa0]+$",
            r"(\d*)(\D*)",
            r"(^|\s)lhv\-h(\s|$)",
            r"\?[\w\W]*(sevraqvq|punaaryvq|tebhcvq)=([^\&\?#]*)",
            r"^\s*(\S*(\s+\S+)*)\s*$",
            r"(-[a-z])",
            r"qqqq|qqq|qq|q|ZZZZ|ZZZ|ZZ|Z",
            r"(a*)*b",
            r"(a|)*",
            r"(a*?)+?b",
            r"(?:a|b)*?c",
            r"(z)((a+)?(b+)?(c))*",
            r"(a)|b",
            r"(a)\1",
            r"\1(a)",
            r"(a\1)",
            r"(?<x>a)\k<x>",
            r"(?=(a+))a*b\1",
            r"(?=(a+))",
            r"(?!(a)b)\w",
            r"(.)(?!\1)",
            r"a{2,3}",
            r"(ab){0}c",
            r"(a{2,}?)(a*)",
            r"x*",
            r"[^abc]+",
            r"\B\w+",
            r"^.*$",
            r"(?:)",
            r"((a)|b)+",
            r"(a)?(b)?",
            r"[a-z]+\d{1,2}",
            r".",
            r"(?:(a)|(b))+",
            r"\s*$",
            r"a?$|b",
            r"x*\b",
            r"(?=a)b|$",
            r"(?:^|c)a",
            r"\b(?:a|$)",
            r"a*(?!b)$",
            r"(x?)\1$",
            r"[ab]*c?$",
            r"NQ_VQ",
            r"ab(c)?",
            r"pad",
            r"(a?)?b",
            r"(a)??b",
            r"(?:(a)|b)??(a)",
            r"((a)?)?c",
            r"(x?)?",
            r"(a)?\1b",
            r";\s?",
            r"[;,]x",
            r"(?:a|b|c)d",
            r"[abcd]e",
            r"^ab|c",
            r"(?:x|;)y?$",
            r"\s?;\s?",
            r"^c",
            r"a$|^b",
            r"\nx",
            r"(a)?(b)?\2",
            r"((a)|(b))?c",
            r"(?:(x)|y)?(z)?",
            r"(a)?(?=(b))",
            r"((a)?b)?\2",
            r"(?:(a)?b)?a\1",
            r"((a)?)?\2b",
            r"(?:((a)b)?c)?\2",
            r"(?=((a)?))\1b",
            r"(?:(a)|b)*?\1",
            // Start sets the scans cover: byte sets, up to four ranges, more than four
            // ranges (the table), members past ASCII, and anchored first units.
            r"[0-9a-f]+",
            r"[A-Z_][a-z]",
            r"[0-2]|[5-6]|[b-c]|[x-y]|q",
            r"[!#%&(*,.]",
            r"\s+\w",
            r"\d\s",
            r"^(?=x)a",
            r"^$",
            r"^(?:ba|q)",
            r"^\s*a",
            // Guarded splits: optional bodies and alternatives whose first units, anchors
            // or boundaries decide whether they can start at a position.
            r"(^a)?b",
            r"(?:$|a)?b",
            r"(?:x|y\b|z)?\w",
            r"(?:(?=a)\w|b)c",
            r"(a|b\1)?c",
            r"(?:a|^b|c$)+?d",
            r"(?:ab|ac|b)",
            r"(;)?\s|(x)?$",
            r"(?:(\w+):)?(\w*)",
            r"(?:[+, ]|ab)",
            // Literal prefixes: whole literals with and without case, folded prefixes with
            // and without a caseless unit, followed by captures and other parts.
            r"zfvr",
            r"aBc",
            r"a(b)c",
            r"##yv4##",
            r"k#s",
            r"kelvin",
            r"ss\w",
            r"Ab|c",
        ];
        let subjects = [
            "",
            "a",
            "b",
            "ab",
            "aab",
            "aaab",
            "abab",
            "ba123",
            "pyvpx",
            "zaacbbbcac",
            "uggc://jjj.snprobbx.pbz/ybtva.cuc?x=1#frag",
            "  pad  ",
            "\u{a0} x \u{a0}",
            "qqqq, ZZZ q, llll",
            "line\nnext\r\nend",
            "abc1 xyz22 q333",
            "aaaa",
            "AbAB",
            "-x-Y",
            "é a ë",
            "x\u{2028}y",
            "\u{1F600}a\u{1F600}",
            "lhv-h lhv-h",
            "?sevraqvq=12&q",
            "a c ",
            "xx\n",
            " b \t",
            "cab\nab",
            "a;b ; c,x;",
            "bcd ade cxy",
            "c\nx",
            "ab;",
            // Longer than one scan block, with the match past the block body and in the tail.
            "...........................Q_bar 7f;",
            "................................. 5 x",
            "zzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzzc",
            "xa..............................&",
            "\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0}\u{a0} a",
            // A split-style subject: a separator run, then ones past one scan block.
            "a ; b;c ;d , e+f..............................; g ;",
            "xb yb\nab b$a cd bd ac",
            "xxZfVr zfvrzfv aBC Abc abC",
            "##YV4## ##yv4# K#S k#\u{17F}",
            "\u{212A}elvin kElvin KELVIN \u{17F}\u{17F}x sSx",
        ];
        for flags in ["", "i", "m", "s", "u", "im", "iu"] {
            for pattern in patterns {
                let Ok(regex) = Regex::with_flags(pattern, Flags::from(flags)) else {
                    continue;
                };
                let program = regex
                    .complete()
                    .backtracking
                    .as_ref()
                    .unwrap_or_else(|| panic!("/{pattern}/{flags} compiles"));
                for subject in subjects {
                    let units = units_from_str(subject, regex.flags.unicode);
                    for first in 0..=units.len() {
                        let expected = regex
                            .find_units(&units, first)
                            .map(|found| (found.range, found.captures));
                        let actual = program
                            .find(&units[..], first, false, None)
                            .ok()
                            .expect("an unbudgeted search completes")
                            .map(|found| {
                                let found = regex.unit_match(
                                    &units,
                                    found.start,
                                    found.end,
                                    found.captures,
                                );
                                (found.range, found.captures)
                            });
                        assert_eq!(
                            actual, expected,
                            "/{pattern}/{flags} on {subject:?} from {first}"
                        );
                        let raw = |found: Option<super::Found>| {
                            found.map(|found| (found.start..found.end, found.captures))
                        };
                        if !regex.flags.unicode {
                            // Outside unicode mode a decoded unit is a code unit.
                            let code_units = subject.encode_utf16().collect::<Vec<_>>();
                            let wide = program.find(&code_units[..], first, false, None).ok();
                            let decoded = program.find(&units[..], first, false, None).ok();
                            assert_eq!(
                                wide.map(raw),
                                decoded.map(raw),
                                "/{pattern}/{flags} on code units {subject:?} from {first}"
                            );
                        }
                        if subject.is_ascii() {
                            let bytes = program
                                .find(subject.as_bytes(), first, false, None)
                                .ok()
                                .expect("an unbudgeted search completes")
                                .map(|found| (found.start..found.end, found.captures));
                            assert_eq!(
                                bytes, expected,
                                "/{pattern}/{flags} on bytes {subject:?} from {first}"
                            );
                        }
                        // A sticky search is the search's match when that starts at `first`,
                        // on every text form.
                        let at_first = |text_find: &dyn Fn(bool) -> Option<super::Found>| {
                            let sticky = raw(text_find(true));
                            let search =
                                raw(text_find(false)).filter(|(range, _)| range.start == first);
                            assert_eq!(
                                sticky, search,
                                "/{pattern}/{flags} sticky on {subject:?} at {first}"
                            );
                        };
                        let decoded = |sticky| program.find(&units[..], first, sticky, None).ok().flatten();
                        at_first(&decoded);
                        if subject.is_ascii() {
                            let bytes = subject.as_bytes();
                            at_first(&|sticky| program.find(bytes, first, sticky, None).ok().flatten());
                        }
                    }
                }
            }
        }
    }
}

//! A compiled backtracking matcher for the lowered pattern (`Expr`).
//!
//! The ECMAScript pattern semantics are a backtracking search: alternatives in order,
//! quantifiers greedy or lazy, captures reset per iteration and empty iterations rejected
//! once the minimum is met. `match_expr_with` states them as continuation-passing
//! recursion; this matcher runs the same steps as a program over registers, with an
//! explicit stack of choice points and register restores, so a match allocates nothing
//! once its workspace exists.
//!
//! A lookbehind body is compiled to run backward (the spec's `direction` of -1): its
//! sequences right to left, each unit read before the position, its captures closed at the
//! left end. Classes that can match a string of several units are not compiled; such
//! patterns keep the repository matcher.

use std::cell::RefCell;
use std::ops::Range;

mod compile;
pub mod dispatch;
pub mod machine;
pub mod matcher;
pub mod native;
mod possessive;
mod scan;
// The per-instruction semantics over a stage trait; a child module so it sees this module's private items.
#[path = "specialize.rs"]
mod specialize;
mod workspace;
use dispatch::Dispatch;
use matcher::{Admission, AssertionTest};
// The instructions and unit tests, which the stencil build mounts beside the rows.
mod insts;
pub use insts::{Assertion, Inst, UnitTest};
pub(crate) use insts::{FOLDED_WORD_UNITS, LINE_TERMINATORS, WORD_UNITS};
use native::NativeSlot;
pub use machine::CAPTURE_REGISTERS;
use specialize::{replay, step, Concrete, Cursor, Replay, Step};
pub(crate) use workspace::recycle_captures;
use workspace::{AnchoredRun, ClassRun, anchored_run_of, class_run_of};

use super::{
    canonical, canonical_preimages_of, class_is_single_unit, class_match_widths, class_max_width, class_item_matches, class_matches, equal, escape_matches, is_line_terminator, merge_flags,
    literal_finder, Backreference, ClassExpr, ClassItem, ClassKind, Expr, Flags, LiteralFinder, Lookaround,
    Unit, ASCII_MAX,
};

/// A register holding no position: an unset capture bound.
const UNSET: usize = usize::MAX;
mod exhaustion;
pub use exhaustion::take_backtrack_exhausted;
use exhaustion::{EXHAUSTED, MAXIMUM_STACK_FRAMES};
use machine::{NO_START, UNBOUNDED};

/// The code units an ASCII bitmap covers.
const ASCII_UNITS: u32 = ASCII_MAX as u32 + 1;

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

    #[inline(always)]
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

/// A set of units: exact for ASCII by bitmap, and by `beyond` for any other unit.
struct UnitSet {
    ascii: u128,
    beyond: Beyond,
}

/// The ASCII units of a mask as machine words, derived for the sets a program matches with
/// (`MatchSet`): a test of an ASCII unit loads its word and shifts it, where a shift of the
/// 128-bit mask takes several instructions. The set algebra that builds those sets keeps
/// only the mask. A flag per unit had cost 128 bytes a set, and sets were the largest part
/// of a compiled pattern (V8-v7 RegExp's 237 patterns: 123 of 312 KiB, x86_64, 2026-10-24).
#[derive(Clone, Copy)]
struct AsciiTable([u64; ASCII_TABLE_WORDS]);

/// The words of an `AsciiTable`, one bit per ASCII unit.
const ASCII_TABLE_WORDS: usize = (ASCII_UNITS / u64::BITS) as usize;

impl AsciiTable {
    /// Whether `unit`, a byte of an ASCII subject, is in the set; masked to ASCII, so the
    /// word index needs no bounds check.
    #[inline(always)]
    fn contains(&self, unit: u8) -> bool {
        let unit = u32::from(unit & ASCII_MAX as u8);
        self.0[(unit / u64::BITS) as usize] >> (unit % u64::BITS) & 1 != 0
    }
}

fn ascii_table(ascii: u128) -> AsciiTable {
    AsciiTable(std::array::from_fn(|word| (ascii >> (word as u32 * u64::BITS)) as u64))
}

/// A set a program's instructions test units against: the set with its `AsciiTable`.
struct MatchSet {
    table: AsciiTable,
    /// A byte per unit of an ASCII subject, for the scan of a repeat (`run_end`): a load
    /// and a compare a unit, where the bit table takes a shift, a load and a bit test. Only
    /// a set a `UnitRepeat` scans has one (`Program::compile`): inline in every set it made
    /// prismjs's ~2,600 sets 0.5 -> 3.7 MB (+1.2 MB max RSS).
    bytes: Option<Box<[bool; 256]>>,
    beyond: Beyond,
}

impl From<UnitSet> for MatchSet {
    fn from(set: UnitSet) -> Self {
        Self {
            table: ascii_table(set.ascii),
            bytes: None,
            beyond: set.beyond,
        }
    }
}

impl MatchSet {
    #[inline(always)]
    fn contains(&self, value: u32) -> bool {
        if value < ASCII_UNITS {
            self.table.contains(value as u8)
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
    /// Boxed: a class past ASCII that is not plain ranges is rare, and inline it had made
    /// every `Beyond`, and so every match set, 16 bytes larger.
    Class {
        class: Box<ClassExpr>,
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
                class: Box::new(class.clone()),
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
        ClassItem::Escape(escape) => escape_ascii_mask(escape, ignore_case, unicode),
        ClassItem::Range(min, max) => folded_range_ascii_mask(min, max, unicode),
        _ => (0..ASCII_UNITS)
            .filter(|value| class_item_matches(item, *value, ignore_case, unicode))
            .fold(0u128, |bits, value| bits | 1 << value),
    }
}

/// The ASCII units a case-folded range admits (`class_item_matches`), by testing each unit
/// once per range and folding: patterns repeat a few ranges (`[a-z]`, `[0-9a-f]`) across
/// all their classes, and a schema validator compiles dozens of such patterns. The memo
/// holds one entry per distinct range a run's case-insensitive patterns name.
fn folded_range_ascii_mask(min: u32, max: u32, unicode: bool) -> u128 {
    thread_local! {
        static MASKS: RefCell<std::collections::HashMap<(u32, u32, bool), u128>> = RefCell::new(Default::default());
    }
    MASKS.with(|masks| {
        *masks.borrow_mut().entry((min, max, unicode)).or_insert_with(|| {
            let range = ClassItem::Range(min, max);
            (0..ASCII_UNITS)
                .filter(|unit| class_item_matches(&range, *unit, true, unicode))
                .fold(0u128, |bits, unit| bits | 1 << unit)
        })
    })
}

/// The class escapes, in `escape_ascii_mask`'s table order.
const CLASS_ESCAPES: [oxc::regular_expression::ast::CharacterClassEscapeKind; 6] = {
    use oxc::regular_expression::ast::CharacterClassEscapeKind as Escape;
    [Escape::D, Escape::NegativeD, Escape::S, Escape::NegativeS, Escape::W, Escape::NegativeW]
};

/// One mask per flag pair, indexed by `ignore_case` then `unicode`.
type FlagMasks = [[u128; 2]; 2];

/// The ASCII units each class escape admits under each flag pair (`escape_matches`), derived
/// when the crate is compiled.
const ESCAPE_ASCII_MASKS: [FlagMasks; CLASS_ESCAPES.len()] = {
    let mut masks = [[[0u128; 2]; 2]; CLASS_ESCAPES.len()];
    let mut escape = 0;
    while escape < CLASS_ESCAPES.len() {
        let mut flags = 0;
        while flags < 4 {
            let (ignore_case, unicode) = (flags & 2 != 0, flags & 1 != 0);
            let mut unit = 0;
            while unit < ASCII_UNITS {
                if escape_matches(CLASS_ESCAPES[escape], unit, ignore_case, unicode) {
                    masks[escape][ignore_case as usize][unicode as usize] |= 1 << unit;
                }
                unit += 1;
            }
            flags += 1;
        }
        escape += 1;
    }
    masks
};

/// The ASCII units a class escape admits under the flags (`ESCAPE_ASCII_MASKS`).
fn escape_ascii_mask(escape: oxc::regular_expression::ast::CharacterClassEscapeKind, ignore_case: bool, unicode: bool) -> u128 {
    let index = CLASS_ESCAPES.iter().position(|known| *known == escape).expect("every class escape has a mask");
    ESCAPE_ASCII_MASKS[index][usize::from(ignore_case)][usize::from(unicode)]
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
    /// A greedy `UnitRepeatBack` that may give back units up to `ceiling`.
    GreedyBack {
        pc: u32,
        ceiling: usize,
        position: usize,
    },
    /// A lazy `UnitRepeatBack` that may take units down to `floor`.
    LazyBack {
        pc: u32,
        test: UnitTest,
        floor: usize,
        position: usize,
    },
    /// The bottom of one run: a search, or a lookaround body.
    Barrier,
}

/// The positions a search may start a match at, through `last`.
#[derive(Clone, Copy)]
struct Search {
    last: usize,
}

/// A match: the start and end positions and each capture's positions.
pub(crate) struct Found {
    pub(crate) start: usize,
    pub(crate) end: usize,
    pub(crate) captures: Vec<Option<Range<usize>>>,
}

pub(crate) struct Program {
    instructions: Box<[Inst]>,
    sets: Box<[MatchSet]>,
    /// Capture index lists for `ResetCaptures` and `Backreference`.
    lists: Box<[u32]>,
    /// The starts of guarded bodies, for `GuardedSplit`.
    guards: Box<[Admission<UnitTest>]>,
    dispatches: Box<[Dispatch]>,
    /// The classes of strings `Inst::ClassWidths` matches.
    classes: Box<[StringClass]>,
    capture_count: usize,
    register_count: usize,
    /// The pattern's facts about where matches start and how a search finds them, derived
    /// once in `Program::compile` and read by every search, interpreted or native.
    facts: RowFacts,
    /// The program's native search once it is hot (`native::NativeSlot`).
    native: NativeSlot,
}

/// The facts of a program's rows that the pattern fixes at compile time (its edges): where a
/// match can start, what finds the start, and the shortcut runs a pattern of one shape takes.
/// Built once from the instructions and the pattern; no search derives any of them.
struct RowFacts {
    /// Where matches can start; `None` where any position can.
    starts: Option<MatchStarts>,
    /// How an ASCII subject's scan finds the next of `starts`' units; derived from them once
    /// here, as the set algebra that builds `starts` never scans. Only their ASCII part
    /// matters there: an ASCII subject holds no other unit.
    scan: AsciiScan,
    /// The literal units every match begins with, searched for directly in ASCII subjects.
    prefix: Option<Prefix>,
    /// The ASCII units each of a match's first units can be (`Lookahead`), tested at a start
    /// the scan found in an ASCII subject before a run; `None` where only the first is known.
    lookahead: Option<Box<Lookahead>>,
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
    /// Boxed, as only anchored patterns have one and each program carries the field (with
    /// `Beyond::Class` boxed, V8-v7 RegExp's 237 patterns: 309 -> 285 KiB live, `Regex` 464 ->
    /// 400 bytes; x86_64, 2026-10-25).
    anchored_first: Option<Box<MatchSet>>,
    /// The length of `prefix` when it is the whole pattern and the pattern has no captures:
    /// an ASCII subject's match is then where the search finds it, with no run. A folded
    /// prefix holds this only in an ASCII subject, whose units have no other case forms
    /// (the Kelvin sign and long s are past ASCII); other subjects run the program.
    literal_length: Option<usize>,
    /// The unit test a pattern of one unbounded greedy `+` repeat is (`class_run_of`).
    class_run: Option<Box<ClassRun>>,
    /// The whole-subject test a pattern of one unit repeat between anchors is (`anchored_run_of`).
    anchored_run: Option<AnchoredRun>,
    /// The test of the atomic repeat a run begins with, where a failed run from a start fails
    /// from every start up to the end of the units it passes (`possessive::restart_skip`).
    restart_skip: Option<UnitTest>,
    /// The leading run's test and the byte after it (`possessive::run_then_literal`): starts
    /// are found from occurrences of the byte (`inner_literal_start`).
    inner_literal: Option<(UnitTest, u8)>,
}

/// The compiled form of a construct this matcher does not run.
struct Unsupported;

mod string_class;
use string_class::StringClass;

#[derive(Default)]
struct Compiler {
    instructions: Vec<Inst>,
    sets: Vec<MatchSet>,
    lists: Vec<u32>,
    guards: Vec<Admission<UnitTest>>,
    dispatches: Vec<Dispatch>,
    classes: Vec<StringClass>,
    registers: u32,
    /// Bodies being compiled that one path may run more than once, or whose registers a
    /// caller keeps across their run: loops with more than one iteration and lookarounds.
    /// Outside them, a group runs at most once per path.
    repeated: u32,
    /// The latest pc read as a position (`pc`), which a jump may target: an instruction there
    /// starts afresh rather than joining the one before it.
    observed: std::cell::Cell<u32>,
    /// Whether the body being compiled runs backward (a lookbehind's, outside any nested
    /// lookahead).
    backward: bool,
}

impl Compiler {
    fn emit(&mut self, instruction: Inst) -> u32 {
        self.instructions.push(instruction);
        self.instructions.len() as u32 - 1
    }

    fn pc(&self) -> u32 {
        let pc = self.instructions.len() as u32;
        self.observed.set(pc);
        pc
    }

    /// The instruction before the next one, where no position read since could target the
    /// next one, so the next one may join it.
    fn joinable_last(&mut self) -> Option<&mut Inst> {
        if self.observed.get() == self.instructions.len() as u32 {
            return None;
        }
        self.instructions.last_mut()
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
        // A guard tests the unit at the position, which a backward body reads last.
        if self.backward {
            return Inst::Split { alternative };
        }
        match starts(body, flags) {
            Some(Starts { at_start, at_end, units, .. }) => {
                let units = self.add_set(units);
                self.guards.push(Admission { at_start, at_end, units });
                Inst::GuardedSplit {
                    alternative,
                    guard: self.guards.len() as u32 - 1,
                }
            }
            None => Inst::Split { alternative },
        }
    }

    fn add_set(&mut self, set: UnitSet) -> UnitTest {
        self.sets.push(set.into());
        UnitTest::Set(self.sets.len() as u32 - 1)
    }

    fn set(&mut self, beyond: Beyond) -> UnitTest {
        self.add_set(UnitSet::new(beyond))
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
            Expr::Class(class) if class_is_single_unit(class) => self.set(Beyond::class(class, flags.ignore_case, flags.unicode)),
            _ => return Ok(None),
        }))
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

/// The haystack length below which a byte scan beats `memchr`'s vector setup.
const SHORT_SCAN_BYTES: usize = 16;

/// The offset of the first `byte` in `haystack`: a plain loop over a short haystack, whose
/// few compares cost less than `memchr`'s dispatch and vector setup, else `memchr`.
#[inline(always)]
fn find_byte(byte: u8, haystack: &[u8]) -> Option<usize> {
    if haystack.len() < SHORT_SCAN_BYTES {
        return haystack.iter().position(|&unit| unit == byte);
    }
    memchr::memchr(byte, haystack)
}

/// The most start units a scan finds with `memchr`, `memchr2` or `memchr3`.
const START_SCAN_BYTES: usize = 3;
/// The most runs of consecutive ASCII start units a scan tests a block of units against at
/// once (`StartRanges`); a set of more runs is scanned unit by unit.
const START_SCAN_RANGES: usize = 6;

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
            rest &= rest.wrapping_add(rest.isolate_lowest_one());
        }
        Some(ranges)
    }

    /// The offset of `haystack`'s first unit in a range, `table` deciding the units after the
    /// last whole block.
    fn find(self, haystack: &[u8], admits: impl Fn(u8) -> bool) -> Option<usize> {
        if haystack.len() < SCAN_BLOCK {
            return haystack.iter().position(|unit| admits(*unit));
        }
        #[cfg(target_arch = "x86_64")]
        return self.find_blocks(haystack);
        #[cfg(not(target_arch = "x86_64"))]
        haystack.iter().position(|unit| admits(*unit))
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
            1 => find_byte(first, haystack),
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
/// The most units a `Lookahead` tests.
const LOOKAHEAD_UNITS: usize = 8;

/// The scanned starts a search tests against its `Lookahead` before it gives up on the
/// scan and runs `Lookahead::find` over the rest.
const LOOKAHEAD_SCAN_MISSES: usize = 4;

/// The mask of an ASCII unit's bits.
const ASCII_MAX_UNIT: u8 = 0x7f;

/// The ASCII units each of the first `length` units of every match can be, as a bit per
/// position for each ASCII unit: `table[unit] >> position & 1` is set where `unit` may be the
/// match's unit at that position. A start whose units fail it cannot begin a match, so an
/// ASCII subject's search skips it without a run (V8's quick check reads such masks too).
struct Lookahead {
    table: [u8; ASCII_UNITS as usize],
    length: usize,
}

impl Lookahead {
    /// The lookahead of `expr`'s matches, when it tests more than the first unit, which the
    /// start scan already finds.
    fn of(expr: &Expr, flags: Flags) -> Option<Box<Self>> {
        let mut units = Vec::new();
        lookahead_units(expr, flags, &mut units)?;
        units.truncate(LOOKAHEAD_UNITS);
        if units.len() < 2 || units[1..].iter().all(|set| *set == u128::MAX) {
            return None;
        }
        let mut table = [0u8; ASCII_UNITS as usize];
        for (position, set) in units.iter().enumerate() {
            for (unit, bits) in table.iter_mut().enumerate() {
                if set >> unit & 1 != 0 {
                    *bits |= 1 << position;
                }
            }
        }
        Some(Box::new(Self { table, length: units.len() }))
    }

    /// The first start from `from` through `last` in `bytes`, an ASCII subject, that
    /// `admits` passes, by Shift-And: bit `position` of `state` is set after a unit where
    /// the units before it, through it, fit the first `position + 1` positions.
    fn find(&self, bytes: &[u8], from: usize, last: usize) -> Option<usize> {
        let accept = 1u8 << (self.length - 1);
        let end = bytes.len().min(last.saturating_add(self.length));
        let mut state = 0u8;
        for (offset, unit) in bytes.get(from..end)?.iter().enumerate() {
            state = (state << 1 | 1) & self.table[usize::from(*unit & ASCII_MAX_UNIT)];
            if state & accept != 0 {
                return Some(from + offset + 1 - self.length);
            }
        }
        None
    }

    /// Whether the units of `bytes`, an ASCII subject, from `start` can begin a match: each
    /// of the first `length` is one its position admits, and the subject holds them all.
    #[inline(always)]
    fn admits(&self, bytes: &[u8], start: usize) -> bool {
        let Some(units) = bytes.get(start..start + self.length) else {
            return false;
        };
        units.iter().enumerate().all(|(position, unit)| {
            self.table.get(usize::from(*unit)).is_some_and(|bits| bits >> position & 1 != 0)
        })
    }
}

/// Appends to `units` the ASCII units each next unit of every match of `expr` can be, one
/// set per position, for as many positions as every match is known to consume units at;
/// `None` where nothing is known of them (a non-ASCII literal under case folding, a
/// backreference). Zero-width parts are skipped: they consume nothing.
fn lookahead_units(expr: &Expr, flags: Flags, units: &mut Vec<u128>) -> Option<Known> {
    if units.len() >= LOOKAHEAD_UNITS {
        return Some(Known::Partly);
    }
    match expr {
        Expr::Literal(value) if flags.ignore_case && *value >= ASCII_UNITS => None,
        Expr::Literal(_) | Expr::Dot | Expr::Class(_) => {
            if let Expr::Class(class) = expr {
                if !class_is_single_unit(class) {
                    return Some(Known::Partly);
                }
            }
            units.push(consuming_starts(expr, flags)?.units.ascii);
            Some(Known::Whole)
        }
        Expr::Sequence(parts) => {
            for part in parts {
                if lookahead_units(part, flags, units)? == Known::Partly {
                    return Some(Known::Partly);
                }
            }
            Some(Known::Whole)
        }
        Expr::Alternation(alternatives) => {
            let start = units.len();
            let mut common: Option<Vec<u128>> = None;
            let mut whole = true;
            for alternative in alternatives {
                let mut own = units[..start].to_vec();
                whole &= lookahead_units(alternative, flags, &mut own)? == Known::Whole;
                let own = own.split_off(start);
                common = Some(match common {
                    None => own,
                    Some(mut all) => {
                        whole &= all.len() == own.len();
                        all.truncate(own.len());
                        for (set, more) in all.iter_mut().zip(own) {
                            *set |= more;
                        }
                        all
                    }
                });
            }
            units.extend(common?);
            Some(if whole { Known::Whole } else { Known::Partly })
        }
        Expr::Capture { body, .. } => lookahead_units(body, flags, units),
        Expr::Mode { body, flags: local } => lookahead_units(body, merge_flags(flags, *local), units),
        Expr::Repeat { body, min, max, .. } => {
            for _ in 0..*min {
                if lookahead_units(body, flags, units)? == Known::Partly {
                    return Some(Known::Partly);
                }
            }
            Some(if *max == Some(*min) { Known::Whole } else { Known::Partly })
        }
        Expr::Assertion(_) | Expr::Lookaround { .. } => Some(Known::Whole),
        Expr::Backreference(_) => Some(Known::Partly),
    }
}

/// Whether `lookahead_units` knew every unit of an expression's matches (a fixed number of
/// them), so the units after it follow at known positions.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Known {
    Whole,
    Partly,
}

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

/// `sets` with a byte table for each set a unit repeat scans (`Program::run_end`).
fn scanned_sets(mut sets: Vec<MatchSet>, instructions: &[Inst]) -> Box<[MatchSet]> {
    for instruction in instructions {
        if let Inst::UnitRepeat { test: UnitTest::Set(set), .. } | Inst::UnitRepeatBack { test: UnitTest::Set(set), .. } = *instruction {
            let set = &mut sets[set as usize];
            let ascii = set.table;
            set.bytes.get_or_insert_with(|| Box::new(std::array::from_fn(|unit| unit < ASCII_UNITS as usize && ascii.contains(unit as u8))));
        }
    }
    sets.into_boxed_slice()
}

impl Program {
    /// The program for `expr`, or `None` when it uses a construct this matcher does not run.
    pub(crate) fn compile(expr: &Expr, flags: Flags, capture_count: usize) -> Option<Self> {
        let mut compiler = Compiler { registers: u32::try_from(2 * capture_count).ok()?, ..Compiler::default() };
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
        // `class_run` reads the instructions before `possessive::mark` rewrites the repeats it
        // scans; the rest read them after.
        let class_run = class_run_of(&compiler.instructions, &compiler.sets, capture_count);
        possessive::mark(&mut compiler.instructions, &compiler.sets);
        let restart_skip = possessive::restart_skip(&compiler.instructions, anchored || !prefix.is_empty());
        let inner_literal = possessive::run_then_literal(&compiler.instructions, &compiler.sets, anchored || !prefix.is_empty());
        let facts = RowFacts {
            scan: starts.as_ref().map_or(AsciiScan::Nothing, |starts| AsciiScan::of(starts.units.ascii)),
            starts: starts.map(MatchStarts::from),
            prefix: Prefix::of(&prefix),
            lookahead: if prefix.is_empty() { Lookahead::of(expr, flags) } else { None },
            entry,
            anchored,
            anchored_first: anchored.then(|| anchored_first_units(expr, flags)).flatten().map(|units| Box::new(MatchSet::from(units))),
            literal_length: (whole_literal && capture_count == 0 && !prefix.is_empty()).then_some(prefix.len()),
            class_run,
            anchored_run: anchored_run_of(&compiler.instructions, capture_count),
            restart_skip,
            inner_literal,
        };
        Some(Self {
            sets: scanned_sets(std::mem::take(&mut compiler.sets), &compiler.instructions),
            native: NativeSlot::default(),
            instructions: compiler.instructions.into_boxed_slice(),
            lists: compiler.lists.into_boxed_slice(),
            guards: compiler.guards.into_boxed_slice(),
            dispatches: compiler.dispatches.into_boxed_slice(),
            classes: compiler.classes.into_boxed_slice(),
            capture_count,
            register_count: compiler.registers as usize,
            facts,
        })
    }

    /// Whether a match can only start at the subject's start (`Starts::only_at_start`), so a
    /// search tries that one position and scans for nothing.
    /// Whether a search from `start` finds no match because of the anchor alone: an
    /// anchored pattern begins only at the subject's start, and only with one of its
    /// `anchored_first` units (`first`, the subject's unit there, `None` past its end), as
    /// `find` rejects before a run.
    pub(crate) fn anchor_excludes(&self, start: usize, first: Option<u32>) -> bool {
        self.facts.anchored
            && (start != 0
                || self
                    .facts
                    .anchored_first
                    .as_ref()
                    .is_some_and(|units| first.is_none_or(|first| !units.contains(first))))
    }

    /// Whether no match starts at `start` of a subject of `length` units, whose unit there is
    /// `first` (`None` at its end): the pattern's start positions (`starts`) admit neither
    /// that unit nor an edge there, as `next_start` finds for a one-position search. Only an
    /// ASCII unit is decided, as it is one whole code point whichever units the program reads.
    pub(crate) fn start_excludes(&self, start: usize, length: usize, first: Option<u32>) -> bool {
        let Some(starts) = &self.facts.starts else {
            return false;
        };
        if starts.at_start && start == 0 {
            return false;
        }
        match first {
            Some(unit) if unit < ASCII_UNITS => !starts.units.contains(unit),
            Some(_) => false,
            None => !(starts.at_end && start == length),
        }
    }

    pub(crate) fn anchored_at_start(&self) -> bool {
        self.facts.anchored
    }

    /// Whether every ASCII search starts by finding `literal` as the program's prefix
    /// (`next_start`).
    pub(crate) fn searches_prefix(&self, literal: &[u8]) -> bool {
        matches!(&self.facts.prefix, Some(Prefix::Exact(prefix)) if prefix.needle() == literal)
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
    #[inline(always)]
    fn run_end<T: Text + ?Sized>(&self, test: UnitTest, text: &T, from: usize, to: usize) -> usize {
        let taken = match (test, text.bytes()) {
            (UnitTest::Set(set), Some(bytes)) => {
                // Every set a run scans is a `UnitRepeat`'s (its own, a class run's or an
                // anchored run's), which `Program::compile` gave a byte table.
                let table = self.sets[set as usize].bytes.as_deref();
                debug_assert!(table.is_some());
                // SAFETY: as above, a scanned set has its table.
                let table = unsafe { table.unwrap_unchecked() };
                bytes[from..to].iter().position(|unit| !table[usize::from(*unit)])
            }
            (UnitTest::Exact(expected), Some(bytes)) => {
                bytes[from..to].iter().position(|unit| u32::from(*unit) != expected)
            }
            (test, None) => (from..to).position(|index| !self.test(test, text.unit(index))),
        };
        taken.map_or(to, |taken| from + taken)
    }

    /// `run_end` read backward: the start of the run of units that pass `test` ending at
    /// `from`, going down to at most `to` (`to <= from`).
    fn run_start_back<T: Text + ?Sized>(&self, test: UnitTest, text: &T, from: usize, to: usize) -> usize {
        let kept = match (test, text.bytes()) {
            (UnitTest::Set(set), Some(bytes)) => {
                let table = self.sets[set as usize].bytes.as_deref();
                debug_assert!(table.is_some());
                // SAFETY: a scanned set has its table (`Program::compile`).
                let table = unsafe { table.unwrap_unchecked() };
                bytes[to..from].iter().rposition(|unit| !table[usize::from(*unit)])
            }
            (UnitTest::Exact(expected), Some(bytes)) => bytes[to..from].iter().rposition(|unit| u32::from(*unit) != expected),
            (test, None) => (to..from).rposition(|index| !self.test(test, text.unit(index))),
        };
        kept.map_or(to, |failed| to + failed + 1)
    }

    /// The first match at a position from `first`, trying only `first` when `sticky`, with
    /// its captures collected where `captures` asks for them.
    pub(crate) fn find<T: Text + ?Sized>(&self, text: &T, first: usize, sticky: bool, captures: bool) -> Option<Found> {
        self.search(text, first, sticky, |registers, spare| {
            if !captures || self.capture_count == 0 {
                return Vec::new();
            }
            let bounds = registers[..2 * self.capture_count]
                .as_chunks::<2>().0.iter()
                .map(|bounds| (bounds[0] != UNSET).then(|| bounds[0]..bounds[1]));
            let mut collected = std::mem::take(spare);
            collected.clear();
            collected.extend(bounds);
            collected
        })
    }

    /// `find`'s match bounds alone: its captures are never collected.
    pub(crate) fn find_bounds<T: Text + ?Sized>(&self, text: &T, first: usize, sticky: bool) -> Option<Range<usize>> {
        self.find(text, first, sticky, false).map(|found| found.start..found.end)
    }

    /// Where a run of a search from `first` starts, once the start scan has found it: `None`
    /// where no run can match (`next_start`, and an anchored pattern's first units).
    #[inline(always)]
    fn run_start<T: Text + ?Sized>(&self, text: &T, first: usize, sticky: bool) -> Option<(usize, Search)> {
        let search = Search { last: if sticky { first } else { text.len() } };
        let start = self.next_start(text, first, search.last)?;
        // An anchored search has the one start, which a first unit outside the anchored
        // pattern's first units fails before a run.
        if let Some(units) = &self.facts.anchored_first {
            if start >= text.len() || !units.contains(text.unit(start)) {
                return None;
            }
        }
        Some((start, search))
    }

    /// `find`, with the captures of a match built from its registers by `captures`.
    #[inline(always)]
    fn search<T: Text + ?Sized>(
        &self,
        text: &T,
        first: usize,
        sticky: bool,
        captures: impl FnOnce(&[usize], &mut Vec<Option<Range<usize>>>) -> Vec<Option<Range<usize>>>,
    ) -> Option<Found> {
        if let (Some(length), Some(_)) = (self.facts.literal_length, text.bytes()) {
            let last = if sticky { first } else { text.len() };
            return self.next_start(text, first, last).map(|start| Found {
                start,
                end: start + length,
                captures: Vec::new(),
            });
        }
        if let Some(found) = self.class_run_found(text, first, sticky).or_else(|| self.anchored_run_found(text, first)) {
            return found;
        }
        let (start, search) = self.run_start(text, first, sticky)?;
        self.run_in_workspace(text, start, search, captures)
    }

    /// The pc and position a run from the search start `start` (`next_start`) begins at:
    /// past the prefix the scan of an ASCII subject has matched there (`entry`).
    #[inline(always)]
    fn entry<T: Text + ?Sized>(&self, text: &T, start: usize) -> (u32, usize) {
        match (self.facts.entry, text.bytes()) {
            (Some(pc), Some(_)) => (pc, start + pc as usize),
            _ => (0, start),
        }
    }

    /// `next_start` of a program whose matches begin with a run of `run`'s units and then
    /// `literal` (`inner_literal`): the start of the run, from `from` on, before the first
    /// occurrence of the byte that has one.
    #[inline(always)]
    fn inner_literal_start(&self, bytes: &[u8], (run, literal): (UnitTest, u8), from: usize, last: usize) -> Option<usize> {
        let mut at = from;
        loop {
            let found = at + find_byte(literal, bytes.get(at..)?)?;
            let start = self.run_start_back(run, bytes, found, from);
            if start < found {
                return (start <= last).then_some(start);
            }
            at = found + 1;
        }
    }

    /// The first position from `from` through `last` where a match can start.
    /// Whether no match can start at `at` of an ASCII subject's `bytes`, by the start set's
    /// table alone: a sticky search's one start, refused before the search machinery (a
    /// tokenizer tries each token pattern at the same position, and most refuse it).
    #[inline(always)]
    pub(crate) fn refuses_start(&self, bytes: &[u8], at: usize) -> bool {
        match &self.facts.starts {
            Some(starts) if !starts.at_start && !starts.at_end => bytes.get(at).is_none_or(|unit| !starts.units.table.contains(*unit)),
            _ => false,
        }
    }

    /// The first position past a start whose run failed that a run may still match from: the
    /// next one, or past the units the run's leading atomic repeat took (`restart_skip`).
    #[inline(always)]
    fn next_candidate<T: Text + ?Sized>(&self, text: &T, failed: usize) -> usize {
        match self.facts.restart_skip {
            Some(test) => self.run_end(test, text, failed, text.len()) + 1,
            None => failed + 1,
        }
    }

    #[inline(always)]
    fn next_start<T: Text + ?Sized>(&self, text: &T, from: usize, last: usize) -> Option<usize> {
        if from > last {
            return None;
        }
        // An anchored pattern starts at the subject's start or nowhere, whatever its prefix.
        if self.facts.anchored {
            return (from == 0).then_some(0);
        }
        if let (Some(prefix), Some(bytes)) = (&self.facts.prefix, text.bytes()) {
            return prefix.find(bytes, from, last);
        }
        if let (Some((run, literal)), Some(bytes)) = (self.facts.inner_literal, text.bytes()) {
            return self.inner_literal_start(bytes, (run, literal), from, last);
        }
        if let (Some(lookahead), Some(bytes)) = (&self.facts.lookahead, text.bytes()) {
            // Each start the scan finds must also hold the units the match's next units can be.
            let mut from = from;
            for _ in 0..LOOKAHEAD_SCAN_MISSES {
                let start = self.next_scanned_start(bytes, from, last)?;
                if lookahead.admits(bytes, start) {
                    return Some(start);
                }
                from = start + 1;
                if from > last {
                    return None;
                }
            }
            // Starts the scan finds keep failing: the subject is dense in first units, so
            // one bit-parallel pass over the rest tests every position at once.
            return lookahead.find(bytes, from, last);
        }
        match &self.facts.starts {
            None => Some(from),
            Some(starts) if let Some(bytes) = text.bytes() => self.next_scanned_start(bytes, from, last),
            Some(starts) => (from..=last).find(|position| starts.admits(text, *position)),
        }
    }

    /// `next_start` in an ASCII subject's `bytes`, from the scan for `starts`' units.
    #[inline(always)]
    fn next_scanned_start(&self, bytes: &[u8], from: usize, last: usize) -> Option<usize> {
        match &self.facts.starts {
            None => Some(from),
            Some(starts) => {
                // A start right here, as in a run of matches, needs no scan; a start at the
                // subject's start is found here or first in the scan.
                if bytes.get(from).is_some_and(|unit| starts.units.table.contains(*unit)) {
                    return Some(from);
                }
                // A one-position search, as a sticky one, has no further unit to scan for.
                if from == last {
                    let edge =
                        (starts.at_start && from == 0) || (starts.at_end && from == bytes.len());
                    return edge.then_some(from);
                }
                Self::scan_start_bytes(starts, self.facts.scan, bytes, from, last)
            }
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
            .and_then(|rest| scan.find(rest, |unit| starts.units.table.contains(unit)))
            .map(|offset| from + offset)
            .or_else(|| starts.at_end.then_some(bytes.len()))?;
        (found <= last).then_some(found)
    }

    /// Runs from `pc` at `position` until a `Match` or `LookSucceed`, whose position it
    /// returns, or until backtracking reaches the run's barrier. Each instruction's meaning is
    /// `specialize::step` on the `Concrete` stage.
    fn run<T: Text + ?Sized>(
        &self,
        text: &T,
        (pc, mut start, position): (u32, usize, usize),
        search: Option<Search>,
        registers: &mut [usize],
        stack: &mut Vec<Frame>,
    ) -> Option<(usize, usize)> {
        let mut stage = Concrete { program: self, text, registers, stack };
        let mut at = Cursor::at(pc, position);
        loop {
            match step(&mut stage, self.instructions[at.pc as usize], at) {
                Step::Continue(next) => at = next,
                Step::Accept => return Some((start, at.position)),
                Step::Fail => at = self.resume(&mut stage, search, &mut start)?,
                // SAFETY: only an emitting machine branches; `Concrete::choose` returns an arm's step.
                Step::Branched => unsafe { std::hint::unreachable_unchecked() },
            }
        }
    }

    /// Backtracks to the latest choice of this run (`specialize::replay`): at the run's barrier
    /// a search moves on to its next start while a run of a body fails.
    fn resume<T: Text + ?Sized>(&self, stage: &mut Concrete<'_, T>, search: Option<Search>, start: &mut usize) -> Option<Cursor> {
        loop {
            match replay(stage) {
                Replay::Undone => {}
                Replay::Resume(at) => return Some(at),
                Replay::Exhausted => {
                    if stage.stack.capacity() >= MAXIMUM_STACK_FRAMES && EXHAUSTED.get() {
                        return None;
                    }
                    let next = search.and_then(|search| self.next_start(stage.text, self.next_candidate(stage.text, *start), search.last))?;
                    stage.stack.push(Frame::Barrier);
                    *start = next;
                    let (pc, position) = self.entry(stage.text, next);
                    return Some(Cursor::at(pc, position));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests;

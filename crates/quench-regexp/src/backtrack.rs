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
    class_is_single_unit, class_matches, equal, is_line_terminator, is_word_mode, merge_flags,
    literal_finder, Assertion, Backreference, ClassExpr, Expr, Flags, LiteralFinder, Lookaround,
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
}

impl UnitSet {
    fn union(&self, other: &Self) -> Self {
        Self {
            ascii: self.ascii | other.ascii,
            beyond: Beyond::Either(Box::new([self.beyond.clone(), other.beyond.clone()])),
        }
    }

    fn new(beyond: Beyond) -> Self {
        let ascii = (0..ASCII_UNITS)
            .filter(|value| beyond.matches(*value))
            .fold(0u128, |bits, value| bits | 1 << value);
        Self { ascii, beyond }
    }

    /// Whether no unit is in the set.
    fn is_empty(&self) -> bool {
        self.ascii == 0 && self.beyond.is_never()
    }

    #[inline(always)]
    fn contains(&self, value: u32) -> bool {
        if value < ASCII_UNITS {
            self.ascii >> value & 1 != 0
        } else {
            self.beyond.matches(value)
        }
    }
}

impl Beyond {
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
        }
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
    Jump(u32),
    CaptureOpen {
        temporary: u32,
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
    sets: Box<[UnitSet]>,
    /// Capture index lists for `ResetCaptures` and `Backreference`.
    lists: Box<[u32]>,
    capture_count: usize,
    register_count: usize,
    /// Where matches can start; `None` where any position can.
    starts: Option<Starts>,
    /// The literal bytes every match begins with, searched for directly in ASCII subjects.
    prefix: Option<LiteralFinder>,
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
    sets: Vec<UnitSet>,
    lists: Vec<u32>,
    registers: u32,
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

    fn set(&mut self, beyond: Beyond) -> UnitTest {
        self.sets.push(UnitSet::new(beyond));
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
                self.set(Beyond::Class {
                    class: class.clone(),
                    ignore_case: flags.ignore_case,
                    unicode: flags.unicode,
                })
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
                for (index, alternative) in alternatives.iter().enumerate() {
                    let last = index + 1 == alternatives.len();
                    let split = (!last).then(|| self.emit(Inst::Split { alternative: 0 }));
                    self.compile(alternative, flags)?;
                    if let Some(split) = split {
                        exits.push(self.emit(Inst::Jump(0)));
                        let alternative = self.pc();
                        self.instructions[split as usize] = Inst::Split { alternative };
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
                self.emit(Inst::CaptureOpen { temporary });
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
                    if count > 0 {
                        self.emit(Inst::ResetCaptures { first, count });
                    }
                    self.compile(body, flags)?;
                    self.emit(Inst::EmptyCheck { start });
                    let exit = self.pc();
                    match skip {
                        // Greedy: the iteration first, the exit on backtracking.
                        None => {
                            self.instructions[split as usize] = Inst::Split { alternative: exit }
                        }
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
                self.compile(body, flags)?;
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
                self.compile(body, flags)?;
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

/// The ASCII units every match of `expr` begins with, matched exactly.
fn literal_prefix(expr: &Expr, flags: Flags, prefix: &mut Vec<u8>) -> bool {
    match expr {
        Expr::Literal(value) if !flags.ignore_case && *value < ASCII_UNITS => {
            prefix.push(*value as u8);
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

/// Where a match can start: at the subject's start when `at_start`, and wherever the unit
/// there is in `units`. `only_at_start` is derived: no unit admits a start, so the subject's
/// start is the sole candidate and a search need not scan for another.
struct Starts {
    at_start: bool,
    units: UnitSet,
    only_at_start: bool,
}

impl Starts {
    fn new(at_start: bool, units: UnitSet) -> Self {
        let only_at_start = units.is_empty();
        Self {
            at_start,
            units,
            only_at_start,
        }
    }

    fn union(self, other: Self) -> Self {
        Self::new(self.at_start || other.at_start, self.units.union(&other.units))
    }

    #[inline(always)]
    fn admits<T: Text + ?Sized>(&self, text: &T, position: usize) -> bool {
        (self.at_start && position == 0)
            || (position < text.len() && self.units.contains(text.unit(position)))
    }
}

/// The starts of every match of `expr`, or `None` where any position may start one, as for
/// an expression that can match empty.
fn starts(expr: &Expr, flags: Flags) -> Option<Starts> {
    let units = |beyond| {
        Some(Starts::new(false, UnitSet::new(beyond)))
    };
    match expr {
        Expr::Literal(value) if !flags.ignore_case => {
            let value = *value;
            Some(Starts::new(false, UnitSet {
                    ascii: if value < ASCII_UNITS { 1 << value } else { 0 },
                    beyond: if value < ASCII_UNITS {
                        Beyond::Never
                    } else {
                        Beyond::Literal {
                            value,
                            unicode: flags.unicode,
                        }
                    },
                },
            ))
        }
        Expr::Literal(value) => units(Beyond::Literal {
            value: *value,
            unicode: flags.unicode,
        }),
        Expr::Dot => units(Beyond::Dot {
            dot_all: flags.dot_all,
        }),
        Expr::Class(class) if class_is_single_unit(class) => units(Beyond::Class {
            class: class.clone(),
            ignore_case: flags.ignore_case,
            unicode: flags.unicode,
        }),
        Expr::Sequence(parts) => match parts.first()? {
            // Outside multiline mode `^` holds only at the start, whatever follows.
            Expr::Assertion(Assertion::Start) if !flags.multiline => Some(Starts::new(true, UnitSet::new(Beyond::Never))),
            Expr::Assertion(Assertion::StartBuffer) => Some(Starts::new(true, UnitSet::new(Beyond::Never))),
            first => starts(first, flags),
        },
        Expr::Alternation(alternatives) => {
            let mut alternatives = alternatives.iter();
            let first = starts(alternatives.next()?, flags)?;
            alternatives.try_fold(first, |all, alternative| {
                Some(all.union(starts(alternative, flags)?))
            })
        }
        Expr::Capture { body, .. } => starts(body, flags),
        Expr::Repeat { body, min, .. } if *min > 0 => starts(body, flags),
        Expr::Assertion(Assertion::Start) if !flags.multiline => Some(Starts::new(true, UnitSet::new(Beyond::Never))),
        Expr::Mode { body, flags: local } => starts(body, merge_flags(flags, *local)),
        _ => None,
    }
}

impl Program {
    /// The program for `expr`, or `None` when it uses a construct this matcher does not run.
    pub(crate) fn compile(expr: &Expr, flags: Flags, capture_count: usize) -> Option<Self> {
        let mut compiler = Compiler {
            instructions: Vec::new(),
            sets: Vec::new(),
            lists: Vec::new(),
            registers: u32::try_from(2 * capture_count).ok()?,
        };
        compiler.compile(expr, flags).ok()?;
        compiler.emit(Inst::Match);
        Some(Self {
            instructions: compiler.instructions.into_boxed_slice(),
            sets: compiler.sets.into_boxed_slice(),
            lists: compiler.lists.into_boxed_slice(),
            capture_count,
            register_count: compiler.registers as usize,
            starts: starts(expr, flags),
            prefix: {
                let mut prefix = Vec::new();
                literal_prefix(expr, flags, &mut prefix);
                (!prefix.is_empty()).then(|| literal_finder(&prefix))
            },
        })
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

    /// The first match at a position from `first`, trying only `first` when `sticky`.
    pub(crate) fn find<T: Text + ?Sized>(
        &self,
        text: &T,
        first: usize,
        sticky: bool,
        budget: Option<usize>,
    ) -> Result<Option<Found>, Exhausted> {
        WORKSPACE.with_borrow_mut(|workspace| {
            let Workspace { registers, stack } = &mut *workspace;
            // A failed run restores every register it set, so one reset serves every start.
            registers.clear();
            registers.resize(self.register_count, UNSET);
            let mut steps = budget.unwrap_or(usize::MAX);
            let search = Search {
                last: if sticky { first } else { text.len() },
            };
            let Some(start) = self.next_start(text, first, search.last) else {
                return Ok(None);
            };
            stack.clear();
            stack.push(Frame::Barrier);
            let Some((start, end)) =
                self.run(text, 0, start, Some(search), registers, stack, &mut steps)?
            else {
                return Ok(None);
            };
            let captures = registers[..2 * self.capture_count]
                .chunks_exact(2)
                .map(|bounds| (bounds[0] != UNSET).then(|| bounds[0]..bounds[1]))
                .collect();
            Ok(Some(Found {
                start,
                end,
                captures,
            }))
        })
    }

    /// The first position from `from` through `last` where a match can start.
    #[inline(always)]
    fn next_start<T: Text + ?Sized>(&self, text: &T, from: usize, last: usize) -> Option<usize> {
        if from > last {
            return None;
        }
        if let (Some(prefix), Some(bytes)) = (&self.prefix, text.bytes()) {
            let found = from + prefix.find(&bytes[from..])?;
            return (found <= last).then_some(found);
        }
        match &self.starts {
            None => Some(from),
            Some(starts) if starts.only_at_start => (starts.at_start && from == 0).then_some(0),
            Some(starts) => (from..=last).find(|position| starts.admits(text, *position)),
        }
    }

    /// Runs from `pc` at `position` until a `Match` or `LookSucceed`, whose position it
    /// returns, or until backtracking reaches the run's barrier.
    #[allow(clippy::too_many_lines)]
    fn run<T: Text + ?Sized>(
        &self,
        text: &T,
        mut pc: u32,
        mut position: usize,
        search: Option<Search>,
        registers: &mut [usize],
        stack: &mut Vec<Frame>,
        steps: &mut usize,
    ) -> Result<Option<(usize, usize)>, Exhausted> {
        let mut start = position;
        let length = text.len();
        let set = |registers: &mut [usize], stack: &mut Vec<Frame>, register: u32, value| {
            let slot = &mut registers[register as usize];
            stack.push(Frame::Restore {
                register,
                value: *slot,
            });
            *slot = value;
        };
        loop {
            *steps = steps.checked_sub(1).ok_or(Exhausted)?;
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
                        let mut end = position;
                        while end < ceiling && self.test(test, text.unit(end)) {
                            end += 1;
                        }
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
                        let mut end = position;
                        while end < floor && self.test(test, text.unit(end)) {
                            end += 1;
                        }
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
                Inst::Jump(target) => {
                    pc = target;
                    true
                }
                Inst::CaptureOpen { temporary } => {
                    set(registers, stack, temporary, position);
                    pc += 1;
                    true
                }
                Inst::CaptureClose { capture, temporary } => {
                    let start = registers[temporary as usize];
                    set(registers, stack, 2 * capture, start);
                    set(registers, stack, 2 * capture + 1, position);
                    pc += 1;
                    true
                }
                Inst::ResetCaptures { first, count } => {
                    for &capture in &self.lists[first as usize..(first + count) as usize] {
                        if registers[2 * capture as usize] != UNSET {
                            set(registers, stack, 2 * capture, UNSET);
                            set(registers, stack, 2 * capture + 1, UNSET);
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
                    let found = self.run(text, pc + 1, position, None, registers, stack, steps)?;
                    match (found.is_some(), negative) {
                        (true, false) => {
                            // A lookahead is atomic: its choices go, the restores of its
                            // captures stay for backtracking past it.
                            let mut kept = base;
                            for index in base + 1..stack.len() {
                                if let Frame::Restore { .. } = stack[index] {
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
                                if let Some(Frame::Restore { register, value }) = stack.pop() {
                                    registers[register as usize] = value;
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
                Inst::LookSucceed | Inst::Match => return Ok(Some((start, position))),
            };
            if advanced {
                continue;
            }
            // Backtrack to the latest choice of this run.
            loop {
                *steps = steps.checked_sub(1).ok_or(Exhausted)?;
                match stack.pop() {
                    None | Some(Frame::Barrier) => {
                        // A search moves on to its next start; a run of a body fails.
                        let next =
                            search.and_then(|search| self.next_start(text, start + 1, search.last));
                        let Some(next) = next else {
                            return Ok(None);
                        };
                        stack.push(Frame::Barrier);
                        start = next;
                        pc = 0;
                        position = next;
                        break;
                    }
                    Some(Frame::Restore { register, value }) => {
                        registers[register as usize] = value;
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
            r"NQ_VQ",
            r"ab(c)?",
            r"pad",
            r"(a?)?b",
            r"(a)??b",
            r"(?:(a)|b)??(a)",
            r"((a)?)?c",
            r"(x?)?",
            r"(a)?\1b",
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
                        if !regex.flags.unicode {
                            // Outside unicode mode a decoded unit is a code unit.
                            let raw = |found: Option<super::Found>| {
                                found.map(|found| (found.start..found.end, found.captures))
                            };
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
                    }
                }
            }
        }
    }
}

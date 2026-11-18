//! The pattern's expression compiled to the VM's instructions: one method per construct,
//! each emitting the forward or the backward (lookbehind) form of its instructions.
use super::matcher::AssertionTest;
use super::{class_max_width, merge_flags, nullable, Backreference, Compiler, Expr, Flags, Inst, Lookaround, StringClass, Unsupported, NO_START, UNBOUNDED};
use crate::ClassExpr;

/// A quantifier as the VM keeps it: its bounds and greediness, and the captures each
/// iteration resets (`Program::lists[first..first + count]`).
#[derive(Clone, Copy)]
struct Quantifier {
    min: u32,
    max: u32,
    greedy: bool,
    first: u32,
    count: u32,
}

/// A bound as a `u32`: past `UNBOUNDED - 1` no subject holds that many units, so a larger
/// bound means the same as the largest one kept.
fn bound(value: usize) -> u32 {
    u32::try_from(value).unwrap_or(UNBOUNDED).min(UNBOUNDED - 1)
}

impl Compiler {
    pub(super) fn compile(&mut self, expr: &Expr, flags: Flags) -> Result<(), Unsupported> {
        if let Some(test) = self.unit_test(expr, flags)? {
            self.emit(if self.backward { Inst::UnitBack(test) } else { Inst::Unit(test) });
            return Ok(());
        }
        match expr {
            // ES2018 lookbehind: a backward sequence matches its parts right to left.
            Expr::Sequence(parts) if self.backward => parts.iter().rev().try_for_each(|part| self.compile(part, flags))?,
            Expr::Sequence(parts) => parts.iter().try_for_each(|part| self.compile(part, flags))?,
            Expr::Alternation(alternatives) => self.alternation(alternatives, flags)?,
            Expr::Capture { index, body } => self.capture(*index, body, flags)?,
            Expr::Repeat { body, min, max, greedy, captures } => {
                let (first, count) = self.list(captures.iter().copied())?;
                let max = max.map_or(UNBOUNDED, bound);
                let quantifier = Quantifier { min: bound(*min), max, greedy: *greedy, first, count };
                self.repeat(body, quantifier, flags)?;
            }
            Expr::Mode { body, flags: local } => self.compile(body, merge_flags(flags, *local))?,
            atom => self.atom(atom, flags)?,
        }
        Ok(())
    }

    /// The atoms that are not a single unit: assertions, lookarounds, backreferences and
    /// classes of strings.
    fn atom(&mut self, expr: &Expr, flags: Flags) -> Result<(), Unsupported> {
        match expr {
            Expr::Assertion(assertion) => {
                let word_ignore_case = flags.ignore_case && flags.unicode;
                self.emit(Inst::Assert(AssertionTest { assertion: *assertion, multiline: flags.multiline, word_ignore_case }));
            }
            Expr::Lookaround { kind, body } => self.lookaround(*kind, body, flags)?,
            Expr::Backreference(reference) => self.backreference(reference, flags)?,
            Expr::Class(class) => self.string_class(class, flags),
            _ => unreachable!("`compile` takes the composite expressions and single units"),
        }
        Ok(())
    }

    /// A group: its start into a temporary (joined with a group opening at the same pc), its
    /// body, then both bounds at its close.
    fn capture(&mut self, index: usize, body: &Expr, flags: Flags) -> Result<(), Unsupported> {
        let capture = u32::try_from(index).map_err(|_| Unsupported)?;
        let temporary = self.register();
        let restores = self.repeated > 0;
        match self.joinable_last() {
            Some(Inst::CaptureOpen { temporary: first, count, restores: joined }) if *first + *count == temporary && *joined == restores => *count += 1,
            _ => {
                self.emit(Inst::CaptureOpen { temporary, count: 1, restores });
            }
        }
        self.compile(body, flags)?;
        self.emit(if self.backward { Inst::CaptureCloseBack { capture, temporary } } else { Inst::CaptureClose { capture, temporary } });
        Ok(())
    }

    /// RepeatMatcher: no iteration at a maximum of zero, a unit repeat for a single-unit body
    /// without captures, an optional atom at a maximum of one, else a counted loop.
    fn repeat(&mut self, body: &Expr, quantifier: Quantifier, flags: Flags) -> Result<(), Unsupported> {
        let Quantifier { min, max, greedy, count, .. } = quantifier;
        if max == 0 {
            return Ok(());
        }
        if count == 0 {
            if let Some(test) = self.unit_test(body, flags)? {
                self.emit(if self.backward { Inst::UnitRepeatBack { test, min, max, greedy } } else { Inst::UnitRepeat { test, min, max, greedy } });
                return Ok(());
            }
        }
        if min == 0 && max == 1 {
            return self.optional(body, quantifier, flags);
        }
        self.counted(body, quantifier, flags)
    }

    /// A maximum of one: one iteration, or none, in the quantifier's order; once it is taken
    /// no further iteration is tried, so no counter is kept.
    fn optional(&mut self, body: &Expr, Quantifier { greedy, first, count, .. }: Quantifier, flags: Flags) -> Result<(), Unsupported> {
        // A body that always consumes a unit never ends where it began, so only a nullable
        // one keeps the iteration's start for the empty check.
        let start = nullable(body).then(|| self.register());
        let split = self.emit(Inst::Split { alternative: 0 });
        let skip = (!greedy).then(|| self.emit(Inst::Jump(0)));
        let body_start = self.pc();
        if let Some(start) = start {
            self.emit(Inst::LoopIterate { start });
        }
        // Outside repeated bodies the iteration's captures run only here, once per path, so
        // they are still unset and need no reset.
        if count > 0 && self.repeated > 0 {
            self.emit(Inst::ResetCaptures { first, count });
        }
        self.compile(body, flags)?;
        if let Some(start) = start {
            self.emit(Inst::EmptyCheck { start });
        }
        let exit = self.pc();
        match skip {
            // Greedy: the iteration first, the exit on backtracking.
            None => self.instructions[split as usize] = self.split(exit, body, flags),
            // Lazy: the exit first, the iteration on backtracking.
            Some(skip) => {
                self.instructions[split as usize] = Inst::Split { alternative: body_start };
                self.instructions[skip as usize] = Inst::Jump(exit);
            }
        }
        Ok(())
    }

    /// A counted loop: the head decides each iteration, the tail counts it and rejects an
    /// empty one past the minimum.
    fn counted(&mut self, body: &Expr, Quantifier { min, max, greedy, first, count }: Quantifier, flags: Flags) -> Result<(), Unsupported> {
        // A body that cannot match empty never ends an iteration where it began, so only a
        // nullable one keeps each iteration's start for the empty check.
        let counter = self.register();
        let start = if nullable(body) { self.register() } else { NO_START };
        self.emit(Inst::LoopInit { counter });
        let head = self.emit(Inst::LoopHead { counter, min, max, greedy, exit: 0 });
        if start != NO_START {
            self.emit(Inst::LoopIterate { start });
        }
        if count > 0 {
            self.emit(Inst::ResetCaptures { first, count });
        }
        self.repeated += 1;
        let compiled = self.compile(body, flags);
        self.repeated -= 1;
        compiled?;
        self.emit(Inst::LoopTail { counter, start, min, head });
        let exit = self.pc();
        self.instructions[head as usize] = Inst::LoopHead { counter, min, max, greedy, exit };
        Ok(())
    }

    /// A lookaround: its body runs nested, backward for a lookbehind.
    fn lookaround(&mut self, kind: Lookaround, body: &Expr, flags: Flags) -> Result<(), Unsupported> {
        let (negative, backward) = match kind {
            Lookaround::Ahead => (false, false),
            Lookaround::NegativeAhead => (true, false),
            Lookaround::Behind => (false, true),
            Lookaround::NegativeBehind => (true, true),
        };
        let look = self.emit(Inst::Look { negative, end: 0 });
        self.repeated += 1;
        let outer = std::mem::replace(&mut self.backward, backward);
        let compiled = self.compile(body, flags);
        self.backward = outer;
        self.repeated -= 1;
        compiled?;
        self.emit(Inst::LookSucceed);
        let end = self.pc();
        self.instructions[look as usize] = Inst::Look { negative, end };
        Ok(())
    }

    fn backreference(&mut self, reference: &Backreference, flags: Flags) -> Result<(), Unsupported> {
        // Backreferences count captures from one; the registers from zero.
        let register = |index: &usize| index.checked_sub(1).ok_or(Unsupported);
        let (first, count) = match reference {
            Backreference::Index(index) => self.list(register(index))?,
            Backreference::Named(indices) => self.list(indices.iter().map(register).collect::<Result<Vec<_>, _>>()?)?,
        };
        let (ignore_case, unicode) = (flags.ignore_case, flags.unicode);
        self.emit(if self.backward {
            Inst::BackreferenceBack { first, count, ignore_case, unicode }
        } else {
            Inst::Backreference { first, count, ignore_case, unicode }
        });
        Ok(())
    }

    /// A class that can match strings of several units.
    fn string_class(&mut self, class: &ClassExpr, flags: Flags) {
        let max_width = class_max_width(class);
        self.classes.push(StringClass { class: class.clone(), ignore_case: flags.ignore_case, unicode: flags.unicode, max_width });
        let class = self.classes.len() as u32 - 1;
        self.emit(Inst::ClassWidths { class, backward: self.backward });
    }
}

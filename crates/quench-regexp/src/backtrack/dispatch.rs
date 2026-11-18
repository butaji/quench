//! An alternation of branches that each begin with known units compiled to one lookup of the
//! subject's unit (`Inst::Dispatch`), where trying each branch's guard in turn costs a test
//! of its start units per branch at every start position.
use super::{Beyond, Compiler, Expr, Flags, Frame, Inst, Program, Starts, Text, Unsupported, ASCII_UNITS, nullable, starts};

/// The next unit after one: an interval ends where the next range's units begin.
const NEXT_UNIT: u32 = 1;
/// Consecutive bounds delimit an elementary interval.
const BOUND_PAIR: usize = 2;
/// The fewest branches a dispatch replaces: with fewer, their guards cost less than a lookup.
const DISPATCH_MIN_BRANCHES: usize = 4;
/// The most branches and start ranges of all branches a dispatch table is built for.
const DISPATCH_MAX_BRANCHES: usize = 32;
const DISPATCH_MAX_RANGES: usize = 256;

/// The units from `lo` through `hi`, whose branches are `Program::lists[first..first + count]`
/// (their entry pcs, in branch order).
pub(super) struct Interval {
    lo: u32,
    hi: u32,
    first: u32,
    count: u32,
}

/// Sorted, disjoint intervals of the units some branch can begin with.
pub(super) struct Dispatch {
    intervals: Box<[Interval]>,
}

/// The units branch `expr` can begin with as inclusive ranges, or `None` where they are not
/// known exactly enough: an anchor, an empty match, or a class that is not plain ranges.
fn branch_ranges(expr: &Expr, flags: Flags) -> Option<Vec<(u32, u32)>> {
    if nullable(expr) {
        return None;
    }
    let Starts { at_start: false, at_end: false, units, .. } = starts(expr, flags)? else {
        return None;
    };
    let mut ranges: Vec<(u32, u32)> = (0..ASCII_UNITS).filter(|unit| units.ascii >> unit & 1 != 0).map(|unit| (unit, unit)).collect();
    match &units.beyond {
        Beyond::Never => {}
        Beyond::Ranges { ranges: beyond, negative: false } => ranges.extend(beyond.iter().copied()),
        _ => return None,
    }
    Some(ranges)
}

impl Compiler {
    /// The code of an alternation: one `Dispatch` of the subject's unit where every branch's
    /// start units are known ranges, else a split before each branch but the last.
    pub(super) fn alternation(&mut self, alternatives: &[Expr], flags: Flags) -> Result<(), Unsupported> {
        let ranges = self.dispatch_ranges(alternatives, flags);
        let dispatch = ranges.as_ref().map(|_| self.emit(Inst::Dispatch { table: 0 }));
        let (mut exits, mut entries) = (Vec::new(), Vec::new());
        for (index, alternative_expr) in alternatives.iter().enumerate() {
            let last = index + 1 == alternatives.len();
            entries.push(self.pc());
            let split = (!last && dispatch.is_none()).then(|| self.emit(Inst::Split { alternative: 0 }));
            self.compile(alternative_expr, flags)?;
            if !last {
                exits.push(self.emit(Inst::Jump(0)));
                if let Some(split) = split {
                    let alternative = self.pc();
                    self.instructions[split as usize] = self.split(alternative, alternative_expr, flags);
                }
            }
        }
        let end = self.pc();
        for exit in exits {
            self.instructions[exit as usize] = Inst::Jump(end);
        }
        if let (Some(at), Some(ranges)) = (dispatch, ranges) {
            let dispatch = self.dispatch(&ranges, &entries)?;
            self.dispatches.push(dispatch);
            self.instructions[at as usize] = Inst::Dispatch { table: self.dispatches.len() as u32 - 1 };
        }
        Ok(())
    }

    fn dispatch_ranges(&self, alternatives: &[Expr], flags: Flags) -> Option<Vec<Vec<(u32, u32)>>> {
        if !(DISPATCH_MIN_BRANCHES..=DISPATCH_MAX_BRANCHES).contains(&alternatives.len()) {
            return None;
        }
        let ranges: Vec<_> = alternatives.iter().map(|alternative| branch_ranges(alternative, flags)).collect::<Option<_>>()?;
        (ranges.iter().map(Vec::len).sum::<usize>() <= DISPATCH_MAX_RANGES).then_some(ranges)
    }

    /// The table that sends each unit to the branches whose ranges hold it: the elementary
    /// intervals between the ranges' bounds, each with its branches' entries in order.
    fn dispatch(&mut self, ranges: &[Vec<(u32, u32)>], entries: &[u32]) -> Result<Dispatch, Unsupported> {
        let mut bounds: Vec<u32> = ranges.iter().flatten().flat_map(|&(lo, hi)| [lo, hi.saturating_add(NEXT_UNIT)]).collect();
        bounds.sort_unstable();
        bounds.dedup();
        let mut intervals = Vec::new();
        for pair in bounds.windows(BOUND_PAIR) {
            let (lo, hi) = (pair[0], pair[1] - NEXT_UNIT);
            let pcs: Vec<usize> = ranges
                .iter()
                .zip(entries)
                .filter(|(ranges, _)| ranges.iter().any(|&(min, max)| min <= lo && lo <= max))
                .map(|(_, &entry)| entry as usize)
                .collect();
            if pcs.is_empty() {
                continue;
            }
            let (first, count) = self.list(pcs)?;
            intervals.push(Interval { lo, hi, first, count });
        }
        Ok(Dispatch { intervals: intervals.into_boxed_slice() })
    }
}

impl Program {
    /// The pc of the first branch that can begin with the unit at `position`, the other
    /// branches that can being pushed as choices in order; `None` where none can.
    #[inline(always)]
    pub(super) fn dispatch_target<T: Text + ?Sized>(&self, table: u32, text: &T, position: usize, stack: &mut Vec<Frame>) -> Option<u32> {
        if position >= text.len() {
            return None;
        }
        let unit = text.unit(position);
        let intervals = &self.dispatches[table as usize].intervals;
        let at = intervals.partition_point(|interval| interval.hi < unit);
        let interval = intervals.get(at).filter(|interval| interval.lo <= unit)?;
        let pcs = &self.lists[interval.first as usize..(interval.first + interval.count) as usize];
        let (first, rest) = pcs.split_first()?;
        stack.extend(rest.iter().rev().map(|&pc| Frame::Choice { pc, position }));
        Some(*first)
    }
}

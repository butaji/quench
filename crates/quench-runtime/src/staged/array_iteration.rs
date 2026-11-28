//! The step semantics of the array callback methods (`forEach`, `map`, `filter`, the reducers,
//! `some`, `every` and the finds), written once over plain data: the index an iteration visits
//! next, the present element it reads, how a callback's result applies per kind, and the
//! method's completion value. The interpreter's lane (`dispatch_fast::array_steps`) decodes its
//! operands into these functions and keeps only the heap accesses and the callback call. Built
//! over `core` alone, so it compiles into the stencils too.

use crate::Value;

/// The array callback method an iteration runs, as plain data.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Kind {
    /// `Array.prototype.forEach`: its callback's results are not read.
    ForEach,
    /// `Array.prototype.map`: each result is stored at its element's index.
    Map,
    /// `Array.prototype.filter`: each element whose result is truthy is kept, in order.
    Filter,
    /// `Array.prototype.reduce`: each result becomes the accumulator.
    Reduce,
    /// `Array.prototype.reduceRight`: as `Reduce`, visiting from the end.
    ReduceRight,
    /// `Array.prototype.some`: a truthy result ends the method with `true`.
    Some,
    /// `Array.prototype.every`: a falsy result ends the method with `false`.
    Every,
    /// `Array.prototype.find`: a truthy result ends the method with the element.
    Find,
    /// `Array.prototype.findIndex`: a truthy result ends the method with the index.
    FindIndex,
    /// `Array.prototype.findLast`: as `Find`, visiting from the end.
    FindLast,
    /// `Array.prototype.findLastIndex`: as `FindIndex`, visiting from the end.
    FindLastIndex,
}

/// What a callback's result does to its iteration's result.
#[derive(Clone, Copy)]
pub enum Effect {
    /// The step leaves the result as it is.
    Nothing,
    /// Store `value` at `slot` of the result: a map's element at its index, a filter's kept
    /// element at the count kept before it.
    Store { slot: usize, value: Value },
    /// The reducer's accumulator becomes `value`.
    Accumulate(Value),
    /// The step ends the method with `value`.
    End(Value),
    /// The step ends the method with the number of `index`, a findIndex's answer.
    EndIndex(usize),
}

/// The method's answer once every element was visited without a step ending it.
#[derive(Clone, Copy)]
pub enum Answer {
    /// The method's value.
    Value(Value),
    /// `findIndex` and `findLastIndex` found no element: the answer is -1, which the vm builds as
    /// a number.
    NotFoundIndex,
}

/// Whether the kind visits its elements from the end: `findLast`, `findLastIndex` and
/// `reduceRight`.
#[inline(always)]
pub fn visits_backwards(kind: Kind) -> bool {
    matches!(kind, Kind::FindLast | Kind::FindLastIndex | Kind::ReduceRight)
}

/// Whether the kind reads every index by [[Get]], holes included, with no HasProperty first: the
/// finds (`find`, `findIndex`, `findLast`, `findLastIndex`).
#[inline(always)]
pub fn visits_holes(kind: Kind) -> bool {
    matches!(kind, Kind::Find | Kind::FindIndex | Kind::FindLast | Kind::FindLastIndex)
}

/// Whether the kind's callback passes a reducer's accumulator first.
#[inline(always)]
pub fn takes_accumulator(kind: Kind) -> bool {
    matches!(kind, Kind::Reduce | Kind::ReduceRight)
}

/// Whether a step of the kind applies its callback's result at all. A `forEach` step applies
/// nothing, so it neither reads the result nor allocates for it.
#[inline(always)]
pub fn applies_results(kind: Kind) -> bool {
    kind != Kind::ForEach
}

/// The next element an iteration visits, given `visited` elements visited of `length`: its
/// index and the visited count after it; `None` past the last element.
#[inline(always)]
pub fn next_index(kind: Kind, visited: usize, length: usize) -> Option<(usize, usize)> {
    if exhausted(visited, length) {
        return None;
    }
    let visited = visited + 1;
    let index = if visits_backwards(kind) { length - visited } else { visited - 1 };
    Some((index, visited))
}

/// Whether an iteration that has visited `visited` of `length` elements has none left.
#[inline(always)]
pub fn exhausted(visited: usize, length: usize) -> bool {
    visited == length
}

/// The element of a dense storage slot that HasProperty finds and Get reads, an own data
/// property observing nothing. `None` for a hole, which leaves the element to the generic loop.
#[inline(always)]
pub fn present(slot: Value) -> Option<Value> {
    (!slot.is_deleted()).then_some(slot)
}

/// The effect of a callback's `mapped` result for `element` at `index` (`truthy` is its
/// ToBoolean, asked only where the kind reads it). `kept` is a filter's count of kept elements,
/// advanced when it keeps one.
#[inline(always)]
pub fn apply(
    kind: Kind,
    index: usize,
    element: Value,
    mapped: Value,
    kept: &mut usize,
    truthy: impl FnOnce() -> bool,
) -> Effect {
    match kind {
        Kind::ForEach => Effect::Nothing,
        Kind::Map => Effect::Store { slot: index, value: mapped },
        Kind::Reduce | Kind::ReduceRight => Effect::Accumulate(mapped),
        Kind::Filter => {
            if !truthy() {
                return Effect::Nothing;
            }
            let slot = *kept;
            *kept += 1;
            Effect::Store { slot, value: element }
        }
        Kind::Some => if truthy() { Effect::End(Value::TRUE) } else { Effect::Nothing },
        Kind::Every => if truthy() { Effect::Nothing } else { Effect::End(Value::FALSE) },
        Kind::Find | Kind::FindLast => if truthy() { Effect::End(element) } else { Effect::Nothing },
        Kind::FindIndex | Kind::FindLastIndex => {
            if truthy() { Effect::EndIndex(index) } else { Effect::Nothing }
        }
    }
}

/// The method's value once every element was visited without a step ending it: the result for
/// the methods that make one, else the answer the method gives when no element ended it.
#[inline(always)]
pub fn completion(kind: Kind, result: Value) -> Answer {
    match kind {
        Kind::Map | Kind::Filter | Kind::Reduce | Kind::ReduceRight => Answer::Value(result),
        Kind::Some => Answer::Value(Value::FALSE),
        Kind::Every => Answer::Value(Value::TRUE),
        Kind::FindIndex | Kind::FindLastIndex => Answer::NotFoundIndex,
        Kind::ForEach | Kind::Find | Kind::FindLast => Answer::Value(Value::UNDEFINED),
    }
}

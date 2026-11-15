//! The search of a steady RegExp call over its subject, flat or a rope (`steady_search`).

use super::*;
use crate::vm::wtf16::FlatUnits;

/// `steady_search` of an ASCII rope, searched in a reused buffer of its bytes
/// (`JsString::with_ascii_rope_bytes`): the match keeps the rope and positions in it.
#[cold]
#[inline(never)]
fn steady_rope_search(
    meta: &crate::heap::RegExpMeta,
    regex: &Rc<quench_regexp::Regex>,
    input: &JsString,
    slot: usize,
    last_index: Value,
    index: f64,
    captures: bool,
) -> SteadyRead {
    let stateful = meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
    let start = if stateful { crate::units::to_length(index) } else { 0 };
    let sticky = meta.flags.contains(RegExpFlags::STICKY);
    match input.with_ascii_rope_bytes(|bytes| regex.find_ascii(bytes, start, sticky, captures)) {
        Some(Some(found)) => SteadyRead::Ascii {
            slot,
            stateful,
            matched: found.map(|found| (Rc::clone(regex), input.clone(), found)),
        },
        _ => SteadyRead::Prefiltered(slot, last_index, Prefilter::Undecided),
    }
}

/// `regexp_steady_state`'s search of the flat `input` from the regexp's `lastIndex` number `index`.
#[inline(always)]
pub(super) fn steady_search(meta: &crate::heap::RegExpMeta, input: &JsString, slot: usize, last_index: Value, index: f64, captures: bool) -> SteadyRead {
    let Some(regex) = meta.matcher.0.get() else {
        return SteadyRead::Prefiltered(slot, last_index, Prefilter::Undecided);
    };
    let Some(units) = input.existing_flat() else {
        return steady_rope_search(meta, regex, input, slot, last_index, index, captures);
    };
    let stateful = meta.flags.intersects(RegExpFlags::GLOBAL.with(RegExpFlags::STICKY));
    let start = if stateful { crate::units::to_length(index) } else { 0 };
    let sticky = meta.flags.contains(RegExpFlags::STICKY);
    if !stateful && !captures && regex.matches_at_every_start() {
        // Nothing reads the match but where it starts (test, or an exec whose result is
        // unused, and no lastIndex to store): a search from 0 matches at 0. The legacy
        // statics find the match again from there if read (`record_regexp_legacy_pending`).
        return SteadyRead::Ascii { slot, stateful, matched: Some((Rc::clone(regex), input.clone(), quench_regexp::Match::native(0..0))) };
    }
    if let crate::vm::wtf16::Units::Ascii(bytes) = units.view() {
        // A flat ASCII subject is searched in place, which makes the prefilter's tests
        // itself (`Regex::find_ascii`): the match outlives the borrows of the regexp and the
        // subject, which a result or the legacy statics keep, so only a match clones them.
        if let Some(found) = regex.find_ascii(bytes, start, sticky, captures) {
            let matched = found.map(|found| (Rc::clone(regex), input.clone(), found));
            return SteadyRead::Ascii { slot, stateful, matched };
        }
        return SteadyRead::Prefiltered(slot, last_index, Prefilter::Undecided);
    }
    SteadyRead::Prefiltered(slot, last_index, wide_prefilter(regex, units, input.len(), stateful, start, sticky))
}

/// The prefilter of a search of a subject that is not ASCII: whether `regex` excludes it at
/// `start`, which a stateful search past the end excludes too.
fn wide_prefilter(regex: &quench_regexp::Regex, units: &FlatUnits, length: usize, stateful: bool, start: usize, sticky: bool) -> Prefilter {
    let rejected = (stateful && start > length) || regex.excludes_search(length, start, sticky, || units.unit(start));
    if rejected { Prefilter::Excludes { stateful } } else { Prefilter::Admits }
}

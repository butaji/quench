//! The `SetFunctionName` rows of a function body: where the instruction's site recorded the naming
//! (`Vm::native_function_name`, the edge from the function's shape with the name slot) the row runs
//! `staged::naming`'s row (`Heap::Name`) as the fast arm, and the helper step behind it (which names
//! any other function, allocating nothing but the name's string, and records the edge) is the slow
//! arm. Nothing here is a Stable fact: the edge is an append-only shape transition and the name's
//! string a kept program atom, so the row tests the function's shape, empty name, storage and gate.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::add_plan::Transition;
use quench_runtime::staged::naming::name_form;
use quench_bjit::{padded_holes, HOLES};

/// What one `SetFunctionName` instruction runs: the site's edge, whose slot is the function's `name`
/// slot, and the word of the name's string.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Name {
    pub(crate) transition: Transition,
    pub(crate) word: u64,
}

/// A function register's frame offset travels above `NAME_FUNCTION_AT` in hole 2.
const _: () = assert!((Register::MAX as u64) * SLOT < 1 << (u64::BITS - numeric::site_bits::NAME_FUNCTION_AT));

impl Name {
    /// The heap row that runs it.
    pub(crate) fn heap(self) -> Heap {
        Heap::Name { form: name_form(self.transition.slot) }
    }

    /// The row's holes: the before and after layouts (as an add's), the name's word, and the name slot
    /// with the function register's frame offset above it (`numeric::site_bits::NAME_FUNCTION_AT`).
    pub(crate) fn holes(self, function: u64) -> [u64; HOLES] {
        use numeric::site_bits::{ADD_AFTER_AT, NAME_FUNCTION_AT};
        let [before, after] = self.transition.layouts.map(u64::from);
        padded_holes([before | after << ADD_AFTER_AT, self.word, u64::from(self.transition.slot) | function << NAME_FUNCTION_AT])
    }
}

/// The naming `instruction` runs where its site recorded one that a row may hold.
pub(crate) fn of(known: Known, instruction: WideInstruction) -> Option<Name> {
    let names = known.names?;
    if !matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Protocol {
        step: quench_runtime::staged::Protocol::Class(quench_runtime::staged::protocol_classes::ClassStep::SetFunctionName { .. }), .. }))
        || instruction.result_flags() != 0 {
        return None;
    }
    let (transition, word) = names(instruction.cache_site_index(), instruction.atom_index())?;
    Some(Name { transition, word })
}

/// The run of a naming at `pc`: the row over the function's frame slot, whose hit hops to the next
/// instruction and whose miss enters `slow`, laid out behind it, or with none leaves at `pc`
/// (`Step::Guard`). A function held outside its frame slot (a pinned register holds a Number, which is
/// no function) has the slow arm alone, or the leave.
pub(crate) fn plan(decoded: &Decoded, layout: &Layout, pc: u32, slow: Option<Planned>) -> Result<Planned, String> {
    let name = decoded.name.ok_or("name: no site")?;
    let (function, function_hole) = operand(&layout.homes, decoded.a);
    if function != Place::Mem {
        return slow.map_or_else(|| steps::leave(pc), Ok);
    }
    let row = HeapRow { heap: name.heap(), dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: false };
    let mut run = Planned::single(base_rows::stencil(row)?, name.holes(function_hole), Some(pc + 1), slow.is_none().then_some(pc));
    Ok(match slow {
        Some(slow) => {
            run.exits = vec![Exit::Ahead(1)];
            steps::behind(run, slow)
        }
        None => run,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use numeric::site_bits::{ADD_AFTER_AT, NAME_FUNCTION_AT};

    /// The holes carry what `heap_rows::name_row` unpacks: shapes, the name word, the slot and the function offset.
    #[test]
    fn the_holes_pack_the_edge_the_row_unpacks() {
        let name = Name { transition: Transition { before: 5, after: 9, slot: 1, layouts: [5, 9] }, word: 0xABCD };
        let [shapes, word, site, ..] = name.holes(24);
        assert_eq!((shapes & u64::from(u32::MAX), shapes >> ADD_AFTER_AT, word), (5, 9, 0xABCD));
        assert_eq!((site & ((1 << numeric::site_bits::ADD_SLOT_BITS) - 1), site >> NAME_FUNCTION_AT), (1, 24));
    }

    /// The name slot is an inline-or-arena slot below the inline count and an arena run past it.
    #[test]
    fn the_form_follows_the_slot() {
        let heap = |slot| Name { transition: Transition { before: 0, after: 1, slot, layouts: [0, 1] }, word: 0 }.heap();
        assert_eq!(heap(0), Heap::Name { form: quench_runtime::staged::Form::Either });
        assert_eq!(heap(u16::MAX), Heap::Name { form: quench_runtime::staged::Form::Arena });
    }
}

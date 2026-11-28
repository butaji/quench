//! The property-add rows of a function body: a `DefineField`, or a named field store whose site
//! recorded a class-keyed add, runs the class add row (`Heap::AddClass`) over the transition the site's add
//! cache holds. An add no class keys, or a store to `this`, has no row and takes the helper step (S7j4a).
//! The lowering is a fast arm: a hit continues past the instruction, a miss enters the
//! helper step behind the row (`DefineField`, which has one) or leaves at the instruction (a store,
//! which has none, `Step::Guard`).
//!
//! Facts the row relies on rather than checks: that no method cache names the atom (`Known::stores_in_place`,
//! a Stable fact the body holds, `Decoded::atom`) and, for a store, that the receiver's prototype
//! chain holds no setter or read-only name for it, proved for one prototype link (`AddSite::link`),
//! which the row tests per receiver; a layout change of a prototype on that chain retires the code
//! (`Add::chain`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::staged::add_plan::AddSite;
use quench_bjit::{padded_holes, HOLES};
#[allow(unused_imports)] use quench_runtime::staged::glue_prelude::*;

/// What one add instruction runs: the class-keyed transition of its site.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) struct Add {
    pub(crate) site: AddSite,
}

/// An object register's frame offset travels in `ADD_OBJECT_BITS` bits of hole 2.
const _: () = assert!((Register::MAX as u64) * SLOT < 1 << numeric::site_bits::ADD_OBJECT_BITS);

impl Add {
    /// The heap row that runs it: the class-keyed add in its live cell (`Heap::AddClass`).
    pub(crate) fn heap(self) -> Heap {
        Heap::AddClass
    }

    /// The chain whose prototypes' layouts the code holds for the chain proof, where the add has one.
    pub(crate) fn chain(self) -> Option<quench_runtime::code_dependencies::PrototypeChain> {
        self.site.link.map(|link| quench_runtime::code_dependencies::PrototypeChain { link, depth: self.site.depth })
    }

    /// The row's holes: the before-key and after-class of the class transition, the value operand's frame offset, and the
    /// slot with the prototype link and the object register's frame offset (`numeric::site_bits::ADD_*`).
    pub(crate) fn holes(self, object: u64, value: u64) -> [u64; HOLES] {
        use numeric::site_bits::{ADD_AFTER_AT, ADD_LINK_AT, ADD_OBJECT_AT};
        let transition = self.site.transition;
        let [_, after] = transition.layouts.map(u64::from);
        let layouts = u64::from(transition.before) | after << ADD_AFTER_AT;
        let site = u64::from(transition.slot) | u64::from(self.site.link.unwrap_or_default()) << ADD_LINK_AT | object << ADD_OBJECT_AT;
        padded_holes([layouts, value, site])
    }
}

/// Whether the add instruction defines an own field (`DefineField`) rather than stores one.
fn is_define(instruction: WideInstruction) -> bool {
    matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::Protocol {
        step: quench_runtime::staged::Protocol::Class(quench_runtime::staged::protocol_classes::ClassStep::DefineField { .. }), .. }))
}

/// The receiver register of an add instruction: a define's object, a store's named register, or
/// none for a store to `this`.
fn receiver(instruction: WideInstruction) -> Option<Register> {
    if is_define(instruction) { Some(instruction.register_b()) } else { instruction.field_store_register() }
}

/// The add `instruction` runs where its site recorded a class-keyed transition a row may hold: the atom
/// is unnamed by method caches, the receiver is no result of the frame's own completion, and the
/// storage form can take the slot.
pub(crate) fn of(known: Known, instruction: WideInstruction) -> Option<Add> {
    let adds = known.adds?;
    let defines = is_define(instruction);
    let stores = matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::SetField { .. })) && !instruction.returns_from_frame() && !instruction.writes_current_this();
    if !(defines || stores) || instruction.result_flags() != 0 {
        return None;
    }
    let atom = instruction.atom_index();
    if !known.stores_in_place(atom) { return None; }
    let site = adds(instruction.cache_site_index(), atom, !defines)?;
    // S7f DESIGN: an add no class keys has no row since the object-record add rows went (S7j4a); its helper step adds it.
    // The class row names its object register: a store to `this` has none.
    let keyed = site.transition.is_class_keyed() && site.transition.form().is_some() && receiver(instruction).is_some();
    keyed.then_some(Add { site })
}

/// The registers an add row names: the value in `a`, the receiver through `base`.
pub(crate) fn operands(instruction: WideInstruction) -> (Option<Source>, Option<Register>) {
    (Some(Source::Register(instruction.register_a())), receiver(instruction))
}

/// The run of an add at `pc`: the row over the value's and the receiver's frame slots, whose hit
/// hops to the next instruction and whose miss enters `slow`, laid out behind it, or with none
/// leaves at `pc`. A value or receiver held outside its frame slot (a pinned register holds a
/// Number, which is no receiver and takes a narrow slot only as an int) has the slow arm alone, or the leave.
pub(crate) fn plan(decoded: &Decoded, layout: &Layout, pc: u32, slow: Option<Planned>) -> Result<Planned, String> {
    let add = decoded.add.ok_or("add: no site")?;
    let (value, value_hole) = operand(&layout.homes, decoded.a);
    let (object, object_hole) = operand(&layout.homes, decoded.base.map(Source::Register));
    if value != Place::Mem || object != Place::Mem {
        return slow.map_or_else(|| steps::leave(pc), Ok);
    }
    let row = HeapRow { heap: add.heap(), dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: add.site.link.is_some() };
    let mut run = Planned::single(base_rows::stencil(row)?, add.holes(object_hole, value_hole), Some(pc + 1), slow.is_none().then_some(pc));
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
    use quench_runtime::staged::add_plan::Transition;
    use numeric::site_bits::{ADD_AFTER_AT, ADD_LINK_AT, ADD_OBJECT_AT};

    fn add(link: Option<u32>) -> Add {
        Add { site: AddSite { transition: Transition { before: 5, after: 9, slot: 2, layouts: [5, 9] }, link, depth: 3 } }
    }

    /// The holes carry what `heap_rows::add_row` unpacks: shapes, value offset, slot, link and object offset.
    #[test]
    fn the_holes_pack_the_transition_the_row_unpacks() {
        let [shapes, value, site, ..] = add(Some(0xABC)).holes(24, 16);
        assert_eq!((shapes & u64::from(u32::MAX), shapes >> ADD_AFTER_AT, value), (5, 9, 16));
        let slot_mask = (1 << numeric::site_bits::ADD_SLOT_BITS) - 1;
        let link_mask = (1 << numeric::site_bits::ADD_LINK_BITS) - 1;
        assert_eq!((site & slot_mask, (site >> ADD_LINK_AT) & link_mask, site >> ADD_OBJECT_AT), (2, 0xABC, 24));
    }

    /// Only a chained add (a store) holds a prototype-layout dependency, on the chain its proof read.
    #[test]
    fn a_chained_add_holds_its_proofs_chain_and_a_define_none() {
        assert_eq!(add(Some(1)).chain(), Some(quench_runtime::code_dependencies::PrototypeChain { link: 1, depth: 3 }));
        assert_eq!(add(None).chain(), None);
    }

    /// The class-keyed add's row is in the catalog, chained or not.
    #[test]
    fn the_class_add_row_is_a_catalog_row_chained_or_not() {
        for chained in [false, true] {
            let row = HeapRow { heap: Heap::AddClass, dst: Place::Mem, a: Place::Mem, b: Place::Mem, entry: chained };
            assert!(base_rows::stencil(row).is_ok(), "{row:?}");
        }
    }
}

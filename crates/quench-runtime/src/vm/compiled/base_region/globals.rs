//! A global name's access as rows: the by-name loads and stores and `typeof` of a name the global
//! object owns as a data property (`Vm::stable_global_slot`), and the immutable globals.
//!
//! The code holds the global object's layout (`GlobalLayoutFact`: the slot stays a data property at
//! its index until the object's shape, integrity or prototype use changes, or a lexical declaration
//! shadows the name) and the slot's observed type (`SlotDependency`), so a row names the slot by a
//! per-site byte offset and the object by its record id, and checks nothing per use. A realm proof
//! once at entry (`Heap::Realm`) says the VM runs the realm whose global object that is.
//!
//! 1. A load reads the slot live (`Heap::GlobalRead`), a Number where the slot's type says so.
//! 2. A `typeof` whose slot type spells one name is that name (`Heap::Word`).
//! 3. A store of a value its place's type fits to the slot's is one direct write (`Heap::GlobalStore`),
//!    through the store gate where the value may be a heap reference; any other store, and any
//!    access with no slot fact, is the VM's own (`Step::CallOut`) or the interpreter's.
//!
//! A name call's receiver, `undefined`, is a second definition of the row (`Decoded::also`).

use super::*;
use crate::code_dependencies::SlotDependency;
use crate::heap::GlobalLayoutFact;
use crate::typed_ir::Type;
use crate::vm::global_facts::{GlobalRead, GlobalSlot};
use quench_bjit::{padded_holes, HOLES};

/// The global object's slot a row holds: the fact the VM took (`Vm::stable_global_slot`) and the
/// record id the object is named by.
#[derive(Clone, Copy)]
pub(super) struct Access {
    pub(super) slot: GlobalSlot,
    pub(super) id: u32,
}

impl Access {
    fn of(slot: GlobalSlot) -> Option<Self> {
        Some(Self {
            id: slot.global.direct_object_index()?,
            slot,
        })
    }

    /// The byte offset of the slot's value in the arena run behind the object's extras.
    fn offset(self) -> u64 {
        u64::from(self.slot.slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64
    }

    /// Whether the slot's observed type is Numbers only, which a load of it yields.
    pub(super) fn numeric(self) -> bool {
        self.slot.ty != Type::NEVER && self.slot.ty.is_subtype(Type::NUMBER)
    }
}

/// The type of the value an operand place holds: a pinned int32, an int slot or an int32 immediate an
/// int32; a pinned double or a constant a Number; a frame slot any value.
pub(super) fn value_fact(place: Place) -> Type {
    match place {
        Place::Pin(_) | Place::MemInt | Place::Imm => Type::INT32,
        Place::PinF(_) | Place::Const => Type::NUMBER,
        Place::Mem => Type::ANY,
    }
}

/// The row of the by-name access at `pc`, where the table (`Known::globals`) holds one a row takes.
pub(super) fn global_row(known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match *known.globals?.get(&(pc as usize))? {
        GlobalRead::Constant(value) if instruction.op().is_throwing_name_load() => {
            Some(word_row(instruction, value, None))
        }
        GlobalRead::Slot(slot) => slot_row(known, instruction, Access::of(slot)?),
        _ => None,
    }
}

/// The row of an access of the global object's slot: a load reads it, a `typeof` is the name its
/// type spells, and a store writes it.
fn slot_row(known: Known, instruction: WideInstruction, access: Access) -> Option<Decoded> {
    match instruction.op().class() {
        Class::LoadName | Class::LoadNameCall => Some(Decoded {
            hole: access.offset(),
            global: Some(access),
            also: receiver(instruction),
            ..Decoded::heap(
                Heap::GlobalRead,
                Some(instruction.result_register()),
                None,
                None,
            )
        }),
        Class::LoadNameTypeof => Some(word_row(
            instruction,
            access.slot.typeof_name?,
            Some(access),
        )),
        Class::StoreName => store_row(known, instruction, access),
        _ => None,
    }
}

/// Whether `instruction` stores to a global name, which may write a field of the global object that a
/// loop's entry record holds (`Facts::Entry`).
pub(super) fn stores_name(instruction: WideInstruction) -> bool {
    instruction.op().class() == Class::StoreName
}

/// The register a name call's `undefined` receiver is defined in.
fn receiver(instruction: WideInstruction) -> Option<Register> {
    instruction
        .op()
        .is_load_name_call()
        .then(|| instruction.register_b())
}

/// The row that defines the load's result as the constant `value`, holding the slot it was folded from.
fn word_row(instruction: WideInstruction, value: Value, global: Option<Access>) -> Decoded {
    Decoded {
        hole: value.to_bits(),
        global,
        also: receiver(instruction),
        ..Decoded::heap(Heap::Word, Some(instruction.result_register()), None, None)
    }
}

/// The store of register A to the slot, in place only for an atom no method cache names (a Stable fact:
/// a cache naming it retires the code, `Decoded::atom`).
fn store_row(known: Known, instruction: WideInstruction, access: Access) -> Option<Decoded> {
    let atom = instruction.atom_index();
    known.stores_in_place(atom).then(|| {
        let value = Some(Source::Register(instruction.register_a()));
        Decoded {
            hole: access.offset(),
            global: Some(access),
            atom: Some(atom),
            ..Decoded::heap(Heap::GlobalStore, None, value, None)
        }
    })
}

/// The hole values of a global slot row, given its holes of destination and first operand: `None` for
/// any other row. The slot's byte offset is hole 2 of the slot rows, the word of a constant row hole 1.
pub(super) fn global_holes(
    decoded: &Decoded,
    heap: Heap,
    [dst_hole, a_hole, _]: [u64; 3],
) -> Option<[u64; HOLES]> {
    let id = decoded.global.map_or(0, |access| u64::from(access.id));
    Some(padded_holes(match heap {
        Heap::GlobalRead => [dst_hole, id, decoded.hole],
        Heap::GlobalStore => [id, a_hole, decoded.hole],
        Heap::Word => [dst_hole, decoded.hole, 0],
        _ => return None,
    }))
}

/// Whether a store row's operand cannot be written straight to the slot: the place's type is not the
/// slot's observed type (a wider value would widen the observation, which the VM must notice).
pub(super) fn declines(decoded: &Decoded, operand: Place) -> bool {
    decoded.heap == Some(Heap::GlobalStore)
        && decoded
            .global
            .is_some_and(|access| !value_fact(operand).is_subtype(access.slot.ty))
}

/// The row that defines a name call's receiver, `undefined`, in the frame slot of `register`.
pub(super) fn receiver_row(homes: &Homes, register: Register, pc: u32) -> Result<Planned, String> {
    let row = HeapRow {
        heap: Heap::Word,
        dst: Place::Mem,
        a: Place::Mem,
        b: Place::Mem,
        entry: false,
    };
    let slot = u64::from(homes.orig[usize::from(register)]) * SLOT;
    Ok(Planned::single(
        base_rows::stencil(row)?,
        quench_bjit::padded_holes([slot, Value::UNDEFINED.to_bits()]),
        None,
        Some(pc),
    ))
}

/// The realm the rows' slots belong to: every row of one compile names the same one.
pub(super) fn held_realm(decoded: &[Decoded]) -> Option<RealmProof> {
    decoded
        .iter()
        .find_map(|row| row.global)
        .map(|access| RealmProof {
            global: access.slot.global.to_bits(),
            offset: u64::from(access.slot.realm_offset.unsigned_abs()),
        })
}

/// The global objects whose layout the rows hold (a Stable fact): a change of one retires the code.
pub(super) fn held_layouts(decoded: &[Decoded]) -> Box<[GlobalLayoutFact]> {
    distinct(
        decoded
            .iter()
            .filter_map(|row| row.global)
            .map(|access| access.slot.layout),
    )
}

/// The slot types the rows rely on, each once: a write of another type retires the code.
pub(super) fn held_slot_types(decoded: &[Decoded]) -> Box<[SlotDependency]> {
    let mut types: Vec<SlotDependency> = decoded
        .iter()
        .filter_map(|row| row.global)
        .filter_map(|access| access.slot.dependency)
        .collect();
    types.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
    types.dedup();
    types.into()
}

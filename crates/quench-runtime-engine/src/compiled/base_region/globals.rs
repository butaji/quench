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
//! 2. A store of a value its place's type fits to the slot's is one direct write (`Heap::GlobalStore`),
//!    through the store gate where the value may be a heap reference; any other store, and any
//!    access with no slot fact, is the VM's own (`Step::CallOut`) or the interpreter's.
//!
//! A global lexical binding (a script's top-level `let`, `const` or class the declarative record holds, not
//! the global object) is the same access one level down: its word lives at an address of the record
//! (`Vm::global_lexical_slot`), which stays put until a new declaration changes the global layout (the
//! same dependency), so a load reads it live (`Heap::LexicalRead`) and a store of a binding no code holds the
//! value of writes it (`Heap::LexicalWrite`); a binding in its dead zone leaves, for the interpreter to raise.
//!
//! A name call's receiver, `undefined`, is a second definition of the row (`Decoded::also`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use quench_runtime::code_dependencies::SlotDependency;
use quench_runtime::heap::{GlobalFact, GlobalLayoutFact};
use quench_runtime::typed_ir::Type;
use quench_runtime::vm::global_facts::{GlobalRead, GlobalSlot, LexicalSlot, StableCell};
use quench_bjit::{padded_holes, HOLES};

/// The global object's slot a row holds: the fact the VM took (`Vm::stable_global_slot`) and the
/// record id the object is named by.
#[derive(Clone, Copy)]
pub(crate) struct Access {
    pub(crate) slot: GlobalSlot,
    pub(crate) id: u32,
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
        u64::from(self.slot.slot) * quench_runtime::heap::NATIVE_OBJECT_LAYOUT.property_value_size as u64
    }

    /// Whether the slot's observed type is Numbers only, which a load of it yields.
    pub(crate) fn numeric(self) -> bool {
        self.slot.ty != Type::NEVER && self.slot.ty.is_subtype(Type::NUMBER)
    }
}

/// The type of the value an operand place holds: a pinned int32, an int slot or an int32 immediate an
/// int32; a pinned double or a constant a Number; a frame slot any value.
pub(crate) fn value_fact(place: Place) -> Type {
    match place {
        Place::Pin(_) | Place::MemInt | Place::Imm => Type::INT32,
        Place::PinF(_) | Place::Const => Type::NUMBER,
        Place::Mem => Type::ANY,
    }
}

/// The row of the by-name access at `pc`, where the table (`Known::globals`) holds one a row takes.
pub(crate) fn global_row(known: Known, pc: u32, instruction: WideInstruction) -> Option<Decoded> {
    match *known.globals?.get(&(pc as usize))? {
        GlobalRead::Constant(value) if super::super::meaning_compiled::loads_name(instruction) => {
            Some(word_row(instruction, value))
        }
        GlobalRead::Slot(slot) | GlobalRead::NativeSlot { slot, .. } => slot_row(known, instruction, Access::of(slot)?),
        GlobalRead::Lexical(slot) => lexical_row(instruction, slot),
        // A held object (`lane_sites::held_objects`): its load is the constant, held by the cell's fact, with no read per use.
        GlobalRead::Stable { value, fact, global, realm_offset, .. } if instruction.op().loads_name() => Some(Decoded {
            cell: Some(StableCell { fact, global, realm_offset }),
            ..word_row(instruction, value)
        }),
        _ => None,
    }
}

/// The row of an access of the global object's slot: a load reads it (a `typeof`'s load too, whose
/// value the `Unary` after it names), and a store writes it.
fn slot_row(known: Known, instruction: WideInstruction, access: Access) -> Option<Decoded> {
    if loads_global(instruction) {
        Some(Decoded {
            hole: access.offset(),
            global: Some(access),
            also: receiver(instruction),
            ..Decoded::heap(
                Heap::GlobalRead,
                Some(instruction.result_register()),
                None,
                None,
            )
        })
    } else if stores_name(instruction) {
        store_row(known, instruction, access)
    } else {
        None
    }
}

/// The row of an access of a global lexical binding's word at its address: a load reads it, leaving where it is in its dead zone, and a store
/// writes it where it is initialized. Any other access is the VM's.
fn lexical_row(instruction: WideInstruction, slot: LexicalSlot) -> Option<Decoded> {
    let row = if named_load(instruction) {
        Decoded { also: receiver(instruction), ..Decoded::heap(Heap::LexicalRead, Some(instruction.result_register()), None, None) }
    } else if stores_name(instruction) {
        Decoded::heap(Heap::LexicalWrite, None, Some(Source::Register(instruction.register_a())), None)
    } else {
        return None;
    };
    Some(Decoded { hole: slot.address as u64, lexical: Some(slot), ..row })
}

/// Whether `instruction` stores to a global name, which may write a field of the global object that a
/// loop's entry record holds (`Facts::Entry`), or to a script-top-level lexical, which writes the realm's record.
pub(crate) fn stores_name(instruction: WideInstruction) -> bool {
    crate::compiled::meaning_compiled::stores_name(instruction)
        || matches!(quench_runtime::staged::Operation::<(), ()>::decode(instruction, quench_runtime::staged::DecodeContext::EMPTY),
            Some(quench_runtime::staged::Operation::GlobalLexicalStore { .. }))
}

/// Whether `instruction` is a by-name read, `LoadName` or a call's `LoadNameCall`: the decode names both
/// (`Operation::decode_load_name`), whose proof decides nothing about which opcode it is.
fn named_load(instruction: WideInstruction) -> bool {
    quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()).is_some()
}

/// Whether `instruction` loads a global name's value: a by-name read (`named_load`), or the load a
/// `typeof` reads (`Operation::GlobalTypeof`).
fn loads_global(instruction: WideInstruction) -> bool {
    named_load(instruction)
        || matches!(crate::compiled::decoded(instruction), Some(quench_runtime::staged::Operation::GlobalTypeof { .. }))
}

/// The register a name call's `undefined` receiver is defined in.
fn receiver(instruction: WideInstruction) -> Option<Register> {
    matches!(quench_runtime::staged::Operation::<(), ()>::decode_load_name(instruction, ()),
        Some(quench_runtime::staged::Operation::LoadName { receiver: Some(_), .. })).then(|| instruction.register_b())
}

/// The row that defines the load's result as the constant `value`, an immutable global's.
fn word_row(instruction: WideInstruction, value: Value) -> Decoded {
    Decoded {
        hole: value.to_bits(),
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
pub(crate) fn global_holes(
    decoded: &Decoded,
    heap: Heap,
    [dst_hole, a_hole, _]: [u64; 3],
) -> Option<[u64; HOLES]> {
    let id = decoded.global.map_or(0, |access| u64::from(access.id));
    Some(padded_holes(match heap {
        Heap::GlobalRead => [dst_hole, id, decoded.hole],
        Heap::GlobalStore => [id, a_hole, decoded.hole],
        Heap::LexicalRead | Heap::CalleeCapture => [dst_hole, decoded.hole, 0],
        Heap::LexicalWrite => [0, a_hole, decoded.hole],
        Heap::Word => [dst_hole, decoded.hole, 0],
        _ => return None,
    }))
}

/// The row that defines a name call's receiver, `undefined`, in the frame slot of `register`.
pub(crate) fn receiver_row(homes: &Homes, register: Register, pc: u32) -> Result<Planned, String> {
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

/// The realm the rows' slots and lexical bindings belong to: every row of one compile names the same one.
pub(crate) fn held_realm(decoded: &[Decoded]) -> Option<RealmProof> {
    let proof = |global: Value, realm_offset: i32| RealmProof { global: global.to_bits(), offset: u64::from(realm_offset.unsigned_abs()) };
    decoded.iter().find_map(|row| {
        row.global.map(|access| proof(access.slot.global, access.slot.realm_offset))
            .or_else(|| row.lexical.map(|slot| proof(slot.global, slot.realm_offset)))
            .or_else(|| row.cell.map(|cell| proof(cell.global, cell.realm_offset)))
    })
}

/// The global lexical cells the region's stores run through the VM for while they are not `Mutable` (`GlobalRead::Held`): the
/// write that makes one so retires the code, whose next compile stores the word live.
pub(crate) fn held_stores(known: Known, region: &LoopRegion) -> impl Iterator<Item = GlobalFact> {
    let table = known.globals.into_iter().flat_map(|table| table.range(region.header as usize..=region.backedge as usize));
    table.filter_map(|(_, read)| if let GlobalRead::Held(fact) = read { Some(*fact) } else { None })
}

/// The global objects whose layout the rows hold (a Stable fact): a change of one retires the code.
pub(crate) fn held_layouts(decoded: &[Decoded]) -> Box<[GlobalLayoutFact]> {
    distinct(decoded.iter().filter_map(|row| row.global.map(|access| access.slot.layout).or_else(|| row.lexical.map(|slot| slot.layout))))
}

/// The slot types the rows rely on, each once: a global object's slots, and the field slots a read takes
/// unchecked (`Decoded::typed`). A write of another type retires the code.
pub(crate) fn held_slot_types(decoded: &[Decoded]) -> Box<[SlotDependency]> {
    let mut types: Vec<SlotDependency> = decoded
        .iter()
        .filter_map(|row| row.global)
        .filter_map(|access| access.slot.dependency)
        .chain(decoded.iter().filter_map(|row| row.typed))
        .collect();
    types.sort_unstable_by_key(|dependency| (dependency.shape, dependency.slot));
    types.dedup();
    types.into()
}

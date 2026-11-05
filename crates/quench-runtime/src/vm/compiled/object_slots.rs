//! Physical own-data projections shared by native region consumers.
//!
//! An exact logical shape does not establish its storage encoding. Discharge
//! that separate obligation and the complete slot range once, then use fixed
//! displacements. Layout proofs never freeze writable property contents.

use super::*;
use crate::heap::PlainPropertyStorage;
use crate::value_vec::SlotRepresentation;

#[derive(Clone, Copy)]
pub(super) struct ObjectHeader {
    pub(super) record: ir::Value,
    pub(super) shape: ir::Value,
    pub(super) inline: ir::Value,
}

impl ObjectHeader {
    pub(super) fn prove(builder: &mut FunctionBuilder<'_>, bits: ir::Value,
        records: ir::Value, miss: ir::Block) -> Self {
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let family = builder.ins().band_imm_u(bits, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
        let object = builder.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
        emit_js_guard(builder, object, miss);
        // A direct object's id names a record of the reservation, which every native entry
        // and allocating callout reads the base of (`JsStepDispatch::refresh_allocation_bases`).
        let id = builder.ins().ireduce(types::I32, bits);
        let id = builder.ins().uextend(types::I64, id);
        let offset = builder.ins().imul_imm_u(id, layout.record_size as i64);
        let record = builder.ins().iadd(records, offset);
        let shape = builder.ins().load(types::I32, MemFlagsData::trusted(), record, layout.auxiliary_offset as i32);
        let inline = builder.ins().band_imm_u(shape, i64::from(layout.packed_storage_mask));
        Self { record, shape, inline }
    }

    /// `prove` for a read, which a function's own fields answer too: a direct function's
    /// record holds its `Object` at `NATIVE_FUNCTION_LAYOUT.object_offset`, of the same words.
    /// A store must not take this proof: its barrier reads object record marks.
    pub(super) fn prove_readable(builder: &mut FunctionBuilder<'_>, bits: ir::Value,
        records: ir::Value, functions: ir::Value, miss: ir::Block) -> Self {
        use crate::value::ValueLayout;
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let function = crate::heap::NATIVE_FUNCTION_LAYOUT;
        let (object_block, function_block, join) = (builder.create_block(), builder.create_block(), builder.create_block());
        // A function's own read is the rare one; the object path stays the straight line.
        builder.set_cold_block(function_block);
        let record = builder.append_block_param(join, types::I64);
        let family = builder.ins().band_imm_u(bits, ValueLayout::HEAP_FAMILY_MASK as i64);
        let id = builder.ins().ireduce(types::I32, bits);
        let id = builder.ins().uextend(types::I64, id);
        let object = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_OBJECT_TAG as i64);
        builder.ins().brif(object, object_block, &[], function_block, &[]);
        builder.switch_to_block(object_block);
        let offset = builder.ins().imul_imm_u(id, layout.record_size as i64);
        let object_record = builder.ins().iadd(records, offset);
        builder.ins().jump(join, &[ir::BlockArg::Value(object_record)]);
        builder.switch_to_block(function_block);
        let is_function = builder.ins().icmp_imm_u(IntCC::Equal, family, ValueLayout::DIRECT_FUNCTION_TAG as i64);
        let reserved = builder.ins().icmp_imm_u(IntCC::NotEqual, functions, 0);
        let readable = builder.ins().band(is_function, reserved);
        emit_js_guard(builder, readable, miss);
        let offset = builder.ins().imul_imm_u(id, function.record_size as i64);
        let function_record = builder.ins().iadd(functions, offset);
        let function_object = builder.ins().iadd_imm_u(function_record, function.object_offset as i64);
        builder.ins().jump(join, &[ir::BlockArg::Value(function_object)]);
        builder.switch_to_block(join);
        let shape = builder.ins().load(types::I32, MemFlagsData::trusted(), record, layout.auxiliary_offset as i32);
        let inline = builder.ins().band_imm_u(shape, i64::from(layout.packed_storage_mask));
        Self { record, shape, inline }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct NativeFieldLoad {
    pub(crate) shape: u32,
    /// The exact shape of the receiver's prototype, an object record, that holds the slot;
    /// `None` for the receiver's own (`FieldCache::holder`).
    pub(crate) holder: Option<u32>,
    pub(crate) slot: u16,
    pub(crate) storage_len: usize,
    /// The representation of every value the slot has held when the code was compiled
    /// (`Heap::slot_representation`): a read speculates on it and leaves where it fails.
    pub(crate) representation: SlotRepresentation,
    /// For a holder read, the value its proved immediate prototype holds when the code was
    /// compiled (`Vm::inherited_constant`): a read takes it while the receiver's prototype is
    /// that one and no prototype has changed since (`heap::prototype_value_epoch`).
    pub(crate) inherited: Option<InheritedConstant>,
}

/// The most prototypes of one holder shape an inherited read folds: sibling classes whose
/// prototypes hold the same names share a shape, and each holds its own values.
pub(crate) const INHERITED_PROTOTYPES: usize = 4;

/// An inherited read's values, each fixed by the receiver's prototype link that names its
/// holder, under the prototype value epoch they were read at.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct InheritedConstant {
    values: [(u32, Value); INHERITED_PROTOTYPES],
    count: u8,
    pub(crate) epoch: u64,
}

impl InheritedConstant {
    /// The values `holders` (prototype link and value) give, read under `epoch`; `None`
    /// where there are none or more than the read folds.
    pub(crate) fn of(holders: impl IntoIterator<Item = (u32, Value)>, epoch: u64) -> Option<Self> {
        let mut constant = Self { values: [(0, Value::UNDEFINED); INHERITED_PROTOTYPES], count: 0, epoch };
        for holder in holders {
            *constant.values.get_mut(usize::from(constant.count))? = holder;
            constant.count += 1;
        }
        (constant.count > 0).then_some(constant)
    }

    pub(crate) fn values(&self) -> &[(u32, Value)] {
        &self.values[..usize::from(self.count)]
    }

    /// These values and `other`'s, read under one epoch, each prototype once; `None` where
    /// the epochs differ, a prototype holds two values, or there are more than the read folds.
    pub(crate) fn joined(self, other: Self) -> Option<Self> {
        if self.epoch != other.epoch { return None; }
        let mut values: Vec<(u32, Value)> = self.values().to_vec();
        for (prototype, value) in other.values() {
            match values.iter().find(|(known, _)| known == prototype) {
                Some((_, held)) if held != value => return None,
                Some(_) => {}
                None => values.push((*prototype, *value)),
            }
        }
        Self::of(values, self.epoch)
    }
}

impl NativeFieldLoad {
    /// Whether a miss of a site's `loads` asks the VM's caches before it leaves: a slot held
    /// by the immediate prototype proves only that prototype, where a deeper holder's read
    /// may answer, and a polymorphic site's other receivers may be cached. A monomorphic
    /// site's inherited constant proves its value exactly; its miss leaves.
    pub(super) fn probes_misses(loads: &[Self]) -> bool {
        match loads {
            [load] => load.holder.is_some() && load.inherited.is_none(),
            loads => loads.iter().any(|load| load.holder.is_some()),
        }
    }
    pub(super) fn valid(self) -> bool {
        // A holder read loads the holder's slot; only an own read's packed receiver bounds it.
        usize::from(self.slot) < self.storage_len
            && (self.holder.is_some()
                || self.storage() != Some(PlainPropertyStorage::PackedInline)
                || self.storage_len <= crate::heap::NATIVE_OBJECT_LAYOUT.inline_property_count)
    }

    /// Where a receiver of the exact shape keeps its values (`PlainPropertyStorage::of_exact`),
    /// which the read's shape match proves.
    pub(super) fn storage(self) -> Option<PlainPropertyStorage> {
        PlainPropertyStorage::of_exact(self.shape)
    }
}

/// The caller's admitted effects determine whether packed contents can change.
#[derive(Clone, Copy)]
pub(super) enum PackedAccess { Snapshot, Reload }

#[derive(Clone, Copy, PartialEq, Eq)]
enum PackedValue { Snapshot(ir::Value), Record(ir::Value) }

impl PackedValue {
    fn load(self, builder: &mut FunctionBuilder<'_>) -> ir::Value {
        match self {
            Self::Snapshot(bits) => bits,
            Self::Record(record) => builder.ins().load(types::I64, MemFlagsData::trusted(), record,
                crate::heap::NATIVE_OBJECT_LAYOUT.payload_offset as i32),
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Slots {
    Packed(PackedValue),
    Arena(ir::Value),
}

/// The address of the first arena slot of the object whose payload word is `payload`, in the
/// arena at `base`. An object in arena form without extras (`PlainPropertyStorage::Arena`)
/// keeps its storage start alone in that word (`Object::plain_storage_start`). A live object of
/// an exact shape with flat storage holds its run inside the arena: allocation places it there,
/// compaction moves position and base together, and native code reads the base again after
/// anything that allocates and spans no collection. So the range needs no bound.
fn arena_range(builder: &mut FunctionBuilder<'_>, payload: ir::Value, base: ir::Value) -> ir::Value {
    let offset = builder.ins().imul_imm_u(payload, crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as i64);
    builder.ins().iadd(base, offset)
}

/// A projection for a guarded receiver and exact shape. It may only be reused
/// while no admitted effect can change its layout, arena base or object lifetime.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CheckedObject {
    slots: Slots,
    pub(super) storage_len: usize,
}

impl CheckedObject {
    /// A flat store can reuse the range proof; packed writes use the VM's
    /// narrow-value encoder and layout-preserving barrier helper instead.
    pub(super) fn arena_address(self, builder: &mut FunctionBuilder<'_>, slot: u16) -> Option<ir::Value> {
        debug_assert!(usize::from(slot) < self.storage_len);
        let Slots::Arena(base) = self.slots else { return None };
        Some(builder.ins().iadd_imm_u(base,
            i64::from(slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as i64))
    }

    /// The projection of a receiver whose identity and exact shape, `plan.shape`, the caller
    /// proved: that shape's storage form fixes where its values live, so the projection reads
    /// the record's payload word once and tests nothing. `None` for values behind extras.
    pub(super) fn prove(
        builder: &mut FunctionBuilder<'_>,
        record: ir::Value,
        base: ir::Value,
        plan: NativeFieldLoad,
        access: PackedAccess,
    ) -> Option<Self> {
        if !plan.valid() { return None; }
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let slots = match plan.storage()? {
            PlainPropertyStorage::PackedInline => Slots::Packed(match access {
                PackedAccess::Snapshot => PackedValue::Snapshot(
                    builder.ins().load(types::I64, MemFlagsData::trusted(), record, layout.payload_offset as i32)),
                PackedAccess::Reload => PackedValue::Record(record),
            }),
            PlainPropertyStorage::Arena => {
                let payload = builder.ins().load(types::I64, MemFlagsData::trusted(), record, layout.payload_offset as i32);
                Slots::Arena(arena_range(builder, payload, base))
            }
        };
        Some(Self { slots, storage_len: plan.storage_len })
    }

    /// Read fresh contents using only the established layout and slot bound.
    pub(super) fn read(self, builder: &mut FunctionBuilder<'_>, slot: u16) -> ir::Value {
        debug_assert!(usize::from(slot) < self.storage_len);
        match self.slots {
            Slots::Arena(base) => {
                let offset = i32::from(slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as i32;
                builder.ins().load(types::I64, MemFlagsData::trusted(), base, offset)
            }
            Slots::Packed(packed) => {
                let join = builder.create_block();
                let bits = builder.append_block_param(join, types::I64);
                let payload = packed.load(builder);
                emit_packed_object_slot_read(builder, payload, slot, join);
                builder.switch_to_block(join);
                builder.seal_block(join);
                bits
            }
        }
    }
}

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
        let meta = builder.ins().load(types::I32, MemFlagsData::trusted(), record, layout.meta_offset as i32);
        let inline = builder.ins().band_imm_u(meta, i64::from(layout.inline_storage_mask));
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
        let meta = builder.ins().load(types::I32, MemFlagsData::trusted(), record, layout.meta_offset as i32);
        let inline = builder.ins().band_imm_u(meta, i64::from(layout.inline_storage_mask));
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
    pub(crate) storage: Option<PlainPropertyStorage>,
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
        usize::from(self.slot) < self.storage_len
            && (self.storage != Some(PlainPropertyStorage::PackedInline)
                || self.storage_len <= crate::heap::NATIVE_OBJECT_LAYOUT.inline_property_count)
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
    /// Packed inline or in the arena, as the record's flag says; `arena` is the proved range's
    /// address where the flag selects it (`arena_range`), zero where the object is inline.
    Dynamic { packed: PackedValue, inline: ir::Value, arena: ir::Value },
}

/// The address of the first arena slot of the object whose payload word is `payload`, its
/// `storage_len` slots proved inside the arena of `base` and `length`; else `miss`.
fn arena_range(builder: &mut FunctionBuilder<'_>, payload: ir::Value, base: ir::Value, length: ir::Value,
    storage_len: usize, miss: ir::Block) -> ir::Value {
    let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
    let start = builder.ins().ireduce(types::I32, payload);
    let start = builder.ins().band_imm_u(start, layout.storage_start_mask as i64);
    let start = builder.ins().uextend(types::I64, start);
    let end = builder.ins().iadd_imm_u(start, storage_len as i64);
    let inside = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, end, length);
    let present = builder.ins().icmp_imm_u(IntCC::NotEqual, base, 0);
    let valid = builder.ins().band(inside, present);
    emit_js_guard(builder, valid, miss);
    let offset = builder.ins().imul_imm_u(start, layout.property_value_size as i64);
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
    pub(super) fn arena_address(self, builder: &mut FunctionBuilder<'_>, slot: u16, miss: ir::Block) -> Option<ir::Value> {
        debug_assert!(usize::from(slot) < self.storage_len);
        let base = match self.slots {
            Slots::Packed(_) => return None,
            Slots::Arena(base) => base,
            Slots::Dynamic { inline, arena, .. } => {
                let flat = builder.ins().icmp_imm_u(IntCC::Equal, inline, 0);
                emit_js_guard(builder, flat, miss);
                arena
            }
        };
        Some(builder.ins().iadd_imm_u(base,
            i64::from(slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as i64))
    }

    /// The caller proves receiver identity and exact shape before this operation.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prove(
        builder: &mut FunctionBuilder<'_>,
        record: ir::Value,
        inline: ir::Value,
        base: ir::Value,
        length: ir::Value,
        plan: NativeFieldLoad,
        access: PackedAccess,
        miss: ir::Block,
    ) -> Option<Self> {
        if !plan.valid() { return None; }
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let payload = builder.ins().load(types::I64, MemFlagsData::trusted(), record, layout.payload_offset as i32);
        let extras = builder.ins().band_imm_u(payload, layout.extras_tag_mask as i64);
        let plain = builder.ins().icmp_imm_u(IntCC::NotEqual, extras, layout.extras_tag as i64);
        emit_js_guard(builder, plain, miss);
        let packed = match access {
            PackedAccess::Snapshot => PackedValue::Snapshot(payload),
            PackedAccess::Reload => PackedValue::Record(record),
        };
        let slots = if plan.storage == Some(PlainPropertyStorage::PackedInline) {
            emit_js_guard(builder, inline, miss);
            Slots::Packed(packed)
        } else if plan.storage.is_none() && plan.storage_len <= layout.inline_property_count {
            // The range is proved once, on the arena side of the flag only.
            let (arena_block, join) = (builder.create_block(), builder.create_block());
            let arena = builder.append_block_param(join, types::I64);
            let none = builder.ins().iconst(types::I64, 0);
            builder.ins().brif(inline, join, &[ir::BlockArg::Value(none)], arena_block, &[]);
            builder.switch_to_block(arena_block);
            builder.seal_block(arena_block);
            let address = arena_range(builder, payload, base, length, plan.storage_len, miss);
            builder.ins().jump(join, &[ir::BlockArg::Value(address)]);
            builder.switch_to_block(join);
            builder.seal_block(join);
            Slots::Dynamic { packed, inline, arena }
        } else {
            let flat = builder.ins().icmp_imm_u(IntCC::Equal, inline, 0);
            emit_js_guard(builder, flat, miss);
            Slots::Arena(arena_range(builder, payload, base, length, plan.storage_len, miss))
        };
        Some(Self { slots, storage_len: plan.storage_len })
    }

    /// Read fresh contents using only the established layout and slot bound.
    pub(super) fn read(self, builder: &mut FunctionBuilder<'_>, slot: u16) -> ir::Value {
        debug_assert!(usize::from(slot) < self.storage_len);
        let offset = i32::from(slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as i32;
        if let Slots::Arena(base) = self.slots {
            return builder.ins().load(types::I64, MemFlagsData::trusted(), base, offset);
        }
        let join = builder.create_block();
        let bits = builder.append_block_param(join, types::I64);
        let packed = match self.slots {
            Slots::Packed(packed) => packed,
            Slots::Dynamic { packed, inline, arena } => {
                let inline_block = builder.create_block();
                let arena_block = builder.create_block();
                builder.ins().brif(inline, inline_block, &[], arena_block, &[]);
                builder.switch_to_block(arena_block);
                builder.seal_block(arena_block);
                let value = builder.ins().load(types::I64, MemFlagsData::trusted(), arena, offset);
                builder.ins().jump(join, &[ir::BlockArg::Value(value)]);
                builder.switch_to_block(inline_block);
                builder.seal_block(inline_block);
                packed
            }
            Slots::Arena(_) => unreachable!(),
        };
        let payload = packed.load(builder);
        emit_packed_object_slot_read(builder, payload, slot, join);
        builder.switch_to_block(join);
        builder.seal_block(join);
        bits
    }
}

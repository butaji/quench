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
        let id = builder.ins().ireduce(types::I32, bits);
        let inside = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, id, layout.record_capacity as i64);
        emit_js_guard(builder, inside, miss);
        let present = builder.ins().icmp_imm_u(IntCC::NotEqual, records, 0);
        emit_js_guard(builder, present, miss);
        let id = builder.ins().uextend(types::I64, id);
        let offset = builder.ins().imul_imm_u(id, layout.record_size as i64);
        let record = builder.ins().iadd(records, offset);
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
}

impl NativeFieldLoad {
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
    Dynamic { packed: PackedValue, inline: ir::Value, arena: ir::Value },
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
        } else {
            let start = builder.ins().ireduce(types::I32, payload);
            let start = builder.ins().band_imm_u(start, layout.storage_start_mask as i64);
            let start = builder.ins().uextend(types::I64, start);
            let end = builder.ins().iadd_imm_u(start, plan.storage_len as i64);
            let inside = builder.ins().icmp(IntCC::UnsignedLessThanOrEqual, end, length);
            let present = builder.ins().icmp_imm_u(IntCC::NotEqual, base, 0);
            let arena = builder.ins().icmp_imm_u(IntCC::Equal, inline, 0);
            let valid = builder.ins().band(inside, present);
            let valid = builder.ins().band(valid, arena);
            let valid = if plan.storage.is_none() && plan.storage_len <= layout.inline_property_count {
                let is_packed = builder.ins().icmp_imm_u(IntCC::NotEqual, inline, 0);
                builder.ins().bor(valid, is_packed)
            } else { valid };
            emit_js_guard(builder, valid, miss);
            let offset = builder.ins().imul_imm_u(start, layout.property_value_size as i64);
            let arena = builder.ins().iadd(base, offset);
            if plan.storage == Some(PlainPropertyStorage::Arena) || plan.storage_len > layout.inline_property_count {
                Slots::Arena(arena)
            } else {
                Slots::Dynamic { packed, inline, arena }
            }
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

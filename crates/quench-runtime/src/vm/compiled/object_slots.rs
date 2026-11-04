//! Physical own-data projections shared by native region consumers.
//!
//! An exact logical shape does not establish its storage encoding. Discharge
//! that separate obligation and the complete slot range once, then use fixed
//! displacements. Layout proofs never freeze writable property contents.

use super::*;
use crate::heap::PlainPropertyStorage;

/// A non-null table reservation admitted for this native run. This proves no
/// receiver identity, ID range, record liveness or property-storage lifetime.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CheckedObjectTable(ir::Value);

impl CheckedObjectTable {
    pub(super) fn prove(builder: &mut FunctionBuilder<'_>, base: ir::Value, miss: ir::Block) -> Self {
        let present = builder.ins().icmp_imm_u(IntCC::NotEqual, base, 0);
        emit_js_guard(builder, present, miss);
        Self(base)
    }

    pub(super) fn base(self) -> ir::Value { self.0 }
}

/// Legacy operation-local admission and region-entry admission use the same
/// projector; only the latter has already discharged the table-present guard.
#[derive(Clone, Copy)]
pub(super) enum ObjectTable {
    Unchecked(ir::Value),
    Checked(CheckedObjectTable),
}

impl From<ir::Value> for ObjectTable {
    fn from(base: ir::Value) -> Self { Self::Unchecked(base) }
}

impl From<CheckedObjectTable> for ObjectTable {
    fn from(table: CheckedObjectTable) -> Self { Self::Checked(table) }
}

#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct ObjectHeader {
    pub(super) record: ObjectRecord,
    pub(super) shape: ir::Value,
    pub(super) inline: ir::Value,
}

/// A verified direct-object locator. Indexed records keep the admitted base and
/// byte offset; address values are derived only for their consumers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ObjectRecord {
    /// A record address already admitted by the region's receiver entry.
    Address(ir::Value),
    Indexed { base: ir::Value, offset: ir::Value },
}

impl ObjectRecord {
    pub(super) fn prove(builder: &mut FunctionBuilder<'_>, bits: ir::Value,
        records: impl Into<ObjectTable>, known: Option<typed_ir::VerifiedFact>, miss: ir::Block) -> Self {
        let (records, table_present) = match records.into() {
            ObjectTable::Unchecked(base) => (base, false),
            ObjectTable::Checked(table) => (table.base(), true),
        };
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let id = builder.ins().ireduce(types::I32, bits);
        // Only a dominating verified definition/edge can supply this fact.
        // No safepoint or layout-changing operation may cross its lifetime.
        if !known.is_some_and(|fact| fact.ty.is_direct_object_family()) {
            let family = builder.ins().band_imm_u(bits, crate::value::ValueLayout::HEAP_FAMILY_MASK as i64);
            let object = builder.ins().icmp_imm_u(IntCC::Equal, family, crate::value::ValueLayout::DIRECT_OBJECT_TAG as i64);
            emit_js_guard(builder, object, miss);
        }
        if known.and_then(|fact| fact.ty.object_shape()).is_none() {
            let inside = builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, id, layout.record_capacity as i64);
            emit_js_guard(builder, inside, miss);
            if !table_present {
                let present = builder.ins().icmp_imm_u(IntCC::NotEqual, records, 0);
                emit_js_guard(builder, present, miss);
            }
        }
        let id = builder.ins().uextend(types::I64, id);
        let offset = builder.ins().imul_imm_u(id, layout.record_size as i64);
        Self::Indexed { base: records, offset }
    }

    pub(super) fn address(self, builder: &mut FunctionBuilder<'_>) -> ir::Value {
        match self {
            Self::Address(address) => address,
            Self::Indexed { base, offset } => builder.ins().iadd(base, offset),
        }
    }

    /// Combine the slot displacement with the record index before projecting
    /// an address, so the record's summed pointer need not span the guards.
    pub(super) fn offset_address(self, builder: &mut FunctionBuilder<'_>, displacement: ir::Value) -> ir::Value {
        let (base, offset) = match self {
            Self::Address(address) => (address, displacement),
            Self::Indexed { base, offset } => (base, builder.ins().iadd(offset, displacement)),
        };
        builder.ins().iadd(base, offset)
    }

    pub(super) fn header(self, builder: &mut FunctionBuilder<'_>, known: Option<typed_ir::VerifiedFact>) -> ObjectHeader {
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let address = self.address(builder);
        let shape = match known.and_then(|fact| fact.ty.object_shape()) {
            Some(shape) => builder.ins().iconst(types::I32, i64::from(shape)),
            None => builder.ins().load(types::I32, MemFlagsData::trusted(), address, layout.auxiliary_offset as i32),
        };
        let meta = builder.ins().load(types::I32, MemFlagsData::trusted(), address, layout.meta_offset as i32);
        let inline = builder.ins().band_imm_u(meta, i64::from(layout.inline_storage_mask));
        ObjectHeader { record: self, shape, inline }
    }
}

impl ObjectHeader {
    pub(super) fn prove(builder: &mut FunctionBuilder<'_>, bits: ir::Value,
        records: impl Into<ObjectTable>, known: Option<typed_ir::VerifiedFact>, miss: ir::Block) -> Self {
        ObjectRecord::prove(builder, bits, records, known, miss).header(builder, known)
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
    /// The observed slot type (`Heap::slot_type`), not a write-maintained
    /// invariant: a read speculates on it and leaves where its check fails.
    pub(crate) ty: crate::typed_ir::Type,
}

impl NativeFieldLoad {
    pub(super) fn valid(self) -> bool {
        self.shape & crate::value_vec::DICTIONARY_STORAGE == 0
            && usize::from(self.slot) < self.storage_len
            && (self.storage != Some(PlainPropertyStorage::PackedInline)
                || self.storage_len <= crate::heap::NATIVE_OBJECT_LAYOUT.inline_property_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_field_plans_reject_dictionary_and_missing_receiver_shapes() {
        let field = NativeFieldLoad { shape: 0, holder: None, slot: 0, storage_len: 1, storage: None, ty: crate::typed_ir::Type::ANY };
        assert!(field.valid());
        for shape in [super::super::super::NO_FIELD_RECEIVER,
            crate::value_vec::DICTIONARY_STORAGE, crate::value_vec::DICTIONARY_STORAGE | 1] {
            assert!(!NativeFieldLoad { shape, ..field }.valid());
        }
    }

    #[cfg(feature = "jit")]
    #[test]
    fn checked_table_admits_once_without_admitting_receiver_handles() {
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let mut signature = Signature::new(isa.default_call_conv());
        signature.params.extend([AbiParam::new(types::I64); 3]);
        signature.returns.push(AbiParam::new(types::I64));
        let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
        let mut context = FunctionBuilderContext::new();
        let mut builder = FunctionBuilder::new(&mut native, &mut context);
        let entry = builder.create_block();
        builder.append_block_params_for_function_params(entry);
        builder.switch_to_block(entry);
        let parameters = builder.block_params(entry).to_vec();
        let miss = builder.create_block();
        let table = CheckedObjectTable::prove(&mut builder, parameters[0], miss);
        // The second receiver retains its own family and range checks, even
        // after the first receiver has proved both against this same table.
        ObjectRecord::prove(&mut builder, parameters[1], table, None, miss);
        let record = ObjectRecord::prove(&mut builder, parameters[2], table, None, miss);
        let address = record.address(&mut builder);
        builder.ins().return_(&[address]);
        builder.switch_to_block(miss);
        let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
        builder.ins().return_(&[deleted]);
        builder.seal_all_blocks();
        builder.finalize(isa.frontend_config());
        let guards = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
            .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Brif).count();
        const TABLE_GUARDS: usize = 1;
        const RECEIVER_GUARDS: usize = 2;
        assert_eq!(guards, TABLE_GUARDS + 2 * RECEIVER_GUARDS, "{native}");
        let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
        // SAFETY: the compiled function only checks bits and computes an
        // address; it dereferences neither table nor supplied receiver.
        let run = unsafe { std::mem::transmute::<*mut u8,
            unsafe extern "C" fn(u64, u64, u64) -> u64>(memory.pointer()) };
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        let records = vec![0_u8; layout.record_size * 2];
        let base = records.as_ptr() as u64;
        let object = Value::direct_object(1).to_bits();
        let outside = Value::direct_object(layout.record_capacity as u32).to_bits();
        assert_eq!(unsafe { run(base, object, object) }, base + layout.record_size as u64);
        for (table, first, second) in [
            (0, object, object), (base, Value::NULL.to_bits(), object),
            (base, object, Value::NULL.to_bits()), (base, outside, object), (base, object, outside),
        ] {
            assert_eq!(unsafe { run(table, first, second) }, Value::DELETED.to_bits());
        }
    }

    #[cfg(feature = "jit")]
    #[test]
    fn direct_family_admission_keeps_range_and_table_base_guards() {
        use crate::typed_ir::{BindingTime, Fact, Projection, Proof, Representation, Type};
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        for (ty, expected_guards) in [
            (None, 3),
            (Some(Type::language(crate::ValueType::Object)), 3),
            (Some(Type::DIRECT_OBJECT_FAMILY), 2),
            (Some(Type::direct_object(0)), 0),
        ] {
            let mut signature = Signature::new(isa.default_call_conv());
            signature.params.extend([AbiParam::new(types::I64); 2]);
            signature.returns.push(AbiParam::new(types::I64));
            let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
            let mut context = FunctionBuilderContext::new();
            let mut builder = FunctionBuilder::new(&mut native, &mut context);
            let entry = builder.create_block();
            builder.append_block_params_for_function_params(entry);
            builder.switch_to_block(entry);
            let parameters = builder.block_params(entry).to_vec();
            let miss = builder.create_block();
            let known = ty.map(|ty| Fact { ty, representation: Representation::Tagged,
                binding: BindingTime::Dynamic, proof: Proof::Definition, projection: Projection::Exact }
                .verified().unwrap());
            let record = ObjectRecord::prove(&mut builder, parameters[1], parameters[0], known, miss);
            let address = record.address(&mut builder);
            builder.ins().return_(&[address]);
            builder.switch_to_block(miss);
            let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
            builder.ins().return_(&[deleted]);
            builder.seal_all_blocks();
            builder.finalize(isa.frontend_config());
            let guards = native.layout.blocks().flat_map(|block| native.layout.block_insts(block))
                .filter(|inst| native.dfg.insts[*inst].opcode() == ir::Opcode::Brif).count();
            assert_eq!(guards, expected_guards, "{ty:?}: {native}");
            // Exact shapes retain the previous complete admission contract.
            // Only the weaker family contract is exercised with invalid IDs/bases.
            if ty.is_some_and(|ty| ty.object_shape().is_some()) { continue; }
            let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: the entry takes two u64 words and performs guarded address
            // arithmetic only; it never dereferences the supplied table address.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
            let records = vec![0_u8; layout.record_size * 2];
            let base = records.as_ptr() as u64;
            let bits = Value::direct_object(1).to_bits();
            assert_eq!(unsafe { run(base, bits) }, base + layout.record_size as u64);
            assert_eq!(unsafe { run(0, bits) }, Value::DELETED.to_bits(), "{ty:?}");
            let outside = Value::direct_object(layout.record_capacity as u32).to_bits();
            assert_eq!(unsafe { run(base, outside) }, Value::DELETED.to_bits(), "{ty:?}");
            if ty != Some(Type::DIRECT_OBJECT_FAMILY) {
                assert_eq!(unsafe { run(base, Value::NULL.to_bits()) }, Value::DELETED.to_bits(), "{ty:?}");
            }
        }
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

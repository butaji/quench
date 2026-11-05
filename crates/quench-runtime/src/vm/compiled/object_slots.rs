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

/// A projection for a guarded receiver and exact shape: the shared layout
/// (`staged::Layout`) of its IR values. It may only be reused while no admitted effect can
/// change its layout, arena base or object lifetime.
pub(super) type CheckedObject = crate::vm::staged::Layout<ir::Value>;

/// The specializer's stage (`staged::Stage`): each operation of the shared semantics emits
/// its IR, a guard leaving through its miss block.
pub(super) struct Emit<'a, 'b> {
    pub(super) builder: &'a mut FunctionBuilder<'b>,
    /// The property arena's base the residual reads (`JsStepDispatch::property_values`),
    /// for an operation that addresses the arena.
    pub(super) property_base: Option<ir::Value>,
}

impl crate::vm::staged::Stage for Emit<'_, '_> {
    type Word = ir::Value;
    type Cond = ir::Value;
    type Miss = ir::Block;
    fn constant(&mut self, bits: u64) -> ir::Value { self.builder.ins().iconst(types::I64, bits as i64) }
    fn load_u32(&mut self, base: ir::Value, offset: i32) -> ir::Value {
        let word = self.builder.ins().load(types::I32, MemFlagsData::trusted(), base, offset);
        self.builder.ins().uextend(types::I64, word)
    }
    fn load_u64(&mut self, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(types::I64, MemFlagsData::trusted(), base, offset)
    }
    fn store_u32(&mut self, base: ir::Value, offset: i32, value: ir::Value) {
        let word = self.builder.ins().ireduce(types::I32, value);
        self.builder.ins().store(MemFlagsData::trusted(), word, base, offset);
    }
    fn store_u64(&mut self, base: ir::Value, offset: i32, value: ir::Value) {
        self.builder.ins().store(MemFlagsData::trusted(), value, base, offset);
    }
    fn load_table(&mut self, address: ir::Value) -> ir::Value {
        self.builder.ins().load(types::I64, invariant(), address, 0)
    }
    fn add(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().iadd(a, b) }
    fn or(&mut self, a: ir::Value, b: ir::Value) -> ir::Value { self.builder.ins().bor(a, b) }
    fn add_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().iadd_imm_u(a, imm as i64) }
    fn mul_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().imul_imm_u(a, imm as i64) }
    fn and_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().band_imm_u(a, imm as i64) }
    fn or_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().bor_imm_u(a, imm as i64) }
    fn shr_imm(&mut self, a: ir::Value, shift: u32) -> ir::Value { self.builder.ins().ushr_imm_u(a, i64::from(shift)) }
    fn sar32_imm(&mut self, a: ir::Value, shift: u32) -> ir::Value {
        let low = self.builder.ins().ireduce(types::I32, a);
        let shifted = self.builder.ins().sshr_imm_u(low, i64::from(shift));
        self.builder.ins().uextend(types::I64, shifted)
    }
    fn eq_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::Equal, a, imm as i64) }
    fn below_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::UnsignedLessThan, a, imm as i64) }
    fn choose(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value, otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        let (then_block, otherwise_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let result = self.builder.append_block_param(join, types::I64);
        self.builder.ins().brif(cond, then_block, &[], otherwise_block, &[]);
        for (block, arm) in [(then_block, Box::new(then) as Box<dyn FnOnce(&mut Self) -> ir::Value + '_>), (otherwise_block, Box::new(otherwise))] {
            self.builder.switch_to_block(block);
            self.builder.seal_block(block);
            let value = arm(self);
            self.builder.ins().jump(join, &[ir::BlockArg::Value(value)]);
        }
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        result
    }
    fn guard(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        emit_js_guard(self.builder, cond, miss);
        Some(())
    }
    fn property_base(&mut self) -> ir::Value { self.property_base.expect("an arena layout's caller passes the arena base") }
}

impl CheckedObject {
    /// The projection of a receiver whose identity and exact shape, `plan.shape`, the caller
    /// proved (`staged::layout`). `None` for values behind extras.
    pub(super) fn prove(
        builder: &mut FunctionBuilder<'_>,
        record: ir::Value,
        base: ir::Value,
        plan: NativeFieldLoad,
        access: PackedAccess,
    ) -> Option<Self> {
        if !plan.valid() { return None; }
        let mut stage = Emit { builder, property_base: Some(base) };
        crate::vm::staged::layout(&mut stage, record, plan.shape, plan.storage_len, matches!(access, PackedAccess::Snapshot))
    }

    /// Read fresh contents using only the established layout and slot bound (`staged::read_slot`).
    pub(super) fn read(self, builder: &mut FunctionBuilder<'_>, slot: u16) -> ir::Value {
        crate::vm::staged::read_slot(&mut Emit { builder, property_base: None }, self, slot)
    }
}

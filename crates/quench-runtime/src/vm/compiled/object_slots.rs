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
        let records = match records.into() {
            ObjectTable::Unchecked(base) => base,
            ObjectTable::Checked(table) => table.base(),
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
        // A direct object's id names a record of the reservation, whose base every native
        // entry and allocating callout reads (`JsStepDispatch::refresh_allocation_bases`).
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
        Self { record: ObjectRecord::Address(record), shape, inline }
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
    /// Historical pointee shape, not a heap-access proof.
    pub(crate) target_shape: Option<u32>,
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
    pub(super) fn slot_dependency(self) -> Option<crate::code_dependencies::SlotDependency> {
        use crate::typed_ir::Type;
        (self.ty != Type::ANY && self.ty != Type::NEVER).then_some(crate::code_dependencies::SlotDependency {
            shape: self.holder.unwrap_or(self.shape), slot: self.slot, ty: self.ty, target_shape: self.target_shape,
        })
    }

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
        let field = NativeFieldLoad { shape: 0, holder: None, slot: 0, storage_len: 1, storage: None, ty: crate::typed_ir::Type::ANY, target_shape: None, inherited: None };
        assert!(field.valid());
        for shape in [super::super::super::NO_FIELD_RECEIVER,
            crate::value_vec::DICTIONARY_STORAGE, crate::value_vec::DICTIONARY_STORAGE | 1] {
            assert!(!NativeFieldLoad { shape, ..field }.valid());
        }
    }

    /// The residual of `staged::own_layout` and `read_slot` reads what the interpreter's
    /// reading of the same definition reads, for every form a site may prove, and leaves
    /// for the forms it does not handle (values extras displaced, or a form the site did not
    /// prove).
    #[cfg(feature = "jit")]
    #[test]
    fn emitted_own_reads_agree_with_the_interpreter_or_leave() {
        use crate::heap::{Cell, Heap, Object};
        use crate::staged::Form;
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let mut heap = Heap::new();
        let wide = crate::value_vec::INLINE_PROPERTY_COUNT + 2;
        for shape in 1..=wide {
            heap.register_property_shape(shape as u32, shape, None);
        }
        let reference = heap.alloc(Cell::Error("reference".into()));
        let mut objects = Vec::new();
        for prototype in [false, true] {
            for values in [vec![Value::number(7.0), reference], vec![Value::number(0.5), Value::NULL],
                (0..wide).map(|index| Value::number(index as f64)).collect()] {
                let object = heap.alloc(Cell::Object(Object::new(Value::NULL)));
                for (slot, value) in values.iter().enumerate() {
                    heap.property_add(object, *value, slot as u32 + 1);
                }
                if prototype { heap.object_record_mut(object).unwrap().mark_as_prototype(); }
                objects.push((object, values));
            }
        }
        for form in [Form::Packed, Form::Arena, Form::Either] {
            for slot in 0..2_u16 {
                let mut signature = Signature::new(isa.default_call_conv());
                signature.params.extend([AbiParam::new(types::I64); 2]);
                signature.returns.push(AbiParam::new(types::I64));
                let mut native = ir::Function::with_name_signature(UserFuncName::user(0, 0), signature);
                let mut context = FunctionBuilderContext::new();
                let mut builder = FunctionBuilder::new(&mut native, &mut context);
                let entry = builder.create_block();
                builder.append_block_params_for_function_params(entry);
                builder.switch_to_block(entry);
                let [record, base] = builder.block_params(entry).try_into().unwrap();
                let miss = builder.create_block();
                let mut stage = Emit { builder: &mut builder, property_values: Some(base) };
                let inline = crate::staged::inline_storage(&mut stage, record);
                let layout = crate::staged::own_layout(&mut stage, record, inline, form,
                    usize::from(slot) + 1, PackedAccess::Reload, miss).unwrap();
                let value = crate::staged::read_slot(&mut stage, layout, slot);
                builder.ins().return_(&[value]);
                builder.switch_to_block(miss);
                let deleted = builder.ins().iconst(types::I64, Value::DELETED.to_bits() as i64);
                builder.ins().return_(&[deleted]);
                builder.seal_all_blocks();
                builder.finalize(isa.frontend_config());
                let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
                // SAFETY: the function reads the record and the arena run it names, both live.
                let run = unsafe { std::mem::transmute::<*mut u8, unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
                for (object, values) in &objects {
                    let data = heap.object(*object).unwrap();
                    let Some(expected) = values.get(usize::from(slot)) else { continue };
                    let read = Value::from_bits(unsafe {
                        run(data as *const Object as u64, heap.property_values_base() as u64)
                    });
                    let storage = data.plain_property_storage();
                    let handled = match (form, storage) {
                        (_, None) => false,
                        (Form::Packed, Some(storage)) => storage == PlainPropertyStorage::PackedInline,
                        (Form::Arena, Some(storage)) => storage == PlainPropertyStorage::Arena,
                        (Form::Either, Some(_)) => true,
                    };
                    if handled {
                        assert_eq!(read, *expected, "{form:?} slot {slot} {storage:?}");
                    } else {
                        assert_eq!(read, Value::DELETED, "{form:?} slot {slot} {storage:?} leaves");
                    }
                }
            }
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
        // A receiver's family alone: a direct object's id names a record of the reservation.
        const TABLE_GUARDS: usize = 1;
        const RECEIVER_GUARDS: usize = 1;
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
        assert_eq!(unsafe { run(base, object, object) }, base + layout.record_size as u64);
        for (table, first, second) in [
            (0, object, object), (base, Value::NULL.to_bits(), object), (base, object, Value::NULL.to_bits()),
        ] {
            assert_eq!(unsafe { run(table, first, second) }, Value::DELETED.to_bits());
        }
    }

    #[cfg(feature = "jit")]
    #[test]
    fn direct_family_admission_tests_the_family_alone() {
        use crate::typed_ir::{BindingTime, Fact, Projection, Proof, Representation, Type};
        let isa = super::super::super::jit_backend::target_isa("speed").unwrap();
        let layout = crate::heap::NATIVE_OBJECT_LAYOUT;
        // A direct object's id names a record of the reservation, whose base every native
        // entry reads: only the family needs a test, and none where a fact proves it.
        for (ty, expected_guards) in [
            (None, 1),
            (Some(Type::language(crate::ValueType::Object)), 1),
            (Some(Type::DIRECT_OBJECT_FAMILY), 0),
            (Some(Type::direct_object(0)), 0),
            (Some(Type::direct_object(0).join(Type::language(crate::ValueType::Null))), 1),
            (Some(Type::direct_object(0).join(Type::NULLISH)), 1),
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
            if ty.is_some_and(|ty| ty.is_direct_object_family()) { continue; }
            let memory = compile_native_function(native, &*isa, usize::MAX, CodeMemory::new).unwrap();
            // SAFETY: the entry takes two u64 words and performs guarded address
            // arithmetic only; it never dereferences the supplied table address.
            let run = unsafe { std::mem::transmute::<*mut u8,
                unsafe extern "C" fn(u64, u64) -> u64>(memory.pointer()) };
            let records = vec![0_u8; layout.record_size * 2];
            let base = records.as_ptr() as u64;
            let bits = Value::direct_object(1).to_bits();
            assert_eq!(unsafe { run(base, bits) }, base + layout.record_size as u64);
            assert_eq!(unsafe { run(base, Value::NULL.to_bits()) }, Value::DELETED.to_bits(), "{ty:?}");
            assert_eq!(unsafe { run(base, Value::UNDEFINED.to_bits()) }, Value::DELETED.to_bits(), "{ty:?}");
        }
    }
}

pub(super) use crate::staged::PackedAccess;
use crate::staged::{Form, Layout, Slots, Stage, Table};

/// The specializer's stage (`crate::staged`): each operation emits its residual as Cranelift
/// IR into `builder`, where a guard's failure jumps to its miss block.
pub(super) struct Emit<'a, 'b> {
    pub(super) builder: &'a mut FunctionBuilder<'b>,
    /// The property arena's value base (`Table::PropertyValues`), where the operation
    /// addresses a run; a read through a proved layout already holds its address.
    pub(super) property_values: Option<ir::Value>,
}

impl Stage for Emit<'_, '_> {
    type Word = ir::Value;
    /// Any integer: nonzero holds.
    type Cond = ir::Value;
    type Miss = ir::Block;

    fn constant(&mut self, bits: u64) -> ir::Value { self.builder.ins().iconst(types::I64, bits as i64) }
    fn load_u32(&mut self, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().uload32(MemFlagsData::trusted(), base, offset)
    }
    fn load_u64(&mut self, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(types::I64, MemFlagsData::trusted(), base, offset)
    }
    fn load_table(&mut self, address: ir::Value) -> ir::Value {
        self.builder.ins().load(types::I64, MemFlagsData::trusted().with_readonly().with_can_move(), address, 0)
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
    fn ne_imm(&mut self, a: ir::Value, imm: u64) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::NotEqual, a, imm as i64) }
    fn not(&mut self, cond: ir::Value) -> ir::Value { self.builder.ins().icmp_imm_u(IntCC::Equal, cond, 0) }
    fn choose(&mut self, cond: ir::Value, then: impl FnOnce(&mut Self) -> ir::Value,
        otherwise: impl FnOnce(&mut Self) -> ir::Value) -> ir::Value {
        let (then_block, else_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let value = self.builder.append_block_param(join, types::I64);
        self.builder.ins().brif(cond, then_block, &[], else_block, &[]);
        self.builder.switch_to_block(then_block);
        self.builder.seal_block(then_block);
        let taken = then(self);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(taken)]);
        self.builder.switch_to_block(else_block);
        self.builder.seal_block(else_block);
        let other = otherwise(self);
        self.builder.ins().jump(join, &[ir::BlockArg::Value(other)]);
        self.builder.switch_to_block(join);
        self.builder.seal_block(join);
        value
    }
    fn guard(&mut self, cond: ir::Value, miss: ir::Block) -> Option<()> {
        emit_js_guard(self.builder, cond, miss);
        Some(())
    }
    fn known(&self, _cond: ir::Value) -> Option<bool> { None }
    fn table(&mut self, table: Table) -> ir::Value {
        match table { Table::PropertyValues => self.property_values.expect("an operation addressing the arena has its base") }
    }
}

/// A projection for a guarded receiver and exact shape: `staged::own_layout`'s residual. It may
/// only be reused while no admitted effect can change its layout, arena base or object lifetime.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CheckedObject(Layout<ir::Value, ir::Value>);

impl CheckedObject {
    pub(super) fn storage_len(self) -> usize { self.0.storage_len }

    /// A flat store can reuse the range proof; packed writes use the VM's
    /// narrow-value encoder and layout-preserving barrier helper instead.
    pub(super) fn arena_address(self, builder: &mut FunctionBuilder<'_>, slot: u16, miss: ir::Block) -> Option<ir::Value> {
        debug_assert!(usize::from(slot) < self.0.storage_len);
        let base = match self.0.slots {
            Slots::Packed(_) => return None,
            Slots::Flat(base) => base,
            Slots::Either { inline, flat, .. } => {
                let is_flat = builder.ins().icmp_imm_u(IntCC::Equal, inline, 0);
                emit_js_guard(builder, is_flat, miss);
                flat
            }
        };
        Some(builder.ins().iadd_imm_u(base,
            i64::from(slot) * crate::heap::NATIVE_OBJECT_LAYOUT.property_value_size as i64))
    }

    /// The caller proves receiver identity and exact shape before this operation; `inline`
    /// is the record's inline-storage bits (`ObjectRecord::header`), `base` the arena's.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn prove(
        builder: &mut FunctionBuilder<'_>,
        record: ir::Value,
        inline: ir::Value,
        base: ir::Value,
        plan: NativeFieldLoad,
        access: PackedAccess,
        miss: ir::Block,
    ) -> Option<Self> {
        if !plan.valid() { return None; }
        let mut stage = Emit { builder, property_values: Some(base) };
        crate::staged::own_layout(&mut stage, record, inline, Form::of(plan.storage, plan.storage_len),
            plan.storage_len, access, miss).map(Self)
    }

    /// Read fresh contents using only the established layout and slot bound.
    pub(super) fn read(self, builder: &mut FunctionBuilder<'_>, slot: u16) -> ir::Value {
        crate::staged::read_slot(&mut Emit { builder, property_values: None }, self.0, slot)
    }
}

//! Physical own-data projections shared by native region consumers.
//!
//! An exact logical shape does not establish its storage encoding. Discharge
//! that separate obligation and the complete slot range once, then use fixed
//! displacements. Layout proofs never freeze writable property contents.

use super::*;
use crate::compiled::regions;
use quench_runtime::heap::PlainPropertyStorage;
use quench_runtime::staged::machine::{Binary, Lane, MachineFlow, MachineScalar, Relation, Unary};

/// A non-null table reservation admitted for this native run. This proves no
/// receiver identity, ID range, record liveness or property-storage lifetime.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CheckedObjectTable(ir::Value);

impl CheckedObjectTable {
    pub(super) fn prove(builder: &mut FunctionBuilder<'_>, base: ir::Value, miss: ir::Block) -> Self {
        let present = emit(builder).raw_compare_imm(Relation::NotEqual, base, 0);
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
    /// The record's address, computed once for the header loads and every later read.
    pub(super) address: ir::Value,
    /// The record's layout id, where it was loaded (`IcOp::GuardLayout`).
    pub(super) word: Option<ir::Value>,
    /// The record's auxiliary word (exact shape and storage-form bits) the header read, which a
    /// formed read compares (`staged::property_slots::formed_slot`) instead of reading it again.
    pub(super) auxiliary: Option<ir::Value>,
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
        let layout = quench_runtime::heap::NATIVE_OBJECT_LAYOUT;
        let id = emit(builder).raw_unary(Unary::Narrow(Lane::I32), bits);
        // Only a dominating verified definition/edge can supply this fact.
        // No safepoint or layout-changing operation may cross its lifetime.
        if !known.is_some_and(|fact| fact.ty.is_direct_object_family()) {
            let object = quench_runtime::staged::value::is_direct_object(&mut emit(builder), bits);
            emit_js_guard(builder, object, miss);
        }
        // A direct object's id names a record of the reservation, whose base every native
        // entry and allocating callout reads (`JsStepDispatch::refresh_allocation_bases`).
        let id = emit(builder).raw_unary(Unary::ZeroExtend(Lane::I64), id);
        let offset = emit(builder).raw_binary_imm(Binary::Mul, id, layout.record_size as i64);
        Self::Indexed { base: records, offset }
    }

    pub(super) fn address(self, builder: &mut FunctionBuilder<'_>) -> ir::Value {
        match self {
            Self::Address(address) => address,
            Self::Indexed { base, offset } => emit(builder).raw_binary(Binary::Add, base, offset),
        }
    }

    /// Combine the slot displacement with the record index before projecting
    /// an address, so the record's summed pointer need not span the guards.
    pub(super) fn offset_address(self, builder: &mut FunctionBuilder<'_>, displacement: ir::Value) -> ir::Value {
        let (base, offset) = match self {
            Self::Address(address) => (address, displacement),
            Self::Indexed { base, offset } => (base, emit(builder).raw_binary(Binary::Add, offset, displacement)),
        };
        emit(builder).raw_binary(Binary::Add, base, offset)
    }

    pub(super) fn header(self, builder: &mut FunctionBuilder<'_>, known: Option<typed_ir::VerifiedFact>) -> ObjectHeader {
        let address = self.address(builder);
        let (loaded, word, inline, auxiliary) = shape_and_inline(builder, address);
        let (shape, word) = match known.and_then(|fact| fact.ty.object_shape()) {
            Some(shape) => (emit(builder).raw_const(Lane::I32, i64::from(shape)), None),
            None => (loaded, Some(word)),
        };
        ObjectHeader { record: self, shape, inline, address, word, auxiliary: Some(auxiliary) }
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
        let layout = quench_runtime::heap::NATIVE_OBJECT_LAYOUT;
        let function = quench_runtime::heap::NATIVE_FUNCTION_LAYOUT;
        let (object_block, function_block, join) = (builder.create_block(), builder.create_block(), builder.create_block());
        // A function's own read is the rare one; the object path stays the straight line.
        builder.set_cold_block(function_block);
        let record = builder.append_block_param(join, types::I64);
        let id = emit(builder).raw_unary(Unary::Narrow(Lane::I32), bits);
        let id = emit(builder).raw_unary(Unary::ZeroExtend(Lane::I64), id);
        let object = quench_runtime::staged::value::is_direct_object(&mut emit(builder), bits);
        emit(builder).branch_if(object, (object_block, &[]), (function_block, &[]));
        builder.switch_to_block(object_block);
        let offset = emit(builder).raw_binary_imm(Binary::Mul, id, layout.record_size as i64);
        let object_record = emit(builder).raw_binary(Binary::Add, records, offset);
        emit(builder).goto(join, &[object_record]);
        builder.switch_to_block(function_block);
        let is_function = quench_runtime::staged::value::is_direct_function(&mut emit(builder), bits);
        let reserved = emit(builder).raw_compare_imm(Relation::NotEqual, functions, 0);
        let readable = emit(builder).raw_binary(Binary::And, is_function, reserved);
        emit_js_guard(builder, readable, miss);
        let offset = emit(builder).raw_binary_imm(Binary::Mul, id, function.record_size as i64);
        let function_record = emit(builder).raw_binary(Binary::Add, functions, offset);
        let function_object = emit(builder).raw_binary_imm(Binary::Add, function_record, function.object_offset as i64);
        emit(builder).goto(join, &[function_object]);
        builder.switch_to_block(join);
        let (shape, word, inline, auxiliary) = shape_and_inline(builder, record);
        Self { record: ObjectRecord::Address(record), shape, inline, address: record, word: Some(word), auxiliary: Some(auxiliary) }
    }
}

/// The stage whose residual `staged` operations that address no arena emit into `builder`.
pub(super) fn emit<'a, 'b>(builder: &'a mut FunctionBuilder<'b>) -> Emit<'a, 'b> {
    Emit { builder, property_values: None, gate: None, payload: None }
}

/// The exact shape (`Object::exact_shape`) of the record at `address`, as an `I32`: its
/// auxiliary word without the storage-form bits, which no recorded shape carries.
pub(super) fn exact_shape(builder: &mut FunctionBuilder<'_>, flags: ir::MemFlagsData, address: ir::Value) -> ir::Value {
    exact_shape_word(builder, flags, address).0
}

/// `exact_shape` with the record's layout id (`I32`), the word it was read through.
pub(super) fn exact_shape_word(builder: &mut FunctionBuilder<'_>, flags: ir::MemFlagsData, address: ir::Value) -> (ir::Value, ir::Value) {
    let (layout_id, word) = auxiliary_word(builder, flags, address);
    let layout = quench_runtime::heap::NATIVE_OBJECT_LAYOUT;
    (builder.ins().band_imm_u(word, i64::from(!(layout.inline_form | layout.extras_form | layout.narrow_form))), layout_id)
}

/// The layout id (`I32`) of the record at `address` and its layout's auxiliary word (shape and
/// storage form, `I32`).
fn auxiliary_word(builder: &mut FunctionBuilder<'_>, flags: ir::MemFlagsData, address: ir::Value) -> (ir::Value, ir::Value) {
    let layout = quench_runtime::heap::NATIVE_OBJECT_LAYOUT;
    // The id load is the one `staged::object::layout_entry` emits, so the two merge.
    let id = builder.ins().uload32(flags, address, layout.layout_offset as i32);
    let layout_id = builder.ins().ireduce(types::I32, id);
    let offset = builder.ins().imul_imm_u(id, 8);
    let base = builder.ins().iconst(types::I64, quench_runtime::heap::layouts::table_base() as i64);
    let entry = builder.ins().iadd(base, offset);
    // The table entry is read-only memory that hoists and merges like any immutable read.
    let table = regions::flags(quench_runtime::staged::region::Region::HeapMeta.readonly());
    (layout_id, builder.ins().load(types::I32, table, entry, layout.auxiliary_offset as i32))
}

/// The record's exact shape, its layout id, its inline-storage flag and the auxiliary word they
/// came from, from one layout read:
/// the auxiliary word's storage-form bits carry the header's inline flag
/// (`Object::storage_form`), so the flag needs no second header word.
pub(super) fn shape_and_inline(builder: &mut FunctionBuilder<'_>, address: ir::Value) -> (ir::Value, ir::Value, ir::Value, ir::Value) {
    let layout = quench_runtime::heap::NATIVE_OBJECT_LAYOUT;
    let (layout_id, word) = auxiliary_word(builder, regions::shape_word(), address);
    let shape = builder.ins().band_imm_u(word, i64::from(!(layout.inline_form | layout.extras_form | layout.narrow_form)));
    let form = builder.ins().band_imm_u(word, i64::from(layout.inline_form));
    let inline = emit(builder).raw_compare_imm(Relation::NotEqual, form, 0);
    (shape, layout_id, inline, word)
}

/// `staged::object::inline_storage`'s residual for the record at `record`.
pub(super) fn inline_storage(builder: &mut FunctionBuilder<'_>, record: ir::Value) -> ir::Value {
    quench_runtime::staged::object::inline_storage(&mut emit(builder), record)
}

pub(crate) use quench_runtime::vm::engine_types::NativeFieldLoad;

/// The most shape-guarded load groups one field read lowers before it calls the field cache
/// instead (`field_load_groups`): V8 inlines 4 maps per access and SpiderMonkey folds 16
/// shapes into one guard; families make a group of many shapes.
const FIELD_LOAD_GROUPS: usize = 8;
/// The widest span of shape ids one shape-set guard tests by bitset, in 64-bit words
/// (`shape_in_set`); a wider set compares each shape.
const SHAPE_SET_WORDS: usize = 8;
const SHAPE_SET_WORD_BITS: u32 = u64::BITS;

/// A field read's loads as layout families: own fields at one slot of one storage form share
/// one decode, bounded by the family's shortest storage (which still holds the slot), behind
/// one shape-set guard (`shape_in_set`), as V8's homomorphic IC and SpiderMonkey's
/// GuardMultipleShapes do. A prototype-held load keeps its own group. Each group's shapes
/// come sorted; its load is the family's shortest. `None` for no loads, or more groups than
/// one read lowers (`FIELD_LOAD_GROUPS`): the read then takes the field cache.
pub(crate) fn field_load_groups(loads: &[NativeFieldLoad]) -> Option<Vec<(NativeFieldLoad, Vec<u32>)>> {
    let mut groups: Vec<(NativeFieldLoad, Vec<u32>)> = Vec::new();
    for load in loads {
        let family = groups.iter_mut().find(|(known, _)| load.holder.is_none() && known.holder.is_none()
            && known.slot == load.slot && known.storage == load.storage && known.raw_double == load.raw_double);
        match family {
            Some((known, shapes)) => {
                shapes.push(load.shape);
                if load.storage_len < known.storage_len { *known = *load; }
            }
            None => groups.push((*load, vec![load.shape])),
        }
    }
    for (_, shapes) in &mut groups {
        shapes.sort_unstable();
    }
    (!groups.is_empty() && groups.len() <= FIELD_LOAD_GROUPS).then_some(groups)
}

/// Whether the 32-bit header word `shape` is one of `shapes` (sorted, non-empty): one
/// compare for one shape, a range check and a bit of a constant bitset over the span the
/// shapes cover, else a compare per shape. Flag bits above the shape id put a header outside
/// every span.
pub(super) fn shape_in_set(builder: &mut FunctionBuilder<'_>, shape: ir::Value, shapes: &[u32]) -> ir::Value {
    let (low, high) = (shapes[0], shapes[shapes.len() - 1]);
    let span = u64::from(high - low) + 1;
    if shapes.len() == 1 || span > SHAPE_SET_WORDS as u64 * u64::from(SHAPE_SET_WORD_BITS) {
        let mut tests = shapes.iter().map(|&known| emit(builder).raw_compare_imm(Relation::Equal, shape, i64::from(known)));
        let first = tests.next().expect("a shape set is non-empty");
        let rest: Vec<_> = tests.collect();
        return rest.into_iter().fold(first, |any, test| emit(builder).raw_binary(Binary::Or, any, test));
    }
    let mut words = [0_u64; SHAPE_SET_WORDS];
    for &known in shapes {
        let offset = known - low;
        words[(offset / SHAPE_SET_WORD_BITS) as usize] |= 1 << (offset % SHAPE_SET_WORD_BITS);
    }
    let used = span.div_ceil(u64::from(SHAPE_SET_WORD_BITS)) as usize;
    let offset = emit(builder).raw_binary_imm(Binary::Add, shape, -i64::from(low));
    let in_span = emit(builder).raw_compare_imm(Relation::UnsignedLess, offset, span as i64);
    let offset = emit(builder).raw_unary(Unary::ZeroExtend(Lane::I64), offset);
    let index = emit(builder).raw_binary_imm(Binary::ShiftRightUnsigned, offset, i64::from(SHAPE_SET_WORD_BITS.trailing_zeros()));
    let mut word = emit(builder).raw_const(Lane::I64, words[used - 1] as i64);
    for (position, &bits) in words[..used - 1].iter().enumerate().rev() {
        let here = emit(builder).raw_compare_imm(Relation::Equal, index, position as i64);
        let bits = emit(builder).raw_const(Lane::I64, bits as i64);
        word = emit(builder).raw_select(here, bits, word);
    }
    let bit = emit(builder).raw_binary_imm(Binary::And, offset, i64::from(SHAPE_SET_WORD_BITS - 1));
    let bit = emit(builder).raw_binary(Binary::ShiftRightUnsigned, word, bit);
    let bit = emit(builder).raw_binary_imm(Binary::And, bit, 1);
    let member = emit(builder).raw_compare_imm(Relation::NotEqual, bit, 0);
    emit(builder).raw_binary(Binary::And, in_span, member)
}

pub(crate) use quench_runtime::vm::engine_types::INHERITED_PROTOTYPES;

pub(crate) use quench_runtime::vm::engine_types::InheritedConstant;

#[quench_macros::ext(InheritedConstantOps1)]
impl InheritedConstant {
    /// The chains the values were read through, one per value.
    pub(crate) fn chains(&self) -> impl Iterator<Item = quench_runtime::code_dependencies::PrototypeChain> + '_ {
        self.values().iter().zip(self.depths).map(|((link, _), depth)| quench_runtime::code_dependencies::PrototypeChain { link: *link, depth })
    }
}

#[quench_macros::ext(NativeFieldLoadOps1)]
impl NativeFieldLoad {
    /// Whether a read of it proves its receiver a direct object of its shape: an own slot of
    /// a shape no function record held.
    pub(crate) fn own_direct(self) -> bool {
        self.holder.is_none() && !self.function_shape
    }

    pub(super) fn slot_dependency(self) -> Option<quench_runtime::code_dependencies::SlotDependency> {
        use quench_runtime::typed_ir::Type;
        (self.ty != Type::ANY && self.ty != Type::NEVER).then_some(quench_runtime::code_dependencies::SlotDependency {
            shape: self.holder.unwrap_or(self.shape), slot: self.slot, ty: self.ty,
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
        self.shape & quench_runtime::value_vec::DICTIONARY_STORAGE == 0
            && usize::from(self.slot) < self.storage_len
            && (self.storage != Some(PlainPropertyStorage::PackedInline)
                || self.storage_len <= quench_runtime::heap::NATIVE_OBJECT_LAYOUT.inline_property_count)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn direct_field_plans_reject_dictionary_and_missing_receiver_shapes() {
        let field = NativeFieldLoad { shape: 0, holder: None, slot: 0, storage_len: 1, storage: None, ty: quench_runtime::typed_ir::Type::ANY, inherited: None, function_shape: false, raw_double: false, formed: None, holder_id: None };
        assert!(field.valid());
        for shape in [quench_runtime::NO_FIELD_RECEIVER,
            quench_runtime::value_vec::DICTIONARY_STORAGE, quench_runtime::value_vec::DICTIONARY_STORAGE | 1] {
            assert!(!NativeFieldLoad { shape, ..field }.valid());
        }
    }

    #[cfg(feature = "jit")]
    #[test]
    fn checked_table_admits_once_without_admitting_receiver_handles() {
        let isa = quench_runtime::jit_backend::target_isa().unwrap();
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
        let layout = quench_runtime::heap::NATIVE_OBJECT_LAYOUT;
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
        use quench_runtime::typed_ir::{BindingTime, Fact, Projection, Proof, Representation, Type};
        let isa = quench_runtime::jit_backend::target_isa().unwrap();
        let layout = quench_runtime::heap::NATIVE_OBJECT_LAYOUT;
        // A direct object's id names a record of the reservation, whose base every native
        // entry reads: only the family needs a test, and none where a fact proves it.
        for (ty, expected_guards) in [
            (None, 1),
            (Some(Type::language(quench_runtime::ValueType::Object)), 1),
            (Some(Type::DIRECT_OBJECT_FAMILY), 0),
            (Some(Type::direct_object(0)), 0),
            (Some(Type::direct_object(0).join(Type::language(quench_runtime::ValueType::Null))), 1),
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

use quench_runtime::staged::{Layout, Slots};
mod emit;
pub(super) use emit::Emit;

/// A projection for a guarded receiver and exact shape: `staged::own_layout`'s residual. It may
/// only be reused while no admitted effect can change its layout, arena base or object lifetime.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) struct CheckedObject(Layout<ir::Value, ir::Value>);

impl CheckedObject {
    /// The proof of a layout a staged read established.
    pub(super) fn of(layout: Layout<ir::Value, ir::Value>) -> Self { Self(layout) }

    /// A flat store can reuse the range proof; packed writes use the VM's
    /// narrow-value encoder and layout-preserving barrier helper instead.
    pub(super) fn arena_address(self, builder: &mut FunctionBuilder<'_>, slot: u16, miss: ir::Block) -> Option<ir::Value> {
        debug_assert!(usize::from(slot) < self.0.storage_len);
        let base = match self.0.slots {
            Slots::Packed(_) | Slots::Narrow(_) => return None,
            Slots::Flat(base) => base,
            Slots::Either { inline, flat, .. } => {
                let is_flat = emit(builder).raw_compare_imm(Relation::Equal, inline, 0);
                emit_js_guard(builder, is_flat, miss);
                flat
            }
        };
        Some(emit(builder).raw_binary_imm(Binary::Add, base, i64::from(slot) * quench_runtime::heap::NATIVE_OBJECT_LAYOUT.property_value_size as i64))
    }

    /// The projection of the record `record` whose `storage_len` values the arena at `base` holds
    /// behind its extras (`staged::arena_extras_layout`), as a global object by name does: its
    /// shape is held by a dependency, so only the representation is tested.
    pub(super) fn prove_arena_extras(builder: &mut FunctionBuilder<'_>, record: ir::Value, inline: ir::Value,
        base: ir::Value, storage_len: usize, miss: ir::Block) -> Option<Self> {
        let mut stage = Emit { builder, property_values: Some(base), gate: None, payload: None };
        quench_runtime::staged::arena_extras_layout(&mut stage, record, inline, storage_len, miss).map(Self)
    }

    /// The layout this proof carries, for a staged read of fresh contents through it.
    pub(super) fn layout(self) -> Layout<ir::Value, ir::Value> { self.0 }
}

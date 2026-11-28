//! Physical own-data projections shared by native region consumers.
//!
//! An exact logical shape does not establish its storage encoding. Discharge
//! that separate obligation and the complete slot range once, then use fixed
//! displacements. Layout proofs never freeze writable property contents.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use crate::compiled::regions;
use quench_runtime::tier_surface::PlainPropertyStorage;
use quench_runtime::staged::machine::{Binary, Lane, MachineAccess, MachineFlow, MachineScalar, Relation, Unary};

#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) struct ObjectHeader {
    pub(crate) record: ObjectRecord,
    /// The record's address, computed once for the header loads and every later read; a fixed class cell's cell.
    pub(crate) address: ir::Value,
    /// The exact shape the record holds (`None` for a fixed class cell, which holds no record shape: its class is the fact).
    pub(crate) shape: Option<ir::Value>,
    /// The record's inline-storage flag (`None` for a fixed class cell, whose storage form the union layout fixes).
    pub(crate) inline: Option<ir::Value>,
    /// The record's layout id, where it was loaded (`IcOp::GuardLayout`).
    pub(crate) word: Option<ir::Value>,
    /// The record's auxiliary word (exact shape and storage-form bits) the header read, which a
    /// formed read compares (`staged::property_slots::formed_slot`) instead of reading it again.
    pub(crate) auxiliary: Option<ir::Value>,
    /// The fact behind `shape` where it is a constant: the register's type (`ShapeFact::of_type`)
    /// or the guard that proved it (`headers::prove_header_shape`).
    pub(crate) shape_fact: Option<ShapeFact>,
}

/// S7f DESIGN: the object record table is gone and every ordinary object is a class cell. A probe that needed a
/// record id, or a table address, declines to `miss` unconditionally (`decline_record_table`) until the class cell
/// is addressed directly; no native read reaches a record.
pub(crate) fn decline_record_table(builder: &mut FunctionBuilder<'_>, miss: ir::Block) {
    let never = emit(builder).raw_const(Lane::I32, 0);
    emit_js_guard(builder, never, miss);
}

/// A verified direct-object locator. Address values are derived only for their consumers.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum ObjectRecord {
    /// A record address already admitted by the region's receiver entry.
    Address(ir::Value),
    /// The class cell of a fixed class (`AotFacts::class_fixed_cell`): every instance is this cell from birth, so no record
    /// exists, and no presence or class test is made. The cell is `class_base + granule * 8`, whose fields are the
    /// union slots (`offset_address`); the granule is the cell's record id in the class space (`Stage::store_gate`).
    FixedCell { cell: ir::Value, granule: ir::Value },
}

impl ObjectRecord {
    /// The class cell of a fixed class's direct object reference `bits` (`staged::object::cell_at_granule`): the granule
    /// the reference names, in the class space at `class_base`. The class is the fact, so nothing is tested.
    pub(crate) fn prove_fixed(builder: &mut FunctionBuilder<'_>, bits: ir::Value, class_base: ir::Value) -> Self {
        use quench_runtime::staged::layout_offsets::CLASS_CELL_GRANULE_BYTES;
        let granule = emit(builder).raw_unary(Unary::Narrow(Lane::I32), bits);
        let granule = emit(builder).raw_unary(Unary::ZeroExtend(Lane::I64), granule);
        let offset = emit(builder).raw_binary_imm(Binary::Mul, granule, CLASS_CELL_GRANULE_BYTES as i64);
        Self::FixedCell { cell: emit(builder).raw_binary(Binary::Add, class_base, offset), granule }
    }

    /// The record probe of a direct object (staged `object::object_record_of`). The record table it addressed is
    /// gone, so the probe declines (`decline_record_table`); the address that follows is unreachable.
    pub(crate) fn prove(builder: &mut FunctionBuilder<'_>, _bits: ir::Value, class_base: ir::Value,
        _known: Option<typed_ir::VerifiedFact>, miss: ir::Block) -> Self {
        decline_record_table(builder, miss);
        Self::Address(class_base)
    }

    /// The record's address, or a fixed class's cell (which holds no record).
    pub(crate) fn address(self, builder: &mut FunctionBuilder<'_>) -> ir::Value {
        match self {
            Self::Address(address) | Self::FixedCell { cell: address, .. } => address,
        }
    }

    /// The granule of a fixed class's cell (its class-space record id), or None for a record.
    pub(crate) fn granule(self) -> Option<ir::Value> {
        match self {
            Self::FixedCell { granule, .. } => Some(granule),
            Self::Address(_) => None,
        }
    }

    /// Combine the slot displacement with the record index before projecting
    /// an address, so the record's summed pointer need not span the guards. A fixed class's
    /// displacement is from its cell, whose fields are the union slots.
    pub(crate) fn offset_address(self, builder: &mut FunctionBuilder<'_>, displacement: ir::Value) -> ir::Value {
        let base = match self {
            Self::Address(address) | Self::FixedCell { cell: address, .. } => address,
        };
        emit(builder).raw_binary(Binary::Add, base, displacement)
    }

    pub(crate) fn header(self, builder: &mut FunctionBuilder<'_>, known: Option<typed_ir::VerifiedFact>) -> ObjectHeader {
        let address = self.address(builder);
        // A fixed class's cell holds no record: no shape or storage form is read from it, and its class is the fact.
        if let Self::FixedCell { .. } = self {
            return ObjectHeader { record: self, address, shape: None, inline: None, word: None, auxiliary: None, shape_fact: None };
        }
        let (loaded, word, inline, auxiliary) = shape_and_inline(builder, address);
        let (shape, word, shape_fact) = match known.and_then(|fact| ShapeFact::of_type(fact.ty)) {
            Some((shape, fact)) => (emit(builder).raw_const(Lane::I32, i64::from(shape)), None, Some(fact)),
            None => (loaded, Some(word), None),
        };
        ObjectHeader { record: self, address, shape: Some(shape), inline: Some(inline), word, auxiliary: Some(auxiliary), shape_fact }
    }
}

impl ObjectHeader {
    pub(crate) fn prove(builder: &mut FunctionBuilder<'_>, bits: ir::Value,
        class_base: ir::Value, known: Option<typed_ir::VerifiedFact>, miss: ir::Block) -> Self {
        ObjectRecord::prove(builder, bits, class_base, known, miss).header(builder, known)
    }

    /// `prove` for a read, which a function's own fields answer too: a direct function's
    /// record holds its `Object` at `NATIVE_FUNCTION_LAYOUT.object_offset`, of the same words.
    /// A direct object's record is gone (`decline_record_table`), so only a function's record is read here.
    /// A store must not take this proof: its barrier reads object record marks.
    pub(crate) fn prove_readable(builder: &mut FunctionBuilder<'_>, bits: ir::Value, known: Option<typed_ir::VerifiedFact>,
        functions: ir::Value, class_base: ir::Value, miss: ir::Block) -> Self {
        let function = quench_runtime::tier_surface::NATIVE_FUNCTION_LAYOUT;
        let id = emit(builder).raw_unary(Unary::Narrow(Lane::I32), bits);
        let id = emit(builder).raw_unary(Unary::ZeroExtend(Lane::I64), id);
        let object = quench_runtime::staged::value::is_direct_object(&mut emit(builder), bits);
        let not_object = emit(builder).raw_compare_imm(Relation::Equal, object, 0);
        emit_js_guard(builder, not_object, miss);
        // A solver type that proves a function proves the function tag too: only the table's reservation is tested.
        let reserved = emit(builder).raw_compare_imm(Relation::NotEqual, functions, 0);
        let readable = if known.is_some_and(|fact| proves_function(fact.ty)) { reserved } else {
            let is_function = quench_runtime::staged::value::is_direct_function(&mut emit(builder), bits);
            emit(builder).raw_binary(Binary::And, is_function, reserved)
        };
        emit_js_guard(builder, readable, miss);
        let offset = emit(builder).raw_binary_imm(Binary::Mul, id, function.record_size as i64);
        let function_record = emit(builder).raw_binary(Binary::Add, functions, offset);
        let record = emit(builder).raw_binary_imm(Binary::Add, function_record, function.object_offset as i64);
        let (shape, word, inline, auxiliary) = shape_and_inline(builder, record);
        Self { record: ObjectRecord::Address(record), address: record, shape: Some(shape), inline: Some(inline), word: Some(word), auxiliary: Some(auxiliary), shape_fact: None }
    }
}

/// Whether every value of solver type `ty` is a function (and the type is not empty): its direct-function tag is proved.
fn proves_function(ty: typed_ir::Type) -> bool {
    ty != typed_ir::Type::NEVER && ty.only_languages(typed_ir::Type::language(quench_runtime::ValueType::Function))
}

/// The stage whose residual `staged` operations that address no arena emit into `builder`.
pub(crate) fn emit<'a, 'b>(builder: &'a mut FunctionBuilder<'b>) -> Emit<'a, 'b> {
    Emit { builder, property_values: None, gate: None, payload: None, facts: Default::default(), fresh: false }
}

/// The exact shape (`Object::exact_shape`) of the record at `address`, as an `I32`: its
/// auxiliary word without the storage-form bits, which no recorded shape carries.
pub(crate) fn exact_shape(builder: &mut FunctionBuilder<'_>, flags: ir::MemFlagsData, address: ir::Value) -> ir::Value {
    exact_shape_word(builder, flags, address).0
}

/// `exact_shape` with the record's layout id (`I32`), the word it was read through.
pub(crate) fn exact_shape_word(builder: &mut FunctionBuilder<'_>, flags: ir::MemFlagsData, address: ir::Value) -> (ir::Value, ir::Value) {
    let (layout_id, word) = auxiliary_word(builder, flags, address);
    let layout = quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT;
    (builder.ins().band_imm_u(word, i64::from(!(layout.inline_form | layout.extras_form | layout.narrow_form))), layout_id)
}

/// The layout id (`I32`) of the record at `address` and its layout's auxiliary word (shape and
/// storage form, `I32`).
fn auxiliary_word(builder: &mut FunctionBuilder<'_>, flags: ir::MemFlagsData, address: ir::Value) -> (ir::Value, ir::Value) {
    let layout = quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT;
    // The id load is the one `staged::object::layout_entry` emits, so the two merge.
    let id = builder.ins().uload32(flags, address, layout.layout_offset as i32);
    let layout_id = builder.ins().ireduce(types::I32, id);
    let offset = builder.ins().imul_imm_u(id, 8);
    let base = builder.ins().iconst(types::I64, quench_runtime::tier_surface::layouts::table_base() as i64);
    let entry = builder.ins().iadd(base, offset);
    // The table entry is read-only memory that hoists and merges like any immutable read.
    let table = regions::flags(quench_runtime::staged::region::Region::HeapMeta.readonly());
    (layout_id, builder.ins().load(types::I32, table, entry, layout.auxiliary_offset as i32))
}

/// The record's exact shape, its layout id, its inline-storage flag and the auxiliary word they
/// came from, from one layout read:
/// the auxiliary word's storage-form bits carry the header's inline flag
/// (`Object::storage_form`), so the flag needs no second header word.
pub(crate) fn shape_and_inline(builder: &mut FunctionBuilder<'_>, address: ir::Value) -> (ir::Value, ir::Value, ir::Value, ir::Value) {
    let layout = quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT;
    let (layout_id, word) = auxiliary_word(builder, regions::shape_word(), address);
    let shape = builder.ins().band_imm_u(word, i64::from(!(layout.inline_form | layout.extras_form | layout.narrow_form)));
    let form = builder.ins().band_imm_u(word, i64::from(layout.inline_form));
    let inline = emit(builder).raw_compare_imm(Relation::NotEqual, form, 0);
    (shape, layout_id, inline, word)
}

/// `staged::object::inline_storage`'s residual for the record at `record`.
pub(crate) fn inline_storage(builder: &mut FunctionBuilder<'_>, record: ir::Value) -> ir::Value {
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
            && known.slot == load.slot && known.storage == load.storage);
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
pub(crate) fn shape_in_set(builder: &mut FunctionBuilder<'_>, shape: ir::Value, shapes: &[u32]) -> ir::Value {
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
    /// a shape no function record held. A guessed shape proves nothing (`guessed`). A class-keyed read
    /// proves a live cell, which no record-path consumer of the shape may take for a record: no proof.
    pub(crate) fn own_direct(self) -> bool {
        self.holder.is_none() && !self.function_shape && !self.guessed() && self.class.is_none()
    }

    /// Whether the shape is an ahead image's guess (`ic::SYMBOLIC_SHAPE`), never an observation:
    /// its miss asks the VM's caches and goes on, so no fact of the receiver follows from it, and
    /// its id is a symbol only the guard itself emits.
    pub(crate) fn guessed(self) -> bool {
        self.shape & quench_runtime::staged::ic::SYMBOLIC_SHAPE != 0
    }

    /// Whether this is an ahead image's guess of the own slot of a construction that is a class cell (more
    /// fields than inline storage holds): its guard compares the live cell's class with the one the loader
    /// resolves (`ImageWord::Layout`, `symbols::guarded_layout`, `IcOp::GuardImageClass`), then loads the field.
    pub(crate) fn image_layout(self) -> bool {
        self.guessed() && self.holder.is_none() && self.storage.is_none()
            && self.storage_len > quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.inline_property_count
    }

    /// Whether this is an ahead image's guess that reads its slot as one class, the class every store
    /// of the field writes (`aot_image::symbols::GuardAccess::read`), under the slot's dependency.
    pub(crate) fn closed_typed(self) -> bool {
        self.guessed() && self.holder.is_none() && self.ty != quench_runtime::typed_ir::Type::ANY
    }

    pub(crate) fn slot_dependency(self) -> Option<quench_runtime::code_dependencies::SlotDependency> {
        use quench_runtime::typed_ir::Type;
        (self.ty != Type::ANY && self.ty != Type::NEVER).then_some(quench_runtime::code_dependencies::SlotDependency {
            shape: self.holder.unwrap_or(self.shape), slot: self.field_index() as u16, ty: self.ty,
        })
    }

    /// The logical field index the load's slot word names (`field_cache_layout::field_index_of`): a spilled class
    /// field's index in its class's union, else the slot itself.
    pub(crate) fn field_index(self) -> usize {
        quench_runtime::staged::field_cache_layout::field_index_of(self.slot) as usize
    }

    /// The inline slot of a shape-keyed load, which never spills: a record's `u16` field index.
    pub(crate) fn record_slot(self) -> u16 {
        debug_assert!(self.class.is_none() && !quench_runtime::staged::field_cache_layout::is_spilled_slot(self.slot));
        self.slot as u16
    }

    /// Whether a miss of a site's `loads` asks the VM's caches before it leaves: a slot held
    /// by the immediate prototype proves only that prototype, where a deeper holder's read
    /// may answer, and a polymorphic site's other receivers may be cached. A monomorphic
    /// site's inherited constant proves its value exactly; its miss leaves. An ahead image's
    /// shape (`ic::SYMBOLIC_SHAPE`) is the closed world's guess, never an observation: its miss
    /// asks the caches, so a wrong guess costs a compare, never a leave, unless the guess reads its
    /// slot as one class (`closed_typed`), whose miss leaves so the value keeps the class.
    pub(crate) fn probes_misses(loads: &[Self]) -> bool {
        if loads.iter().any(|load| load.guessed() && !load.closed_typed() && !load.image_layout()) { return true; }
        match loads {
            [load] => load.holder.is_some() && load.inherited.is_none(),
            loads => loads.iter().any(|load| load.holder.is_some()),
        }
    }
    pub(crate) fn valid(self) -> bool {
        self.shape & quench_runtime::value_vec::DICTIONARY_STORAGE == 0
            && self.field_index() < self.storage_len
            && (self.storage != Some(PlainPropertyStorage::PackedInline)
                || self.storage_len <= quench_runtime::tier_surface::NATIVE_OBJECT_LAYOUT.inline_property_count)
    }
}

#[cfg(test)]
mod record_tests;

use quench_runtime::staged::ShapeFact;
mod checked;
pub(crate) use checked::CheckedObject;
pub(crate) mod emit;
pub(crate) use emit::Emit;

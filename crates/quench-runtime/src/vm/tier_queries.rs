//! The heap the tiers see (S1.4, private internals): a tier crate reads the heap only through these,
//! never through `Vm::heap` itself. `HeapReads` is a shared view that has no writer, so the heap's
//! representation stays the runtime's own; every write a tier makes is one of the named mutations
//! below, each the heap's own tracked write (its barrier, its shape and store-watch invalidation).

use super::Vm;
use crate::heap::{ArrayElements, ArrayObject, Cell, EnvironmentRecord, FunctionRecord, Heap, MemoryPolicy, Object, ObjectMut, ObjectRef, RootId, TableWords, WasmGcRecord};
use crate::vm::engine_types::InstanceRecord;
use crate::vm::wtf16::JsString;
use crate::Value;
use crate::code_dependencies::{DependentCode, PrototypeChain, PublishedCode, SlotDependency};
use std::rc::Rc;

impl Vm {
    /// The offset of the heap's property buffer word from the VM, which compiled code reads through
    /// the VM pointer it holds (`Heap::PROPERTY_BUFFER_OFFSET`).
    pub const PROPERTY_BUFFER_OFFSET: usize = std::mem::offset_of!(Vm, heap) + Heap::PROPERTY_BUFFER_OFFSET;

    /// Whether a collection is due (`Heap::should_collect`): an allocating tier helper leaves first,
    /// and the interpreter collects at the instruction.
    #[inline(always)]
    pub fn collection_due(&self) -> bool { self.heap.should_collect() }

    /// The function record of `value`, a closure, native or bound function (`Heap::function`): a tier
    /// reads a callee's kind, environment and program through it.
    #[inline(always)]
    pub fn function_record(&self, value: Value) -> Option<&FunctionRecord> { self.heap.function(value) }

    /// The heap as a tier reads it: queries only, no writer.
    #[inline(always)]
    pub fn heap_reads(&self) -> HeapReads<'_> { HeapReads(&self.heap) }
}

/// The heap writes a tier makes, each the heap's own tracked mutation.
impl Vm {
    /// The write barrier of `cell` before a tier's raw stores into it (`Heap::remember_cell`): the
    /// collector then rescans it.
    #[inline(always)]
    pub fn write_barrier(&mut self, cell: Value) { self.heap.remember_cell(cell) }

    /// `Heap::environment_slot_ordinary`: slot `slot` of `environment`, through a shared slot's owner.
    #[inline(always)]
    pub fn environment_slot_ordinary(&self, environment: Value, slot: usize) -> Option<Value> {
        self.heap.environment_slot_ordinary(environment, slot)
    }

    /// `Heap::store_environment_slot_ordinary`: the store to slot `slot` of `environment`, through a shared slot's owner.
    #[inline(always)]
    pub fn store_environment_slot_ordinary(&mut self, environment: Value, slot: usize, value: Value) -> bool {
        self.heap.store_environment_slot_ordinary(environment, slot, value)
    }

    /// A new object of record `object` (`Heap::alloc_object`).
    #[inline(always)]
    pub fn allocate_object(&mut self, object: Object) -> Value { self.heap.alloc_object(object) }

    /// A new object of `proto` and `shape` holding `values` (`Heap::alloc_object_with_properties`).
    #[inline(always)]
    pub fn allocate_object_with_properties(&mut self, proto: Value, shape: u32, values: &[Value]) -> Value {
        self.heap.alloc_object_with_properties(proto, shape, values)
    }

    /// A new heap string (`Heap::alloc_string`).
    #[inline(always)]
    pub fn allocate_string(&mut self, string: JsString) -> Value { self.heap.alloc_string(string) }

    /// A new object of a construct plan's `header` and `prototype` holding the first `len` of `values`
    /// (`Heap::alloc_planned_object`).
    #[inline(always)]
    pub fn allocate_planned_object<const N: usize>(&mut self, header: crate::heap::InlineObjectHeader, prototype: Value,
        values: &[Value; N], len: usize) -> Value {
        self.heap.alloc_planned_object(header, prototype, values, len)
    }

    /// Holds the roots `code` folded into its machine code until the code is dropped (`Heap::hold_folded_roots`).
    #[inline(always)]
    pub fn hold_folded_roots(&mut self, code: &std::rc::Rc<dyn crate::folded_roots::RootedCode>) { self.heap.hold_folded_roots(code) }

    /// A new iterator of record `iterator` (`Heap::alloc_iterator`).
    #[inline(always)]
    pub fn allocate_iterator(&mut self, iterator: crate::heap::IteratorRecord) -> Value { self.heap.alloc_iterator(iterator) }

    /// The record of the iterator `value`, for a library step that advances it (`Heap::iterator_mut`).
    #[inline(always)]
    pub fn iterator_mut(&mut self, value: Value) -> Option<&mut crate::heap::IteratorRecord> { self.heap.iterator_mut(value) }

    /// The cell of `value`, for a library layer (Intl, Temporal) that updates a cell kind it defines
    /// (`Heap::get_mut`). The tiers write through the named mutations above instead.
    #[inline(always)]
    pub fn library_cell_mut(&mut self, value: Value) -> Option<&mut Cell> { self.heap.get_mut(value) }

    /// A new string of `units` (`Heap::alloc_unit_run`).
    #[inline(always)]
    pub fn allocate_unit_run(&mut self, units: crate::vm::wtf16::Units<'_>) -> Value { self.heap.alloc_unit_run(units) }

    /// Charges a materialized text to the collector's pacing (`Heap::charge_materialized_text`).
    #[inline(always)]
    pub fn charge_materialized_text(&mut self) { self.heap.charge_materialized_text() }

    /// A new symbol (`Heap::alloc_symbol`).
    #[inline(always)]
    pub fn allocate_symbol(&mut self, symbol: crate::heap::SymbolData) -> Value { self.heap.alloc_symbol(symbol) }

    /// A new cell (`Heap::alloc`).
    #[inline(always)]
    pub fn allocate_cell(&mut self, cell: Cell) -> Value { self.heap.alloc(cell) }

    /// A root that keeps `value` alive across a tier's collection point (`Heap::root`).
    #[inline(always)]
    pub fn hold_root(&mut self, value: Value) -> RootId { self.heap.root(value) }

    /// Releases a root `hold_root` made (`Heap::release_root`).
    #[inline(always)]
    pub fn drop_root(&mut self, root: RootId) -> bool { self.heap.release_root(root) }

    /// Adds `value` as the next property of `object`, which moves to `shape` (`Heap::property_add`).
    #[inline(always)]
    pub fn add_property(&mut self, object: Value, value: Value, shape: u32) { self.heap.property_add(object, value, shape) }

    /// Stores `value` in property `slot` of `object` (`Heap::property_set`): the store's representation
    /// and watch invalidation are the heap's.
    #[inline(always)]
    pub fn store_property_slot(&mut self, object: Value, slot: usize, value: Value) { self.heap.property_set(object, slot, value) }

    /// Marks `prototype` as some object's prototype (`Heap::mark_prototype`): its writes then invalidate
    /// the facts proved over it.
    #[inline(always)]
    pub fn mark_prototype(&mut self, prototype: Value) { self.heap.mark_prototype(prototype) }

    /// Admits the writes of `shape`'s `slot` as `writes` (`Heap::admit_slot_writes_as`), before a tier
    /// stores raw words of that type.
    #[inline(always)]
    pub fn admit_slot_writes_as(&mut self, shape: u32, slot: usize, writes: crate::typed_ir::Type, raw_double: bool) -> bool {
        self.heap.admit_slot_writes_as(shape, slot, writes, raw_double)
    }

    /// The elements of the record array `value` for an in-place store of `stored`, unlogged
    /// (`Heap::record_array_store_mut_unlogged`).
    #[inline(always)]
    pub fn record_array_store_mut_unlogged(&mut self, value: Value, stored: Value) -> Option<&mut ArrayObject> {
        self.heap.record_array_store_mut_unlogged(value, stored)
    }

    /// Slot `slot` of `environment`, for a store (`Heap::environment_slot_mut`).
    #[inline(always)]
    pub fn environment_slot_mut(&mut self, environment: Value, slot: usize) -> Option<&mut Value> {
        self.heap.environment_slot_mut(environment, slot)
    }

    /// The instance record of the Wasm instance cell `record`, which compiled code enters.
    pub fn wasm_instance_record_mut(&mut self, record: Value) -> Option<&mut InstanceRecord> {
        match self.heap.get_mut(record) { Some(Cell::WasmInstance(record)) => Some(record), _ => None }
    }

    /// The area compiled Wasm code addresses for the binding `binding`: a global's bits or a table's words.
    pub fn wasm_binding_area(&mut self, binding: Value) -> Option<WasmBindingArea> {
        match self.heap.get_mut(binding) {
            Some(Cell::WasmGlobal { bits, .. }) => Some(WasmBindingArea::Global(bits as *mut u64)),
            Some(Cell::WasmTable { elements, .. }) => Some(WasmBindingArea::Table(elements.words() as *const TableWords)),
            _ => None,
        }
    }
}

/// Where compiled Wasm code reads and writes a binding (`Vm::wasm_binding_area`).
pub enum WasmBindingArea {
    /// A global's value bits.
    Global(*mut u64),
    /// A table's words.
    Table(*const TableWords),
}

/// A shared, read-only view of the heap (`Vm::heap_reads`). Each query is the heap's own; there is no
/// method that writes, so a tier holding one cannot change a cell behind an invalidation.
#[derive(Clone, Copy)]
pub struct HeapReads<'a>(&'a Heap);

impl<'a> HeapReads<'a> {
    /// The view of `heap`, for the runtime's own readers.
    #[inline(always)]
    pub(crate) fn of(heap: &'a Heap) -> Self { Self(heap) }
}

macro_rules! heap_reads {
    ($($(#[$doc:meta])* fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        impl<'a> HeapReads<'a> {
            $($(#[$doc])* #[inline(always)] pub fn $name(self, $($arg: $ty),*) -> $ret { self.0.$name($($arg),*) })*
        }
    };
}

heap_reads! {
    /// Property `slot` of `object` (`Heap::property_get`).
    fn property_get(object: &Object, slot: usize) -> Option<Value>;
    /// The store-watch word compiled code compares (`Heap::store_watch_word`).
    fn store_watch_word() -> usize;
    /// The class space's base, which a direct object's granule is added to (`Heap::class_space_base`).
    fn class_space_base() -> usize;
    /// The record id a direct object names (`Heap::record_id`): its stub's header, the record table's index;
    /// `None` for a live class cell, which has no record.
    fn record_id(object: crate::value::ObjectRef32) -> Option<u32>;
    /// The weight allocated since the last collection (`Heap::allocated_weight`).
    fn allocated_weight() -> usize;
    /// An object held as a record (`Heap::record_object_value`).
    fn record_object_value(value: Value) -> Option<&'a Object>;
    /// The array record arena's base (`Heap::array_record_base`).
    fn array_record_base() -> Option<*const ArrayObject>;
    /// The Map and Set record arena's base (`Heap::collection_record_base`).
    fn collection_record_base() -> Option<*const u8>;
    /// The function record arena's base (`Heap::function_record_base`).
    fn function_record_base() -> Option<*const FunctionRecord>;
    /// The string record arena's base (`Heap::string_record_base`).
    fn string_record_base() -> Option<*const JsString>;
    /// The one-unit ASCII strings' base (`Heap::ascii_unit_strings_base`).
    fn ascii_unit_strings_base() -> Option<*const Value>;
    /// The live array records (`Heap::live_array_records`).
    fn live_array_records() -> *const ArrayObject;
    /// The live string records (`Heap::live_string_records`).
    fn live_string_records() -> *const JsString;
    /// The region 0 stub cursor an inline empty object allocation bumps (`Heap::native_stub_cursor`).
    fn native_stub_cursor() -> usize;
    /// The native object allocation cursor (`Heap::native_object_cursor`).
    fn native_object_cursor() -> usize;
    /// The native array allocation cursor (`Heap::native_array_cursor`).
    fn native_array_cursor() -> usize;
    /// The native objects' mark blocks (`Heap::native_object_mark_blocks`).
    fn native_object_mark_blocks() -> (*const u8, usize);
    /// The native functions' mark blocks (`Heap::native_function_mark_blocks`).
    fn native_function_mark_blocks() -> (*const u8, usize);
    /// The native arrays' mark blocks (`Heap::native_array_mark_blocks`).
    fn native_array_mark_blocks() -> (*const u8, usize);
    /// The shapes of function objects (`Heap::function_shapes`).
    fn function_shapes() -> crate::bitset::BitSet;
    /// An iterator's record (`Heap::iterator`).
    fn iterator(value: Value) -> Option<&'a crate::heap::IteratorRecord>;
    /// An async function's await state (`Heap::await_state`).
    fn await_state(value: Value) -> Option<&'a crate::heap::AwaitState>;
    /// Whether `value` is a Set (`Heap::is_set`).
    fn is_set(value: Value) -> bool;
    /// A Set's entries (`Heap::set_entries`).
    fn set_entries(value: Value) -> Option<&'a crate::heap::Entries<Value>>;
    /// Whether `value` is a symbol (`Heap::is_symbol`).
    fn is_symbol(value: Value) -> bool;
    /// Whether `shape`'s `slot` holds raw doubles (`Heap::slot_holds_raw_doubles`).
    fn slot_holds_raw_doubles(shape: u32, slot: usize) -> bool;
    /// A v128 cell's bytes (`Heap::v128`).
    fn v128(value: Value) -> Option<[u8; crate::wasm::V128_BYTES]>;
    /// The object part of a function receiver (`Heap::function_receiver`).
    fn function_receiver(value: Value) -> Option<&'a Object>;
    /// The object part of an array, function or builtin receiver (`Heap::held_other_receiver`).
    fn held_other_receiver(value: Value) -> Option<&'a Object>;
    /// Whether storing `stored` into the array `base` in place needs no store log (`Heap::array_store_unlogged`).
    fn array_store_unlogged(base: Value, stored: Value) -> bool;
    /// Whether `value` is a function (`Heap::is_function_value`).
    fn is_function_value(value: Value) -> bool;
    /// An iterator's record (`Heap::iterator`) is listed above; a Map test (`Heap::is_map`).
    fn is_map(value: Value) -> bool;
    /// The cell of a heap value (`Heap::get`).
    fn get(value: Value) -> Option<&'a Cell>;
    /// The read view of an object (`Heap::object`): the cell-answerable reads, else its record.
    fn object(value: Value) -> Option<ObjectRef<'a>>;
    /// Property `slot` of the object view `object` (`Heap::property_get_ref`).
    fn property_get_ref(object: ObjectRef<'_>, slot: usize) -> Option<Value>;
    /// An ordinary object's record (`Heap::ordinary_object`).
    fn ordinary_object(value: Value) -> Option<&'a Object>;
    /// An object held in the record arena (`Heap::object_record`).
    fn object_record(value: Value) -> Option<&'a Object>;
    /// The record of arena id `record_id` (`Heap::object_record_by_id`).
    fn object_record_by_id(record_id: u32) -> &'a Object;
    /// The record arena's base, for a tier that addresses records in place (`Heap::object_record_base`).
    fn object_record_base() -> Option<*const Object>;
    /// The live record arena (`Heap::live_object_records`).
    fn live_object_records() -> *const Object;
    /// The property value arena's base (`Heap::arena_values_base`).
    fn arena_values_base() -> *const Value;
    /// The native property values (`Heap::native_property_values`).
    fn native_property_values() -> (*const Value, usize);
    /// A string's contents (`Heap::string`).
    fn string(value: Value) -> Option<&'a JsString>;
    /// Whether `value` is a heap string (`Heap::is_string`).
    fn is_string(value: Value) -> bool;
    /// An environment's record (`Heap::environment`).
    fn environment(value: Value) -> Option<&'a EnvironmentRecord>;
    /// An environment reachable from a closure (`Heap::reachable_environment`).
    fn reachable_environment(value: Value) -> Option<&'a EnvironmentRecord>;
    /// An environment slot's value (`Heap::environment_slot`).
    fn environment_slot(environment: Value, slot: usize) -> Option<Value>;
    /// An array's record (`Heap::array`).
    fn array(value: Value) -> Option<&'a ArrayObject>;
    /// A record array (`Heap::record_array`).
    fn record_array(value: Value) -> Option<&'a ArrayObject>;
    /// An array's elements (`Heap::array_elements`).
    fn array_elements(value: Value) -> Option<&'a ArrayElements>;
    /// The memory policy (`Heap::memory_policy`).
    fn memory_policy() -> MemoryPolicy;
    /// The collection epoch (`Heap::collection_epoch`).
    fn collection_epoch() -> u64;
    /// The native layout epoch (`Heap::native_layout_epoch`).
    fn native_layout_epoch() -> u64;
    /// Whether slot representations are tracked (`Heap::tracks_slot_representations`).
    fn tracks_slot_representations() -> bool;
    /// The held type of a tracked store row through a hint (`Heap::tracked_store_held`).
    fn tracked_store_held(layout: u32, slot: u16) -> u8;
    /// The typed view directory (`Heap::typed_view_directory`).
    fn typed_view_directory() -> (*const usize, usize);
    /// The Wasm GC record arena's base (`Heap::wasm_gc_record_base`).
    fn wasm_gc_record_base() -> Option<*const WasmGcRecord>;
}

impl<'a> HeapReads<'a> {
    /// Property `slot` of `object` by its shape (`Heap::property_get_shaped`).
    ///
    /// # Safety
    /// As `Heap::property_get_shaped`: `slot` lies within the object's shape.
    #[inline(always)]
    pub unsafe fn property_get_shaped(self, object: &Object, slot: usize) -> Option<Value> { unsafe { self.0.property_get_shaped(object, slot) } }

    /// The string of record `record_id` (`Heap::string_record_by_id`).
    ///
    /// # Safety
    /// As `Heap::string_record_by_id`: `record_id` names a live string record.
    #[inline(always)]
    pub unsafe fn string_record_by_id(self, record_id: u32) -> &'a JsString { unsafe { self.0.string_record_by_id(record_id) } }
}

/// More heap writes a tier makes, each the heap's own tracked mutation.
impl Vm {
    /// The storage span of the array `base`, its values and readable and writable lengths (`Heap::array_storage_span`).
    #[inline(always)]
    pub fn array_storage_span(&mut self, base: Value) -> Option<(std::ptr::NonNull<Value>, usize, usize)> { self.heap.array_storage_span(base) }
    /// Moves the inline properties of `object` to its spilled storage (`Heap::spill_inline_properties`).
    #[inline(always)]
    pub fn spill_inline_properties(&mut self, object: Value) { self.heap.spill_inline_properties(object) }
    /// A new v128 cell (`Heap::alloc_v128`).
    #[inline(always)]
    pub fn allocate_v128(&mut self, bytes: [u8; crate::wasm::V128_BYTES]) -> Value { self.heap.alloc_v128(bytes) }
    /// A new array of record `object` (`Heap::alloc_array`).
    #[inline(always)]
    pub fn allocate_array(&mut self, object: ArrayObject) -> Value { self.heap.alloc_array(object) }
    /// A new environment and its slots' storage (`Heap::alloc_environment`).
    #[inline(always)]
    pub fn allocate_environment(&mut self, parent: Value, function: u32, slots: crate::heap::EnvironmentSlots,
        scope: crate::heap::EnvironmentScopeRef) -> (Value, *const crate::heap::EnvironmentSlots) {
        self.heap.alloc_environment(parent, function, slots, scope)
    }
    /// A new object of a construct plan's inline layout (`Heap::alloc_planned_inline`).
    #[inline(always)]
    pub fn allocate_planned_inline(&mut self, header: crate::heap::InlineObjectHeader, prototype: Value,
        inline: [Value; crate::value_vec::INLINE_PROPERTY_COUNT], len: usize) -> Value {
        self.heap.alloc_planned_inline(header, prototype, inline, len)
    }
    /// `allocate_planned_inline` where the nursery has room, else none (`Heap::alloc_planned_inline_quick`).
    #[inline(always)]
    pub fn allocate_planned_inline_quick(&mut self, header: crate::heap::InlineObjectHeader,
        inline: [Value; crate::value_vec::INLINE_PROPERTY_COUNT], len: usize) -> Option<Value> {
        self.heap.alloc_planned_inline_quick(header, inline, len)
    }
    /// The record of the environment `value`, for a store (`Heap::environment_mut`).
    #[inline(always)]
    pub fn environment_mut(&mut self, value: Value) -> Option<&mut EnvironmentRecord> { self.heap.environment_mut(value) }
    /// The exclusive view of the object `value`, for a store (`Heap::object_mut`).
    #[inline(always)]
    pub fn object_mut(&mut self, value: Value) -> Option<ObjectMut<'_>> { self.heap.object_mut(value) }
    /// The record of `value` for a record-only edit (`Heap::object_record_mut`): a live class cell departs first.
    #[inline(always)]
    pub fn object_record_mut(&mut self, value: Value) -> Option<&mut Object> { self.heap.object_record_mut(value) }
    /// The exclusive view of the object `value` for a store of `stored` (`Heap::object_store_mut`).
    #[inline(always)]
    pub fn object_store_mut(&mut self, value: Value, stored: Value) -> Option<ObjectMut<'_>> { self.heap.object_store_mut(value, stored) }
    /// Charges `bytes` of payload growth to the collector's pacing (`Heap::charge_payload_growth`).
    #[inline(always)]
    pub fn charge_payload_growth(&mut self, bytes: usize) { self.heap.charge_payload_growth(bytes) }
    /// Whether array runs stay narrow (`Heap::set_narrow_runs`).
    #[inline(always)]
    pub fn set_narrow_runs(&mut self, narrow: bool) { self.heap.set_narrow_runs(narrow) }
    /// Visits every Wasm table's elements for a store (`Heap::for_each_wasm_table_mut`).
    #[inline(always)]
    pub fn for_each_wasm_table_mut(&mut self, visit: impl FnMut(&mut crate::heap::WasmTableElements)) { self.heap.for_each_wasm_table_mut(visit) }
}

/// The fact subscriptions a compiled unit takes when it publishes (`Vm::fact_watch`): the heap's reader
/// tables and the method atoms' readers, and nothing else of the heap. A tier subscribes its code through
/// these methods; each fact it names retires the code when a tracked mutation changes it.
pub struct FactWatch<'a> {
    heap: &'a mut Heap,
    readers: &'a mut crate::code_dependencies::CodeReaders,
}

impl Vm {
    /// The fact subscriptions of a publication, beside the program store that publishes the code.
    #[inline(always)]
    pub fn fact_watch(&mut self) -> (FactWatch<'_>, &crate::vm::program_store::ProgramStore) {
        (FactWatch { heap: &mut self.heap, readers: self.method_cache_atoms.readers() }, &self.programs)
    }
}

impl<'a> FactWatch<'a> {
    /// Retires `code` when a shape departs (`Heap::subscribe_shape`); `false` where the table is full.
    #[inline(always)]
    pub fn subscribe_shape(&mut self, shape: u32, code: &Rc<dyn DependentCode>) -> bool { self.heap.subscribe_shape(shape, code) }
    /// Retires `code` when a slot's observation changes (`Heap::subscribe_slot_observation`).
    #[inline(always)]
    pub fn subscribe_slot_observation(&mut self, dependency: SlotDependency, code: &Rc<dyn DependentCode>) -> bool {
        self.heap.subscribe_slot_observation(dependency, code)
    }
    /// Retires `code` when a prototype of `chain` changes (`Heap::subscribe_prototype_chain`).
    #[inline(always)]
    pub fn subscribe_prototype_chain(&mut self, chain: PrototypeChain, code: &Rc<dyn DependentCode>) -> bool {
        self.heap.subscribe_prototype_chain(chain, code)
    }
    /// Retires `code` when cell `key` widens (`Dependencies::subscribe_cell`); `false` where the table is full.
    #[inline(always)]
    pub fn subscribe_cell(&mut self, key: crate::typed_ir::FactKey, code: &Rc<dyn DependentCode>) -> bool {
        self.heap.dependencies_mut().subscribe_cell(key, code)
    }
    /// Retires `code` when a method cache names `atom` (`CodeReaders::subscribe`).
    #[inline(always)]
    pub fn subscribe_atom(&mut self, atom: u32, code: &Rc<dyn DependentCode>) -> bool { self.readers.subscribe(atom, code) }
    /// Keeps the heap identities `code` embeds alive until it retires (`PublishedCode::hold_roots`).
    #[inline(always)]
    pub fn hold_roots<T: 'static>(&mut self, code: &Rc<PublishedCode<T>>, roots: crate::folded_roots::FoldedRoots) {
        code.hold_roots(roots, self.heap)
    }
    /// The heap and readers, for the runtime's own subscriptions.
    #[inline(always)]
    pub(crate) fn parts(&mut self) -> (&mut Heap, &mut crate::code_dependencies::CodeReaders) { (self.heap, self.readers) }
    /// A watch over `heap` and `readers`, for the runtime's own publications.
    #[inline(always)]
    #[quench_macros::test_visible]
    pub(crate) fn of(heap: &'a mut Heap, readers: &'a mut crate::code_dependencies::CodeReaders) -> Self { Self { heap, readers } }
}

/// The `len` field values of a planned object, each `field(index)`, in a fixed array of `N` with
/// the rest undefined (`Vm::allocate_planned_object`): a loop the caller inlines, not a closure
/// call per slot.
#[inline(always)]
pub fn planned_fields<const N: usize>(len: usize, field: impl Fn(usize) -> Value) -> [Value; N] {
    let mut values = [Value::UNDEFINED; N];
    for (index, value) in values.iter_mut().enumerate().take(len) {
        *value = field(index);
    }
    values
}

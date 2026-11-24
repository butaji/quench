//! The heap the tiers see (S1.4, private internals): a tier crate reads the heap only through these,
//! never through `Vm::heap` itself. `HeapReads` is a shared view that has no writer, so the heap's
//! representation stays the runtime's own and every write goes through a tracked mutation that fires
//! its invalidation.

use super::Vm;
use crate::heap::{ArrayElements, ArrayObject, Cell, EnvironmentRecord, FunctionRecord, Heap, MemoryPolicy, Object, WasmGcRecord};
use crate::vm::wtf16::JsString;
use crate::Value;

impl Vm {
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

/// A shared, read-only view of the heap (`Vm::heap_reads`). Each query is the heap's own; there is no
/// method that writes, so a tier holding one cannot change a cell behind an invalidation.
#[derive(Clone, Copy)]
pub struct HeapReads<'a>(&'a Heap);

macro_rules! heap_reads {
    ($($(#[$doc:meta])* fn $name:ident($($arg:ident: $ty:ty),*) -> $ret:ty;)*) => {
        impl<'a> HeapReads<'a> {
            $($(#[$doc])* #[inline(always)] pub fn $name(self, $($arg: $ty),*) -> $ret { self.0.$name($($arg),*) })*
        }
    };
}

heap_reads! {
    /// The cell of a heap value (`Heap::get`).
    fn get(value: Value) -> Option<&'a Cell>;
    /// An object's record (`Heap::object`).
    fn object(value: Value) -> Option<&'a Object>;
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
    /// The typed view directory (`Heap::typed_view_directory`).
    fn typed_view_directory() -> (*const usize, usize);
    /// The Wasm GC record arena's base (`Heap::wasm_gc_record_base`).
    fn wasm_gc_record_base() -> Option<*const WasmGcRecord>;
}

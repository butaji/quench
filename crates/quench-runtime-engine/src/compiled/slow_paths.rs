//! The slow paths compiled code may call (TOD-2194): the one inventory of the helpers an emitted body
//! reaches outside its own code. A leaf call (`MachineAccess::call_leaf`) names one of these, never an
//! address, so a call to any other code does not type-check.

quench_runtime::slow_paths! {
    /// The engine's slow paths: Wasm memory, table, exception and GC operations the machine rows leave
    /// to the VM, and the bulk memory and SIMD kernels.
    pub(crate) enum EngineSlowPath {
        MemoryFill => super::compiled_memory_fill,
        MemoryCopy => super::compiled_memory_copy,
        SimdKernel => super::compiled_simd_kernel,
        MemoryGrow => super::wasm_memory_stage::memory_grow_callout,
        ExceptionCatch => super::wasm_catch::exception_catch_callout,
        ExceptionNew => super::wasm_exception_stage::exception_new_callout,
        ExceptionMatch => super::wasm_exception_stage::exception_match_callout,
        ExceptionPayload => super::wasm_exception_stage::exception_payload_callout,
        ExceptionThrow => super::wasm_exception_stage::exception_throw_callout,
        SwitchedMemoryLength => super::wasm_instance_switch::switched_memory_length,
        GcAllocate => super::wasm_gc_allocate::gc_allocate,
        TableAccepts => super::wasm_table_stage::compiled_table_accepts,
        TableOp => super::wasm_table_ops_stage::table_op_callout,
        GcFieldSlot => super::wasm_gc_stage::gc_field_slot,
        GcStorageAdmits => super::wasm_gc_stage::gc_storage_admits,
        GcReject => super::wasm_gc_stage::gc_reject,
        GcIsObject => super::wasm_gc_stage::gc_is_object,
        GcFillFields => super::wasm_gc_stage::gc_fill_fields,
        GcFieldsExceed => super::wasm_gc_stage::gc_fields_exceed,
        GcFieldSlotForWrite => super::wasm_gc_stage::gc_field_slot_for_write,
        GcFieldCount => super::wasm_gc_stage::gc_field_count,
        GcCopyFields => super::wasm_gc_stage::gc_copy_fields,
        GcArrayMutable => super::wasm_gc_stage::gc_array_mutable,
        GcAdmits => super::wasm_gc_stage::gc_admits,
    }
}

quench_runtime::entry_sources! {
    /// Where the engine's emitted code reads the words it calls: every call it makes that is no
    /// slow path names one of these.
    pub(crate) enum EngineEntry {
        /// A lane helper of the payload's helper table (`LaneHelpers`, `LaneHelper`).
        LaneHelper,
        /// The Wasm context's call-out to the VM (`CONTEXT_CALLOUT`).
        WasmCallOut,
        /// The Wasm context's indirect or reference call to the VM (`CONTEXT_CALL_INDIRECT`, `CONTEXT_CALL_REFERENCE`).
        WasmTableCallOut,
        /// A compiled Wasm function's entry, from the context's function table.
        WasmFunctionEntry,
        /// A compiled Wasm body's entry, from a table row.
        WasmTableBody,
        /// A compiled unit's entry (`FlowStage::call_entry`).
        CompiledEntry,
    }
}

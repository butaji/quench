#![deny(warnings)]

mod bigint;
mod bytecode;
mod compile;
mod heap;
mod host;
#[cfg(feature = "profile-memory")]
mod memory_edge;
mod module_identity;
mod number_to_string;
mod profile;
pub(crate) use quench_stack as stack;
mod unicode;
mod value;
mod value_vec;
mod vm;
mod wasm;
pub use wasm::{
    WasmCallableType, WasmConstantExpression, WasmData, WasmDataMode, WasmElement, WasmElementMode,
    WasmExport,
    WasmFunction, WasmFunctionBody, WasmFunctionImport, WasmGlobal, WasmGlobalInitializer,
    WasmI32Function, WasmImportName, WasmInstance, WasmMemory, WasmModule,
    WasmReferenceInitializer, WasmReferenceKind, WasmSignature, WasmTable, WasmTableInitializer,
    WasmTag, WasmTrap, WasmType, WasmTypes, WasmValue,
};

pub use bytecode::ResidualProgram;
pub use compile::{Diagnostic, Engine};
pub use heap::{RootId, HeapMemoryComposition};
pub use host::{
    CapabilityId, Host, HostContext, HostExecutionContext, HostGlobal, ModuleSource, SystemHost,
    WasmHostFunctionId, WasmHostValue,
};
#[cfg(feature = "profile-memory")]
pub use memory_edge::report_allocator_memory;
pub use stack::{STACK_BUDGET_BYTES, STACK_HEADROOM_BYTES, WORKER_STACK_SIZE};
pub use value::Value;
pub use vm::{ExecutionMode, JsError, SerializedValue};

/// Serialized values cross to other agents' threads.
const _: fn() = || {
    fn sendable<T: Send>() {}
    sendable::<SerializedValue>();
};

mod api;
pub use api::{
    BufferStorage, BuiltinBrand, ValueType, BuiltinData, CollectionPreview, PromiseSnapshot, ExecutionRequest, HostFunction, HostFunctionId, HostWeakRoot,
    PreparedScript, NativeArithmetic, NativeContext, PromiseRejectionEvent, RootedError, RootedIterator, Runtime,
    CompiledScript, RuntimeError, SourceKind, ErrorSourcePosition, SourceStackFrame, SynchronousModule,
};

#[cfg(test)]
mod host_stack_tests;

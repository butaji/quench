#![deny(warnings)]

mod execution_interrupt;
pub use execution_interrupt::ExecutionInterrupt;
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
    WasmExport, WasmFunction, WasmFunctionBody, WasmFunctionImport, WasmGlobal,
    WasmGlobalInitializer, WasmI32Function, WasmImportName, WasmInstance, WasmMemory, WasmModule,
    WasmReferenceInitializer, WasmReferenceKind, WasmSignature, WasmTable, WasmTableInitializer,
    WasmTag, WasmTrap, WasmType, WasmTypes, WasmValue,
};

pub use bytecode::ResidualProgram;
pub use compile::{Diagnostic, Engine};
pub use heap::{HeapMemoryComposition, RootId};
pub use host::{
    CapabilityId, Host, HostContext, HostExecutionContext, HostGlobal, ModuleSource, SystemHost,
    WasmHostFunctionId, WasmHostValue,
};
#[cfg(feature = "profile-memory")]
pub use memory_edge::report_allocator_memory;
pub use stack::{STACK_BUDGET_BYTES, STACK_HEADROOM_BYTES, WORKER_STACK_SIZE};
pub use value::Value;
pub use vm::{ExecutionMode, JsError};

mod api;
pub use api::{
    BufferStorage, BuiltinBrand, BuiltinData, CollectionPreview, CompiledScript,
    ErrorSourcePosition, ExecutionRequest, HostFunction, HostFunctionId, HostWeakRoot,
    NativeArithmetic, NativeContext, PreparedScript, PromiseRejectionEvent, PromiseSnapshot,
    RootedError, RootedIterator, Runtime, RuntimeError, SourceKind, SourceStackFrame,
    SynchronousModule, ValueType,
};

#[cfg(test)]
mod host_stack_tests;

pub mod owned_clone;

pub mod shared_backing;

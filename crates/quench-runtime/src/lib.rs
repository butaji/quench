#![deny(warnings)]

mod execution_interrupt;
pub use execution_interrupt::ExecutionInterrupt;
mod bigint;
mod bytecode;
mod compile;
mod heap;
mod host;
#[cfg(any(feature = "profile-memory", feature = "profile-allocator"))]
mod memory_edge;
mod module_identity;
mod number_to_string;
mod profile;
mod snapshot;
pub(crate) use quench_stack as stack;
mod unicode;
mod value;
mod value_vec;
mod vm;
mod wasm;
pub use wasm::{
    WasmCallableType, WasmConstantExpression, WasmCustomSection, WasmData, WasmDataMode,
    WasmElement, WasmElementMode, WasmExport, WasmFunction, WasmFunctionBody, WasmFunctionImport,
    WasmGlobal, WasmGlobalInitializer, WasmI32Function, WasmImportName, WasmInstance, WasmMemory,
    WasmModule, WasmReferenceInitializer, WasmReferenceKind, WasmSignature, WasmTable,
    WasmTableInitializer, WasmTag, WasmTrap, WasmType, WasmTypes, WasmValue,
};

pub use bytecode::{KeptBytes, ResidualProgram};
pub use compile::{CompilePhase, Diagnostic, Engine, set_compile_phase_boundary};
pub use heap::{HeapMemoryComposition, MemoryPolicy, RootId};
pub use host::{
    CapabilityId, Host, HostContext, HostExecutionContext, HostGlobal, ModuleLoadError,
    ModuleLoadErrorKind, ModuleSource, SystemHost, WasmHostFunctionId, WasmHostValue,
};
#[cfg(any(feature = "profile-memory", feature = "profile-allocator"))]
pub use memory_edge::report_allocator_memory;
pub use snapshot::{SnapshotReader, SnapshotWriter};
pub use stack::{STACK_BUDGET_BYTES, STACK_HEADROOM_BYTES, WORKER_STACK_SIZE};
pub use value::Value;
pub use vm::{
    ExecutionMode, HostObjectRead, JsError, SerializedValue, SharedWireInput, ValueDeserializer,
    ValueSerializer, WIRE_FORMAT_VERSION, WireInput, WireOutput, write_double, write_raw_bytes,
    write_uint32, write_uint64,
};

/// Serialized values cross to other agents' threads.
const _: fn() = || {
    fn sendable<T: Send>() {}
    sendable::<SerializedValue>();
};

mod api;
pub use api::{
    BufferStorage, BuiltinBrand, BuiltinData, CollectionPreview, CompiledScript, DescriptorFields,
    DeserializerDelegate, ErrorSourcePosition, ExecutionRequest, HostFunction, HostFunctionId,
    HostWeakRoot, NativeArithmetic, NativeContext, PreparedScript, PromiseRejectionEvent,
    PromiseSnapshot, RootScopeMark, RootedError, RootedIterator, Runtime, RuntimeError,
    SerializerDelegate, SourceKind, SourceStackFrame, SynchronousModule, ValueType, wrapper_origin,
};

#[cfg(test)]
mod host_stack_tests;
#[cfg(test)]
mod subset_tests;
#[cfg(test)]
mod test_host;

pub mod owned_clone;

pub mod shared_backing;

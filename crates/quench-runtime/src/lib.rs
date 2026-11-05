#![deny(warnings)]
#![feature(rust_preserve_none_cc)]
// `heap::array_proofs` reads its armed pointer as a plain thread-local load on every array
// storage change (`ARMED_PROOFS`).
#![feature(thread_local)]
#![cfg_attr(all(test, feature = "bench-primitives"), feature(test))]

mod execution_interrupt;
pub use execution_interrupt::ExecutionInterrupt;
#[cfg(unix)]
pub use vm::NativeFaultHandler;
mod bigint;
mod bytecode;
mod compile;
mod code_dependencies;
mod heap;
mod host;
#[cfg(any(feature = "profile-memory", feature = "profile-allocator"))]
mod memory_edge;
mod module_identity;
mod number_to_string;
#[cfg(all(test, feature = "bench-primitives"))]
#[path = "../benches/support/number_inputs.rs"]
mod number_inputs;
mod profile;
mod snapshot;
mod typed_ir;
pub(crate) use quench_stack as stack;
mod unicode;
mod value;
mod value_vec;
mod vm;
mod wasm;
pub use wasm::{
    WasmCallableType, WasmConstantExpression, WasmData, WasmDataMode, WasmElement, WasmElementMode,
    WasmCustomSection, WasmExport,
    WasmFunction, WasmFunctionBody, WasmFunctionImport, WasmGlobal, WasmGlobalInitializer,
    WasmI32Function, WasmImportName, WasmInstance, WasmMemory, WasmModule,
    WasmReferenceInitializer, WasmReferenceKind, WasmSignature, WasmTable, WasmTableInitializer,
    WasmTag, WasmTrap, WasmType, WasmTypes, WasmValue,
};

pub use bytecode::{KeptBytes, ResidualProgram, ModuleRequest, ModuleRequestPhase};
pub use module_identity::normalized_text as normalized_module_name;
pub use compile::{CompilePhase, Diagnostic, Engine, set_compile_phase_boundary};
pub use heap::{HeapMemoryComposition, MemoryPolicy, RootId};
pub use snapshot::{SnapshotReader, SnapshotWriter};
pub use host::{
    CapabilityId, Host, HostContext, HostExecutionContext, HostGlobal, ModuleLoadError, ModuleLoadErrorKind,
    ModuleSource, SystemHost,
    WasmHostFunctionId, WasmHostValue,
};
#[cfg(any(feature = "profile-memory", feature = "profile-allocator"))]
pub use memory_edge::report_allocator_memory;
pub use stack::{STACK_BUDGET_BYTES, STACK_HEADROOM_BYTES, WORKER_STACK_SIZE};
pub use value::Value;
pub use vm::{
    write_double, write_raw_bytes, write_uint32, write_uint64, ExecutionMode, HostObjectRead, JsError, SerializedValue, SharedWireInput,
    ValueDeserializer, ValueSerializer, WireInput, WireOutput, WIRE_FORMAT_VERSION,
};

/// Serialized values cross to other agents' threads.
const _: fn() = || {
    fn sendable<T: Send>() {}
    sendable::<SerializedValue>();
};

mod api;
pub use api::{
    CollectionContext,
    BufferStorage, BuiltinBrand, DescriptorFields, ValueType, BuiltinData, CollectionPreview, PromiseSnapshot, ExecutionRequest, HostCallback, HostFunction, HostFunctionDescriptor, HostFunctionId, HostWeakRoot, IntactBuiltin,
    PreparedScript, NativeArithmetic, NativeContext, PromiseRejectionEvent, RootScopeMark, RootedError, RootedIterator, Runtime,
    CompiledScript, RuntimeError, SourceKind, ErrorSourcePosition, SourceStackFrame, SynchronousModule, ModuleAcceptArguments, ModuleAcceptCallback,
    DeserializerDelegate, SerializerDelegate,
    wrapper_origin,
};

#[cfg(test)]
mod host_stack_tests;
#[cfg(test)]
mod test_host;
#[cfg(test)]
mod subset_tests;

pub mod owned_clone;

pub mod shared_backing;

#[cfg(feature = "native-execution")]
pub use bytecode::{NATIVE_JS_ABI_SYMBOL, NATIVE_JS_LANE_HELPERS_SYMBOL, NATIVE_JS_IMAGE_HEADER_WORDS, NATIVE_JS_OPERATION_PREFIX, NativeProgramNamespace};
#[cfg(feature = "native-execution")]
pub use vm::{
    NativeJsOperands, NativeJsOperationEntry, native_js_lane_helpers, native_js_operation_entry, native_js_operation_entry_for,
    native_program::{NativeJsFunctionEntry, NativeJsProgram, NativeProgramResolver},
};
#[cfg(feature = "aot")]
pub use vm::{
    native_graph::{NativeJsModule, NativeJsBackend, NativeJsCodegenOptions, lower_native_js_module, lower_native_js_module_with_options},
    native_object::{
        NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeData, NativeFunction, NativeFunctionAddress,
        emit_native_module, emit_native_object,
    },
};

#[cfg(feature = "aot")]
pub use compile::{NativeJsCompiler, NativeSourceKey, NativeSourceCompilation};
pub use compile::{DynamicFunctionKind, HostScriptOptions, STATIC_MODULE_GOAL, STATIC_TYPESCRIPT_MODULE_GOAL, GLOBAL_SCRIPT_GOAL, GLOBAL_SCRIPT_SOURCE_NAME};

/// Ordinary builds and producers support dynamic compilation and execution.
/// Execution-only native support requires linked code and archived records.
pub(crate) const DYNAMIC_EXECUTION_ENABLED: bool = cfg!(any(
    not(feature = "native-execution"), feature = "jit", feature = "aot"
));

/// Whether this build requires linked native code and archived program facts.
pub const NATIVE_SUPPORT_IS_EXECUTION_ONLY: bool = !DYNAMIC_EXECUTION_ENABLED;

#[cfg(feature = "native-execution")]
pub use host::NativeSourceError;

mod native_image;
#[cfg(feature = "native-execution")]
pub use native_image::NativeImageSupport;

#[cfg(feature = "aot")]
pub mod native_link;

#![deny(warnings)]
// The v2-merged sync left items both sides added in different places, one copy unused, and AOT items
// named in wider interfaces than their types: allowed until the merge's dead copies are removed.
#![allow(dead_code, private_interfaces, private_bounds)]
#![feature(rust_preserve_none_cc)]
// Lane handlers dispatch their successor with `become`, so every dispatch is a sibling jump
// whatever the handler body keeps on its stack (`vm::dispatch_fast`).
#![cfg_attr(quench_lane_tail_calls, feature(explicit_tail_calls))]
#![cfg_attr(quench_lane_tail_calls, allow(incomplete_features))]
// `heap::array_proofs` reads its armed pointer as a plain thread-local load on every array
// storage change (`ARMED_PROOFS`).
#![feature(thread_local)]
// Staged rows mark their branch closures `#[inline]`, so the interpreter's stage runs them
// in place: a closure is one instantiation per crate, which another unit only calls.
#![feature(stmt_expr_attributes)]
#![cfg_attr(all(test, feature = "bench-primitives"), feature(test))]

#[doc(hidden)]
pub mod execution_interrupt;
/// Numeric facts shared by the runtime and its hosts.
pub mod units {
    pub use quench_temporal::units::*;
    /// The most elements an array holds: its length is a `u32`.
    pub const MAX_ARRAY_LENGTH: usize = u32::MAX as usize;
    pub use crate::integer_conversion::{relative_index, to_index, to_integer_or_infinity, to_length};
    pub use crate::value::number_to_u32 as to_uint32;
    pub use crate::value::number_to_i32 as to_int32;
    pub const MILLISECONDS_PER_SECOND: f64 = 1_000.0;
    pub const BINARY_RADIX: u32 = 2;
    pub const OCTAL_RADIX: u32 = 8;
    pub const DECIMAL_RADIX: u32 = 10;
    pub const HEXADECIMAL_RADIX: u32 = 16;
    /// A fraction's scale as a percentage.
    pub const PERCENT: f64 = 100.0;
    /// The UTF-16 code units of the line terminators text readers split on.
    pub const LINE_FEED: u16 = b'\n' as u16;
    pub const CARRIAGE_RETURN: u16 = b'\r' as u16;
}

/// JavaScript property names the runtime and its hosts both spell.
pub mod property_names {
    pub const LENGTH: &str = "length";
}

pub use vm::value_serializer::DESERIALIZATION_ERROR;
pub use execution_interrupt::ExecutionInterrupt;
#[cfg(unix)]
pub use vm::NativeFaultHandler;
#[doc(hidden)]
pub mod bigint;
#[doc(hidden)]
pub mod bitset;
#[doc(hidden)]
pub mod dataflow;
#[doc(hidden)]
pub mod webs;
#[doc(hidden)]
pub mod dense_map;
#[doc(hidden)]
pub mod bytecode;
#[doc(hidden)]
pub mod compile;
#[doc(hidden)]
pub mod code_dependencies;
#[doc(hidden)]
pub mod folded_roots;
#[doc(hidden)]
pub mod float_layout;
#[doc(hidden)]
pub mod heap;
#[cfg_attr(
    not(all(target_os = "linux", any(target_arch = "x86_64", target_arch = "aarch64"))),
    allow(dead_code)
)]
#[doc(hidden)]
pub mod jit_coverage;
#[doc(hidden)]
pub mod jit_mode;
pub use jit_mode::JitMode;
#[doc(hidden)]
pub mod host;
#[doc(hidden)]
pub mod integer_conversion;
#[cfg(any(feature = "profile-memory", feature = "profile-allocator"))]
#[doc(hidden)]
pub mod memory_edge;
#[doc(hidden)]
pub mod module_identity;
#[doc(hidden)]
pub mod number_to_string;
#[cfg(all(test, feature = "bench-primitives"))]
#[path = "../benches/support/number_inputs.rs"]
#[doc(hidden)]
pub mod number_inputs;
#[doc(hidden)]
pub mod profile;
pub mod trace;
pub mod clock;
#[doc(hidden)]
pub mod snapshot;
#[doc(hidden)]
pub mod staged;
#[doc(hidden)]
pub mod typed_ir;
pub use quench_stack as stack;
#[doc(hidden)]
pub mod unicode;
#[doc(hidden)]
pub mod value;
#[doc(hidden)]
pub mod value_vec;
#[doc(hidden)]
pub mod vm;
#[doc(hidden)]
pub mod wasm;
pub use wasm::{
    WasmCallableType, WasmConstantExpression, WasmData, WasmDataMode, WasmElement, WasmElementItems, WasmElementMode,
    WasmCustomSection, WasmExport,
    WasmFunction, WasmFunctionBody, WasmFunctionImport, WasmGlobal, WasmGlobalInitializer,
    WasmI32Function, WasmImportName, WasmInstance, WasmLazyBody, WasmLazyCode, WasmMemory, WasmModule,
    WasmReferenceInitializer, WasmReferenceKind, WasmSignature, WasmTable, WasmTableInitializer,
    WasmTag, WasmTrap, WasmType, WasmTypes, WasmValue, extern_kind::WasmExternKind,
};

pub use vm::AheadImages;
pub use vm::tier_events::{Event as TierEvent, TierEvents};
pub use bytecode::{set_record_built, set_record_read, KeptBytes, ResidualProgram, ModuleRequest, ModuleRequestPhase, FlowFunction, FlowInstruction};
pub use module_identity::normalized_text as normalized_module_name;
pub use compile::{CompilePhase, Diagnostic, Engine, set_compile_phase_boundary};
pub use compile::{CheckedParameters, ParameterSite};
pub use heap::{HeapMemoryComposition, MemoryPolicy, RootId, memory_mix_has};
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
    local_timezone_changed, write_double, write_raw_bytes, write_uint32, write_uint64, ExecutionMode, CompileFailureKind, HostObjectRead, JsError, SerializedValue, SharedWireInput,
    ValueDeserializer, ValueSerializer, WireInput, WireOutput, WIRE_FORMAT_VERSION,
};

/// Serialized values cross to other agents' threads.
const _: fn() = || {
    fn sendable<T: Send>() {}
    sendable::<SerializedValue>();
};

#[doc(hidden)]
pub mod api;
pub use api::{
    CollectionContext,
    BufferStorage, Detach, BuiltinBrand, DescriptorFields, ValueType, BuiltinData, CollectionPreview, PromiseSnapshot, ExecutionRequest, HostCallback, HostFunction, HostFunctionDescriptor, HostFunctionId, HostWeakRoot, IntactBuiltin,
    PreparedScript, NativeArithmetic, NativeContext, PromiseRejectionEvent, RootScopeMark, RootedError, RootedIterator, Runtime,
    CompiledScript, RuntimeError, SourceKind, ErrorSourcePosition, SourceStackFrame, SynchronousModule, ModuleAcceptArguments, ModuleAcceptCallback,
    DeserializerDelegate, SerializerDelegate,
    wrapper_origin,
};

#[cfg(test)]
#[doc(hidden)]
pub mod host_stack_tests;
#[cfg(test)]
#[doc(hidden)]
pub mod test_host;
#[cfg(test)]
#[doc(hidden)]
pub mod cross_program_call_tests;
#[cfg(test)]
#[doc(hidden)]
pub mod eval_name_scope_tests;
#[cfg(test)]
#[doc(hidden)]
pub mod subset_scope_tests;
#[cfg(test)]
#[doc(hidden)]
pub mod subset_tests;
#[cfg(test)]
#[doc(hidden)]
pub mod string_parts_tests;

pub mod owned_clone;

pub mod shared_backing;

#[cfg(feature = "native-execution")]
pub use bytecode::{NATIVE_JS_ABI_SYMBOL, NATIVE_JS_LANE_HELPERS_SYMBOL, NATIVE_JS_IMAGE_HEADER_WORDS, NATIVE_JS_OPERATION_PREFIX, NativeProgramNamespace};
#[cfg(feature = "native-execution")]
pub use vm::{
    NativeJsOperands, NativeJsOperationEntry, define_lane_helper_symbols, native_js_lane_helpers, native_js_operation_entry, native_js_operation_entry_for,
    native_program::{NativeJsProgram, NativeJsPublicationCounts, NativeProgramResolver},
};
pub use vm::{
    IsaBaseline, NativeTarget,
    native_object::{
        NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeCode, NativeData, NativeDataAddress, NativeFunction, NativeFunctionAddress,
        NativeJsModule, NativeReloc, emit_native_module, emit_native_module_for, emit_native_object, lower_native_js_module,
        lower_native_js_module_for,
    },
};

pub use compile::{NativeJsCompiler, NativeSourceKey, NativeSourceCompilation, SourceDemands};
pub use compile::{DynamicFunctionKind, HostScriptOptions, STATIC_MODULE_GOAL, STATIC_TYPESCRIPT_MODULE_GOAL, GLOBAL_SCRIPT_GOAL, GLOBAL_SCRIPT_SOURCE_NAME};

/// Ordinary builds and producers (with the JIT) support dynamic compilation and execution.
/// Execution-only native support (a static AOT image's, built without the JIT) requires
/// linked code and archived records.
pub(crate) const DYNAMIC_EXECUTION_ENABLED: bool = cfg!(feature = "jit");

/// Whether this build requires linked native code and archived program facts.
pub const NATIVE_SUPPORT_IS_EXECUTION_ONLY: bool = !DYNAMIC_EXECUTION_ENABLED;

#[cfg(feature = "native-execution")]
pub use host::NativeSourceError;

#[doc(hidden)]
pub mod native_image;
#[cfg(feature = "native-execution")]
pub use native_image::NativeImageSupport;

pub mod native_link;

// Library tests run with the layer crates compiled in as modules: a layer crate names this crate
// `quench_runtime` and its own root `crate`, so both resolve here (`quench-layers` links the
// layer crates into every other build).
#[cfg(test)]
extern crate self as quench_runtime;
#[cfg(test)]
#[path = "../../quench-runtime-intl/src/lib.rs"]
mod intl_layer;
#[cfg(test)]
use intl_layer::*;
#[cfg(test)]
#[path = "../../quench-runtime-engine/src/lib.rs"]
mod engine_layer;
#[cfg(test)]
use engine_layer::*;

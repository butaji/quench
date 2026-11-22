//! The interpreter's lane handlers (`dispatch_fast`) and the compiled tiers (`compiled`,
//! `js_compiled`, `jit_backend`): a layer crate above the VM core (`quench_runtime::vm::layer_hooks`).
//! They hold most of the runtime's generated code, so they build in their own crate.
#![deny(warnings)]
#![allow(dead_code, private_interfaces, private_bounds)]
#![feature(rust_preserve_none_cc)]
#![cfg_attr(quench_lane_tail_calls, feature(explicit_tail_calls))]
#![cfg_attr(quench_lane_tail_calls, allow(incomplete_features))]
#![feature(thread_local)]
#![feature(stmt_expr_attributes)]

pub mod dispatch_fast;
pub mod compiled;
pub mod js_compiled;
pub mod aot_image;
pub mod native_program;
pub mod native_object;
pub mod regexp_native;
mod engine_calls;
mod engine_calls2;
mod lifted;
mod symbol_words;

/// What every layer module sees: the VM core, the layer's modules and its extension traits.
#[allow(unused_imports)]
pub(crate) mod engine_prelude {
    pub use crate::lifted::*;
    pub use quench_runtime::vm::*;
    // The names the VM core's modules import (`quench-runtime/src/vm.rs`), which its submodules saw.
    #[cfg(feature = "native-execution")]
    pub use quench_runtime::vm::native_operands::NativeJsOperands;
    #[cfg(unix)]
    pub use quench_runtime::vm::engine_types::NativeFaultHandler;
    pub use quench_runtime::vm::jit_backend::{IsaBaseline, NativeTarget};
    pub use quench_runtime::vm::engine_types::{AheadImages, ExecutionMode};
    pub use quench_runtime::vm::engine_types::InstanceRecord;
    pub use quench_runtime::vm::date::local_timezone_changed;
    pub use quench_runtime::vm::wasm_exception::{js_tag_cell, js_tag_type};
    pub use quench_runtime::Value;
    pub use quench_runtime::bytecode::{Atom, AtomTable, Constant, DispatchClass, FieldBase, Instr, LEXICAL_THIS_BINDING, NEW_TARGET_BINDING, Op, Operand, Register, ResidualProgram, WideInstruction};
    pub use quench_runtime::staged::add_plan::{AddSite, Transition, concrete_add_property, concrete_set_function_name};
    #[cfg(feature = "profile-memory")]
    pub use quench_runtime::heap::CellKind;
    pub use quench_runtime::heap::{ CallSiteLocation, CallSiteName, CallSiteRecord, Cell, CollectionStorage, FunctionKind, FunctionRecord, GcTraceKind, IteratorRecord, Heap, IteratorConsumer, IteratorHelper, IteratorKind, Native, Object, ProtoLink, ProxyKind, RootId, StackData, TypedArrayKind, WeakHandle, };
    pub use quench_runtime::host::{CapabilityId, Host, HostContext};
    pub use quench_runtime::profile::{CallSource, Profile, TerminalCall};
    pub use quench_runtime::value::number_to_u32;
    pub use quench_runtime::vm::activation::{CallCompletion, CallContext, ConstructionTarget};
    pub use quench_runtime::vm::atomics::Test262AgentState;
    pub use rustc_hash::{FxHashMap, FxHashSet};
    pub use std::cell::OnceCell;
    pub use std::hash::{Hash, Hasher};
    pub use std::rc::Rc;
    pub use crate::dispatch_fast;
    pub use crate::compiled;
    pub use crate::js_compiled;
    pub use crate::aot_image;
    pub use crate::native_program;
    pub use crate::native_object;
    pub use crate::regexp_native;
    pub use crate::native_program::NativeProgramVm;
    pub use crate::dispatch_fast::DispatchFastVm;
    pub use crate::dispatch_fast::DispatchFastVm2;
    pub use crate::dispatch_fast::DispatchFastVm3;
    pub use crate::dispatch_fast::DispatchFastVm4;
    pub use crate::dispatch_fast::DispatchFastVm5;
    pub use crate::dispatch_fast::DispatchFastVm6;
    pub use crate::dispatch_fast::DispatchFastVm7;
    pub use crate::dispatch_fast::proven_callees::DispatchFastProvenCalleesVm;
    pub use crate::dispatch_fast::window_owner::DispatchFastWindowOwnerVm;
    pub use crate::dispatch_fast::lane_stack_trace::DispatchFastLaneStackTraceVm;
    pub use crate::dispatch_fast::root_constants::DispatchFastRootConstantsVm;
    pub use crate::dispatch_fast::native_frames::DispatchFastNativeFramesVm;
    pub use crate::dispatch_fast::in_place_natives::DispatchFastInPlaceNativesVm;
    pub use crate::dispatch_fast::with_scopes::DispatchFastWithScopesVm;
    pub use crate::dispatch_fast::on_demand_metadata::DispatchFastOnDemandMetadataVm;
    pub use crate::dispatch_fast::scoped_names::DispatchFastScopedNamesVm;
    pub use crate::dispatch_fast::lane_roots::DispatchFastLaneRootsVm;
    pub use crate::dispatch_fast::setters::DispatchFastSettersVm;
    pub use crate::dispatch_fast::generator_yields::DispatchFastGeneratorYieldsVm;
    pub use crate::dispatch_fast::mapped_windows::DispatchFastMappedWindowsVm;
    pub use crate::dispatch_fast::proxy_traps::DispatchFastProxyTrapsVm;
    pub use crate::dispatch_fast::closure_targets::DispatchFastClosureTargetsVm;
    pub use crate::compiled::CompiledVm;
    pub use crate::compiled::CompiledVm2;
    pub use crate::compiled::CompiledVm3;
    pub use crate::compiled::CompiledVm4;
    pub use crate::compiled::CompiledVm5;
    pub use crate::compiled::lane_sites::CompiledLaneSitesVm;
    pub use crate::compiled::base_body::CompiledBaseBodyVm;
    pub use crate::compiled::lane_compile::CompiledLaneCompileVm;
    pub use crate::compiled::lane_compile::CompiledLaneCompileVm2;
    pub use crate::compiled::ahead_images::CompiledAheadImagesVm;
    pub use crate::compiled::wasm_table_ops_stage::CompiledWasmTableOpsStageVm;
    pub use crate::compiled::instance_context::CompiledInstanceContextVm;
    pub use crate::compiled::lane_body_run::CompiledLaneBodyRunVm;
    pub use crate::compiled::osr_compile::calls::CompiledOsrCompileCallsVm;
}

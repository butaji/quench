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
#[cfg(feature = "jit")]
pub mod regexp_native;
pub(crate) mod engine_calls;
pub(crate) mod engine_calls2;
pub(crate) mod lifted;
pub(crate) mod symbol_words;

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
    pub use quench_runtime::tier_surface::CellKind;
    pub use quench_runtime::tier_surface::{ CallSiteLocation, CallSiteName, CallSiteRecord, Cell, CollectionStorage, FunctionKind, FunctionRecord, GcTraceKind, IteratorRecord, IteratorConsumer, IteratorHelper, IteratorKind, Native, Object, ProtoLink, ProxyKind, RootId, StackData, TypedArrayKind, WeakHandle, };
    pub use quench_runtime::host::{CapabilityId, Host, HostContext};
    pub use quench_runtime::profile::{CallSource, Profile, TerminalCall};
    pub use quench_runtime::value::number_to_u32;
    pub use quench_runtime::vm::activation::{CallCompletion, CallContext, ConstructionTarget};
    pub use quench_runtime::vm::atomics::Test262AgentState;
    pub use rustc_hash::{FxHashMap, FxHashSet};
    pub use std::cell::OnceCell;
    pub use std::hash::{Hash, Hasher};
    pub use std::rc::Rc;
    pub use quench_runtime::units::MAX_SAFE_INTEGER;
    pub use quench_runtime::bytecode::MAX_ARRAY_LENGTH;
    pub use quench_runtime::vm::pending_job::PendingJob;
    pub use quench_runtime::vm::atom_set::{AtomSet, IndexAtoms};
    pub use quench_runtime::vm::call_printer::{freeze_call_sites, freeze_source_call_sites};
    pub use quench_runtime::vm::engine_types::{copy_arguments, fill_window as fill_registers};
    pub use quench_runtime::vm::group_by::GroupByKind;
    pub use quench_runtime::vm::property_definition::PropertyDefinitionKind;
    pub use quench_runtime::vm::call_arguments::CallArguments;
    pub use quench_runtime::vm::object::AtomClass;
    pub use quench_runtime::vm::json::json_literal_length;
    pub use quench_runtime::vm::object_static::array_index;
    pub use quench_runtime::vm::program_store::ProgramId;
    pub use quench_runtime::vm::regexp::RegExpFlags;
    pub use quench_runtime::vm::program_store::{ModuleImport, ProgramStore};
    pub use quench_runtime::vm::promise::PromiseRuntime;
    pub use quench_runtime::vm::property_key::PropertyKey;
    pub use quench_runtime::vm::root_bindings::RootBinding;
    pub use quench_runtime::vm::var_owner::{EvalCaller, VarOwner};
    pub use quench_runtime::vm::well_known_symbols::{WellKnownSymbol, WellKnownSymbols};
    pub use quench_runtime::vm::error::{CompileFailureKind, JsError};
    pub use quench_runtime::vm::value_serializer::{ write_double, write_raw_bytes, write_uint32, write_uint64, HostObjectRead, SerializedValue, SharedWireInput, ValueDeserializer, ValueSerializer, WireInput, WireOutput, WIRE_FORMAT_VERSION, };
    pub use quench_runtime::vm::value_serializer::{ReadHooks, WriteHooks};
    pub use quench_runtime::vm::wtf16::JsString;
    pub use quench_runtime::staged::field_cache_layout::{ FieldCacheWords};
    pub use quench_runtime::tier_surface::{INHERITED_FIELD_CACHE_MAX_DEPTH, InheritedFieldAnswer, InheritedFieldCache};
    pub use quench_runtime::staged::layout_offsets::NATIVE_FIELD_CACHE_LAYOUT;
    pub use quench_runtime::vm::engine_types::AHEAD_BUILD_ID_PREFIX;
    pub use crate::compiled::AHEAD_BUILD_ID;
    pub use quench_runtime::vm::activation::{JobResume, JobRoot, TrapAccess, TrapCheck};
    pub use quench_runtime::vm::activation::frame_record::FrameRecord;
    pub use quench_runtime::vm::dispatch_frame::PendingGeneralCall;
    pub use quench_runtime::vm::lane_completion::LaneCompletion;
    pub use quench_runtime::vm::proxy::ProxyTrap;
    pub use crate::dispatch_fast;
    pub use crate::compiled;
    pub use crate::js_compiled;
    pub use crate::aot_image;
    pub use crate::native_program;
    pub use crate::native_object;
    #[cfg(feature = "jit")]
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

/// Every extension trait of the engine, which each engine module imports: the methods the
/// engine adds to the core's types read as inherent ones.
#[allow(unused_imports)]
pub(crate) mod ext_traits {
    pub use crate::aot_image::AotImageOps1 as _;
    pub use crate::aot_image::base::BaseImageOps1 as _;
    pub use crate::aot_image::bound::BoundCallsOps1 as _;
    pub use crate::aot_image::live::LinkedTableOps1 as _;
    pub use crate::compiled::CompiledVm as _;
    pub use crate::compiled::CallContextOps1 as _;
    pub use crate::compiled::CompiledProgramOps1 as _;
    pub use crate::compiled::EntryStampOps1 as _;
    pub use crate::compiled::CompiledOsrRegionOps1 as _;
    pub use crate::compiled::LaneMethodOps1 as _;
    pub use crate::compiled::LaneExitOps1 as _;
    pub use crate::compiled::CompiledVm2 as _;
    pub use crate::compiled::CompiledVm3 as _;
    pub use crate::compiled::JsStepDispatchOps1 as _;
    pub use crate::compiled::CompiledJsFunctionOps1 as _;
    pub use crate::compiled::CompiledVm4 as _;
    pub use crate::compiled::CompiledVm5 as _;
    pub use crate::compiled::DirectTierEntryOps1 as _;
    pub use crate::compiled::ahead_images::AheadImagesOps1 as _;
    pub use crate::compiled::ahead_images::CompiledAheadImagesVm as _;
    pub use crate::compiled::base_body::LaneMethodOps2 as _;
    pub use crate::compiled::base_body::LaneMethodOps3 as _;
    pub use crate::compiled::base_body::CompiledBaseBodyVm as _;
    pub use crate::compiled::base_region::resume::ResumeTableOps1 as _;
    pub use crate::compiled::base_region::sites::CompiledBaseRegionSitesVm as _;
    pub use crate::compiled::base_region::span::LiveOps1 as _;
    pub use crate::compiled::base_region::span::SpanOps1 as _;
    pub use crate::compiled::code_heap::BlockOps1 as _;
    pub use crate::compiled::code_heap::CodeMemoryOps1 as _;
    pub use crate::compiled::code_heap::views::ViewsOps1 as _;
    #[cfg(target_os = "linux")] pub use crate::compiled::code_heap::views::ViewsOps2 as _;
    #[cfg(target_vendor = "apple")] pub use crate::compiled::code_heap::views::ViewsOps3 as _;
    pub use crate::compiled::direct_callee::DirectCalleeOps1 as _;
    pub use crate::compiled::entry_cell::EntryCellOps1 as _;
    pub use crate::compiled::exit_protocol::ExitKindOps1 as _;
    pub use crate::compiled::exit_protocol::LaneExitOps2 as _;
    pub use crate::compiled::exit_protocol::LoopRegionOps3 as _;
    pub use crate::compiled::exit_protocol::ExitTableOps1 as _;
    pub use crate::compiled::fault_handler::NativeFaultHandlerOps1 as _;
    pub use crate::compiled::frameless::CalleeAdmissionsOps1 as _;
    pub use crate::compiled::indirect_profile::SiteHitsOps1 as _;
    pub use crate::compiled::indirect_profile::CompiledProgramOps2 as _;
    pub use crate::compiled::instance_context::CallContextOps2 as _;
    pub use crate::compiled::instance_context::InstanceContextOps1 as _;
    pub use crate::compiled::instance_context::InstanceRecordOps1 as _;
    pub use crate::compiled::instance_context::CompiledProgramOps3 as _;
    pub use crate::compiled::instance_context::CompiledInstanceContextVm as _;
    pub use crate::compiled::instance_context::NativeRootsOps1 as _;
    pub use crate::compiled::lane_body_run::CompiledLaneBodyRunVm as _;
    pub use crate::compiled::lane_compile::CompiledLaneCompileVm as _;
    pub use crate::compiled::lane_compile::CompiledLaneCompileVm2 as _;
    pub use crate::compiled::lane_compile::PendingLaneMethodOps1 as _;
    pub use crate::compiled::lane_compile::LaneMethodOps4 as _;
    pub use crate::compiled::lane_compile::dependencies::LaneDependenciesOps1 as _;
    pub use crate::compiled::lane_region::ElementViewsOps1 as _;
    pub use crate::compiled::lane_region::kept_shapes::StateOps2 as _;
    pub use crate::compiled::lane_region::scalar::CompletionOps3 as _;
    pub use crate::compiled::lane_sites::CompiledLaneSitesVm as _;
    pub use crate::compiled::lane_storage::JsStepDispatchOps2 as _;
    pub use crate::compiled::lane_unit::CallTargetOps1 as _;
    pub use crate::compiled::lane_unit::InlinedCallOps1 as _;
    pub use crate::compiled::numeric_image::NumericImageOps1 as _;
    pub use crate::compiled::object_slots::InheritedConstantOps1 as _;
    pub use crate::compiled::object_slots::NativeFieldLoadOps1 as _;
    pub use crate::compiled::osr_compile::OsrFactsOps2 as _;
    pub use crate::compiled::osr_compile::PendingOsrRegionOps1 as _;
    pub use crate::compiled::osr_compile::calls::CompiledOsrCompileCallsVm as _;
    pub use crate::compiled::osr_compile::calls::CallCodeOps1 as _;
    pub use crate::compiled::osr_loop::LoopRegionOps1 as _;
    pub use crate::compiled::osr_loop::RegionAnalysisFailureOps1 as _;
    pub use crate::compiled::osr_loop::FieldPathOps1 as _;
    pub use crate::compiled::osr_loop::FieldStoreOps1 as _;
    pub use crate::compiled::osr_loop::InliningOps1 as _;
    pub use crate::compiled::osr_loop::LoopRegionOps2 as _;
    pub use crate::compiled::osr_loop::function::LoopRegionOps4 as _;
    pub use crate::compiled::osr_loop::receivers::FieldReceiverOps1 as _;
    pub use crate::compiled::wasm_table_ops_stage::CompiledWasmTableOpsStageVm as _;
    pub use crate::dispatch_fast::LaneInstructionOps1 as _;
    pub use crate::dispatch_fast::LoopSiteOps1 as _;
    pub use crate::dispatch_fast::LoopSiteOps2 as _;
    pub use crate::dispatch_fast::IndirectSlotOps1 as _;
    pub use crate::dispatch_fast::LaneViewOps1 as _;
    pub use crate::dispatch_fast::ActivationShapeOps1 as _;
    pub use crate::dispatch_fast::LaneRecoverySpaceOps1 as _;
    pub use crate::dispatch_fast::LaneViewOps2 as _;
    pub use crate::dispatch_fast::RegistersOps1 as _;
    pub use crate::dispatch_fast::SlotOps1 as _;
    pub use crate::dispatch_fast::MemoryViewOps1 as _;
    pub use crate::dispatch_fast::ActivationOps1 as _;
    pub use crate::dispatch_fast::ActivationOps2 as _;
    pub use crate::dispatch_fast::ActivationOps3 as _;
    pub use crate::dispatch_fast::CompletionOps1 as _;
    pub use crate::dispatch_fast::ScriptCallTargetOps1 as _;
    pub use crate::dispatch_fast::AdmissionOps1 as _;
    pub use crate::dispatch_fast::ScriptCallTargetOps2 as _;
    pub use crate::dispatch_fast::ScriptTargetsOps1 as _;
    pub use crate::dispatch_fast::LaneStackOps1 as _;
    pub use crate::dispatch_fast::LaneStackOps2 as _;
    pub use crate::dispatch_fast::DispatchFastVm as _;
    pub use crate::dispatch_fast::DispatchFastVm2 as _;
    pub use crate::dispatch_fast::DispatchFastVm3 as _;
    pub use crate::dispatch_fast::DispatchFastVm4 as _;
    pub use crate::dispatch_fast::DispatchFastVm5 as _;
    pub use crate::dispatch_fast::MemoLeafOps1 as _;
    pub use crate::dispatch_fast::MemoReadOps1 as _;
    pub use crate::dispatch_fast::ForOfTestOps1 as _;
    pub use crate::dispatch_fast::DispatchFastVm6 as _;
    pub use crate::dispatch_fast::PreparedShapeOps1 as _;
    pub use crate::dispatch_fast::CallbackTrampolineOps1 as _;
    pub use crate::dispatch_fast::PreparedEntryOps1 as _;
    pub use crate::dispatch_fast::PreparedShapeOps2 as _;
    pub use crate::dispatch_fast::DispatchFastVm7 as _;
    pub use crate::dispatch_fast::body_tier_in_lane::LaneViewOpsX1 as _;
    pub use crate::dispatch_fast::body_tier_in_lane::specialize::LaneViewSpecializeOps as _;
    pub use crate::dispatch_fast::call_entry::CallEntryOps1 as _;
    pub use crate::dispatch_fast::call_entry::CallEntryOps2 as _;
    pub use crate::dispatch_fast::call_entry::CallEntryOps3 as _;
    pub use crate::dispatch_fast::closure_targets::DispatchFastClosureTargetsVm as _;
    pub use crate::dispatch_fast::construct_plan::ConstructPlanOps1 as _;
    pub use crate::dispatch_fast::construct_plan::nested::NestedFieldOps1 as _;
    pub use crate::dispatch_fast::construct_plan::nested::ConstructPlanOps2 as _;
    pub use crate::dispatch_fast::frame_walk::LaneStackOps5 as _;
    pub use crate::dispatch_fast::generator_windows::ActivationOps4 as _;
    pub use crate::dispatch_fast::generator_windows::ActivationViewOps2 as _;
    pub use crate::dispatch_fast::generator_windows::LaneStackOps8 as _;
    pub use crate::dispatch_fast::generator_windows::WindowPlanOps1 as _;
    pub use crate::dispatch_fast::generator_yields::DispatchFastGeneratorYieldsVm as _;
    pub use crate::dispatch_fast::in_place_natives::DispatchFastInPlaceNativesVm as _;
    pub use crate::dispatch_fast::lane_body_stats::LaneBodyStatsOps1 as _;
    pub use crate::dispatch_fast::lane_eval::LaneStackOps10 as _;
    pub use crate::dispatch_fast::lane_frames::ActivationViewOps1 as _;
    pub use crate::dispatch_fast::lane_frames::SidePayloadOps1 as _;
    pub use crate::dispatch_fast::lane_frames::SideOps1 as _;
    pub use crate::dispatch_fast::lane_frames::LaneStackOps3 as _;
    pub use crate::dispatch_fast::lane_leaves::LaneLeavesOps1 as _;
    pub use crate::dispatch_fast::lane_roots::LaneRootOps1 as _;
    pub use crate::dispatch_fast::lane_roots::LaneContextOps1 as _;
    pub use crate::dispatch_fast::lane_roots::DispatchFastLaneRootsVm as _;
    pub use crate::dispatch_fast::lane_stack_growth::LaneStackOps4 as _;
    pub use crate::dispatch_fast::lane_stack_trace::DispatchFastLaneStackTraceVm as _;
    pub use crate::dispatch_fast::lane_trace::LaneStackOps12 as _;
    pub use crate::dispatch_fast::loop_body::LoopBodyOps1 as _;
    pub use crate::dispatch_fast::loop_body::LoopSiteOps3 as _;
    pub use crate::dispatch_fast::loop_body::LoopBodyOps2 as _;
    pub use crate::dispatch_fast::loop_kernel::LoopKernelOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::machine::MachineOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::recur::CodeOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::recur::RecurrenceOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::recur::FoldOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::recur::ChainOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::recur::FusionOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::recur::WordsOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::run::StateOps1 as _;
    pub use crate::dispatch_fast::loop_kernel::run::LoopKernelOps2 as _;
    pub use crate::dispatch_fast::loop_runs::LoopSiteRunsOps as _;
    pub use crate::dispatch_fast::loop_tier::LoopTierOps1 as _;
    pub use crate::dispatch_fast::mapped_windows::DispatchFastMappedWindowsVm as _;
    pub use crate::dispatch_fast::native_frames::DispatchFastNativeFramesVm as _;
    pub use crate::dispatch_fast::native_frames::LaneStackOps6 as _;
    pub use crate::dispatch_fast::on_demand_metadata::DispatchFastOnDemandMetadataVm as _;
    pub use crate::dispatch_fast::operand_feedback::OperandKindsOps1 as _;
    pub use crate::dispatch_fast::operand_feedback::RecordsOps1 as _;
    pub use crate::dispatch_fast::operand_feedback::LaneViewOpsX2 as _;
    pub use crate::dispatch_fast::proven_callees::DispatchFastProvenCalleesVm as _;
    pub use crate::dispatch_fast::proxy_traps::DispatchFastProxyTrapsVm as _;
    pub use crate::dispatch_fast::scoped_names::DispatchFastScopedNamesVm as _;
    pub use crate::dispatch_fast::setters::DispatchFastSettersVm as _;
    pub use crate::dispatch_fast::string_case_filter::StringCaseFilterOps1 as _;
    pub use crate::dispatch_fast::string_case_filter::derivation::StringCaseFilterOpsX3 as _;
    pub use crate::dispatch_fast::this_binding::ThisBindingOps1 as _;
    pub use crate::dispatch_fast::vector_region::VectorRegionOps1 as _;
    pub use crate::dispatch_fast::window_owner::DispatchFastWindowOwnerVm as _;
    pub use crate::dispatch_fast::with_scopes::LaneStackOps7 as _;
    pub use crate::dispatch_fast::with_scopes::DispatchFastWithScopesVm as _;
    pub use crate::lifted::AheadImagesLifted160 as _;
    pub use crate::lifted::LaneViewLifted648 as _;
    pub use crate::native_program::NativeJsProgramOps1 as _;
    pub use crate::native_program::NativeProgramVm as _;
    pub use crate::dispatch_fast::operand_feedback::OperandFeedbackVm as _;
}

/// What the executable and tools call: native object emission for AOT images, the image bundle,
/// and the native fault handler.
pub mod api {
    pub use crate::native_object::{
        NATIVE_DATA_NAMESPACE, NATIVE_FUNCTION_NAMESPACE, NativeCode, NativeData, NativeDataAddress, NativeFunction, NativeFunctionAddress,
        NativeJsModule, NativeReloc, emit_native_module, emit_native_module_for, emit_native_object, lower_native_js_module,
        lower_native_js_module_for,
    };
    pub use crate::compiled::ahead_images::AheadImagesOps1;
    pub use crate::compiled::fault_handler::NativeFaultHandlerOps1;
    pub use crate::native_program::NativeJsProgramOps1;
}

//! The engine's definitions of the core's calls into it (`quench_runtime::vm::engine_calls`).
#![allow(clippy::too_many_arguments)]
#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::engine_prelude::*;
use quench_runtime::vm::engine_types::*;
use cranelift_codegen::ir;
use quench_runtime::vm::engine_types::CallContext;

#[unsafe(no_mangle)]
fn quench_engine_vm_install_wasm_instance(this: &mut Vm, env: Value, program: ProgramId) -> Option<()> { <Vm as CompiledInstanceContextVm>::install_wasm_instance(this, env, program) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_install_wasm_instance = quench_engine_vm_install_wasm_instance;
#[unsafe(no_mangle)]
fn quench_engine_vm_bind_wasm_instance(this: &mut Vm, env: Value, program: ProgramId) -> Option<()> { <Vm as CompiledInstanceContextVm>::bind_wasm_instance(this, env, program) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_bind_wasm_instance = quench_engine_vm_bind_wasm_instance;
#[unsafe(no_mangle)]
fn quench_engine_vm_run_compiled(this: &mut Vm, program: ProgramId, env: Value, id: u32, args: &[Value]) -> Option<Result<Value, JsError>> { <Vm as CompiledVm4>::run_compiled(this, program, env, id, args) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_run_compiled = quench_engine_vm_run_compiled;
#[unsafe(no_mangle)]
fn quench_engine_vm_materialize_deferred_lane(this: &mut Vm) -> () { <Vm as DispatchFastVm2>::materialize_deferred_lane(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_materialize_deferred_lane = quench_engine_vm_materialize_deferred_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_wasm_instance_address(this: &Vm, env: Value, program: ProgramId) -> Option<usize> { <Vm as CompiledInstanceContextVm>::wasm_instance_address(this, env, program) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_wasm_instance_address = quench_engine_vm_wasm_instance_address;
#[unsafe(no_mangle)]
fn quench_engine_vm_running_owner(this: &mut Vm) -> VarOwner { <Vm as DispatchFastWindowOwnerVm>::running_owner(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_running_owner = quench_engine_vm_running_owner;
#[unsafe(no_mangle)]
fn quench_engine_vm_offer_compiled(this: &mut Vm, caller: usize, id: u32) -> Option<std::rc::Rc<CompiledProgram>> { <Vm as CompiledVm4>::offer_compiled(this, caller, id) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_offer_compiled = quench_engine_vm_offer_compiled;
#[unsafe(no_mangle)]
fn quench_engine_vm_run_offered(this: &mut Vm, compiled: std::rc::Rc<CompiledProgram>, caller: usize, id: u32, window: quench_runtime::bytecode::RegisterWindow) -> Option<Result<Value, JsError>> { <Vm as CompiledVm4>::run_offered(this, compiled, caller, id, window) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_run_offered = quench_engine_vm_run_offered;
#[unsafe(no_mangle)]
fn quench_engine_vm_js_jit_compiled_call(this: &mut Vm, program: &ResidualProgram, arg1: (ProgramId, u32), returns: bool, arguments: &[Value]) -> Option<Value> { <Vm as CompiledVm4>::js_jit_compiled_call(this, program, arg1, returns, arguments) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_js_jit_compiled_call = quench_engine_vm_js_jit_compiled_call;
#[unsafe(no_mangle)]
fn quench_engine_vm_admits_in_lane(this: &Vm, program: ProgramId, code: &ResidualProgram) -> bool { <Vm as DispatchFastVm2>::admits_in_lane(this, program, code) }
#[unsafe(no_mangle)]
fn quench_engine_vm_may_run_in_lane(this: &Vm, program: ProgramId, code: &ResidualProgram) -> bool { <Vm as DispatchFastVm2>::may_run_in_lane(this, program, code) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_admits_in_lane = quench_engine_vm_admits_in_lane;
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_may_run_in_lane = quench_engine_vm_may_run_in_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_entry_lane_view(this: &Vm, program: ProgramId, function: u32) -> *const LaneView { <Vm as DispatchFastVm6>::entry_lane_view(this, program, function) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_entry_lane_view = quench_engine_vm_entry_lane_view;
#[unsafe(no_mangle)]
fn quench_engine_vm_hot_loop_lane_view(this: &Vm, program: ProgramId, function: u32) -> *const LaneView { <Vm as DispatchFastVm2>::hot_loop_lane_view(this, program, function) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_hot_loop_lane_view = quench_engine_vm_hot_loop_lane_view;
#[unsafe(no_mangle)]
fn quench_engine_vm_runs_call_in_lane(view: *const LaneView) -> bool { <Vm as DispatchFastVm6>::runs_call_in_lane(view) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_runs_call_in_lane = quench_engine_vm_runs_call_in_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_tier_may_run(this: &Vm, tiers: &[crate::compiled::admission::Tier]) -> bool { crate::compiled::admission::vm_may_run(this, tiers) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_tier_may_run = quench_engine_vm_tier_may_run;
#[unsafe(no_mangle)]
fn quench_engine_vm_view_any_in_lane(this: &Vm, program: ProgramId, function: u32) -> Option<*const LaneView> { <Vm as DispatchFastVm2>::view_any_in_lane(this, program, function) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_view_any_in_lane = quench_engine_vm_view_any_in_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_runs_in_lane(view: *const LaneView, pc: usize) -> bool { <Vm as DispatchFastVm6>::runs_in_lane(view, pc) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_runs_in_lane = quench_engine_vm_runs_in_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_run_fast_lane(this: &mut Vm, code: &ResidualProgram, program: ProgramId, view: *const LaneView, frame: &mut usize, pc: usize, pending: &mut Vec<PendingGeneralCall>, executing: &mut quench_runtime::vm::dispatch_frame::ExecutingProgram, inline_calls: bool) -> usize {
    #[cfg(feature = "jit")]
    { <Vm as DispatchFastVm6>::run_fast_lane(this, code, program, view, frame, pc, pending, executing, inline_calls) }
    // An execution-only build runs the image's body at the entry, not the records (`run_ahead_frame`).
    #[cfg(not(feature = "jit"))]
    { <Vm as crate::ahead_runner::AheadRunnerVm>::run_ahead_frame(this, code, program, view, frame, pc, pending, executing, inline_calls) }
}
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_run_fast_lane = quench_engine_vm_run_fast_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_js_tier_frame_entry(this: &mut Vm, program: &ResidualProgram, frame: usize) -> Option<usize> { <Vm as CompiledVm4>::js_tier_frame_entry(this, program, frame) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_js_tier_frame_entry = quench_engine_vm_js_tier_frame_entry;
#[unsafe(no_mangle)]
fn quench_engine_vm_trap_result_error(this: &mut Vm, p: &ResidualProgram, pending: &PendingGeneralCall, frame: usize, value: Value) -> Option<(u32, JsError)> { <Vm as DispatchFastProxyTrapsVm>::trap_result_error(this, p, pending, frame, value) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_trap_result_error = quench_engine_vm_trap_result_error;
#[unsafe(no_mangle)]
fn quench_engine_vm_completed_call_value(this: &Vm, completion: &CallCompletion, caller: usize, destination: quench_runtime::bytecode::Register, value: Value) -> Value { <Vm as DispatchFastSettersVm>::completed_call_value(this, completion, caller, destination, value) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_completed_call_value = quench_engine_vm_completed_call_value;
#[unsafe(no_mangle)]
fn quench_engine_vm_record_native_execution_rejection(this: &mut Vm) -> () { <Vm as CompiledVm4>::record_native_execution_rejection(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_record_native_execution_rejection = quench_engine_vm_record_native_execution_rejection;
#[unsafe(no_mangle)]
fn quench_engine_vm_runs_windows_in_lane(this: &Vm) -> bool { <Vm as DispatchFastVm2>::runs_windows_in_lane(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_runs_windows_in_lane = quench_engine_vm_runs_windows_in_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_wasm_table_step(this: &mut Vm, f: usize, i: WideInstruction) -> Result<(), JsError> { <Vm as CompiledWasmTableOpsStageVm>::wasm_table_step(this, f, i) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_wasm_table_step = quench_engine_vm_wasm_table_step;
#[unsafe(no_mangle)]
fn quench_engine_vm_lane_stack_sites(this: &mut Vm) -> Option<LaneSites> { <Vm as DispatchFastLaneStackTraceVm>::lane_stack_sites(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_lane_stack_sites = quench_engine_vm_lane_stack_sites;
#[unsafe(no_mangle)]
fn quench_engine_vm_resolve_native_program(this: &mut Vm, name: &str, parts: &[&str]) -> Result<Option<NativeJsProgram>, quench_runtime::NativeSourceError> { <Vm as NativeProgramVm>::resolve_native_program(this, name, parts) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_resolve_native_program = quench_engine_vm_resolve_native_program;
#[unsafe(no_mangle)]
fn quench_engine_vm_prepare_native_script(this: &mut Vm, native: NativeJsProgram) -> Result<quench_runtime::PreparedScript, JsError> { <Vm as NativeProgramVm>::prepare_native_script(this, native) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_prepare_native_script = quench_engine_vm_prepare_native_script;
#[unsafe(no_mangle)]
fn quench_engine_vm_attach_native_js(this: &mut Vm, id: ProgramId, code: &NativeJsCode) -> Result<(), String> { <Vm as NativeProgramVm>::attach_native_js(this, id, code) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_attach_native_js = quench_engine_vm_attach_native_js;
#[unsafe(no_mangle)]
fn quench_engine_vm_innermost_lane_state(this: &Vm) -> [Value; INNERMOST_STATE_LEN] { <Vm as DispatchFastLaneRootsVm>::innermost_lane_state(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_innermost_lane_state = quench_engine_vm_innermost_lane_state;
#[unsafe(no_mangle)]
fn quench_engine_vm_run_lane_job(this: &mut Vm, code: &ResidualProgram, job: JobRoot, frame: *const FrameRecord, resume: JobResume) -> Result<JobEnd, JobResume> { <Vm as DispatchFastLaneRootsVm>::run_lane_job(this, code, job, frame, resume) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_run_lane_job = quench_engine_vm_run_lane_job;
#[unsafe(no_mangle)]
fn quench_engine_vm_specialized_operations_enabled(this: &Vm, program: &ResidualProgram) -> bool { <Vm as DispatchFastVm2>::specialized_operations_enabled(this, program) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_specialized_operations_enabled = quench_engine_vm_specialized_operations_enabled;
#[unsafe(no_mangle)]
fn quench_engine_vm_call_stored_setter(this: &mut Vm, p: &ResidualProgram, site: u16, setter: Value, object: Value, value: Value) -> Result<Value, JsError> { <Vm as DispatchFastSettersVm>::call_stored_setter(this, p, site, setter, object, value) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_call_stored_setter = quench_engine_vm_call_stored_setter;
#[unsafe(no_mangle)]
fn quench_engine_vm_enter_with_scope(this: &mut Vm) -> () { <Vm as DispatchFastWithScopesVm>::enter_with_scope(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_enter_with_scope = quench_engine_vm_enter_with_scope;
#[unsafe(no_mangle)]
fn quench_engine_vm_store_native_module_program(this: &mut Vm, native: NativeJsProgram) -> Result<ProgramId, JsError> { <Vm as NativeProgramVm>::store_native_module_program(this, native) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_store_native_module_program = quench_engine_vm_store_native_module_program;
#[unsafe(no_mangle)]
fn quench_engine_vm_kept_native_module_program(this: &mut Vm, module: &quench_runtime::ModuleSource) -> Result<Option<ProgramId>, JsError> { <Vm as NativeProgramVm>::kept_native_module_program(this, module) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_kept_native_module_program = quench_engine_vm_kept_native_module_program;
#[unsafe(no_mangle)]
fn quench_engine_vm_run_chained_lane_job(this: &mut Vm, code: &ResidualProgram, arg1: (JobRoot, *const FrameRecord), resume: JobResume, pending: &mut Vec<PendingGeneralCall>) -> Result<JobEnd, JobResume> { <Vm as DispatchFastLaneRootsVm>::run_chained_lane_job(this, code, arg1, resume, pending) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_run_chained_lane_job = quench_engine_vm_run_chained_lane_job;
#[cfg(feature = "jit")]
#[unsafe(no_mangle)]
fn quench_engine_vm_report_lane_plan(this: &mut Vm) -> () { <Vm as DispatchFastVm3>::report_lane_plan(this) }
// An execution-only build derives no lane view: there is no plan to report.
#[cfg(not(feature = "jit"))]
#[unsafe(no_mangle)]
fn quench_engine_vm_report_lane_plan(_this: &mut Vm) -> () {}
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_report_lane_plan = quench_engine_vm_report_lane_plan;
#[unsafe(no_mangle)]
fn quench_engine_vm_rebind_lane_field_entries(this: &Vm) -> () { <Vm as CompiledVm3>::rebind_lane_field_entries(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_rebind_lane_field_entries = quench_engine_vm_rebind_lane_field_entries;
#[unsafe(no_mangle)]
fn quench_engine_vm_finish_lane_callback(this: &mut Vm, prepared: PreparedLaneCallback) -> () { <Vm as DispatchFastVm7>::finish_lane_callback(this, prepared) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_finish_lane_callback = quench_engine_vm_finish_lane_callback;
#[unsafe(no_mangle)]
fn quench_engine_vm_call_prepared_lane(this: &mut Vm, prepared: &mut PreparedLaneCallback, args: &[Value]) -> Option<Result<Value, JsError>> { <Vm as DispatchFastVm7>::call_prepared_lane(this, prepared, args) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_call_prepared_lane = quench_engine_vm_call_prepared_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_prepare_lane_callback(this: &mut Vm, callee: Value, receiver: Value) -> Option<PreparedLaneCallback> { <Vm as DispatchFastVm7>::prepare_lane_callback(this, callee, receiver) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_prepare_lane_callback = quench_engine_vm_prepare_lane_callback;
#[unsafe(no_mangle)]
fn quench_engine_vm_native_callback_in_lane(this: &mut Vm, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>> { <Vm as DispatchFastVm6>::native_callback_in_lane(this, callee, receiver, args) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_native_callback_in_lane = quench_engine_vm_native_callback_in_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_callee_runs_beside_activations(this: &Vm, callee: Value) -> bool { <Vm as DispatchFastVm2>::callee_runs_beside_activations(this, callee) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_callee_runs_beside_activations = quench_engine_vm_callee_runs_beside_activations;
#[unsafe(no_mangle)]
fn quench_engine_vm_deferred_callback_in_lane(this: &mut Vm, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>> { <Vm as DispatchFastVm6>::deferred_callback_in_lane(this, callee, receiver, args) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_deferred_callback_in_lane = quench_engine_vm_deferred_callback_in_lane;
#[unsafe(no_mangle)]
fn quench_engine_vm_execution_mode(this: &Vm) -> ExecutionMode { <Vm as CompiledVm4>::execution_mode(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_execution_mode = quench_engine_vm_execution_mode;
#[unsafe(no_mangle)]
fn quench_engine_vm_load_ahead_images(this: &mut Vm, images: AheadImages) -> () { <Vm as CompiledAheadImagesVm>::load_ahead_images(this, images) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_load_ahead_images = quench_engine_vm_load_ahead_images;
#[unsafe(no_mangle)]
fn quench_engine_vm_stage_images_at_load(this: &mut Vm, id: ProgramId, program: &ResidualProgram) -> () { <Vm as CompiledAheadImagesVm>::stage_images_at_load(this, id, program) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_stage_images_at_load = quench_engine_vm_stage_images_at_load;
#[unsafe(no_mangle)]
fn quench_engine_vm_set_execution_mode(this: &mut Vm, mode: ExecutionMode) -> () { <Vm as CompiledVm4>::set_execution_mode(this, mode) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_set_execution_mode = quench_engine_vm_set_execution_mode;
#[unsafe(no_mangle)]
fn quench_engine_vm_enable_lane_methods(this: &mut Vm) -> () { <Vm as CompiledVm4>::enable_lane_methods(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_enable_lane_methods = quench_engine_vm_enable_lane_methods;
#[unsafe(no_mangle)]
fn quench_engine_vm_compile_wasm_eagerly(this: &mut Vm) -> () {
    use crate::compiled::admission::{specialize, UnitFacts, UnitKind, UnitRequest};
    let facts = UnitFacts { wasm_first_entry: <Vm as CompiledVm4>::wasm_compile_at_first_entry(this), ..UnitFacts::default() };
    if let Some(unit) = specialize(UnitRequest { kind: UnitKind::WasmEager, facts }).compiled() { <Vm as CompiledVm4>::compile_wasm_eagerly(this, unit) }
}
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_compile_wasm_eagerly = quench_engine_vm_compile_wasm_eagerly;
#[unsafe(no_mangle)]
fn quench_engine_vm_native_execution_rejections(this: &Vm) -> u64 { <Vm as CompiledVm4>::native_execution_rejections(this) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_native_execution_rejections = quench_engine_vm_native_execution_rejections;
#[unsafe(no_mangle)]
fn quench_engine_vm_set_native_program_resolver_boxed(this: &mut Vm, resolver: Box<NativeProgramResolver>) -> () { <Vm as NativeProgramVm>::set_native_program_resolver_boxed(this, resolver) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_set_native_program_resolver_boxed = quench_engine_vm_set_native_program_resolver_boxed;
#[unsafe(no_mangle)]
fn quench_engine_vm_execute_native_program(this: &mut Vm, native: &NativeJsProgram, drain_jobs: bool) -> Result<Value, JsError> { <Vm as NativeProgramVm>::execute_native_program(this, native, drain_jobs) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_execute_native_program = quench_engine_vm_execute_native_program;
#[unsafe(no_mangle)]
fn quench_engine_vm_execute_native_after_scripts(this: &mut Vm, scripts: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError> { <Vm as NativeProgramVm>::execute_native_after_scripts(this, scripts, native) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_execute_native_after_scripts = quench_engine_vm_execute_native_after_scripts;
#[unsafe(no_mangle)]
fn quench_engine_vm_execute_native_module_graph(this: &mut Vm, scripts: &[NativeJsProgram], modules: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError> { <Vm as NativeProgramVm>::execute_native_module_graph(this, scripts, modules, native) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_execute_native_module_graph = quench_engine_vm_execute_native_module_graph;
#[unsafe(no_mangle)]
fn quench_engine_fn_flush_retired_writers() -> () { crate::compiled::code_heap::retire::flush() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_flush_retired_writers = quench_engine_fn_flush_retired_writers;
#[unsafe(no_mangle)]
fn quench_engine_fn_direct_memory(vm: &Vm, binding: Value) -> Option<(*mut u8, usize)> { crate::dispatch_fast::direct_memory(vm, binding) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_direct_memory = quench_engine_fn_direct_memory;
#[unsafe(no_mangle)]
fn quench_engine_fn_in_place_in_loop(native: Native) -> bool { crate::dispatch_fast::in_place_in_loop(native) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_in_place_in_loop = quench_engine_fn_in_place_in_loop;
#[unsafe(no_mangle)]
fn quench_engine_fn_note_int_speculation_retired() -> bool { crate::dispatch_fast::int_kinds::note_retired() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_note_int_speculation_retired = quench_engine_fn_note_int_speculation_retired;
#[unsafe(no_mangle)]
fn quench_engine_fn_retire_running_lane_context() -> () { crate::dispatch_fast::retire_running_lane_context() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_retire_running_lane_context = quench_engine_fn_retire_running_lane_context;
#[unsafe(no_mangle)]
fn quench_engine_fn_names_dead_callee(inlined: &[InlinedCall], direct: &[DirectCallee], live: &dyn Fn(Value) -> bool) -> bool { crate::compiled::direct_callee::names_dead_callee(inlined, direct, live) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_names_dead_callee = quench_engine_fn_names_dead_callee;
#[unsafe(no_mangle)]
fn quench_engine_fn_lane_miss_report(events: &quench_runtime::vm::tier_events::TierEvents) -> String { crate::dispatch_fast::lane_body_stats::miss_report(events) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_lane_miss_report = quench_engine_fn_lane_miss_report;
#[unsafe(no_mangle)]
fn quench_engine_fn_lane_unlowered_report() -> String { crate::dispatch_fast::lane_body_stats::unlowered_report() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_lane_unlowered_report = quench_engine_fn_lane_unlowered_report;
#[unsafe(no_mangle)]
fn quench_engine_fn_census() -> (u64, u64, u64) { crate::compiled::base_region::census() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_census = quench_engine_fn_census;
#[unsafe(no_mangle)]
fn quench_engine_fn_jit_wasm_selective_enabled() -> bool { crate::compiled::jit_wasm_selective_enabled() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_jit_wasm_selective_enabled = quench_engine_fn_jit_wasm_selective_enabled;
#[unsafe(no_mangle)]
fn quench_engine_fn_candidate_rejection_reason(program: &ResidualProgram, function_id: u32) -> &'static str { crate::js_compiled::candidate_rejection_reason(program, function_id) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_candidate_rejection_reason = quench_engine_fn_candidate_rejection_reason;
#[unsafe(no_mangle)]
fn quench_engine_fn_settle_name_record(view: &LaneView, pc: usize, op: Op, lands: bool) -> () { crate::dispatch_fast::name_probe::settle_name_record(view, pc, op, lands) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_settle_name_record = quench_engine_fn_settle_name_record;
#[unsafe(no_mangle)]
fn quench_engine_fn_native_js_lane_helpers() -> *const () { crate::compiled::native_js_lane_helpers() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_native_js_lane_helpers = quench_engine_fn_native_js_lane_helpers;
#[unsafe(no_mangle)]
fn quench_engine_fn_native_js_operation_entry_for(opcode: usize) -> Option<NativeJsOperationEntry> { crate::compiled::native_js_operation_entry_for(opcode) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_native_js_operation_entry_for = quench_engine_fn_native_js_operation_entry_for;
#[unsafe(no_mangle)]
fn quench_engine_fn_declare_regions(function: &mut ir::Function) -> () { crate::compiled::regions::declare_regions(function) }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_declare_regions = quench_engine_fn_declare_regions;

#[unsafe(no_mangle)]
fn quench_engine_vm_nested_lane_run(this: &mut Vm, innermost: [Value; 2], run: &mut dyn FnMut(&mut Vm)) {
    <Vm as crate::engine_prelude::DispatchFastLaneRootsVm>::nested_lane_run(this, innermost, |vm| run(vm))
}
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_nested_lane_run = quench_engine_vm_nested_lane_run;
#[unsafe(no_mangle)]
fn quench_engine_vm_restore_native_snapshot(this: &mut Vm, bytes: quench_runtime::KeptBytes, native: &NativeJsProgram,
    read_host: Box<dyn FnOnce(&mut dyn Host, &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> + '_>) -> Result<(), JsError> {
    <Vm as crate::engine_prelude::NativeProgramVm>::restore_native_snapshot(this, bytes, native, read_host)
}
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_vm_restore_native_snapshot = quench_engine_vm_restore_native_snapshot;
#[unsafe(no_mangle)]
fn quench_engine_fn_define_lane_helper_symbols() { crate::compiled::define_lane_helper_symbols() }
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_define_lane_helper_symbols = quench_engine_fn_define_lane_helper_symbols;
#[cfg(feature = "native-execution")]
#[unsafe(no_mangle)]
unsafe fn quench_engine_fn_js_semantic_callout_single(operation: Option<StaticJsOperation>, context: *mut CallContext, function: u64, arguments: *const u64, result: *mut u64) -> i64 {
    // SAFETY: forwarded from `native_js_operation_entry`'s contract.
    unsafe { crate::compiled::js_semantic_callout_with::<true>(operation, context, function, arguments, result) }
}
#[cfg(feature = "native-execution")]
const _: quench_runtime::vm::engine_calls::signatures::quench_engine_fn_js_semantic_callout_single = quench_engine_fn_js_semantic_callout_single;

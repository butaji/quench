//! Calls from the VM core into its engine layer (`quench-runtime-engine`: the lane handlers and
//! compiled tiers). Each is a link-time symbol the engine defines (`#[unsafe(no_mangle)]`), checked
//! against `signatures` there, so a signature change fails the engine's build. Generated from the
//! engine's definitions; the core sees the methods through `EngineVm`, which the engine itself does
//! not import, so its own calls stay direct.
#![allow(clippy::too_many_arguments)]
use super::*;
use super::engine_types::*;
use super::engine_types::CallContext;
use super::activation::{FrameRecord, JobRoot, JobResume};
use super::dispatch_frame::PendingGeneralCall;

unsafe extern "Rust" {
    safe fn quench_engine_vm_nested_lane_run(this: &mut Vm, innermost: [Value; 2], run: &mut dyn FnMut(&mut Vm));
    safe fn quench_engine_vm_restore_native_snapshot(this: &mut Vm, bytes: crate::KeptBytes, native: &NativeJsProgram,
        read_host: Box<dyn FnOnce(&mut dyn Host, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String> + '_>) -> Result<(), JsError>;
    safe fn quench_engine_fn_define_lane_helper_symbols();
    #[cfg(feature = "native-execution")]
    unsafe fn quench_engine_fn_js_semantic_callout_single(operation: Option<StaticJsOperation>, context: *mut CallContext, function: u64, arguments: *const u64, result: *mut u64) -> i64;
    pub(super) safe fn quench_engine_vm_install_wasm_instance(this: &mut Vm, env: Value, program: ProgramId) -> Option<()>;
    pub(super) safe fn quench_engine_vm_bind_wasm_instance(this: &mut Vm, env: Value, program: ProgramId) -> Option<()>;
    pub(super) safe fn quench_engine_vm_run_compiled(this: &mut Vm, program: ProgramId, env: Value, id: u32, args: &[Value]) -> Option<Result<Value, JsError>>;
    safe fn quench_engine_vm_materialize_deferred_lane(this: &mut Vm) -> ();
    pub(super) safe fn quench_engine_vm_wasm_instance_address(this: &Vm, env: Value, program: ProgramId) -> Option<usize>;
    safe fn quench_engine_vm_running_owner(this: &mut Vm) -> VarOwner;
    pub(super) safe fn quench_engine_vm_offer_compiled(this: &mut Vm, caller: usize, id: u32) -> Option<std::rc::Rc<CompiledProgram>>;
    pub(super) safe fn quench_engine_vm_run_offered(this: &mut Vm, compiled: std::rc::Rc<CompiledProgram>, caller: usize, id: u32, window: crate::bytecode::RegisterWindow) -> Option<Result<Value, JsError>>;
    pub(super) safe fn quench_engine_vm_js_jit_compiled_call(this: &mut Vm, program: &ResidualProgram, arg1: (ProgramId, u32), returns: bool, arguments: &[Value]) -> Option<Value>;
    safe fn quench_engine_vm_admits_in_lane(this: &Vm, program: ProgramId, code: &ResidualProgram) -> bool;
    safe fn quench_engine_vm_may_run_in_lane(this: &Vm, program: ProgramId, code: &ResidualProgram) -> bool;
    safe fn quench_engine_vm_entry_lane_view(this: &Vm, program: ProgramId, function: u32) -> *const LaneView;
    pub(super) safe fn quench_engine_vm_hot_loop_lane_view(this: &Vm, program: ProgramId, function: u32) -> *const LaneView;
    safe fn quench_engine_vm_runs_call_in_lane(view: *const LaneView) -> bool;
    pub(super) safe fn quench_engine_vm_tier_may_run(this: &Vm, tiers: &[crate::jit_mode::Tier]) -> bool;
    safe fn quench_engine_vm_view_any_in_lane(this: &Vm, program: ProgramId, function: u32) -> Option<*const LaneView>;
    safe fn quench_engine_vm_runs_in_lane(view: *const LaneView, pc: usize) -> bool;
    pub(super) safe fn quench_engine_vm_run_fast_lane(this: &mut Vm, code: &ResidualProgram, program: ProgramId, view: *const LaneView, frame: &mut usize, pc: usize, pending: &mut Vec<PendingGeneralCall>, executing: &mut crate::vm::dispatch_frame::ExecutingProgram, inline_calls: bool) -> usize;
    pub(super) safe fn quench_engine_vm_js_tier_frame_entry(this: &mut Vm, program: &ResidualProgram, frame: usize) -> Option<usize>;
    safe fn quench_engine_vm_trap_result_error(this: &mut Vm, p: &ResidualProgram, pending: &PendingGeneralCall, frame: usize, value: Value) -> Option<(u32, JsError)>;
    safe fn quench_engine_vm_completed_call_value(this: &Vm, completion: &CallCompletion, caller: usize, destination: crate::bytecode::Register, value: Value) -> Value;
    pub(super) safe fn quench_engine_vm_record_native_execution_rejection(this: &mut Vm) -> ();
    safe fn quench_engine_vm_runs_windows_in_lane(this: &Vm) -> bool;
    pub(super) safe fn quench_engine_vm_wasm_table_step(this: &mut Vm, f: usize, i: WideInstruction) -> Result<(), super::JsError>;
    safe fn quench_engine_vm_lane_stack_sites(this: &mut Vm) -> Option<LaneSites>;
    safe fn quench_engine_vm_resolve_native_program(this: &mut Vm, name: &str, parts: &[&str]) -> Result<Option<NativeJsProgram>, crate::NativeSourceError>;
    pub(super) safe fn quench_engine_vm_prepare_native_script(this: &mut Vm, native: NativeJsProgram) -> Result<crate::PreparedScript, JsError>;
    pub(super) safe fn quench_engine_vm_attach_native_js(this: &mut Vm, id: ProgramId, code: &NativeJsCode) -> Result<(), String>;
    safe fn quench_engine_vm_innermost_lane_state(this: &Vm) -> [Value; INNERMOST_STATE_LEN];
    safe fn quench_engine_vm_run_lane_job(this: &mut Vm, code: &ResidualProgram, job: JobRoot, frame: *const FrameRecord, resume: JobResume) -> Result<JobEnd, JobResume>;
    safe fn quench_engine_vm_specialized_operations_enabled(this: &Vm, program: &ResidualProgram) -> bool;
    safe fn quench_engine_vm_call_stored_setter(this: &mut Vm, p: &ResidualProgram, site: u16, setter: Value, object: Value, value: Value) -> Result<Value, JsError>;
    safe fn quench_engine_vm_enter_with_scope(this: &mut Vm) -> ();
    pub(super) safe fn quench_engine_vm_store_native_module_program(this: &mut Vm, native: NativeJsProgram) -> Result<ProgramId, JsError>;
    pub(super) safe fn quench_engine_vm_kept_native_module_program(this: &mut Vm, module: &crate::ModuleSource) -> Result<Option<ProgramId>, JsError>;
    safe fn quench_engine_vm_run_chained_lane_job(this: &mut Vm, code: &ResidualProgram, arg1: (JobRoot, *const FrameRecord), resume: JobResume, pending: &mut Vec<PendingGeneralCall>) -> Result<JobEnd, JobResume>;
    pub(super) safe fn quench_engine_vm_report_lane_plan(this: &mut Vm) -> ();
    safe fn quench_engine_vm_rebind_lane_field_entries(this: &Vm) -> ();
    safe fn quench_engine_vm_finish_lane_callback(this: &mut Vm, prepared: PreparedLaneCallback) -> ();
    safe fn quench_engine_vm_call_prepared_lane(this: &mut Vm, prepared: &mut PreparedLaneCallback, args: &[Value]) -> Option<Result<Value, JsError>>;
    safe fn quench_engine_vm_prepare_lane_callback(this: &mut Vm, callee: Value, receiver: Value) -> Option<PreparedLaneCallback>;
    safe fn quench_engine_vm_native_callback_in_lane(this: &mut Vm, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>>;
    safe fn quench_engine_vm_callee_runs_beside_activations(this: &Vm, callee: Value) -> bool;
    safe fn quench_engine_vm_deferred_callback_in_lane(this: &mut Vm, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>>;
    safe fn quench_engine_vm_execution_mode(this: &Vm) -> ExecutionMode;
    pub(super) safe fn quench_engine_vm_load_ahead_images(this: &mut Vm, images: AheadImages) -> ();
    pub(super) safe fn quench_engine_vm_stage_images_at_load(this: &mut Vm, id: ProgramId, program: &ResidualProgram) -> ();
    safe fn quench_engine_vm_set_execution_mode(this: &mut Vm, mode: ExecutionMode) -> ();
    safe fn quench_engine_vm_enable_lane_methods(this: &mut Vm) -> ();
    pub(super) safe fn quench_engine_vm_compile_wasm_eagerly(this: &mut Vm) -> ();
    pub(super) safe fn quench_engine_vm_native_execution_rejections(this: &Vm) -> u64;
    safe fn quench_engine_vm_set_native_program_resolver_boxed(this: &mut Vm, resolver: Box<NativeProgramResolver>) -> ();
    safe fn quench_engine_vm_execute_native_program(this: &mut Vm, native: &NativeJsProgram, drain_jobs: bool) -> Result<Value, JsError>;
    safe fn quench_engine_vm_execute_native_after_scripts(this: &mut Vm, scripts: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError>;
    safe fn quench_engine_vm_execute_native_module_graph(this: &mut Vm, scripts: &[NativeJsProgram], modules: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError>;
    safe fn quench_engine_fn_flush_retired_writers() -> ();
    safe fn quench_engine_fn_direct_memory(vm: &Vm, binding: Value) -> Option<(*mut u8, usize)>;
    safe fn quench_engine_fn_in_place_in_loop(native: Native) -> bool;
    safe fn quench_engine_fn_retire_running_lane_context() -> ();
    safe fn quench_engine_fn_names_dead_callee(inlined: &[InlinedCall], direct: &[DirectCallee], live: &dyn Fn(Value) -> bool) -> bool;
    safe fn quench_engine_fn_lane_miss_report(events: &crate::vm::tier_events::TierEvents) -> String;
    safe fn quench_engine_fn_lane_unlowered_report() -> String;
    safe fn quench_engine_fn_census() -> (u64, u64, u64);
    safe fn quench_engine_fn_jit_wasm_selective_enabled() -> bool;
    safe fn quench_engine_fn_candidate_rejection_reason(program: &ResidualProgram, function_id: u32) -> &'static str;
    safe fn quench_engine_fn_settle_name_record(view: &LaneView, pc: usize, op: Op, lands: bool) -> ();
    safe fn quench_engine_fn_native_js_lane_helpers() -> *const ();
    safe fn quench_engine_fn_native_js_operation_entry_for(opcode: usize) -> Option<NativeJsOperationEntry>;
}

/// The engine's methods of `Vm` that the core calls.
pub trait EngineVm {
    /// `run` over a lane stack of its own (`nested_lane_run` in the engine).
    fn nested_lane_run<T>(&mut self, innermost: [Value; 2], run: impl FnOnce(&mut Vm) -> T) -> T;
    fn restore_native_snapshot(&mut self, bytes: crate::KeptBytes, native: &NativeJsProgram,
        read_host: impl FnOnce(&mut dyn Host, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>) -> Result<(), JsError>;
    fn set_native_program_resolver(&mut self, resolver: impl Fn(&str, &[&str]) -> Result<Option<NativeJsProgram>, crate::NativeSourceError> + 'static);
    fn install_wasm_instance(&mut self, env: Value, program: ProgramId) -> Option<()>;
    fn bind_wasm_instance(&mut self, env: Value, program: ProgramId) -> Option<()>;
    fn run_compiled(&mut self, program: ProgramId, env: Value, id: u32, args: &[Value]) -> Option<Result<Value, JsError>>;
    fn materialize_deferred_lane(&mut self) -> ();
    fn wasm_instance_address(&self, env: Value, program: ProgramId) -> Option<usize>;
    fn running_owner(&mut self) -> VarOwner;
    fn offer_compiled(&mut self, caller: usize, id: u32) -> Option<std::rc::Rc<CompiledProgram>>;
    fn run_offered(&mut self, compiled: std::rc::Rc<CompiledProgram>, caller: usize, id: u32, window: crate::bytecode::RegisterWindow) -> Option<Result<Value, JsError>>;
    fn js_jit_compiled_call(&mut self, program: &ResidualProgram, arg1: (ProgramId, u32), returns: bool, arguments: &[Value]) -> Option<Value>;
    fn admits_in_lane(&self, program: ProgramId, code: &ResidualProgram) -> bool;
    fn may_run_in_lane(&self, program: ProgramId, code: &ResidualProgram) -> bool;
    fn entry_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView;
    fn hot_loop_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView;
    fn runs_call_in_lane(view: *const LaneView) -> bool;
    fn tier_may_run(&self, tiers: &[crate::jit_mode::Tier]) -> bool;
    fn view_any_in_lane(&self, program: ProgramId, function: u32) -> Option<*const LaneView>;
    fn runs_in_lane(view: *const LaneView, pc: usize) -> bool;
    fn run_fast_lane(&mut self, code: &ResidualProgram, program: ProgramId, view: *const LaneView, frame: &mut usize, pc: usize, pending: &mut Vec<PendingGeneralCall>, executing: &mut crate::vm::dispatch_frame::ExecutingProgram, inline_calls: bool) -> usize;
    fn js_tier_frame_entry(&mut self, program: &ResidualProgram, frame: usize) -> Option<usize>;
    fn trap_result_error(&mut self, p: &ResidualProgram, pending: &PendingGeneralCall, frame: usize, value: Value) -> Option<(u32, JsError)>;
    fn completed_call_value(&self, completion: &CallCompletion, caller: usize, destination: crate::bytecode::Register, value: Value) -> Value;
    fn record_native_execution_rejection(&mut self) -> ();
    fn runs_windows_in_lane(&self) -> bool;
    fn wasm_table_step(&mut self, f: usize, i: WideInstruction) -> Result<(), super::JsError>;
    fn lane_stack_sites(&mut self) -> Option<LaneSites>;
    fn resolve_native_program(&mut self, name: &str, parts: &[&str]) -> Result<Option<NativeJsProgram>, crate::NativeSourceError>;
    fn prepare_native_script(&mut self, native: NativeJsProgram) -> Result<crate::PreparedScript, JsError>;
    fn attach_native_js(&mut self, id: ProgramId, code: &NativeJsCode) -> Result<(), String>;
    fn innermost_lane_state(&self) -> [Value; INNERMOST_STATE_LEN];
    fn run_lane_job(&mut self, code: &ResidualProgram, job: JobRoot, frame: *const FrameRecord, resume: JobResume) -> Result<JobEnd, JobResume>;
    fn specialized_operations_enabled(&self, program: &ResidualProgram) -> bool;
    fn call_stored_setter(&mut self, p: &ResidualProgram, site: u16, setter: Value, object: Value, value: Value) -> Result<Value, JsError>;
    fn enter_with_scope(&mut self) -> ();
    fn store_native_module_program(&mut self, native: NativeJsProgram) -> Result<ProgramId, JsError>;
    fn kept_native_module_program(&mut self, module: &crate::ModuleSource) -> Result<Option<ProgramId>, JsError>;
    fn run_chained_lane_job(&mut self, code: &ResidualProgram, arg1: (JobRoot, *const FrameRecord), resume: JobResume, pending: &mut Vec<PendingGeneralCall>) -> Result<JobEnd, JobResume>;
    fn report_lane_plan(&mut self) -> ();
    fn rebind_lane_field_entries(&self) -> ();
    fn finish_lane_callback(&mut self, prepared: PreparedLaneCallback) -> ();
    fn call_prepared_lane(&mut self, prepared: &mut PreparedLaneCallback, args: &[Value]) -> Option<Result<Value, JsError>>;
    fn prepare_lane_callback(&mut self, callee: Value, receiver: Value) -> Option<PreparedLaneCallback>;
    fn native_callback_in_lane(&mut self, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>>;
    fn callee_runs_beside_activations(&self, callee: Value) -> bool;
    fn deferred_callback_in_lane(&mut self, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>>;
    fn execution_mode(&self) -> ExecutionMode;
    fn load_ahead_images(&mut self, images: AheadImages) -> ();
    fn stage_images_at_load(&mut self, id: ProgramId, program: &ResidualProgram) -> ();
    fn set_execution_mode(&mut self, mode: ExecutionMode) -> ();
    fn enable_lane_methods(&mut self) -> ();
    fn compile_wasm_eagerly(&mut self) -> ();
    fn native_execution_rejections(&self) -> u64;
    fn set_native_program_resolver_boxed(&mut self, resolver: Box<NativeProgramResolver>) -> ();
    fn execute_native_program(&mut self, native: &NativeJsProgram, drain_jobs: bool) -> Result<Value, JsError>;
    fn execute_native_after_scripts(&mut self, scripts: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError>;
    fn execute_native_module_graph(&mut self, scripts: &[NativeJsProgram], modules: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError>;
}

impl EngineVm for Vm {
    fn nested_lane_run<T>(&mut self, innermost: [Value; 2], run: impl FnOnce(&mut Vm) -> T) -> T {
        let (mut run, mut out) = (Some(run), None);
        quench_engine_vm_nested_lane_run(self, innermost, &mut |vm| out = Some((run.take().expect("a nested run runs once"))(vm)));
        out.expect("the nested run ran")
    }
    fn restore_native_snapshot(&mut self, bytes: crate::KeptBytes, native: &NativeJsProgram,
        read_host: impl FnOnce(&mut dyn Host, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>) -> Result<(), JsError> {
        quench_engine_vm_restore_native_snapshot(self, bytes, native, Box::new(read_host))
    }
    fn set_native_program_resolver(&mut self, resolver: impl Fn(&str, &[&str]) -> Result<Option<NativeJsProgram>, crate::NativeSourceError> + 'static) {
        self.set_native_program_resolver_boxed(Box::new(resolver));
    }
    #[inline(always)]
    fn install_wasm_instance(&mut self, env: Value, program: ProgramId) -> Option<()> { self.execution.install_wasm_instance.and_then(|install| install(self, env, program)) }
    #[inline(always)]
    fn bind_wasm_instance(&mut self, env: Value, program: ProgramId) -> Option<()> { self.execution.bind_wasm_instance.and_then(|bind| bind(self, env, program)) }
    #[inline(always)]
    fn run_compiled(&mut self, program: ProgramId, env: Value, id: u32, args: &[Value]) -> Option<Result<Value, JsError>> { self.execution.run_compiled.and_then(|run| run(self, program, env, id, args)) }
    #[inline(always)]
    fn materialize_deferred_lane(&mut self) -> () { quench_engine_vm_materialize_deferred_lane(self) }
    #[inline(always)]
    fn wasm_instance_address(&self, env: Value, program: ProgramId) -> Option<usize> { self.execution.wasm_instance_address.and_then(|address| address(self, env, program)) }
    #[inline(always)]
    fn running_owner(&mut self) -> VarOwner { quench_engine_vm_running_owner(self) }
    #[inline(always)]
    fn offer_compiled(&mut self, caller: usize, id: u32) -> Option<std::rc::Rc<CompiledProgram>> { self.execution.offer_compiled.and_then(|offer| offer(self, caller, id)) }
    #[inline(always)]
    fn run_offered(&mut self, compiled: std::rc::Rc<CompiledProgram>, caller: usize, id: u32, window: crate::bytecode::RegisterWindow) -> Option<Result<Value, JsError>> { self.execution.run_offered.and_then(|run| run(self, compiled, caller, id, window)) }
    #[inline(always)]
    fn js_jit_compiled_call(&mut self, program: &ResidualProgram, arg1: (ProgramId, u32), returns: bool, arguments: &[Value]) -> Option<Value> { self.execution.js_jit_compiled_call.and_then(|call| call(self, program, arg1, returns, arguments)) }
    #[inline(always)]
    fn admits_in_lane(&self, program: ProgramId, code: &ResidualProgram) -> bool { quench_engine_vm_admits_in_lane(self, program, code) }
    #[inline(always)]
    fn may_run_in_lane(&self, program: ProgramId, code: &ResidualProgram) -> bool { quench_engine_vm_may_run_in_lane(self, program, code) }
    #[inline(always)]
    fn entry_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView { quench_engine_vm_entry_lane_view(self, program, function) }
    #[inline(always)]
    fn hot_loop_lane_view(&self, program: ProgramId, function: u32) -> *const LaneView {
        self.execution.hot_loop_lane_view.map_or(std::ptr::null(), |view| view(self, program, function))
    }
    #[inline(always)]
    fn runs_call_in_lane(view: *const LaneView) -> bool { quench_engine_vm_runs_call_in_lane(view) }
    #[inline(always)]
    fn tier_may_run(&self, tiers: &[crate::jit_mode::Tier]) -> bool { self.execution.tier_may_run.is_some_and(|may_run| may_run(self, tiers)) }
    #[inline(always)]
    fn view_any_in_lane(&self, program: ProgramId, function: u32) -> Option<*const LaneView> { quench_engine_vm_view_any_in_lane(self, program, function) }
    #[inline(always)]
    fn runs_in_lane(view: *const LaneView, pc: usize) -> bool { quench_engine_vm_runs_in_lane(view, pc) }
    #[inline(always)]
    fn run_fast_lane(&mut self, code: &ResidualProgram, program: ProgramId, view: *const LaneView, frame: &mut usize, pc: usize, pending: &mut Vec<PendingGeneralCall>, executing: &mut crate::vm::dispatch_frame::ExecutingProgram, inline_calls: bool) -> usize {
        let interpret = self.execution.interpret.unwrap_or_else(|| super::execution_table::unproven_entry("run_fast_lane"));
        interpret(self, code, program, view, frame, pc, pending, executing, inline_calls)
    }
    #[inline(always)]
    fn js_tier_frame_entry(&mut self, program: &ResidualProgram, frame: usize) -> Option<usize> { self.execution.js_tier_frame_entry.and_then(|enter| enter(self, program, frame)) }
    #[inline(always)]
    fn trap_result_error(&mut self, p: &ResidualProgram, pending: &PendingGeneralCall, frame: usize, value: Value) -> Option<(u32, JsError)> { quench_engine_vm_trap_result_error(self, p, pending, frame, value) }
    #[inline(always)]
    fn completed_call_value(&self, completion: &CallCompletion, caller: usize, destination: crate::bytecode::Register, value: Value) -> Value { quench_engine_vm_completed_call_value(self, completion, caller, destination, value) }
    #[inline(always)]
    fn record_native_execution_rejection(&mut self) -> () { if let Some(record) = self.execution.record_native_execution_rejection { record(self) } }
    #[inline(always)]
    fn runs_windows_in_lane(&self) -> bool { quench_engine_vm_runs_windows_in_lane(self) }
    #[inline(always)]
    fn wasm_table_step(&mut self, f: usize, i: WideInstruction) -> Result<(), super::JsError> {
        match self.execution.wasm_table_step {
            Some(step) => step(self, f, i),
            None => Err(JsError::validation("image proved no wasm table: its execution table leaves wasm_table_step out".to_owned())),
        }
    }
    #[inline(always)]
    fn lane_stack_sites(&mut self) -> Option<LaneSites> { quench_engine_vm_lane_stack_sites(self) }
    #[inline(always)]
    fn resolve_native_program(&mut self, name: &str, parts: &[&str]) -> Result<Option<NativeJsProgram>, crate::NativeSourceError> { quench_engine_vm_resolve_native_program(self, name, parts) }
    #[inline(always)]
    fn prepare_native_script(&mut self, native: NativeJsProgram) -> Result<crate::PreparedScript, JsError> {
        match self.execution.prepare_native_script {
            Some(prepare) => prepare(self, native),
            None => Err(JsError::validation("image proved no source compile: its execution table leaves prepare_native_script out".to_owned())),
        }
    }
    #[inline(always)]
    fn attach_native_js(&mut self, id: ProgramId, code: &NativeJsCode) -> Result<(), String> {
        match self.execution.attach_native_js {
            Some(attach) => attach(self, id, code),
            None => Err("image proved no native JS attach: its execution table leaves attach_native_js out".to_owned()),
        }
    }
    #[inline(always)]
    fn innermost_lane_state(&self) -> [Value; INNERMOST_STATE_LEN] { quench_engine_vm_innermost_lane_state(self) }
    #[inline(always)]
    fn run_lane_job(&mut self, code: &ResidualProgram, job: JobRoot, frame: *const FrameRecord, resume: JobResume) -> Result<JobEnd, JobResume> { quench_engine_vm_run_lane_job(self, code, job, frame, resume) }
    #[inline(always)]
    fn specialized_operations_enabled(&self, program: &ResidualProgram) -> bool { quench_engine_vm_specialized_operations_enabled(self, program) }
    #[inline(always)]
    fn call_stored_setter(&mut self, p: &ResidualProgram, site: u16, setter: Value, object: Value, value: Value) -> Result<Value, JsError> { quench_engine_vm_call_stored_setter(self, p, site, setter, object, value) }
    #[inline(always)]
    fn enter_with_scope(&mut self) -> () { quench_engine_vm_enter_with_scope(self) }
    #[inline(always)]
    fn store_native_module_program(&mut self, native: NativeJsProgram) -> Result<ProgramId, JsError> {
        match self.execution.store_native_module_program {
            Some(store) => store(self, native),
            None => Err(JsError::validation("image proved no native module compile: its execution table leaves store_native_module_program out".to_owned())),
        }
    }
    #[inline(always)]
    fn kept_native_module_program(&mut self, module: &crate::ModuleSource) -> Result<Option<ProgramId>, JsError> {
        self.execution.kept_native_module_program.map_or(Ok(None), |kept| kept(self, module))
    }
    #[inline(always)]
    fn run_chained_lane_job(&mut self, code: &ResidualProgram, arg1: (JobRoot, *const FrameRecord), resume: JobResume, pending: &mut Vec<PendingGeneralCall>) -> Result<JobEnd, JobResume> { quench_engine_vm_run_chained_lane_job(self, code, arg1, resume, pending) }
    #[inline(always)]
    fn report_lane_plan(&mut self) -> () { if let Some(report) = self.execution.report_lane_plan { report(self) } }
    #[inline(always)]
    fn rebind_lane_field_entries(&self) -> () { quench_engine_vm_rebind_lane_field_entries(self) }
    #[inline(always)]
    fn finish_lane_callback(&mut self, prepared: PreparedLaneCallback) -> () { quench_engine_vm_finish_lane_callback(self, prepared) }
    #[inline(always)]
    fn call_prepared_lane(&mut self, prepared: &mut PreparedLaneCallback, args: &[Value]) -> Option<Result<Value, JsError>> { quench_engine_vm_call_prepared_lane(self, prepared, args) }
    #[inline(always)]
    fn prepare_lane_callback(&mut self, callee: Value, receiver: Value) -> Option<PreparedLaneCallback> { quench_engine_vm_prepare_lane_callback(self, callee, receiver) }
    #[inline(always)]
    fn native_callback_in_lane(&mut self, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>> { quench_engine_vm_native_callback_in_lane(self, callee, receiver, args) }
    #[inline(always)]
    fn callee_runs_beside_activations(&self, callee: Value) -> bool { quench_engine_vm_callee_runs_beside_activations(self, callee) }
    #[inline(always)]
    fn deferred_callback_in_lane(&mut self, callee: Value, receiver: Value, args: &[Value]) -> Option<Result<Value, JsError>> { quench_engine_vm_deferred_callback_in_lane(self, callee, receiver, args) }
    #[inline(always)]
    fn execution_mode(&self) -> ExecutionMode { quench_engine_vm_execution_mode(self) }
    #[inline(always)]
    fn load_ahead_images(&mut self, images: AheadImages) -> () { if let Some(load) = self.execution.load_ahead_images { load(self, images) } }
    #[inline(always)]
    fn stage_images_at_load(&mut self, id: ProgramId, program: &ResidualProgram) -> () { if let Some(stage) = self.execution.stage_images_at_load { stage(self, id, program) } }
    #[inline(always)]
    fn set_execution_mode(&mut self, mode: ExecutionMode) -> () { quench_engine_vm_set_execution_mode(self, mode) }
    #[inline(always)]
    fn enable_lane_methods(&mut self) -> () { quench_engine_vm_enable_lane_methods(self) }
    #[inline(always)]
    fn compile_wasm_eagerly(&mut self) -> () { if let Some(compile) = self.execution.compile_wasm_eagerly { compile(self) } }
    #[inline(always)]
    fn native_execution_rejections(&self) -> u64 { self.execution.native_execution_rejections.map_or(0, |count| count(self)) }
    #[inline(always)]
    fn set_native_program_resolver_boxed(&mut self, resolver: Box<NativeProgramResolver>) -> () { quench_engine_vm_set_native_program_resolver_boxed(self, resolver) }
    #[inline(always)]
    fn execute_native_program(&mut self, native: &NativeJsProgram, drain_jobs: bool) -> Result<Value, JsError> { quench_engine_vm_execute_native_program(self, native, drain_jobs) }
    #[inline(always)]
    fn execute_native_after_scripts(&mut self, scripts: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError> { quench_engine_vm_execute_native_after_scripts(self, scripts, native) }
    #[inline(always)]
    fn execute_native_module_graph(&mut self, scripts: &[NativeJsProgram], modules: &[NativeJsProgram], native: &NativeJsProgram) -> Result<Value, JsError> { quench_engine_vm_execute_native_module_graph(self, scripts, modules, native) }
}

#[inline(always)]
pub fn flush_retired_writers() -> () { quench_engine_fn_flush_retired_writers() }

#[inline(always)]
pub fn direct_memory(vm: &Vm, binding: Value) -> Option<(*mut u8, usize)> { quench_engine_fn_direct_memory(vm, binding) }

#[inline(always)]
pub fn in_place_in_loop(native: Native) -> bool { quench_engine_fn_in_place_in_loop(native) }

#[inline(always)]
pub fn retire_running_lane_context() -> () { quench_engine_fn_retire_running_lane_context() }

#[inline(always)]
pub fn names_dead_callee(inlined: &[InlinedCall], direct: &[DirectCallee], live: &dyn Fn(Value) -> bool) -> bool { quench_engine_fn_names_dead_callee(inlined, direct, live) }

#[inline(always)]
pub fn lane_miss_report(events: &crate::vm::tier_events::TierEvents) -> String { quench_engine_fn_lane_miss_report(events) }

#[inline(always)]
pub fn lane_unlowered_report() -> String { quench_engine_fn_lane_unlowered_report() }

#[inline(always)]
pub fn census() -> (u64, u64, u64) { quench_engine_fn_census() }

#[inline(always)]
pub fn jit_wasm_selective_enabled() -> bool { quench_engine_fn_jit_wasm_selective_enabled() }

#[inline(always)]
pub fn candidate_rejection_reason(program: &ResidualProgram, function_id: u32) -> &'static str { quench_engine_fn_candidate_rejection_reason(program, function_id) }

#[inline(always)]
pub fn settle_name_record(view: &LaneView, pc: usize, op: Op, lands: bool) -> () { quench_engine_fn_settle_name_record(view, pc, op, lands) }

#[inline(always)]
pub fn native_js_lane_helpers() -> *const () { quench_engine_fn_native_js_lane_helpers() }

#[inline(always)]
pub fn native_js_operation_entry_for(opcode: usize) -> Option<NativeJsOperationEntry> { quench_engine_fn_native_js_operation_entry_for(opcode) }

/// Defines the lane helpers' named entries (`quench_lane_helper_<Name>`) an ahead image's object
/// links against (`export_native_js_operations!`).
#[inline(always)]
pub fn define_lane_helper_symbols() { quench_engine_fn_define_lane_helper_symbols() }

/// Execute one compile-time-selected semantic operation and return its exact
/// continuation. This entry never batches subsequent bytecodes.
///
/// # Safety
/// `context` must be a live shared native-entry context for `H`, with its
/// frame, program and adapter payload rooted through this call. `result` must
/// point to a writable result word. `arguments` must point to an aligned,
/// live `NativeJsOperands` from this compiler build, matching the instruction
/// at the supplied PC. Operand structure is resolved before native execution.
#[cfg(feature = "native-execution")]
pub unsafe extern "C" fn native_js_operation_entry<const OPCODE: usize>(
    context: *mut std::ffi::c_void,
    pc: u64,
    arguments: *const u64,
    result: *mut u64,
) -> i64 {
    #[cfg(feature = "profile-native-ops")]
    crate::vm::native_op_profile::record(OPCODE);
    // The opcode resolves its operation statically; the boundary around the step is one body
    // for every opcode, in the engine (`js_semantic_callout_with`).
    let operation = const {
        match Op::from_index(OPCODE) {
            Some(opcode) => Some(StaticJsOperation { opcode, execute: Vm::resolve_step_operation(opcode) }),
            None => None,
        }
    };
    // SAFETY: forwarded from this entry's native ABI contract. Invalid opcode
    // constants are reported inside the shared exception boundary.
    unsafe { quench_engine_fn_js_semantic_callout_single(operation, context.cast(), pc, arguments, result) }
}

/// The type of each engine entry, which its definition is checked against.
pub mod signatures {
    use super::*;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_nested_lane_run = fn(&mut Vm, [Value; 2], &mut dyn FnMut(&mut Vm));
    #[allow(non_camel_case_types)] pub type quench_engine_vm_restore_native_snapshot = for<'a> fn(&mut Vm, crate::KeptBytes, &NativeJsProgram,
        Box<dyn FnOnce(&mut dyn Host, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String> + 'a>) -> Result<(), JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_define_lane_helper_symbols = fn();
    #[cfg(feature = "native-execution")]
    #[allow(non_camel_case_types)] pub type quench_engine_fn_js_semantic_callout_single = unsafe fn(Option<StaticJsOperation>, *mut CallContext, u64, *const u64, *mut u64) -> i64;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_install_wasm_instance = fn(&mut Vm, Value, ProgramId) -> Option<()>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_bind_wasm_instance = fn(&mut Vm, Value, ProgramId) -> Option<()>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_run_compiled = fn(&mut Vm, ProgramId, Value, u32, &[Value]) -> Option<Result<Value, JsError>>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_materialize_deferred_lane = fn(&mut Vm) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_wasm_instance_address = fn(&Vm, Value, ProgramId) -> Option<usize>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_running_owner = fn(&mut Vm) -> VarOwner;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_offer_compiled = fn(&mut Vm, usize, u32) -> Option<std::rc::Rc<CompiledProgram>>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_run_offered = fn(&mut Vm, std::rc::Rc<CompiledProgram>, usize, u32, crate::bytecode::RegisterWindow) -> Option<Result<Value, JsError>>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_js_jit_compiled_call = fn(&mut Vm, &ResidualProgram, (ProgramId, u32), bool, &[Value]) -> Option<Value>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_admits_in_lane = fn(&Vm, ProgramId, &ResidualProgram) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_may_run_in_lane = fn(&Vm, ProgramId, &ResidualProgram) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_entry_lane_view = fn(&Vm, ProgramId, u32) -> *const LaneView;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_hot_loop_lane_view = fn(&Vm, ProgramId, u32) -> *const LaneView;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_runs_call_in_lane = fn(*const LaneView) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_tier_may_run = fn(&Vm, &[crate::jit_mode::Tier]) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_view_any_in_lane = fn(&Vm, ProgramId, u32) -> Option<*const LaneView>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_runs_in_lane = fn(*const LaneView, usize) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_run_fast_lane = fn(&mut Vm, &ResidualProgram, ProgramId, *const LaneView, &mut usize, usize, &mut Vec<PendingGeneralCall>, &mut crate::vm::dispatch_frame::ExecutingProgram, bool) -> usize;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_js_tier_frame_entry = fn(&mut Vm, &ResidualProgram, usize) -> Option<usize>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_trap_result_error = fn(&mut Vm, &ResidualProgram, &PendingGeneralCall, usize, Value) -> Option<(u32, JsError)>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_completed_call_value = fn(&Vm, &CallCompletion, usize, crate::bytecode::Register, Value) -> Value;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_record_native_execution_rejection = fn(&mut Vm) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_runs_windows_in_lane = fn(&Vm) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_wasm_table_step = fn(&mut Vm, usize, WideInstruction) -> Result<(), super::JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_lane_stack_sites = fn(&mut Vm) -> Option<LaneSites>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_resolve_native_program = fn(&mut Vm, &str, &[&str]) -> Result<Option<NativeJsProgram>, crate::NativeSourceError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_prepare_native_script = fn(&mut Vm, NativeJsProgram) -> Result<crate::PreparedScript, JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_attach_native_js = fn(&mut Vm, ProgramId, &NativeJsCode) -> Result<(), String>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_innermost_lane_state = fn(&Vm) -> [Value; INNERMOST_STATE_LEN];
    #[allow(non_camel_case_types)] pub type quench_engine_vm_run_lane_job = fn(&mut Vm, &ResidualProgram, JobRoot, *const FrameRecord, JobResume) -> Result<JobEnd, JobResume>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_specialized_operations_enabled = fn(&Vm, &ResidualProgram) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_call_stored_setter = fn(&mut Vm, &ResidualProgram, u16, Value, Value, Value) -> Result<Value, JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_enter_with_scope = fn(&mut Vm) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_store_native_module_program = fn(&mut Vm, NativeJsProgram) -> Result<ProgramId, JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_kept_native_module_program = fn(&mut Vm, &crate::ModuleSource) -> Result<Option<ProgramId>, JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_run_chained_lane_job = fn(&mut Vm, &ResidualProgram, (JobRoot, *const FrameRecord), JobResume, &mut Vec<PendingGeneralCall>) -> Result<JobEnd, JobResume>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_report_lane_plan = fn(&mut Vm) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_rebind_lane_field_entries = fn(&Vm) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_finish_lane_callback = fn(&mut Vm, PreparedLaneCallback) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_call_prepared_lane = fn(&mut Vm, &mut PreparedLaneCallback, &[Value]) -> Option<Result<Value, JsError>>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_prepare_lane_callback = fn(&mut Vm, Value, Value) -> Option<PreparedLaneCallback>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_native_callback_in_lane = fn(&mut Vm, Value, Value, &[Value]) -> Option<Result<Value, JsError>>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_callee_runs_beside_activations = fn(&Vm, Value) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_deferred_callback_in_lane = fn(&mut Vm, Value, Value, &[Value]) -> Option<Result<Value, JsError>>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_execution_mode = fn(&Vm) -> ExecutionMode;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_load_ahead_images = fn(&mut Vm, AheadImages) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_stage_images_at_load = fn(&mut Vm, ProgramId, &ResidualProgram) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_set_execution_mode = fn(&mut Vm, ExecutionMode) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_enable_lane_methods = fn(&mut Vm) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_compile_wasm_eagerly = fn(&mut Vm) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_native_execution_rejections = fn(&Vm) -> u64;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_set_native_program_resolver_boxed = fn(&mut Vm, Box<NativeProgramResolver>) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_vm_execute_native_program = fn(&mut Vm, &NativeJsProgram, bool) -> Result<Value, JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_execute_native_after_scripts = fn(&mut Vm, &[NativeJsProgram], &NativeJsProgram) -> Result<Value, JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_vm_execute_native_module_graph = fn(&mut Vm, &[NativeJsProgram], &[NativeJsProgram], &NativeJsProgram) -> Result<Value, JsError>;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_flush_retired_writers = fn() -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_fn_direct_memory = fn(&Vm, Value) -> Option<(*mut u8, usize)>;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_in_place_in_loop = fn(Native) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_retire_running_lane_context = fn() -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_fn_names_dead_callee = fn(&[InlinedCall], &[DirectCallee], &dyn Fn(Value) -> bool) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_lane_miss_report = fn(&crate::vm::tier_events::TierEvents) -> String;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_lane_unlowered_report = fn() -> String;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_census = fn() -> (u64, u64, u64);
    #[allow(non_camel_case_types)] pub type quench_engine_fn_jit_wasm_selective_enabled = fn() -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_candidate_rejection_reason = fn(&ResidualProgram, u32) -> &'static str;
    #[allow(non_camel_case_types)] pub type quench_engine_fn_settle_name_record = fn(&LaneView, usize, Op, bool) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine_fn_native_js_lane_helpers = fn() -> *const ();
    #[allow(non_camel_case_types)] pub type quench_engine_fn_native_js_operation_entry_for = fn(usize) -> Option<NativeJsOperationEntry>;
}

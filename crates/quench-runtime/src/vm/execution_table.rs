//! The engine entries the VM core calls, as link-time `Option<fn>`s (C5 B2, B3).
//!
//! Each entry is the engine's definition of one `quench_engine_vm_*` hook (`engine_calls.rs`). The
//! core reaches it only through this table, so an image whose closed world proved the entry
//! unreachable links it as `None`, and the engine code behind it drops out of the link. The table
//! is built from `Demand::execution_table` (`quench-runtime-engine/src/aot_image/demand.rs`), one
//! group per demand flag:
//!
//! * `interpret`: the frame loop's interpreter, and the counter of the native executions it rejects
//!   (`Demand::needs_interpreter`);
//! * `tier_up`: the tiers that take a frame over or offer it compiled code, the hot-loop view they
//!   derive, and the lane plan report that derives views (`Demand::needs_exits`);
//! * `compile_source`: run-time compilation of source text (`eval`, `Function`), and the native
//!   module programs it stores (`Demand::compile_source`, always on until E1);
//! * `link_live`: the VM's linking of a lane body from the image file, and the AOT images it loads
//!   (`Demand::needs_live_link`);
//! * `wasm`: the WebAssembly tier (`Demand::wasm`).
//!
//! `ALL` is the complete table every non-image runtime uses, so its behaviour is unchanged. A `None`
//! the closed world proved unreachable is a typed abort where the core has no answer (the frame
//! loop, which cannot proceed); a tier or query entry that is absent answers "not taken" (no
//! compiled code, no tier), which is what an image with no tier means.

use super::engine_calls::signatures as entry;

/// The interpreter: runs a lane frame (`quench_engine_vm_run_fast_lane`).
pub type RunLaneFn = entry::quench_engine_vm_run_fast_lane;
/// Takes a frame over at its entry (`quench_engine_vm_js_tier_frame_entry`).
pub type TierFrameEntryFn = entry::quench_engine_vm_js_tier_frame_entry;
/// The tiers' answer to a call entry (`quench_engine_vm_offer_compiled`).
pub type OfferCompiledFn = entry::quench_engine_vm_offer_compiled;
/// Runs an offered compiled program (`quench_engine_vm_run_offered`).
pub type RunOfferedFn = entry::quench_engine_vm_run_offered;
/// The compiled call a known callee makes without a frame (`quench_engine_vm_js_jit_compiled_call`).
pub type JsJitCompiledCallFn = entry::quench_engine_vm_js_jit_compiled_call;
/// Whether a compiled tier may run under the given tiers (`quench_engine_vm_tier_may_run`).
pub type TierMayRunFn = entry::quench_engine_vm_tier_may_run;
/// Compiles a native source program for `eval` or `Function` (`quench_engine_vm_prepare_native_script`).
pub type PrepareNativeScriptFn = entry::quench_engine_vm_prepare_native_script;
/// Stages a program's AOT units at load (`quench_engine_vm_stage_images_at_load`).
pub type StageImagesAtLoadFn = entry::quench_engine_vm_stage_images_at_load;
/// Attaches native JS code to a program (`quench_engine_vm_attach_native_js`).
pub type AttachNativeJsFn = entry::quench_engine_vm_attach_native_js;
/// Installs a Wasm instance record (`quench_engine_vm_install_wasm_instance`).
pub type InstallWasmInstanceFn = entry::quench_engine_vm_install_wasm_instance;
/// Binds a Wasm instance record (`quench_engine_vm_bind_wasm_instance`).
pub type BindWasmInstanceFn = entry::quench_engine_vm_bind_wasm_instance;
/// Runs a compiled Wasm function (`quench_engine_vm_run_compiled`).
pub type RunCompiledFn = entry::quench_engine_vm_run_compiled;
/// Steps a Wasm table instruction (`quench_engine_vm_wasm_table_step`).
pub type WasmTableStepFn = entry::quench_engine_vm_wasm_table_step;
/// The address of a Wasm instance record (`quench_engine_vm_wasm_instance_address`).
pub type WasmInstanceAddressFn = entry::quench_engine_vm_wasm_instance_address;
/// Compiles the module's Wasm functions eagerly (`quench_engine_vm_compile_wasm_eagerly`).
pub type CompileWasmEagerlyFn = entry::quench_engine_vm_compile_wasm_eagerly;
/// The count of rejected native executions (`quench_engine_vm_native_execution_rejections`).
pub type NativeExecutionRejectionsFn = entry::quench_engine_vm_native_execution_rejections;
/// Counts a rejected native execution (`quench_engine_vm_record_native_execution_rejection`).
pub type RecordNativeExecutionRejectionFn = entry::quench_engine_vm_record_native_execution_rejection;
/// The lane view of a function a hot loop of it turned hot (`quench_engine_vm_hot_loop_lane_view`).
pub type HotLoopLaneViewFn = entry::quench_engine_vm_hot_loop_lane_view;
/// Reports the lane plan of a finished run (`quench_engine_vm_report_lane_plan`).
pub type ReportLanePlanFn = entry::quench_engine_vm_report_lane_plan;
/// Stores a native module's program (`quench_engine_vm_store_native_module_program`).
pub type StoreNativeModuleProgramFn = entry::quench_engine_vm_store_native_module_program;
/// Admits a kept native module's program (`quench_engine_vm_kept_native_module_program`).
pub type KeptNativeModuleProgramFn = entry::quench_engine_vm_kept_native_module_program;
/// Loads AOT images into the runtime (`quench_engine_vm_load_ahead_images`).
pub type LoadAheadImagesFn = entry::quench_engine_vm_load_ahead_images;

/// The engine entries one runtime links. A field is `None` when the runtime's closed world proved
/// the entry unreachable; `ALL` is every entry.
#[derive(Clone, Copy)]
pub struct ExecutionTable {
    /// `interpret` group.
    pub interpret: Option<RunLaneFn>,
    pub native_execution_rejections: Option<NativeExecutionRejectionsFn>,
    pub record_native_execution_rejection: Option<RecordNativeExecutionRejectionFn>,
    /// `tier_up` group.
    pub js_tier_frame_entry: Option<TierFrameEntryFn>,
    pub offer_compiled: Option<OfferCompiledFn>,
    pub run_offered: Option<RunOfferedFn>,
    pub js_jit_compiled_call: Option<JsJitCompiledCallFn>,
    pub tier_may_run: Option<TierMayRunFn>,
    pub hot_loop_lane_view: Option<HotLoopLaneViewFn>,
    pub report_lane_plan: Option<ReportLanePlanFn>,
    /// `compile_source` group.
    pub prepare_native_script: Option<PrepareNativeScriptFn>,
    pub store_native_module_program: Option<StoreNativeModuleProgramFn>,
    pub kept_native_module_program: Option<KeptNativeModuleProgramFn>,
    /// `link_live` group.
    pub stage_images_at_load: Option<StageImagesAtLoadFn>,
    pub attach_native_js: Option<AttachNativeJsFn>,
    pub load_ahead_images: Option<LoadAheadImagesFn>,
    /// `wasm` group.
    pub install_wasm_instance: Option<InstallWasmInstanceFn>,
    pub bind_wasm_instance: Option<BindWasmInstanceFn>,
    pub run_compiled: Option<RunCompiledFn>,
    pub wasm_table_step: Option<WasmTableStepFn>,
    pub wasm_instance_address: Option<WasmInstanceAddressFn>,
    pub compile_wasm_eagerly: Option<CompileWasmEagerlyFn>,
}

impl ExecutionTable {
    /// Every engine entry: what every non-image runtime links.
    pub const ALL: Self = Self {
        interpret: Some(super::engine_calls::quench_engine_vm_run_fast_lane),
        native_execution_rejections: Some(super::engine_calls::quench_engine_vm_native_execution_rejections),
        record_native_execution_rejection: Some(super::engine_calls::quench_engine_vm_record_native_execution_rejection),
        js_tier_frame_entry: Some(super::engine_calls::quench_engine_vm_js_tier_frame_entry),
        offer_compiled: Some(super::engine_calls::quench_engine_vm_offer_compiled),
        run_offered: Some(super::engine_calls::quench_engine_vm_run_offered),
        js_jit_compiled_call: Some(super::engine_calls::quench_engine_vm_js_jit_compiled_call),
        tier_may_run: Some(super::engine_calls::quench_engine_vm_tier_may_run),
        hot_loop_lane_view: Some(super::engine_calls::quench_engine_vm_hot_loop_lane_view),
        report_lane_plan: Some(super::engine_calls::quench_engine_vm_report_lane_plan),
        prepare_native_script: Some(super::engine_calls::quench_engine_vm_prepare_native_script),
        store_native_module_program: Some(super::engine_calls::quench_engine_vm_store_native_module_program),
        kept_native_module_program: Some(super::engine_calls::quench_engine_vm_kept_native_module_program),
        stage_images_at_load: Some(super::engine_calls::quench_engine_vm_stage_images_at_load),
        attach_native_js: Some(super::engine_calls::quench_engine_vm_attach_native_js),
        load_ahead_images: Some(super::engine_calls::quench_engine_vm_load_ahead_images),
        install_wasm_instance: Some(super::engine_calls::quench_engine_vm_install_wasm_instance),
        bind_wasm_instance: Some(super::engine_calls::quench_engine_vm_bind_wasm_instance),
        run_compiled: Some(super::engine_calls::quench_engine_vm_run_compiled),
        wasm_table_step: Some(super::engine_calls::quench_engine_vm_wasm_table_step),
        wasm_instance_address: Some(super::engine_calls::quench_engine_vm_wasm_instance_address),
        compile_wasm_eagerly: Some(super::engine_calls::quench_engine_vm_compile_wasm_eagerly),
    };
}

/// A table entry the image's closed world proved unreachable was reached at run time: the program
/// does not match the image it was linked with. There is no answer the frame loop can give.
#[cold]
#[track_caller]
pub(crate) fn unproven_entry(name: &str) -> ! {
    panic!("image proved no `{name}` (its execution table leaves the entry out)")
}

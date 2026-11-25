//! The JIT's compile routes, which link only into a binary whose runtimes run them: `Vm::new`
//! installs them (`quench_layer_enable_jit`) as it makes a runtime, while a binary that only
//! resumes a runtime saved at rest (`Vm::new_at_rest`) never names them, so it links no
//! Cranelift. Without them every route declines and the interpreter runs, as under `--no-jit`.
#[allow(unused_imports)]
use crate::ext_traits::*;

/// The JIT's route of `specialize_in_lane` for a function with no ahead image.
pub(crate) type LaneJit = fn(*mut quench_runtime::vm::engine_types::LaneContext, &quench_runtime::vm::engine_types::LaneView, u32, &[quench_runtime::Value],
    Option<(u64, u64)>, crate::compiled::LaneBodyWork<'_>) -> crate::compiled::LaneBodyOutcome;

/// The routes `Vm::new` installs.
pub(crate) struct JitRoutes {
    /// `dispatch_fast::specialize_in_lane`'s route for a function with no ahead image.
    pub(crate) lane: LaneJit,
    /// A lane body's plan and compile (`Vm::body_work_in_lane`).
    pub(crate) body: crate::compiled::lane_compile::BodyWork,
    /// A numeric function's image (`js_compiled::start`).
    pub(crate) numeric: crate::js_compiled::NumericStart,
    /// A hot loop's region (`compiled::start_osr_region`).
    pub(crate) region: crate::compiled::RegionStart,
    /// A Wasm program's code (`compile_program_with_diagnostics_and_root_mode`).
    pub(crate) wasm: crate::compiled::WasmCompile,
    /// The compiler of hot regexp programs (`regexp_native::enable`).
    pub(crate) regexp: fn(bool, (u32, u32)),
}

static JIT: std::sync::OnceLock<JitRoutes> = std::sync::OnceLock::new();

/// The installed routes, or `None` in a binary that never makes a runtime with `Vm::new`.
#[inline]
pub(crate) fn jit() -> Option<&'static JitRoutes> {
    JIT.get()
}

#[unsafe(no_mangle)]
fn quench_layer_enable_jit() {
    JIT.get_or_init(|| JitRoutes {
        lane: crate::dispatch_fast::jit_lane_body,
        body: <quench_runtime::vm::Vm as crate::compiled::lane_compile::CompiledLaneCompileVm>::compile_body_work,
        numeric: crate::js_compiled::start_numeric,
        region: crate::compiled::compile_osr_region,
        wasm: <quench_runtime::vm::Vm as crate::compiled::CompiledVm4>::compile_wasm_program,
        regexp: crate::regexp_native::enable_compiler,
    });
}
const _: quench_runtime::vm::layer_hooks::signatures::enable_jit = quench_layer_enable_jit;

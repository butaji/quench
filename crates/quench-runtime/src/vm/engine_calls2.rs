//! Calls from the VM core into its engine layer (`quench-runtime-engine`: the lane handlers and
//! compiled tiers). Each is a link-time symbol the engine defines (`#[unsafe(no_mangle)]`), checked
//! against `signatures` there, so a signature change fails the engine's build. Generated from the
//! engine's definitions; the core sees the methods through `EngineVm2`, which the engine itself does
//! not import, so its own calls stay direct.
#![allow(clippy::too_many_arguments)]
use super::*;
use super::engine_types::*;
use crate::bytecode::Function;

unsafe extern "Rust" {
    safe fn quench_engine2_aheadimages_compile(program: &ResidualProgram) -> Result<AheadImages, String>;
    safe fn quench_engine2_aheadimages_compile_wasm(vm: &Vm, id: ProgramId, program: &ResidualProgram, bindings: &[crate::WasmBindingType]) -> Result<AheadImages, String>;
    safe fn quench_engine2_laneview_invalidate_shape_bindings(this: &LaneView) -> ();
    safe fn quench_engine2_fn_function_candidate(function: &Function, program: &ResidualProgram) -> bool;
    safe fn quench_engine2_fn_subscribe_stable(stable: &StableFacts, code: &Rc<dyn crate::code_dependencies::DependentCode>, facts: &mut crate::vm::tier_queries::FactWatch<'_>) -> bool;
    safe fn quench_engine2_fn_ahead_program_fingerprint(program: &ResidualProgram) -> u64;
    safe fn quench_engine2_fn_reserve_lane_scratch(width: usize) -> ();
    safe fn quench_engine2_fn_is_call_rejection(opcode: u16) -> bool;
}

/// The engine methods of `AheadImages` the core calls.
pub trait EngineAheadImages {
    fn compile(program: &ResidualProgram) -> Result<AheadImages, String>;
    fn compile_wasm(vm: &Vm, id: ProgramId, program: &ResidualProgram, bindings: &[crate::WasmBindingType]) -> Result<AheadImages, String>;
}
impl EngineAheadImages for AheadImages {
    #[inline(always)]
    fn compile(program: &ResidualProgram) -> Result<AheadImages, String> { quench_engine2_aheadimages_compile(program) }
    #[inline(always)]
    fn compile_wasm(vm: &Vm, id: ProgramId, program: &ResidualProgram, bindings: &[crate::WasmBindingType]) -> Result<AheadImages, String> { quench_engine2_aheadimages_compile_wasm(vm, id, program, bindings) }
}

/// The engine methods of `LaneView` the core calls.
pub trait EngineLaneView {
    fn invalidate_shape_bindings(&self) -> ();
}
impl EngineLaneView for LaneView {
    #[inline(always)]
    fn invalidate_shape_bindings(&self) -> () { quench_engine2_laneview_invalidate_shape_bindings(self) }
}

#[inline(always)]
pub fn function_candidate(function: &Function, program: &ResidualProgram) -> bool { quench_engine2_fn_function_candidate(function, program) }

#[inline(always)]

#[inline(always)]
pub fn subscribe_stable(stable: &StableFacts, code: &Rc<dyn crate::code_dependencies::DependentCode>, facts: &mut crate::vm::tier_queries::FactWatch<'_>) -> bool { quench_engine2_fn_subscribe_stable(stable, code, facts) }

#[inline(always)]
pub fn ahead_program_fingerprint(program: &ResidualProgram) -> u64 { quench_engine2_fn_ahead_program_fingerprint(program) }

#[inline(always)]
pub fn reserve_lane_scratch(width: usize) -> () { quench_engine2_fn_reserve_lane_scratch(width) }

#[inline(always)]
pub fn is_call_rejection(opcode: u16) -> bool { quench_engine2_fn_is_call_rejection(opcode) }

/// The type of each engine entry, which its definition is checked against.
pub mod signatures {
    use super::*;
    #[allow(non_camel_case_types)] pub type quench_engine2_aheadimages_compile = fn(&ResidualProgram) -> Result<AheadImages, String>;
    #[allow(non_camel_case_types)] pub type quench_engine2_aheadimages_compile_wasm = fn(&Vm, ProgramId, &ResidualProgram, &[crate::WasmBindingType]) -> Result<AheadImages, String>;
    #[allow(non_camel_case_types)] pub type quench_engine2_laneview_invalidate_shape_bindings = fn(&LaneView) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine2_fn_function_candidate = fn(&Function, &ResidualProgram) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine2_fn_subscribe_stable = fn(&StableFacts, &Rc<dyn crate::code_dependencies::DependentCode>, &mut crate::vm::tier_queries::FactWatch<'_>) -> bool;
    #[allow(non_camel_case_types)] pub type quench_engine2_fn_ahead_program_fingerprint = fn(&ResidualProgram) -> u64;
    #[allow(non_camel_case_types)] pub type quench_engine2_fn_reserve_lane_scratch = fn(usize) -> ();
    #[allow(non_camel_case_types)] pub type quench_engine2_fn_is_call_rejection = fn(u16) -> bool;
}

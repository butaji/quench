//! The engine's definitions of the core's calls into it (`quench_runtime::vm::engine_calls2`).
#![allow(clippy::too_many_arguments)]
#[allow(unused_imports)]
use crate::ext_traits::*;
use crate::engine_prelude::*;
use quench_runtime::vm::engine_types::{AheadImages, LaneView, StableFacts};
use quench_runtime::bytecode::Function;

#[unsafe(no_mangle)]
fn quench_engine2_aheadimages_compile(program: &ResidualProgram) -> Result<AheadImages, String> { <AheadImages as AheadImagesLifted160>::compile(program) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_aheadimages_compile = quench_engine2_aheadimages_compile;
#[unsafe(no_mangle)]
fn quench_engine2_laneview_invalidate_shape_bindings(this: &LaneView) -> () { <LaneView as LaneViewLifted648>::invalidate_shape_bindings(this) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_laneview_invalidate_shape_bindings = quench_engine2_laneview_invalidate_shape_bindings;
#[unsafe(no_mangle)]
fn quench_engine2_fn_function_candidate(function: &Function, program: &ResidualProgram) -> bool { crate::js_compiled::function_candidate(function, program) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_fn_function_candidate = quench_engine2_fn_function_candidate;
#[unsafe(no_mangle)]
fn quench_engine2_fn_pays_native_entry(body: &quench_runtime::bytecode::Function) -> bool { crate::compiled::pays_native_entry(body) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_fn_pays_native_entry = quench_engine2_fn_pays_native_entry;
#[unsafe(no_mangle)]
fn quench_engine2_fn_subscribe_stable(stable: &StableFacts, code: &Rc<dyn quench_runtime::code_dependencies::DependentCode>, heap: &mut quench_runtime::heap::Heap, method_atoms: &mut quench_runtime::code_dependencies::CodeReaders) -> bool { crate::compiled::lane_compile::dependencies::subscribe_stable(stable, code, heap, method_atoms) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_fn_subscribe_stable = quench_engine2_fn_subscribe_stable;
#[unsafe(no_mangle)]
fn quench_engine2_fn_ahead_program_fingerprint(program: &ResidualProgram) -> u64 { crate::compiled::ahead_images::ahead_program_fingerprint(program) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_fn_ahead_program_fingerprint = quench_engine2_fn_ahead_program_fingerprint;
#[unsafe(no_mangle)]
fn quench_engine2_fn_reserve_lane_scratch(width: usize) -> () { crate::compiled::entry_cell::reserve_lane_scratch(width) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_fn_reserve_lane_scratch = quench_engine2_fn_reserve_lane_scratch;
#[unsafe(no_mangle)]
fn quench_engine2_fn_is_call_rejection(opcode: u16) -> bool { crate::compiled::meaning_compiled::is_call_rejection(opcode) }
const _: quench_runtime::vm::engine_calls2::signatures::quench_engine2_fn_is_call_rejection = quench_engine2_fn_is_call_rejection;

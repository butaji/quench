//! A lane body's code with its scalar entry beside it (`lane_region::scalar`): one executable
//! mapping holds both, the entry after the body at the next function alignment.

use super::*;

/// The code of a lane body, and where its scalar entry lies in it.
pub(crate) struct LaneImage {
    pub(super) code: Vec<u8>,
    pub(super) scalar: Option<usize>,
}

/// The alignment each function's code starts at.
const FUNCTION_ALIGN: usize = 16;

/// The code a compiled lane body may take before it is refused. The budget itself is enforced
/// before compiling, on the plan's IR (`Lowered::of`), as Cranelift gives no
/// code size short of a whole compile; this ceiling only bounds a body whose bytes per IR
/// instruction land far above the planned rate, so a body near its budget is kept rather than
/// compiled, refused, planned again and compiled a second time.
const LANE_BODY_CODE_CEILING: usize = 2 * crate::vm::compiled::tier_policy::LANE_BODY_MAX_CODE_BYTES;

/// The code of one function, compiled on the calling thread.
fn function_code(function: ir::Function, level: super::super::jit_backend::CompileLevel) -> Result<Vec<u8>, super::super::jit_backend::NativeCompileRejection> {
    let level = match level {
        super::super::jit_backend::CompileLevel::Optimized | super::super::jit_backend::CompileLevel::Quick if function.dfg.num_insts() > super::LANE_BACKTRACKING_MAX_INSTS =>
            super::super::jit_backend::CompileLevel::Large,
        level => level,
    };
    let isa = super::super::jit_backend::target_isa_level(level)
        .ok_or(super::super::jit_backend::NativeCompileRejection::Publish)?;
    compile_native_function(function, &*isa, LANE_BODY_CODE_CEILING, |code| Some(code.to_vec()))
}

/// The code of one lane body and of its `scalar` entry, compiled on the calling thread: where the
/// entry does not compile, neither does the image, as its body calls it.
pub(super) fn compile(function: ir::Function, scalar: Option<ir::Function>, level: super::super::jit_backend::CompileLevel) -> super::LaneCode {
    let mut code = function_code(function, level)?;
    let Some(scalar) = scalar else { return Ok(LaneImage { code, scalar: None }) };
    let at = code.len().next_multiple_of(FUNCTION_ALIGN);
    code.resize(at, 0);
    code.extend(function_code(scalar, level)?);
    Ok(LaneImage { code, scalar: Some(at) })
}

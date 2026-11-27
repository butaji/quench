//! Eager native publication. An eager program compiles its AOT image now and links it into each
//! VM that runs it (`aot_image::live::install`): the image's atoms and recipes are VM-specific, so
//! the image is carried as its file and linked at attach (`Vm::attach_native_js`).

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::ResidualProgram;
#[cfg(feature = "jit")]
use quench_runtime::vm::engine_types::AheadImages;
use super::admission::CompiledUnit;

#[cfg(feature = "jit")]
pub(crate) fn compile_eager_native(program: ResidualProgram, unit: CompiledUnit) -> Result<quench_runtime::NativeJsProgram, String> {
    // The unit is the gate's proof that an eager compile is admitted; the image lowers every
    // function the closed world proves (`AheadImages::compile`).
    let _ = unit;
    let images = AheadImages::compile(&program)?;
    if images.is_empty() { return Err("eager publication compiled no image".into()); }
    quench_runtime::NativeJsProgram::compile_eager_at(program, images)
}

/// An execution-only build cannot lower an image; it only loads one.
#[cfg(not(feature = "jit"))]
pub(crate) fn compile_eager_native(_: ResidualProgram, _: CompiledUnit) -> Result<quench_runtime::NativeJsProgram, String> {
    Err("eager native publication requires the jit feature".into())
}

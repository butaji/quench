//! Eager native publication. An eager program links its AOT image (`aot_image::live::install`)
//! into the VM that runs it; `compile_eager` has no host and so no VM, and the image's atoms and
//! recipes are VM-specific, so it cannot link yet. It reports that instead of publishing a
//! fallback body.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::bytecode::ResidualProgram;

pub(crate) fn compile_eager_native(program: ResidualProgram) -> Result<quench_runtime::NativeJsProgram, String> {
    let _ = program;
    Err("eager publication links its image into a VM, and compile_eager has none yet".into())
}

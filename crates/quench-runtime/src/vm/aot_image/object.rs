//! The object linker: an `AotImage` as a relocatable object (`emit_native_module`) whose
//! symbols are object relocations, plus the entry table `NativeJsProgram::from_static_image`
//! adopts and the recipe tables the loader resolves. `quench compile` links it.
//!
//! Owner: one pipeline.

use super::AotImage;

/// The object file of `image`, its symbols named under `namespace`.
pub(crate) fn emit(image: &AotImage, namespace: &str) -> Result<crate::vm::native_graph::NativeJsModule, String> {
    let _ = (image, namespace);
    todo!("one pipeline: object linker")
}

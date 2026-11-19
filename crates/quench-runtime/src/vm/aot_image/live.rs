//! The live linker: an `AotImage` placed in this process's executable memory, every symbol
//! resolved now (helpers, entry cells, semantic entries, and heap values and shapes built from
//! their recipes in `vm`), then published as each function's entry. Eager publication and
//! the ahead-image loader both use it.
//!
//! Owner: one pipeline.

use super::AotImage;

/// Links `image` into `vm` and publishes every function's entry.
pub(crate) fn install<H: crate::Host>(image: &AotImage, vm: &mut crate::vm::Vm<H>) -> Result<(), String> {
    let _ = (image, vm);
    todo!("one pipeline: live linker")
}

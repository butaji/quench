//! The base body of an AOT function: the whole function stitched from BaseJIT stencils
//! (`base_region::function`, as `--b-jit` builds it) but emitted relocatable, every hole that
//! names a run-time address a `Symbol`. A record with no stencil row takes the generic
//! semantic row: pinned registers written back, `Symbol::SemanticEntry(op)` called on
//! (context, pc, operands, result word), and control continuing at the PC its status names,
//! inside the body. The body never leaves to a per-PC loop; generic rows are counted so the
//! AOT gate can drive them to zero by staging the opcodes they stand for.
//!
//! Owner: base tier (relocatable stencils + generic row).

use super::{ImageTarget, symbols::SymbolTable};
use crate::bytecode::ResidualProgram;
use crate::compile::aot_facts::AotFacts;
use crate::vm::jit_backend::relocatable_code::RelocatableCode;

/// A base body: code, relocations, and where each record starts.
pub(crate) struct BaseImage {
    pub(crate) code: RelocatableCode,
    /// Byte offset in `code` of record `pc`'s first stencil; `NO_ENTRY` where no control
    /// enters it from outside the body (so no lane leave may target it).
    pub(crate) pc_offsets: Box<[u32]>,
    /// The records lowered as the generic semantic row, ascending.
    pub(crate) generic: Box<[u32]>,
}

/// `pc_offsets` value of a record nothing outside the body enters.
pub(crate) const NO_ENTRY: u32 = u32::MAX;

impl BaseImage {
    pub(crate) fn generic_rows(&self) -> usize { self.generic.len() }
}

/// The base body of function `id`.
pub(crate) fn lower(
    program: &ResidualProgram,
    facts: &AotFacts,
    target: ImageTarget,
    id: u32,
    symbols: &mut SymbolTable,
) -> Result<BaseImage, String> {
    let _ = (program, facts, target, id, symbols);
    todo!("base tier")
}

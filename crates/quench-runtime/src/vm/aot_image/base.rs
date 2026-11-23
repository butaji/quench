//! The base body of an AOT function: the whole function stitched from BaseJIT stencils
//! (`base_region::function`, as `--b-jit` builds it) but emitted relocatable, every hole that
//! names a run-time address a `Symbol`. A record with no stencil row takes the generic
//! semantic row: pinned registers written back, `Symbol::SemanticEntry(op)` called on
//! (context, pc, operands, result word), and control continuing at the PC its status names,
//! inside the body. The body never leaves to a per-PC loop; generic rows are counted so the
//! AOT gate can drive them to zero by staging the opcodes they stand for.
//!
//! Owner: base tier (relocatable stencils + generic row).

use super::{CallSite, ImageTarget, symbols::SymbolTable};
use crate::bytecode::ResidualProgram;
use crate::vm::jit_backend::relocatable_code::{RelocatableCode, Relocation};

/// A base body: code, relocations, and where each record starts.
pub(crate) struct BaseImage {
    pub(crate) code: RelocatableCode,
    /// Byte offset in `code` of record `pc`'s first stencil; `NO_ENTRY` where no control
    /// enters it from outside the body (so no lane leave may target it).
    pub(crate) pc_offsets: Box<[u32]>,
    /// The records lowered as the generic semantic row, ascending.
    pub(crate) generic: Box<[u32]>,
    /// The registers past the function's own its rows reach (`ImageBody::scratch`).
    pub(crate) scratch: u32,
}

/// `pc_offsets` value of a record nothing outside the body enters.
pub(crate) const NO_ENTRY: u32 = u32::MAX;

impl BaseImage {
    pub(crate) fn generic_rows(&self) -> usize { self.generic.len() }
}

/// The base body of function `id`: the BaseJIT body over the program alone (no profile), its calls
/// direct where `facts` prove their callee, its helper and call record addresses as `Reloc::Abs8`
/// relocations to the symbols the loader resolves; and those calls' sites (`AotImage::calls`).
pub(crate) fn lower(
    program: &ResidualProgram,
    target: &ImageTarget,
    id: u32,
    symbols: &mut SymbolTable,
) -> Result<(BaseImage, Vec<CallSite>), String> {
    // The target changes no base row: the body names no target's layout.
    let _ = target;
    let built = crate::vm::compiled::image_body(program, id)?;
    let relocations: Box<[Relocation]> = built.symbols.iter().map(|site| {
        let (namespace, key) = symbols.intern(site.symbol);
        Relocation { offset: site.offset, kind: site.kind, namespace, key, addend: 0 }
    }).collect();
    let mut pc_offsets = vec![NO_ENTRY; program.function(id as usize).instructions().count()];
    for &(pc, offset) in built.entries.iter() {
        pc_offsets[pc as usize] = u32::try_from(offset).map_err(|_| format!("code offset {offset} past the image's reach"))?;
    }
    let calls = built.calls.iter().map(|&(pc, callee, record)| CallSite { function: id, pc, callee, record }).collect();
    Ok((BaseImage { code: RelocatableCode { code: built.code.into(), relocations }, pc_offsets: pc_offsets.into(), generic: built.generic, scratch: u32::try_from(built.scratch).map_err(|_| "base body scratch past u32")? }, calls))
}

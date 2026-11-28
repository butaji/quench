//! The base body of an AOT function: the whole function stitched from BaseJIT stencils
//! (`base_region::function`, as the b-jit tier builds it) but emitted relocatable, every hole that
//! names a run-time address a `Symbol`. A record with no stencil row leaves, as a b-jit body's
//! does, and the lane's interpreter runs it: the image is the projection's first body, not a second
//! semantics. The records it leaves at are counted (`generic`) so the AOT gate drives them to zero
//! by staging the opcodes they stand for.
//!
//! Owner: base tier (relocatable stencils + generic row).

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::{CallSite, ImageTarget, symbols::SymbolTable};
use quench_runtime::bytecode::ResidualProgram;
use quench_runtime::vm::jit_backend::relocatable_code::{PatchKind, RelocatableCode, Relocation};

pub(crate) use quench_runtime::vm::engine_types::BaseImage;

/// `pc_offsets` value of a record nothing outside the body enters.
pub(crate) const NO_ENTRY: u32 = u32::MAX;

#[quench_macros::ext(BaseImageOps1)]
impl BaseImage {
    pub(crate) fn generic_rows(&self) -> usize { self.generic.len() }
}

/// The base body of function `id`: the BaseJIT body over the program alone (no profile), its calls
/// direct where `facts` prove their callee, its helper and call record addresses as `PatchKind::Abs8`
/// relocations to the symbols the loader resolves; and those calls' sites (`AotImage::calls`).
pub(crate) fn lower(
    program: &ResidualProgram,
    target: &ImageTarget,
    id: u32,
    symbols: &mut SymbolTable,
) -> Result<(BaseImage, Vec<CallSite>), String> {
    // The body is the target CPU's stencils, not the compiling CPU's.
    let built = quench_bjit::rustc::with_library(target.x86_64_v3_stencils(), || crate::compiled::image_body(program, id))?;
    let relocations: Box<[Relocation]> = built.symbols.iter().map(|site| {
        let (namespace, key) = symbols.intern(site.symbol);
        // Apple AArch64 names a helper call as a branch hole; a guard's shape is a 32-bit immediate; a
        // PC-relative field keeps its addend; every other site is a 64-bit word.
        let (kind, addend) = match site.class {
            quench_bjit::BRANCH26 => (PatchKind::Arm64Call, 0),
            quench_bjit::ABS32 => (PatchKind::Abs4, 0),
            quench_bjit::REL32 => (PatchKind::X86PCRel4, site.addend),
            _ => (PatchKind::Abs8, 0),
        };
        Relocation { offset: site.offset, kind, namespace, key, addend }
    }).collect();
    let mut pc_offsets = vec![NO_ENTRY; program.function(id as usize).instructions().count()];
    for &(pc, offset) in built.entries.iter() {
        pc_offsets[pc as usize] = u32::try_from(offset).map_err(|_| format!("code offset {offset} past the image's reach"))?;
    }
    let calls = built.calls.iter().map(|&(pc, callee, record)| CallSite { function: id, pc, callee, record }).collect();
    Ok((BaseImage { code: RelocatableCode { code: built.code.into(), relocations }, pc_offsets: pc_offsets.into(), generic: built.generic, scratch: u32::try_from(built.scratch).map_err(|_| "base body scratch past u32")? }, calls))
}

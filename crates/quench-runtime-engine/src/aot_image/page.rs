//! An image body's absolute words, as the page references Mach-O can link in a text section.
//! Cranelift and the stitched stencils materialize a symbol's value as a literal word the code
//! loads (`ldr xN, word ; b past ; word`). An absolute word in `__text` is legal on x86-64 and
//! on ELF AArch64, where a word relocation fills it, but Mach-O AArch64 forbids the relocation
//! and the image is linked by ld64. Each such word therefore becomes `adrp xN, page ; add xN,
//! xN, offset`, with the two relocations a linker fills (`Arm64Page21`, `Arm64PageOff12`) and no
//! absolute word left behind. The register the load wrote is the one the pair writes, so the
//! code after the word reads exactly what it did.
//!
//! Owner: one pipeline (image lowering).

use quench_runtime::vm::jit_backend::relocatable_code::RelocatableCode;
#[cfg(target_arch = "aarch64")]
use quench_runtime::vm::jit_backend::relocatable_code::{RelocKind, Relocation};

/// AArch64 `ldr xN, label` and the branch over its word.
const A64_LDR_LITERAL_MASK: u32 = 0xFF00_0000;
const A64_LDR_LITERAL: u32 = 0x5800_0000;
const A64_LDR_IMM19_SHIFT: u32 = 5;
const A64_LDR_IMM19_BITS: u32 = 19;
const A64_BRANCH_MASK: u32 = 0xFC00_0000;
const A64_BRANCH: u32 = 0x1400_0000;
const A64_BRANCH_IMM26_BITS: u32 = 26;
/// `adrp xN, page` and `add xN, xN, #page-offset`: the pair a page reference links into.
const A64_ADRP: u32 = 0x9000_0000;
const A64_ADD_REGISTER: u32 = 0x9100_0000;
const A64_NOP: u32 = 0xD503_201F;
const A64_REGISTER_MASK: u32 = 0x1F;
const A64_WORD: usize = 4;
/// The load, its branch, and any alignment padding before the word.
const A64_HOLE_SEARCH_BYTES: usize = 16;

/// Rewrites every absolute word of `code` into a page reference, where the target writes a
/// relocatable object with Mach-O's rules. On other targets a word stays a word, which its
/// linker fills.
pub(crate) fn rewrite(code: &mut RelocatableCode) -> Result<(), String> {
    #[cfg(target_arch = "aarch64")]
    {
        let mut relocations = Vec::with_capacity(code.relocations.len());
        for relocation in code.relocations.iter() {
            if relocation.kind != RelocKind::Abs8 {
                relocations.push(*relocation);
                continue;
            }
            let [page, offset] = page_reference(&mut code.code, relocation.offset)?;
            relocations.push(Relocation { offset: page, kind: RelocKind::Arm64Page21, ..*relocation });
            relocations.push(Relocation { offset, kind: RelocKind::Arm64PageOff12, ..*relocation });
        }
        code.relocations = relocations.into_boxed_slice();
    }
    #[cfg(not(target_arch = "aarch64"))]
    let _ = code;
    Ok(())
}

/// The `bits`-wide field of `word` as a signed offset.
#[cfg(target_arch = "aarch64")]
fn signed(word: u32, bits: u32) -> i64 {
    i64::from((word << (32 - bits)) as i32) >> (32 - bits)
}

/// `at`'s instruction of `code`.
#[cfg(target_arch = "aarch64")]
fn instruction(code: &[u8], at: usize) -> Result<u32, String> {
    let word = code.get(at..at + A64_WORD).ok_or("a value hole lies outside its code")?;
    Ok(u32::from_le_bytes(word.try_into().expect("a four-byte instruction")))
}

/// Rewrites the literal word at `literal` (and the load and branch around it) into a page
/// reference, returning the offsets of its `adrp` and `add` instructions. The shape is checked
/// before it is replaced, so a body whose word is not the load-branch-literal form is refused
/// rather than mislinked.
#[cfg(target_arch = "aarch64")]
fn page_reference(code: &mut [u8], literal: usize) -> Result<[usize; 2], String> {
    let (load_at, load) = (0..A64_HOLE_SEARCH_BYTES / A64_WORD).find_map(|back| {
        let at = literal.checked_sub(A64_WORD * (back + 1))?;
        let word = instruction(code, at).ok()?;
        let target = at as i64 + signed(word >> A64_LDR_IMM19_SHIFT & ((1 << A64_LDR_IMM19_BITS) - 1), A64_LDR_IMM19_BITS) * A64_WORD as i64;
        (word & A64_LDR_LITERAL_MASK == A64_LDR_LITERAL && target == literal as i64).then_some((at, word))
    }).ok_or("a symbolic word has no literal load")?;
    let branch_at = load_at + A64_WORD;
    let branch = instruction(code, branch_at)?;
    let continuation = literal + 2 * A64_WORD;
    let target = branch_at as i64 + signed(branch & ((1 << A64_BRANCH_IMM26_BITS) - 1), A64_BRANCH_IMM26_BITS) * A64_WORD as i64;
    if branch & A64_BRANCH_MASK != A64_BRANCH || target != continuation as i64 {
        return Err("a symbolic word's load is not followed by the branch over it".to_string());
    }
    let padding = code.get(branch_at + A64_WORD..literal).ok_or("a symbolic word lies outside its code")?;
    if !padding.chunks_exact(A64_WORD).all(|word| u32::from_le_bytes(word.try_into().expect("a four-byte instruction")) == A64_NOP) {
        return Err("a symbolic word is not preceded only by its load, branch and padding".to_string());
    }
    let register = load & A64_REGISTER_MASK;
    let field = code.get_mut(load_at..continuation).ok_or("a symbolic word lies outside its code")?;
    field[..A64_WORD].copy_from_slice(&(A64_ADRP | register).to_le_bytes());
    field[A64_WORD..2 * A64_WORD].copy_from_slice(&(A64_ADD_REGISTER | (register << 5) | register).to_le_bytes());
    for at in (2 * A64_WORD..field.len()).step_by(A64_WORD) {
        field[at..at + A64_WORD].copy_from_slice(&A64_NOP.to_le_bytes());
    }
    Ok([load_at, branch_at])
}

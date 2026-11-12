//! BaseJIT: stencils built by rustc/LLVM at build time from the same staged rows the interpreter
//! runs (`stencils/`, see `build.rs`), copied and patched at run time (copy-and-patch). A row's
//! static operands are *holes*: extern statics whose relocations `build.rs` reads from the
//! object, and every successor is a `become` the relocation of which is a `jmp rel32` (x86-64)
//! or a `b`/`b.cond` (AArch64). Stencils use `rust-preserve-none`; the register map (frame,
//! context and pins) is `stencils/rows.rs`'s, assigned by LLVM, and `PINNED` and `PINNED_F`
//! count the pins while their indexes stay abstract (`stencils/codes.rs`).

pub mod class;
pub mod patch;
pub mod numeric;
pub mod stub;
pub use class::{ABS64, BRANCH14, BRANCH19, BRANCH26, MOVW, REL32};
pub use patch::{HOLES, Imm32, PatchError, patch_successor, patch_value};
pub use stub::{EXIT_STUB_LEN, exit_stub};

/// Pinned int registers a stencil takes, and the double registers (xmm0 to xmm7) after them.
pub const PINNED: usize = 4;
pub const PINNED_F: usize = 8;

/// Successor hole indexes.
pub const NEXT: u32 = 0;
pub const TAKEN: u32 = 1;
/// Exit hole indexes start here: exit `kind` is hole `EXIT + kind`, so each guard kind reaches
/// its own exit stub (an `ExitSite` id), not one shared hole.
pub const EXIT: u32 = 2;

/// `jmp rel32`: the opcode byte, then the displacement a relocation patches.
const JMP_REL32_LEN: usize = 5;

/// A relocation of a stencil: a value hole (`ABS64`, `MOVW`) or a successor transfer (`REL32`,
/// `BRANCH*`), patched as its `class` says. `offset` is the patched field (x86-64 `rel32`) or
/// instruction (AArch64).
#[derive(Clone, Debug)]
pub struct Reloc {
    pub offset: u32,
    pub class: u8,
    pub index: u32,
    pub addend: i64,
}

/// One stencil: its code, relocations and 32-bit hole fields.
#[derive(Clone, Debug)]
pub struct Stencil {
    pub code: Vec<u8>,
    pub relocs: Vec<Reloc>,
    /// The code's 32-bit hole fields.
    pub imm32: Vec<Imm32>,
    /// Where the trailing jump to the next row starts when it ends the code, so adjacency drops it.
    pub next_jump: Option<usize>,
    /// Bytes of that tail.
    pub tail_len: usize,
    /// Bytes of the code.
    pub raw_len: usize,
}

/// One row of a program: its stencil, hole values and branch target row.
pub struct Row<'a> {
    pub stencil: &'a Stencil,
    pub hole: &'a dyn Fn(u32) -> u64,
    pub taken: Option<usize>,
}

/// Each row's start offset and the code length once copied back to back, a `jmp next` dropped
/// where the next stencil follows.
fn layout(rows: &[Row]) -> (Vec<usize>, Vec<usize>, usize) {
    let (mut starts, mut sizes, mut len) = (Vec::new(), Vec::new(), 0);
    for (k, row) in rows.iter().enumerate() {
        starts.push(len);
        let drop = if row.stencil.next_jump.is_some() && k + 1 < rows.len() { row.stencil.tail_len } else { 0 };
        sizes.push(row.stencil.code.len() - drop);
        len += sizes[k];
    }
    (starts, sizes, len)
}

/// Copy `rows` into `out` back to back and patch their holes; `exit` gives each row's exit
/// stub address for an exit kind. Returns the code length and each row's start offset.
///
/// # Safety
/// `out` must be writable for the whole code.
pub unsafe fn copy_patch(rows: &[Row], out: *mut u8, exit: impl Fn(usize, u32) -> usize) -> (usize, Vec<usize>) {
    let (starts, sizes, len) = layout(rows);
    for (k, row) in rows.iter().enumerate() {
        // SAFETY: the caller guarantees `out` holds the layout computed above, and the rows' spans are disjoint.
        let code = unsafe { std::slice::from_raw_parts_mut(out.add(starts[k]), sizes[k]) };
        code.copy_from_slice(&row.stencil.code[..sizes[k]]);
        for field in row.stencil.imm32.iter().filter(|f| (f.offset as usize) < sizes[k]) {
            let at = field.offset as usize;
            code[at..at + size_of::<u32>()].copy_from_slice(&field.value((row.hole)(field.index) as u32).to_le_bytes());
        }
        for r in row.stencil.relocs.iter().filter(|r| (r.offset as usize) < sizes[k]) {
            let place = out as usize + starts[k] + r.offset as usize;
            if !class::is_successor(r.class) {
                patch_value(code, r.offset as usize, r.class, (row.hole)(r.index));
                continue;
            }
            let target = match r.index {
                NEXT => out as usize + starts[k + 1],
                TAKEN => out as usize + starts[row.taken.expect("a branch row names its target")],
                index => exit(k, index - EXIT),
            };
            patch_successor(code, r.offset as usize, r.class, target as i64 + r.addend - place as i64).expect("a successor within branch reach");
        }
    }
    (len, starts)
}

/// Stencils compiled by rustc from `stencils/rows.rs` (see `build.rs`), as [`Stencil`]s.
pub mod rustc {
    use super::{class, Imm32, Reloc, Stencil, JMP_REL32_LEN, NEXT};
    use crate::patch::INSN_BYTES;

    /// A stencil as `build.rs` read it from the object: relocations as (offset, class, index, addend).
    pub(crate) struct RawStencil {
        pub(crate) name: &'static str,
        pub(crate) code: &'static [u8],
        pub(crate) relocs: &'static [(u16, u8, u8, i32)],
    }

    include!(concat!(env!("OUT_DIR"), "/rustc_stencils.rs"));

    /// The stencil of the row function `name`, if the object has it.
    pub fn stencil(name: &str) -> Option<Stencil> {
        let raw = &RAW[RAW.binary_search_by(|raw| raw.name.cmp(name)).ok()?];
        let (mut relocs, mut imm32) = (Vec::new(), Vec::new());
        for &(offset, kind, index, addend) in raw.relocs {
            let (offset, index) = (u32::from(offset), u32::from(index));
            match kind {
                class::ABS32 => imm32.push(Imm32 { offset, index, negate: false, addend: addend as u32 }),
                other => relocs.push(Reloc { offset, class: other, index, addend: i64::from(addend) }),
            }
        }
        let len = raw.code.len();
        let (next_jump, tail_len) = trailing_next_jump(&relocs, len).map_or((None, 0), |(at, tail)| (Some(at), tail));
        Some(Stencil { code: raw.code.to_vec(), relocs, imm32, next_jump, tail_len, raw_len: len })
    }

    /// Where the jump to the next row starts and its length, when it is the code's last
    /// instruction: `jmp rel32` (the relocation is its last four bytes) or AArch64 `b`.
    fn trailing_next_jump(relocs: &[Reloc], len: usize) -> Option<(usize, usize)> {
        relocs.iter().filter(|r| r.index == NEXT).find_map(|r| {
            let at = r.offset as usize;
            match r.class {
                class::REL32 if at + size_of::<i32>() == len => Some((at - 1, JMP_REL32_LEN)),
                class::BRANCH26 if at + INSN_BYTES == len => Some((at, INSN_BYTES)),
                _ => None,
            }
        })
    }
}

#[cfg(test)]
mod rustc_tests {
    use super::{numeric::library, rustc};

    /// Every row the library names was built, and canonicalizing is idempotent.
    #[test]
    fn the_library_is_built() {
        for code in library::library() {
            assert_eq!(library::canonical(code), code);
            assert!(rustc::stencil(&library::name(code)).is_some(), "{code:?}");
        }
        for name in ["entry_thunk", "leave_word", "leave_pop"] {
            assert!(rustc::stencil(name).is_some(), "{name}");
        }
        for code in library::heap_library() {
            assert_eq!(library::heap_canonical(code), code);
            assert!(rustc::stencil(&library::heap_name(code)).is_some(), "{code:?}");
        }
    }

    /// Every patched field of every stencil lies inside its code, and names a hole the copy
    /// patches: the range `copy_patch` writes through.
    #[test]
    fn patch_ranges_lie_inside_the_code() {
        let names = library::library().into_iter().map(library::name).chain(library::heap_library().into_iter().map(library::heap_name));
        for name in names.chain(["entry_thunk", "leave_word", "leave_pop"].map(String::from)) {
            let stencil = rustc::stencil(&name).unwrap_or_else(|| panic!("{name}"));
            for field in &stencil.imm32 {
                assert!(field.offset as usize + 4 <= stencil.code.len() && (field.index as usize) < crate::HOLES, "{name}: {field:?}");
            }
            for reloc in &stencil.relocs {
                let width = if reloc.class == crate::ABS64 { 8 } else { 4 };
                assert!(reloc.offset as usize + width <= stencil.code.len(), "{name}: {reloc:?}");
                assert!(crate::class::is_successor(reloc.class) || (reloc.index as usize) < crate::HOLES, "{name}: {reloc:?}");
            }
        }
    }
}

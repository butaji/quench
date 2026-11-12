//! BaseJIT: stencils built by rustc/LLVM at build time from the same staged rows the interpreter
//! runs (`stencils/`, see `build.rs`), copied and patched at run time (copy-and-patch). A row's
//! static operands are *holes*: extern statics whose relocations `build.rs` reads from the
//! object, and every successor is a `become` the relocation of which is a `jmp rel32`.
//! Stencils use `rust-preserve-none` with the frame in r12, the context in r13 and the pinned
//! values in the registers `PINNED` and `PINNED_F` count.

mod patch;
pub mod numeric;
pub use patch::{padded_holes, HOLES, Imm32};

/// Pinned int registers a stencil takes, and the double registers (xmm0 to xmm7) after them.
pub const PINNED: usize = 4;
pub const PINNED_F: usize = 8;

/// Namespace of value holes (64-bit immediates) and of successor holes.
const HOLE_VALUE: u32 = 1;
const HOLE_CODE: u32 = 2;
/// Successor hole indexes.
pub const NEXT: u32 = 0;
pub const TAKEN: u32 = 1;
/// Exit hole indexes start here: exit `kind` is hole `EXIT + kind`, so each guard kind reaches
/// its own exit stub (an `ExitSite` id), not one shared hole.
pub const EXIT: u32 = 2;

/// `ret`: what a leave routine ends with.
pub const RET: u8 = 0xc3;

/// `jmp rel32`: the opcode byte, then the displacement a relocation patches.
const JMP_REL32_LEN: usize = 5;

/// A relocation of a stencil: a 64-bit hole value, or a 32-bit pc-relative successor.
#[derive(Clone, Debug)]
pub struct Reloc {
    pub offset: u32,
    pub pc_relative: bool,
    pub namespace: u32,
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
    /// Where the trailing `jmp next` starts when it ends the code, so adjacency drops it.
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

/// Bytes a 32-bit immediate or pc-relative patch writes, and an absolute one.
pub const IMM32_WIDTH: usize = 4;
const ABS64_WIDTH: usize = 8;

/// Every patch lands inside the code it patches, at link time (a row's copied code: a write
/// past it would overwrite the next row's first bytes) and at run time (an inline-cache
/// re-patch of published code, which must end inside the body's own bytes).
pub fn patch_ends_inside(offset: u32, width: usize, size: usize, what: &str) {
    assert!(offset as usize + width <= size, "{what} patch ends past its row's code");
}

/// Copy `rows` into `out` back to back and patch their holes; `exit` gives each row's exit
/// stub address for an exit kind. Returns the code length and each row's start offset.
///
/// # Safety
/// `out` must be writable for the whole code.
pub unsafe fn copy_patch(rows: &[Row], out: *mut u8, exit: impl Fn(usize, u32) -> usize) -> (usize, Vec<usize>) {
    let (starts, sizes, len) = layout(rows);
    for (k, row) in rows.iter().enumerate() {
        // SAFETY: the caller guarantees `out` holds the layout computed above.
        unsafe {
            let base = out.add(starts[k]);
            std::ptr::copy_nonoverlapping(row.stencil.code.as_ptr(), base, sizes[k]);
            for field in row.stencil.imm32.iter().filter(|f| (f.offset as usize) < sizes[k]) {
                patch_ends_inside(field.offset, IMM32_WIDTH, sizes[k], "an immediate");
                std::ptr::write_unaligned(base.add(field.offset as usize).cast::<u32>(), field.value((row.hole)(field.index) as u32));
            }
            for r in row.stencil.relocs.iter().filter(|r| (r.offset as usize) < sizes[k]) {
                let place = base.add(r.offset as usize);
                patch_ends_inside(r.offset, if r.pc_relative { IMM32_WIDTH } else { ABS64_WIDTH }, sizes[k], "a relocation");
                if !r.pc_relative {
                    std::ptr::write_unaligned(place.cast::<u64>(), (row.hole)(r.index));
                    continue;
                }
                let target = match r.index {
                    NEXT => out as usize + starts[k + 1],
                    TAKEN => out as usize + starts[row.taken.expect("a branch row names its target")],
                    index => exit(k, index - EXIT),
                };
                let displacement = i32::try_from(target as i64 + r.addend - place as i64).expect("a successor within rel32 reach");
                std::ptr::write_unaligned(place.cast::<i32>(), displacement);
            }
        }
    }
    (len, starts)
}

/// Stencils compiled by rustc from `stencils/rows.rs` (see `build.rs`), as [`Stencil`]s.
pub mod rustc {
    use super::{Imm32, Reloc, Stencil, HOLE_CODE, NEXT};

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
        for &(offset, class, index, addend) in raw.relocs {
            let (offset, index) = (u32::from(offset), u32::from(index));
            match class {
                0 => imm32.push(Imm32 { offset, index, negate: false, addend: addend as u32 }),
                1 => relocs.push(Reloc { offset, pc_relative: true, namespace: HOLE_CODE, index, addend: i64::from(addend) }),
                _ => relocs.push(Reloc { offset, pc_relative: false, namespace: super::HOLE_VALUE, index, addend: i64::from(addend) }),
            }
        }
        let len = raw.code.len();
        let next_jump = relocs.iter().find(|r| r.index == NEXT && r.offset as usize + REL32 == len).map(|r| r.offset as usize - 1);
        Some(Stencil { code: raw.code.to_vec(), relocs, imm32, next_jump, tail_len: super::JMP_REL32_LEN, raw_len: len })
    }

    const REL32: usize = 4;
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
                let width = if reloc.pc_relative { 4 } else { 8 };
                assert!(reloc.offset as usize + width <= stencil.code.len(), "{name}: {reloc:?}");
                assert!(reloc.pc_relative || (reloc.index as usize) < crate::HOLES, "{name}: {reloc:?}");
            }
        }
    }

    /// The cache rows (`stencils/ic_rows.rs`) patch the holes `base_rows` fills: a get its
    /// destination, receiver, shape and slot offset (a holder read also the holder's shape), a
    /// store its receiver, value, atom, shape and slot offset; a chained row's hit takes the
    /// branch target and the last entry's miss takes the exit.
    #[test]
    fn cache_rows_patch_their_entry_holes_and_take_their_successors() {
        use super::numeric::library::{heap_canonical, heap_name, IC_GET_HOLDER_ROW, IC_GET_ROW, IC_SET_ROW, MEM};
        let row = |heap, chained| {
            let stencil = rustc::stencil(&heap_name(heap_canonical((heap, chained, MEM, MEM, MEM)))).expect("a cache row");
            let mut values: Vec<u32> = stencil.imm32.iter().map(|field| field.index).collect();
            values.sort_unstable();
            values.dedup();
            let successors: Vec<u32> = stencil.relocs.iter().filter(|reloc| reloc.pc_relative).map(|reloc| reloc.index).collect();
            (values, successors)
        };
        assert_eq!(row(IC_GET_ROW, false).0, [0, 1, 2, 3]);
        assert_eq!(row(IC_GET_HOLDER_ROW, false).0, [0, 1, 2, 3, 4]);
        assert_eq!(row(IC_SET_ROW, false).0, [0, 1, 2, 3, 4]);
        for heap in [IC_GET_ROW, IC_GET_HOLDER_ROW, IC_SET_ROW] {
            let (last, chained) = (row(heap, false).1, row(heap, true).1);
            assert!(last.contains(&crate::EXIT) && !last.contains(&crate::TAKEN), "{heap}: {last:?}");
            assert!(chained.contains(&crate::TAKEN) && chained.contains(&crate::NEXT), "{heap}: {chained:?}");
        }
    }
}

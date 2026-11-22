//! BaseJIT: stencils built by rustc/LLVM at build time from the same staged rows the interpreter
//! runs (`stencils/`, see `build.rs`), copied and patched at run time (copy-and-patch). A row's
//! static operands are *holes*: extern statics whose relocations `build.rs` reads from the
//! object, and every successor is a `become` the relocation of which is a `jmp rel32` (x86-64)
//! or a `b`/`b.cond` (AArch64). Stencils use `rust-preserve-none`; the register map (frame,
//! context and pins) is `stencils/rows.rs`'s, assigned by LLVM, and `PINNED` and `PINNED_F`
//! count the pins while their indexes stay abstract (`stencils/codes.rs`).

#[macro_use]
#[path = "../stencils/row_catalog.rs"]
mod row_catalog;
pub mod class;
pub mod patch;
#[cfg(test)]
#[allow(dead_code)] // the tests read some of the build's weights
#[path = "../stencils/shards.rs"]
mod shards;
pub mod numeric;
pub mod call;
pub mod call_record;
pub mod shared_words;
pub mod stub;
// The projection seal the shared number rows name (`vm/operations/number_rows.rs`).
#[allow(dead_code, unused_macros)]
#[path = "../../quench-stack/src/projection.rs"]
mod projection;
#[path = "../stencils/wasm_kinds.rs"]
mod wasm_kinds;
#[path = "../stencils/wasm_families.rs"]
mod wasm_families;
#[path = "../stencils/wasm_layout.rs"]
mod wasm_layout;
#[path = "../stencils/wasm_runtime_codes.rs"]
mod wasm_runtime_codes;
#[path = "../stencils/wasm_library.rs"]
mod wasm_library;
/// The Wasm rows and codes the runtime reads, one namespace over the files that define them
/// (`stencils/wasm_kinds.rs`, `wasm_families.rs`, `wasm_layout.rs`, `wasm_runtime_codes.rs`, `wasm_library.rs`).
pub mod wasm {
    pub use crate::{wasm_families::*, wasm_kinds::*, wasm_layout::*, wasm_library::*, wasm_runtime_codes::*};
}
pub use class::{EXIT, LOCAL, NEXT, TAKEN, ABS64, BRANCH14, BRANCH19, BRANCH26, MOVW, REL32};
pub use patch::{HOLES, Imm32, padded_holes, PatchError, patch_successor, patch_value};
pub use stub::{EXIT_STUB_LEN, exit_stub};

/// Pinned int registers a stencil takes, and the double registers (xmm0 to xmm7) after them.
pub const PINNED: usize = 4;
pub const PINNED_F: usize = 8;

/// `jmp rel32`: the opcode byte, then the displacement a relocation patches. The one home of the
/// length: the stitcher's jump drop, the exit stub and a resume stub's jump all derive from it.
pub const JMP_REL32_LEN: usize = 1 + size_of::<u32>();
/// The pc-relative addend of a rel32 field that ends its instruction.
const REL32_ADDEND: i64 = -4;

/// The machine code of a bare return, which a layout test places as a row's whole body.
#[cfg(target_arch = "x86_64")]
pub const RET: u8 = 0xc3;

/// The machine code of a bare return on AArch64 (`ret`, 0xd65f03c0), little-endian, which a layout
/// test places as a row's whole body.
#[cfg(target_arch = "aarch64")]
pub const RET: [u8; 4] = 0xd65f_03c0u32.to_le_bytes();

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
    /// Where the trailing jump to the next row starts when it ends the hot part, so adjacency drops it.
    pub next_jump: Option<usize>,
    /// Bytes of that tail.
    pub tail_len: usize,
    /// Bytes of the code.
    pub raw_len: usize,
    /// Bytes of the hot part, which `code` starts with; the rest is the cold part, which the
    /// stitcher places after every hot part (`LOCAL` relocations reach across).
    pub hot_len: usize,
}

/// One row of a program: its stencil, hole values and branch target row.
pub struct Row<'a> {
    pub stencil: &'a Stencil,
    pub hole: &'a [u64; HOLES],
    pub taken: Option<usize>,
    /// A row only a miss reaches (an inline-cache row's fallback): placed after every other row's
    /// hot part, so the hot rows around it stay back to back.
    pub cold: bool,
}

/// Where a row's parts go: the hot parts back to back (a jump to the next row dropped where the
/// next row follows), then every cold part.
struct Layout {
    hot: Vec<usize>,
    hot_sizes: Vec<usize>,
    cold: Vec<usize>,
    len: usize,
}

fn layout(rows: &[Row]) -> Layout {
    let n = rows.len();
    let order: Vec<usize> = (0..n).filter(|&k| !rows[k].cold).chain((0..n).filter(|&k| rows[k].cold)).collect();
    let mut follows = vec![None; n];
    for pair in order.windows(2) {
        follows[pair[0]] = Some(pair[1]);
    }
    let (mut hot, mut hot_sizes, mut len) = (vec![0; n], vec![0; n], 0);
    for &k in &order {
        hot[k] = len;
        let stencil = rows[k].stencil;
        let drop = match trailing_jump(&stencil.code, &stencil.relocs, stencil.hot_len) {
            Some((NEXT, tail)) if k + 1 < n && follows[k] == Some(k + 1) => tail,
            Some((TAKEN, tail)) if follows[k].is_some() && follows[k] == Some(rows[k].taken.unwrap_or(k + 1)) => tail,
            _ => 0,
        };
        hot_sizes[k] = rows[k].stencil.hot_len - drop;
        len += hot_sizes[k];
    }
    let mut cold = vec![0; n];
    for &k in &order {
        cold[k] = len;
        len += rows[k].stencil.code.len() - rows[k].stencil.hot_len;
    }
    Layout { hot, hot_sizes, cold, len }
}

/// The successor the unconditional jump ending a hot part names (`NEXT` or `TAKEN`) and its length:
/// the jump is the hot part's last bytes, so it starts `len` bytes before `hot_len`. A rel32 counts
/// only after the `0xE9` opcode and a `b` only with the branch encoding, so a rel32 that is the tail
/// of some other instruction (a `jcc rel32`) is never taken for a jump and its bytes are never
/// dropped. The stitcher drops it where the row it names is placed right behind; `rustc` uses the
/// same check for a stencil's `next_jump`.
fn trailing_jump(code: &[u8], relocs: &[Reloc], hot_len: usize) -> Option<(u32, usize)> {
    relocs.iter().filter(|r| r.index == NEXT || r.index == TAKEN).find_map(|r| {
        let at = r.offset as usize;
        match r.class {
            class::REL32 if at + IMM32_WIDTH == hot_len && at >= 1 && code[at - 1] == class::JMP_REL32 => Some((r.index, JMP_REL32_LEN)),
            class::BRANCH26 if at + patch::INSN_BYTES == hot_len && code[at + 3] & 0xfc == 0x14 => Some((r.index, patch::INSN_BYTES)),
            _ => None,
        }
    })
}

impl Layout {
    /// The part of row `k` holding offset `at`: its start in the output, its copied size, and the
    /// offset of the part's first byte in the row's code.
    fn part(&self, rows: &[Row], k: usize, at: usize) -> (usize, usize, usize) {
        let stencil = rows[k].stencil;
        if at < stencil.hot_len { (self.hot[k], self.hot_sizes[k], 0) } else { (self.cold[k], stencil.code.len() - stencil.hot_len, stencil.hot_len) }
    }

    /// Where offset `at` of row `k`'s code lands: in its hot part (an offset in a dropped jump
    /// lands on the next row, which follows) or its cold part.
    fn address(&self, rows: &[Row], k: usize, at: usize) -> usize {
        let (start, _, base) = self.part(rows, k, at);
        start + at - base
    }

    /// Whether the offset `at` of row `k` is copied: not inside a dropped tail.
    fn kept(&self, rows: &[Row], k: usize, at: usize) -> bool {
        at >= rows[k].stencil.hot_len || at < self.hot_sizes[k]
    }
}

/// Bytes a 32-bit immediate or pc-relative patch writes.
pub const IMM32_WIDTH: usize = 4;
/// Bytes a 64-bit absolute patch writes.
const ABS64_WIDTH: usize = 8;

/// Every patch lands inside the code it patches, at link time (a row's copied part: a write
/// past it would overwrite the next part's first bytes) and at run time (an inline-cache
/// re-patch of published code, which must end inside the body's own bytes).
pub fn patch_ends_inside(offset: u32, width: usize, size: usize, what: &str) {
    assert!(offset as usize + width <= size, "{what} patch ends past its row's code");
}

/// The width of the field a relocation of `class` patches.
const fn reloc_width(class: u8) -> usize {
    if class == ABS64 { ABS64_WIDTH } else { IMM32_WIDTH }
}

/// The address a pc-relative relocation of row `k` names, and the addend its displacement takes.
fn successor_target(layout: &Layout, rows: &[Row], out: usize, k: usize, r: &Reloc, exit: &impl Fn(usize, u32) -> usize) -> (usize, i64) {
    match r.index {
        NEXT => (out + layout.hot[k + 1], r.addend),
        // A row that names no target never takes this edge (a helper row whose step cannot branch): it falls through.
        TAKEN => (out + layout.hot[rows[k].taken.unwrap_or(k + 1)], r.addend),
        LOCAL => (out + layout.address(rows, k, r.addend as usize), REL32_ADDEND),
        index => (exit(k, index - EXIT), r.addend),
    }
}

/// Copy row `k`'s hot and cold parts to their places in `out`.
///
/// # Safety
/// `out` must hold `layout`, whose parts' spans are disjoint.
unsafe fn copy_parts(layout: &Layout, stencil: &Stencil, k: usize, out: *mut u8) {
    // SAFETY: as the caller guarantees.
    unsafe {
        std::ptr::copy_nonoverlapping(stencil.code.as_ptr(), out.add(layout.hot[k]), layout.hot_sizes[k]);
        std::ptr::copy_nonoverlapping(stencil.code[stencil.hot_len..].as_ptr(), out.add(layout.cold[k]), stencil.code.len() - stencil.hot_len);
    }
}

/// Copy `rows` into `out`, every hot part back to back and then every cold part, and patch
/// their holes; `exit` gives each row's exit stub address for an exit kind. Returns the code
/// length and each row's start offset.
///
/// # Safety
/// `out` must be writable for the whole code.
pub unsafe fn copy_patch(rows: &[Row], out: *mut u8, exit: impl Fn(usize, u32) -> usize) -> (usize, Vec<usize>) {
    let layout = layout(rows);
    for (k, row) in rows.iter().enumerate() {
        let stencil = row.stencil;
        // SAFETY: the caller guarantees `out` holds the layout computed above.
        unsafe { copy_parts(&layout, stencil, k, out) };
        let part_slice = |at: usize| {
            let (start, size, base) = layout.part(rows, k, at);
            // SAFETY: as above; the part is `size` bytes at `start`.
            (unsafe { std::slice::from_raw_parts_mut(out.add(start), size) }, base)
        };
        for field in stencil.imm32.iter().filter(|f| layout.kept(rows, k, f.offset as usize)) {
            let (code, base) = part_slice(field.offset as usize);
            let at = field.offset as usize - base;
            patch_ends_inside(at as u32, IMM32_WIDTH, code.len(), "an immediate");
            code[at..at + IMM32_WIDTH].copy_from_slice(&field.value(row.hole[field.index as usize] as u32).to_le_bytes());
        }
        for r in stencil.relocs.iter().filter(|r| layout.kept(rows, k, r.offset as usize)) {
            let (code, base) = part_slice(r.offset as usize);
            let at = r.offset as usize - base;
            patch_ends_inside(at as u32, reloc_width(r.class), code.len(), "a relocation");
            if !class::is_successor(r.class) {
                patch_value(code, at, r.class, row.hole[r.index as usize]);
                continue;
            }
            let place = out as usize + layout.address(rows, k, r.offset as usize);
            let (target, addend) = successor_target(&layout, rows, out as usize, k, r, &exit);
            patch_successor(code, at, r.class, target as i64 + addend - place as i64).expect("a successor within branch reach");
        }
    }
    (layout.len, layout.hot)
}

/// The code length and each row's start offset that [`copy_patch`] gives `rows`, without copying or
/// patching anything. Sound because `copy_patch` places its rows by the same [`layout`], which reads
/// only each row's stencil, its `cold` flag and its `taken` target, never its hole values or exit
/// addresses; so a caller may size and lay out the buffer's tail before copying, and check the
/// starts `copy_patch` returns against these.
pub fn placement(rows: &[Row]) -> (usize, Vec<usize>) {
    let layout = layout(rows);
    (layout.len, layout.hot)
}

/// A value field `symbolic_fields` names: the row and hole it takes, the offset of the field in the
/// code `copy_patch` writes (from its `out`), and the relocation class.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct SymbolicField {
    pub row: usize,
    pub hole: u32,
    pub offset: usize,
    pub class: u8,
}

/// The value fields of `rows` whose hole `symbolic` names, at the offsets `copy_patch` places them: a
/// relocatable image writes no value there and records the field for its loader instead.
pub fn symbolic_fields(rows: &[Row], symbolic: impl Fn(usize, u32) -> bool) -> Vec<SymbolicField> {
    let layout = layout(rows);
    let mut fields = Vec::new();
    for (k, row) in rows.iter().enumerate() {
        let relocs = row.stencil.relocs.iter();
        for r in relocs.filter(|r| !class::is_successor(r.class) && layout.kept(rows, k, r.offset as usize) && symbolic(k, r.index)) {
            fields.push(SymbolicField { row: k, hole: r.index, offset: layout.address(rows, k, r.offset as usize), class: r.class });
        }
    }
    fields
}

/// Stencils compiled by rustc from `stencils/rows.rs` (see `build.rs`), as [`Stencil`]s.
// The library tests read the stencil tables, which `quench-stencils` defines.
#[cfg(test)]
extern crate quench_stencils;

pub mod rustc {
    use super::{class, trailing_jump, Imm32, Reloc, Stencil, NEXT};

    /// A stencil as `build.rs` read it from the object: relocations as (offset, class, index, addend).
    #[doc(hidden)]
    pub struct RawStencil {
        pub name: &'static str,
        pub code: &'static [u8],
        pub hot_len: usize,
        pub relocs: &'static [(u16, u8, u8, i32)],
    }

    /// One CPU variant's stencils, and what `build.rs` counted over them (`build/checks.rs`).
    #[doc(hidden)]
    pub struct RawLibrary {
        /// Rows that adjust or use the stack, where a pinned register would spill.
        pub stencils_with_stack_frame: usize,
        /// Rows whose hot path does not end with the continuation jump the stitcher drops.
        pub stencils_not_ending_in_continuation: usize,
        pub rows: &'static [RawStencil],
    }

    // The tables are the `quench-stencils` crate's (its build compiles every row), linked into the binary by
    // `quench-layers`: the runtime crates depend on this crate alone, so they compile while the stencils build.
    unsafe extern "Rust" {
        safe static QUENCH_STENCILS_BASELINE: &'static RawLibrary;
        safe static QUENCH_STENCILS_V3: &'static RawLibrary;
    }

    /// Whether this CPU runs the x86-64-v3 build: its AVX2 and BMI2 baseline with the rest of the
    /// level (AVX, BMI1, F16C, FMA, LZCNT, MOVBE). Other targets have only the baseline table.
    fn runs_v3() -> bool {
        #[cfg(target_arch = "x86_64")]
        {
            use std::arch::is_x86_feature_detected as has;
            has!("avx") && has!("avx2") && has!("bmi1") && has!("bmi2") && has!("f16c") && has!("fma") && has!("lzcnt") && has!("movbe")
        }
        #[cfg(not(target_arch = "x86_64"))]
        {
            false
        }
    }

    /// The library this process's stencils come from, chosen once at first use.
    fn library() -> &'static RawLibrary {
        static CHOSEN: std::sync::OnceLock<&'static RawLibrary> = std::sync::OnceLock::new();
        CHOSEN.get_or_init(|| if runs_v3() { QUENCH_STENCILS_V3 } else { QUENCH_STENCILS_BASELINE })
    }

    /// Rows of the chosen library that keep a stack frame.
    pub fn stencils_with_stack_frame() -> usize {
        library().stencils_with_stack_frame
    }

    /// Rows of the chosen library whose hot path does not end with the continuation jump.
    pub fn stencils_not_ending_in_continuation() -> usize {
        library().stencils_not_ending_in_continuation
    }

    /// The stencil of the row function `name`, if the object has it.
    pub fn stencil(name: &str) -> Option<Stencil> {
        let rows = library().rows;
        let raw = &rows[rows.binary_search_by(|raw| raw.name.cmp(name)).ok()?];
        let (mut relocs, mut imm32) = (Vec::new(), Vec::new());
        for &(offset, kind, index, addend) in raw.relocs {
            let (offset, index) = (u32::from(offset), u32::from(index));
            match kind {
                class::ABS32 => imm32.push(Imm32 { offset, index, negate: false, addend: addend as u32 }),
                other => relocs.push(Reloc { offset, class: other, index, addend: i64::from(addend) }),
            }
        }
        let len = raw.code.len();
        let (next_jump, tail_len) = match trailing_jump(raw.code, &relocs, raw.hot_len) {
            Some((NEXT, tail)) => (Some(raw.hot_len - tail), tail),
            _ => (None, 0),
        };
        Some(Stencil { code: raw.code.to_vec(), relocs, imm32, next_jump, tail_len, raw_len: len, hot_len: raw.hot_len })
    }

    /// Stencils memoized by key (`memoized`): each is built once and leaked, so it lives for the life of the process.
    pub type Memo<K> = std::sync::OnceLock<std::sync::Mutex<std::collections::HashMap<K, &'static Stencil>>>;

    /// The stencil memoized under `key` in `memo`, built by `build` on the key's first use; a build that
    /// fails is an error, and the next use builds again.
    pub fn memoized<K: std::hash::Hash + Eq + Copy>(memo: &'static Memo<K>, key: K, build: impl FnOnce() -> Result<Stencil, String>) -> Result<&'static Stencil, String> {
        let mut table = memo.get_or_init(Default::default).lock().map_err(|e| e.to_string())?;
        if let Some(&found) = table.get(&key) {
            return Ok(found);
        }
        let stencil: &'static Stencil = Box::leak(Box::new(build()?));
        table.insert(key, stencil);
        Ok(stencil)
    }
}

#[cfg(test)]
#[path = "../build/layout.rs"]
mod layout;

#[cfg(test)]
mod rustc_tests {
    use super::{numeric::library, rustc};

    /// The routines around a region's rows (`stencils/entry.rs`).
    const ROUTINES: [&str; 4] = ["entry_thunk", "entry_resume", "leave_word", "leave_pop"];
    /// The routines around a wasm function's rows (`wasm_entry.rs`, x86-64): its entry and the leaves with a word,
    /// with none, and with a trap's status.
    #[allow(dead_code)] // named for the reader; no test checks the Wasm routines yet
    const WASM_ROUTINES: [&str; 4] = ["entry_wasm", "leave_wasm_word", "leave_wasm_void", "leave_wasm_trap"];

    /// Every row the library names was built, and canonicalizing is idempotent.
    #[test]
    fn the_library_is_built() {
        // The base tier's rows are x86-64's only (`build.rs` `instances`).
        #[cfg(target_arch = "x86_64")]
        for code in library::library() {
            assert_eq!(library::canonical(code), code);
            assert!(rustc::stencil(&library::name(code)).is_some(), "{code:?}");
        }
        for name in ROUTINES {
            assert!(rustc::stencil(name).is_some(), "{name}");
        }
        #[cfg(target_arch = "x86_64")]
        for code in library::heap_library() {
            assert_eq!(library::heap_canonical(code), code);
            assert!(rustc::stencil(&library::heap_name(code)).is_some(), "{code:?}");
        }
        for code in crate::wasm::wasm_library() {
            assert!(rustc::stencil(&crate::wasm::wasm_name(code)).is_some(), "{code:?}");
        }
        #[cfg(target_arch = "x86_64")]
        for name in crate::wasm::ROUTINES {
            assert!(rustc::stencil(name).is_some(), "{name}");
        }
    }

    /// Every patched field of every stencil lies inside its code, and names a hole the copy
    /// patches: the range `copy_patch` writes through.
    #[test]
    fn patch_ranges_lie_inside_the_code() {
        let mut names: Vec<String> = ROUTINES.map(String::from).into();
        #[cfg(target_arch = "x86_64")]
        names.extend(library::library().into_iter().map(library::name).chain(library::heap_library().into_iter().map(library::heap_name)));
        for name in names {
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

    /// The cache rows (`stencils/ic_rows.rs`) patch the holes `base_rows` fills: a get its
    /// destination and receiver, the upper-word table, shape and slot offset (a holder read also the holder's shape), a
    /// store its receiver, value, atom, shape and slot offset; a chained row's hit takes the
    /// branch target and the last entry's miss takes the exit.
    #[cfg(target_arch = "x86_64")]
    #[test]
    fn cache_rows_patch_their_entry_holes_and_take_their_successors() {
        use super::numeric::library::{heap_canonical, heap_name, MEM};
        use super::numeric::row_names::{IC_GET_HOLDER_ROW, IC_GET_ROW, IC_SET_ROW};
        let row = |heap, chained| {
            let stencil = rustc::stencil(&heap_name(heap_canonical((heap, chained, MEM, MEM, MEM)))).expect("a cache row");
            let mut values: Vec<u32> = stencil.imm32.iter().map(|field| field.index).collect();
            values.sort_unstable();
            values.dedup();
            let successors: Vec<u32> = stencil.relocs.iter().filter(|reloc| crate::class::is_successor(reloc.class)).map(|reloc| reloc.index).collect();
            (values, successors)
        };
        // A get's receiver offset (hole 0's upper half) and upper-word table (hole 1) are 64-bit fields.
        assert_eq!(row(IC_GET_ROW, false).0, [0, 2, 3]);
        assert_eq!(row(IC_GET_HOLDER_ROW, false).0, [0, 2, 3, 4]);
        // A store's atom (hole 2) is a Stable fact the code holds, which the row does not read.
        assert_eq!(row(IC_SET_ROW, false).0, [0, 1, 3, 4]);
        for heap in [IC_GET_ROW, IC_GET_HOLDER_ROW, IC_SET_ROW] {
            let (last, chained) = (row(heap, false).1, row(heap, true).1);
            assert!(last.contains(&crate::EXIT) && !last.contains(&crate::TAKEN), "{heap}: {last:?}");
            assert!(chained.contains(&crate::TAKEN) && chained.contains(&crate::NEXT), "{heap}: {chained:?}");
        }
    }
}

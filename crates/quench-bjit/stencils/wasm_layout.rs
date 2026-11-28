//! The codes and offsets the stencil rows read, which the stencil crate and the lib share: the frame's
//! header words, `CallContext`'s and `TableTarget`'s offsets, the callout's absent operand, the value
//! types `T_*` name, the page size, and the memory loads and vector loads the rows dispatch on. Each is
//! asserted equal to the runtime's own layout in `quench-runtime/src/vm/compiled/wasm_codes_check.rs`.
//! The build script does not include this file (no row is built from these), so none of it compiles there.

use crate::wasm_kinds::*;

#[macro_use]
#[path = "../../quench-runtime/src/staged/wasm_lists.rs"]
mod wasm_lists;

macro_rules! page_bits_constant {
    ($bits:expr) => {
        /// The log2 of a Wasm page. The runtime asserts it equals its `WASM_PAGE_BITS`
        /// (`wasm_codes_check.rs`).
        pub const WASM_PAGE_BITS: u32 = $bits;
    };
}

/// The memory loads, from the list: each load's tag as `MemoryLoadTag`.
macro_rules! memory_load_tag_enum {
    ($($name:ident $family:ident),* $(,)?) => {
        /// A memory load's tag: its position in the list (the runtime's `MemoryLoad` has the same order).
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(u32)]
        pub enum MemoryLoadTag {
            $($name,)*
        }
    };
}
memory_load_list!(memory_load_tag_enum);

wasm_page_bits!(page_bits_constant);

pub const T_I32: u8 = AccessType::I32 as u8;
pub const T_I64: u8 = AccessType::I64 as u8;
pub const T_F32: u8 = AccessType::F32 as u8;

/// The Wasm frame's header words are defined once, in `quench-runtime/src/staged/frame.rs` (the one
/// frame model), mounted here so the stencil crate and the lib share it. The names below stay for
/// the rows that use them. `FRAME_HEADER_WORDS` is one past the last header word.
// Only the Wasm header words are read here; the rest of the frame model is the JS rows' and the runtime's.
#[allow(dead_code)]
#[path = "../../quench-runtime/src/staged/frame.rs"]
mod frame;
pub(crate) use frame::{WASM_BASE_WORD, WASM_HEADER_WORDS, WASM_LENGTH_WORD, WASM_RESULT_WORD};

pub const FRAME_BASE_WORD: usize = WASM_BASE_WORD;
pub const FRAME_LENGTH_WORD: usize = WASM_LENGTH_WORD;
pub const FRAME_HEADER_WORDS: u64 = WASM_HEADER_WORDS as u64;

/// `CallContext`'s byte offsets the `call_indirect` row reads (`wasm_rows_indirect.rs`). The
/// runtime's `offset_of!` values are asserted equal to these in `wasm_codes_check.rs`.
pub const CONTEXT_TABLE_WORDS_AT: usize = 0x48;
pub const CONTEXT_CALL_REFERENCE_AT: usize = 0x58;
pub const CONTEXT_MEMORY_LENGTH_AT: usize = 0x78;
pub const CONTEXT_INSTANCE_AT: usize = 0xC0;

/// The frame offset a callout's absent operand names: the row passes the zero word.
pub const CALLOUT_ABSENT: u32 = u32::MAX;

/// `TableTarget`'s size and the offsets of its fields (`SIGNATURE`, `ENTRY`, `INSTANCE`, `BODY`),
/// and `TableWords`' (`BASE`, `LENGTH`). Asserted against the runtime's types in
/// `wasm_codes_check.rs`.
/// `TableTarget`'s layout as a row reads it: `#[repr(C)]` with the runtime struct's fields
/// (`heap/cell/wasm_fields.rs`), the signature one word. The runtime asserts each offset and the size.
#[repr(C)]
pub(crate) struct TableTargetLayout {
    pub(crate) signature: u32,
    pub(crate) entry: u32,
    pub(crate) instance: u64,
    pub(crate) body: usize,
}
pub const TARGET_BYTES: usize = core::mem::size_of::<TableTargetLayout>();
pub const TARGET_SIGNATURE_AT: usize = core::mem::offset_of!(TableTargetLayout, signature);
pub const TARGET_INSTANCE_AT: usize = core::mem::offset_of!(TableTargetLayout, instance);
pub const TARGET_BODY_AT: usize = core::mem::offset_of!(TableTargetLayout, body);

pub const SIMD_V128_LOAD8X8_S: u32 = MemoryLoadTag::V128Load8x8S as u32;
pub const SIMD_V128_LOAD8X8_U: u32 = MemoryLoadTag::V128Load8x8U as u32;
pub const SIMD_V128_LOAD16X4_S: u32 = MemoryLoadTag::V128Load16x4S as u32;
pub const SIMD_V128_LOAD16X4_U: u32 = MemoryLoadTag::V128Load16x4U as u32;
pub const SIMD_V128_LOAD32X2_S: u32 = MemoryLoadTag::V128Load32x2S as u32;
pub const SIMD_V128_LOAD32X2_U: u32 = MemoryLoadTag::V128Load32x2U as u32;
pub const SIMD_V128_LOAD8_SPLAT: u32 = MemoryLoadTag::V128Load8Splat as u32;
pub const SIMD_V128_LOAD16_SPLAT: u32 = MemoryLoadTag::V128Load16Splat as u32;
pub const SIMD_V128_LOAD32_SPLAT: u32 = MemoryLoadTag::V128Load32Splat as u32;
pub const SIMD_V128_LOAD64_SPLAT: u32 = MemoryLoadTag::V128Load64Splat as u32;
pub const SIMD_V128_LOAD32_ZERO: u32 = MemoryLoadTag::V128Load32Zero as u32;

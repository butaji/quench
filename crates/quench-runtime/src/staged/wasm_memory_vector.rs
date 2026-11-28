//! The vector and lane accesses of linear memory as staged rules over `WasmMemory`: `load_vector`
//! and `store_vector` move the 16 bytes of a v128 access, `load_lane` the zero-extended lane a
//! splat or widening load reads. Each checks its access first, in the order the scalar `load` and
//! `store` keep. Both builds include this file: the interpreter (`wasm_memory_bytes`), the u-jit
//! and the b-jit's stencil rows.

use super::wasm_memory::{Access, WasmMemory, WasmMemoryWrite, check_access, load};
use crate::WasmType;

/// The v128 at `index + offset`: the check of `Access::VECTOR`, then the 16 bytes.
#[inline(always)]
pub fn load_vector<S: WasmMemory>(
    stage: &mut S,
    index: S::Word,
    offset: u32,
    proven: bool,
) -> Result<S::Vector, S::Exit> {
    check_access(stage, Access::VECTOR, index, offset, proven)?;
    Ok(stage.memory_load_vector(index, offset))
}

/// Writes the v128 `value` at `index + offset`: the check of `Access::VECTOR`, then the 16 bytes.
#[inline(always)]
pub fn store_vector<S: WasmMemoryWrite>(
    stage: &mut S,
    index: S::Word,
    offset: u32,
    value: S::Vector,
    proven: bool,
) -> Result<(), S::Exit> {
    check_access(stage, Access::VECTOR, index, offset, proven)?;
    stage.memory_store_vector(index, offset, value);
    Ok(())
}

/// The access of a lane of `bytes` bytes (1, 2, 4 or 8) that a splat or widening load reads: its
/// word zero-extended to 64 bits.
#[inline(always)]
pub(crate) const fn lane(bytes: usize) -> Access {
    Access { ty: WasmType::I64, bytes, signed: false }
}

/// The zero-extended lane of `bytes` bytes at `index + offset`: the staged `load` of `lane`.
#[inline(always)]
pub(crate) fn load_lane<S: WasmMemory>(
    stage: &mut S,
    bytes: usize,
    index: S::Word,
    offset: u32,
    proven: bool,
) -> Result<S::Word, S::Exit> {
    load(stage, lane(bytes), index, offset, proven)
}

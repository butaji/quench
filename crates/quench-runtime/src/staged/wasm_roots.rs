//! The native roots of a compiled Wasm frame's reference registers, written once over `WasmRoots`:
//! a push holds the registers in the roots after the live ones and records the depth it found, a
//! restore or pop puts the depth back. A push past the capacity traps `CallStackExhausted`, as the
//! native stack's budget would, and writes nothing. The interpreter's frame and the u-jit
//! (`vm/compiled/wasm_roots.rs`) run these rules over their own roots; the stencil's rows call them
//! over the context's `NativeRoots` words.

use super::wasm::{WasmStage, Width};
use crate::WasmTrap;
use crate::wasm::integer::I64BinaryOperator as Binary;

/// The native roots of a frame: a buffer of value words of which `depth` are live.
pub trait WasmRoots: WasmStage {
    /// The handle of the roots buffer: loaded once per rule by `native_roots`, then passed to each access.
    type Roots: Copy;
    /// The roots buffer of the frame's context.
    fn native_roots(&mut self) -> Self::Roots;
    /// The number of live roots.
    fn root_depth(&mut self, roots: Self::Roots) -> Self::Word;
    /// The number of roots the buffer holds.
    fn root_capacity(&mut self, roots: Self::Roots) -> Self::Word;
    /// Stores `value` as root `depth + index`, which the rule has checked is within the capacity.
    fn store_root(&mut self, roots: Self::Roots, depth: Self::Word, index: usize, value: Self::Word);
    /// Makes `depth` the number of live roots.
    fn set_root_depth(&mut self, roots: Self::Roots, depth: Self::Word);
}

/// Holds `values` in the roots after the live ones; returns the depth they found, for `restore`.
/// Where the roots have no room for them, the push traps and writes nothing.
#[inline(always)]
pub fn push<S: WasmRoots>(stage: &mut S, values: &[S::Word]) -> Result<S::Word, S::Exit> {
    let roots = stage.native_roots();
    let depth = stage.root_depth(roots);
    let count = stage.constant(Width::I64, values.len() as i64);
    let needed = stage.operate(Binary::Add, Width::I64, depth, count);
    let capacity = stage.root_capacity(roots);
    let exhausted = stage.operate(Binary::LessUnsigned, Width::I64, capacity, needed);
    stage.trap_when(exhausted, WasmTrap::CallStackExhausted)?;
    for (index, value) in values.iter().enumerate() {
        stage.store_root(roots, depth, index, *value);
    }
    stage.set_root_depth(roots, needed);
    Ok(depth)
}

/// Makes `depth` the live count again: the restore of a `push` that returned it.
#[inline(always)]
#[cfg(not(stencil_build))]
pub fn restore<S: WasmRoots>(stage: &mut S, depth: S::Word) {
    let roots = stage.native_roots();
    stage.set_root_depth(roots, depth);
}

/// Releases the `count` roots a stencil's push held: the depth goes back down by it.
#[inline(always)]
#[cfg(stencil_build)]
pub fn pop<S: WasmRoots>(stage: &mut S, count: usize) {
    let roots = stage.native_roots();
    let depth = stage.root_depth(roots);
    let count = stage.constant(Width::I64, count as i64);
    let released = stage.operate(Binary::Subtract, Width::I64, depth, count);
    stage.set_root_depth(roots, released);
}

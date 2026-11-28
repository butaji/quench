//! The register frame every staged operation runs on, and an operation's outcome: its own file
//! so the b-jit stencil build (`quench-bjit/build.rs`) compiles the same trait the interpreter
//! and the specializers use.

use super::value::TypeofKind;
use super::{Allocation, CallOut, Edge, Stage};
use crate::bytecode::Register;
use crate::typed_ir::Type;

#[path = "register_frame/frame_trait.rs"]
mod frame_trait;
#[path = "register_frame/rows.rs"]
mod rows;
pub use frame_trait::*;
pub use rows::*;

/// A constant an instruction materializes: its value and the semantic type a stage that
/// keeps type facts records for it.
#[derive(Clone, Copy)]
pub struct Constant {
    pub value: crate::Value,
    pub ty: Type,
}

impl Constant {
    /// A constant whose type the stage does not keep.
    #[inline(always)]
    pub const fn untyped(value: crate::Value) -> Self {
        Self { value, ty: Type::ANY }
    }
}

/// The representation a Wasm scalar takes in a register frame (`Frame::wasm_read`, `Frame::wasm_write`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum WasmRep {
    /// An i32 or f32: its 32 bits, zero-extended; a register holds them as an encoded integer.
    Int32,
    /// An i64 or a boxed f64: its 64 bits in place, where the value is untagged.
    Bits64,
    /// A raw f64: its 64 bits as the register's machine word, never boxed or canonicalized.
    RawBits64,
    /// The register's tagged word as it is (a select's copy).
    Tagged,
}

/// A staged instruction's outcome: the word it produced, or where control goes.
pub enum Step<W, C> {
    /// The operation produced `W`; control falls through.
    Value(W),
    /// The operation transferred control to `C`.
    Control(C),
    /// The operation left at its miss edge (a failed guard), or a value's heap cell decides
    /// its truthiness: the interpreter must take its fallback path.
    Unresolved,
}

/// The bytes of a Wasm `v128` (`wasm::V128_BYTES`; the stencil build has no `wasm` module).
pub const VECTOR_BYTES: usize = 16;

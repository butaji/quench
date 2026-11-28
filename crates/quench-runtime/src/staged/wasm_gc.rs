//! Wasm GC operations as one semantics: the checks of a struct or array access, of an array's bulk
//! operations, and of a reference's null check and cast, with their order and traps, written once
//! over `WasmGc`. The interpreter runs them on its heap (`HeapStage`, `vm/wasm_gc/stage.rs`); a
//! specializer runs them over its own cells. Allocation and the descriptor operations stay with the
//! stage that owns the heap.

#[allow(unused_imports)] // the children's `use super::*` take these names
use super::machine::MachineStage;
#[allow(unused_imports)]
use super::wasm::Width;
#[allow(unused_imports)]
use crate::WasmTrap;
#[allow(unused_imports)]
use crate::WasmTypes;
#[allow(unused_imports)]
use crate::bytecode::Op;
#[allow(unused_imports)]
use crate::wasm::gc::{ArraySegmentKind, GcField, GcFieldAccess};
#[allow(unused_imports)]
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
#[allow(unused_imports)]
use crate::wasm::reference::ReferenceTarget;

mod primitives;
mod rules;
mod step;
mod table;

pub use primitives::{Aggregate, WasmGc};
pub use rules::{array_copy, array_fill, array_length, field_read, field_write, non_null, reference_cast};
#[cfg(not(stencil_build))]
pub use step::gc_value_op;
pub use step::{ArrayConstruction, GcOp, GcStep};

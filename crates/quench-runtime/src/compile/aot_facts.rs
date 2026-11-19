//! Closed-world facts for AOT: what holds of the program alone when no `eval`, `Function` or
//! dynamic import can add code (`SourceDemands`). Each fact is a dependency in the one
//! dependency system (`code_dependencies`), never a per-use check, and is read by every tier
//! that lowers the program (lane bodies, BaseJIT bodies), so no layer re-derives it.
//!
//! - Call targets: a call whose callee is a binding written once with a function, or a method
//!   on a predicted shape, names that function (direct call, inlinable).
//! - Constructor shapes: the fields a constructor's `this.x = ...` stores add in order, with
//!   its `prototype`, as a `ShapeRecipe` (reloc class D), so `new F()` allocates the final shape
//!   and field reads of it are constant offsets.
//! - Number representations: registers proven int32 or double for the whole function by a
//!   forward dataflow over the bytecode (`dataflow.rs`), so lane bodies keep them unboxed.
//!
//! Owner: AOT facts.

use crate::bytecode::ResidualProgram;

/// The facts of one program.
#[derive(Default)]
pub(crate) struct AotFacts {
    /// `(function, pc)` of a call → the function it always calls.
    pub(crate) call_targets: std::collections::BTreeMap<(u32, u32), u32>,
    /// Function id of a constructor → its instances' final fields in order (atom numbers).
    pub(crate) constructor_shapes: std::collections::BTreeMap<u32, Box<[u32]>>,
    /// Function id → register → proven representation.
    pub(crate) numbers: std::collections::BTreeMap<u32, Box<[Option<crate::typed_ir::Representation>]>>,
}

impl AotFacts {
    pub(crate) fn is_empty(&self) -> bool {
        self.call_targets.is_empty() && self.constructor_shapes.is_empty() && self.numbers.is_empty()
    }
}

/// The facts of `program`, or none where the program can add code at run time.
pub(crate) fn analyze(program: &ResidualProgram) -> AotFacts {
    let _ = program;
    todo!("AOT facts")
}

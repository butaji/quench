//! Whether a function's activation reads its `this`: the one fact a direct call needs to write the receiver into the
//! callee's window header (`CONTEXT_THIS`, `quench-bjit/stencils/call_rows.rs`). Conservative: any op that may read it counts.

use super::*;

/// Whether `function` of `code` may read the `this` of its activation: it loads, initializes or checks `this`, reads a
/// field of base `this`, makes an arrow that reads an implicit binding, or may read its scope by name (`eval`, `with`).
pub(crate) fn activation_reads_this(code: &ResidualProgram, function: &Function) -> bool {
    function.has_dynamic_scope_access() || function.instructions().any(|instruction| {
        let op = instruction.op();
        op.immediate_role() == ImmediateRole::SuperinstructionIndex
            || match op {
                // A store to a `this` field and a call of a `this` method name their receiver
                // implicitly, through no register and no field lookup.
                Op::LoadThis | Op::InitializeThis | Op::SuperCallCheck
                    | Op::SetThisField | Op::SetThisFieldStrict | Op::CallThisMethod => true,
                Op::MakeClosure => arrow_chain_reads_implicit_bindings(code, instruction.closure_function_index()),
                _ => reads_this_field(code, instruction),
            }
    })
}

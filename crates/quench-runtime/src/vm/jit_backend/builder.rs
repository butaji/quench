//! The one way a lowering starts: a function of its signature, entered at its first block with
//! the signature's parameters, built under this thread's reused SSA builder context. Cranelift
//! means a `FunctionBuilderContext` to be reused: a finished lowering's `finalize` empties it and
//! keeps its tables' capacity for the next, so a fresh context per lowering pays its allocations
//! again for nothing.

use cranelift_codegen::ir;
use cranelift_frontend::{FunctionBuilder, FunctionBuilderContext};

thread_local! {
    /// The SSA builder context this thread's last finished lowering left empty.
    static HELD: std::cell::Cell<Option<FunctionBuilderContext>> = const { std::cell::Cell::new(None) };
}

/// This thread's builder context, empty: the one the last finished lowering gave back, or a new one.
pub fn take_context() -> FunctionBuilderContext {
    HELD.with(|held| held.take()).unwrap_or_default()
}

/// Gives `context` back for the next lowering of this thread, once its builder's `finalize` emptied it;
/// a lowering that gave up drops its context instead.
pub fn keep_context(context: FunctionBuilderContext) {
    HELD.with(|held| held.set(Some(context)));
}

/// Enters `builder`'s function at a new first block holding its signature's parameters, which it
/// returns.
pub fn enter(builder: &mut FunctionBuilder<'_>) -> (ir::Block, Vec<ir::Value>) {
    let entry = builder.create_block();
    builder.append_block_params_for_function_params(entry);
    builder.switch_to_block(entry);
    (entry, builder.block_params(entry).to_vec())
}

/// Builds `function` with `body`, which receives the builder entered at its first block and the
/// parameters; the builder is sealed and finalized after it, and the context kept for the next.
pub fn build<R>(function: &mut ir::Function, isa: &dyn cranelift_codegen::isa::TargetIsa, body: impl FnOnce(&mut FunctionBuilder<'_>, &[ir::Value]) -> R) -> R {
    let mut context = take_context();
    let mut builder = FunctionBuilder::new(function, &mut context);
    let (_, parameters) = enter(&mut builder);
    let result = body(&mut builder, &parameters);
    builder.seal_all_blocks();
    builder.finalize(isa.frontend_config());
    keep_context(context);
    result
}

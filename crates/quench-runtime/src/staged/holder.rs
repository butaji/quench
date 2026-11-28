//! A method read through a prototype hint (`Vm::holder_hint_hit`) as a staged row: a receiver
//! of one exact shape and prototype link, under a watch on the prototype's values, reads the
//! value the hint recorded. The guard word and value are per-site operands; the watch is an
//! assumption, tested by the interpreter and proved once by a specializer.

use super::Stage;

/// The row is defined with the inline caches' ops (`IcOp::Held`), whose home is `ic`.
pub use super::ic::holder_read;

/// The value a method read of a string receiver finds on `String.prototype`: every direct string
/// has the one prototype, so the family test replaces the shape and link guards, and the watch on
/// the prototype's values is the assumption. `None` (leaving at `miss`) for any other receiver or
/// once the watch is retired.
// Read by the u-jit and b-jit projections of GetField on string receivers (threads 13 and 7).
#[allow(dead_code)]
#[inline(always)]
pub fn string_holder_read<S: Stage>(stage: &mut S, receiver: S::Word, watch: u64, value: u64, miss: S::Miss)
    -> Option<S::Word> {
    let string = super::value::is_direct_string(stage, receiver);
    stage.guard(string, miss)?;
    let watch = stage.site(watch);
    let holds = stage.assumption(watch);
    stage.guard(holds, miss)?;
    let zero = stage.constant(0);
    let value = stage.site(value);
    Some(stage.add_imm(zero, value))
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;

#[cfg(test)]
mod tests;

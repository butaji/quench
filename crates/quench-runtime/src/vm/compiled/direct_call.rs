//! Direct calls of numeric compiled bodies from the lane.
use super::{CompiledJsFunction, NativeEntryMode, native_poll_page};
use crate::Value;

const STACK_PARAMS: usize = 8;

pub(super) fn direct_call(
    compiled: &CompiledJsFunction,
    arguments: &[Value],
    interrupt: &crate::ExecutionInterrupt,
) -> Option<Value> {
    let params = compiled.params().min(arguments.len());
    // The common case, a few parameters, converts into a fixed array on the stack.
    if params <= STACK_PARAMS {
        let mut numeric = [0.0_f64; STACK_PARAMS];
        numeric_arguments(arguments, &mut numeric[..params])?;
        return direct_call_numbers(compiled, &numeric[..params], interrupt);
    }
    let mut numeric = vec![0.0_f64; params];
    numeric_arguments(arguments, &mut numeric)?;
    direct_call_numbers(compiled, &numeric, interrupt)
}

#[inline(always)]
fn direct_call_numbers(
    compiled: &CompiledJsFunction,
    numeric_arguments: &[f64],
    interrupt: &crate::ExecutionInterrupt,
) -> Option<Value> {
    // Only a body that polls reads the poll page.
    let poll_page = (compiled.native_entry_mode() == NativeEntryMode::Resumable)
        .then(|| native_poll_page(interrupt))
        .flatten();
    compiled.call(numeric_arguments, interrupt, poll_page)
}

/// Fills `numbers` with the Numbers `arguments` start with; `None` where one is no Number.
#[inline(always)]
fn numeric_arguments(arguments: &[Value], numbers: &mut [f64]) -> Option<()> {
    for (number, argument) in numbers.iter_mut().zip(arguments) {
        *number = match argument.as_int() {
            Some(integer) => f64::from(integer),
            None => argument.as_number()?,
        };
    }
    Some(())
}

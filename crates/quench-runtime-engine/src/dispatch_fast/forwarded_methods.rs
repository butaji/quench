//! The method a forwarding constructor body calls on `this`, read from its function record
//! (`Function::forwarded_this`) for a target that forwards (`ScriptCallTarget::forwards_method`).

use quench_runtime::bytecode::ForwardedThisMethod;
use quench_runtime::vm::engine_types::ScriptCallTarget;
use quench_runtime::vm::Vm;

/// The method `target`'s body forwards its construction to, or `None` where the target does not
/// forward, or its function's body is not the forwarding shape. Both are fixed by the program's
/// records, so nothing is recorded per lane.
pub(crate) fn forwarded_method_of(vm: &Vm, target: &ScriptCallTarget) -> Option<ForwardedThisMethod> {
    if !target.body.forwards_method {
        return None;
    }
    let code = vm.programs.residual(target.body.admission.program())?;
    code.function(target.body.function as usize).forwarded_this(&code.constants)
}

//! What quench-runtime-engine's own tests reach of the core (feature `test-support`): a VM to
//! run a program on, outside the embedding API the engine's production code goes through.

use crate::vm::Vm;
use crate::{Host, JsError, ResidualProgram, Value};

/// A VM for `host`.
pub fn vm<T: Host>(host: T) -> Vm {
    Vm::new(host)
}

/// Runs `program` on `vm`, then its jobs.
pub fn execute(vm: &mut Vm, program: &ResidualProgram) -> Result<Value, JsError> {
    vm.execute(program)
}

/// Whether `invalidation` has fired.
pub fn invalidation_is_set(invalidation: &crate::code_dependencies::Invalidation) -> bool {
    invalidation.is_set()
}

/// `Priors::repaying_length`: the fewest straight-line bytecodes whose interpretation repays an entry.
pub fn repaying_length(priors: &crate::vm::tier_policy::Priors, share: u64) -> usize {
    priors.repaying_length(share)
}

/// `ResidualProgram::with_record`: `read` on the function record at `index`.
pub fn with_record<R>(program: &ResidualProgram, index: usize, read: impl FnOnce(&crate::bytecode::Function) -> R) -> R {
    program.with_record(index, read)
}

/// Runs the host-supplied global `scripts`, then `program`, in one realm, then its jobs.
pub fn execute_after_scripts(vm: &mut Vm, scripts: &[(&str, &str)], program: &ResidualProgram) -> Result<Value, JsError> {
    vm.execute_after_scripts(scripts, program)
}

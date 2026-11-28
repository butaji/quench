//! What quench-runtime-engine's own tests reach of the core (feature `test-support`): a VM to
//! run a program on, outside the embedding API the engine's production code goes through.

use crate::vm::Vm;
use crate::{Host, JsError, ResidualProgram, Value};

/// A VM for `host`.
/// The stack an image compile needs: the lane lowering's Cranelift pipeline uses more than a test
/// thread's default 2 MiB (an offline compile runs on a compiler's main thread).
pub const COMPILE_STACK_BYTES: usize = 8 * 1024 * 1024;

/// Runs `body` on a thread with `COMPILE_STACK_BYTES`, so a test compiles an image as the compiler
/// does without asking the harness for `RUST_MIN_STACK`.
pub fn on_compile_stack<T: Send>(body: impl FnOnce() -> T + Send) -> T {
    std::thread::scope(|scope| {
        std::thread::Builder::new().stack_size(COMPILE_STACK_BYTES)
            .spawn_scoped(scope, body).expect("a compile thread").join().expect("the test body")
    })
}

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

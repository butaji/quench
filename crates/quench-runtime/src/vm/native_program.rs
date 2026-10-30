//! Ownership of linked JavaScript code and its compiler-resolved program.

use super::{ProgramId, Vm};
use crate::{Host, JsError, ResidualProgram, Value};
use std::{any::Any, marker::PhantomData, rc::Rc};

/// The shared native function ABI, with an opaque VM-owned call context.
pub type NativeJsFunctionEntry =
    unsafe extern "C" fn(*mut std::ffi::c_void, u64, u64, *const u64, *mut u64) -> u32;

pub(super) struct NativeJsCode {
    pub(super) entries: Box<[NativeJsFunctionEntry]>,
    // Code and operand/asset storage must survive every closure and queued job.
    _owner: Rc<dyn Any>,
}

/// A program bound to native entries compiled for the same host and semantic ABI.
pub struct NativeJsProgram<H: Host> {
    program: Rc<ResidualProgram>,
    code: Rc<NativeJsCode>,
    host: PhantomData<fn(&mut H)>,
}

impl<H: Host> NativeJsProgram<H> {
    /// The canonical metadata used by host job checkpoints and source reporting.
    pub fn residual(&self) -> &ResidualProgram {
        &self.program
    }
    /// Bind native code to its source program. Entry indices are function IDs.
    ///
    /// # Safety
    /// Every entry must implement this compiler build's native graph ABI for
    /// the corresponding function in `program` and host `H`. `owner` must keep
    /// all executable code and native operands/assets alive, without untraced
    /// VM heap pointers. Linked static code can use an owner of `Rc::new(())`.
    pub unsafe fn from_entries(
        program: ResidualProgram,
        entries: Vec<NativeJsFunctionEntry>,
        owner: Rc<dyn Any>,
    ) -> Result<Self, String> {
        if program.kind == crate::bytecode::ProgramKind::Wasm {
            return Err("native JavaScript entries require a JS or TS program".into());
        }
        program.validate_unless_proven()?;
        if entries.len() != program.function_count() {
            return Err("native entry count does not match program functions".into());
        }
        Ok(Self {
            program: Rc::new(program),
            code: Rc::new(NativeJsCode {
                entries: entries.into_boxed_slice(),
                _owner: owner,
            }),
            host: PhantomData,
        })
    }
}

impl<H: Host> Vm<H> {
    pub(crate) fn execute_native_program(
        &mut self,
        native: &NativeJsProgram<H>,
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        self.initialize_shared(&native.program, super::builtins::RealmScope::Complete)?;
        self.programs
            .attach_native_js(ProgramId::MAIN, native.code.clone())
            .map_err(JsError::validation)?;
        self.run_main(&native.program, drain_jobs)
    }
}

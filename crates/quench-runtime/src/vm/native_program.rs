//! Ownership of linked JavaScript code and its compiler-resolved program.

use super::{ProgramId, Vm};
use crate::{Host, JsError, ResidualProgram, Value};
use std::{any::Any, marker::PhantomData, rc::Rc};

/// The shared native function ABI, with an opaque VM-owned call context.
pub type NativeJsFunctionEntry =
    unsafe extern "C" fn(*mut std::ffi::c_void, u64, u64, *const u64, *mut u64) -> u32;

pub(super) enum NativeJsCode {
    Linked(&'static [NativeJsFunctionEntry]),
    Owned {
        entries: Box<[NativeJsFunctionEntry]>,
        // Code and operand/asset storage survive every closure and queued job.
        _owner: Rc<dyn Any>,
    },
}

impl NativeJsCode {
    pub(super) fn entries(&self) -> &[NativeJsFunctionEntry] {
        match self {
            Self::Linked(entries) => entries,
            Self::Owned { entries, .. } => entries,
        }
    }
}

/// A program bound to native entries compiled for the same host and semantic ABI.
pub struct NativeJsProgram<H: Host> {
    program: Rc<ResidualProgram>,
    code: Rc<NativeJsCode>,
    host: PhantomData<fn(&mut H)>,
}

impl<H: Host> NativeJsProgram<H> {
    /// Adopt a statically linked image table emitted by `lower_native_js_module`.
    /// Its header retains the compiler's ABI marker at link time.
    ///
    /// # Safety
    /// `image` must be the aligned entry-table symbol for this exact program,
    /// host H and ABI. Its code and operand data must live for the process's
    /// lifetime. The table has the ABI header and one entry per function.
    pub unsafe fn from_linked_image(
        program: ResidualProgram,
        image: *const u8,
    ) -> Result<Self, String> {
        if image.is_null() || image.addr() % std::mem::align_of::<NativeJsFunctionEntry>() != 0 {
            return Err("native image table is missing or misaligned".into());
        }
        let header_bytes =
            crate::NATIVE_JS_IMAGE_HEADER_WORDS * std::mem::size_of::<NativeJsFunctionEntry>();
        // SAFETY: the image contract supplies the full table. Skip the marker
        // before interpreting words as functions of the execution-entry ABI.
        let entries = unsafe {
            std::slice::from_raw_parts(
                image.add(header_bytes).cast::<NativeJsFunctionEntry>(),
                program.function_count(),
            )
        };
        // SAFETY: the linked image is static and bound to this program and H.
        Self::bind(program, NativeJsCode::Linked(entries))
    }

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
        Self::bind(
            program,
            NativeJsCode::Owned {
                entries: entries.into_boxed_slice(),
                _owner: owner,
            },
        )
    }

    fn bind(program: ResidualProgram, code: NativeJsCode) -> Result<Self, String> {
        if program.kind == crate::bytecode::ProgramKind::Wasm {
            return Err("native JavaScript entries require a JS or TS program".into());
        }
        program.validate_unless_proven()?;
        if code.entries().len() != program.function_count() {
            return Err("native entry count does not match program functions".into());
        }
        Ok(Self {
            program: Rc::new(program),
            code: Rc::new(code),
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

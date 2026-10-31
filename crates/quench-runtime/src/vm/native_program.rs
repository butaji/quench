//! Ownership of linked JavaScript code and its compiler-resolved program.

use super::{ProgramId, Vm};
use crate::{Host, JsError, ResidualProgram, Value};
use std::{any::Any, marker::PhantomData, rc::Rc};

/// The shared native function ABI, with an opaque VM-owned call context.
pub type NativeJsFunctionEntry =
    unsafe extern "C" fn(*mut std::ffi::c_void, u64, u64, *const u64, *mut u64) -> u32;

pub(crate) enum NativeJsCode {
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

impl<H: Host> Clone for NativeJsProgram<H> {
    fn clone(&self) -> Self {
        Self { program: self.program.clone(), code: self.code.clone(), host: PhantomData }
    }
}

impl<H: Host> NativeJsProgram<H> {
    /// Compile every function before execution using the object backend's graph
    /// and the JIT's executable-memory publisher. No hotness or fallback tier.
    #[cfg(feature = "aot")]
    pub fn compile_eager(program: ResidualProgram) -> Result<Self, String> {
        Self::compile_eager_with_options(program, crate::NativeJsCodegenOptions::default())
    }

    #[cfg(feature = "aot")]
    pub fn compile_eager_with_options(
        program: ResidualProgram, options: crate::NativeJsCodegenOptions,
    ) -> Result<Self, String> {
        super::compiled::compile_eager_native(program, options)
    }

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
        if !crate::DYNAMIC_EXECUTION_ENABLED && program.source.as_ref().is_some_and(|source| source.call_sites.get().is_none()) {
            return Err("native program has no frozen call-site descriptions".into());
        }
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
    /// Admit a linked module without remapping the atom IDs embedded in its code.
    /// Module records, environments and evaluation remain owned by the shared loader.
    pub(super) fn kept_native_module_program(
        &mut self, module: &crate::ModuleSource,
    ) -> Result<Option<ProgramId>, JsError> {
        let parts = [self.specialization_part(), module.compilation_goal(), &module.source];
        let Some(native) = self.host.kept_native_program(&module.name, &parts).map_err(|error| JsError::validation(error.to_string()))? else {
            return Ok(None);
        };
        self.store_native_module_program(native).map(Some)
    }

    pub(super) fn store_native_module_program(&mut self, native: NativeJsProgram<H>) -> Result<ProgramId, JsError> {
        if native.program.kind != crate::bytecode::ProgramKind::Module {
            return Err(JsError::validation("native module has the wrong parse goal".into()));
        }
        let prepared = self.prepare_native_script(native)?;
        let id = self.store_module_program_shared(prepared.program)
            .ok_or_else(|| JsError::validation("native module program store is full".into()))?;
        self.programs.attach_native_js(id, prepared.native.expect("native module code admitted"))
            .map_err(JsError::validation)?;
        Ok(id)
    }

    /// Native operands already contain atom IDs. Only an identical prefix of
    /// the initialized VM namespace can be adopted without rewriting the code.
    pub(super) fn prepare_native_script(
        &self, native: NativeJsProgram<H>,
    ) -> Result<crate::PreparedScript, JsError> {
        if native.program.specialized != self.specialized {
            return Err(JsError::validation("native script specialization does not match runtime".into()));
        }
        if native.program.atoms.len() > self.atom_sequence.len()
            || native.program.atoms.iter().enumerate().any(|(id, name)| {
                !self.atom_units_equal_str(id as u32, name)
            })
        {
            return Err(JsError::validation("native script atom IDs do not match runtime namespace".into()));
        }
        let mut program = Rc::unwrap_or_clone(native.program);
        program.atoms = crate::bytecode::AtomTable::shared(
            self.atom_sequence.clone(), program.atoms.len(),
        );
        Ok(crate::PreparedScript { program: Rc::new(program), native: Some(native.code) })
    }

    pub(crate) fn execute_native_program(
        &mut self,
        native: &NativeJsProgram<H>,
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        self.execute_native_with_scripts(&[], &[], native, drain_jobs)
    }

    pub(crate) fn execute_native_after_scripts(
        &mut self,
        scripts: &[NativeJsProgram<H>],
        native: &NativeJsProgram<H>,
    ) -> Result<Value, JsError> {
        self.execute_native_with_scripts(scripts, &[], native, true)
    }

    pub(crate) fn execute_native_module_graph(
        &mut self, scripts: &[NativeJsProgram<H>], modules: &[NativeJsProgram<H>], native: &NativeJsProgram<H>,
    ) -> Result<Value, JsError> {
        self.execute_native_with_scripts(scripts, modules, native, true)
    }

    fn execute_native_with_scripts(
        &mut self,
        scripts: &[NativeJsProgram<H>],
        modules: &[NativeJsProgram<H>],
        native: &NativeJsProgram<H>,
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        self.initialize_shared(&native.program, super::builtins::RealmScope::Complete)?;
        self.programs.attach_native_js(ProgramId::MAIN, native.code.clone()).map_err(JsError::validation)?;
        for module in modules {
            if native.program.kind == crate::bytecode::ProgramKind::Module
                && crate::module_identity::same_name(&module.program.source_name, &native.program.source_name) {
                return Err(JsError::validation("native dependency duplicates the entry module".into()));
            }
            self.register_native_module(module.clone())
                .inspect_err(|_| self.record_native_execution_rejection())?;
        }
        for script in scripts {
            let prepared = self.prepare_native_script(script.clone())?;
            self.evaluate_prepared_global_script(&prepared)?;
        }
        self.run_main(&native.program, drain_jobs)
    }
}

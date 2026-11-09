//! Ownership of linked JavaScript code and its compiler-resolved program.

use super::{ProgramId, Vm};
use crate::{Host, JsError, ResidualProgram, Value};
use std::{any::Any, marker::PhantomData, rc::Rc};

/// The shared native function ABI, with an opaque VM-owned call context.
pub type NativeJsFunctionEntry =
    unsafe extern "C" fn(*mut std::ffi::c_void, u64, u64, *const u64, *mut u64) -> u32;

pub type NativeProgramResolver<H> = dyn Fn(&str, &[&str]) -> Result<Option<NativeJsProgram<H>>, crate::NativeSourceError>;

pub(crate) enum NativeJsCode {
    Linked {
        entries: &'static [NativeJsFunctionEntry],
    },
    Owned {
        entries: Box<[NativeJsFunctionEntry]>,
        // Code and operand/asset storage survive every closure and queued job.
        _owner: Rc<dyn Any>,
    },
}

impl NativeJsCode {
    #[inline]
    pub(super) fn entries(&self) -> &[NativeJsFunctionEntry] {
        match self {
            Self::Linked { entries, .. } => entries,
            Self::Owned { entries, .. } => entries,
        }
    }

}

/// What a publication lowered, so admission is measured rather than assumed.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NativeJsPublicationCounts {
    /// Functions in the program.
    pub functions: usize,
    /// Functions published through the general-dispatch baseline.
    pub general_dispatch: usize,
    /// Functions left on the call-threaded body (per-PC semantic entries).
    pub call_threaded: usize,
    /// Typed regions inlined into their caller.
    pub typed_regions: usize,
    /// Scalar regions published beside their caller.
    pub scalar_regions: usize,
    /// Instruction sites whose continuation is a folded static transfer.
    pub static_transfers: usize,
    /// Instruction sites that call their opcode's semantic entry.
    pub semantic_entry_sites: usize,
    /// Functions whose entry is the specialized body (general dispatch + typed regions).
    pub tier_specialized: usize,
    /// Functions whose entry is general dispatch without a typed region.
    pub tier_general: usize,
    /// Functions whose entry is the call-threaded body (the AOT tier zero).
    pub tier_call_threaded: usize,
    /// Whole-program globals the image folds, each checked at its read site.
    pub frozen_globals: usize,
    /// Whole-program prototype values the image folds, each guarded at its read site.
    pub frozen_fields: usize,
}

/// The whole-program assumptions an eager image folded, replayed when it loads.
pub(crate) struct FrozenAssumptions {
    pub(crate) invalidation: crate::code_dependencies::Invalidation,
    /// Method atoms the code holds unnamed (`StableFacts::atoms`).
    pub(crate) atoms: Box<[crate::bytecode::Atom]>,
    /// Whole-program globals the code folds (its read-site guards verify them).
    pub(crate) globals: Box<[crate::bytecode::Atom]>,
    /// Whole-program prototype values the code folds.
    pub(crate) fields: Box<[crate::bytecode::Atom]>,
    /// Set when any assumption no longer holds; the code leaves at its invalidation point.
    pub(crate) watch: Rc<crate::code_dependencies::PublishedCode<()>>,
}

impl Clone for FrozenAssumptions {
    fn clone(&self) -> Self {
        Self { invalidation: self.invalidation.clone(), atoms: self.atoms.clone(), globals: self.globals.clone(), fields: self.fields.clone(), watch: self.watch.clone() }
    }
}

/// A program bound to native entries compiled for the same host and semantic ABI.
pub struct NativeJsProgram<H> {
    program: Rc<ResidualProgram>,
    code: Rc<NativeJsCode>,
    counts: NativeJsPublicationCounts,
    frozen: Option<FrozenAssumptions>,
    host: PhantomData<fn(&mut H)>,
}

impl<H: Host> Clone for NativeJsProgram<H> {
    fn clone(&self) -> Self {
        Self { program: self.program.clone(), code: self.code.clone(), counts: self.counts, frozen: self.frozen.clone(), host: PhantomData }
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
        if image.is_null() || !image.addr().is_multiple_of(std::mem::align_of::<NativeJsFunctionEntry>()) {
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
        Self::bind(program, NativeJsCode::Linked { entries }, NativeJsPublicationCounts::default(), None)
    }

    /// What this publication lowered: the counters that make an AOT claim
    /// measurable (`native_execution_rejections` covers the runtime half).
    pub fn publication_counts(&self) -> NativeJsPublicationCounts { self.counts }

    /// The whole-program globals the image folded (tests).
    #[cfg(test)]
    pub(crate) fn frozen_global_atoms_for_test(&self) -> &[crate::bytecode::Atom] {
        self.frozen.as_ref().map_or(&[], |frozen| &frozen.globals)
    }

    /// The whole-program prototype values the image folded (tests).
    #[cfg(test)]
    pub(crate) fn frozen_field_atoms_for_test(&self) -> &[crate::bytecode::Atom] {
        self.frozen.as_ref().map_or(&[], |frozen| &frozen.fields)
    }

    /// The method atoms the image holds unnamed, replayed at load.
    #[cfg(test)]
    pub(crate) fn frozen_atoms(&self) -> &[crate::bytecode::Atom] {
        self.frozen.as_ref().map_or(&[], |frozen| &frozen.atoms)
    }

    /// Whether a replayed assumption retired the image's stable facts.
    #[cfg(test)]
    pub(crate) fn frozen_invalidated(&self) -> bool {
        self.frozen.as_ref().is_some_and(|frozen| frozen.invalidation.is_set())
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
        // SAFETY: the caller's contract is this function's contract.
        unsafe { Self::from_entries_with_counts(program, entries, owner, NativeJsPublicationCounts::default(), None) }
    }

    pub(crate) unsafe fn from_entries_with_counts(
        program: ResidualProgram,
        entries: Vec<NativeJsFunctionEntry>,
        owner: Rc<dyn Any>,
        counts: NativeJsPublicationCounts,
        frozen: Option<FrozenAssumptions>,
    ) -> Result<Self, String> {
        Self::bind(
            program,
            NativeJsCode::Owned {
                entries: entries.into_boxed_slice(),
                _owner: owner,
            },
            counts,
            frozen,
        )
    }

    fn bind(program: ResidualProgram, code: NativeJsCode, counts: NativeJsPublicationCounts, frozen: Option<FrozenAssumptions>) -> Result<Self, String> {
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
            counts,
            frozen,
            host: PhantomData,
        })
    }
}

impl<H: Host> Vm<H> {
    pub(crate) fn set_native_program_resolver(&mut self, resolver: impl Fn(&str, &[&str]) -> Result<Option<NativeJsProgram<H>>, crate::NativeSourceError> + 'static) {
        self.set_native_program_resolver_boxed(Box::new(resolver));
    }

    pub(crate) fn set_native_program_resolver_boxed(&mut self, resolver: Box<NativeProgramResolver<H>>) {
        self.native_program_resolver = Some(resolver);
    }

    pub(super) fn resolve_native_program(&mut self, name: &str, parts: &[&str]) -> Result<Option<NativeJsProgram<H>>, crate::NativeSourceError> {
        if let Some(resolver) = &self.native_program_resolver
            && let Some(program) = resolver(name, parts)? { return Ok(Some(program)); }
        self.host.kept_native_program(name, parts)
    }

    /// Admit a linked module without remapping the atom IDs embedded in its code.
    /// Module records, environments and evaluation remain owned by the shared loader.
    pub(super) fn kept_native_module_program(
        &mut self, module: &crate::ModuleSource,
    ) -> Result<Option<ProgramId>, JsError> {
        let parts = [self.specialization_part(), module.compilation_goal(), &module.source];
        let Some(native) = self.resolve_native_program(&module.name, &parts).map_err(|error| JsError::validation(error.to_string()))? else {
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
        self.validate_native_namespace(&native)?;
        let mut program = Rc::unwrap_or_clone(native.program);
        program.atoms = crate::bytecode::AtomTable::shared(
            self.atom_sequence.clone(), program.atoms.len(),
        );
        Ok(crate::PreparedScript { program: Rc::new(program), native: Some(native.code) })
    }

    fn validate_native_namespace(&self, native: &NativeJsProgram<H>) -> Result<(), JsError> {
        if native.program.specialized != self.specialized {
            return Err(JsError::validation("native script specialization does not match runtime".into()));
        }
        if !self.atom_sequence.proves_prefix(&native.program.atoms)
            && (native.program.atoms.len() > self.atom_sequence.len()
            || native.program.atoms.iter().enumerate().any(|(id, name)| {
                !self.atom_units_equal_str(id as u32, name)
            }))
        {
            return Err(JsError::validation("native script atom IDs do not match runtime namespace".into()));
        }
        Ok(())
    }

    /// Establish the native namespace before publishing code; its operands cannot rebase.
    pub(crate) fn restore_native_snapshot(
        &mut self,
        bytes: crate::KeptBytes,
        native: &NativeJsProgram<H>,
        read_host: impl FnOnce(&mut H, &mut crate::SnapshotReader<'_, '_>) -> Result<(), String>,
    ) -> Result<(), JsError> {
        self.check_execution_interrupt()?;
        self.restore_snapshot_state(bytes, Some(&native.program), read_host).map_err(|error| JsError::validation(error.0))?;
        self.install_restored_main(native.program.clone());
        self.programs.attach_native_js(ProgramId::MAIN, native.code.clone()).map_err(JsError::validation)
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

    /// Replays an eager image's whole-program assumptions against this realm. A named
    /// method atom sets the image's invalidation word; the code then leaves at its
    /// invalidation point, and the shared body runs instead. The subscription retires it
    /// if a later call names one.
    fn replay_frozen_assumptions(&mut self, native: &NativeJsProgram<H>) {
        let Some(frozen) = &native.frozen else { return };
        let watcher: Rc<dyn crate::code_dependencies::DependentCode> = frozen.watch.clone();
        for &atom in frozen.atoms.iter() {
            if self.method_cache_atoms.contains(atom) {
                frozen.watch.retire();
            }
            self.method_cache_atoms.readers().subscribe(atom, &watcher);
        }
    }

    fn execute_native_with_scripts(
        &mut self,
        scripts: &[NativeJsProgram<H>],
        modules: &[NativeJsProgram<H>],
        native: &NativeJsProgram<H>,
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        self.replay_frozen_assumptions(native);
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


//! Ownership of linked JavaScript code and its compiler-resolved program.

#[allow(unused_imports)]
use crate::ext_traits::*;
use quench_runtime::vm::{ProgramId, Vm};
use quench_runtime::{Host, JsError, ResidualProgram, Value};
use std::rc::Rc;

pub use quench_runtime::vm::engine_types::NativeProgramResolver;

pub(crate) use quench_runtime::vm::engine_types::NativeJsCode;

pub use quench_runtime::vm::engine_types::NativeJsPublicationCounts;

pub(crate) use quench_runtime::vm::engine_types::FrozenAssumptions;



pub use quench_runtime::vm::engine_types::NativeJsProgram;



#[quench_macros::ext(NativeJsProgramOps1)]
impl NativeJsProgram {
    /// Compile every function before execution as one AOT image. No hotness or fallback tier.
    pub fn compile_eager(program: ResidualProgram) -> Result<Self, String> {
        let unit = crate::compiled::admission::ahead_unit(0).ok_or_else(|| "eager publication was declined".to_string())?;
        crate::compiled::compile_eager_native(program, unit)
    }

    /// Adopt a statically linked image table emitted by `lower_native_js_module`.
    ///
    /// # Safety
    /// `image` must be the aligned entry-table symbol for this exact program and
    /// host H, emitted by this build's object linker (`aot_image::object`). Its code,
    /// image file and records must live for the process's lifetime.
    pub unsafe fn from_linked_image(
        program: ResidualProgram,
        image: *const u8,
    ) -> Result<Self, String> {
        // SAFETY: the caller's contract is the table's.
        let table = unsafe { crate::aot_image::live::LinkedTable::read(image, program.function_count()) }?;
        Self::bind(program, NativeJsCode { table }, NativeJsPublicationCounts::default(), None)
    }

    /// What this publication lowered: the counters that make an AOT claim
    /// measurable (`native_execution_rejections` covers the runtime half).
    pub fn publication_counts(&self) -> NativeJsPublicationCounts { self.counts }

    /// The whole-program globals the image folded (tests).
    #[cfg(test)]
    pub(crate) fn frozen_global_atoms_for_test(&self) -> &[quench_runtime::bytecode::Atom] {
        self.frozen.as_ref().map_or(&[], |frozen| &frozen.globals)
    }

    /// Retire the image's stable facts, as a changed assumption would (tests).
    #[cfg(test)]
    pub(crate) fn invalidate_frozen(&self) {
        if let Some(frozen) = &self.frozen { frozen.watch.retire() }
    }

    /// How many constants the image materializes (tests).
    #[cfg(test)]
    pub(crate) fn materialized_constants(&self) -> usize {
        self.frozen.as_ref().map_or(0, |frozen| frozen.constants.borrow().len())
    }

    /// The whole-program prototype values the image folded (tests).
    #[cfg(test)]
    pub(crate) fn frozen_field_atoms_for_test(&self) -> &[quench_runtime::bytecode::Atom] {
        self.frozen.as_ref().map_or(&[], |frozen| &frozen.fields)
    }

    /// The method atoms the image holds unnamed, replayed at load.
    #[cfg(test)]
    pub(crate) fn frozen_atoms(&self) -> &[quench_runtime::bytecode::Atom] {
        self.frozen.as_ref().map_or(&[], |frozen| &frozen.atoms)
    }

    /// Whether a replayed assumption retired the image's stable facts.
    #[cfg(test)]
    pub(crate) fn frozen_invalidated(&self) -> bool {
        self.frozen.as_ref().is_some_and(|frozen| quench_runtime::test_support::invalidation_is_set(&frozen.invalidation))
    }
    fn bind(program: ResidualProgram, code: NativeJsCode, counts: NativeJsPublicationCounts, frozen: Option<FrozenAssumptions>) -> Result<Self, String> {
        if !quench_runtime::DYNAMIC_EXECUTION_ENABLED && program.source.as_ref().is_some_and(|source| source.call_sites.get().is_none()) {
            return Err("native program has no frozen call-site descriptions".into());
        }
        if program.kind == quench_runtime::bytecode::ProgramKind::Wasm {
            return Err("native JavaScript entries require a JS or TS program".into());
        }
        program.validate_unless_proven()?;
        Ok(Self {
            program: Rc::new(program),
            code: Rc::new(code),
            counts,
            frozen,
        })
    }
}

#[quench_macros::ext(NativeProgramVm)]
impl Vm {
    /// Loads the image the system linker placed for program `id` (`NativeJsCode`) as its ahead
    /// images (`Vm::load_linked_image`).
    pub(crate) fn attach_native_js(&mut self, id: ProgramId, code: &NativeJsCode) -> Result<(), String> {
        let program = self.programs.get(id).ok_or("native JS program is missing")?;
        self.load_linked_image(&program, &code.table)
    }

    pub(crate) fn set_native_program_resolver(&mut self, resolver: impl Fn(&str, &[&str]) -> Result<Option<NativeJsProgram>, quench_runtime::NativeSourceError> + 'static) {
        self.set_native_program_resolver_boxed(Box::new(resolver));
    }

    pub(crate) fn set_native_program_resolver_boxed(&mut self, resolver: Box<NativeProgramResolver>) {
        self.native_program_resolver = Some(resolver);
    }

    pub(crate) fn resolve_native_program(&mut self, name: &str, parts: &[&str]) -> Result<Option<NativeJsProgram>, quench_runtime::NativeSourceError> {
        if let Some(resolver) = &self.native_program_resolver
            && let Some(program) = resolver(name, parts)? { return Ok(Some(program)); }
        self.host.kept_native_program(name, parts)
    }

    /// Admit a linked module without remapping the atom IDs embedded in its code.
    /// Module records, environments and evaluation remain owned by the shared loader.
    pub(crate) fn kept_native_module_program(
        &mut self, module: &quench_runtime::ModuleSource,
    ) -> Result<Option<ProgramId>, JsError> {
        let parts = [self.specialization_part(), module.compilation_goal(), &module.source];
        let Some(native) = self.resolve_native_program(&module.name, &parts).map_err(|error| JsError::validation(error.to_string()))? else {
            return Ok(None);
        };
        self.store_native_module_program(native).map(Some)
    }

    pub(crate) fn store_native_module_program(&mut self, native: NativeJsProgram) -> Result<ProgramId, JsError> {
        if native.program.kind != quench_runtime::bytecode::ProgramKind::Module {
            return Err(JsError::validation("native module has the wrong parse goal".into()));
        }
        let prepared = self.prepare_native_script(native)?;
        let id = self.store_module_program_shared(prepared.program)
            .ok_or_else(|| JsError::validation("native module program store is full".into()))?;
        self.attach_native_js(id, &prepared.native.expect("native module code admitted"))
            .map_err(JsError::validation)?;
        Ok(id)
    }

    /// Native operands already contain atom IDs. Only an identical prefix of
    /// the initialized VM namespace can be adopted without rewriting the code.
    pub(crate) fn prepare_native_script(
        &mut self, native: NativeJsProgram,
    ) -> Result<quench_runtime::PreparedScript, JsError> {
        self.validate_native_namespace(&native)?;
        self.materialize_frozen_constants(&native);
        let mut program = Rc::unwrap_or_clone(native.program);
        program.atoms = quench_runtime::bytecode::AtomTable::shared(
            self.atom_sequence.clone(), program.atoms.len(),
        );
        Ok(quench_runtime::PreparedScript { program: Rc::new(program), native: Some(native.code) })
    }

    fn validate_native_namespace(&self, native: &NativeJsProgram) -> Result<(), JsError> {
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
        bytes: quench_runtime::KeptBytes,
        native: &NativeJsProgram,
        read_host: impl FnOnce(&mut dyn Host, &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String>,
    ) -> Result<(), JsError> {
        self.check_execution_interrupt()?;
        self.restore_snapshot_state(bytes, Some(&native.program), read_host).map_err(|error| JsError::validation(error.0))?;
        self.install_restored_main(native.program.clone());
        self.attach_native_js(ProgramId::MAIN, &native.code).map_err(JsError::validation)
    }

    pub(crate) fn execute_native_program(
        &mut self,
        native: &NativeJsProgram,
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        self.execute_native_with_scripts(&[], &[], native, drain_jobs)
    }

    pub(crate) fn execute_native_after_scripts(
        &mut self,
        scripts: &[NativeJsProgram],
        native: &NativeJsProgram,
    ) -> Result<Value, JsError> {
        self.execute_native_with_scripts(scripts, &[], native, true)
    }

    pub(crate) fn execute_native_module_graph(
        &mut self, scripts: &[NativeJsProgram], modules: &[NativeJsProgram], native: &NativeJsProgram,
    ) -> Result<Value, JsError> {
        self.execute_native_with_scripts(scripts, modules, native, true)
    }

    /// Replays an eager image's whole-program assumptions against this realm. A named
    /// method atom sets the image's invalidation word; the code then leaves at its
    /// invalidation point, and the shared body runs instead. The subscription retires it
    /// if a later call names one.
    fn replay_frozen_assumptions(&mut self, native: &NativeJsProgram) {
        let Some(frozen) = &native.frozen else { return };
        let watcher: Rc<dyn quench_runtime::code_dependencies::DependentCode> = frozen.watch.clone();
        for &atom in frozen.atoms.iter() {
            if self.method_cache_atoms.contains(atom) {
                frozen.watch.retire();
            }
            self.method_cache_atoms.readers().subscribe(atom, &watcher);
        }
    }

    /// Materializes the image's constant block in the fresh realm: strings are allocated once
    /// here, so the compiled literal reads the block instead of materializing per use.
    fn materialize_frozen_constants(&mut self, native: &NativeJsProgram) {
        let Some(frozen) = &native.frozen else { return };
        let mut slots = frozen.constants.borrow_mut();
        for (index, constant) in native.program.constants.iter().enumerate() {
            slots[index] = match constant {
                quench_runtime::bytecode::Constant::String(text) =>
                    self.allocate_string(quench_runtime::vm::wtf16::JsString::from_str(text)),
                other => crate::compiled::facts::immediate(other).unwrap_or(Value::UNDEFINED),
            };
        }
    }

    fn execute_native_with_scripts(
        &mut self,
        scripts: &[NativeJsProgram],
        modules: &[NativeJsProgram],
        native: &NativeJsProgram,
        drain_jobs: bool,
    ) -> Result<Value, JsError> {
        self.check_execution_interrupt()?;
        self.replay_frozen_assumptions(native);
        self.initialize_shared(&native.program, quench_runtime::vm::builtins::RealmScope::Complete)?;
        self.materialize_frozen_constants(native);
        self.attach_native_js(ProgramId::MAIN, &native.code).map_err(JsError::validation)?;
        for module in modules {
            if native.program.kind == quench_runtime::bytecode::ProgramKind::Module
                && quench_runtime::module_identity::same_name(&module.program.source_name, &native.program.source_name) {
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

/// The core's `NativeJsProgram::from_linked_image` (a link-time hook).
///
/// # Safety
/// The caller's contract is `NativeJsProgramOps1::from_linked_image`'s.
#[unsafe(no_mangle)]
unsafe fn quench_engine_native_from_linked_image(program: ResidualProgram, image: *const u8) -> Result<NativeJsProgram, String> {
    // SAFETY: the caller's contract.
    unsafe { <NativeJsProgram as NativeJsProgramOps1>::from_linked_image(program, image) }
}
const _: unsafe fn(ResidualProgram, *const u8) -> Result<NativeJsProgram, String> = quench_engine_native_from_linked_image;

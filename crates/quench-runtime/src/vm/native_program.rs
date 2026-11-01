//! Ownership of linked JavaScript code and its compiler-resolved program.

use super::{ProgramId, Vm};
use crate::{Host, JsError, ResidualProgram, Value};
use std::{any::Any, marker::PhantomData, rc::Rc};

/// The shared native function ABI, with an opaque VM-owned call context.
pub type NativeJsFunctionEntry =
    unsafe extern "C" fn(*mut std::ffi::c_void, u64, u64, *const u64, *mut u64) -> u32;

pub type NativeProgramResolver<H> = dyn Fn(&str, &[&str]) -> Result<Option<NativeJsProgram<H>>, crate::NativeSourceError>;

/// A pure numeric predicate's statically resolved inputs and shared JIT body.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeBooleanRegion {
    pub(super) function: u32,
    pub(super) capture_count: u16,
    pub(super) field_count: u16,
    pub(super) flags: u32,
    pub(super) capture_slots: [u16; super::js_compiled::MAX_CAPTURE_ARGUMENTS],
    pub(super) field_sites: [u16; super::js_compiled::MAX_NUMERIC_FIELD_INPUTS],
    pub(super) field_sources: [u16; super::js_compiled::MAX_NUMERIC_FIELD_INPUTS],
    pub(super) entry: unsafe extern "C" fn(*const f64) -> u32,
}

impl NativeBooleanRegion {
    pub(super) const SIGNED_INT32_INPUTS: u32 = 1;
}

pub(super) const BOOLEAN_REGION_COUNT_WORD: usize = 1;
pub(super) const CAPTURED_STORE_COUNT_WORD: usize = 2;

/// A projection of the shared captured-store proof needed by a linked call.
#[repr(C)]
#[derive(Clone, Copy)]
pub(crate) struct NativeCapturedStore {
    pub(super) function: u32,
    pub(super) atom: crate::bytecode::Atom,
    pub(super) capture_slot: u16,
    pub(super) cache_site: u16,
    pub(super) operator: u16,
}
impl NativeCapturedStore {
    pub(super) fn update(self) -> Option<super::operations::NumberBitwise> {
        (self.operator != super::js_compiled::CAPTURED_STORE_ASSIGNMENT).then(|| {
            super::operations::NumberBitwise::from_operator(u32::from(self.operator))
                .expect("validated native captured-store operator")
        })
    }
}
const _: () = assert!(std::mem::align_of::<NativeBooleanRegion>() <= std::mem::align_of::<usize>());

pub(crate) enum NativeJsCode {
    Linked {
        entries: &'static [NativeJsFunctionEntry],
        boolean_regions: &'static [NativeBooleanRegion],
        captured_stores: &'static [NativeCapturedStore],
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

    #[inline]
    pub(super) fn boolean_region(&self, function: u32) -> Option<NativeBooleanRegion> {
        let Self::Linked { boolean_regions, .. } = self else { return None; };
        boolean_regions.binary_search_by_key(&function, |region| region.function)
            .ok().map(|index| boolean_regions[index])
    }

    #[inline]
    pub(super) fn captured_store(&self, function: u32) -> Option<NativeCapturedStore> {
        let Self::Linked { captured_stores, .. } = self else { return None; };
        captured_stores.binary_search_by_key(&function, |store| store.function)
            .ok().map(|index| captured_stores[index])
    }

    #[inline]
    pub(super) fn has_guarded_calls(&self) -> bool {
        matches!(self, Self::Linked { boolean_regions, captured_stores, .. }
            if !boolean_regions.is_empty() || !captured_stores.is_empty())
    }
}

/// A program bound to native entries compiled for the same host and semantic ABI.
pub struct NativeJsProgram<H> {
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
    /// lifetime. The table has the ABI header, one entry per function, and
    /// its statically generated boolean-region and captured-store records.
    pub unsafe fn from_linked_image(
        program: ResidualProgram,
        image: *const u8,
    ) -> Result<Self, String> {
        if image.is_null() || image.addr() % std::mem::align_of::<NativeJsFunctionEntry>() != 0 {
            return Err("native image table is missing or misaligned".into());
        }
        let header_bytes =
            crate::NATIVE_JS_IMAGE_HEADER_WORDS * std::mem::size_of::<NativeJsFunctionEntry>();
        // SAFETY: the image contract supplies its aligned header.
        let region_count = unsafe {
            image.cast::<usize>().add(BOOLEAN_REGION_COUNT_WORD).read()
        };
        if region_count > program.function_count() {
            return Err("native boolean region count exceeds function count".into());
        }
        // SAFETY: the image contract supplies the full table. Skip the marker
        // before interpreting words as functions of the execution-entry ABI.
        let entries = unsafe {
            std::slice::from_raw_parts(
                image.add(header_bytes).cast::<NativeJsFunctionEntry>(),
                program.function_count(),
            )
        };
        let region_offset = program.function_count()
            .checked_mul(std::mem::size_of::<NativeJsFunctionEntry>())
            .and_then(|bytes| header_bytes.checked_add(bytes))
            .ok_or("native boolean region offset overflow")?;
        if region_count.checked_mul(std::mem::size_of::<NativeBooleanRegion>())
            .is_none_or(|bytes| bytes > isize::MAX as usize)
        {
            return Err("native boolean region size overflow".into());
        }
        // SAFETY: the image contract supplies the recorded region table and its code.
        let boolean_regions = unsafe {
            std::slice::from_raw_parts(image.add(region_offset).cast::<NativeBooleanRegion>(), region_count)
        };
        if boolean_regions.iter().enumerate().any(|(index, region)| {
            region.function as usize >= program.function_count()
                || usize::from(region.capture_count) > region.capture_slots.len()
                || usize::from(region.field_count) > region.field_sites.len()
                || region.flags & !NativeBooleanRegion::SIGNED_INT32_INPUTS != 0
                || region.field_sources[..usize::from(region.field_count)].iter()
                    .enumerate().any(|(index, source)| usize::from(*source) > index)
                || (index > 0 && boolean_regions[index - 1].function >= region.function)
        }) {
            return Err("invalid native boolean region metadata".into());
        }
        // SAFETY: the image contract supplies the complete ABI header.
        let store_count = unsafe { image.cast::<usize>().add(CAPTURED_STORE_COUNT_WORD).read() };
        if store_count > program.function_count() {
            return Err("native captured-store count exceeds function count".into());
        }
        let store_offset = region_count.checked_mul(std::mem::size_of::<NativeBooleanRegion>())
            .and_then(|bytes| region_offset.checked_add(bytes))
            .ok_or("native captured-store offset overflow")?;
        if store_count.checked_mul(std::mem::size_of::<NativeCapturedStore>())
            .and_then(|bytes| store_offset.checked_add(bytes))
            .is_none_or(|end| end > isize::MAX as usize)
        {
            return Err("native captured-store size overflow".into());
        }
        // SAFETY: the linked image contract supplies these static records.
        let captured_stores = unsafe {
            std::slice::from_raw_parts(image.add(store_offset).cast::<NativeCapturedStore>(), store_count)
        };
        if captured_stores.iter().enumerate().any(|(index, store)| {
            store.function as usize >= program.function_count()
                || store.atom as usize >= program.atoms.len()
                || (store.operator != super::js_compiled::CAPTURED_STORE_ASSIGNMENT
                    && super::operations::NumberBitwise::from_operator(u32::from(store.operator)).is_none())
                || (index > 0 && captured_stores[index - 1].function >= store.function)
        }) {
            return Err("invalid native captured-store metadata".into());
        }
        // SAFETY: the linked image is static and bound to this program and H.
        Self::bind(program, NativeJsCode::Linked { entries, boolean_regions, captured_stores })
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

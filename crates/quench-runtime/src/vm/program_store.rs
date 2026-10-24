use super::wtf16::JsString;
use crate::Value;
use crate::bytecode::ResidualProgram;
use crate::heap::WeakHandle;
use rustc_hash::FxHashMap;
use std::rc::{Rc, Weak};

struct ProgramEntry {
    residual: Rc<ResidualProgram>,
    wasm_signatures: Option<Rc<crate::wasm::WasmSignatures>>,
    constants: Vec<Value>,
    const_arrays: Vec<Option<Rc<Vec<Value>>>>,
    function_sources: Vec<Option<Value>>,
    regexp_literals: Vec<Option<Result<Rc<quench_regexp::Regex>, Rc<str>>>>,
    module_environment: Option<Value>,
    import_meta: Option<Value>,
    module_imports: Vec<(u16, ModuleImport)>,
    module: bool,
    /// Per-function lane views, derived from the residual on first lane entry.
    lane_views: Vec<std::cell::OnceCell<super::dispatch_fast::LaneView>>,
    /// The program's compiled functions, once the compiled mode first calls
    /// one; `Some(None)` records that none compile.
    compiled: Option<Option<Rc<super::compiled::CompiledProgram>>>,
    /// The environment the program's root body evaluates in.
    scope: ProgramScope,
    /// Source provenance is independent of the root binding environment.
    origin: CompilationOrigin,
    /// The Script or Module an eval program's code belongs to, which resolves
    /// its `import()` specifiers; `None` for a program that is its own.
    script_or_module: Option<Rc<str>>,
    /// Dynamic code (eval, `Function`, host scripts): reclaimed once nothing
    /// can run it. The main program and modules stay for the realm's life.
    reclaimable: bool,
    /// The reclamation pass current at insertion.
    pass: u64,
}

pub(super) const ANONYMOUS_EVAL_ORIGIN: &str = "eval at <anonymous>";

#[derive(Clone)]
pub(super) enum CompilationOrigin {
    Source,
    /// Execution state in another realm of one logical host Script.
    SharedScript {
        source: ProgramId,
    },
    Eval {
        caller: Option<JsString>,
    },
    FunctionConstructor {
        caller: JsString,
    },
}

/// Where a stored program's root body evaluates, which decides whether its
/// by-name loads can only reach the global environment.
#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum ProgramScope {
    /// As its kind dictates: Script code in the global environment.
    #[default]
    Own,
    /// An eval program evaluated in the global environment (indirect eval or a
    /// host script), whose names resolve as script code's do.
    Global,
    /// A sloppy direct eval of global code: it declares global variables but
    /// still sees its caller's lexical scope.
    GlobalVariables,
    /// A sloppy direct eval in a function's or module's scope: its variables
    /// belong to its caller's variable environment.
    Caller,
}

#[derive(Clone, Copy)]
pub(crate) enum ModuleImport {
    Value(Value),
    Binding(ProgramId, u16, Value),
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub(crate) struct ProgramId(u32);

impl ProgramId {
    pub(crate) const MAIN: Self = Self(0);

    fn from_index(index: usize) -> Option<Self> {
        u32::try_from(index).ok().map(Self)
    }

    fn index(self) -> usize {
        self.0 as usize
    }

    pub(crate) const fn raw(self) -> u32 {
        self.0
    }

    pub(crate) const fn from_raw(raw: u32) -> Self {
        Self(raw)
    }
}

struct OwnedScript {
    owner: Weak<crate::api::CompiledScriptState>,
    instances: Vec<(WeakHandle, ProgramId)>,
}

#[derive(Default)]
pub(crate) struct ProgramStore {
    /// Indexed by `ProgramId`; a reclaimed program leaves an empty slot, so an
    /// id never names another program.
    programs: Vec<Option<ProgramEntry>>,
    /// Weak compiled-script identities for global eval; contexts are supplied per call.
    global_eval_programs: FxHashMap<JsString, ProgramId>,
    owned_scripts: FxHashMap<ProgramId, OwnedScript>,
    /// Completed reclamation passes; see `reclaim_unreferenced`.
    reclaim_passes: u64,
    /// Reclaimable programs inserted since the last reclamation pass.
    unchecked_reclaimable: usize,
}

impl ProgramStore {
    pub(super) fn cached_global_eval(&self, source: &JsString) -> Option<ProgramId> {
        let program = *self.global_eval_programs.get(source)?;
        self.entry(program).map(|_| program)
    }

    pub(super) fn cache_global_eval(&mut self, source: JsString, program: ProgramId) {
        self.global_eval_programs.insert(source, program);
    }

    pub(crate) fn len(&self) -> usize {
        self.programs.len()
    }

    fn entry(&self, id: ProgramId) -> Option<&ProgramEntry> {
        self.programs.get(id.index())?.as_ref()
    }

    fn entry_mut(&mut self, id: ProgramId) -> Option<&mut ProgramEntry> {
        self.programs.get_mut(id.index())?.as_mut()
    }

    /// Inserts dynamic code that `reclaim_unreferenced` may later drop.
    pub(crate) fn insert_reclaimable_shared(
        &mut self,
        program: Rc<ResidualProgram>,
    ) -> Option<ProgramId> {
        let id = self.insert_shared(program)?;
        if let Some(entry) = self.entry_mut(id) {
            entry.reclaimable = true;
        }
        self.unchecked_reclaimable += 1;
        Some(id)
    }

    #[cfg(test)]
    pub(crate) fn live_count(&self) -> usize {
        self.programs.iter().flatten().count()
    }

    /// Whether dynamic code may be reclaimable, including dropped host owners.
    pub(crate) fn has_unchecked_reclaimable(&self) -> bool {
        self.unchecked_reclaimable > 0
            || self
                .owned_scripts
                .values()
                .any(|script| script.owner.strong_count() == 0)
    }

    /// Drops every reclaimable program that `referenced` does not mark and that
    /// predates the previous pass. A program therefore survives the first pass
    /// after its insertion, while its first closure or frame is still being
    /// created. Returns the reclaimed ids.
    pub(crate) fn reclaim_unreferenced(&mut self, mut referenced: Vec<bool>) -> Vec<ProgramId> {
        // Owning a Script retains its code and live-realm execution state. An
        // escaped function retains its source record even after the owner drops.
        for (&source, script) in &self.owned_scripts {
            let owned = script.owner.strong_count() != 0;
            let live_instance = script
                .instances
                .iter()
                .any(|(_, id)| referenced.get(id.index()).copied().unwrap_or(false));
            if (owned || live_instance)
                && let Some(mark) = referenced.get_mut(source.index())
            {
                *mark = true;
            }
            if owned {
                for (_, id) in &script.instances {
                    if let Some(mark) = referenced.get_mut(id.index()) {
                        *mark = true;
                    }
                }
            }
        }
        let previous_pass = self.reclaim_passes;
        self.reclaim_passes += 1;
        let mut reclaimed = Vec::new();
        let mut pending = 0;
        for (index, slot) in self.programs.iter_mut().enumerate() {
            let Some(entry) = slot else { continue };
            if !entry.reclaimable || referenced.get(index).copied().unwrap_or(true) {
                continue;
            }
            if entry.pass >= previous_pass {
                pending += 1;
                continue;
            }
            *slot = None;
            reclaimed.extend(ProgramId::from_index(index));
        }
        self.unchecked_reclaimable = pending;
        if !reclaimed.is_empty() {
            let programs = &self.programs;
            self.global_eval_programs
                .retain(|_, id| programs.get(id.index()).is_some_and(Option::is_some));
            self.owned_scripts
                .retain(|source, _| programs.get(source.index()).is_some_and(Option::is_some));
        }
        reclaimed
    }

    pub(crate) fn reset(&mut self, main: Rc<ResidualProgram>) -> ProgramId {
        self.global_eval_programs.clear();
        self.owned_scripts.clear();
        self.programs.clear();
        self.unchecked_reclaimable = 0;
        let module = main.is_module();
        let id = self.insert_shared(main).unwrap_or(ProgramId::MAIN);
        if module && let Some(entry) = self.entry_mut(id) {
            entry.module = true;
        }
        id
    }

    pub(super) fn set_script_or_module(&mut self, id: ProgramId, name: Rc<str>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.script_or_module = Some(name);
        }
    }

    /// The name of the Script or Module the program's code belongs to.
    pub(super) fn script_or_module(&self, id: ProgramId) -> Option<Rc<str>> {
        let entry = self.entry(id)?;
        Some(
            entry
                .script_or_module
                .clone()
                .unwrap_or_else(|| Rc::from(entry.residual.source_name.as_str())),
        )
    }

    pub(super) fn register_owned_script(&mut self, script: &Rc<crate::api::CompiledScriptState>) {
        self.owned_scripts.insert(
            script.source,
            OwnedScript {
                owner: Rc::downgrade(script),
                instances: Vec::new(),
            },
        );
    }

    pub(super) fn owns_script(&self, script: &Rc<crate::api::CompiledScriptState>) -> bool {
        self.owned_scripts
            .get(&script.source)
            .is_some_and(|entry| std::ptr::eq(entry.owner.as_ptr(), Rc::as_ptr(script)))
    }

    pub(super) fn owned_script_instance(
        &self,
        source: ProgramId,
        realm: WeakHandle,
    ) -> Option<ProgramId> {
        self.owned_scripts
            .get(&source)?
            .instances
            .iter()
            .find_map(|(owner, id)| (*owner == realm).then_some(*id))
    }

    pub(super) fn bind_script_instance(
        &mut self,
        source: ProgramId,
        realm: WeakHandle,
        id: ProgramId,
    ) {
        self.set_origin(id, CompilationOrigin::SharedScript { source });
        self.owned_scripts
            .get_mut(&source)
            .expect("registered Script source")
            .instances
            .push((realm, id));
    }

    pub(super) fn source_identity(&self, id: ProgramId) -> ProgramId {
        match self.entry(id).map(|entry| &entry.origin) {
            Some(CompilationOrigin::SharedScript { source }) => *source,
            _ => id,
        }
    }

    /// Prune realm identities before swept heap slots can be reused. Compiled
    /// Script ownership must never keep a realm global alive through this cache.
    pub(super) fn prune_owned_script_realms(&mut self, mut live: impl FnMut(WeakHandle) -> bool) {
        for script in self.owned_scripts.values_mut() {
            let before = script.instances.len();
            script.instances.retain(|(realm, _)| live(*realm));
            self.unchecked_reclaimable += before - script.instances.len();
        }
    }

    pub(super) fn set_origin(&mut self, id: ProgramId, origin: CompilationOrigin) {
        if let Some(entry) = self.entry_mut(id) {
            entry.origin = origin;
        }
    }

    pub(super) fn eval_origin(&self, id: ProgramId) -> Option<JsString> {
        match &self.entry(id)?.origin {
            CompilationOrigin::Source | CompilationOrigin::SharedScript { .. } => None,
            CompilationOrigin::Eval { caller } => Some(
                caller
                    .clone()
                    .unwrap_or_else(|| JsString::from_str(ANONYMOUS_EVAL_ORIGIN)),
            ),
            CompilationOrigin::FunctionConstructor { caller } => Some(caller.clone()),
        }
    }

    pub(super) fn is_function_constructor_body(&self, id: ProgramId, function: u32) -> bool {
        self.entry(id).is_some_and(|entry| {
            matches!(entry.origin, CompilationOrigin::FunctionConstructor { .. })
                && entry
                    .residual
                    .functions
                    .get(function as usize)
                    .is_some_and(|function| function.parent == Some(super::ROOT_FUNCTION_ID))
        })
    }

    pub(crate) fn mark_global_scope(&mut self, id: ProgramId) {
        self.set_scope(id, ProgramScope::Global);
    }

    pub(crate) fn mark_global_variables(&mut self, id: ProgramId) {
        self.set_scope(id, ProgramScope::GlobalVariables);
    }

    /// Whether the program's root declares the realm's global variables.
    pub(crate) fn declares_global_variables(&self, id: ProgramId) -> bool {
        self.entry(id).is_some_and(|entry| match entry.scope {
            ProgramScope::Own => entry.residual.kind == crate::bytecode::ProgramKind::Script,
            ProgramScope::Global | ProgramScope::GlobalVariables => true,
            ProgramScope::Caller => false,
        })
    }

    pub(crate) fn mark_caller_scope(&mut self, id: ProgramId) {
        self.set_scope(id, ProgramScope::Caller);
    }

    /// Whether the program's root variables belong to its caller.
    pub(crate) fn runs_in_caller_scope(&self, id: ProgramId) -> bool {
        self.entry(id)
            .is_some_and(|entry| entry.scope == ProgramScope::Caller)
    }

    fn set_scope(&mut self, id: ProgramId, scope: ProgramScope) {
        if let Some(entry) = self.entry_mut(id) {
            entry.scope = scope;
        }
    }

    /// Whether the program's root body evaluates in the global environment.
    pub(crate) fn runs_in_global_scope(&self, id: ProgramId) -> bool {
        self.entry(id).is_some_and(|entry| match entry.scope {
            ProgramScope::Own => entry.residual.kind == crate::bytecode::ProgramKind::Script,
            ProgramScope::Global => true,
            ProgramScope::GlobalVariables | ProgramScope::Caller => false,
        })
    }

    pub(crate) fn insert(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        self.insert_shared(Rc::new(program))
    }

    pub(crate) fn find_shared(&self, program: &Rc<ResidualProgram>) -> Option<ProgramId> {
        self.programs
            .iter()
            .position(|entry| {
                entry
                    .as_ref()
                    .is_some_and(|entry| Rc::ptr_eq(&entry.residual, program))
            })
            .and_then(ProgramId::from_index)
    }

    pub(crate) fn insert_shared(&mut self, program: Rc<ResidualProgram>) -> Option<ProgramId> {
        let id = ProgramId::from_index(self.programs.len())?;
        let regexp_literals = vec![None; program.regexp_literal_sites.len()];
        let function_sources = vec![None; program.functions.len()];
        let lane_views = program
            .functions
            .iter()
            .map(|_| std::cell::OnceCell::new())
            .collect();
        let origin = if program.kind == crate::bytecode::ProgramKind::Eval {
            CompilationOrigin::Eval { caller: None }
        } else {
            CompilationOrigin::Source
        };
        self.programs.push(Some(ProgramEntry {
            residual: program,
            wasm_signatures: None,
            constants: Vec::new(),
            const_arrays: Vec::new(),
            function_sources,
            regexp_literals,
            module_environment: None,
            import_meta: None,
            module_imports: Vec::new(),
            module: false,
            lane_views,
            compiled: None,
            scope: ProgramScope::Own,
            origin,
            script_or_module: None,
            reclaimable: false,
            pass: self.reclaim_passes,
        }));
        Some(id)
    }

    /// Attach the module's canonical type facts; a code identity cannot acquire
    /// a different signature authority after functions have been registered.
    pub(crate) fn attach_wasm_signatures(
        &mut self,
        id: ProgramId,
        signatures: &Rc<crate::wasm::WasmSignatures>,
    ) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        if let Some(existing) = &entry.wasm_signatures {
            return Rc::ptr_eq(existing, signatures);
        }
        if entry.residual.validate().is_err()
            || signatures.defined_count() != entry.residual.function_count()
        {
            return false;
        }
        for function in &entry.residual.functions {
            for instruction in &function.code {
                let instruction = if instruction.is_wide() {
                    function.wide[instruction.wide_index()]
                } else {
                    instruction.as_wide()
                };
                let valid = match instruction.op() {
                    crate::bytecode::Op::WasmRefFunc => {
                        signatures.get(instruction.imm() as usize).is_some()
                    }
                    crate::bytecode::Op::WasmIndirectTarget => {
                        signatures.type_signature(instruction.imm()).is_some()
                    }
                    op if crate::wasm::gc::StructConstruction::from_op(op).is_some() => {
                        let mode = crate::wasm::gc::StructConstruction::from_op(op).unwrap();
                        signatures
                            .declarations
                            .struct_fields(instruction.imm())
                            .is_some_and(|fields| {
                                mode.described()
                                    == signatures
                                        .declarations
                                        .descriptor_type(instruction.imm())
                                        .is_some()
                                    && fields.iter().all(|field| match field.element_type {
                                        wasmparser::StorageType::I8
                                        | wasmparser::StorageType::I16 => true,
                                        wasmparser::StorageType::Val(ty) => {
                                            signatures
                                                .declarations
                                                .callable_value_type(ty)
                                                .is_some()
                                                && (!mode.defaulted() || ty.is_defaultable())
                                        }
                                    })
                                    && (mode == crate::wasm::gc::StructConstruction::Default
                                        || usize::from(instruction.register_window().count)
                                            == if mode.defaulted() {
                                                1
                                            } else {
                                                fields.len() + usize::from(mode.described())
                                            })
                            })
                    }
                    crate::bytecode::Op::WasmRefGetDesc => signatures
                        .declarations
                        .descriptor_type(instruction.imm())
                        .is_some(),
                    crate::bytecode::Op::WasmArrayNewData
                    | crate::bytecode::Op::WasmArrayNewElem => {
                        use crate::wasm::gc::{ArraySegmentInput, ArraySegmentKind};
                        instruction.register_window().count == ArraySegmentInput::COUNT
                            && signatures
                                .declarations
                                .array_field(instruction.imm())
                                .is_some_and(|field| {
                                    ArraySegmentKind::from_op(instruction.op())
                                        .unwrap()
                                        .accepts(field.element_type)
                                        && match field.element_type {
                                            wasmparser::StorageType::Val(ty) => signatures
                                                .declarations
                                                .callable_value_type(ty)
                                                .is_some(),
                                            _ => true,
                                        }
                                })
                    }
                    crate::bytecode::Op::WasmArrayNew
                    | crate::bytecode::Op::WasmArrayNewDefault
                    | crate::bytecode::Op::WasmArrayNewFixed => signatures
                        .declarations
                        .array_field(instruction.imm())
                        .is_some_and(|field| {
                            let valid = match field.element_type {
                                wasmparser::StorageType::I8 | wasmparser::StorageType::I16 => true,
                                wasmparser::StorageType::Val(ty) => {
                                    signatures.declarations.callable_value_type(ty).is_some()
                                        && (instruction.op()
                                            != crate::bytecode::Op::WasmArrayNewDefault
                                            || ty.is_defaultable())
                                }
                            };
                            valid
                                && (instruction.op() != crate::bytecode::Op::WasmArrayNewFixed
                                    || usize::from(instruction.register_b())
                                        .checked_add(usize::from(
                                            instruction.register_window().count,
                                        ))
                                        .is_some_and(|end| end <= usize::from(function.registers)))
                        }),
                    crate::bytecode::Op::WasmDescriptorTest
                    | crate::bytecode::Op::WasmDescriptorCast => {
                        crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm())
                            .and_then(|target| target.descriptor_type(&signatures.declarations))
                            .is_some()
                    }
                    crate::bytecode::Op::WasmRefTest | crate::bytecode::Op::WasmRefCast => {
                        crate::wasm::reference::ReferenceTarget::from_tag(instruction.imm())
                            .and_then(|target| target.reference_type())
                            .and_then(|ty| {
                                signatures
                                    .declarations
                                    .callable_value_type(wasmparser::ValType::Ref(ty))
                            })
                            .is_some()
                    }
                    _ => true,
                };
                if !valid {
                    return false;
                }
            }
        }
        entry.wasm_signatures = Some(signatures.clone());
        true
    }

    pub(crate) fn wasm_function_signature(
        &self,
        id: ProgramId,
        index: u32,
    ) -> Option<&crate::WasmSignature> {
        self.entry(id)?
            .wasm_signatures
            .as_ref()?
            .defined_signature(index)
    }

    pub(crate) fn wasm_signatures(&self, id: ProgramId) -> Option<&crate::wasm::WasmSignatures> {
        self.entry(id)?.wasm_signatures.as_deref()
    }

    pub(crate) fn insert_module(&mut self, program: ResidualProgram) -> Option<ProgramId> {
        let id = self.insert(program)?;
        if let Some(entry) = self.entry_mut(id) {
            entry.module = true;
        }
        Some(id)
    }

    /// Whether `slot` of `id`'s root environment is the binding itself: no global var,
    /// global lexical or module-import projection reads through it.
    pub(crate) fn root_slot_is_binding(&self, id: ProgramId, slot: u16) -> bool {
        self.entry(id).is_some_and(|entry| {
            entry.residual.root_slots_are_bindings()
                && !entry.module_imports.iter().any(|(local, _)| *local == slot)
        })
    }

    pub(crate) fn is_module(&self, id: ProgramId) -> bool {
        self.entry(id).is_some_and(|entry| entry.module)
    }

    pub(crate) fn set_module_environment(&mut self, id: ProgramId, environment: Value) {
        if let Some(entry) = self.entry_mut(id)
            && entry.module
        {
            entry.module_environment = Some(environment);
        }
    }

    pub(crate) fn module_environment(&self, id: ProgramId) -> Option<Value> {
        self.entry(id)?.module_environment
    }

    pub(crate) fn import_meta(&self, id: ProgramId) -> Option<Value> {
        self.entry(id)?.import_meta
    }

    pub(crate) fn source_name(&self, id: ProgramId) -> Option<&str> {
        Some(&self.entry(id)?.residual.source_name)
    }

    pub(crate) fn set_import_meta(&mut self, id: ProgramId, value: Value) {
        if let Some(entry) = self.entry_mut(id)
            && entry.module
        {
            entry.import_meta = Some(value);
        }
    }

    pub(crate) fn set_module_imports(&mut self, id: ProgramId, imports: Vec<(u16, ModuleImport)>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.module_imports = imports;
        }
    }

    pub(crate) fn module_imports(&self, id: ProgramId) -> &[(u16, ModuleImport)] {
        self.entry(id).map_or(&[], |entry| &entry.module_imports)
    }

    pub(crate) fn module_import(&self, id: ProgramId, slot: u16) -> Option<ModuleImport> {
        self.entry(id).and_then(|entry| {
            entry
                .module_imports
                .iter()
                .find_map(|(local, import)| (*local == slot).then_some(*import))
        })
    }

    /// Borrows the residual without the reference-count traffic of `get`.
    pub(crate) fn residual(&self, id: ProgramId) -> Option<&ResidualProgram> {
        self.entry(id).map(|entry| &*entry.residual)
    }

    pub(crate) fn get(&self, id: ProgramId) -> Option<Rc<ResidualProgram>> {
        self.entry(id).map(|entry| entry.residual.clone())
    }

    pub(crate) fn set_constants(&mut self, id: ProgramId, constants: Vec<Value>) {
        if let Some(entry) = self.entry_mut(id) {
            entry.const_arrays = vec![None; constants.len()];
            entry.constants = constants;
            // Lane views carry constants, so they derive from the new set.
            entry
                .lane_views
                .iter_mut()
                .for_each(|view| *view = Default::default());
        }
    }

    /// The lane view of `function`, derived by `derive` on first use from the
    /// immutable residual and the program's constants.
    pub(super) fn compiled(
        &self,
        id: ProgramId,
    ) -> Option<Option<Rc<super::compiled::CompiledProgram>>> {
        self.entry(id)?.compiled.clone()
    }

    pub(super) fn set_compiled(
        &mut self,
        id: ProgramId,
        compiled: Option<Rc<super::compiled::CompiledProgram>>,
    ) {
        if let Some(entry) = self.entry_mut(id) {
            entry.compiled = Some(compiled);
        }
    }

    /// A program's lane views by function, derived on first use.
    pub(super) fn lane_views(
        &self,
        id: ProgramId,
    ) -> Option<&[std::cell::OnceCell<super::dispatch_fast::LaneView>]> {
        Some(&self.entry(id)?.lane_views)
    }

    pub(super) fn lane_view(
        &self,
        id: ProgramId,
        function: u32,
        derive: impl FnOnce(
            &crate::bytecode::Function,
            &[Value],
            crate::bytecode::ProgramKind,
        ) -> super::dispatch_fast::LaneView,
    ) -> Option<*const super::dispatch_fast::LaneView> {
        let entry = self.entry(id)?;
        let code = entry.residual.functions.get(function as usize)?;
        let view = entry.lane_views.get(function as usize)?;
        Some(view.get_or_init(|| derive(code, &entry.constants, entry.residual.kind)))
    }

    pub(crate) fn constant(&self, id: ProgramId, index: usize) -> Option<Value> {
        self.constants(id).get(index).copied()
    }

    pub(crate) fn constants(&self, id: ProgramId) -> &[Value] {
        self.entry(id)
            .map_or(&[], |entry| entry.constants.as_slice())
    }

    pub(crate) fn const_array(
        &mut self,
        id: ProgramId,
        start: usize,
        count: usize,
    ) -> Option<Rc<Vec<Value>>> {
        let entry = self.entry_mut(id)?;
        let end = start.checked_add(count)?;
        let cached = entry.const_arrays.get_mut(start)?;
        if cached.is_none() {
            *cached = Some(Rc::new(entry.constants.get(start..end)?.to_vec()));
        }
        cached.clone()
    }

    pub(crate) fn function_source(&self, id: ProgramId, function: u32) -> Option<Value> {
        self.entry(id)?
            .function_sources
            .get(function as usize)
            .copied()
            .flatten()
    }

    pub(crate) fn cache_function_source(
        &mut self,
        id: ProgramId,
        function: u32,
        source: Value,
    ) -> Option<Value> {
        let cached = self
            .entry_mut(id)?
            .function_sources
            .get_mut(function as usize)?;
        Some(*cached.get_or_insert(source))
    }

    pub(crate) fn regexp_literal_matcher(
        &self,
        id: ProgramId,
        site: usize,
    ) -> Option<Result<Rc<quench_regexp::Regex>, Rc<str>>> {
        self.entry(id)?.regexp_literals.get(site)?.as_ref().cloned()
    }

    pub(crate) fn cache_regexp_literal_matcher(
        &mut self,
        id: ProgramId,
        site: usize,
        matcher: Result<Rc<quench_regexp::Regex>, Rc<str>>,
    ) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let Some(cached) = entry.regexp_literals.get_mut(site) else {
            return false;
        };
        if cached.is_none() {
            *cached = Some(matcher);
        }
        true
    }

    #[cfg(test)]
    pub(crate) fn compiled_regexp_literal_count(&self, id: ProgramId) -> usize {
        self.entry(id).map_or(0, |entry| {
            entry
                .regexp_literals
                .iter()
                .filter(|site| site.is_some())
                .count()
        })
    }

    /// Appends every value the stored programs keep alive to `roots`.
    pub(crate) fn extend_roots(&self, roots: &mut Vec<Value>) {
        for entry in self.programs.iter().flatten() {
            roots.extend_from_slice(&entry.constants);
            roots.extend(entry.function_sources.iter().flatten());
            roots.extend(entry.module_environment);
            roots.extend(entry.import_meta);
            roots.extend(entry.module_imports.iter().map(|(_, import)| match import {
                ModuleImport::Value(value) => *value,
                ModuleImport::Binding(_, _, fallback) => *fallback,
            }));
        }
    }
}

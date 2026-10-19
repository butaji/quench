use crate::Value;
use super::wtf16::JsString;
use crate::bytecode::ResidualProgram;
use std::rc::{Rc, Weak};
use crate::heap::WeakHandle;
use rustc_hash::FxHashMap;
use std::io::Write as _;

#[derive(Default)]
struct JsJitCounters {
    source_name: Option<Box<str>>,
    function_name: Option<Box<str>>,
    entries: u64,
    numeric_entries: u64,
    promotions: u64,
    compile_attempts: u64,
    compile_successes: u64,
    compile_ns: u64,
    native_entries: u64,
    code_bytes: usize,
}

/// Prespecified looks for the Number-entry profile. Critical counts are the
/// exact one-sided Binomial( n, 0.80 ) cutoffs at alpha = 0.05 / 4 per look.
/// A union bound caps false promotion at 5% across looks for one function
/// under a stationary Bernoulli model; it is not a VM-wide multiple-test bound.
pub(super) const JS_JIT_PROMOTION_LOOKS: [(u8, u8); 4] =
    [(32, 31), (64, 59), (128, 113), (255, 219)];
const JS_JIT_MAX_FUNCTIONS: usize = 8;
const JS_JIT_MAX_TRACKED_FUNCTIONS: usize = 256;
const JS_JIT_MAX_MAPPED_BYTES: usize = 64 * 1024;
/// Promote a Wasm function only after repeated interpreter entries. Wasm
/// signatures and bytecode are fixed, so this is a call-hotness gate.
pub(super) const WASM_JIT_PROMOTION_ENTRIES: u8 = 32;
const WASM_JIT_MAX_TIER_ROOTS: usize = 32;
const WASM_JIT_MAX_TIER_MAPPED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone)]
pub(super) enum JsTierState {
    Hot { observations: u8, numeric: u8 },
    /// A hot function whose promotion observation had a non-numeric argument;
    /// compile on its next numeric entry.
    PromotionPending,
    Unsupported,
    Compiled(Rc<super::compiled::CompiledJsFunction>),
}

#[derive(Default)]
struct WasmTierState {
    /// The high bit records a one-shot promotion attempt; low bits hold the
    /// number of interpreter entries observed before that attempt.
    hotness: u8,
    compiled: Option<Rc<super::compiled::CompiledProgram>>,
}

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
    /// Sparse, lazy JavaScript tiers. A program only pays for this map when
    /// JIT mode actually enters one of its functions.
    js_tiers: FxHashMap<u32, JsTierState>,
    /// Sparse hotness state and bounded root-closure artifacts for Wasm
    /// functions the eager baseline compiler could not lower.
    wasm_tiers: FxHashMap<u32, WasmTierState>,
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
    /// Its root values changed since a collection last traced them. A minor collection
    /// traces only such programs: what an earlier collection reached stays marked until
    /// the next full collection, which traces every program.
    roots_pending: bool,
}

pub(super) const ANONYMOUS_EVAL_ORIGIN: &str = "eval at <anonymous>";

#[derive(Clone)]
pub(super) enum CompilationOrigin {
    Source,
    /// Execution state in another realm of one logical host Script.
    SharedScript { source: ProgramId },
    Eval { caller: Option<JsString> },
    FunctionConstructor { caller: JsString },
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
    /// VM-local bounds for the optional JavaScript tier cache.
    js_jit_tracked_functions: usize,
    js_jit_compiled_functions: usize,
    js_jit_mapped_bytes: usize,
    wasm_jit_tier_roots: usize,
    wasm_jit_tier_mapped_bytes: usize,
    /// Allocated only when opt-in JavaScript JIT diagnostics are requested.
    js_jit_diagnostics: Option<FxHashMap<(u32, u32), JsJitCounters>>,
}

impl Default for ProgramStore {
    fn default() -> Self {
        Self {
            programs: Vec::new(),
            global_eval_programs: FxHashMap::default(),
            owned_scripts: FxHashMap::default(),
            reclaim_passes: 0,
            unchecked_reclaimable: 0,
            js_jit_tracked_functions: 0,
            js_jit_compiled_functions: 0,
            js_jit_mapped_bytes: 0,
            wasm_jit_tier_roots: 0,
            wasm_jit_tier_mapped_bytes: 0,
            js_jit_diagnostics: std::env::var_os("QUENCH_JS_JIT_STATS")
                .is_some_and(|value| value == "1")
                .then(FxHashMap::default),
        }
    }
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

    /// Every mutation goes through here, so it also marks the program's roots pending.
    fn entry_mut(&mut self, id: ProgramId) -> Option<&mut ProgramEntry> {
        let entry = self.programs.get_mut(id.index())?.as_mut()?;
        entry.roots_pending = true;
        Some(entry)
    }

    /// Inserts dynamic code that `reclaim_unreferenced` may later drop.
    pub(crate) fn insert_reclaimable_shared(&mut self, program: Rc<ResidualProgram>) -> Option<ProgramId> {
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
            || self.owned_scripts.values().any(|script| script.owner.strong_count() == 0)
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
            let live_instance = script.instances.iter().any(|(_, id)| {
                referenced.get(id.index()).copied().unwrap_or(false)
            });
            if (owned || live_instance) && let Some(mark) = referenced.get_mut(source.index()) { *mark = true; }
            if owned {
                for (_, id) in &script.instances {
                    if let Some(mark) = referenced.get_mut(id.index()) { *mark = true; }
                }
            }
        }
        let previous_pass = self.reclaim_passes;
        self.reclaim_passes += 1;
        let mut reclaimed = Vec::new();
        let mut reclaimed_tiers = (0usize, 0usize, 0usize);
        let mut reclaimed_wasm_tiers = (0usize, 0usize);
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
            reclaimed_tiers.0 += entry.js_tiers.len();
            for state in entry.js_tiers.values() {
                if let JsTierState::Compiled(compiled) = state {
                    reclaimed_tiers.1 += 1;
                    reclaimed_tiers.2 += compiled.mapped_bytes();
                }
            }
            reclaimed_wasm_tiers.1 += entry
                .wasm_tiers
                .values()
                .filter_map(|state| state.compiled.as_ref())
                .map(|compiled| compiled.mapped_bytes())
                .sum::<usize>();
            reclaimed_wasm_tiers.0 += entry
                .wasm_tiers
                .values()
                .filter(|state| state.compiled.is_some())
                .count();
            *slot = None;
            reclaimed.extend(ProgramId::from_index(index));
        }
        self.js_jit_tracked_functions = self
            .js_jit_tracked_functions
            .saturating_sub(reclaimed_tiers.0);
        self.js_jit_compiled_functions = self
            .js_jit_compiled_functions
            .saturating_sub(reclaimed_tiers.1);
        self.js_jit_mapped_bytes = self.js_jit_mapped_bytes.saturating_sub(reclaimed_tiers.2);
        self.wasm_jit_tier_roots = self
            .wasm_jit_tier_roots
            .saturating_sub(reclaimed_wasm_tiers.0);
        self.wasm_jit_tier_mapped_bytes = self
            .wasm_jit_tier_mapped_bytes
            .saturating_sub(reclaimed_wasm_tiers.1);
        self.unchecked_reclaimable = pending;
        if !reclaimed.is_empty() {
            let programs = &self.programs;
            self.global_eval_programs.retain(|_, id| programs.get(id.index()).is_some_and(Option::is_some));
            self.owned_scripts.retain(|source, _| programs.get(source.index()).is_some_and(Option::is_some));
        }
        reclaimed
    }

    pub(crate) fn reset(&mut self, main: Rc<ResidualProgram>) -> ProgramId {
        self.global_eval_programs.clear();
        self.owned_scripts.clear();
        self.programs.clear();
        self.unchecked_reclaimable = 0;
        self.js_jit_tracked_functions = 0;
        self.js_jit_compiled_functions = 0;
        self.js_jit_mapped_bytes = 0;
        self.wasm_jit_tier_roots = 0;
        self.wasm_jit_tier_mapped_bytes = 0;
        if let Some(diagnostics) = &mut self.js_jit_diagnostics {
            diagnostics.clear();
        }
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
        self.owned_scripts.insert(script.source, OwnedScript { owner: Rc::downgrade(script), instances: Vec::new() });
    }

    pub(super) fn owns_script(&self, script: &Rc<crate::api::CompiledScriptState>) -> bool {
        self.owned_scripts.get(&script.source).is_some_and(|entry| {
            std::ptr::eq(entry.owner.as_ptr(), Rc::as_ptr(script))
        })
    }

    pub(super) fn owned_script_instance(&self, source: ProgramId, realm: WeakHandle) -> Option<ProgramId> {
        self.owned_scripts.get(&source)?.instances.iter()
            .find_map(|(owner, id)| (*owner == realm).then_some(*id))
    }

    pub(super) fn bind_script_instance(&mut self, source: ProgramId, realm: WeakHandle, id: ProgramId) {
        self.set_origin(id, CompilationOrigin::SharedScript { source });
        self.owned_scripts.get_mut(&source).expect("registered Script source")
            .instances.push((realm, id));
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

    /// Clear call-site caches holding non-owning heap identities before swept
    /// slots can be reused for a different function Value.
    pub(super) fn clear_wasm_reference_caches(&self) {
        for entry in self.programs.iter().flatten() {
            if let Some(Some(compiled)) = &entry.compiled {
                compiled.clear_indirect_call_caches();
            }
            for state in entry.wasm_tiers.values() {
                if let Some(compiled) = &state.compiled {
                    compiled.clear_indirect_call_caches();
                }
            }
        }
    }

    pub(super) fn set_origin(&mut self, id: ProgramId, origin: CompilationOrigin) {
        if let Some(entry) = self.entry_mut(id) { entry.origin = origin; }
    }

    pub(super) fn eval_origin(&self, id: ProgramId) -> Option<JsString> {
        match &self.entry(id)?.origin {
            CompilationOrigin::Source | CompilationOrigin::SharedScript { .. } => None,
            CompilationOrigin::Eval { caller } => Some(caller.clone()
                .unwrap_or_else(|| JsString::from_str(ANONYMOUS_EVAL_ORIGIN))),
            CompilationOrigin::FunctionConstructor { caller } => Some(caller.clone()),
        }
    }

    pub(super) fn is_function_constructor_body(&self, id: ProgramId, function: u32) -> bool {
        self.entry(id).is_some_and(|entry| {
            matches!(entry.origin, CompilationOrigin::FunctionConstructor { .. })
                && entry.residual.functions.get(function as usize)
                    .is_some_and(|function| function.header().parent == Some(super::ROOT_FUNCTION_ID))
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
        let origin = if program.kind == crate::bytecode::ProgramKind::Eval { CompilationOrigin::Eval { caller: None } } else { CompilationOrigin::Source };
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
            js_tiers: FxHashMap::default(),
            wasm_tiers: FxHashMap::default(),
            scope: ProgramScope::Own,
            origin,
            script_or_module: None,
            reclaimable: false,
            pass: self.reclaim_passes,
            roots_pending: true,
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
    /// global lexical (`ResidualProgram::root_slot_is_binding`) or module-import projection
    /// reads through it.
    pub(crate) fn root_slot_is_binding(
        &self,
        id: ProgramId,
        slot: u16,
        lexical_slots_are_bindings: bool,
    ) -> bool {
        self.entry(id).is_some_and(|entry| {
            entry.residual.root_slot_is_binding(slot, lexical_slots_are_bindings)
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

    /// Every live program's compiled form.
    #[cfg(feature = "profile-memory")]
    pub(crate) fn residuals(&self) -> impl Iterator<Item = &ResidualProgram> {
        self.programs.iter().flatten().map(|entry| &*entry.residual)
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

    pub(super) fn has_compiled_function_or_note_fallback(
        &self,
        id: ProgramId,
        function: usize,
    ) -> bool {
        self.entry(id)
            .and_then(|entry| entry.compiled.as_ref()?.as_ref())
            .is_some_and(|program| program.has_function_or_note_fallback(function))
    }

    /// Whether an already retained selective-tier artifact contains this
    /// function. This check is intentionally borrow-only so lane dispatches
    /// do not clone its `Rc` on every call.
    pub(super) fn has_wasm_tier_containing(&self, id: ProgramId, function: usize) -> bool {
        let Some(tiers) = self.entry(id).map(|entry| &entry.wasm_tiers) else {
            return false;
        };
        tiers
            .get(&(function as u32))
            .is_some_and(|state| state.compiled.is_some())
            || tiers.values().any(|state| {
                state
                    .compiled
                    .as_ref()
                    .is_some_and(|compiled| compiled.has_function(function))
            })
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

    /// Record another interpreter entry and return true exactly once when a
    /// cold unsupported root reaches the threshold. Terminal attempt state
    /// prevents repeated compile failures from turning hot calls into work.
    pub(super) fn note_wasm_tier_entry(&mut self, id: ProgramId, function: u32) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let state = entry.wasm_tiers.entry(function).or_default();
        if state.compiled.is_some() {
            return false;
        }
        if state.hotness & 0x80 != 0 {
            return false;
        }
        let count = (state.hotness & 0x7f).saturating_add(1);
        state.hotness = count;
        if count < WASM_JIT_PROMOTION_ENTRIES {
            return false;
        }
        state.hotness |= 0x80;
        true
    }

    /// Count a nonterminal `CallKnown` activation that stays in the fast
    /// interpreter lane. On the threshold call, leave the count unchanged and
    /// hand it to the general dispatcher, whose normal entry hook records the
    /// final observation and claims the one-shot compile attempt.
    pub(super) fn note_wasm_tier_lane_entry(&mut self, id: ProgramId, function: u32) -> bool {
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        if !entry
            .residual
            .functions
            .get(function as usize)
            .is_some_and(|function| {
                function.code.len() <= super::compiled::MAX_JIT_TIER_BYTECODES_PER_FUNCTION
            })
        {
            return false;
        }
        let state = entry.wasm_tiers.entry(function).or_default();
        if state.compiled.is_some() || state.hotness & 0x80 != 0 {
            return false;
        }
        let count = state.hotness & 0x7f;
        if count.saturating_add(1) >= WASM_JIT_PROMOTION_ENTRIES {
            return true;
        }
        state.hotness = count.saturating_add(1);
        false
    }

    pub(super) fn wasm_tier(
        &self,
        id: ProgramId,
        function: u32,
    ) -> Option<Rc<super::compiled::CompiledProgram>> {
        self.entry(id)?
            .wasm_tiers
            .get(&function)?
            .compiled
            .clone()
    }

    /// Find a root artifact whose static closure includes this function.
    pub(super) fn wasm_tier_containing(
        &self,
        id: ProgramId,
        function: u32,
    ) -> Option<Rc<super::compiled::CompiledProgram>> {
        let tiers = &self.entry(id)?.wasm_tiers;
        tiers
            .get(&function)
            .and_then(|state| state.compiled.clone())
            .or_else(|| {
                tiers
                    .values()
                    .filter_map(|state| state.compiled.as_ref())
                    .find(|compiled| compiled.has_function(function as usize))
                    .cloned()
            })
    }

    /// Find a selective tier with the requested function in the currently
    /// executing artifact. Indirect-call promotion can place both caller and
    /// profile-selected target in one closure; prefer that closure so the
    /// native call does not bounce through the baseline artifact.
    pub(super) fn wasm_tier_for_artifact_containing(
        &self,
        id: ProgramId,
        function: usize,
        artifact: *const super::compiled::CompiledProgram,
    ) -> Option<Rc<super::compiled::CompiledProgram>> {
        self.entry(id)?
            .wasm_tiers
            .values()
            .filter_map(|state| state.compiled.as_ref())
            .find(|compiled| {
                std::ptr::eq(Rc::as_ptr(compiled), artifact) && compiled.has_function(function)
            })
            .cloned()
    }

    /// Retain one compiled root closure if the VM-wide tier budget permits
    /// it. Active calls retain their own Rc through native return.
    pub(super) fn install_wasm_tier(
        &mut self,
        id: ProgramId,
        root: u32,
        compiled: Rc<super::compiled::CompiledProgram>,
    ) -> bool {
        if !compiled.has_function(root as usize) {
            return false;
        }
        if self
            .entry(id)
            .and_then(|entry| entry.wasm_tiers.get(&root))
            .is_some_and(|state| state.compiled.is_some())
        {
            return true;
        }
        let bytes = compiled.mapped_bytes();
        if self.wasm_jit_tier_roots >= WASM_JIT_MAX_TIER_ROOTS
            || self
                .wasm_jit_tier_mapped_bytes
                .saturating_add(bytes)
                > WASM_JIT_MAX_TIER_MAPPED_BYTES
        {
            return false;
        }
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        entry.wasm_tiers.entry(root).or_default().compiled = Some(compiled);
        self.wasm_jit_tier_roots += 1;
        self.wasm_jit_tier_mapped_bytes += bytes;
        true
    }

    #[cfg(test)]
    pub(super) fn wasm_tier_entry_count(&self, id: ProgramId, function: u32) -> Option<u8> {
        self.entry(id)
            .and_then(|entry| entry.wasm_tiers.get(&function))
            .map(|state| state.hotness & 0x7f)
    }

    #[cfg(test)]
    pub(super) fn wasm_tier_attempted(&self, id: ProgramId, function: u32) -> Option<bool> {
        self.entry(id)
            .and_then(|entry| entry.wasm_tiers.get(&function))
            .map(|state| state.hotness & 0x80 != 0)
    }

    pub(super) fn js_tier(&self, id: ProgramId, function: u32) -> Option<JsTierState> {
        self.entry(id)?.js_tiers.get(&function).cloned()
    }

    pub(super) fn js_jit_diagnostics_enabled(&self) -> bool {
        self.js_jit_diagnostics.is_some()
    }

    /// Record the input guard outcome. Promotion checks only at prespecified
    /// sample sizes; exact binomial cutoffs spend alpha across those looks.
    pub(super) fn note_js_function_entry(
        &mut self,
        id: ProgramId,
        function: u32,
        numeric: bool,
    ) -> bool {
        let compiled_functions = self.js_jit_compiled_functions;
        let can_track = self.js_jit_tracked_functions < JS_JIT_MAX_TRACKED_FUNCTIONS;
        let mut inserted = false;
        let mut promoted = false;
        let Some(entry) = self.entry_mut(id) else {
            return false;
        };
        let should_compile = match entry.js_tiers.entry(function) {
            std::collections::hash_map::Entry::Vacant(slot) => {
                if !can_track {
                    return false;
                }
                inserted = true;
                if compiled_functions >= JS_JIT_MAX_FUNCTIONS {
                    slot.insert(JsTierState::Unsupported);
                } else {
                    slot.insert(JsTierState::Hot {
                        observations: 1,
                        numeric: u8::from(numeric),
                    });
                }
                false
            }
            std::collections::hash_map::Entry::Occupied(mut slot) => match slot.get_mut() {
                JsTierState::Hot {
                    observations,
                    numeric: numeric_entries,
                } => {
                    *observations = observations.saturating_add(1);
                    *numeric_entries = numeric_entries.saturating_add(u8::from(numeric));
                    let decision = numeric_promotion_decision(*numeric_entries, *observations);
                    if compiled_functions >= JS_JIT_MAX_FUNCTIONS {
                        *slot.get_mut() = JsTierState::Unsupported;
                        false
                    } else if decision == Some(true) {
                        promoted = true;
                        if numeric {
                            true
                        } else {
                            *slot.get_mut() = JsTierState::PromotionPending;
                            false
                        }
                    } else {
                        if decision == Some(false) && *observations == u8::MAX {
                            *slot.get_mut() = JsTierState::Unsupported;
                        }
                        false
                    }
                }
                JsTierState::PromotionPending => {
                    if compiled_functions >= JS_JIT_MAX_FUNCTIONS {
                        *slot.get_mut() = JsTierState::Unsupported;
                        false
                    } else {
                        numeric
                    }
                }
                JsTierState::Unsupported | JsTierState::Compiled(_) => false,
            },
        };
        if inserted {
            self.js_jit_tracked_functions += 1;
        }
        if promoted {
            self.record_js_jit_promotion(id, function);
        }
        should_compile
    }

    pub(super) fn record_js_jit_entry(&mut self, id: ProgramId, function: u32, numeric: bool) {
        let key = (id.raw(), function);
        let Some(diagnostics) = self.js_jit_diagnostics.as_ref() else {
            return;
        };
        if diagnostics.contains_key(&key) {
            if let Some(counters) = self
                .js_jit_diagnostics
                .as_mut()
                .and_then(|diagnostics| diagnostics.get_mut(&key))
            {
                counters.entries = counters.entries.saturating_add(1);
                counters.numeric_entries = counters
                    .numeric_entries
                    .saturating_add(u64::from(numeric));
            }
            return;
        }
        if diagnostics.len() >= JS_JIT_MAX_TRACKED_FUNCTIONS {
            return;
        }
        let (source_name, function_name) = self
            .entry(id)
            .map(|entry| {
                let function_name = entry
                    .residual
                    .functions
                    .get(function as usize)
                    .and_then(|function| function.name.or(function.inferred_name))
                    .and_then(|atom| {
                        let index = atom as usize;
                        (index < entry.residual.atoms.len())
                            .then(|| entry.residual.atoms[index].to_owned())
                    })
                    .unwrap_or_default();
                (entry.residual.source_name.clone(), function_name)
            })
            .unwrap_or_default();
        if let Some(diagnostics) = self.js_jit_diagnostics.as_mut() {
            let counters = diagnostics.entry(key).or_default();
            counters.source_name = Some(source_name.into_boxed_str());
            counters.function_name = Some(function_name.into_boxed_str());
            counters.entries = counters.entries.saturating_add(1);
            counters.numeric_entries = counters
                .numeric_entries
                .saturating_add(u64::from(numeric));
        }
    }

    fn record_js_jit_promotion(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.promotions = counters.promotions.saturating_add(1);
        }
    }

    pub(super) fn record_js_jit_compile(
        &mut self,
        id: ProgramId,
        function: u32,
        elapsed_ns: u64,
        code_bytes: usize,
        installed: bool,
    ) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.compile_attempts = counters.compile_attempts.saturating_add(1);
            counters.compile_successes = counters
                .compile_successes
                .saturating_add(u64::from(installed));
            counters.compile_ns = counters.compile_ns.saturating_add(elapsed_ns);
            counters.code_bytes = counters.code_bytes.saturating_add(code_bytes);
        }
    }

    pub(super) fn record_js_jit_native_entry(&mut self, id: ProgramId, function: u32) {
        if let Some(counters) = self
            .js_jit_diagnostics
            .as_mut()
            .and_then(|diagnostics| diagnostics.get_mut(&(id.raw(), function)))
        {
            counters.native_entries = counters.native_entries.saturating_add(1);
        }
    }

    pub(super) fn install_js_tier(
        &mut self,
        id: ProgramId,
        function: u32,
        compiled: Option<Rc<super::compiled::CompiledJsFunction>>,
    ) -> bool {
        if self.entry(id).is_none() {
            return false;
        }
        let compiled = compiled.filter(|compiled| {
            self.js_jit_compiled_functions < JS_JIT_MAX_FUNCTIONS
                && self
                    .js_jit_mapped_bytes
                    .saturating_add(compiled.mapped_bytes())
                    <= JS_JIT_MAX_MAPPED_BYTES
        });
        if let Some(compiled) = &compiled {
            self.js_jit_compiled_functions += 1;
            self.js_jit_mapped_bytes += compiled.mapped_bytes();
        }
        if let Some(entry) = self.entry_mut(id) {
            entry.js_tiers.insert(
                function,
                compiled.map_or(JsTierState::Unsupported, JsTierState::Compiled),
            );
        }
        self.js_tier(id, function)
            .is_some_and(|tier| matches!(tier, JsTierState::Compiled(_)))
    }

    #[cfg(test)]
    pub(super) fn js_jit_tracked_function_count(&self) -> usize {
        self.js_jit_tracked_functions
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

    /// Appends the values the stored programs keep alive to `roots`: every program's for a
    /// full collection, only those of programs whose roots changed since the last
    /// collection for a minor one.
    pub(crate) fn extend_roots(&mut self, roots: &mut Vec<Value>, full: bool) {
        for entry in self.programs.iter_mut().flatten() {
            if !full && !entry.roots_pending {
                continue;
            }
            entry.roots_pending = false;
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

impl Drop for ProgramStore {
    fn drop(&mut self) {
        let Some(diagnostics) = &self.js_jit_diagnostics else {
            return;
        };
        let mut records: Vec<_> = diagnostics.iter().collect();
        records.sort_unstable_by_key(|(key, _)| **key);
        let stderr = std::io::stderr();
        let mut stderr = stderr.lock();
        for ((program, function), counters) in records {
            let entry = self
                .programs
                .get(*program as usize)
                .and_then(Option::as_ref);
            let status = entry
                .and_then(|entry| entry.js_tiers.get(function))
                .map(|tier| match tier {
                    JsTierState::Hot { .. } => "hot",
                    JsTierState::PromotionPending => "promotion-pending",
                    JsTierState::Unsupported => "unsupported",
                    JsTierState::Compiled(_) => "compiled",
                })
                .unwrap_or("reclaimed");
            let function_name = entry
                .and_then(|entry| entry.residual.functions.get(*function as usize).map(|f| (entry, f)))
                .and_then(|(entry, function)| {
                    let header = function.header();
                    let atom = header.name.or(header.inferred_name)? as usize;
                    (atom < entry.residual.atoms.len()).then(|| &entry.residual.atoms[atom])
                })
                .or_else(|| counters.function_name.as_deref())
                .unwrap_or("");
            let source_name = entry
                .map(|entry| entry.residual.source_name.as_str())
                .or_else(|| counters.source_name.as_deref())
                .unwrap_or("");
            let _ = write!(
                stderr,
                "QUENCH_JS_JIT_STATS {{\"program\":{program},\"function\":{function},\"function_name\":"
            );
            let _ = write_json_string(&mut stderr, function_name);
            let _ = write!(stderr, ",\"source_name\":");
            let _ = write_json_string(&mut stderr, source_name);
            let _ = writeln!(
                stderr,
                ",\"status\":\"{status}\",\"entries\":{},\"numeric_entries\":{},\"promotions\":{},\"compile_attempts\":{},\"compile_successes\":{},\"compile_ns\":{},\"native_entries\":{},\"code_bytes\":{}}}",
                counters.entries,
                counters.numeric_entries,
                counters.promotions,
                counters.compile_attempts,
                counters.compile_successes,
                counters.compile_ns,
                counters.native_entries,
                counters.code_bytes,
            );
        }
    }
}

fn write_json_string(output: &mut impl std::io::Write, value: &str) -> std::io::Result<()> {
    write!(output, "\"")?;
    for character in value.chars() {
        match character {
            '"' => write!(output, "\\\"")?,
            '\\' => write!(output, "\\\\")?,
            '\n' => write!(output, "\\n")?,
            '\r' => write!(output, "\\r")?,
            '\t' => write!(output, "\\t")?,
            character if character <= '\u{1f}' => {
                write!(output, "\\u{:04x}", character as u32)?
            }
            character => write!(output, "{character}")?,
        }
    }
    write!(output, "\"")
}

fn numeric_promotion_decision(numeric: u8, observations: u8) -> Option<bool> {
    let minimum = JS_JIT_PROMOTION_LOOKS
        .iter()
        .find_map(|(look, minimum)| (*look == observations).then_some(*minimum))?;
    Some(numeric >= minimum)
}

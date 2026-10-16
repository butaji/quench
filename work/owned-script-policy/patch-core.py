from pathlib import Path
r=Path('/workspace/quench/work/prepared-realm-worktree')
def edit(n,a,b):
 p=r/n;s=p.read_text();assert a in s,(n,a[:100]);p.write_text(s.replace(a,b))
edit('crates/quench-runtime/src/api_native.rs','/// Opaque index into the embedding\'s stable native-operation table.','''/// A logical host Script with one source identity and separate execution caches
/// in each realm. The handle owns code, not guest objects or realm globals.
#[derive(Clone)]
pub struct CompiledScript {
    pub(crate) state: std::rc::Rc<CompiledScriptState>,
}

pub(crate) struct CompiledScriptState {
    pub(crate) program: std::rc::Rc<crate::ResidualProgram>,
    pub(crate) source: crate::vm::ProgramId,
}

/// Opaque index into the embedding's stable native-operation table.''')
edit('crates/quench-runtime/src/api.rs','    NativeContext, RootedIterator, ErrorSourcePosition, SourceStackFrame, SynchronousModule,','    CompiledScript, NativeContext, RootedIterator, ErrorSourcePosition, SourceStackFrame, SynchronousModule,')
edit('crates/quench-runtime/src/api.rs','pub use native::{','pub(crate) use native::CompiledScriptState;\npub use native::{')
edit('crates/quench-runtime/src/lib.rs','    RuntimeError, SourceKind, ErrorSourcePosition, SourceStackFrame, SynchronousModule,','    CompiledScript, RuntimeError, SourceKind, ErrorSourcePosition, SourceStackFrame, SynchronousModule,')
edit('crates/quench-runtime/src/api_native.rs','    pub fn prepared_script_is_current(&self, prepared: &PreparedScript) -> bool {','''    /// Compile one logical Script. Its source identity persists across runs;
    /// realm-bound execution state is allocated independently on first use.
    pub fn compile_script_in_realm_with_origin(
        &mut self, global: RootId, source: &str, name: &str,
        line_offset: u32, column_offset: u32,
    ) -> Result<CompiledScript, RootedError> {
        self.vm.compile_embedding_script_in_realm(
            global, source, name, (line_offset, column_offset),
        ).map_err(|error| self.error(error))
    }

    pub fn compiled_script_is_current(&self, script: &CompiledScript) -> bool {
        self.vm.compiled_script_is_current(script)
    }

    pub fn evaluate_compiled_script_in_realm_rooted(
        &mut self, global: RootId, script: &CompiledScript,
    ) -> Result<RootId, RootedError> {
        let result = self.vm.evaluate_embedding_compiled_script_in_realm(global, script);
        self.completion(result)
    }

    pub fn prepared_script_is_current(&self, prepared: &PreparedScript) -> bool {''')
edit('crates/quench-runtime/src/vm/program_store.rs','use std::rc::Rc;','use std::rc::{Rc, Weak};\nuse crate::heap::WeakHandle;')
edit('crates/quench-runtime/src/vm/program_store.rs','    Source,\n    Eval', '''    Source,
    /// Execution state in another realm of one logical host Script.
    SharedScript { source: ProgramId },
    Eval''')
edit('crates/quench-runtime/src/vm/program_store.rs','#[derive(Default)]\npub(crate) struct ProgramStore {','''struct OwnedScript {
    owner: Weak<crate::api::CompiledScriptState>,
    instances: Vec<(WeakHandle, ProgramId)>,
}

#[derive(Default)]
pub(crate) struct ProgramStore {''')
edit('crates/quench-runtime/src/vm/program_store.rs','    global_eval_programs: FxHashMap<JsString, ProgramId>,','    global_eval_programs: FxHashMap<JsString, ProgramId>,\n    owned_scripts: FxHashMap<ProgramId, OwnedScript>,')
edit('crates/quench-runtime/src/vm/program_store.rs','        self.unchecked_reclaimable > 0','''        self.unchecked_reclaimable > 0
            || self.owned_scripts.values().any(|script| script.owner.strong_count() == 0)''')
edit('crates/quench-runtime/src/vm/program_store.rs','    pub(crate) fn reclaim_unreferenced(&mut self, referenced: &[bool]) -> Vec<ProgramId> {','''    pub(crate) fn reclaim_unreferenced(&mut self, mut referenced: Vec<bool>) -> Vec<ProgramId> {
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
        }''')
edit('crates/quench-runtime/src/vm/program_store.rs','            self.global_eval_programs.retain(|_, id| programs.get(id.index()).is_some_and(Option::is_some));','''            self.global_eval_programs.retain(|_, id| programs.get(id.index()).is_some_and(Option::is_some));
            self.owned_scripts.retain(|source, _| programs.get(source.index()).is_some_and(Option::is_some));''')
edit('crates/quench-runtime/src/vm/program_store.rs','        self.global_eval_programs.clear();','        self.global_eval_programs.clear();\n        self.owned_scripts.clear();')
edit('crates/quench-runtime/src/vm/program_store.rs','            CompilationOrigin::Source => None,','            CompilationOrigin::Source | CompilationOrigin::SharedScript { .. } => None,')
edit('crates/quench-runtime/src/vm/program_store.rs','    pub(super) fn set_origin(&mut self, id: ProgramId, origin: CompilationOrigin) {','''    pub(super) fn register_owned_script(&mut self, script: &Rc<crate::api::CompiledScriptState>) {
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

    pub(super) fn set_origin(&mut self, id: ProgramId, origin: CompilationOrigin) {''')
edit('crates/quench-runtime/src/vm/gc.rs','self.programs.reclaim_unreferenced(&referenced)','self.programs.reclaim_unreferenced(referenced)')
edit('crates/quench-runtime/src/vm/gc.rs','        self.prune_function_values();','''        let heap = &self.heap;
        self.programs.prune_owned_script_realms(|realm| heap.weak_value(realm).is_some());
        self.prune_function_values();''')
edit('crates/quench-runtime/src/vm/embedding.rs','script_id: u64::from(program.raw()) + 1,','script_id: u64::from(self.programs.source_identity(program).raw()) + 1,')
edit('crates/quench-runtime/src/vm/embedding.rs','    fn with_embedding_realm<T>(','''    pub(crate) fn compile_embedding_script_in_realm(
        &mut self, global: RootId, source: &str, name: &str, origin: (u32, u32),
    ) -> Result<crate::CompiledScript, JsError> {
        self.with_embedding_realm(global, |vm| vm.compile_embedding_host_script(source, name, origin))
    }

    pub(crate) fn evaluate_embedding_compiled_script_in_realm(
        &mut self, global: RootId, script: &crate::CompiledScript,
    ) -> Result<Value, JsError> {
        self.with_embedding_realm(global, |vm| vm.evaluate_embedding_compiled_script(script))
    }

    fn with_embedding_realm<T>(''')
edit('crates/quench-runtime/src/vm/eval.rs','    pub(crate) fn prepared_script_is_current(&self, prepared: &crate::PreparedScript) -> bool {','''    pub(crate) fn compile_embedding_host_script(
        &mut self, source: &str, name: &str, origin: (u32, u32),
    ) -> Result<crate::CompiledScript, JsError> {
        let prepared = self.prepare_embedding_specialized_script(source, name, Some(origin))?;
        let context = self.embedding_program()?;
        let Some(source) = self.programs.insert_reclaimable_shared(prepared.program.clone()) else {
            return Err(self.type_error(&context, "dynamic program store is full".into()));
        };
        let state = Rc::new(crate::api::CompiledScriptState { program: prepared.program, source });
        self.programs.register_owned_script(&state);
        Ok(crate::CompiledScript { state })
    }

    pub(crate) fn compiled_script_is_current(&self, script: &crate::CompiledScript) -> bool {
        script.state.program.atoms.belongs_to(&self.atom_sequence)
            && self.programs.owns_script(&script.state)
    }

    pub(crate) fn evaluate_embedding_compiled_script(
        &mut self, script: &crate::CompiledScript,
    ) -> Result<Value, JsError> {
        let context = self.embedding_program()?;
        if !self.compiled_script_is_current(script) {
            return Err(self.type_error(&context, "compiled Script belongs to a different runtime state".into()));
        }
        let realm = self.heap.weak_handle(self.realm.globals).expect("live realm global");
        let id = match self.programs.owned_script_instance(script.state.source, realm) {
            Some(id) => id,
            None => {
                let Some(id) = self.store_dynamic_program_shared(script.state.program.clone()) else {
                    return Err(self.type_error(&context, "dynamic program store is full".into()));
                };
                self.programs.bind_script_instance(script.state.source, realm, id);
                id
            }
        };
        self.run_global_eval_program(&context, id)
    }

    pub(crate) fn prepared_script_is_current(&self, prepared: &crate::PreparedScript) -> bool {''')

use super::*;

impl Vm {
    pub(super) fn cached_functions(
        &self,
        program: ProgramId,
        id: u32,
    ) -> impl DoubleEndedIterator<Item = Value> + '_ {
        self.function_values
            .get(&(program, id))
            .into_iter()
            .flatten()
            .filter_map(|handle| self.heap.weak_value(*handle))
    }

    pub(super) fn cached_functions_in_environment(
        &self,
        program: ProgramId,
        id: u32,
        environment: Value,
    ) -> impl DoubleEndedIterator<Item = Value> + '_ {
        self.cached_functions(program, id).filter(move |function| {
            matches!(self.heap.function(*function), Some(FunctionRecord { env, .. }) if *env == environment)
        })
    }

    /// Drops the cache's handles a collection freed, before their slots are reused. A minor
    /// collection frees only young cells, so it visits only the entries that hold one.
    pub(super) fn prune_function_values(&mut self, full: bool) {
        let heap = &self.heap;
        let FunctionValues { entries, young } = &mut self.function_values;
        let live = |entry: &mut Handles| {
            entry.retain(|handle| heap.weak_value(*handle).is_some());
            !entry.is_empty()
        };
        if full {
            entries.retain(|_, entry| live(entry));
            young.clear();
            return;
        }
        young.sort_unstable_by_key(|(program, id)| (program.raw(), *id));
        young.dedup();
        young.retain(|key| {
            let Some(entry) = entries.get_mut(key) else { return false };
            if !live(entry) {
                entries.remove(key);
                return false;
            }
            entry.iter().any(|handle| heap.weak_value(*handle).is_some_and(|value| !heap.is_old(value)))
        });
    }
}

/// A function's closures: nearly every function makes one (a Wasm module's table entries,
/// 20K on dotnet-aot-wasm), held in place without an allocation of its own.
pub(super) type Handles = smallvec::SmallVec<[WeakHandle; 1]>;

/// The closures each top-level function made, by weak handle, and the keys whose closures
/// may still be young: a minor collection prunes only those.
#[derive(Default)]
pub(super) struct FunctionValues {
    entries: FxHashMap<(ProgramId, u32), Handles>,
    young: Vec<(ProgramId, u32)>,
}

impl FunctionValues {
    /// Records `handles`, new closures of `key`, which are young until a collection promotes them.
    pub(super) fn record(&mut self, key: (ProgramId, u32), handles: impl IntoIterator<Item = WeakHandle>) {
        self.entries.entry(key).or_default().extend(handles);
        self.young.push(key);
    }
}

/// The hoisted function declarations of each module instance (one `ProgramId`), by function id.
/// `Vm::instantiate_module_environment` makes each one, and the module body's own declaration of
/// it (`Op::MakeClosure` at the root) finds it here. Weak, as `FunctionValues` holds closures: the
/// module environment's binding keeps each one alive while it holds it.
#[derive(Default)]
pub(super) struct ModuleFunctions {
    instances: FxHashMap<ProgramId, FxHashMap<u32, WeakHandle>>,
}

impl ModuleFunctions {
    pub(super) fn get(&self, program: ProgramId, id: u32) -> Option<WeakHandle> {
        self.instances.get(&program)?.get(&id).copied()
    }

    pub(super) fn contains(&self, program: ProgramId, id: u32) -> bool {
        self.instances.get(&program).is_some_and(|functions| functions.contains_key(&id))
    }

    pub(super) fn set(&mut self, program: ProgramId, id: u32, handle: WeakHandle) {
        self.instances.entry(program).or_default().insert(id, handle);
    }

    /// Every recorded function of every instance, by its handle.
    pub(super) fn iter(&self) -> impl Iterator<Item = (ProgramId, u32, WeakHandle)> + '_ {
        self.instances
            .iter()
            .flat_map(|(program, functions)| functions.iter().map(move |(id, handle)| (*program, *id, *handle)))
    }

    /// Keeps the functions `keep` accepts, and drops an instance left with none.
    pub(super) fn retain(&mut self, mut keep: impl FnMut(WeakHandle) -> bool) {
        self.instances.retain(|_, functions| {
            functions.retain(|_, handle| keep(*handle));
            !functions.is_empty()
        });
    }

    /// Drops the instances of programs the VM reclaimed.
    pub(super) fn drop_programs(&mut self, reclaimed: &FxHashSet<ProgramId>) {
        self.instances.retain(|program, _| !reclaimed.contains(program));
    }

    pub(super) fn clear(&mut self) {
        self.instances.clear();
    }
}

impl Vm {
    /// The hoisted function `id` of module instance `program`, if its instance made one that is
    /// still live.
    pub(super) fn module_function(&self, program: ProgramId, id: u32) -> Option<Value> {
        self.module_functions.get(program, id).and_then(|handle| self.heap.weak_value(handle))
    }

    /// Whether module instance `program` made hoisted function `id` at instantiation.
    pub(super) fn is_module_function(&self, program: ProgramId, id: u32) -> bool {
        self.module_functions.contains(program, id)
    }

    /// Whether `Op::MakeClosure` of function `id` in frame `frame` is a module's hoisted
    /// declaration, which the frame's instance made at instantiation.
    pub(super) fn is_module_root_hoisted(&self, p: &ResidualProgram, frame: usize, id: u32) -> bool {
        p.is_module()
            && self.frames[frame].function == crate::vm::ROOT_FUNCTION_ID
            && self.is_module_function(self.frames[frame].program, id)
    }

    /// Records `function`, the hoisted function `id` that module instance `program` made.
    pub(super) fn record_module_function(&mut self, program: ProgramId, id: u32, function: Value) {
        let handle = self.heap.weak_handle(function).expect("new closure is live");
        self.module_functions.set(program, id, handle);
    }

    /// Drops the module functions' handles a collection freed, before their slots are reused.
    pub(super) fn prune_module_functions(&mut self) {
        let heap = &self.heap;
        self.module_functions.retain(|handle| heap.weak_value(handle).is_some());
    }
}

impl std::ops::Deref for FunctionValues {
    type Target = FxHashMap<(ProgramId, u32), Handles>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl std::ops::DerefMut for FunctionValues {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

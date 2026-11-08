use super::*;

impl<H: Host> Vm<H> {
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
        let live = |entry: &mut Vec<WeakHandle>| {
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

/// The closures each top-level function made, by weak handle, and the keys whose closures
/// may still be young: a minor collection prunes only those.
#[derive(Default)]
pub(super) struct FunctionValues {
    entries: FxHashMap<(ProgramId, u32), Vec<WeakHandle>>,
    young: Vec<(ProgramId, u32)>,
}

impl FunctionValues {
    /// Records `handles`, new closures of `key`, which are young until a collection promotes them.
    pub(super) fn record(&mut self, key: (ProgramId, u32), handles: impl IntoIterator<Item = WeakHandle>) {
        self.entries.entry(key).or_default().extend(handles);
        self.young.push(key);
    }
}

impl std::ops::Deref for FunctionValues {
    type Target = FxHashMap<(ProgramId, u32), Vec<WeakHandle>>;
    fn deref(&self) -> &Self::Target {
        &self.entries
    }
}

impl std::ops::DerefMut for FunctionValues {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.entries
    }
}

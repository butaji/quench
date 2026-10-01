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
            matches!(self.heap.get(*function), Some(Cell::Function { env, .. }) if *env == environment)
        })
    }

    pub(super) fn prune_function_values(&mut self) {
        let heap = &self.heap;
        self.function_values.retain(|_, entries| {
            entries.retain(|handle| heap.weak_value(*handle).is_some());
            !entries.is_empty()
        });
    }
}

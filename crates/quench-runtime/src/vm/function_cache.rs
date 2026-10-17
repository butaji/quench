use super::*;

impl<H: Host> Vm<H> {
    /// Whether closures of `function` join `function_values`, the identity cache its readers
    /// search: the closures a module root makes, which its `MakeClosure` reuses and reading
    /// its exports finds, and a Wasm program's functions, which its tables reuse. Each is a
    /// root's direct child or a Wasm program's.
    pub(super) fn caches_function_identity(
        p: &ResidualProgram,
        function: &crate::bytecode::Function,
    ) -> bool {
        p.kind == crate::bytecode::ProgramKind::Wasm
            || function.parent.is_none_or(|parent| parent == super::ROOT_FUNCTION_ID)
    }

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

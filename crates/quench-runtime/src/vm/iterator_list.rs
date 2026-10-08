use super::*;

impl<H: Host> Vm<H> {
    // On success the caller owns the value roots; on error this operation releases them.
    pub(super) fn rooted_iterator_list(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        method: Value,
    ) -> Result<Vec<RootId>, JsError> {
        let iterator = self.call_value(p, method, source, &[])?;
        if !self.is_object_like(iterator) {
            return Err(self.type_error(p, "iterator method did not return an object".into()));
        }
        self.rooted_iterator_values(p, iterator)
    }

    pub(super) fn rooted_iterator_values(
        &mut self,
        p: &ResidualProgram,
        iterator: Value,
    ) -> Result<Vec<RootId>, JsError> {
        let iterator_root = self.heap.root(iterator);
        let mut next = None;
        let mut values = Vec::new();
        let outcome = (|| {
            let value = self.heap.root_value(iterator_root).unwrap();
            let next_atom = self.intern_atom("next");
            let value = self.get_property(p, value, next_atom)?;
            let next_root = self.heap.root(value);
            next = Some(next_root);
            loop {
                let outcome = self.rooted_iterator_step_value(p, iterator_root, next_root);
                match outcome? {
                    Some(value) => values.push(self.heap.root(value)),
                    None => return Ok(()),
                }
            }
        })();
        for root in [Some(iterator_root), next].into_iter().flatten() {
            self.heap.release_root(root);
        }
        match outcome {
            Ok(()) => Ok(values),
            Err(error) => {
                for value in values {
                    self.heap.release_root(value);
                }
                Err(error)
            }
        }
    }

    pub(super) fn rooted_iterator_step_value(
        &mut self,
        p: &ResidualProgram,
        iterator: RootId,
        next: RootId,
    ) -> Result<Option<Value>, JsError> {
        let receiver = self.heap.root_value(iterator).unwrap();
        let method = self.heap.root_value(next).unwrap();
        let step = self.call_value(p, method, receiver, &[])?;
        self.iterator_result_value(p, step)
    }

    pub(super) fn iterator_result_value(
        &mut self,
        p: &ResidualProgram,
        step: Value,
    ) -> Result<Option<Value>, JsError> {
        if !self.is_object_like(step) {
            return Err(self.type_error(p, "iterator next result is not an object".into()));
        }
        self.with_call_roots([step], |vm| {
            let done_atom = vm.intern_atom("done");
            let done = vm.get_property(p, step, done_atom)?;
            if vm.truthy(done) {
                return Ok(None);
            }
            let value_atom = vm.intern_atom("value");
            vm.get_property(p, step, value_atom).map(Some)
        })
    }
}

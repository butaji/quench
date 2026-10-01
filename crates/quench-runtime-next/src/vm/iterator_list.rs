use super::*;

impl<H: Host> Vm<H> {
    // On success the caller owns the value roots; on error this operation releases them.
    pub(super) fn rooted_iterator_list(
        &mut self,
        p: &ResidualProgram,
        source: Value,
        method: Value,
    ) -> Result<Vec<RootId>, JsError> {
        let mut iterator = None;
        let mut next = None;
        let mut values = Vec::new();
        let outcome = (|| {
            let value = self.call_value(p, method, source, &[])?;
            if !self.is_object_like(value) {
                return Err(self.type_error(p, "iterator method did not return an object".into()));
            }
            let iterator_root = self.heap.root(value);
            iterator = Some(iterator_root);
            let next_atom = self.intern_atom("next");
            let value = self.get_property(p, value, next_atom)?;
            let next_root = self.heap.root(value);
            next = Some(next_root);
            let done_atom = self.intern_atom("done");
            let value_atom = self.intern_atom("value");
            loop {
                let iterator = self.heap.root_value(iterator_root).unwrap();
                let next = self.heap.root_value(next_root).unwrap();
                let step = self.call_value(p, next, iterator, &[])?;
                if !self.is_object_like(step) {
                    return Err(self.type_error(p, "iterator next result is not an object".into()));
                }
                let step = self.heap.root(step);
                let outcome = (|| {
                    let object = self.heap.root_value(step).unwrap();
                    let done = self.get_property(p, object, done_atom)?;
                    if self.truthy(done) {
                        return Ok(None);
                    }
                    let object = self.heap.root_value(step).unwrap();
                    self.get_property(p, object, value_atom).map(Some)
                })();
                self.heap.release_root(step);
                match outcome? {
                    Some(value) => values.push(self.heap.root(value)),
                    None => return Ok(()),
                }
            }
        })();
        for root in [iterator, next].into_iter().flatten() {
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
}

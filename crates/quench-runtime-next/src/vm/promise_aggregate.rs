use super::promise::{AggregateJob, AggregateMode, AggregateRecord};
use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn aggregate_result(
        &mut self,
        record: &AggregateRecord,
        values: Vec<Value>,
    ) -> Result<Value, JsError> {
        let Some(keys) = record.keys.as_ref() else {
            return Ok(self.heap.alloc(Cell::Array {
                object: Self::empty_object(self.array_proto),
                elements: std::rc::Rc::new(values),
            }));
        };
        let result = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        for (key, value) in keys.iter().copied().zip(values) {
            let key = match self.heap.get(key).cloned() {
                Some(Cell::String(name)) => PropertyKey::string(self.intern_js_atom(&name)),
                Some(Cell::Symbol(_)) => PropertyKey::symbol(key),
                _ => continue,
            };
            self.set_shape_property(result, key, value)?;
        }
        Ok(result)
    }

    pub(super) fn aggregate_error(&mut self, errors: Vec<Value>) -> Result<Value, JsError> {
        let errors = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: std::rc::Rc::new(errors),
        });
        let errors = self.heap.root(errors);
        let prototype_atom = self.intern_atom("prototype");
        let constructor_atom = self.intern_atom("AggregateError");
        let constructor = self
            .own_property(self.realm.globals, constructor_atom)
            .unwrap_or_else(|| self.native_value(Native::AggregateError));
        let prototype = self
            .own_property(constructor, prototype_atom)
            .unwrap_or(self.object_proto);
        let error = self.heap.alloc(Cell::Object(Self::empty_object(prototype)));
        let error = self.heap.root(error);
        let result = (|| {
            let error = self.heap.root_value(error).unwrap_or(Value::UNDEFINED);
            self.set_builtin_value_named(error, "\0rqj:error-brand", Value::TRUE)?;
            let errors = self.heap.root_value(errors).unwrap_or(Value::UNDEFINED);
            self.set_builtin_value_named(error, "errors", errors)?;
            Ok(error)
        })();
        self.heap.release_root(error);
        self.heap.release_root(errors);
        let error = result?;
        Ok(error)
    }

    pub(super) fn enqueue_aggregate_input(
        &mut self,
        p: &ResidualProgram,
        aggregate: Value,
        index: usize,
        input: Value,
    ) -> Result<(), JsError> {
        let then_atom = self.intern_atom("then");
        let then = self.get_property(p, input, then_atom)?;
        if !self.is_function(then) {
            return Err(self.type_error(p, "Promise resolve result has no callable then".into()));
        }
        let record = self
            .promise
            .aggregates
            .get(&aggregate)
            .cloned()
            .ok_or_else(|| JsError("invalid Promise aggregate".into()))?;
        let (fulfilled, rejected) = match record.mode {
            AggregateMode::Race => (record.resolve, record.reject),
            AggregateMode::All | AggregateMode::AllKeyed => (
                self.aggregate_element_function(aggregate, index, false),
                record.reject,
            ),
            AggregateMode::Any => (
                record.resolve,
                self.aggregate_element_function(aggregate, index, true),
            ),
            AggregateMode::AllSettled | AggregateMode::AllSettledKeyed => (
                self.aggregate_element_function(aggregate, index, false),
                self.aggregate_element_function(aggregate, index, true),
            ),
        };
        self.call_value(p, then, input, &[fulfilled, rejected])?;
        Ok(())
    }

    fn aggregate_element_function(
        &mut self,
        aggregate: Value,
        index: usize,
        rejected: bool,
    ) -> Value {
        let function = self.native_with_env(Native::PromiseAggregateJob, Value::UNDEFINED);
        self.promise.aggregate_jobs.insert(
            function,
            AggregateJob {
                aggregate,
                index,
                rejected,
            },
        );
        function
    }
}

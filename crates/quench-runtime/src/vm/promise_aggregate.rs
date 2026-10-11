use super::promise::{AggregateJob, AggregateMode};
use super::property_key::PropertyKey;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn aggregate_result(
        &mut self,
        keys: Option<Vec<Value>>,
        values: Vec<Value>,
    ) -> Result<Value, JsError> {
        let Some(keys) = keys else {
            return Ok(self.heap.alloc(Cell::array(
                self.array_proto,
                std::rc::Rc::new(values),
            )));
        };
        let result = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        for (key, value) in keys.into_iter().zip(values) {
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
        let array_prototype = self.array_prototype_for_realm(self.realm.globals);
        let errors = self.heap.alloc(Cell::array(
            array_prototype,
            std::rc::Rc::new(errors),
        ));
        let errors = self.heap.root(errors);
        let prototype =
            self.realm.intrinsics.builtin_prototypes[&(self.realm.globals, Native::AggregateError)];
        let error = self.heap.alloc(Cell::Object(Object::error(prototype)));
        let error = self.heap.root(error);
        let result = (|| {
            let error = self.heap.root_value(error).unwrap_or(Value::UNDEFINED);
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
        let input_root = self.heap.root(input);
        let aggregate_root = self.heap.root(aggregate);
        let mut then_root = None;
        let outcome = (|| {
            let then_atom = self.intern_atom("then");
            let then =
                self.get_property(p, self.heap.root_value(input_root).unwrap(), then_atom)?;
            if !self.is_function(then) {
                return Err(
                    self.type_error(p, "Promise resolve result has no callable then".into())
                );
            }
            then_root = Some(self.heap.root(then));
            let aggregate = self.heap.root_value(aggregate_root).unwrap();
            let (mode, resolve, reject) = self
                .realm
                .promise
                .aggregates
                .get(&aggregate)
                .map(|record| (record.mode, record.resolve, record.reject))
                .ok_or_else(|| JsError("invalid Promise aggregate".into()))?;
            let (fulfilled, rejected) = match mode {
                AggregateMode::Race => (resolve, reject),
                AggregateMode::All | AggregateMode::AllKeyed => (
                    self.aggregate_element_function(aggregate, index, false),
                    reject,
                ),
                AggregateMode::Any => (
                    resolve,
                    self.aggregate_element_function(aggregate, index, true),
                ),
                AggregateMode::AllSettled | AggregateMode::AllSettledKeyed => (
                    self.aggregate_element_function(aggregate, index, false),
                    self.aggregate_element_function(aggregate, index, true),
                ),
            };
            let then = self.heap.root_value(then_root.unwrap()).unwrap();
            let input = self.heap.root_value(input_root).unwrap();
            self.call_value(p, then, input, &[fulfilled, rejected])?;
            Ok(())
        })();
        self.heap.release_root(input_root);
        self.heap.release_root(aggregate_root);
        if let Some(root) = then_root {
            self.heap.release_root(root);
        }
        outcome
    }

    fn aggregate_element_function(
        &mut self,
        aggregate: Value,
        index: usize,
        rejected: bool,
    ) -> Value {
        let function = self.native_with_env(Native::PromiseAggregateJob, Value::UNDEFINED);
        self.realm.promise.aggregate_jobs.insert(
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

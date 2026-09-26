use super::promise::{AggregateJob, AggregateMode};
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn aggregate_error(&mut self, errors: Vec<Value>) -> Result<Value, JsError> {
        let errors = self.heap.alloc(Cell::Array {
            object: Self::empty_object(self.array_proto),
            elements: std::rc::Rc::new(errors),
        });
        let error = self
            .heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)));
        let name_atom = self.intern_atom("name");
        let message_atom = self.intern_atom("message");
        let errors_atom = self.intern_atom("errors");
        let name = self.heap.alloc(Cell::String("AggregateError".into()));
        let message = self
            .heap
            .alloc(Cell::String("All promises were rejected".into()));
        self.set_property(error, name_atom, name)?;
        self.set_property(error, message_atom, message)?;
        self.set_property(error, errors_atom, errors)?;
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
            AggregateMode::All => (
                self.aggregate_element_function(aggregate, index, false),
                record.reject,
            ),
            AggregateMode::Any => (
                record.resolve,
                self.aggregate_element_function(aggregate, index, true),
            ),
            AggregateMode::AllSettled => (
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

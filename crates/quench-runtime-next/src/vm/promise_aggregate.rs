use super::promise::{AggregateJob, PromiseReaction, PromiseState};
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
        value: Value,
    ) -> Result<(), JsError> {
        {
            let record = self
                .promise
                .aggregates
                .get_mut(&aggregate)
                .ok_or_else(|| JsError("invalid Promise aggregate".into()))?;
            record.values.push(Value::UNDEFINED);
            record.remaining += 1;
        }
        let value_root = self.heap.root(value);
        let input = self.promise_for_value(
            p,
            self.heap
                .root_value(value_root)
                .expect("aggregate input root exists"),
        );
        self.heap.release_root(value_root);
        let input = input?;
        let fulfilled = self.native_with_env(Native::PromiseAggregateJob, Value::NULL);
        let rejected = self.native_with_env(Native::PromiseAggregateJob, Value::NULL);
        self.promise.aggregate_jobs.insert(
            fulfilled,
            AggregateJob {
                aggregate,
                index,
                rejected: false,
            },
        );
        self.promise.aggregate_jobs.insert(
            rejected,
            AggregateJob {
                aggregate,
                index,
                rejected: true,
            },
        );
        let next = self.promise_object();
        let reaction = PromiseReaction {
            on_fulfilled: fulfilled,
            on_rejected: rejected,
            next,
        };
        let record = self
            .promise
            .records
            .get(&input)
            .cloned()
            .ok_or_else(|| JsError("invalid Promise input".into()))?;
        if record.state == PromiseState::Pending {
            self.promise
                .records
                .get_mut(&input)
                .expect("Promise record exists")
                .reactions
                .push(reaction);
        } else {
            self.enqueue_promise_reaction(p, reaction, record.state, record.result);
        }
        Ok(())
    }
}

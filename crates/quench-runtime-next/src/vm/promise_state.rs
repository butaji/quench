use super::promise::PromiseState;
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn promise_settle(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        state: PromiseState,
        result: Value,
    ) -> Result<(), JsError> {
        let (reactions, finally_reactions) = {
            let Some(record) = self.promise.records.get_mut(&promise) else {
                return Err(JsError("invalid Promise state".into()));
            };
            if record.state != PromiseState::Pending {
                return Ok(());
            }
            record.state = state;
            record.result = result;
            (
                std::mem::take(&mut record.reactions),
                std::mem::take(&mut record.finally_reactions),
            )
        };
        for reaction in reactions {
            self.enqueue_promise_reaction(p, reaction, state, result);
        }
        for reaction in finally_reactions {
            self.enqueue_promise_finally(reaction, state, result);
        }
        Ok(())
    }
}

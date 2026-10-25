use super::promise::{PromiseRejectionState, PromiseState, RejectionNotification};
use super::*;

impl<H: Host> Vm<H> {
    /// The state of `promise`, or `None` when the value is not a promise.
    pub(super) fn promise_record(&self, promise: Value) -> Option<&super::promise::PromiseRecord> {
        self.heap.promise(promise).map(|promise| &promise.record)
    }

    /// The state of `promise` for an update; the heap's write barrier logs the promise, so a
    /// minor collection sees the reactions and result stored into an old one.
    pub(super) fn promise_record_mut(
        &mut self,
        promise: Value,
    ) -> Option<&mut super::promise::PromiseRecord> {
        self.heap.promise_mut(promise).map(|promise| &mut promise.record)
    }

    pub(super) fn is_promise(&self, value: Value) -> bool {
        self.promise_record(value).is_some()
    }

    pub(super) fn observe_promise_rejection(&mut self, promise: Value) {
        let Some(record) = self.promise_record_mut(promise) else {
            return;
        };
        let notification = match record.rejection {
            PromiseRejectionState::Unobserved => {
                record.rejection = PromiseRejectionState::Observed;
                None
            }
            PromiseRejectionState::AwaitingReport => {
                record.rejection = PromiseRejectionState::Observed;
                None
            }
            PromiseRejectionState::Reporting => None,
            PromiseRejectionState::Reported => {
                record.rejection = PromiseRejectionState::HandledReported;
                Some(RejectionNotification::Handled(promise))
            }
            PromiseRejectionState::Observed
            | PromiseRejectionState::HandledReported
            | PromiseRejectionState::Handled => None,
        };
        if let Some(notification) = notification {
            self.realm
                .promise
                .rejection_notifications
                .push_back(notification);
        }
    }

    pub(crate) fn take_promise_rejection_handled_events(
        &mut self,
    ) -> Vec<crate::api::PromiseRejectionEvent> {
        let notifications = std::mem::take(&mut self.realm.promise.rejection_notifications);
        let mut pending = std::collections::VecDeque::new();
        let mut events = Vec::new();
        for notification in notifications {
            let RejectionNotification::Handled(promise) = notification else {
                pending.push_back(notification);
                continue;
            };
            let Some(id) = (|| {
                let record = self.promise_record_mut(promise)?;
                if record.rejection != PromiseRejectionState::HandledReported
                    || record.state != PromiseState::Rejected
                {
                    return None;
                }
                let id = record.rejection_id?.get();
                record.rejection = PromiseRejectionState::Handled;
                Some(id)
            })() else {
                continue;
            };
            events.push(crate::api::PromiseRejectionEvent::Handled {
                id,
                promise: self.root(promise),
            });
        }
        self.realm.promise.rejection_notifications = pending;
        events
    }

    pub(crate) fn take_promise_rejection_unhandled_events(
        &mut self,
    ) -> Vec<crate::api::PromiseRejectionEvent> {
        let notifications = std::mem::take(&mut self.realm.promise.rejection_notifications);
        let mut pending = std::collections::VecDeque::new();
        let mut events = Vec::new();
        for notification in notifications {
            let RejectionNotification::Unhandled(promise) = notification else {
                pending.push_back(notification);
                continue;
            };
            let Some((id, result)) = (|| {
                let record = self.promise_record_mut(promise)?;
                if record.rejection != PromiseRejectionState::AwaitingReport
                    || record.state != PromiseState::Rejected
                {
                    return None;
                }
                let id = record.rejection_id?.get();
                record.rejection = PromiseRejectionState::Reporting;
                Some((id, record.result))
            })() else {
                continue;
            };
            events.push(crate::api::PromiseRejectionEvent::Unhandled {
                id,
                promise: self.root(promise),
                reason: self.root(result),
            });
        }
        self.realm.promise.rejection_notifications = pending;
        events
    }

    pub(crate) fn mark_promise_rejection_reported(
        &mut self,
        promise: Value,
    ) -> Result<(), JsError> {
        let Some(record) = self.promise_record_mut(promise) else {
            return Err(JsError("invalid Promise rejection report".into()));
        };
        if record.state != PromiseState::Rejected
            || record.rejection != PromiseRejectionState::Reporting
        {
            return Err(JsError("Promise rejection was not pending report".into()));
        }
        record.rejection = PromiseRejectionState::Reported;
        Ok(())
    }

    pub(super) fn promise_settle(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        state: PromiseState,
        result: Value,
    ) -> Result<(), JsError> {
        let (reactions, notify_unhandled, module_evaluation) = {
            let Some(record) = self.promise_record_mut(promise) else {
                return Err(JsError("invalid Promise state".into()));
            };
            if record.state != PromiseState::Pending {
                return Ok(());
            }
            record.state = state;
            record.result = result;
            let module_evaluation = record.module_evaluation;
            let notify_unhandled = if state == PromiseState::Rejected
                && record.rejection == PromiseRejectionState::Unobserved
            {
                record.rejection = PromiseRejectionState::AwaitingReport;
                true
            } else {
                false
            };
            (std::mem::take(&mut record.reactions), notify_unhandled, module_evaluation)
        };
        self.realm.promise.async_module_settled |= module_evaluation;
        if let Some(resource) = self.promise_record(promise).expect("promise record").host_context.get() {
            // The reactions have left the authoritative record but are not
            // queued yet. Hooks may collect, so keep their entire payload live.
            let roots = std::iter::once(promise).chain(std::iter::once(result)).chain(
                reactions.iter().flat_map(|reaction| [reaction.on_fulfilled, reaction.on_rejected, reaction.next]),
            ).collect::<Vec<_>>();
            self.with_call_roots(roots, |vm| {
                let mut context = crate::NativeContext::new(vm);
                H::promise_settled(&mut context, resource)
                    .map_err(|error| context.finish_error(error))
            })?;
        }
        if notify_unhandled {
            let id = self
                .realm
                .promise
                .last_rejection_id
                .checked_add(1)
                .expect("Promise rejection identifier space exhausted");
            self.realm.promise.last_rejection_id = id;
            self.promise_record_mut(promise)
                .expect("settled Promise record remains registered")
                .rejection_id = std::num::NonZeroU64::new(id);
            self.realm
                .promise
                .rejection_notifications
                .push_back(RejectionNotification::Unhandled(promise));
        }
        for reaction in reactions {
            self.enqueue_promise_reaction(p, reaction, state, result);
        }
        Ok(())
    }
}

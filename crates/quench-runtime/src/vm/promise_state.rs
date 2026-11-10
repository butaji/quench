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
            PromiseRejectionState::AwaitingReport(_) => {
                record.rejection = PromiseRejectionState::Observed;
                None
            }
            PromiseRejectionState::Reported(id) => {
                record.rejection = PromiseRejectionState::HandledReported(id);
                Some(RejectionNotification::Handled(promise))
            }
            PromiseRejectionState::Reporting(_)
            | PromiseRejectionState::Observed
            | PromiseRejectionState::HandledReported(_)
            | PromiseRejectionState::Handled(_) => None,
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
                let PromiseRejectionState::HandledReported(id) = record.rejection else {
                    return None;
                };
                if record.state != PromiseState::Rejected {
                    return None;
                }
                record.rejection = PromiseRejectionState::Handled(id);
                Some(id.get())
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
                let PromiseRejectionState::AwaitingReport(id) = record.rejection else {
                    return None;
                };
                if record.state != PromiseState::Rejected {
                    return None;
                }
                record.rejection = PromiseRejectionState::Reporting(id);
                Some((id.get(), record.result))
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
        let PromiseRejectionState::Reporting(id) = record.rejection else {
            return Err(JsError("Promise rejection was not pending report".into()));
        };
        if record.state != PromiseState::Rejected {
            return Err(JsError("Promise rejection was not pending report".into()));
        }
        record.rejection = PromiseRejectionState::Reported(id);
        Ok(())
    }

    /// Queues the report of `promise`'s rejection with a fresh identifier, unless the host's
    /// settled hook observed it meanwhile.
    fn queue_unhandled_rejection(&mut self, promise: Value) {
        let unobserved = self
            .promise_record(promise)
            .is_some_and(|record| record.rejection == PromiseRejectionState::Unobserved);
        if !unobserved {
            return;
        }
        let id = self
            .realm
            .promise
            .last_rejection_id
            .checked_add(1)
            .expect("Promise rejection identifier space exhausted");
        self.realm.promise.last_rejection_id = id;
        self.promise_record_mut(promise)
            .expect("settled Promise record remains registered")
            .rejection = PromiseRejectionState::AwaitingReport(
            std::num::NonZeroU64::new(id).expect("rejection identifiers start at one"),
        );
        self.realm.promise.rejection_notifications.push_back(RejectionNotification::Unhandled(promise));
    }

    pub(super) fn promise_settle(
        &mut self,
        p: &ResidualProgram,
        promise: Value,
        state: PromiseState,
        result: Value,
    ) -> Result<(), JsError> {
        let (reactions, notify_unhandled, module_evaluation, host_context) = {
            let Some(record) = self.promise_record_mut(promise) else {
                return Err(JsError("invalid Promise state".into()));
            };
            if record.state != PromiseState::Pending {
                return Ok(());
            }
            record.state = state;
            record.result = result;
            let module_evaluation = record.module_evaluation;
            // Whether to queue the report once the host's settled hook has run, which may
            // observe the rejection first.
            let notify_unhandled =
                state == PromiseState::Rejected && record.rejection == PromiseRejectionState::Unobserved;
            (std::mem::take(&mut record.reactions), notify_unhandled, module_evaluation, record.host_context)
        };
        self.realm.promise.async_module_settled |= module_evaluation;
        if let Some(resource) = host_context.get() {
            // The reactions have left the authoritative record but are not
            // queued yet. Hooks may collect, so keep their entire payload live.
            let roots = std::iter::once(promise)
                .chain(std::iter::once(result))
                .chain(reactions.iter().flat_map(|reaction| {
                    [reaction.on_fulfilled, reaction.on_rejected, reaction.next]
                }))
                .collect::<Vec<_>>();
            self.with_call_roots(roots, |vm| {
                let mut context = crate::NativeContext::new(vm);
                H::promise_settled(&mut context, resource)
                    .map_err(|error| context.finish_error(error))
            })?;
        }
        if notify_unhandled {
            self.queue_unhandled_rejection(promise);
        }
        for reaction in reactions {
            self.enqueue_promise_reaction(p, reaction, state, result);
        }
        Ok(())
    }
}

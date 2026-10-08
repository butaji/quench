use super::promise::{PromiseRejectionState, PromiseState, RejectionNotification};
use super::*;

impl<H: Host> Vm<H> {
    pub(super) fn observe_promise_rejection(&mut self, promise: Value) {
        let Some(record) = self.realm.promise.records.get_mut(&promise) else {
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
                let record = self.realm.promise.records.get_mut(&promise)?;
                if record.rejection != PromiseRejectionState::HandledReported
                    || record.state != PromiseState::Rejected
                {
                    return None;
                }
                let id = record.rejection_id?;
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
                let record = self.realm.promise.records.get_mut(&promise)?;
                if record.rejection != PromiseRejectionState::AwaitingReport
                    || record.state != PromiseState::Rejected
                {
                    return None;
                }
                let id = record.rejection_id?;
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
        let Some(record) = self.realm.promise.records.get_mut(&promise) else {
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
        let (reactions, notify_unhandled) = {
            let Some(record) = self.realm.promise.records.get_mut(&promise) else {
                return Err(JsError("invalid Promise state".into()));
            };
            if record.state != PromiseState::Pending {
                return Ok(());
            }
            record.state = state;
            record.result = result;
            let notify_unhandled = if state == PromiseState::Rejected
                && record.rejection == PromiseRejectionState::Unobserved
            {
                record.rejection = PromiseRejectionState::AwaitingReport;
                true
            } else {
                false
            };
            (std::mem::take(&mut record.reactions), notify_unhandled)
        };
        if notify_unhandled {
            let id = self
                .realm
                .promise
                .last_rejection_id
                .checked_add(1)
                .expect("Promise rejection identifier space exhausted");
            self.realm.promise.last_rejection_id = id;
            self.realm
                .promise
                .records
                .get_mut(&promise)
                .expect("settled Promise record remains registered")
                .rejection_id = Some(id);
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

//! The jobs a realm queues.

use super::promise;
use crate::Value;

/// A queued job: a call, or a promise reaction run without a job function to dispatch through.
pub enum PendingJob {
    Call {
        callback: Value,
        this: Value,
        args: Vec<Value>,
    },
    Reaction(promise::PromiseJob),
    Thenable(promise::ThenableJob),
}
const _: () = assert!(size_of::<PendingJob>() == 48);

impl PendingJob {
    /// Pushes the values the job holds onto `roots`, which keep them alive while it runs.
    pub(super) fn push_roots(&self, roots: &mut Vec<Value>) {
        match self {
            Self::Call {
                callback,
                this,
                args,
            } => {
                roots.extend([*callback, *this]);
                roots.extend_from_slice(args);
            }
            Self::Reaction(job) => roots.extend([job.realm, job.handler, job.next, job.value]),
            Self::Thenable(job) => roots.extend([job.realm, job.then, job.thenable, job.promise]),
        }
    }
}

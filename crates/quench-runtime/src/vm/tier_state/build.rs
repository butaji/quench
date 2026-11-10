//! The compile handle every tier polls.

/// A compile under way: the code generator's handle and the entries to let pass before the next
/// look at it. The one polled handle of every tier.
pub(in crate::vm) struct Build<P> {
    pending: Box<P>,
    wait: u16,
}

/// What a look at a compile's handle came to.
pub(in crate::vm) enum Polled<P, A> {
    Ready(A),
    /// A fact the compile took changed while it ran.
    Stale,
    /// The code generator refused to build the code.
    Failed,
    Pending(Box<P>),
}

/// What polling a `Build` came to.
pub(in crate::vm) enum Looked<P, A> {
    Ready(A),
    Stale,
    Failed,
    Pending(Build<P>),
}

impl<P> Build<P> {
    /// A compile to look at after `wait` entries.
    pub(in crate::vm) fn new(pending: Box<P>, wait: u16) -> Self {
        Self { pending, wait }
    }

    pub(in crate::vm) fn pending(&self) -> &P {
        &self.pending
    }

    /// Whether this entry passes without a look: counts the entries left before it.
    pub(in crate::vm) fn waiting(&mut self) -> bool {
        let waiting = self.wait > 0;
        self.wait = self.wait.saturating_sub(1);
        waiting
    }

    /// One entry of the owner: counts down to the next look, which `look` makes at the handle
    /// and which waits `interval` entries again where the code is not ready.
    pub(in crate::vm) fn poll<A>(self, interval: u16, look: impl FnOnce(Box<P>) -> Polled<P, A>) -> Looked<P, A> {
        if self.wait > 0 {
            return Looked::Pending(Self { wait: self.wait - 1, ..self });
        }
        match look(self.pending) {
            Polled::Ready(artifact) => Looked::Ready(artifact),
            Polled::Stale => Looked::Stale,
            Polled::Failed => Looked::Failed,
            Polled::Pending(pending) => Looked::Pending(Self { pending, wait: interval }),
        }
    }
}

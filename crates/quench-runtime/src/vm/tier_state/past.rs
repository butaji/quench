//! The history a tier's bodies leave their successors.

use super::SPECULATIVE_BODIES;

/// The speculative bodies built so far, and what they learned.
pub(in crate::vm) struct Past<F> {
    pub(in crate::vm) bodies: u8,
    /// The compiles the code cache refused and the bodies a lifecycle edge dropped: each doubles
    /// the next warm-up the owner asks for.
    pub(in crate::vm) refused: u8,
    /// The entries in a row the owner declined to run for want of a body that fits them.
    pub(in crate::vm) misses: u32,
    pub(in crate::vm) feedback: F,
}

impl<F: Default> Default for Past<F> {
    fn default() -> Self {
        Self { bodies: 0, refused: 0, misses: 0, feedback: F::default() }
    }
}

impl<F> Past<F> {
    /// Whether the next body is the generic one: the speculative bodies are spent.
    pub(in crate::vm) fn is_spent(&self) -> bool {
        self.bodies >= SPECULATIVE_BODIES
    }

    /// The history after a body that is dropped, planned from `feedback`.
    pub(in crate::vm) fn after(&self, feedback: F) -> Self {
        Self { bodies: self.bodies.min(SPECULATIVE_BODIES), refused: self.refused, misses: self.misses, feedback }
    }

    /// The history whose next body is the generic one.
    pub(in crate::vm) fn spent(feedback: F) -> Self {
        Self { bodies: SPECULATIVE_BODIES, refused: 0, misses: 0, feedback }
    }

    /// The history after a compile that came to nothing (a stale one): it counts as a body, and
    /// like a body a lifecycle edge dropped, it doubles the next warm-up.
    pub(in crate::vm) fn wasted(self) -> Self {
        Self { bodies: self.bodies.saturating_add(1).min(SPECULATIVE_BODIES), ..self }.refusal()
    }

    /// The history after the code cache refused a compile.
    pub(in crate::vm) fn refusal(self) -> Self {
        Self { refused: self.refused.saturating_add(1), ..self }
    }
}

//! The `constructor` read of a native promise, answered without guest code.

use super::*;

impl<H: Host> Vm<H> {
    /// Whether `promise`'s `constructor` reads as `receiver`, this realm's `%Promise%`, without
    /// running guest code: by the `PromiseSpeciesProof`, which holds exactly that for the
    /// receivers of its shape, or by the plain read, which records the proof for them when the
    /// constructor's `@@species` is still intrinsic. `false` leaves the read to [[Get]].
    pub(super) fn promise_constructor_is_intrinsic(&mut self, promise: Value, receiver: Value) -> bool {
        if self.promise_species_proved(promise) {
            return true;
        }
        let constructor_atom = self.runtime_atoms.constructor;
        let Some(observed) = self.plain_named_load(promise, constructor_atom) else {
            return false;
        };
        if observed != receiver {
            return false;
        }
        if self.has_intrinsic_species_getter(observed) {
            self.record_promise_species_proof(promise, observed);
        }
        true
    }
}

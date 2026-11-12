//! Where arrays stay put: a unit no operation of which may move an array's backing or the record
//! table (`Operation::moves_arrays`: an element store or append, an allocation, a call that may run
//! guest code). In one, the record table's base word an element access loads is read-only, so the
//! compiler hoists it out of the loops around it and shares it across accesses
//! (`Emit::fixed_arrays`); any other unit loads it at each access, as an allocation or a helper may
//! have changed it. The backing pointer an access loads from a record stays an ordinary load: it
//! is read after the access proved its array, which a hoist to a loop's entry would precede.

use super::*;
use crate::staged::{Constant, Operation};

impl Lowering<'_, '_> {
    /// Whether every instruction of the unit is a staged operation that cannot move arrays; an
    /// instruction with no staged operation may.
    pub(super) fn arrays_stay_put(&self) -> bool {
        self.unit.instructions.iter().all(|instruction| {
            Operation::<ir::Value, EmitProof>::decode(*instruction, |_| Some(Constant::untyped(Value::UNDEFINED)))
                .is_some_and(|operation| !operation.moves_arrays())
        })
    }
}

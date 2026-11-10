//! Futamura's first projection: the interpreter specialized to one program.
//!
//! The contract is shared by every publication and every tier. AOT and JIT are two
//! invocations of it: they differ in the `Division` they fold and in where the residual
//! code is published, never in what a specializer is. Implementations live beside the
//! lowering they drive (`CallThreaded` in `vm::specialize`, `RegionSpecializer` in
//! `vm::compiled::lane_region`).

use crate::bytecode::ResidualProgram;

/// The first projection of the interpreter onto one program.
///
/// `Residual` is the publication: linked object code, Cranelift IR, copied stencils, or a
/// lowered region. The trait deliberately carries no division or tier: those are the
/// caller's plan, so a JIT and an AOT publication can use the same implementation.
pub(crate) trait Specializer {
    type Residual;

    fn specialize(self, program: &ResidualProgram) -> Result<Self::Residual, String>;
}

//! What decoding an instruction reads beyond its record: the tables its operands index. A
//! context is borrowed slices of the program's own tables, data a tier lends for the decode and
//! keeps nothing of.

use super::Constant;
use crate::bytecode::{MethodSite, Op, Register, ResidualProgram, WideInstruction};

/// The tables a decode reads: the constants a `LoadConst` names and the method sites a method call
/// names (whose argument registers the call's own record holds).
#[derive(Clone, Copy)]
pub struct DecodeContext<'a> {
    pub constants: &'a [Constant],
    /// The constant at an index `constants` does not hold, as the tier types it (a region's Number
    /// table, the compiling VM's values): `None` where `constants` is the whole table.
    pub constant_at: Option<&'a dyn Fn(usize) -> Option<Constant>>,
    pub method_sites: &'a [MethodSite],
}

impl DecodeContext<'static> {
    /// No table: a tier that decodes only the rows whose operands the record holds.
    pub const EMPTY: Self = Self { constants: &[], constant_at: None, method_sites: &[] };
}

impl<'a> DecodeContext<'a> {
    /// The method sites of `program`; its constants are the tier's to type
    /// (`of_constants`), so a `LoadConst` decodes only where the tier gives them.
    pub fn of(program: &'a ResidualProgram) -> Self {
        Self { constants: &[], constant_at: None, method_sites: &program.method_sites }
    }

    /// Only the constants `constants`.
    #[cfg_attr(not(test), allow(dead_code))]
    #[quench_macros::test_visible]
    pub(crate) const fn of_constants(constants: &'a [Constant]) -> Self {
        DecodeContext { constants, ..DecodeContext::EMPTY }
    }

    /// The constant at `index`, as the tier typed it: from `constants`, else from `constant_at`.
    pub(crate) fn constant(self, index: usize) -> Option<Constant> {
        self.constants.get(index).copied().or_else(|| self.constant_at.and_then(|at| at(index)))
    }

    /// The method site at `index`.
    pub fn method_site(self, index: usize) -> Option<&'a MethodSite> {
        self.method_sites.get(index)
    }
}

/// The fields a staged decode reads, from whichever record a tier holds: the bytecode's
/// `WideInstruction`, whose constants index the context, or a tier's own pre-resolved record (the
/// lane's `LaneInstruction`). One decoder per row family serves every record, so a record format
/// is never a second definition of what an instruction means.
pub trait Fields: Copy {
    fn op(self) -> Op;
    fn result_register(self) -> Register;
    fn register_a(self) -> Register;
    fn register_b(self) -> Register;
    /// The constant a `LoadConst` defines, typed as the tier gives it.
    fn constant(self, context: DecodeContext<'_>) -> Option<Constant>;
    /// The record as the bytecode instruction it was made from: its opcode, fields and immediate,
    /// which every decoder but the flow one's constant reads.
    fn wide(self) -> WideInstruction;
}

impl Fields for WideInstruction {
    #[inline(always)] fn op(self) -> Op { WideInstruction::op(self) }
    #[inline(always)] fn result_register(self) -> Register { WideInstruction::result_register(self) }
    #[inline(always)] fn register_a(self) -> Register { WideInstruction::register_a(self) }
    #[inline(always)] fn register_b(self) -> Register { WideInstruction::register_b(self) }
    #[inline(always)] fn constant(self, context: DecodeContext<'_>) -> Option<Constant> {
        context.constant(self.constant_index())
    }
    #[inline(always)] fn wide(self) -> WideInstruction { self }
}

//! What decoding an instruction reads beyond its record: the tables its operands index. A
//! context is borrowed slices of the program's own tables, data a tier lends for the decode and
//! keeps nothing of.

use super::Constant;
use crate::bytecode::{MethodSite, Register, ResidualProgram};

/// The tables a decode reads: the constants a `LoadConst` names, the method sites a method call
/// names, and the call records (the registers each method site passes, from its
/// `argument_start`).
#[derive(Clone, Copy)]
pub(crate) struct DecodeContext<'a> {
    pub(crate) constants: &'a [Constant],
    pub(crate) method_sites: &'a [MethodSite],
    #[allow(dead_code)] // read by the form-generic lane helpers
    pub(crate) call_records: &'a [Register],
}

impl DecodeContext<'static> {
    /// No table: a tier that decodes only the rows whose operands the record holds.
    pub(crate) const EMPTY: Self = Self { constants: &[], method_sites: &[], call_records: &[] };
}

impl<'a> DecodeContext<'a> {
    /// The method sites and call records of `program`; its constants are the tier's to type
    /// (`of_constants`), so a `LoadConst` decodes only where the tier gives them.
    pub(crate) fn of(program: &'a ResidualProgram) -> Self {
        Self { constants: &[], method_sites: &program.method_sites, call_records: &program.method_arguments }
    }

    /// Only the constants `constants`.
    #[cfg_attr(not(test), allow(dead_code))]
    pub(crate) const fn of_constants(constants: &'a [Constant]) -> Self {
        DecodeContext { constants, ..DecodeContext::EMPTY }
    }

    /// The constant at `index`, as the tier typed it.
    pub(crate) fn constant(self, index: usize) -> Option<Constant> {
        self.constants.get(index).copied()
    }

    /// The method site at `index`.
    pub(crate) fn method_site(self, index: usize) -> Option<&'a MethodSite> {
        self.method_sites.get(index)
    }
}

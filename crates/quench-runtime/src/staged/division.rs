//! Ershov's division of each staged operation, as data: which operands the bytecode fixes
//! (Static, Location), which a code dependency may fix (Stable), which feedback sites fix
//! (Site) and which only run time knows (Dynamic). A specializer asks the division
//! (`OperandTimes::folded`) instead of deciding per row: Emit folds the operands it answers
//! to constants (Stable ones under a code dependency), and the stencil stage makes them holes.
//! `Operation::operand_times` matches every variant, so a new operation without a division
//! does not compile.

use smallvec::SmallVec;

use super::{GlobalBinding, NumberOperation, Operation};
use crate::bytecode::Operand;
use crate::typed_ir::{BindingTime, Division};

/// The most operands one staged operation names.
const MAX_OPERANDS: usize = 8;

/// One operand's name (the field it is) and binding time.
pub(crate) type OperandTime = (&'static str, BindingTime);

/// An operation's operands in field order, each with its binding time.
pub(crate) type OperandTimes = SmallVec<[OperandTime; MAX_OPERANDS]>;

/// Which operands of `times` a specializer under `division` may fold.
#[cfg_attr(not(test), allow(dead_code))] // the stencil build and Emit's row decisions ask this
pub(crate) fn folded(times: &[OperandTime], division: Division) -> impl Iterator<Item = &'static str> + '_ {
    times.iter().filter(move |(_, time)| division.folds(*time)).map(|(name, _)| *name)
}

/// The time of the field a heap read takes from its proof: the shape's layout and the
/// slot's type are Stable, since a change to either retires the code.
pub(crate) const FIELD_LAYOUT: BindingTime = BindingTime::Stable;

/// The time of a prototype's field value a read inherits: a code dependency holds it.
pub(crate) const INHERITED_FIELD: BindingTime = BindingTime::Stable;

/// The time of a whole-program field value: the closed program fixes it at specialization,
/// and the read checks it once.
pub(crate) const FROZEN_FIELD: BindingTime = BindingTime::Static;

impl GlobalBinding {
    /// What this binding leaves known: an immutable global's value and a closed program's
    /// value are fixed at specialization, a held value rests on a dependency, and a
    /// dynamic binding is the lane's own read.
    pub(crate) const fn time(&self) -> BindingTime {
        match self {
            Self::Constant(_) | Self::Frozen { .. } => BindingTime::Static,
            Self::Held { .. } => BindingTime::Stable,
            Self::Dynamic => BindingTime::Dynamic,
        }
    }
}

/// A numeric operand's time: a register's address, or a constant the bytecode holds.
fn operand(operand: Operand) -> BindingTime {
    match operand.register_index() {
        Some(_) => BindingTime::Location,
        None => BindingTime::Static,
    }
}

const REGISTER: BindingTime = BindingTime::Location;
const BYTECODE: BindingTime = BindingTime::Static;
const VALUE: BindingTime = BindingTime::Dynamic;
const PROOF: BindingTime = BindingTime::Stable;
const SITE: BindingTime = BindingTime::Site;

fn number(operation: &NumberOperation) -> OperandTimes {
    use NumberOperation::*;
    match operation {
        Arithmetic { left, right, .. } => SmallVec::from_slice(&[("destination", REGISTER),
            ("left", operand(*left)), ("right", operand(*right)), ("operation", BYTECODE),
            ("domain", BYTECODE), ("overflow", BYTECODE)]),
        Relation { left, right, .. } => SmallVec::from_slice(&[("left", operand(*left)),
            ("right", operand(*right)), ("relation", BYTECODE), ("domain", BYTECODE)]),
        Remainder { left, right, .. } => SmallVec::from_slice(&[("destination", REGISTER),
            ("left", operand(*left)), ("right", operand(*right)), ("domain", BYTECODE)]),
        IncDec { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER),
            ("decrement", BYTECODE), ("overflow", BYTECODE)]),
        Unary { source, .. } => SmallVec::from_slice(&[("destination", REGISTER),
            ("source", operand(*source)), ("operation", BYTECODE), ("domain", BYTECODE)]),
        ToNumeric { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER)]),
    }
}

impl<W, P> Operation<W, P> {
    /// This operation's operands with their binding times: register indices and local slots
    /// are locations, constants, atoms, jump targets and flags are fixed by the bytecode,
    /// cache sites are feedback, a proof's facts are Stable, and a stage word is Dynamic.
    pub(crate) fn operand_times(&self) -> OperandTimes {
        match self {
            Self::Move { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER)]),
            Self::LoadConst { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("constant", BYTECODE)]),
            Self::LoadLocalPlain { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("slot", REGISTER)]),
            Self::StoreLocalPlain { .. } => SmallVec::from_slice(&[("source", REGISTER), ("slot", REGISTER),
                ("result", REGISTER)]),
            Self::Jump => SmallVec::from_slice(&[("target", BYTECODE)]),
            Self::JumpIf { .. } => SmallVec::from_slice(&[("register", REGISTER), ("taken", BYTECODE),
                ("target", BYTECODE)]),
            Self::LoadThis { .. } => SmallVec::from_slice(&[("destination", REGISTER)]),
            Self::Number(operation) => number(operation),
            Self::RequireObjectCoercible { .. } => SmallVec::from_slice(&[("register", REGISTER)]),
            Self::LogicalNot { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER)]),
            _ => self.heap_operand_times(),
        }
    }

    /// The heap operations' operands (`operand_times`).
    fn heap_operand_times(&self) -> OperandTimes {
        match self {
            Self::ReadField { .. } => SmallVec::from_slice(&[("base", REGISTER), ("slot", FIELD_LAYOUT),
                ("ty", FIELD_LAYOUT), ("proof", PROOF)]),
            Self::WriteField { .. } => SmallVec::from_slice(&[("base", REGISTER), ("slot", FIELD_LAYOUT),
                ("ty", FIELD_LAYOUT), ("value", VALUE), ("proof", PROOF)]),
            Self::IndexRead { .. } | Self::TypedIndexRead { .. } => SmallVec::from_slice(&[("index", VALUE), ("proof", PROOF)]),
            Self::IndexWrite { .. } | Self::TypedIndexWrite { .. } =>
                SmallVec::from_slice(&[("index", VALUE), ("value", VALUE), ("proof", PROOF)]),
            Self::MakeObject { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("proof", PROOF)]),
            Self::MakeArray { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("length", VALUE),
                ("elements", VALUE), ("proof", PROOF)]),
            Self::MakeConstArray { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("constant", VALUE),
                ("count", VALUE), ("proof", PROOF)]),
            Self::MakeObjectPair { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("site", SITE),
                ("first", VALUE), ("second", VALUE), ("proof", PROOF)]),
            Self::Safepoint { .. } => SmallVec::from_slice(&[("proof", PROOF)]),
            Self::IndexSetHelper { .. } => SmallVec::from_slice(&[("base", VALUE), ("key", VALUE),
                ("value", VALUE), ("proof", PROOF)]),
            Self::GlobalRead { binding, .. } => SmallVec::from_slice(&[("destination", REGISTER),
                ("atom", BYTECODE), ("site", SITE), ("binding", binding.time()), ("proof", PROOF)]),
            Self::FieldStoreMiss { .. } => SmallVec::from_slice(&[("object", VALUE), ("atom", BYTECODE),
                ("site", SITE), ("value", VALUE), ("proof", PROOF)]),
            _ => unreachable!("`operand_times` answers every other operation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::Constant;
    use crate::{typed_ir::Type, Value};

    type Op = Operation<u64, ()>;

    fn samples() -> Vec<Op> {
        let constant = Constant::untyped(Value::number(1.0));
        vec![
            Op::Move { destination: 0, source: 1 },
            Op::LoadConst { destination: 0, constant },
            Op::LoadLocalPlain { destination: 0, slot: 0 },
            Op::StoreLocalPlain { source: 0, slot: 0, result: None },
            Op::Jump,
            Op::JumpIf { register: 0, taken: true },
            Op::LoadThis { destination: 0 },
            Op::Number(NumberOperation::ToNumeric { destination: 0, source: 1 }),
            Op::Number(NumberOperation::IncDec { destination: 0, source: 1, decrement: false,
                overflow: crate::staged::Overflow::Exit }),
            Op::RequireObjectCoercible { register: 0 },
            Op::LogicalNot { destination: 0, source: 1 },
            Op::ReadField { base: 0, slot: 0, ty: Type::ANY, proof: () },
            Op::WriteField { base: 0, slot: 0, ty: Type::ANY, value: 0, proof: () },
            Op::IndexRead { index: 0, proof: () },
            Op::IndexWrite { index: 0, value: 0, proof: () },
            Op::TypedIndexRead { index: 0, proof: () },
            Op::TypedIndexWrite { index: 0, value: 0, proof: () },
            Op::MakeObject { destination: 0, proof: () },
            Op::MakeArray { destination: 0, length: 0, elements: 0, proof: () },
            Op::MakeConstArray { destination: 0, constant: 0, count: 0, proof: () },
            Op::MakeObjectPair { destination: 0, site: 0, first: 0, second: 0, proof: () },
            Op::Safepoint { proof: () },
            Op::IndexSetHelper { base: 0, key: 1, value: 2, proof: () },
            Op::GlobalRead { destination: 0, atom: 0, site: 0, binding: GlobalBinding::Dynamic, proof: () },
            Op::FieldStoreMiss { object: 0, atom: 0, site: 0, value: 1, proof: () },
        ]
    }

    /// Every staged operation has a division, with unique operand names; `operand_times`
    /// matches each variant, so the compiler keeps this total for new operations.
    #[test]
    fn every_staged_operation_has_a_total_division() {
        for operation in samples() {
            let times = operation.operand_times();
            assert!(!times.is_empty());
            let mut names: Vec<_> = times.iter().map(|(name, _)| *name).collect();
            names.sort_unstable();
            names.dedup();
            assert_eq!(names.len(), times.len(), "operand names are unique");
        }
    }

    #[test]
    fn only_stage_words_are_dynamic_and_offline_folds_no_stable_operand() {
        let read = Op::ReadField { base: 0, slot: 3, ty: Type::ANY, proof: () }.operand_times();
        assert!(!folded(&read, Division::Offline).any(|name| name == "slot"));
        assert!(folded(&read, Division::Online).any(|name| name == "slot"));
        let write = Op::WriteField { base: 0, slot: 3, ty: Type::ANY, value: 7, proof: () }.operand_times();
        assert!(!folded(&write, Division::Online).any(|name| name == "value"));
    }

    #[test]
    fn a_global_read_folds_by_its_binding() {
        let times = |binding| Op::GlobalRead { destination: 0, atom: 0, site: 0, binding, proof: () }.operand_times();
        let held = GlobalBinding::Held { value: Value::UNDEFINED, ty: Type::ANY };
        assert!(folded(&times(held), Division::Offline).all(|name| name != "binding"));
        assert!(folded(&times(held), Division::Frozen).any(|name| name == "binding"));
        assert!(folded(&times(GlobalBinding::Constant(Value::UNDEFINED)), Division::Offline).any(|name| name == "binding"));
        assert!(folded(&times(GlobalBinding::Dynamic), Division::Online).all(|name| name != "binding"));
    }
}

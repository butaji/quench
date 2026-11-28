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
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

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
pub const INHERITED_FIELD: BindingTime = BindingTime::Stable;

/// The time of a whole-program field value: the closed program fixes it at specialization,
/// and the read checks it once.
pub const FROZEN_FIELD: BindingTime = BindingTime::Static;

impl GlobalBinding {
    /// What this binding leaves known: an immutable global's value and a closed program's
    /// value are fixed at specialization, a held value or a lexical binding's address rests on a
    /// dependency, and a dynamic binding is the lane's own read.
    pub const fn time(&self) -> BindingTime {
        match self {
            Self::Constant(_) | Self::Frozen { .. } => BindingTime::Static,
            Self::Held { .. } | Self::Typed { .. } | Self::Lexical { .. } => BindingTime::Stable,
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
        IntCase { source, .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", operand(*source)),
            ("minimum", BYTECODE), ("maximum", BYTECODE), ("count", BYTECODE)]),
        IncDec { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER),
            ("decrement", BYTECODE), ("overflow", BYTECODE)]),
        Unary { source, .. } => SmallVec::from_slice(&[("destination", REGISTER),
            ("source", operand(*source)), ("operation", BYTECODE), ("domain", BYTECODE)]),
        BitwiseNot { source, .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", operand(*source))]),
        ToNumeric { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER)]),
        RelationValue { left, right, .. } => SmallVec::from_slice(&[("destination", REGISTER),
            ("left", operand(*left)), ("right", operand(*right)), ("relation", BYTECODE), ("domain", BYTECODE)]),
        Bitwise { left, right, .. } => SmallVec::from_slice(&[("destination", REGISTER),
            ("left", operand(*left)), ("right", operand(*right)), ("operation", BYTECODE)]),
        Math { source, .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", operand(*source)), ("operation", BYTECODE)]),
        Extreme { left, right, .. } => SmallVec::from_slice(&[("destination", REGISTER), ("left", operand(*left)), ("right", operand(*right)), ("maximum", BYTECODE)]),
        Imul { left, right, .. } => SmallVec::from_slice(&[("destination", REGISTER), ("left", operand(*left)), ("right", operand(*right))]),
        Power { base, exponent, .. } => SmallVec::from_slice(&[("destination", REGISTER), ("base", operand(*base)), ("exponent", operand(*exponent))]),
    }
}

/// `Operation`'s division, as an extension trait (`OperationDivisionExt`): the inherent impl
/// of this file moved onto a trait, because quench-staged owns `Operation`.
pub trait OperationDivisionExt<W, P> {
    /// This operation's operands with their binding times.
    fn operand_times(&self) -> OperandTimes;
    /// The control and value operations' operands (`operand_times`).
    fn control_operand_times(&self) -> OperandTimes;
    /// The heap operations' operands (`operand_times`).
    fn heap_operand_times(&self) -> OperandTimes;
    /// The keyed operations' operands (`heap_operand_times`).
    fn keyed_operand_times(&self) -> OperandTimes;
    /// The allocation and safepoint operations' operands (`keyed_operand_times`).
    fn make_operand_times(&self) -> OperandTimes;
}

impl<W, P> OperationDivisionExt<W, P> for Operation<W, P> {
    /// This operation's operands with their binding times: register indices and local slots
    /// are locations, constants, atoms, jump targets and flags are fixed by the bytecode,
    /// cache sites are feedback, a proof's facts are Stable, and a stage word is Dynamic.
    fn operand_times(&self) -> OperandTimes {
        match self {
            Self::Move { .. } | Self::LogicalNot { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER)]),
            Self::LoadConst { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("constant", BYTECODE)]),
            Self::Unary { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER), ("operator", BYTECODE)]),
            Self::LoadLocalPlain { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("slot", REGISTER)]),
            Self::StoreLocalPlain { .. } => SmallVec::from_slice(&[("source", REGISTER), ("slot", REGISTER),
                ("result", REGISTER)]),
            Self::LoadLocalChecked { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("slot", REGISTER)]),
            Self::StoreLocalChecked { .. } => SmallVec::from_slice(&[("source", REGISTER), ("slot", REGISTER),
                ("result", REGISTER)]),
            Self::InitializeLocal { .. } => SmallVec::from_slice(&[("slot", REGISTER)]),
            Self::Number(operation) => number(operation),
            _ => self.control_operand_times(),
        }
    }

    /// The control and value operations' operands (`operand_times`).
    fn control_operand_times(&self) -> OperandTimes {
        match self {
            Self::Jump => SmallVec::from_slice(&[("target", BYTECODE)]),
            Self::JumpIf { .. } => SmallVec::from_slice(&[("register", REGISTER), ("taken", BYTECODE),
                ("target", BYTECODE)]),
            Self::LoadThis { .. } | Self::LoadCallee { .. } => SmallVec::from_slice(&[("destination", REGISTER)]),
            Self::RequireObjectCoercible { .. } | Self::RequireIteratorResult { .. } => SmallVec::from_slice(&[("register", REGISTER)]),
            Self::IteratorCleanupPush { .. } => SmallVec::from_slice(&[("iterator", REGISTER), ("done", REGISTER)]),
            Self::IteratorCleanupPop | Self::Nop => SmallVec::from_slice(&[]),
            Self::StringCaseIndex { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER),
                ("start", BYTECODE), ("count", BYTECODE)]),
            Self::Suspend { .. } => SmallVec::from_slice(&[("kind", BYTECODE), ("destination", REGISTER), ("value", REGISTER)]),
            Self::TypeofIndex { .. } | Self::TypeofString { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER)]),
            Self::SetFunctionName { .. } => SmallVec::from_slice(&[("function", REGISTER), ("name", BYTECODE), ("site", SITE)]),
            Self::TypeofTest { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("source", REGISTER),
                ("index", BYTECODE), ("negated", BYTECODE)]),
            _ => self.heap_operand_times(),
        }
    }

    /// The heap operations' operands (`operand_times`).
    fn heap_operand_times(&self) -> OperandTimes {
        match self {
            Self::GetField { .. } => SmallVec::from_slice(&[("base", REGISTER), ("lookup", SITE)]),
            Self::SetField { .. } => SmallVec::from_slice(&[("value", REGISTER), ("base", REGISTER),
                ("atom", BYTECODE), ("site", SITE)]),
            Self::StoreField { .. } => SmallVec::from_slice(&[("base", REGISTER), ("value", VALUE),
                ("fact", FIELD_LAYOUT), ("atom", BYTECODE), ("site", SITE)]),
            Self::FieldGetHost { .. } => SmallVec::from_slice(&[("object", VALUE), ("atom", BYTECODE), ("site", SITE)]),
            Self::FieldAdd { .. } => SmallVec::from_slice(&[("object", VALUE), ("atom", BYTECODE), ("site", SITE),
                ("value", VALUE)]),
            Self::GlobalLexicalLoad { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("declaration", BYTECODE),
                ("site", SITE)]),
            Self::GlobalLexicalStore { .. } => SmallVec::from_slice(&[("source", REGISTER), ("declaration", BYTECODE),
                ("site", SITE), ("initialize", BYTECODE)]),
            Self::LoadName { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("atom", BYTECODE),
                ("site", SITE), ("receiver", REGISTER), ("proof", PROOF)]),
            Self::FieldStoreMiss { .. } => SmallVec::from_slice(&[("object", VALUE), ("atom", BYTECODE),
                ("site", SITE), ("value", VALUE), ("proof", PROOF)]),
            _ => self.keyed_operand_times(),
        }
    }

    /// The keyed operations' operands (`heap_operand_times`): a delete and the element accesses.
    fn keyed_operand_times(&self) -> OperandTimes {
        match self {
            Self::Delete { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("base", REGISTER), ("key", REGISTER),
                ("strict", BYTECODE)]),
            Self::IndexGetHost { .. } => SmallVec::from_slice(&[("base", VALUE), ("key", VALUE)]),
            Self::ElementRead { .. } => SmallVec::from_slice(&[("base", REGISTER), ("key", REGISTER)]),
            Self::ElementWrite { .. } => SmallVec::from_slice(&[("value", REGISTER), ("base", REGISTER), ("key", REGISTER)]),
            Self::IndexWrite { .. } => SmallVec::from_slice(&[("index", VALUE), ("value", VALUE), ("proof", PROOF)]),
            Self::IndexSetHelper { .. } => SmallVec::from_slice(&[("base", VALUE), ("key", VALUE),
                ("value", VALUE), ("proof", PROOF)]),
            _ => self.make_operand_times(),
        }
    }

    /// The allocation and safepoint operations' operands (`keyed_operand_times`).
    fn make_operand_times(&self) -> OperandTimes {
        match self {
            Self::Allocate { .. } => SmallVec::from_slice(&[("destination", REGISTER), ("allocation", BYTECODE),
                ("proof", PROOF)]),
            Self::Safepoint { .. } => SmallVec::from_slice(&[("proof", PROOF)]),
            Self::DefineElement { .. } => SmallVec::from_slice(&[("array", REGISTER), ("index", BYTECODE), ("value", REGISTER)]),
            Self::CloneEnv { .. } => SmallVec::from_slice(&[("plan", BYTECODE)]),
            _ => unreachable!("`operand_times` answers every other operation"),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::bytecode::{FieldBase, FieldLookup};
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
            Op::GetField { base: FieldBase(FieldBase::NESTED), lookup: FieldLookup::Site(0), destination: 0 },
            Op::SetField { value: 0, base: None, atom: 0, site: 0, strict: false },
            Op::StoreField { base: None, value: 0, fact: Type::ANY, atom: 0, site: 0 },
            Op::FieldGetHost { object: 0, atom: 0, site: 0 },
            Op::FieldAdd { object: 0, atom: 0, site: 0, value: 1 },
            Op::Delete { destination: 2, base: 0, key: 1, strict: false },
            Op::IndexGetHost { base: 0, key: 1 },
            Op::ElementRead { base: 0, key: crate::bytecode::Operand::register(1), destination: 0 },
            Op::ElementWrite { value: 0, base: 1, key: 2, strict: false },
            Op::IndexWrite { index: 0, value: 0, proof: () },
            Op::Allocate { destination: 0, allocation: crate::staged::Allocation::Empty, proof: () },
            Op::Safepoint { proof: () },
            Op::IndexSetHelper { base: 0, key: 1, value: 2, proof: () },
            Op::LoadName { destination: 0, atom: 0, site: 0, receiver: None, proof: () },
            Op::GlobalLexicalLoad { destination: 0, declaration: 0, site: 0 },
            Op::GlobalLexicalStore { source: 0, declaration: 0, site: 0, initialize: false },
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
        let store = Op::StoreField { base: None, value: 7, fact: Type::ANY, atom: 0, site: 0 }.operand_times();
        assert!(!folded(&store, Division::Offline).any(|name| name == "fact"));
        assert!(folded(&store, Division::Online).any(|name| name == "fact"));
        assert!(!folded(&store, Division::Online).any(|name| name == "value"));
    }

    #[test]
    fn a_global_binding_folds_by_its_time() {
        let held = GlobalBinding::Held { value: Value::UNDEFINED, ty: Type::ANY };
        assert!(!Division::Offline.folds(held.time()));
        assert!(Division::Frozen.folds(held.time()));
        assert!(Division::Offline.folds(GlobalBinding::Constant(Value::UNDEFINED).time()));
        assert!(!Division::Online.folds(GlobalBinding::Dynamic.time()));
    }
}

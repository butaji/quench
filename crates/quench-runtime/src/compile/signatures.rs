//! Composed effect signatures per function, solved over the whole program. A function's signature
//! joins its own steps, in code order, with the signature of every callee its call sites may reach
//! (`AotFacts::function_facts(f).call_targets(pc)`); a call with no target the facts name joins
//! `Signature::ANY`. The solve starts every function at `Signature::NEVER` and repeats until a pass
//! changes nothing, which settles because the lattice is finite and each pass is monotone in the
//! signatures before it.
//!
//! A step's leave is decided by the settled facts where they decide it (`step_leave`): a step leaves
//! only where a cell it depends on can move, or where it can really throw, run guest code or collect.
//! The row rule (`Effects::can_leave`) is the fallback for the steps no fact decides.
//!
//! This is Kleisli composition in an effect-graded monad: a call's signature is the composition of
//! its callees' signatures in the caller's sequence of steps. The signatures are retired by the same
//! edges as the facts they read (`AotFacts`), so a dependency that retires a fact retires the
//! signature composed over it.

use crate::bytecode::{Constant, FieldBase, FieldLookup, Operand, OperandKind, ResidualProgram, Traits, WideInstruction};
use crate::staged::effects::{call_form, CallForm, Effects, Signature, Step};
use crate::staged::{Callee, DecodeContext, Invocation, NumberOperation, Operation};
use crate::typed_ir::{Cell, Classes, FactKey, Type};

use super::aot_facts::AotFacts;

/// The signature of each function of a program, by function id, and the targets of each call site
/// the solve read: a call's signature is the join over those targets.
pub struct Signatures {
    functions: Vec<Signature>,
    /// `(function, pc)` of a call → the functions it may reach, empty where none is known.
    calls: std::collections::BTreeMap<(u32, u32), Box<[u32]>>,
}

/// One instruction of a function, as the solve reads it once: its own effect row with whether the
/// settled facts let it leave, or a call and the functions it may reach.
enum Body {
    Row { effects: Effects, can_leave: bool },
    Call { pc: u32, targets: Box<[u32]> },
}

impl Body {
    /// The step this instruction is in a pass, given the signatures the pass before found.
    fn step(&self, functions: &[Signature]) -> Step {
        match self {
            Self::Row { effects, can_leave } => Step::Row { effects: *effects, can_leave: *can_leave },
            Self::Call { targets, .. } if targets.is_empty() => Step::Call(Signature::ANY),
            Self::Call { targets, .. } => Step::Call(targets.iter().fold(Signature::NEVER,
                |joined, &target| joined.join(functions.get(target as usize).copied().unwrap_or(Signature::ANY)))),
        }
    }
}

/// Where a step is decided by the settled facts: the program (for its constants), the facts, and the
/// function and pc of the step.
#[derive(Clone, Copy)]
struct Site<'a> {
    program: &'a ResidualProgram,
    facts: &'a AotFacts,
    function: u32,
    pc: u32,
}

/// The functions the call at `pc` of `function` may reach. A construction reaches the one class it
/// always constructs; a known function's call reaches that function; any other call reaches the
/// targets the closed-world facts name (`call_targets`, including a method read's closed set).
fn reached(facts: &AotFacts, function: u32, pc: u32, invocation: &Invocation, instruction: WideInstruction) -> Box<[u32]> {
    let facts = facts.function_facts(function);
    if let Callee::Known(known) = invocation.callee {
        return Box::new([u32::from(known)]);
    }
    if instruction.op().traits().has(Traits::CONSTRUCTION) {
        return facts.construct_target(pc).into_iter().collect();
    }
    facts.call_targets(pc).into_boxed_slice()
}

/// The cell of the object a named field access on `base` reads or stores: the register's cell, or the
/// function's receiver where `base` is `this`. None where the solve did not settle or names neither.
fn object_cell(site: Site<'_>, base: FieldBase) -> Option<Cell> {
    if base == FieldBase::THIS {
        return site.facts.cell(FactKey::Receiver(site.function));
    }
    site.facts.function_facts(site.function).cell_at(site.pc, base.register_index()?)
}

/// Whether the named field access on `base` at `site` is on a closed set of classes: its object's cell is an
/// object (`DIRECT_OBJECT_FAMILY`, so never nullish) and the solve lists the classes it may hold (`access_classes`).
fn closed_object(site: Site<'_>, base: FieldBase) -> bool {
    object_cell(site, base).is_some_and(|cell| cell.ty.is_subtype(Type::DIRECT_OBJECT_FAMILY))
        && matches!(site.facts.function_facts(site.function).access_classes(site.pc), Classes::Set { .. })
}

/// Whether a store of field `atom` at `site` writes a key new code can move (`AotFacts::live`): the name's
/// any-object key, and each listed class's own field, instance or not.
fn store_moves_live_key(site: Site<'_>, atom: u32) -> bool {
    let classes = site.facts.function_facts(site.function).access_classes(site.pc);
    let mut keys = vec![FactKey::AnyField(atom)];
    for &class in classes.ids() {
        keys.push(FactKey::Field { class, instance: true, atom });
        keys.push(FactKey::Field { class, instance: false, atom });
    }
    keys.into_iter().any(|key| site.facts.live(key))
}

/// Whether an operand is a Number at `site`: a register whose settled cell has a representation, or a
/// Number constant of the program.
fn numeric_operand(site: Site<'_>, operand: Operand) -> bool {
    let cell = match operand.register_index() {
        Some(register) => site.facts.function_facts(site.function).cell_at(site.pc, register),
        None => match site.program.constants.get(usize::from(operand.payload())) {
            Some(Constant::Number(_)) if operand.kind() == Some(OperandKind::Constant) => Some(Cell::of(Type::NUMBER)),
            _ => None,
        },
    };
    cell.is_some_and(|cell| cell.representation().is_some())
}

/// Whether every operand is a Number at `site`.
fn numeric_operands(site: Site<'_>, operands: &[Operand]) -> bool {
    operands.iter().all(|&operand| numeric_operand(site, operand))
}

/// The operands a Number operation reads, in the order its row names them. An `IntCase` decides every
/// value (`none leaves`), so it names none.
fn number_operands(number: &NumberOperation) -> Vec<Operand> {
    match *number {
        NumberOperation::Arithmetic { left, right, .. } | NumberOperation::Relation { left, right, .. }
        | NumberOperation::RelationValue { left, right, .. } | NumberOperation::Bitwise { left, right, .. }
        | NumberOperation::Remainder { left, right, .. } | NumberOperation::Extreme { left, right, .. }
        | NumberOperation::Imul { left, right, .. } => vec![left, right],
        NumberOperation::Unary { source, .. } | NumberOperation::BitwiseNot { source, .. } | NumberOperation::Math { source, .. } => vec![source],
        NumberOperation::IncDec { source, .. } | NumberOperation::ToNumeric { source, .. } => vec![Operand::register(source)],
        NumberOperation::IntCase { .. } => Vec::new(),
    }
}

/// Whether a `Binary` of `operator` is a Number operation when its operands are Numbers: a relation,
/// an arithmetic operator or a bitwise one (`vm::operations::binary_operator`).
fn numeric_operator(operator: u32) -> bool {
    use crate::vm::operations::binary_operator as op;
    matches!(operator, op::Equality | op::Inequality | op::StrictEquality | op::StrictInequality
        | op::LessThan | op::LessEqualThan | op::GreaterThan | op::GreaterEqualThan
        | op::ShiftLeft | op::ShiftRight | op::ShiftRightZeroFill | op::BitwiseOR | op::BitwiseXOR | op::BitwiseAnd
        | op::Addition | op::Subtraction | op::Multiplication | op::Division | op::Remainder)
}

/// Whether the step `op` at `site` can leave the activation, decided by the settled facts. The rules, in order:
///
/// 1. A store (`SetField`, `StoreField`) into a key new code can move (`AotFacts::live`: its any-object key,
///    or a listed class's field) can move a cell that code trusts, so it leaves. A store moves no cell
///    otherwise: under the settled solve every store is within its key's cell.
/// 2. A named field read or store whose object's cell is an object and whose classes the solve lists as a
///    closed set cannot leave. The classes' layouts hold the field under the slot subscription, so a miss is
///    an edge the layout takes, not an exit.
/// 3. A Number operation, or a numeric `Binary`, `Equality` or `CompareJump`, whose operands are Numbers
///    (their cells have a representation) cannot leave. A Number operation over any other operand leaves at
///    its miss edge, as its row says; the row table's `NONE` for Number operations does not hold for those.
/// 4. A constant load, register move, jump, branch or return cannot leave.
/// 5. Any other step keeps its row's rule (`Effects::can_leave`): it leaves if it may throw, run guest code or collect.
fn step_leave(op: &Operation<(), ()>, site: Site<'_>) -> bool {
    let generic = op.effects().can_leave();
    match op {
        Operation::Move { .. } | Operation::LoadConst { .. } | Operation::Jump | Operation::JumpIf { .. } | Operation::Return { .. } => false,
        Operation::SetField { base, atom, .. } | Operation::StoreField { base, atom, .. } => {
            store_moves_live_key(site, *atom) || (generic && !closed_object(site, (*base).map_or(FieldBase::THIS, FieldBase::register)))
        }
        Operation::GetField { lookup: FieldLookup::Atom { base, .. }, .. } => generic && !closed_object(site, *base),
        Operation::Binary { left, right, operator, .. } => generic && !(numeric_operator(*operator) && numeric_operands(site, &[*left, *right])),
        Operation::Equality { left, right, .. } | Operation::CompareJump { left, right, .. } => generic && !numeric_operands(site, &[*left, *right]),
        Operation::Number(number) => !numeric_operands(site, &number_operands(number)),
        _ => generic,
    }
}

/// The step of `instruction` at `site` that is not a call: its effect row, and whether the facts let it leave.
/// A step the facts leave open keeps its row's leave flags; one they decide has them cleared
/// (`Effects::without_leaves`), so the call form of a function that cannot leave is not framed by them.
/// An instruction no row decodes may do anything.
fn row(instruction: WideInstruction, site: Site<'_>) -> Body {
    let decode = DecodeContext::of(site.program);
    match Operation::<(), ()>::decode_completed(instruction, decode)
        .or_else(|| Operation::<(), ()>::decode_literal_completed_with(instruction, None, ()))
        .map(|(operation, _)| operation)
    {
        Some(operation) => {
            let can_leave = step_leave(&operation, site);
            let effects = operation.effects();
            Body::Row { effects: if can_leave { effects } else { effects.without_leaves() }, can_leave }
        }
        None => Body::Row { effects: Signature::ANY.effects, can_leave: true },
    }
}

/// The steps of `program`'s function `function`, in code order.
fn bodies(program: &ResidualProgram, facts: &AotFacts, function: u32) -> Vec<Body> {
    let decode = DecodeContext::of(program);
    program.with_record(function as usize, |record| {
        record.instructions().enumerate().map(|(pc, instruction)| {
            let pc = pc as u32;
            match Invocation::of(instruction, decode) {
                Some(invocation) => Body::Call { pc, targets: reached(facts, function, pc, &invocation, instruction) },
                None => row(instruction, Site { program, facts, function, pc }),
            }
        }).collect()
    })
}

/// Solves the signatures of every function of `program` over `facts`: each pass sets every
/// function's signature to the fold of its steps against the signatures the pass before found,
/// until a pass changes nothing.
pub fn solve(program: &ResidualProgram, facts: &AotFacts) -> Signatures {
    let count = program.function_count();
    let bodies: Vec<Vec<Body>> = (0..count).map(|function| bodies(program, facts, function as u32)).collect();
    let mut functions = vec![Signature::NEVER; count];
    loop {
        let mut changed = false;
        for (function, body) in bodies.iter().enumerate() {
            let next = Signature::of_steps(body.iter().map(|step| step.step(&functions)));
            if next != functions[function] {
                functions[function] = next;
                changed = true;
            }
        }
        if !changed { break; }
    }
    let calls = bodies.iter().enumerate().flat_map(|(function, body)| body.iter().filter_map(move |step| match step {
        Body::Call { pc, targets } => Some(((function as u32, *pc), targets.clone())),
        Body::Row { .. } => None,
    })).collect();
    Signatures { functions, calls }
}

impl Signatures {
    /// The signature of function `function`; top for an id the program does not hold.
    pub fn of(&self, function: u32) -> Signature {
        self.functions.get(function as usize).copied().unwrap_or(Signature::ANY)
    }

    /// The call form of the call at `pc` of `caller`: the join of the signatures of the functions it
    /// may reach, which is top where it reaches none the facts name.
    pub fn call_form(&self, caller: u32, pc: u32, inlinable: bool) -> CallForm {
        let callee = match self.calls.get(&(caller, pc)) {
            Some(targets) if !targets.is_empty() => targets.iter().fold(Signature::NEVER, |joined, &target| joined.join(self.of(target))),
            _ => Signature::ANY,
        };
        call_form(callee, inlinable)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::staged::effects::{Leaves, RegionSet};

    fn writes_frame() -> Effects {
        Effects { writes: RegionSet(1), ..Effects::NONE }
    }

    fn may_throw() -> Effects {
        Effects { may_throw: true, ..Effects::NONE }
    }

    #[test]
    fn leaves_join_is_the_chain_maximum() {
        assert_eq!(Leaves::Never.join(Leaves::RestartCall), Leaves::RestartCall);
        assert_eq!(Leaves::RestartCall.join(Leaves::Never), Leaves::RestartCall);
        assert_eq!(Leaves::RestartCall.join(Leaves::Materialize), Leaves::Materialize);
        assert_eq!(Leaves::Materialize.join(Leaves::Never), Leaves::Materialize);
        assert_eq!(Leaves::Never.join(Leaves::Never), Leaves::Never);
    }

    #[test]
    fn signature_join_is_componentwise_with_bottom_as_identity() {
        let throws = Signature { leaves: Leaves::RestartCall, effects: may_throw() };
        let writes = Signature { leaves: Leaves::Never, effects: writes_frame() };
        let joined = throws.join(writes);
        assert_eq!(joined.leaves, Leaves::RestartCall);
        assert!(joined.effects.may_throw);
        assert!(joined.effects.writes.intersects(RegionSet(1)));
        assert_eq!(Signature::NEVER.join(throws), throws);
        assert_eq!(throws.join(Signature::NEVER), throws);
        assert_eq!(Signature::ANY.join(throws), Signature::ANY);
    }

    #[test]
    fn call_form_inlines_an_inlinable_callee_whatever_its_signature() {
        assert_eq!(call_form(Signature::ANY, true), CallForm::Inline);
        assert_eq!(call_form(Signature::NEVER, true), CallForm::Inline);
    }

    #[test]
    fn call_form_is_frameless_only_when_nothing_leaves_collects_or_runs_guest_code() {
        assert_eq!(call_form(Signature::NEVER, false), CallForm::Frameless);
        let throws = Signature { leaves: Leaves::Never, effects: may_throw() };
        assert_eq!(call_form(throws, false), CallForm::Frameless);
        let leaves = Signature { leaves: Leaves::RestartCall, effects: Effects::NONE };
        assert_eq!(call_form(leaves, false), CallForm::Framed);
        let collects = Signature { leaves: Leaves::Never, effects: Effects { may_collect: true, ..Effects::NONE } };
        assert_eq!(call_form(collects, false), CallForm::Framed);
        let runs = Signature { leaves: Leaves::Never, effects: Effects { may_call_js: true, ..Effects::NONE } };
        assert_eq!(call_form(runs, false), CallForm::Framed);
        assert_eq!(call_form(Signature::ANY, false), CallForm::Framed);
    }
}

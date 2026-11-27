//! Composed effect signatures per function, solved over the whole program. A function's signature
//! joins its own steps, in code order, with the signature of every callee its call sites may reach
//! (`AotFacts::function_facts(f).call_targets(pc)`); a call with no target the facts name joins
//! `Signature::ANY`. The solve starts every function at `Signature::NEVER` and repeats until a pass
//! changes nothing, which settles because the lattice is finite and each pass is monotone in the
//! signatures before it.
//!
//! Every site has one grade, `site_grade`: its operation's kind row (`Operation::effects`) met with what the
//! settled facts prove of the site (`facts_grade`). The fold reads the grade's leave (`Effects::can_leave`),
//! and the lowerings read the same grade per pc (`Signatures::site_effects`), so a site is graded once.
//! The row rule is the kind row alone; the facts only clear what they prove away.
//!
//! This is Kleisli composition in an effect-graded monad: a call's signature is the composition of
//! its callees' signatures in the caller's sequence of steps. The signatures are retired by the same
//! edges as the facts they read (`AotFacts`), so a dependency that retires a fact retires the
//! signature composed over it.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, InvocationFormsExt};
use crate::bytecode::{Constant, FieldBase, FieldLookup, Operand, OperandKind, Register, ResidualProgram, Traits, WideInstruction};
use crate::staged::effects::{call_form, CallForm, ClassWrite, Effects, ElementWrites, Signature, Step};
use crate::staged::{Callee, DecodeContext, Invocation, NumberOperation, Operation};
use crate::typed_ir::{Cell, Classes, ElementClass, FactKey, Type};

use super::aot_facts::AotFacts;

/// What a call passes its callee, as the caller holds it: the element class of the receiver and of each argument, and the
/// caller's own parameter each argument is (`Origin::Parameter`), which an index the callee writes is read through.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallSite {
    pub receiver: ElementClass,
    pub arguments: Box<[Argument]>,
}

/// One argument of a call: its array's class, and the caller's parameter it is on entry, if it is one.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Argument {
    pub class: ElementClass,
    pub parameter: Option<u16>,
}

/// The signature of each function of a program, by function id, the targets of each call site the solve
/// read (a call's signature is the join over those targets), and the graded effects of every pc.
pub struct Signatures {
    functions: Vec<Signature>,
    /// `(function, pc)` of a call → the functions it may reach, empty where none is known.
    calls: std::collections::BTreeMap<(u32, u32), Box<[u32]>>,
    /// Per function, per pc: the graded effects of the step there (`Signatures::site_effects`).
    sites: Vec<Box<[Effects]>>,
}

/// One instruction of a function, as the solve reads it once: its graded effect row, or a call and the
/// functions it may reach.
enum Body {
    Row { effects: Effects },
    Call { pc: u32, targets: Box<[u32]>, site: Option<CallSite> },
}

/// The signature of `callee` as the call `site` makes it: each class the callee writes that is one of its parameters or
/// its receiver is the class the call passes there, and each index is read through the arguments' own parameters
/// (`Indices::substitute`). Without a site, the callee's writes are any writes.
fn lift(callee: u32, signature: Signature, site: Option<&CallSite>) -> Signature {
    let elements = match site {
        None => ElementWrites::Any,
        Some(site) => {
            let argument = |index: u16| site.arguments.get(usize::from(index)).copied();
            signature.effects.elements.map(|write| {
                let class = match write.class {
                    ElementClass::Parameter { function, index } if function == callee => argument(index).map_or(ElementClass::Unnamed, |passed| passed.class),
                    ElementClass::Receiver(function) if function == callee => site.receiver,
                    other => other,
                };
                ClassWrite { class, indices: write.indices.substitute(&|index: u16| argument(index).and_then(|passed| passed.parameter)) }
            })
        }
    };
    Signature { effects: Effects { elements, ..signature.effects }, ..signature }
}

impl Body {
    /// The step this instruction is in a pass, given the signatures the pass before found: a call's step is its callees'
    /// signatures joined, each lifted to the call's site (`lift`).
    fn step(&self, functions: &[Signature]) -> Step {
        match self {
            Self::Row { effects } => Step::Row { effects: *effects },
            Self::Call { targets, .. } if targets.is_empty() => Step::Call(Signature::ANY),
            Self::Call { targets, site, .. } => Step::Call(targets.iter().fold(Signature::NEVER, |joined, &target| {
                let callee = functions.get(target as usize).copied().unwrap_or(Signature::ANY);
                joined.join(lift(target, callee, site.as_ref()))
            })),
        }
    }

    /// The graded effects of this site under the signatures the solve found: a row's grade, or the effects of the
    /// callee signature a call's step stands for.
    fn effects(&self, functions: &[Signature]) -> Effects {
        match self.step(functions) {
            Step::Row { effects } => effects,
            Step::Call(callee) => callee.effects,
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

/// The classes of the object a named field access on `base` at `site` reads or stores, where the object is an
/// object (`DIRECT_OBJECT_FAMILY`, so never nullish) and the solve lists its classes as a closed set
/// (`access_classes`). None where either fails: the object may be nullish or of a class the solve does not list.
fn closed_classes(site: Site<'_>, base: FieldBase) -> Option<Classes> {
    let object = object_cell(site, base)?;
    let classes = site.facts.function_facts(site.function).access_classes(site.pc);
    (object.ty.is_subtype(Type::DIRECT_OBJECT_FAMILY) && matches!(classes, Classes::Set { .. })).then_some(classes)
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

/// The grade of a named field read whose object's closed classes are `classes` (`closed_classes`). Every listed
/// class holding the field as a settled cell makes the read a plain data read: it runs no guest code and throws
/// nothing (`RUNS_NOTHING`). Any other read is open (`ANY`): a class without the field, or an accessor the solve
/// has no key for, may run a getter.
fn field_read_grade(classes: Option<Classes>, holds: impl Fn(u32) -> bool) -> Effects {
    match classes {
        Some(classes @ Classes::Set { .. }) if classes.ids().iter().all(|&class| holds(class)) => Effects::RUNS_NOTHING,
        _ => Effects::ANY,
    }
}

/// The grade of a named store of `atom` on `base`: a store that writes a key new code can move
/// (`store_moves_live_key`) is open; one on a closed receiver writes a slot its classes' layouts hold, so it
/// runs nothing (`RUNS_NOTHING`); any other store is open.
fn store_grade(site: Site<'_>, base: FieldBase, atom: u32) -> Effects {
    if store_moves_live_key(site, atom) || closed_classes(site, base).is_none() { Effects::ANY } else { Effects::RUNS_NOTHING }
}

/// The grade of an operation over `operands` that is a Number operation: `RUNS_NOTHING` where every operand is a
/// Number at `site`, since its miss edge is then never taken; `ANY` otherwise.
fn numeric_grade(site: Site<'_>, operands: &[Operand]) -> Effects {
    if numeric_operands(site, operands) { Effects::RUNS_NOTHING } else { Effects::ANY }
}

/// The grade of an element store: its array's class, and the indices it touches, where the solve kept the store
/// (`FunctionFacts::element_store`); any array otherwise. Its flags are the kind row's (`ANY`), so the grade narrows
/// only the classes it writes.
fn element_store_grade(site: Site<'_>) -> Effects {
    match site.facts.function_facts(site.function).element_store(site.pc) {
        Some(store) => Effects { elements: ElementWrites::of([ClassWrite { class: store.class, indices: store.indices }]), ..Effects::ANY },
        None => Effects::ANY,
    }
}

/// The grade of an element read `base[key]`: with no index accessor in the closed world (`AotFacts::index_accessors_absent`),
/// a Number key converts to no guest code (`lengths::transfer::element_runs_guest_code`), and a base that is never nullish throws
/// nothing, so the read runs nothing (`RUNS_NOTHING`). A hole reads `undefined` through the prototype chain, which holds
/// no index accessor, so the read's bounds do not change its grade. Any other read is open (`ANY`).
fn element_read_grade(site: Site<'_>, base: Register, key: Operand) -> Effects {
    let function = site.facts.function_facts(site.function);
    let never_nullish = function.cell_at(site.pc, base).is_some_and(|cell| !cell.ty.may_be_languages(Type::NULLISH));
    if function.index_accessors_absent() && numeric_operand(site, key) && never_nullish { Effects::RUNS_NOTHING } else { Effects::ANY }
}

/// What the settled facts prove of the operation `op` at `site`, as a grade for `Effects::meet`. Each arm names
/// the one fact that clears the operation's flags; an operation no arm names is `ANY`, so the kind row stands.
///
/// 1. A store (`SetField`, `StoreField`) is graded by `store_grade`.
/// 2. A named field read is graded by `field_read_grade` over the closed classes of its object.
/// 3. A Number operation, a numeric `Binary`, `Equality` or `CompareJump` is graded by `numeric_grade`.
/// 4. An element store names its array's class and indices where the solve found them (`element_store_grade`): it
/// narrows the arrays it may write and clears no flag.
/// 5. An element read is graded by `element_read_grade`: it runs nothing where no index accessor exists and its key is a Number.
fn facts_grade(op: &Operation<(), ()>, site: Site<'_>) -> Effects {
    match op {
        Operation::ElementWrite { .. } => element_store_grade(site),
        Operation::ElementRead { base, key, .. } => element_read_grade(site, *base, *key),
        Operation::SetField { base, atom, .. } | Operation::StoreField { base, atom, .. } => {
            store_grade(site, (*base).map_or(FieldBase::THIS, FieldBase::register), *atom)
        }
        Operation::GetField { lookup: FieldLookup::Atom { base, atom, .. }, .. } => {
            field_read_grade(closed_classes(site, *base), |class| site.facts.cell(FactKey::Field { class, instance: true, atom: *atom }).is_some())
        }
        Operation::Binary { left, right, operator, .. } if numeric_operator(*operator) => numeric_grade(site, &[*left, *right]),
        Operation::Equality { left, right, .. } | Operation::CompareJump { left, right, .. } => numeric_grade(site, &[*left, *right]),
        Operation::Number(number) => numeric_grade(site, &number_operands(number)),
        _ => Effects::ANY,
    }
}

/// The graded effects of the operation `op` at `site`: its kind row, the top, met with what the facts prove of it
/// (`facts_grade`). The one grading: the fold's leave and the per-site grades both read it.
fn site_grade(op: &Operation<(), ()>, site: Site<'_>) -> Effects {
    op.effects().meet(facts_grade(op, site))
}

/// The step of `instruction` at `site` that is not a call: its graded effect row. An instruction no row decodes
/// may do anything.
fn row(instruction: WideInstruction, site: Site<'_>) -> Body {
    let decode = DecodeContext::of(site.program);
    match Operation::<(), ()>::decode_completed(instruction, decode)
        .or_else(|| Operation::<(), ()>::decode_literal_completed_with(instruction, None, ()))
        .map(|(operation, _)| operation)
    {
        Some(operation) => Body::Row { effects: site_grade(&operation, site) },
        None => Body::Row { effects: Effects::ANY },
    }
}

/// The steps of `program`'s function `function`, in code order. A field access that runs an accessor is a call of the
/// accessor's arms (`AotFacts::accessor_arms`), so its step is their composed signature rather than its kind row.
fn bodies(program: &ResidualProgram, facts: &AotFacts, function: u32) -> Vec<Body> {
    let decode = DecodeContext::of(program);
    program.with_record(function as usize, |record| {
        record.instructions().enumerate().map(|(pc, instruction)| {
            let pc = pc as u32;
            match Invocation::of(instruction, decode) {
                Some(invocation) => Body::Call { pc, targets: reached(facts, function, pc, &invocation, instruction),
                    site: facts.function_facts(function).call_site(pc).cloned() },
                None if facts.function_facts(function).accessor_arms(pc).is_some() => Body::Call {
                    pc,
                    targets: facts.function_facts(function).call_targets(pc).into_boxed_slice(),
                    site: facts.function_facts(function).call_site(pc).cloned(),
                },
                None => row(instruction, Site { program, facts, function, pc }),
            }
        }).collect()
    })
}

/// Solves the signatures of every function of `program` over `facts`: each pass sets every
/// function's signature to the fold of its steps against the signatures the pass before found,
/// until a pass changes nothing. The per-site grades are read once the signatures have settled.
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
    let sites: Vec<Box<[Effects]>> = bodies.iter()
        .map(|body| body.iter().map(|step| step.effects(&functions)).collect::<Box<[Effects]>>())
        .collect();
    let calls = bodies.iter().enumerate().flat_map(|(function, body)| body.iter().filter_map(move |step| match step {
        Body::Call { pc, targets, .. } => Some(((function as u32, *pc), targets.clone())),
        Body::Row { .. } => None,
    })).collect();
    Signatures { functions, calls, sites }
}

impl Signatures {
    /// The signature of function `function`; top for an id the program does not hold.
    pub fn of(&self, function: u32) -> Signature {
        self.functions.get(function as usize).copied().unwrap_or(Signature::ANY)
    }

    /// The graded effects of the step at `pc` of `function`: its operation's kind row met with what the settled facts
    /// prove of the site, or for a call the effects of the callees it may reach. Top for a site the program does not hold.
    pub fn site_effects(&self, function: u32, pc: u32) -> Effects {
        self.sites.get(function as usize).and_then(|sites| sites.get(pc as usize)).copied().unwrap_or(Effects::ANY)
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

    /// The kind row of a named read on the register `base`, as the lowering decodes it.
    fn named_read(base: u16) -> Operation<(), ()> {
        Operation::GetField { base: FieldBase::register(base), lookup: FieldLookup::Atom { atom: 0, base: FieldBase::register(base), cache_site: 0 }, destination: 0 }
    }

    #[test]
    fn a_proven_plain_read_does_not_clear_stores() {
        let kind = named_read(1).effects();
        assert!(kind.may_call_js, "a named read's kind row may run a getter");
        let graded = kind.meet(field_read_grade(Some(Classes::one(7, true)), |_| true));
        assert!(!graded.may_call_js && !graded.may_throw && !graded.can_leave());
        assert_eq!(graded.reads, kind.reads, "a plain read still reads the object's slots");
    }

    #[test]
    fn an_open_receiver_or_a_class_without_the_field_keeps_the_read_open() {
        let kind = named_read(1).effects();
        assert!(kind.meet(field_read_grade(None, |_| true)).may_call_js, "an open receiver may run a getter");
        assert!(kind.meet(field_read_grade(Some(Classes::one(7, true)), |_| false)).may_call_js, "a class without the field may run a getter");
        assert!(kind.meet(field_read_grade(Some(Classes::Many), |_| true)).may_call_js, "an unlisted class may run a getter");
    }

    #[test]
    fn a_grade_only_removes_effects_from_its_kind_row() {
        let kind = Effects::ANY;
        assert_eq!(kind.meet(Effects::ANY), kind);
        assert_eq!(writes_frame().meet(Effects::RUNS_NOTHING), writes_frame());
        assert!(!may_throw().meet(Effects::RUNS_NOTHING).may_throw);
        assert_eq!(Effects::NONE.meet(Effects::ANY), Effects::NONE);
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

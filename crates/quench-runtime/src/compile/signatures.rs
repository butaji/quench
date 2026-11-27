//! Composed effect signatures per function, solved over the whole program. A function's signature
//! joins its own steps, in code order, with the signature of every callee its call sites may reach
//! (`AotFacts::function_facts(f).call_targets(pc)`); a call with no target the facts name joins
//! `Signature::ANY`. The solve starts every function at `Signature::NEVER` and repeats until a pass
//! changes nothing, which settles because the lattice is finite and each pass is monotone in the
//! signatures before it.
//!
//! This is Kleisli composition in an effect-graded monad: a call's signature is the composition of
//! its callees' signatures in the caller's sequence of steps. The signatures are retired by the same
//! edges as the call-target facts they read (`AotFacts`), so a dependency that retires a target
//! retires the signature composed over it.

#[cfg(not(stencil_build))]
#[allow(unused_imports)] use crate::staged::{OperationOperationExt, InvocationFormsExt};
use crate::bytecode::{ResidualProgram, Traits, WideInstruction};
use crate::staged::effects::{call_form, CallForm, Effects, Signature, Step};
use crate::staged::{Callee, DecodeContext, Invocation, Operation};

use super::aot_facts::AotFacts;

/// The signature of each function of a program, by function id, and the targets of each call site
/// the solve read: a call's signature is the join over those targets.
pub struct Signatures {
    functions: Vec<Signature>,
    /// `(function, pc)` of a call → the functions it may reach, empty where none is known.
    calls: std::collections::BTreeMap<(u32, u32), Box<[u32]>>,
}

/// One instruction of a function, as the solve reads it once: its own effect row, or a call and the
/// functions it may reach.
enum Body {
    Row(Effects),
    Call { pc: u32, targets: Box<[u32]> },
}

impl Body {
    /// The step this instruction is in a pass, given the signatures the pass before found.
    fn step(&self, functions: &[Signature]) -> Step {
        match self {
            Self::Row(effects) => Step::Row(*effects),
            Self::Call { targets, .. } if targets.is_empty() => Step::Call(Signature::ANY),
            Self::Call { targets, .. } => Step::Call(targets.iter().fold(Signature::NEVER,
                |joined, &target| joined.join(functions.get(target as usize).copied().unwrap_or(Signature::ANY)))),
        }
    }
}

/// The effect row of `instruction`, decoded as the closed-world analysis decodes it (`effects::decoded`);
/// an instruction no row decodes may do anything.
fn row(instruction: WideInstruction, decode: DecodeContext<'_>) -> Effects {
    Operation::<(), ()>::decode_completed(instruction, decode)
        .or_else(|| Operation::<(), ()>::decode_literal_completed_with(instruction, None, ()))
        .map_or(Signature::ANY.effects, |(operation, _)| operation.effects())
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

/// The steps of `program`'s function `function`, in code order.
fn bodies(program: &ResidualProgram, facts: &AotFacts, function: u32) -> Vec<Body> {
    let decode = DecodeContext::of(program);
    program.with_record(function as usize, |record| {
        record.instructions().enumerate().map(|(pc, instruction)| {
            match Invocation::of(instruction, decode) {
                Some(invocation) => {
                    let pc = pc as u32;
                    Body::Call { pc, targets: reached(facts, function, pc, &invocation, instruction) }
                }
                None => Body::Row(row(instruction, decode)),
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
        Body::Row(_) => None,
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

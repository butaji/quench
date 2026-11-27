//! The word computations of a lane body's call (`vm/dispatch_fast/body_calls.rs`) as staged rows:
//! the `this` a body's callee is entered with, and the activation a body's call makes of its target.
//! The interpreter runs them on machine words (`Concrete`); the rows are pure and branch-free, so
//! a tier that emits them computes the same answer.

use super::Stage;
use crate::Value;

/// The `this` a lane body's callee is entered with (`body_calls::lane_body_this`): the deleted
/// marker where the callee binds `this` lexically or the receiver already is the marker, else the
/// receiver as it is.
#[inline(always)]
pub(crate) fn body_this<S: Stage>(stage: &mut S, receiver: S::Word, lexical: S::Cond) -> S::Word {
    let deleted = stage.eq_imm(receiver, Value::DELETED.to_bits());
    let marked = stage.any(lexical, deleted);
    let marker = stage.constant(Value::DELETED.to_bits());
    stage.select(marked, marker, receiver)
}

/// `body_this` on machine words, for the interpreter's body calls.
#[inline(always)]
pub fn concrete_body_this(receiver: u64, lexical: bool) -> u64 {
    body_this(&mut super::Concrete::pure(), receiver, lexical)
}

/// Which activation a lane body's call makes of its target, as three exclusive conditions (`none`
/// where all are false): `plain`, the registers alone; `keeping`, by the keeping form; `environment`,
/// by the environment form.
#[derive(Clone, Copy)]
pub struct CallActivation<C> {
    pub plain: C,
    pub keeping: C,
    pub environment: C,
}

/// The activation of a call whose target's entry `activates` or `forwards`, passed the arguments
/// that `fixed` admits (at most its fixed copies), and whose entry is `environment`'s. A plain
/// activation needs the entry to activate and `fixed`; a keeping one takes any other activating
/// or forwarding entry; the environment form takes the rest.
#[inline(always)]
pub(crate) fn activation<S: Stage>(stage: &mut S, activates: S::Cond, forwards: S::Cond, fixed: S::Cond, environment: S::Cond)
    -> CallActivation<S::Cond> {
    let plain = stage.all(activates, fixed);
    let not_plain = stage.not(plain);
    let either = stage.any(activates, forwards);
    let keeping = stage.all(not_plain, either);
    let not_keeping = stage.not(keeping);
    let rest = stage.all(not_plain, not_keeping);
    let environment = stage.all(rest, environment);
    CallActivation { plain, keeping, environment }
}

/// `activation` on machine words, for the interpreter's body calls.
#[inline(always)]
pub fn concrete_activation(activates: bool, forwards: bool, fixed: bool, environment: bool) -> CallActivation<bool> {
    activation(&mut super::Concrete::pure(), activates, forwards, fixed, environment)
}

/// Whether a lane body's call admits a target the VM resolved (`resolve_script_call_target`):
/// a callee of the running program, whose records the body's view reads; another program's
/// callee takes the crossing call instead. Reads the two program words; writes nothing.
#[inline(always)]
pub(crate) fn admits_target_program<S: Stage>(stage: &mut S, target_program: S::Word, running_program: S::Word) -> S::Cond {
    let differs = stage.sub(target_program, running_program);
    stage.eq_imm(differs, 0u64)
}

/// `admits_target_program` on machine words.
#[inline(always)]
pub fn concrete_admits_target_program(target_program: u32, running_program: u32) -> bool {
    admits_target_program(&mut super::Concrete::pure(), u64::from(target_program), u64::from(running_program))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_body_call_admits_a_target_of_its_own_program() {
        assert!(concrete_admits_target_program(3, 3));
        assert!(!concrete_admits_target_program(3, 4));
    }

    #[test]
    fn a_lexical_or_marked_this_is_the_deleted_marker() {
        assert_eq!(concrete_body_this(7, true), Value::DELETED.to_bits());
        assert_eq!(concrete_body_this(Value::DELETED.to_bits(), false), Value::DELETED.to_bits());
        assert_eq!(concrete_body_this(7, false), 7);
    }

    #[test]
    fn an_activating_entry_within_its_fixed_copies_is_plain_else_keeping_else_environment() {
        let plain = concrete_activation(true, false, true, false);
        assert!(plain.plain && !plain.keeping && !plain.environment);
        let keeping = concrete_activation(true, false, false, false);
        assert!(!keeping.plain && keeping.keeping && !keeping.environment);
        let forwarded = concrete_activation(false, true, true, false);
        assert!(!forwarded.plain && forwarded.keeping);
        let environment = concrete_activation(false, false, false, true);
        assert!(!environment.plain && !environment.keeping && environment.environment);
        let none = concrete_activation(false, false, true, false);
        assert!(!none.plain && !none.keeping && !none.environment);
    }
}

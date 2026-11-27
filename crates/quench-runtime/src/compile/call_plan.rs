//! The call plan of one call site: the one call-protocol fact every tier lowers. A plan names the
//! functions a call reaches as arms, one per class of a closed receiver union (`Classes::Set`) or one
//! for a single target, and the form the call takes (`staged::effects::CallForm`). The interpreter,
//! the BaseJIT, the unit JIT and AOT all lower the same plan, so a site's dispatch is decided once, from
//! the facts, and no tier re-derives it: a closed receiver union is a switch on the class word with a
//! direct call or an inlined body per arm (richards' `this.task.run(packet)`: four arms), and a closed
//! world has no miss path at all, while an open one keeps one subscription to the key that would move
//! (`Miss::Edge`) in place of a compare-and-fallback chain. Owner: call protocol (R5, TOD-2447).

use std::collections::BTreeMap;

use crate::bytecode::{Register, ResidualProgram, Traits};
use crate::staged::effects::{call_form, CallForm, Signature};
use crate::staged::{Callee, DecodeContext, Invocation};
use crate::typed_ir::{Cell, Classes, FactKey};

use super::aot_facts::AotFacts;
use super::signatures::Signatures;

/// One arm of a call plan: the function a call reaches for one class of its receiver's closed union
/// (`class: Some`), or the single function a call with one target reaches (`class: None`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Arm {
    pub class: Option<u32>,
    pub function: u32,
}

/// What a plan does when its receiver or target falls outside its arms.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Miss {
    /// No key the plan read is live: new code cannot move the arms, so the site has no miss path at all.
    None,
    /// The key whose subscription replaces the compare-and-fallback: while the key is live, the code that
    /// dispatches on the arms subscribes to it, and an edge that moves it retires that code.
    Edge(FactKey),
}

/// The plan of one call site: its form, its arms and its miss path. A tier lowers the call from the plan
/// alone: a switch on the receiver's class word with one arm per class when `arms` has classes, else the
/// one arm's direct call or inlined body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CallPlan {
    /// How the call is lowered (`staged::effects::call_form`): inlined when every arm is inlinable, else by
    /// the joined signature of the arms.
    pub form: CallForm,
    pub arms: Box<[Arm]>,
    pub miss: Miss,
}

/// The arms of a closed receiver union: each class in `ids` paired with the function `body` names for it
/// (its settled method cell). None where a class has no single body, so the site keeps its own path.
fn class_arms(ids: &[u32], body: impl Fn(u32) -> Option<u32>) -> Option<Box<[Arm]>> {
    ids.iter().map(|&class| body(class).map(|function| Arm { class: Some(class), function })).collect()
}

/// The miss of a plan that read `keys`: the first key new code can move, or none is live.
fn miss_of(keys: &[FactKey], live: impl Fn(FactKey) -> bool) -> Miss {
    keys.iter().copied().find(|&key| live(key)).map_or(Miss::None, Miss::Edge)
}

/// The form of a call with `arms`: inlined when every arm is inlinable, else lowered by the join of the arms'
/// signatures (`call_form`, never inlined). A plan's arms are the only targets, so their join is the callee.
fn form_of(arms: &[Arm], inlinable: impl Fn(u32) -> bool, signature: impl Fn(u32) -> Signature) -> CallForm {
    if arms.iter().all(|arm| inlinable(arm.function)) {
        return CallForm::Inline;
    }
    let callee = arms.iter().fold(Signature::NEVER, |joined, arm| joined.join(signature(arm.function)));
    call_form(callee, false)
}

/// The plan of a call with the one target `function` and the miss `miss` (`Miss::None` where no key the call
/// reads is live): its one arm, and its form (`form_of`).
fn single_target(function: u32, miss: Miss, signatures: &Signatures, inlinable: impl Fn(u32) -> bool) -> CallPlan {
    let arms = vec![Arm { class: None, function }].into_boxed_slice();
    let form = form_of(&arms, inlinable, |function| signatures.of(function));
    CallPlan { form, arms, miss }
}

/// The one closure body a call reaches when its callee holds `cell` (`Type::body_index`), with the miss of the
/// keys the callee was read from (`miss_of`): a live key the call subscribes to. None where `cell` names no single
/// body, so the site keeps its own path.
fn target_of(cell: Cell, keys: &[FactKey], live: impl Fn(FactKey) -> bool) -> Option<(u32, Miss)> {
    let function = cell.ty.body_index()?;
    Some((function, miss_of(keys, live)))
}

/// The plan of the method call at `pc` of `caller` that names `atom`. Its receiver's classes (`access_classes`)
/// must be a closed union: one arm per class, each the body of that class's own method `atom` (`FactKey::Field`),
/// and a miss on the first of those fields the world keeps live. A union with a class that has no single body,
/// or a receiver of no closed union, keeps the site's own path.
fn method_plan(facts: &AotFacts, signatures: &Signatures, caller: u32, pc: u32, atom: u32, inlinable: impl Fn(u32) -> bool) -> Option<CallPlan> {
    let classes = facts.function_facts(caller).access_classes(pc);
    if !matches!(classes, Classes::Set { .. }) {
        return None;
    }
    let ids = classes.ids();
    let arms = class_arms(ids, |class| facts.cell(FactKey::Field { class, instance: true, atom })?.ty.body_index())?;
    let keys: Vec<FactKey> = ids.iter().map(|&class| FactKey::Field { class, instance: true, atom }).collect();
    let miss = miss_of(&keys, |key| facts.live(key));
    let form = form_of(&arms, &inlinable, |function| signatures.of(function));
    Some(CallPlan { form, arms, miss })
}

/// The plan of the call at `pc` of `caller` whose callee is in `register`, or None where the site keeps its own
/// path. Three facts name its one target, in order:
///
/// - the key the callee was read from (`call_key`): the key's settled cell names the closure body, and the miss is
///   that key while it is live (a store that may write another closure there);
/// - else the register's settled cell at `pc` (`cell_at`) names one body: nothing the call reads is a key, so no miss;
/// - else the census's proof of the call (`proven_call_target`).
fn register_plan(facts: &AotFacts, signatures: &Signatures, caller: u32, pc: u32, register: Register, inlinable: impl Fn(u32) -> bool) -> Option<CallPlan> {
    let site = facts.function_facts(caller);
    let live = |key: FactKey| facts.live(key);
    if let Some((_, key)) = site.call_key(pc) {
        let (function, miss) = target_of(facts.cell(key)?, &[key], live)?;
        return Some(single_target(function, miss, signatures, &inlinable));
    }
    if let Some((function, miss)) = site.cell_at(pc, register).and_then(|cell| target_of(cell, &[], live)) {
        return Some(single_target(function, miss, signatures, &inlinable));
    }
    let &[function] = site.call_targets(pc).as_slice() else { return None };
    if site.proven_call_target(pc) != Some(function) {
        return None;
    }
    Some(single_target(function, Miss::None, signatures, &inlinable))
}

/// The plans of every call `function` makes, by pc. A known function's call and a construction (`new`) with
/// its one constructed function are proven single targets (`Miss::None`); a method call is planned by its name
/// (`Callee::Method`, `method_plan`); a register callee by the target its key or its cell names (`register_plan`).
/// A call no plan covers is absent from the map.
pub fn call_plans(program: &ResidualProgram, facts: &AotFacts, signatures: &Signatures, function: u32, inlinable: impl Fn(u32) -> bool) -> BTreeMap<u32, CallPlan> {
    let decode = DecodeContext::of(program);
    let calls: Vec<(u32, bool, Invocation)> = program.with_record(function as usize, |record| {
        record.instructions().enumerate().filter_map(|(pc, instruction)| {
            let invocation = Invocation::of(instruction, decode)?;
            Some((pc as u32, instruction.op().traits().has(Traits::CONSTRUCTION), invocation))
        }).collect()
    });
    calls.into_iter().filter_map(|(pc, construct, invocation)| {
        let site_plan = if construct {
            facts.function_facts(function).construct_target(pc).map(|target| single_target(target, Miss::None, signatures, &inlinable))
        } else {
            match invocation.callee {
                Callee::Known(known) => Some(single_target(u32::from(known), Miss::None, signatures, &inlinable)),
                Callee::Method { atom, .. } => method_plan(facts, signatures, function, pc, atom, &inlinable),
                Callee::Register(register) => register_plan(facts, signatures, function, pc, register, &inlinable),
            }
        };
        site_plan.map(|site_plan| (pc, site_plan))
    }).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::typed_ir::Type;

    #[test]
    fn a_closed_union_of_four_classes_is_four_arms() {
        let union = [9u32, 10, 11, 12].into_iter().map(|class| Classes::one(class, false)).fold(Classes::Never, Classes::join);
        assert_eq!(union.ids(), &[9, 10, 11, 12]);
        let arms = class_arms(union.ids(), |class| Some(class * 10 + 1)).expect("every class has a body");
        assert_eq!(arms.len(), 4);
        assert_eq!(arms[0], Arm { class: Some(9), function: 91 });
        assert_eq!(arms[3], Arm { class: Some(12), function: 121 });
    }

    #[test]
    fn a_class_without_a_single_body_keeps_the_site_without_a_plan() {
        assert!(class_arms(&[3, 4], |class| (class == 3).then_some(30)).is_none());
    }

    #[test]
    fn a_union_past_the_closed_limit_is_many_and_has_no_arms() {
        let open = (1u32..=5).map(|class| Classes::one(class, true)).fold(Classes::Never, Classes::join);
        assert_eq!(open, Classes::Many);
        assert!(open.ids().is_empty());
    }

    #[test]
    fn miss_is_the_first_live_key_or_none() {
        let keys = [FactKey::Field { class: 9, instance: true, atom: 4 }, FactKey::Field { class: 10, instance: true, atom: 4 }];
        assert_eq!(miss_of(&keys, |_| false), Miss::None);
        assert_eq!(miss_of(&keys, |key| key == keys[1]), Miss::Edge(keys[1]));
        assert_eq!(miss_of(&keys, |_| true), Miss::Edge(keys[0]));
        assert_eq!(miss_of(&[], |_| true), Miss::None);
    }

    #[test]
    fn inline_when_every_arm_is_inlinable_else_the_joined_signature_decides() {
        let arms = [Arm { class: Some(9), function: 1 }, Arm { class: Some(10), function: 2 }];
        assert_eq!(form_of(&arms, |_| true, |_| Signature::ANY), CallForm::Inline);
        assert_eq!(form_of(&arms, |function| function == 1, |_| Signature::NEVER), CallForm::Frameless);
        assert_eq!(form_of(&arms, |_| false, |function| if function == 2 { Signature::ANY } else { Signature::NEVER }), CallForm::Framed);
    }

    #[test]
    fn a_keyed_call_names_its_cells_body_and_subscribes_to_the_key_while_live() {
        let key = FactKey::Global(5);
        let cell = Cell::of(Type::function_body(7));
        assert_eq!(target_of(cell, &[key], |_| true), Some((7, Miss::Edge(key))));
        assert_eq!(target_of(cell, &[key], |_| false), Some((7, Miss::None)));
    }

    #[test]
    fn a_register_callee_with_no_key_names_its_cells_body_with_no_miss() {
        assert_eq!(target_of(Cell::of(Type::function_body(7)), &[], |_| true), Some((7, Miss::None)));
        assert_eq!(target_of(Cell::of(Type::INT32), &[], |_| true), None);
        assert_eq!(target_of(Cell::ANY, &[], |_| true), None);
    }

    #[test]
    fn a_single_target_plan_names_no_key() {
        let arms = vec![Arm { class: None, function: 7 }].into_boxed_slice();
        let form = form_of(&arms, |_| false, |_| Signature::NEVER);
        let plan = CallPlan { form, arms, miss: Miss::None };
        assert_eq!(plan.form, CallForm::Frameless);
        assert_eq!(plan.arms[0].class, None);
        assert_eq!(plan.miss, Miss::None);
    }
}

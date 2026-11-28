//! The identity facts of the closed world: values that are one object in every run, so that a lane body need not compare
//! their identity in execution. Each is an edge fact, proven once from the settled solve, and holds only in `World::Closed`
//! (`analyze` keeps a closed solve only once `gate::verify` holds, so a fact here is never read from an unverified solve).
//!
//! - Builtins (`BuiltinPath`): a global binding, or a member of one, that the closed world never writes, so it holds the
//!   realm's initial builtin. The proofs are `quiet::Proofs`, the one reading of intrinsic builtins the quiet points use.
//! - Closures (`closures_once`): a body whose closure is created at most once per run, so every value of it is one object.
//! - Receivers (`this_undefined`): a function every call of which passes an undefined receiver, so its raw `this` is
//!   `undefined` at entry.

use std::collections::{BTreeMap, BTreeSet};

use super::census::Census;
use super::escapes::{self, Escapes};
use super::flow::FunctionFlow;
use super::gate::{self, World};
use super::quiet::Proofs;
use super::types;
use super::AotFacts;
use crate::bytecode::{Op, ResidualProgram, may_repeat};
use crate::typed_ir::{Cell, Classes, Identity};

/// The identity a cell holds once the closed world settles it: a builtin or a closure the proofs keep stays (it is the proof the
/// lane reads, `Cell::identity`); any other proposed builtin or closure is `Many`. Applied to every settled register and fact cell
/// after the proofs above are built, so an `Identity` a lane reads is always a proven one.
pub(super) fn proven(facts: &AotFacts, cell: Cell) -> Cell {
    let identity = match cell.identity {
        Identity::Builtin(path) if !facts.builtin_identity(path) => Identity::Many,
        Identity::Closure(body) if !facts.closure_identity(body) => Identity::Many,
        other => other,
    };
    Cell { identity, ..cell }
}

/// Names whose presence lets a builtin's property change with no shape the flows record: `Reflect.set` and its kin store
/// through a builtin that no `Shape` names, so a program naming `Reflect` leaves no builtin proven.
const REFLECTIVE_NAMES: [&str; 1] = ["Reflect"];
/// The builtins that define or store properties on the objects they are given (`Scope::adds_properties`). A call of one
/// through a callee that names none (an alias, or `call`, `apply`, `bind`) records no shape, so a program naming one
/// treats every call handed an object that may be a builtin as a define.
const DEFINERS: [&str; 3] = ["defineProperty", "defineProperties", "assign"];

/// Whether the program names `text`.
fn names(program: &ResidualProgram, text: &str) -> bool {
    program.atoms.iter().any(|atom| {
        let atom: &str = atom.as_ref();
        atom == text
    })
}

/// The proofs of the builtins the closed world leaves as the host installed them (`AotFacts::builtin_identity`): `Proofs`
/// over the settled flows. None where the solve is not closed, or the program names a writer the shapes do not record.
pub(super) fn builtin_proofs(program: &ResidualProgram, world: World, flows: &[FunctionFlow]) -> Option<Proofs> {
    if world != World::Closed || REFLECTIVE_NAMES.iter().any(|name| names(program, name)) {
        return None;
    }
    let definers_named = DEFINERS.iter().any(|name| names(program, name));
    let unshaped_definer = definers_named && flows.iter().any(|flow| flow.calls.iter().any(|call| {
        let named_definer = escapes::method_atom(call).is_some_and(|atom| {
            let text: &str = program.atoms[atom as usize].as_ref();
            DEFINERS.contains(&text)
        });
        !named_definer && call.arguments.iter().any(|argument| argument.cell.classes == Classes::Many || argument.cell.global)
    }));
    (!unshaped_definer).then(|| Proofs::of(flows))
}

/// The bodies whose closure is created at most once per run (`AotFacts::closure_identity`): a body with exactly one
/// `MakeClosure` site in the whole program, where that site lies in the root's own code outside every loop of the root.
/// The root runs once, so the site runs at most once and every value of the closure is that one object. A function
/// declaration hoisted at program level is such a site. A body created in a nested function, or in a loop, is not.
pub(super) fn closures_once(program: &ResidualProgram, world: World) -> BTreeSet<u32> {
    if world != World::Closed {
        return BTreeSet::new();
    }
    // Per body, its creation sites as (created by the root, pc).
    let mut sites: BTreeMap<u32, Vec<(bool, usize)>> = BTreeMap::new();
    let mut root_backward = Vec::new();
    for id in 0..program.function_count() {
        let (top, created, backward) = program.with_record(id, |function| {
            let top = function.parent.is_none();
            let created: Vec<(usize, u32)> = (0..function.code.len())
                .filter_map(|pc| {
                    let instruction = function.instruction_at(pc);
                    (instruction.op() == Op::MakeClosure).then(|| (pc, instruction.closure_function_index()))
                })
                .collect();
            let backward = if top { function.backward_transfers() } else { Vec::new() };
            (top, created, backward)
        });
        if top {
            root_backward = backward;
        }
        for (pc, body) in created {
            sites.entry(body).or_default().push((top, pc));
        }
    }
    sites
        .into_iter()
        .filter_map(|(body, created)| match created.as_slice() {
            [(true, pc)] if !may_repeat(&root_backward, *pc) => Some(body),
            _ => None,
        })
        .collect()
}

/// The functions every call of which passes an undefined receiver (`AotFacts::this_undefined`), over the calls of the
/// functions the closed world reaches. A function code outside the call graph may start (`gate::seeds`), or that escapes
/// (a value handed to code the solve does not follow, or to a `call`, `apply` or `bind`), may be called with any receiver,
/// so none. Empty where the solve is not closed.
pub(super) fn this_undefined(program: &ResidualProgram, world: World, flows: &[FunctionFlow], facts: &AotFacts, census: &Census, escaped: &Escapes)
    -> BTreeSet<u32> {
    if world != World::Closed {
        return BTreeSet::new();
    }
    let reached = gate::reached(program, flows, facts, census, escaped);
    let seeds: BTreeSet<u32> = gate::seeds(program, flows, census, escaped).into_iter().collect();
    // The functions a call of a reached function may pass a receiver to: any call that is not plain, or a construction.
    let mut receives: BTreeSet<u32> = BTreeSet::new();
    for (caller, flow) in flows.iter().enumerate() {
        if !reached[caller] {
            continue;
        }
        for call in flow.calls.iter().filter(|call| !call.plain || call.construct) {
            let targets = types::targets(caller as u32, call, facts, census);
            // A receiver-passing call the solve resolves no target for may pass one to any function.
            if targets.is_empty() {
                return BTreeSet::new();
            }
            receives.extend(targets);
        }
    }
    let passed_this: BTreeSet<u32> = escaped.passed_this.iter().map(|&(function, _)| function).collect();
    (0..flows.len() as u32)
        .filter(|function| {
            !receives.contains(function)
                && !seeds.contains(function)
                && !escaped.functions.contains(function)
                && !passed_this.contains(function)
        })
        .collect()
}

//! The identity facts of the closed world: values that are one object in every run, so that a lane body need not compare
//! their identity in execution. Each is an edge fact, proven once from the settled solve, and holds only in `World::Closed`
//! (`analyze` keeps a closed solve only once `gate::verify` holds, so a fact here is never read from an unverified solve).
//!
//! - Builtins (`BuiltinPath`): a global binding, or a member of one, that the closed world never writes, so it holds the
//!   realm's initial builtin. The proofs are `quiet::Proofs`, the one reading of intrinsic builtins the quiet points use.
//! - Closures (`closures_once`): a body whose closure is created at most once per run, so every value of it is one object.
//! - Receivers (`this_undefined`): a function every call of which passes an undefined receiver, so its raw `this` is
//!   `undefined` at entry.
//! - The global object (`Identity::GlobalObject`, `global_object_proof`): the root's `this` of a Script, the value of a
//!   `globalThis` read, and a sloppy function's `this` where its calls are plain. One proof holds for all three, because a
//!   cell carries no source: the program never writes `globalThis` or the global object's shape is fixed, and every sloppy
//!   function a plain call binds `this` of is one `this_undefined` names. Narrower per-source proofs would need a cell to
//!   name its source, which `Identity` does not.

use std::collections::{BTreeMap, BTreeSet};

use super::census::{self, Census};
use super::scope::Scope;
use super::escapes::Escapes;
use super::flow::{Call, FunctionFlow, Target};
use super::gate::{self, World};
use super::quiet::Proofs;
use super::types;
use super::AotFacts;
use crate::bytecode::{Op, ResidualProgram, may_repeat};
use crate::typed_ir::{Cell, FactKey, Identity};

/// The identity a cell holds once the closed world settles it: a builtin, the global object or a closure the proofs keep stays
/// (it is the proof the lane reads, `Cell::identity`); any other proposed builtin, global object or closure is `Many`. Applied
/// to every settled register and fact cell after the proofs above are built, so an `Identity` a lane reads is always a proven one.
pub(super) fn proven(facts: &AotFacts, cell: Cell) -> Cell {
    let identity = match cell.identity {
        Identity::Builtin(path) if !facts.builtin_identity(path) => Identity::Many,
        Identity::GlobalObject if !facts.global_object_identity() => Identity::Many,
        Identity::Closure(body) if !facts.closure_identity(body) => Identity::Many,
        other => other,
    };
    Cell { identity, ..cell }
}

/// Names whose presence lets a builtin's property change with no shape the flows record: `Reflect.set` and its kin store
/// through a builtin that no `Shape` names, so a program naming `Reflect` leaves no builtin proven.
const REFLECTIVE_NAMES: [&str; 1] = ["Reflect"];

/// Whether the program names `text`.
fn names(program: &ResidualProgram, text: &str) -> bool {
    program.atoms.iter().any(|atom| {
        let atom: &str = atom.as_ref();
        atom == text
    })
}

/// The proofs of the builtins the closed world leaves as the host installed them (`AotFacts::builtin_identity`): `Proofs`
/// over the settled flows. None where the solve is not closed, or the program names a writer the shapes do not record. A call
/// that may define properties on what it is handed (`may_define`) is a change to each handed value (`Proofs::handed`), where
/// `cells` are the settled fact cells before the proofs are applied, and `leaked` names the builtins a value may be.
pub(super) fn builtin_proofs(program: &ResidualProgram, world: World, flows: &[FunctionFlow], scope: &Scope, cells: &BTreeMap<FactKey, Cell>)
    -> Option<Proofs> {
    if world != World::Closed || REFLECTIVE_NAMES.iter().any(|name| names(program, name)) {
        return None;
    }
    let leaked = leaked(flows, cells, scope);
    let mut proofs = Proofs::of(flows);
    for call in flows.iter().flat_map(|flow| &flow.calls).filter(|call| may_define(scope, call)) {
        for handed in call.arguments.iter().chain(std::iter::once(&call.receiver)) {
            proofs.handed(&handed.cell, &leaked);
        }
    }
    Some(proofs)
}

/// The global names of the builtins some value of the program may be, where the closed world's proofs do not follow it. A
/// builtin leaks where its value reaches a cell that is no longer exactly that builtin: a register join (`FunctionFlow::leaked`),
/// or a store whose key's settled cell (before the proofs) holds another identity or no fact cell at all. An argument or receiver
/// of a call that runs script code or may define properties (`keeps_arguments`) leaks too, since the solve does not carry the
/// value through that call's own writes.
fn leaked(flows: &[FunctionFlow], cells: &BTreeMap<FactKey, Cell>, scope: &Scope) -> BTreeSet<u32> {
    let mut leaked: BTreeSet<u32> = flows.iter().flat_map(|flow| flow.leaked.iter().copied()).collect();
    for flow in flows {
        for &(_, key, word) in &flow.writes {
            if let Identity::Builtin(path) = word.cell.identity {
                let stored = cells.get(&census::fact_key(key)).map(|cell| cell.identity);
                if stored != Some(Identity::Builtin(path)) {
                    leaked.insert(path.global);
                }
            }
        }
        for call in flow.calls.iter().filter(|call| !keeps_arguments(scope, call)) {
            for handed in call.arguments.iter().chain(std::iter::once(&call.receiver)) {
                if let Identity::Builtin(path) = handed.cell.identity {
                    leaked.insert(path.global);
                }
            }
        }
    }
    leaked
}

/// Whether a call runs script code: the solve resolves it to a script function, or its callee is a function or a closure. Such a
/// call's flow records the shapes of the code it runs.
fn runs_script(call: &Call) -> bool {
    let callee = call.callee.cell;
    matches!(call.target, Target::Known(_)) || callee.ty.body_index().is_some() || matches!(callee.identity, Identity::Closure(_))
}

/// Whether a call neither runs script code nor may define properties on what it is given: its callee is a builtin that defines
/// none (`may_define`). Its arguments and receiver stay where they are.
fn keeps_arguments(scope: &Scope, call: &Call) -> bool {
    !runs_script(call) && !may_define(scope, call)
}

/// Whether a call may define properties on the objects it is handed, by its callee's identity. A call the solve resolves to a
/// script function, or to a closure, runs that function's own shapes, which its flow records. A callee the identity proves to
/// be a builtin that defines no property (`Scope::is_property_builtin`) defines none. Any other callee may define on what it is
/// handed (an alias of `defineProperty`, `call`, `apply`, an unknown method).
fn may_define(scope: &Scope, call: &Call) -> bool {
    if runs_script(call) {
        return false;
    }
    match call.callee.cell.identity {
        Identity::Builtin(path) => scope.is_property_builtin(path.member.unwrap_or(path.global)),
        _ => true,
    }
}

/// Whether a `GlobalObject` identity holds in every run of the closed program (`AotFacts::global_object_identity`), where the
/// settled solve has `this_undefined` as its receivers' proof. Three conditions:
/// - the global object keeps its `globalThis` binding (`Proofs::global_object_identity_fixed`): the program never writes that
///   name, and no store or define reaches a name the analysis cannot follow, so a by-name read of it is the realm's global object;
/// - every sloppy function a plain call may reach is one `this_undefined` names: its calls all pass an undefined receiver, no
///   code the solve does not follow calls it, and no other receiver reaches it, so its `this` is the global object at every entry.
/// False where the solve is not closed (no builtin proofs).
pub(super) fn global_object_proof(program: &ResidualProgram, flows: &[FunctionFlow], facts: &AotFacts, census: &Census, scope: &Scope,
    this_undefined: &BTreeSet<u32>) -> bool {
    let Some(proofs) = facts.builtins.as_ref() else { return false };
    if !proofs.global_object_identity_fixed(scope.global_this()) { return false; }
    flows.iter().enumerate().all(|(caller, flow)| flow.calls.iter().filter(|call| call.plain).all(|call| {
        types::targets(caller as u32, call, facts, census).into_iter().all(|target| {
            let sloppy = flows.get(target as usize).is_none_or(|callee| !callee.strict);
            !sloppy || this_undefined.contains(&target)
        })
    }))
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

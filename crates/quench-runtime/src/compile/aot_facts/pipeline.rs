//! The solve behind `analyze`, `analyze_as` and `world`: the facts are solved in rounds over the bytecode's flows (`flow`),
//! the census of the closures they name (`census`), the types (`types`), the composed signatures (`signatures`) and the
//! length relations (`length_settle`). Each round reads the facts the round before settled, and the solve in a world
//! keeps only the facts that world proves.

use std::collections::BTreeMap;

use super::accessors;
use super::census::{self, Census};
use super::class_ids::ClassIds;
use super::class_layout;
use super::copies;
use super::escapes;
use super::phase;
use super::flow::{self, FunctionFlow};
use super::frozen;
use super::gate::{self, World};
use super::identity;
use super::globals;
use super::length_settle;
use super::readers::Readers;
use super::scope::Scope;
use super::shapes;
use super::super::signatures::{self, Argument, CallSite};
use super::types;
use super::value;
use super::AotFacts;
use super::LENGTH_ROUNDS;
use super::BodyForm;
use super::super::body_forms::{body_entry, forwards_args, returning_read};
use crate::staged::effects::CallForm;
use crate::vm::engine_types::{CallEntry, Framed};
use crate::bytecode::ResidualProgram;

/// Per function, the keys its settled cells were read from (`FunctionFacts::dependencies`): every key its steps
/// read (`flow::FunctionFlow::reads`), a name-wide field read standing for every class's field of that name and
/// the stores into objects of no one class, the returns of every function its calls may reach, its parameters
/// and its receiver. A sound over-approximation of each cell's provenance, per function.
fn dependencies(facts: &AotFacts, reads: BTreeMap<u32, std::collections::BTreeSet<value::Key>>, found: &types::Types)
    -> BTreeMap<u32, Box<[crate::typed_ir::FactKey]>> {
    use crate::typed_ir::FactKey;
    let mut fields: BTreeMap<u32, Vec<FactKey>> = BTreeMap::new();
    for key in facts.cells.keys() {
        if let FactKey::Field { atom, .. } = *key { fields.entry(atom).or_default().push(*key); }
    }
    reads.into_iter().map(|(function, keys)| {
        let mut set: std::collections::BTreeSet<FactKey> = std::collections::BTreeSet::new();
        for key in keys {
            set.insert(census::fact_key(key));
            match key {
                value::Key::Member(atom) => set.extend(fields.get(&atom).into_iter().flatten().copied()),
                // An element read depends on every key of its class: a store through any of them moves the class's elements.
                value::Key::Element(class) => set.extend(found.element_members(class).map(|member| census::fact_key(value::Key::Element(member)))),
                _ => {}
            }
        }
        let range = (function, 0)..=(function, u32::MAX);
        let calls = facts.call_targets.range(range.clone()).map(|(&call, _)| call).chain(facts.accessor_arms.range(range).map(|(&call, _)| call));
        for (caller, pc) in calls {
            set.extend(facts.function_facts(caller).call_targets(pc).into_iter().map(FactKey::Return));
        }
        for (_, set_of) in facts.call_candidates.range((function, 0)..=(function, u32::MAX)) {
            set.extend(set_of.iter().copied().map(FactKey::Return));
        }
        let params = found.param_count(function);
        set.extend((0..params).map(|index| FactKey::Parameter { function, index: index as u16 }));
        set.insert(FactKey::Receiver(function));
        (function, set.into_iter().collect())
    }).collect()
}

/// What the call `call` of `function` passes its callee (`CallSite`): each argument's array class, and the caller's parameter
/// the argument is on entry.
fn call_site(function: u32, call: &flow::Call) -> CallSite {
    let argument = |word: &value::Word| Argument {
        class: word.elements_key(),
        parameter: match word.origin {
            Some(value::Origin::Parameter { function: owner, index }) if owner == function => Some(index),
            _ => None,
        },
    };
    CallSite { receiver: call.receiver.elements_key(), arguments: call.arguments.iter().map(argument).collect() }
}

/// The facts of `program`, solved optimistically in `World::Closed` where the settled solve verifies that no code can be
/// added (`gate::verify`), else in `World::Open`.
pub fn analyze(program: &ResidualProgram) -> AotFacts {
    let Some(openers) = gate::openers(program) else { return analyze_as(program, World::Open) };
    if openers.is_empty() {
        return analyze_as(program, World::Closed);
    }
    match solve(program, World::Closed, Some(&openers)) {
        (facts, true) => facts,
        (_, false) => analyze_as(program, World::Open),
    }
}

/// How closed `program` is (`analyze`). An image keeps the answer its compile found (`AotImage::world`), so a binary without
/// the source frontend derives the same facts again (`analyze_as`).
pub fn world(program: &ResidualProgram) -> World {
    match gate::openers(program) {
        None => World::Open,
        Some(openers) if openers.is_empty() => World::Closed,
        Some(openers) if solve(program, World::Closed, Some(&openers)).1 => World::Closed,
        Some(_) => World::Open,
    }
}

/// The facts of `program`, solved the same way in every world: `world` only marks the keys new code can move
/// (`AotFacts::live`), whose facts the census keeps as guesses (`Census::guessed`). The caller asserts the world (an
/// image's, or `analyze`'s after verification), so nothing is verified here.
pub fn analyze_as(program: &ResidualProgram, world: World) -> AotFacts {
    solve(program, world, None).0
}

/// The solve of `program` in `world`, and whether the world's assumption holds for it: a closed solve with `openers` is
/// verified once settled (`gate::verify`); any other solve, or one that does not settle, is not. The class ids are the
/// solve's own (`ClassIds::build`): a solve that splits no body runs once, and one that does runs again with the split
/// classes, which change no construct site, body or key, so the table of the first run holds in the second.
fn solve(program: &ResidualProgram, world: World, openers: Option<&[u32]>) -> (AotFacts, bool) {
    let (facts, verified) = solve_assumed(program, world, openers, &ClassIds::default());
    if facts.classes.is_empty() { return (facts, verified); }
    let classes = facts.classes.clone();
    solve_assumed(program, world, openers, &classes)
}

/// The rounds `solve_assumed` assumes an escape set in before it takes the untyped one.
const ASSUMPTION_ROUNDS: usize = 3;

/// `solve_with` from an assumption of the functions that escape, which the root's order (`frozen::undominated_closures`) takes as
/// starting at any time. The untyped first pass makes every function escape (its values are all any), so taking its escapes
/// there leaves no closure dominated, no binding single, and no construction resolved, and the solve then escapes everything
/// by its own unresolved calls. So the solve assumes none escapes, and checks the assumption against the escapes it settles: a
/// settled set within the assumption makes the order's seeds a superset of the functions that start at any time, so its
/// dominance holds. A wider set joins the assumption and the solve runs again; past `ASSUMPTION_ROUNDS`, or where no round
/// settles, it takes the untyped pass's escapes.
fn solve_assumed(program: &ResidualProgram, world: World, openers: Option<&[u32]>, classes: &ClassIds) -> (AotFacts, bool) {
    let mut assumed = std::collections::BTreeSet::new();
    for _ in 0..ASSUMPTION_ROUNDS {
        let (facts, verified, escaped) = solve_with(program, world, openers, classes, Some(&assumed));
        match escaped {
            Some(escaped) if escaped.is_subset(&assumed) => return (facts, verified),
            Some(escaped) => assumed.extend(escaped),
            None => break,
        }
    }
    let (facts, verified, _) = solve_with(program, world, openers, classes, None);
    (facts, verified)
}

/// The body forms of every function of `program`, each classified once from the function and its bytecode (`body_forms`). The
/// entry is the one a call takes, from the form the composed signature gives the body (`call_plan::entry_form`). A frameless body
/// that no other frameless answer takes is `Frameless::Native` or `Framed::Activates` by its published native code, which the
/// bytecode does not say, so its entry is None (`AotFacts::body_entry`).
fn body_forms(program: &ResidualProgram, facts: &AotFacts) -> Box<[BodyForm]> {
    (0..program.function_count()).map(|id| program.with_record(id, |body| {
        let frameless = crate::compile::call_plan::entry_form(Some(facts), id as u32) == CallForm::Frameless;
        let entry = body_entry(body, &program.constants, frameless, false);
        BodyForm {
            entry: (!(frameless && entry == CallEntry::framed(Framed::Activates))).then_some(entry),
            returning: returning_read(body),
            forwards: forwards_args(body, &body.call_plan(program)),
        }
    })).collect()
}

/// `solve` with the class ids `classes` the constructions and prototypes resolve to (`ClassIds::of`).
fn solve_with(program: &ResidualProgram, world: World, openers: Option<&[u32]>, classes: &ClassIds,
    assumed: Option<&std::collections::BTreeSet<u32>>,
) -> (AotFacts, bool, Option<std::collections::BTreeSet<u32>>) {
    let scope = Scope::of(program);
    let analyze_typed = |resolved: &std::collections::BTreeMap<value::Key, u32>, names: &std::collections::BTreeSet<u32>,
        types: Option<&types::Types>, feedback: &length_settle::Feedback| -> Vec<FunctionFlow> {
        let mut flows: Vec<FunctionFlow> = (0..program.function_count())
            .map(|id| program.with_record(id, |function| flow::analyze_function(id as u32, function,
                (&scope, crate::staged::DecodeContext::of(program), &program.method_arguments), resolved, classes, names, types, feedback)))
            .collect();
        copies::expand(&mut flows, scope.assign, scope.prototype);
        flows
    };
    // The first pass finds the bindings that hold one closure; the second reads their loads as
    // that closure, so a class object or prototype reached through a binding keeps its identity
    // and its methods prove per class, not per property name. The first pass names no accessor: the names it finds
    // (`accessors::names_of`) are the ones the later passes read.
    let none = length_settle::Feedback::default();
    let first = analyze_typed(&std::collections::BTreeMap::new(), &std::collections::BTreeSet::new(), None, &none);
    let accessor_names = accessors::names_of(&first);
    // A binding or a named property one of whose closure stores no root store dominates is no single closure
    // (`frozen::undominated_closures`): its closure's identity is no proof either, so a read of it holds no closure identity.
    // The first pass's census names the callees the dominance check follows.
    let first_census = Census::of(&first, world, classes.clone());
    let first_facts = AotFacts::unsolved(world);
    let first_escapes = match assumed {
        Some(functions) => escapes::Escapes::assumed(functions.clone()),
        None => escapes::escapes(&first, &first_facts, &first_census, &scope),
    };
    let mut settled_escapes = None;
    let (undominated, _) = frozen::undominated_closures(program, &first, &first_facts, &first_census, &first_escapes);
    let mut resolved = first_census.singles();
    resolved.retain(|key, _| !undominated.contains(&census::fact_key(*key)));
    let flows = analyze_typed(&resolved, &accessor_names, None, &none);
    let mut census = Census::of(&flows, world, classes.clone());
    census.undominated = undominated;
    census.phases = phase::folds(program, &flows, &census);
    let mut facts = AotFacts::unsolved(world);
    for (id, flow) in flows.iter().enumerate() {
        let id = id as u32;
        for call in flow.calls.iter().filter(|call| call.target == flow::Target::Register) {
            if !call.construct && let Some(set) = census.candidates(call.callee) {
                facts.call_candidates.insert((id, call.pc), set.iter().copied().collect());
            }
            let Some(target) = census.target_from(call.callee, id, call.pc) else { continue };
            let guessed = census.guessed(call.callee);
            if call.construct {
                if !guessed {
                    facts.construct_targets.insert((id, call.pc), target);
                    let class = call.callee.origin.and_then(value::Origin::key).map_or(target, |key| classes.of(target, key));
                    facts.construct_classes.insert((id, call.pc), class);
                }
            } else {
                facts.call_targets.insert((id, call.pc), target);
                if guessed { facts.guessed_calls.insert((id, call.pc)); }
            }
        }
        if flow.numbers.iter().any(Option::is_some) {
            facts.numbers.insert(id, flow.numbers.clone());
        }
    }
    // The field, return, parameter and binding types, solved from the bottom (`types`): each pass reads
    // the types the one before found, and the call targets the passes above proved; the least solution is
    // the first pass that changes nothing. A solve that does not settle in `TYPE_PASSES` keeps no type.
    // A pass whose integer range still grows after `WIDENING_ROUNDS` widens it (`Types::widen`), so a counter that
    // climbs settles at int32's span rather than one step a pass.
    // Whether the closed world's assumption holds for the final round's facts (`gate::verify`).
    let mut verified = false;
    // The length facts are solved in rounds (`length_settle::Feedback`). Round 1 solves with none. Round 2 solves again with
    // the facts round 1 proved (filled allocations, hole-free stores, in-bounds reads), and so re-solves the types too: an element
    // cell's holes are the stores' and the allocations', so the types the solve reads must see them. A round whose feedback is
    // the one it solved with is final, so the facts of the last round are the ones its solve read.
    let mut feedback = length_settle::Feedback::default();
    // Without a settled round the table keeps the functions alone (no synthetic class), so its count is the function count.
    let mut built = ClassIds::build(&[], program.function_count() as u32);
    for round in 0..LENGTH_ROUNDS {
        let mut found = types::Types::of(&flows, &facts, &census, &scope).bottom();
        let mut settled = false;
        let mut grown = BTreeMap::new();
        for _ in 0..types::TYPE_PASSES {
            let typed = analyze_typed(&resolved, &accessor_names, Some(&found), &feedback);
            census.retype_wild(&typed);
            let next = types::Types::of(&typed, &facts, &census, &scope).widen(&found, &mut grown);
            settled = next == found;
            found = next;
            if settled { break; }
        }
        if crate::trace::Channel::AotLanes.on() {
            let name = |id: u32| program.with_record(id as usize, |function| function.name).and_then(|atom| program.atoms.get(atom as usize)).map_or("?", |name| name.as_ref());
            eprintln!("aot-lanes: round {round} types settled {settled}; parameters {:?}", found.parameter_types().iter().filter(|(_, types)| !types.is_empty()).map(|(id, types)| (name(*id), types)).collect::<Vec<_>>());
        }
        if crate::trace::Channel::AotLanes.on() {
            let named = |atom: u32| program.atoms.get(atom as usize).map_or("?", |name| name.as_ref()).to_string();
            eprintln!("aot-lanes: classes {:?}", found.class_trace().into_iter().map(|(key, class)| (match key { value::Key::Member(atom) => named(atom), other => format!("{other:?}") }, class)).collect::<Vec<_>>());
        }
        // A round that does not settle keeps the facts of the round before, which solved with its own feedback.
        if !settled { break; }
        // A method read off a value whose class the solve found (`crate::typed_ir::Classes`, a field holding instances of one
        // class) resolves per class (`Origin::MemberOn`, a guess): the calls the passes above found no target for
        // take it from a flow over the settled types.
        let mut reads: BTreeMap<u32, std::collections::BTreeSet<value::Key>> = BTreeMap::new();
        let mut field_readers = Readers::new(program);
        // The maps the settled flows fill by insertion start empty, so a round keeps no entry of the round before.
        facts.element_views.clear();
        facts.access_classes.clear();
        let mut settled_flows = analyze_typed(&resolved, &accessor_names, Some(&found), &feedback);
        census.retype_wild(&settled_flows);
        built = ClassIds::build(&settled_flows, program.function_count() as u32);
        for (id, flow) in settled_flows.iter_mut().enumerate() {
            let id = id as u32;
            reads.insert(id, std::mem::take(&mut flow.reads));
            for call in &flow.calls {
                facts.call_sites.insert((id, call.pc), call_site(id, call));
                if let flow::Target::Accessor { atom, setter } = call.target {
                    let arms = accessors::accessor_arms(call, atom, setter, &census);
                    if !arms.is_empty() { facts.accessor_arms.insert((id, call.pc), accessors::AccessorArms { setter, arms: arms.into_boxed_slice() }); }
                }
            }
            facts.element_stores.extend(flow.element_stores.iter().map(|(&pc, &store)| ((id, pc), store)));
            for call in &flow.calls {
                let keyed = match call.callee.origin {
                    Some(value::Origin::Binding(key)) => Some((None, census::fact_key(key))),
                    Some(value::Origin::Member(atom)) => Some((Some(atom), crate::typed_ir::FactKey::AnyField(atom))),
                    Some(value::Origin::MemberOn { class, instance, atom, .. }) => Some((Some(atom), crate::typed_ir::FactKey::Field { class, instance, atom })),
                    _ => None,
                };
                if let Some(keyed) = keyed { facts.call_keys.insert((id, call.pc), keyed); }
            }
            for (&pc, &view) in &flow.element_views {
                if let Some(kind) = view.kind() { facts.element_views.insert((id, pc), kind); }
            }
            facts.captures.insert(id, flow.captures.iter().copied().collect());
            for (&pc, &classes) in &flow.field_owners {
                if matches!(classes, crate::typed_ir::Classes::Set { .. }) { facts.access_classes.insert((id, pc), classes); }
            }
            for call in &flow.calls {
                if matches!(call.receiver.cell.classes, crate::typed_ir::Classes::Set { .. }) { facts.access_classes.insert((id, call.pc), call.receiver.cell.classes); }
            }
            for call in flow.calls.iter().filter(|call| call.target == flow::Target::Register && !call.construct) {
                if facts.call_targets.contains_key(&(id, call.pc)) { continue; }
                let Some(target) = census.target_from(call.callee, id, call.pc) else { continue };
                facts.call_targets.insert((id, call.pc), target);
                if census.guessed(call.callee) { facts.guessed_calls.insert((id, call.pc)); }
                facts.call_candidates.remove(&(id, call.pc));
            }
            // The callees of this function's calls are settled now: its reads and escapes are scanned against them.
            field_readers.scan(program, id, &flow, &facts, &scope);
            facts.registers.insert(id, std::mem::take(&mut flow.registers));
        }
        let open_escapes = escapes::escapes(&settled_flows, &facts, &census, &scope);
        settled_escapes = Some(open_escapes.functions.clone());
        // The class layouts read the settled shapes: each construction's classes are the settled ones.
        facts.layouts = class_layout::of_program(program, &settled_flows, &census, &facts.constructed_classes());
        if let Some(openers) = openers {
            verified = gate::verify(program, openers, &settled_flows, &facts, &census, &open_escapes);
        }
        // A field no write of reaches (`Type::NEVER` at the fixpoint) is one the analysis sees no value of: any value.
        let seen = |ty: crate::typed_ir::Type| if ty == crate::typed_ir::Type::NEVER { crate::typed_ir::Type::ANY } else { ty };
        facts.field_types = found.fields().map(|(atom, ty)| (atom, seen(ty))).collect();
        // A key no write reaches is `Cell::NEVER`, whose identity is `Never`; seen as any value it is no object's identity.
        facts.cells = found.fact_cells().map(|(key, cell)| (key, crate::typed_ir::Cell {
            ty: seen(cell.ty),
            identity: if cell.ty == crate::typed_ir::Type::NEVER { crate::typed_ir::Identity::Many } else { cell.identity },
            ..cell
        })).collect();
        field_readers.accessor_names(&accessor_names);
        field_readers.protocol_names(program);
        field_readers.finish(&facts.cells, &open_escapes);
        facts.readers = Some(field_readers);
        // The unions read the settled readers and cells, so they are built after both settle.
        facts.unions = class_layout::unions_of(program, &settled_flows, &census, &facts);
        // The `extends` chain and the accessor names `field_holder` reads, settled with the census.
        facts.parents = census.accessors.parents().clone();
        facts.accessor_atoms = accessors::names_of(&settled_flows);
        // The cell facts need the union layout, which holds only in a closed world: no class of an open world keeps one. A fixed
        // cell's slots are stable and every construction is direct; a writable cell is also reached by no flag transition.
        let mut fixed = std::collections::BTreeSet::new();
        let mut writable = std::collections::BTreeSet::new();
        for class in facts.constructed_classes() {
            if !facts.unions.contains_key(&class) { continue; }
            let stable = shapes::stability(&settled_flows, &census, &scope, class);
            if stable.slots && shapes::direct_constructions_only(program, &census, &scope, &open_escapes, class) {
                fixed.insert(class);
                if stable.flags { writable.insert(class); }
            }
        }
        facts.fixed_cells = fixed;
        facts.writable_cells = writable;
        facts.element_classes = found.element_classes();
        facts.dependencies = dependencies(&facts, reads, &found);
        // The closed-world index-accessor fact is read by the signatures' element-read grades, so it is settled first.
        facts.index_accessors_absent = gate::index_accessors_absent(program, world, &settled_flows);
        facts.prototype_indices_absent = facts.index_accessors_absent && gate::prototype_writes_absent(&settled_flows);
        facts.apply_builtin = gate::apply_is_builtin(program, world, &settled_flows);
        // The frozen facts read the settled call graph and the settled flows, so they are solved after the calls settle.
        facts.root_stores = frozen::root_stores(program, world, &settled_flows, &facts, &census, &open_escapes);
        // The identity facts are read from the settled solve, and hold only in the closed world (`identity`).
        facts.builtins = identity::builtin_proofs(program, world, &settled_flows, &scope, &facts.cells);
        facts.closures_once = identity::closures_once(program, world);
        // A closure read before its store holds `undefined` there, so no identity: judged over the settled solve, whose types
        // name the stores the first pass could not (an indexed store `a[i] = x` with `i` a Number writes no named key).
        let (_, settled_undominated) = frozen::undominated_closures(program, &settled_flows, &facts, &census, &open_escapes);
        facts.closures_once.retain(|body| !settled_undominated.contains(body));
        if crate::trace::Channel::AotLanes.on() {
            eprintln!("aot-lanes: once body {}, closures once {:?}", identity::once_body(program), facts.closures_once);
        }
        // The receivers the global object is proven for are read off the calls alone, so they are settled before the cells.
        let this_undefined = identity::this_undefined(program, world, &settled_flows, &facts, &census, &open_escapes);
        facts.global_object_identity = identity::global_object_proof(&settled_flows, &facts, &census, &scope, &this_undefined);
        facts.this_undefined = this_undefined;
        // The settled cells keep only the identities the proofs above keep (`identity::proven`): after this, an `Identity`
        // in a cell is the proof a lane reads.
        let mut registers = std::mem::take(&mut facts.registers);
        for cells in registers.values_mut() { cells.map_cells(|cell| identity::proven(&facts, cell)); }
        facts.registers = registers;
        let mut fact_cells = std::mem::take(&mut facts.cells);
        for cell in fact_cells.values_mut() { *cell = identity::proven(&facts, *cell); }
        facts.cells = fact_cells;
        facts.global_shape = globals::GlobalShape::of(&settled_flows, world);
        facts.uncached = globals::uncached_atoms(program, world);
        facts.global_lexical = globals::GlobalShape::global_lexical_names(program, world);
        // The static name scope of each function (`Function::names_resolve_statically`) is a bytecode property, read in every world.
        facts.names_static = (0..program.function_count() as u32)
            .filter(|&id| program.with_record(id as usize, |function| function.names_resolve_statically(program)))
            .collect();
        // The effect signatures are solved over the settled facts above, so a step leaves only where they leave it open
        // (`signatures::solve`).
        facts.signatures = Some(signatures::solve(program, &facts));
        // The length relations are solved after the signatures, over the settled words (`length_settle::settle`): a call resizes only
        // the arrays its settled effects may write (`FunctionFacts::may_write_elements_of`).
        let settled_lengths: BTreeMap<u32, length_settle::Settled> = settled_flows.iter().enumerate().map(|(id, flow)| {
            let settled = program.with_record(id, |function| length_settle::settle_function(id as u32, function, flow,
                (&scope, crate::staged::DecodeContext::of(program), &program.method_arguments), &resolved, classes, &accessor_names, &found, &facts.function_facts(id as u32), &feedback));
            (id as u32, settled)
        }).collect();
        facts.lengths = settled_lengths;
        let next = length_settle::Feedback::of(&facts.lengths);
        if round + 1 == LENGTH_ROUNDS || next == feedback { break; }
        feedback = next;
    }
    // The body forms read the settled signatures (`call_plan::entry_form`), so they are classified once the solve has settled.
    let bodies = body_forms(program, &facts);
    facts.bodies = bodies;
    facts.classes = built;
    if crate::trace::Channel::AotLanes.on() {
        let named = |atom: &u32| program.atoms.get(*atom as usize).map_or("?", |name| name.as_ref());
        eprintln!("aot-lanes: field types {:?}", facts.field_types.iter().map(|(atom, ty)| (named(atom), *ty)).collect::<Vec<_>>());
        eprintln!("aot-lanes: class field types {:?}", facts.cells.iter().filter_map(|(key, cell)| match *key {
            crate::typed_ir::FactKey::Field { class, instance: true, atom } if facts.field_types.get(&atom) != Some(&cell.ty) => Some((class, named(&atom), cell.ty)),
            _ => None,
        }).collect::<Vec<_>>());
        let (any, total) = facts.any_fields();
        eprintln!("aot-lanes: any fields {any} of {total}");
        eprintln!("aot-lanes: closed world {world:?}: {} call targets ({} guessed), {} closed sets, {} constructions, {} access classes, {} cells", facts.call_targets.len(), facts.guessed_calls.len(), facts.call_candidates.len(), facts.construct_targets.len(), facts.access_classes.len(), facts.cells.len());
    }
    (facts, verified, settled_escapes)
}

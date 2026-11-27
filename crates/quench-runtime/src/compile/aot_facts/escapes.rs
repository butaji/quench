//! Which closures and arrays escape the closed world: the functions code the solve does not follow may call, and the
//! element classes whose arrays it may read or write. `Types::of` takes both from `escapes`.

use super::census::{Census, fact_key};
use super::flow::FunctionFlow;
use super::types::targets;
use super::value::{Key, Origin, Word};
use super::AotFacts;
use crate::typed_ir::{Cell, Classes, ElementClass, Elements, language_bit};
use crate::typed_ir::Type;
use crate::ValueType;
use std::collections::BTreeSet;

/// The functions whose closure may reach a position the solve cannot follow, so that code it does not see may call
/// them with any `this` and arguments (`Types::of` gives an escaped function `Cell::ANY` for its `this` and every
/// parameter). A closure escapes when a sink may hold it:
/// - the callee, `this` or an argument of a recorded call the solve does not resolve (`resolved`): the call may run a
///   native or an unseen function, which the recorded arguments never reach. A callee read off a key (`reads`) holds
///   only the closures written under that key, so only those escape, and a read of a key no write reaches (a host
///   builtin such as `print` or `Math.abs`) holds no program closure at all. A callee of any other origin is its cell;
/// - a value thrown, or a register live at a `yield` or `await` (`FunctionFlow::handed`): a consumer or a reaction runs it;
/// - a store into an element (`Key::Element`): a host builtin reads an array's elements through the array, which may
///   have been stored into through an alias the analysis does not name;
/// - a store into a global that new code can read by name (`Census::live`: never in a closed world, where no code
///   is added to call a global function by name, and no host builtin does);
/// - a return of an escaped function, of the root, or of an async or generator function (`FunctionFlow::returns_out`),
///   whose caller is a consumer the solve does not see;
/// - a store under a named property (`Key::Member`, `MemberOn`, `MemberAny`) that new code can move (`Census::live`), or
///   any store while an object reaches a sink (`object_reader`): unresolved code may read any named property of it.
/// A sink holds a closure by its cell (`mark`): the function its type names, else, where its identity is lost, every
/// materialized closure. A value passed on to a resolved callee reaches the callee's own cells, which its sinks read.
/// The rule is a fixpoint: an escaped function's return is a sink, which may make an object reader, which escapes
/// the named stores. A method call is resolved only where its receiver is a closed set of program classes whose own
/// methods are the targets (`resolved`): a builtin of the same name, which a program's `map` does not shadow, gets
/// the arguments as sinks.
///
/// It also returns the element classes the held values' arrays are in (`Types::of` joins them to the unnamed class): an
/// array a sink holds may be read or written by code the solve does not follow, so its class is not one the solve sees
/// whole, and every array of the class shares the unnamed class's elements.
pub(super) fn escapes(flows: &[FunctionFlow], facts: &AotFacts, census: &Census) -> (BTreeSet<u32>, BTreeSet<ElementClass>) {
    use super::flow::Target;
    let materialized: BTreeSet<u32> = flows.iter().flat_map(|flow| flow.closures.iter().copied()).collect();
    let roots: BTreeSet<u32> = flows.iter().enumerate().filter(|(_, flow)| flow.returns_out).map(|(id, _)| id as u32).collect();
    let mut sinks: Vec<Word> = Vec::new();
    let mut read: Vec<Origin> = Vec::new();
    for (id, flow) in flows.iter().enumerate() {
        for call in &flow.calls {
            if resolved(call, &targets(id as u32, call, facts, census), census) { continue; }
            if call.target == Target::Register {
                match call.callee.origin {
                    Some(origin @ (Origin::Binding(_) | Origin::Member(_) | Origin::MemberOn { .. })) => {
                        read.push(origin);
                        if census.wild_read(origin) { sinks.push(Word::of_cell(Cell::ANY)); }
                    }
                    _ => sinks.push(call.callee),
                }
            }
            sinks.push(call.receiver);
            sinks.extend(call.arguments.iter().copied());
        }
        sinks.extend(flow.handed.iter().copied());
    }
    let writes: Vec<(Key, Word)> = flows.iter().flat_map(|flow| &flow.writes).copied().collect();
    // The closures an unresolved call's callee may be: the writes under the keys its reads may see (`reads`).
    let reached: Vec<Word> = writes.iter().filter(|(key, _)| read.iter().any(|&origin| reads(origin, *key))).map(|&(_, word)| word).collect();
    let mut escaped = BTreeSet::new();
    loop {
        let mut held = sinks.clone();
        held.extend(reached.iter().copied());
        let mut object_reader = sinks.iter().chain(&reached).any(|word| may_be_object(word.cell));
        let mut named = Vec::new();
        for &(key, word) in &writes {
            match key {
                Key::Element(_) => held.push(word),
                Key::Global(_) if census.live(fact_key(key)) => { object_reader |= may_be_object(word.cell); held.push(word); }
                Key::Global(_) => {}
                Key::Return(function) if escaped.contains(&function) || roots.contains(&function) => { object_reader |= may_be_object(word.cell); held.push(word); }
                Key::Member(_) | Key::MemberOn { .. } | Key::MemberAny(_) => named.push((key, word)),
                Key::Return(_) | Key::Slot { .. } => {}
            }
        }
        held.extend(named.into_iter()
            .filter(|(key, _)| object_reader || census.live(fact_key(*key)))
            .map(|(_, word)| word));
        let mut next = escaped.clone();
        for word in &held { mark(&word.cell, &materialized, &mut next); }
        if next == escaped {
            let classes = held.iter().filter(|word| word.cell.elements != Elements::NEVER).map(|word| word.elements_key()).collect();
            return (escaped, classes);
        }
        escaped = next;
    }
}

/// Whether a read off `origin` may take the value `key` stores (`escapes`): the binding itself; a named property of the
/// read's name, whichever object holds it (`Key::Member`, `MemberAny`), or of a class's own method for a read off one
/// class (`MemberOn` of that class and instance). A read off any object may see a class's own method of that name, so
/// `Origin::Member` reaches every `MemberOn` of the name.
fn reads(origin: Origin, key: Key) -> bool {
    match (origin, key) {
        (Origin::Binding(read), _) => read == key,
        (Origin::MemberOn { class, instance, atom, .. }, Key::MemberOn { class: own, instance: own_instance, atom: name }) => own == class && own_instance == instance && name == atom,
        (Origin::Member(atom), Key::Member(name) | Key::MemberAny(name) | Key::MemberOn { atom: name, .. }) => atom == name,
        (Origin::MemberOn { atom, .. }, Key::Member(name) | Key::MemberAny(name)) => atom == name,
        _ => false,
    }
}

/// Whether a recorded call resolves: its targets are known functions (`targets` is not empty) and the program's own code
/// runs (`resolved` below). A method call's arguments reach the callees' parameters (`parameters`) only then, so an
/// unresolved call's arguments escape instead (`escapes`).
///
/// A callee read off a receiver (`Target::Method`, or a method read's `Origin::Member`/`MemberOn`) is a guess of which
/// method runs: a builtin of the same name (`[1].map(g)` beside a program's `map`) may be the one that runs, and `g` is
/// its argument. Only a receiver whose classes are a closed program set, each holding the targets as its own method
/// (`Census::own_methods`), proves the program's method runs.
fn resolved(call: &super::flow::Call, callees: &[u32], census: &Census) -> bool {
    use super::flow::Target;
    if callees.is_empty() { return false; }
    match (call.target, call.callee.origin) {
        (Target::Method(atom), _) => census.own_methods(call.receiver.cell.classes, atom, callees),
        (Target::Register, Some(Origin::Member(atom) | Origin::MemberOn { atom, .. })) => {
            !census.guessed(call.callee) && census.own_methods(call.receiver.cell.classes, atom, callees)
        }
        _ => !census.guessed(call.callee),
    }
}

/// Adds to `escaped` the functions a cell may hold a closure of: its type's identity and its elements' type's identity. A
/// type with a function alternative but no body (identity lost: a join of two closures, or `Type::ANY`) may hold any
/// materialized closure, so each of them escapes.
fn mark(cell: &Cell, materialized: &BTreeSet<u32>, escaped: &mut BTreeSet<u32>) {
    for ty in [cell.ty, cell.elements.ty] {
        if let Some(function) = ty.body_index() {
            escaped.insert(function);
        } else if ty.may_be_languages(Type::language(ValueType::Function)) {
            escaped.extend(materialized.iter().copied());
        }
    }
}

/// Whether a cell may hold an object: an object, a function or an instance of a class. Code the solve does not see may read
/// any named property of such a value, so a store under a named property escapes wherever one reaches a sink (`escapes`).
fn may_be_object(cell: Cell) -> bool {
    let objects = Type::languages(language_bit(ValueType::Object) | language_bit(ValueType::Function));
    cell.ty.may_be_languages(objects) || cell.classes != Classes::Never
}

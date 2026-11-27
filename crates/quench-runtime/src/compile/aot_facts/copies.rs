//! Copy helpers and `Object.assign` as summarized effects (TOD-2448). A copy helper (`Record::copies`: `Object.extend`'s
//! body, a for-in over its source that stores `dst[p] = src[p]`) stores under a key the analysis cannot name, so its body
//! would mark its function `member_wild`, which turns off every fold and gate the closed world proves. Its stores are
//! accounted where it is called instead: a call of it, or of `Object.assign`, whose source may be only object literals of a
//! fixed shape (`Cell::sites`) writes each of their keys as a named store (`Key::Member`) of the value the literal holds
//! for it. A call whose source may be any other object may store any name, so it marks its caller `member_wild`.
//!
//! Why the literal's keys are the whole copy: a for-in over an object visits its own enumerable keys and its prototype's. The
//! literal's keys are its site's, all data properties (an accessor is no site key), and a later store to it is a store of its
//! name the flows already record, which `Key::Member` joins. The one premise the flows do not check is that `Object.prototype`
//! has no enumerable key the program adds.
//!
//! A helper is sealed (its own `member_wild` cleared) only when every call that can reach it is one of these copies: its
//! closure is stored under one name at most, no call, literal, return, accessor or throw passes it on, and no call's callee
//! is of an unknown closure. Otherwise its body keeps its wild store, as it did before.

use std::collections::{BTreeMap, BTreeSet};

use super::flow::{Call, FunctionFlow, Target};
use super::value::{Key, Word};
use crate::typed_ir::Sites;

/// The copy helpers the program may call only where their calls are accounted (`sealed`).
#[derive(Default)]
struct Sealed {
    /// Per sealed helper, the parameter its source comes from (`FunctionFlow::copies`).
    source: BTreeMap<u32, u16>,
    /// The sealed helper a method name reaches, for the names only one sealed helper is stored under.
    by_name: BTreeMap<u32, u32>,
}

/// The name a write stores a property under, if it names one: a named store of any object, of a class, or of an instance of a class.
fn named(key: Key) -> Option<u32> {
    match key {
        Key::Member(atom) | Key::MemberAny(atom) | Key::MemberOn { atom, .. } => Some(atom),
        _ => None,
    }
}

/// The copy helpers `flows` may call only where their calls are accounted: a candidate no value of it escapes from (every write
/// of it stores it under one name, and no call, literal, return, accessor or throw takes it), as long as every callee the
/// program makes is a known closure (a callee of an unknown closure may be one of them).
fn sealed(flows: &[FunctionFlow]) -> Sealed {
    let candidates: BTreeMap<u32, u16> = flows.iter().enumerate()
        .filter_map(|(id, flow)| flow.copies.map(|(_, source)| (id as u32, source)))
        .collect();
    let body = |word: &Word| word.cell.ty.body_index().filter(|function| candidates.contains_key(function));
    let mut names: BTreeMap<u32, BTreeSet<u32>> = BTreeMap::new();
    let mut escaped: BTreeSet<u32> = BTreeSet::new();
    let mut unknown_callee = false;
    for flow in flows {
        for (_, key, word) in &flow.writes {
            if let Some(function) = body(word) {
                match named(*key) {
                    Some(atom) => { names.entry(function).or_default().insert(atom); }
                    None => { escaped.insert(function); }
                }
            }
        }
        for call in &flow.calls {
            escaped.extend(call.arguments.iter().chain(std::iter::once(&call.receiver)).filter_map(body));
            unknown_callee |= call.target == Target::Register && call.callee.cell.ty.body_index().is_none();
        }
        escaped.extend(flow.handed.iter().filter_map(body));
        escaped.extend(flow.accessors.iter().filter_map(|accessor| accessor.function).filter(|function| candidates.contains_key(function)));
        for fields in flow.literals.values() {
            escaped.extend(fields.iter().filter_map(|(_, word)| body(word)));
        }
    }
    let mut sealed = Sealed::default();
    if unknown_callee { return sealed; }
    let mut owners: BTreeMap<u32, Vec<u32>> = BTreeMap::new();
    for (&function, &source) in &candidates {
        let atoms = names.get(&function).cloned().unwrap_or_default();
        if escaped.contains(&function) || atoms.len() > 1 { continue; }
        sealed.source.insert(function, source);
        if let Some(&atom) = atoms.iter().next() { owners.entry(atom).or_default().push(function); }
    }
    // A name two sealed helpers are stored under reaches either: neither is sealed by that name.
    for (atom, functions) in owners {
        if functions.len() == 1 {
            sealed.by_name.insert(atom, functions[0]);
        } else {
            for function in functions { sealed.source.remove(&function); }
        }
    }
    sealed
}

/// The keys and values of the literal sites `sites` may be, if it is a set and every site has its table (`FunctionFlow::literals`):
/// a copy writes all of them, so a site without one (or an object of any other origin) is no copy's source.
fn literal_fields(flows: &[FunctionFlow], sites: Sites) -> Option<Vec<(u32, Word)>> {
    let mut fields = Vec::new();
    for site in sites.set()? {
        fields.extend(flows.get(site.function as usize)?.literals.get(&site.pc)?.iter().copied());
    }
    Some(fields)
}

/// The argument positions `call` copies from, if it is a copy: the source of a sealed helper it calls (`Target::Known`, a register
/// that holds its closure, or a method name only it is stored under), or every argument after the first of `Object.assign`.
fn copied_from(call: &Call, sealed: &Sealed, assign: Option<u32>) -> Option<Vec<usize>> {
    let helper = match call.target {
        Target::Known(function) => Some(function),
        Target::Register => call.callee.cell.ty.body_index(),
        Target::Method(atom) => sealed.by_name.get(&atom).copied(),
        Target::Accessor { .. } => None,
    };
    if let Some(&source) = helper.and_then(|function| sealed.source.get(&function)) {
        return Some(vec![usize::from(source)]);
    }
    match call.target {
        Target::Method(atom) if Some(atom) == assign => Some((1..call.arguments.len()).collect()),
        _ => None,
    }
}

/// Accounts the copies the program makes (see the module docs): each named store a copy of a literal makes, into the caller
/// that makes it, and `member_wild` for each copy from anything but a literal. The sealed helpers' own `member_wild` is cleared,
/// as their stores are the copies' stores. `assign` is the atom `Object.assign` interns to, if the program names it.
///
/// A constructor's `prototype` stored from an object literal (`C.prototype = {...}`, `Key::MemberOn` with `instance: false`) is the
/// same kind of copy: each literal key is a prototype member of `C`, written as `Key::MemberOn { class, instance: true }`, the
/// convention of `C.prototype.m = f`. A value that is no literal names no member the solve can see, so it writes none.
pub(super) fn expand(flows: &mut [FunctionFlow], assign: Option<u32>, prototype: Option<u32>) {
    let sealed = sealed(flows);
    let mut writes: Vec<(usize, u32, Key, Word)> = Vec::new();
    let mut wild: Vec<usize> = Vec::new();
    if let Some(prototype) = prototype {
        for caller in 0..flows.len() {
            let stores: Vec<(u32, u32, Word)> = flows[caller].writes.iter()
                .filter_map(|&(pc, key, word)| match key {
                    Key::MemberOn { class, instance: false, atom } if atom == prototype => Some((pc, class, word)),
                    _ => None,
                })
                .collect();
            for (pc, class, word) in stores {
                for (atom, value) in literal_fields(flows, word.cell.sites).unwrap_or_default() {
                    writes.push((caller, pc, Key::MemberOn { class, instance: true, atom }, value));
                }
            }
        }
    }
    for (caller, flow) in flows.iter().enumerate() {
        for call in &flow.calls {
            let Some(sources) = copied_from(call, &sealed, assign) else { continue };
            for index in sources {
                let fields = call.arguments.get(index).and_then(|word| literal_fields(flows, word.cell.sites));
                match fields {
                    Some(fields) => writes.extend(fields.iter().map(|&(atom, value)| (caller, call.pc, Key::Member(atom), value))),
                    None => wild.push(caller),
                }
            }
        }
    }
    let mut touched: BTreeSet<usize> = BTreeSet::new();
    for (caller, pc, key, value) in writes {
        flows[caller].writes.push((pc, key, value));
        touched.insert(caller);
    }
    for &caller in &touched {
        // Writes are kept in PC order (`FunctionFlow::writes`); the copies land at their call's PC.
        flows[caller].writes.sort_by_key(|(pc, _, _)| *pc);
    }
    for caller in wild { flows[caller].member_wild = true; }
    for &function in sealed.source.keys() { flows[function as usize].member_wild = false; }
}

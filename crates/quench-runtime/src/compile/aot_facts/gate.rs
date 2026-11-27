//! The closed-world gate: which facts hold of a program that may receive code at run time.
//!
//! The solve is optimistic. It runs in `World::Closed` (no code is added), and `verify` checks the assumption once the
//! solve settles; a program that fails the check is solved again in `World::Open`. The check uses the solve's own call
//! graph, so no second reading of the program decides the world.
//!
//! Soundness. The solve assumes that no code is added at run time. Code enters only through an opener site (a direct
//! `eval`, a `Function` constructor, a reflective call that may reach one; `openers`) or a dynamic import (`Open`). The
//! check holds when both of these hold:
//! - No opener lies in a function the call graph reaches. The call graph over-approximates what can run:
//!   - a function is reached by a call the solve resolves to it (`types::targets`), by an unresolved call's callee, which
//!     may be any escaped function (`escapes::escapes`), or without a recorded call when code the solve does not see
//!     starts it: the root, a class constructor (`super` calls run it), an accessor, a closure a named property holds (a
//!     runtime conversion calls `toString` or `valueOf`), and a Module's declarations (the host imports them);
//!   - a declaration nothing calls or escapes is not reached: it runs only when read, and a read the solve follows is a call.
//!   The host is not a caller: it runs no program closure except through an escape, as the solve already assumes.
//! - The global object reaches no place that can name a global the call graph does not record. A value that may be the
//!   global object (`Cell::global`) is produced only by a root's `this`, a global-object name (`globalThis`, `window`,
//!   `self`, `global`, `frames`), and the `this` a plain call binds to a sloppy function (an escaped function's `this`
//!   included). The host hands the program no other global object, as the premise of the closed world. From such a value,
//!   a reached computed access (`g[k]`), a for-in or `in` over it, or a named access (`g.f`, whose result may be a global
//!   function called later) would reach any global function by name: `effects::keyed_global` marks those. An escape that
//!   holds it (`escapes::Escapes::sinks`) gives unseen code the global object, which may read every global function.
//!
//! The check is the standard optimistic-then-verify pattern: the solve's facts under `Closed` are kept only when the
//! assumption they rest on is verified by the settled solve; otherwise they are recomputed under `Open`.

#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::bytecode::{ProgramKind, ResidualProgram};
#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::{Engine, SourceKind};
use super::census::Census;
use super::escapes::{self, Escapes};
use super::flow::FunctionFlow;
use super::types::targets;
use super::value::Key;
use super::AotFacts;
use crate::typed_ir::FactKey;
use std::collections::BTreeSet;

/// The global name a dynamic `import()` calls (`compile::ast::expression`), a host hook that loads code.
const DYNAMIC_IMPORT: &str = "\0quench:dynamic-import";

/// How closed a program is: what code added at run time can change. Every world solves the same
/// facts; the world only says which keys new code can move (`World::live`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum World {
    /// Code may be added that reaches its scopes (a direct `eval`, a dynamic import, source the
    /// gate cannot read): every key is live, slots and parameters included.
    Open,
    /// Code may be added, but only as global code (`Function`, an indirect `eval`, a reflective
    /// `o[k]` that may reach either): it can write globals and any object's properties, never a
    /// function's own bindings or registers. Globals, named members and elements are live; slots,
    /// returns and parameters are not. No verdict produces it (a site that adds code opens the world).
    Locals,
    /// No code can be added: no key is live.
    Closed,
}

impl World {
    /// The world a byte (`World as u8`, an image file's) names; `None` for any other byte.
    pub fn from_byte(byte: u8) -> Option<Self> {
        [Self::Open, Self::Locals, Self::Closed].into_iter().find(|world| *world as u8 == byte)
    }

    /// Whether code added in this world can move `key`: what a fact under `key` guesses. Only a
    /// live key's fact is a guess held under an edge, so only a live key's readers subscribe.
    pub fn live(self, key: FactKey) -> bool {
        match self {
            Self::Closed => false,
            Self::Open => true,
            Self::Locals => matches!(key, FactKey::Global(_) | FactKey::Field { .. } | FactKey::AnyField(_) | FactKey::Elements(_)),
        }
    }
}

/// The sites that may receive code at run time (`SourceDemands`: eval, Function-family and reflective sites), by offset in
/// the program's source, every site whether or not it runs (`verify` decides which run). None where the program's source
/// does not bound its sites (an eval program, a dynamic import, a source that does not parse): the world is Open.
#[cfg(feature = "jit")]
pub(super) fn openers(program: &ResidualProgram) -> Option<Vec<u32>> {
    let kind = match program.kind {
        ProgramKind::Script => SourceKind::Script,
        ProgramKind::Module => SourceKind::Module,
        ProgramKind::Eval | ProgramKind::Wasm => return None,
    };
    let imports = program.atoms.iter().any(|name| name == DYNAMIC_IMPORT);
    // Without the front end nothing reads the source: no program is proved closed.
    if !cfg!(feature = "source") || imports {
        return None;
    }
    let demands = Engine::standalone_source_demands(&program.source.as_ref()?.text, kind)?;
    let mut sites = [demands.eval, demands.function, demands.reflective].concat();
    sites.sort_unstable();
    sites.dedup();
    Some(sites)
}

/// An execution-only build runs no code its image did not archive (`require_source_compilation` refuses dynamic source),
/// so every program it can run is closed.
#[cfg(not(feature = "jit"))]
pub(super) fn openers(program: &ResidualProgram) -> Option<Vec<u32>> {
    let _ = program;
    Some(Vec::new())
}

/// Whether the settled solve's assumption holds: `flows`, `facts`, `census` and `escaped` are the solve's own, taken in
/// `World::Closed` with the program's `openers` (`openers`). See the module's soundness argument.
pub(super) fn verify(program: &ResidualProgram, openers: &[u32], flows: &[FunctionFlow], facts: &AotFacts, census: &Census, escaped: &Escapes) -> bool {
    let reached = reached(program, flows, facts, census, escaped);
    let indexed = flows.iter().zip(&reached).any(|(flow, &live)| live && flow.global_keyed);
    let escaping = escaped.sinks.iter().any(|cell| cell.global);
    let opened = opener_functions(program, openers).is_some_and(|functions| functions.iter().all(|&function| !reached[function as usize]));
    opened && !indexed && !escaping
}

/// The functions the solve's call graph reaches: every seed (`seeds`) and every target a reached function's calls may name.
/// A function nothing names is not reached, so its code runs in no run of a closed program.
fn reached(program: &ResidualProgram, flows: &[FunctionFlow], facts: &AotFacts, census: &Census, escaped: &Escapes) -> Vec<bool> {
    let mut reached = vec![false; flows.len()];
    let mut pending = seeds(program, flows, census, escaped);
    while let Some(function) = pending.pop() {
        if std::mem::replace(&mut reached[function as usize], true) {
            continue;
        }
        for call in &flows[function as usize].calls {
            pending.extend(targets(function, call, facts, census));
        }
    }
    reached
}

/// The functions code the call graph does not record may start: the program's roots, its class constructors, accessors, the
/// closures named properties hold (`escapes::mark`: a closure a conversion or the host may call), the escaped functions, and,
/// for a Module, the functions its root declares (the host imports them).
fn seeds(program: &ResidualProgram, flows: &[FunctionFlow], census: &Census, escaped: &Escapes) -> Vec<u32> {
    let materialized: BTreeSet<u32> = flows.iter().flat_map(|flow| flow.closures.iter().copied()).collect();
    let mut named = BTreeSet::new();
    for flow in flows {
        for (key, word) in &flow.writes {
            if matches!(key, Key::Member(_) | Key::MemberOn { .. } | Key::MemberAny(_)) {
                escapes::mark(&word.cell, &materialized, &mut named);
            }
        }
    }
    let root = (0..flows.len()).find(|&id| program.with_record(id, |function| function.parent.is_none())).map(|id| id as u32);
    let module_root = if matches!(program.kind, ProgramKind::Module) { root } else { None };
    let mut seeds: Vec<u32> = (0..flows.len())
        .filter(|&id| program.with_record(id, |function| {
            function.parent.is_none() || function.is_class_constructor || (module_root.is_some() && function.parent == module_root)
        }))
        .map(|id| id as u32)
        .collect();
    seeds.extend(escaped.functions.iter().copied());
    seeds.extend(census.accessors.functions());
    seeds.extend(named);
    seeds
}

/// The function each opener site lies in: the innermost function whose source span contains it, the root for top-level code.
/// None where a function nested in the root has no span, since a site may then lie in code the spans do not name.
fn opener_functions(program: &ResidualProgram, openers: &[u32]) -> Option<Vec<u32>> {
    let mut spans = Vec::new();
    let mut root = None;
    for id in 0..program.function_count() {
        let (top, span) = program.with_record(id, |function| (function.parent.is_none(), function.source_span));
        if top && root.is_none() {
            root = Some(id as u32);
        }
        match span {
            Some(span) => spans.push((span.start, span.end, id as u32)),
            None if !top => return None,
            None => {}
        }
    }
    let root = root?;
    Some(openers.iter().map(|&offset| {
        spans
            .iter()
            .filter(|&&(start, end, _)| start <= offset && offset < end)
            .min_by_key(|&&(start, end, _)| end - start)
            .map_or(root, |&(_, _, id)| id)
    }).collect())
}

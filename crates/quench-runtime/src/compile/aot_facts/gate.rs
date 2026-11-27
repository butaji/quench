//! The closed-world gate: which facts hold of a program that may receive code at run time.

#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::bytecode::{ProgramKind, ResidualProgram};
#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::{Engine, SourceKind};
use crate::typed_ir::FactKey;

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
    /// `o[k]` that may reach either; `SourceDemands::function`, `reflective`): it can write
    /// globals and any object's properties, never a function's own bindings or registers. Globals,
    /// named members and elements are live; slots, returns and parameters are not.
    Locals,
    /// No code can be added (`SourceDemands::requires_dynamic_source` is false): no key is live.
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

/// How closed `program` is: its source's demands (`Engine::standalone_source_demands`, the
/// decision every producer makes), and whether it calls a dynamic import. A program whose source
/// is not kept is not proved closed.
///
/// Reachability (why an unreferenced `eval` keeps a program Closed). The demands hold only the
/// sites that can run: `standalone_source_demands` drops a site whose innermost function body no
/// run can call (`native_sources::reachability`). The soundness argument, for the gate's first
/// decision, which assumes the world Closed except for the sites that can run:
/// - A site opens the world only if it runs. Code runs from the top level, and a function body
///   runs only if its closure is created by a live body and called. A closure expression, arrow or
///   method is created where it appears, so it is live with its enclosing body. A declaration is
///   called only through a read of its binding, so it is live once such a read sits in a live
///   body. Roots: a Module's top-level declarations (the host imports them), and a Script's global
///   functions once the global object is materialized by a live site (`globalThis`, `window`,
///   `self`, `global`, `frames`, a top-level `this`, or a sloppy function's `this` that a plain
///   call binds). Only then can a computed read (`g[k]`) reach a global; a computed read on any
///   other object reaches no global.
/// - Unresolved calls: a call the solve cannot resolve may reach any escaped function
///   (`escapes::escapes`). An escaped closure is a closure value a live body created, so its body
///   is live by the rule above; it is never a dead body. So the escaped set is within the live set,
///   and no site that opens the world is missed by ignoring dead bodies.
/// - New code in a Closed world: a live site that could add code opens the world, so the world
///   is Closed only when no live site adds code. Code only enters through a site, so a Closed
///   program's dead sites never run and add nothing.
/// The gate does not read the solve's call graph: the solve takes the world as input (`live`),
/// so the call graph cannot decide the world without a second solve. The syntactic fixpoint is a
/// superset of the call graph's reachable set, so it is sound and may be less precise.
/// A dynamic import is still program-wide (`DYNAMIC_IMPORT`), since the compiled atoms carry no
/// position for it.
pub(super) fn world(program: &ResidualProgram) -> World {
    // An execution-only build runs no code its image did not archive (`require_source_compilation`
    // refuses dynamic source), so every program it can run is closed; the source parse below is
    // the compiler's own proof and needs the syntax frontend this build does not carry.
    #[cfg(not(feature = "jit"))]
    {
        let _ = program;
        World::Closed
    }
    #[cfg(feature = "jit")]
    {
        let kind = match program.kind {
            ProgramKind::Script => SourceKind::Script,
            ProgramKind::Module => SourceKind::Module,
            ProgramKind::Eval | ProgramKind::Wasm => return World::Open,
        };
        let imports = program.atoms.iter().any(|name| name == DYNAMIC_IMPORT);
        // Without the front end nothing reads the source: no program is proved closed.
        if !cfg!(feature = "source") || imports {
            return World::Open;
        }
        let Some(demands) = program.source.as_ref().and_then(|source| Engine::standalone_source_demands(&source.text, kind)) else {
            return World::Open;
        };
        if !demands.eval.is_empty() { World::Open }
        else if demands.requires_dynamic_source() { World::Locals }
        else { World::Closed }
    }
}

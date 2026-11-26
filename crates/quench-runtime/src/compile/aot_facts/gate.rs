//! The closed-world gate: which facts hold of a program that may receive code at run time.

use crate::bytecode::{ProgramKind, ResidualProgram};
use crate::{Engine, SourceKind};

/// The global name a dynamic `import()` calls (`compile::ast::expression`), a host hook that loads code.
const DYNAMIC_IMPORT: &str = "\0quench:dynamic-import";

/// How closed a program is: what code added at run time can change.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(super) enum World {
    /// Code may be added that reaches its scopes (a direct `eval`, a dynamic import, source the
    /// gate cannot read): no fact holds.
    Open,
    /// Code may be added, but only as global code (`Function`, an indirect `eval`, a reflective
    /// `o[k]` that may reach either; `SourceDemands::function`, `reflective`): it can write
    /// globals and any object's properties, never a function's own bindings or registers. Facts
    /// of those hold; facts of globals and named members do not.
    Locals,
    /// No code can be added (`SourceDemands::requires_dynamic_source` is false): every fact holds.
    Closed,
}

/// How closed `program` is: its source's demands (`Engine::standalone_source_demands`, the
/// decision every producer makes), and whether it calls a dynamic import. A program whose source
/// is not kept is not proved closed.
pub(super) fn world(program: &ResidualProgram) -> World {
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

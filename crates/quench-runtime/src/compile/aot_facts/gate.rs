//! The closed-world gate: a program that can receive code at run time has no facts.

#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::bytecode::{ProgramKind, ResidualProgram};
#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::{Engine, SourceKind};

/// The global name a dynamic `import()` calls (`compile::ast::expression`), a host hook that loads code.
const DYNAMIC_IMPORT: &str = "\0quench:dynamic-import";

/// Whether `program` can add no code at run time: its source demands no dynamic source
/// (`Engine::source_requires_dynamic_source`, the decision every producer makes) and it calls no
/// dynamic import. A program whose source is not kept is not proved closed.
pub(super) fn closed_world(program: &ResidualProgram) -> bool {
    // An execution-only build runs no code its image did not archive (`require_source_compilation`
    // refuses dynamic source), so every program it can run is closed; the source parse below is
    // the compiler's own proof and needs the syntax frontend this build does not carry.
    #[cfg(not(feature = "jit"))]
    {
        let _ = program;
        return true;
    }
    #[cfg(feature = "jit")]
    {
    let kind = match program.kind {
        ProgramKind::Script => SourceKind::Script,
        ProgramKind::Module => SourceKind::Module,
        ProgramKind::Eval | ProgramKind::Wasm => return false,
    };
    let imports = program.atoms.iter().any(|name| name == DYNAMIC_IMPORT);
    // Without the front end nothing reads the source: no program is proved closed.
    if !cfg!(feature = "source") {
        return false;
    }
    !imports
        && program
            .source
            .as_ref()
            .is_some_and(|source| !Engine::source_requires_dynamic_source(&source.text, kind))
    }
}

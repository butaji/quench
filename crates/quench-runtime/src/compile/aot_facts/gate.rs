//! The closed-world gate: which facts hold of a program that may receive code at run time.

#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::bytecode::{ProgramKind, ResidualProgram};
#[cfg_attr(not(feature = "jit"), allow(unused_imports))]
use crate::{Engine, SourceKind};
use super::flow::{Call, FunctionFlow, Target};
use super::value::Origin;
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

/// Names whose presence lets an element access run an accessor the flows do not see: a proxy's traps run for every access, and a
/// descriptor object (`create`, `defineProperties`) may define an index accessor with keys no flow names.
const INDEX_ACCESSOR_NAMES: [&str; 3] = ["Proxy", "create", "defineProperties"];
/// The builtin that defines one property from its key and descriptor; its key is known only where an argument names a non-index atom.
const PROPERTY_DEFINER: &str = "defineProperty";

/// The text of atom `atom`.
fn atom_text(program: &ResidualProgram, atom: u32) -> &str {
    let text: &str = program.atoms[atom as usize].as_ref();
    text
}

/// Whether atom `atom` is an array index: its text is the canonical form of an integer below `u32::MAX`.
fn index_name(program: &ResidualProgram, atom: u32) -> bool {
    let text = atom_text(program, atom);
    text.parse::<u32>().is_ok_and(|index| index != u32::MAX && index.to_string() == text)
}

/// Whether `call` is a `defineProperty` that may define an index: its callee is that property, read by name or through an alias
/// (`aliased`: the program names it, and the callee is no member read), and no argument names a non-index atom as the key.
fn may_define_index(program: &ResidualProgram, call: &Call, aliased: bool) -> bool {
    let name = match call.target {
        Target::Method(atom) => Some(atom),
        Target::Register => match call.callee.origin {
            Some(Origin::Member(atom) | Origin::MemberOn { atom, .. }) => Some(atom),
            _ => return aliased,
        },
        Target::Known(_) => None,
    };
    name.is_some_and(|atom| atom_text(program, atom) == PROPERTY_DEFINER)
        && !call.arguments.iter().any(|argument| argument.atom_key().is_some_and(|atom| !index_name(program, atom)))
}

/// Whether no index accessor can run for an element access of `program`, so that the access runs no guest code for a key that
/// converts to none (`lengths::element_runs_guest_code`). Only the closed world proves it: code added at run time may define one.
/// Given a closed world, it holds when no accessor the flows define has an index name or an unknown key (`FunctionFlow::member_wild`),
/// no `defineProperty` may name an index, and the program names no proxy or descriptor-defining builtin (`INDEX_ACCESSOR_NAMES`).
pub(super) fn index_accessors_absent(program: &ResidualProgram, world: World, flows: &[FunctionFlow]) -> bool {
    let names = |name: &str| program.atoms.iter().any(|atom| {
        let text: &str = atom.as_ref();
        text == name
    });
    let aliased = names(PROPERTY_DEFINER);
    world == World::Closed
        && !INDEX_ACCESSOR_NAMES.iter().any(|name| names(name))
        && flows.iter().all(|flow| {
            !flow.member_wild
                && flow.accessors.iter().all(|accessor| !index_name(program, accessor.atom))
                && flow.calls.iter().all(|call| !may_define_index(program, call, aliased))
        })
}

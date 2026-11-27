//! What the functions of a program share: each function's parent, the Number constants, and the
//! atoms that name the global object.

use crate::bytecode::{Constant, ResidualProgram};

/// Names that reach the global object as a value (`globalThis`, and Node's `global`).
const GLOBAL_OBJECT_NAMES: [&str; 2] = ["globalThis", "global"];

/// The program-wide tables the per-function analyses read.
pub(super) struct Scope {
    /// Each function's parent, by function id.
    parents: Vec<Option<u32>>,
    /// Each constant's value where it is a Number, by constant index.
    pub(super) constants: Vec<Option<f64>>,
    /// The atoms that name the global object.
    global_objects: Vec<u32>,
    /// The atom `prototype` interns to, if the program names it.
    pub(super) prototype: Option<u32>,
    /// Each String constant's atom, by constant index.
    pub(super) constant_atoms: Vec<Option<u32>>,
    /// Each `null`, `undefined` or Boolean constant's type, by constant index.
    pub(super) immediates: Vec<Option<crate::typed_ir::Type>>,
    /// Each nested field lookup's last field (`FieldSite`: `base.first.second`, or `base.first`): the field
    /// its read delivers, by site index; `None` for a site with a sink, which stores rather than reads.
    pub(super) site_reads: Vec<Option<u32>>,
}

impl Scope {
    pub(super) fn of(program: &ResidualProgram) -> Self {
        Self {
            parents: (0..program.function_count())
                .map(|id| program.with_record(id, |function| function.parent))
                .collect(),
            constants: program
                .constants
                .iter()
                .map(|constant| match constant {
                    Constant::Number(value) => Some(*value),
                    _ => None,
                })
                .collect(),
            global_objects: program
                .atoms
                .iter()
                .enumerate()
                .filter(|(_, name)| GLOBAL_OBJECT_NAMES.contains(name))
                .map(|(atom, _)| atom as u32)
                .collect(),
            prototype: program.atoms.iter().position(|name| name == "prototype").map(|atom| atom as u32),
            constant_atoms: program.constants.iter().map(|constant| match constant {
                Constant::String(text) => program.atoms.iter().position(|name| name == text.as_ref()).map(|atom| atom as u32),
                _ => None,
            }).collect(),
            immediates: program.constants.iter().map(|constant| {
                use crate::typed_ir::Type;
                use crate::ValueType;
                match constant {
                    Constant::Null => Some(Type::language(ValueType::Null)),
                    Constant::Undefined => Some(Type::language(ValueType::Undefined)),
                    Constant::Boolean(_) => Some(Type::language(ValueType::Boolean)),
                    _ => None,
                }
            }).collect(),
            site_reads: program.field_sites.iter().map(|site| site.sink.is_none().then(|| site.second.map_or(site.first.0, |(atom, _)| atom))).collect(),
        }
    }

    pub(super) fn is_global_object(&self, atom: u32) -> bool {
        self.global_objects.contains(&atom)
    }

    /// The function owning the environment `depth` levels out from `function`, which a capture reads.
    pub(super) fn owner(&self, function: u32, depth: u16) -> Option<u32> {
        (0..=depth).try_fold(function, |owner, _| self.parents.get(owner as usize).copied().flatten())
    }
}

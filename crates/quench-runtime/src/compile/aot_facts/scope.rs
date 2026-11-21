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

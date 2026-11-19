//! The eval context a source compiles in: the function, home object, `super` and class
//! initializer around it. A record of the text's compilation, not of the compiler.

use super::Atom;

/// The eval context a source compiles in: the function, home object, `super` and class
/// initializer around it. A record of the text's compilation, not of the compiler.
#[derive(Clone, Copy, Default, PartialEq, Eq, Hash)]
pub(crate) struct EvalContext {
    pub in_function: bool,
    pub home_atom: Option<Atom>,
    pub super_calls: bool,
    pub class_initializer: bool,
}

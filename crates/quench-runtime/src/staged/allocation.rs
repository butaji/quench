//! The allocation rows whose operands the instruction alone fixes: an empty object, an object
//! literal over a register window or of two registers, a constant array, a closure, a literal
//! template and a string concatenation. Each makes a
//! heap value, so the frame's heap decides how (`Frame::allocate_value`): the interpreter
//! allocates in place, a specializer calls out to an allocator that leaves for a due
//! collection first.

use crate::bytecode::{Register, RegisterList, RegisterWindow};

/// What an allocation row makes. Every operand is static: the registers it reads are named by
/// a window, never passed as words, so each tier reads them from its own register homes.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Allocation {
    /// An empty ordinary object (`Vm::object`).
    Empty,
    /// An array of `length` holes (`Vm::hole_array`); its elements, defined after it, are
    /// stores of their own. `numbers`: the literal's binding is declared an array of Numbers, so
    /// an empty one starts with a double backing (`Vm::make_array`); a tier that makes it as any
    /// other array is as exact, the backing turning double at its first Number that is no int32.
    Array { length: u32, numbers: bool },
    /// An array holding the values of `elements`, in order (`Vm::literal_array`): the literal
    /// whose `DefineArrayElement` records follow its `MakeArray` back to back, made at once as
    /// their definitions into its holes would leave it. A fresh array needs no write barrier.
    ArrayOf { elements: RegisterList },
    /// A fresh object of regexp literal `site`'s compiled template: the matcher program is the
    /// site's, its state is fresh. A pattern that fails to compile raises at the instruction.
    /// `callee`: the register holding the callee of the `replace`/`match`/`search`/`split` call
    /// the literal is the first argument of (`CreateRegExpLiteral`'s field B), read where the
    /// tier keeps it: while that callee is the realm's String method the site may answer its one
    /// shared object (`Vm::regexp_literal_argument_of`), keyed by the activation that evaluates it.
    RegExp { site: u32, callee: Option<Register> },
    /// The array of the `count` program constants from constant `constant`.
    ConstArray { constant: u32, count: u16 },
    /// An ordinary object of literal `site`'s shape holding registers `first` and `second`.
    ObjectPair { site: u32, first: Register, second: Register },
    /// An ordinary object of literal `site`'s shape holding the registers of `window`.
    ObjectLiteral { site: u32, window: RegisterWindow },
    /// The closure of function `function` over the activation's environment, which the first
    /// closure promotes (`Vm::promote_frame_environment`).
    Closure { function: u32 },
    /// The value literal template `template` builds from its steps.
    Template { template: u32 },
    /// ToString of each register of `window`, concatenated in order.
    Concat { window: RegisterWindow },
}

/// The empty allocations both JIT tiers make inline from a space's cursor and captured template.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum InlineEmpty {
    /// `Allocation::Empty`: an empty ordinary object.
    Object,
    /// `Allocation::Array { length: 0 }`: a zero-length array, whose elements are the static empty header.
    Array,
}

impl Allocation {
    /// The inline empty allocation this is, or `None`. The one predicate both tiers read: the u-jit's
    /// `allocate_value` and the b-jit planner (`base_region::steps`) decide an inline empty allocation by it.
    pub fn inline_empty(self) -> Option<InlineEmpty> {
        match self {
            Self::Empty => Some(InlineEmpty::Object),
            Self::Array { length: 0, .. } => Some(InlineEmpty::Array),
            _ => None,
        }
    }
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;

#[cfg(test)]
mod tests;

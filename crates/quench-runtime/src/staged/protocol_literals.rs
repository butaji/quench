//! The literal, template, closure and object-construction opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper, which the host makes
//! (`Vm::call_out` -> `Vm::literal_step`). The decode of a step from its record is in `protocol_literals/glue.rs`.
use crate::bytecode::Register;

/// A literal, template, closure and object-construction step the host makes by the interpreter's own definition.
///
/// Operands are the first two registers the step names (`decode`); a step's further registers are
/// read from the current frame by their index here.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LiteralStep {
    /// A fresh empty object (`MakeObject`).
    Object,
    /// A fresh array of `length` holes (`MakeArray`).
    Array { length: usize },
    /// A fresh array of the `count` constants from `constant` (`MakeConstArray`).
    ConstArray { constant: usize, count: u16 },
    /// The two-field object at `site` of the operand registers (`MakeObject2`, not returning from its frame).
    ObjectPair { site: usize },
    /// The object at `site` of the register window `base..base + count` (`MakeObjectLiteral`, not returning from its frame).
    ObjectLiteral { site: usize, base: Register, count: u16 },
    /// The nested literal of constants of template `template` (`MakeLiteralTemplate`).
    LiteralTemplate { template: usize },
    /// The RegExp object of literal site `site` (`CreateRegExpLiteral`).
    RegExp { site: usize },
    /// The template object of `site`, cached on its first make; the first operand is the object (`CacheTemplateObject`).
    CacheTemplate { site: u32 },
    /// The template object of `site`, or `undefined` before it is cached (`LoadCachedTemplateObject`).
    LoadTemplate { site: u32 },
    /// Element `index` of the array (first operand) defined as the value (second operand) (`DefineArrayElement`).
    DefineArrayElement { index: u32 },
    /// Copy the source (second operand) into the target (first operand) except the keys of the `exclusions` register (`CopyDataProperties`).
    CopyDataProperties { exclusions: Register },
    /// Define the key (second operand) of the target (first operand) from the descriptor the definition mode `mode`
    /// (its `PropertyDefinitionMode` word) makes of the `value` register (`DefinePropertyRecord`).
    DefinePropertyRecord { mode: u32, value: Register },
    /// The closure of function `function` over the binding environment the running activation captures (`MakeClosure`).
    Closure { function: u32 },
}

#[cfg(not(stencil_build))]
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;

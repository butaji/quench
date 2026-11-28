//! The literal, template, closure and object-construction opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper, which the host makes
//! (`Vm::call_out` -> `Vm::literal_step`).
use super::Protocol;
use crate::bytecode::{PropertyDefinitionMode, Register};
#[cfg(not(stencil_build))]
use crate::bytecode::{Op, WideInstruction};

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
    /// Define the key (second operand) of the target (first operand) from the descriptor `mode` makes of the `value` register (`DefinePropertyRecord`).
    DefinePropertyRecord { mode: PropertyDefinitionMode, value: Register },
    /// The closure of function `function` over the binding environment the running activation captures (`MakeClosure`).
    Closure { function: u32 },
}

#[cfg(not(stencil_build))]
/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub(super) fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
    let result = || Some(instruction.result_register());
    let (step, destination, operands) = match instruction.op() {
        Op::MakeObject => (LiteralStep::Object, result(), [None, None]),
        Op::MakeArray => (LiteralStep::Array { length: instruction.array_length() }, result(), [None, None]),
        Op::MakeConstArray => (LiteralStep::ConstArray { constant: instruction.constant_index(), count: instruction.element_count() },
            result(), [None, None]),
        Op::MakeObject2 if !instruction.returns_from_frame() => (LiteralStep::ObjectPair { site: instruction.object_site_index() },
            result(), [Some(instruction.register_b()), Some(instruction.register_c())]),
        Op::MakeObjectLiteral if !instruction.returns_from_frame() => {
            let window = instruction.register_window();
            (LiteralStep::ObjectLiteral { site: instruction.object_site_index(), base: window.base, count: window.count },
                result(), [None, None])
        }
        Op::MakeClosure => (LiteralStep::Closure { function: instruction.closure_function_index() }, result(), [None, None]),
        Op::MakeLiteralTemplate => (LiteralStep::LiteralTemplate { template: instruction.literal_template_index() }, result(), [None, None]),
        Op::CreateRegExpLiteral => (LiteralStep::RegExp { site: instruction.regexp_literal_site_index() }, result(), [None, None]),
        Op::CacheTemplateObject => (LiteralStep::CacheTemplate { site: instruction.template_site_index() },
            Some(instruction.register_a()), [Some(instruction.register_a()), None]),
        Op::LoadCachedTemplateObject => (LiteralStep::LoadTemplate { site: instruction.template_site_index() }, result(), [None, None]),
        Op::DefineArrayElement => (LiteralStep::DefineArrayElement { index: instruction.array_index() }, None,
            [Some(instruction.register_b()), Some(instruction.register_a())]),
        Op::CopyDataProperties => (LiteralStep::CopyDataProperties { exclusions: instruction.register_c() }, None,
            [Some(instruction.register_a()), Some(instruction.register_b())]),
        Op::DefinePropertyRecord => (LiteralStep::DefinePropertyRecord { mode: instruction.property_definition_mode()?,
            value: instruction.register_a() }, None, [Some(instruction.register_b()), Some(instruction.register_c())]),
        _ => return None,
    };
    Some((Protocol::Literal(step), destination, operands))
}

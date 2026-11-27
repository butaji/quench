//! Runtime glue of `protocol_literals`: the decode of a literal step from its record.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Op, WideInstruction};
use crate::staged::Protocol;

/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
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
        Op::DefinePropertyRecord => (LiteralStep::DefinePropertyRecord { mode: instruction.property_definition_mode()?.word(),
            value: instruction.register_a() }, None, [Some(instruction.register_b()), Some(instruction.register_c())]),
        _ => return None,
    };
    Some((Protocol::Literal(step), destination, operands))
}

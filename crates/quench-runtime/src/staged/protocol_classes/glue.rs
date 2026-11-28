//! Runtime glue of `protocol_classes`: the decode of a class step from its record.
#![allow(unused_imports)]
use super::*;
use crate::bytecode::{Op, WideInstruction};
use crate::staged::Protocol;

/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
    let (step, destination, operands) = match instruction.op() {
        Op::DefineField => (ClassStep::DefineField { atom: instruction.atom_index(), site: instruction.cache_site_index() },
            None, [Some(instruction.register_a()), Some(instruction.register_b())]),
        Op::DefineComputedField => (ClassStep::DefineComputedField { key: instruction.register_c() },
            None, [Some(instruction.register_a()), Some(instruction.register_b())]),
        Op::DefinePrivateField => (ClassStep::DefinePrivateField { atom: instruction.atom_index(), home: instruction.register_c() },
            None, [Some(instruction.register_a()), Some(instruction.register_b())]),
        Op::MarkPrivateName => (ClassStep::MarkPrivateName { atom: instruction.atom_index() },
            None, [Some(instruction.register_b()), Some(instruction.register_c())]),
        Op::CheckPrivate => (ClassStep::CheckPrivate { atom: instruction.atom_index() },
            None, [Some(instruction.register_a()), None]),
        Op::PrivateIn => (ClassStep::PrivateIn { atom: instruction.atom_index() },
            Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::SetFunctionName => (ClassStep::SetFunctionName { atom: instruction.atom_index(), site: instruction.cache_site_index() },
            None, [Some(instruction.register_a()), None]),
        Op::SetFunctionNameKey => (ClassStep::SetFunctionNameKey { prefix: instruction.function_name_prefix() },
            None, [Some(instruction.register_a()), Some(instruction.register_b())]),
        Op::SetFunctionDebugName => (ClassStep::SetFunctionDebugName { atom: instruction.atom_index() },
            None, [Some(instruction.register_a()), None]),
        Op::ValidateClassHeritage => (ClassStep::ValidateClassHeritage, None, [Some(instruction.register_a()), None]),
        Op::SuperCallCheck => (ClassStep::SuperCallCheck, None, [None, None]),
        Op::InitializeThis => (ClassStep::InitializeThis, None, [Some(instruction.register_a()), None]),
        Op::LoadCallee => (ClassStep::LoadCallee, Some(instruction.result_register()), [None, None]),
        Op::LoadImportMeta => (ClassStep::LoadImportMeta, Some(instruction.result_register()), [None, None]),
        _ => return None,
    };
    Some((Protocol::Class(step), destination, operands))
}

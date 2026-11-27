//! Runtime glue of `names`: the decode of the name, environment and iteration operations from a record
//! (`decode`, `decode_protocol`), the clone plan mask, and the name read of `decode_load_name`.
#![allow(unused_imports)]
use super::*;

/// The name read of `decode` (`LoadName`, `LoadNameCall`) under a proof of its global.
pub trait OperationNamesExt<W, P>: Sized {
    fn decode_load_name(instruction: WideInstruction, proof: P) -> Option<Self>;
}

impl<W, P> OperationNamesExt<W, P> for Operation<W, P> {
    #[cfg(not(stencil_build))]
    /// The name read `instruction` (`LoadName`, or a call's `LoadNameCall`) denotes under the
    /// tier's `proof` of the global it names; `None` for any other opcode.
    #[inline(always)]
    fn decode_load_name(instruction: WideInstruction, proof: P) -> Option<Self> {
        let receiver = match instruction.op() {
            Op::LoadName => None,
            Op::LoadNameCall => Some(instruction.register_b()),
            _ => return None,
        };
        Some(Self::LoadName { destination: instruction.result_register(), atom: instruction.atom_index(),
            site: instruction.cache_site_index(), receiver, proof })
    }
}

#[cfg(not(stencil_build))]
/// The bits of a `CloneEnv`'s immediate that name its clone plan: an environment window's record
/// (`dispatch_fast::specialize_environment_window`) carries its register count above them.
pub const ENVIRONMENT_CLONE_PLAN_MASK: u32 = u16::MAX as u32;

#[cfg(not(stencil_build))]
/// The staged operation `instruction` denotes among the name, environment and iteration
/// opcodes; `None` for any other opcode.
#[inline(always)]
pub fn decode<W, P>(instruction: WideInstruction) -> Option<Operation<W, P>> {
    let destination = move || instruction.result_register();
    let slot = move || Some(crate::staged::window_slot(instruction.imm()));
    Some(match instruction.op() {
        Op::CloneEnv => Operation::CloneEnv { plan: instruction.imm() & ENVIRONMENT_CLONE_PLAN_MASK },
        Op::LoadEnvLocal => Operation::LoadEnvLocal { destination: destination(), slot: slot()? },
        Op::StoreEnvLocal => Operation::StoreEnvLocal { source: instruction.register_a(),
            slot: slot()?, result: instruction.optional_register_b(),
            initialize: instruction.boolean_field(crate::bytecode::InstructionField::C).unwrap_or(false) },
        Op::ArgumentsLength => Operation::ArgumentsLength { destination: destination(), slot: slot()? },
        Op::ArgumentGet => Operation::ArgumentGet { destination: destination(), index: instruction.register_b(), slot: slot()? },
        Op::LoadNameTypeof => Operation::GlobalTypeof { destination: destination(), atom: instruction.atom_index(), site: instruction.cache_site_index() },
        Op::SetFunctionName => Operation::SetFunctionName { function: instruction.register_a(), name: instruction.atom_index(),
            site: instruction.cache_site_index() },
        Op::StoreName => Operation::GlobalWrite { source: instruction.register_a(), atom: instruction.atom_index(),
            site: instruction.cache_site_index(),
            initialize: instruction.boolean_field(crate::bytecode::InstructionField::B)? },
        Op::LoadGlobalLexical => Operation::GlobalLexicalLoad { destination: destination(), declaration: instruction.imm(),
            site: instruction.cache_site_index() },
        Op::StoreGlobalLexical => Operation::GlobalLexicalStore { source: instruction.register_a(), declaration: instruction.imm(),
            site: instruction.cache_site_index(), initialize: instruction.boolean_field(crate::bytecode::InstructionField::B)? },
        Op::ForInNext => Operation::ForInNext { destination: destination(), state: instruction.register_b() },
        Op::GetEnumerated => Operation::EnumeratedGet { destination: destination(), base: instruction.register_b(),
            key: instruction.register_c(), state: crate::bytecode::ImmediateLayout::register_pair(instruction.imm()).0 },
        Op::IteratorCleanupPush => Operation::IteratorCleanupPush { iterator: instruction.register_a(), done: instruction.register_b() },
        Op::IteratorCleanupPop => Operation::IteratorCleanupPop,
        _ => return decode_protocol(instruction),
    })
}

#[cfg(not(stencil_build))]
/// The iteration and spread steps of `decode`, with the registers `Vm::call_out` reads.
#[inline(always)]
fn decode_protocol<W, P>(instruction: WideInstruction) -> Option<Operation<W, P>> {
    let (step, destination, operands) = match instruction.op() {
        Op::AppendArray => (Protocol::AppendArray { length: instruction.array_length(),
            spread: instruction.boolean_field(crate::bytecode::InstructionField::C)? },
            None, [Some(instruction.register_a()), instruction.optional_register_b()]),
        Op::GetIterator => (Protocol::GetIterator, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::ForInStart => (Protocol::ForInStart, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::IteratorClose => (Protocol::IteratorClose, Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::ConstantFieldsHeld => (Protocol::ConstantFieldsHeld { site: instruction.cache_site_index() },
            Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::RestLength => (Protocol::RestView { get: false, fixed: instruction.array_length(), rest: instruction.register_b() },
            Some(instruction.result_register()), [None, Some(instruction.register_b())]),
        Op::RestGet => (Protocol::RestView { get: true, fixed: instruction.array_length(), rest: instruction.register_c() },
            Some(instruction.result_register()), [Some(instruction.register_b()), Some(instruction.register_c())]),
        // A sequence of tests, not `Option::or_else`: its closures left out of line kept every
        // decoder's test at run time in a row whose opcode is a constant (`generated_row`).
        _ => {
            let decoded = if let Some(decoded) = crate::staged::protocol_literals::decode(instruction) { decoded }
                else if let Some(decoded) = crate::staged::protocol_classes::decode(instruction) { decoded }
                else if let Some(decoded) = crate::staged::protocol_values::decode(instruction) { decoded }
                else if let Some(decoded) = crate::staged::protocol_owned::decode(instruction) { decoded }
                else if let Some(decoded) = crate::staged::protocol_wasm::decode(instruction) { decoded }
                else { return crate::staged::spread::decode(instruction) };
            decoded
        }
    };
    Some(Operation::Protocol { step, destination, operands })
}


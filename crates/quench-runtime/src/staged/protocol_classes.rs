//! The class, private-name and function-naming opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper, which the host makes
//! (`Vm::call_out` -> `Vm::class_step`).
use super::Protocol;
use crate::bytecode::Register;
#[cfg(not(stencil_build))]
use crate::bytecode::{Op, WideInstruction};

/// A class, private-name and function-naming step the host makes by the interpreter's own definition.
///
/// The operand registers the decoder names are read by `Vm::call_out`: `first` is the first operand's value and
/// `second` the second's. Any further register a step needs is named in the variant and read by the host.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum ClassStep {
    /// Define the own field `atom` of the object (`second`) with the value (`first`), through cache site `site`
    /// (`DefineField`).
    DefineField { atom: u32, site: u16 },
    /// Define the computed-key field of the object (`second`) keyed by register `key`, with the value (`first`)
    /// (`DefineComputedField`).
    DefineComputedField { key: Register },
    /// Initialize the private field `atom` of the target (`second`) under the home in register `home`, with the
    /// value (`first`) (`DefinePrivateField`).
    DefinePrivateField { atom: u32, home: Register },
    /// Brand the object (`first`) with the private method name `atom` of the home (`second`) (`MarkPrivateName`).
    MarkPrivateName { atom: u32 },
    /// Check the object (`first`) holds the private member `atom` (`CheckPrivate`).
    CheckPrivate { atom: u32 },
    /// `#atom in object` of the object (`second`), defining the boolean (`PrivateIn`).
    PrivateIn { atom: u32 },
    /// Name the function (`first`) `atom`, through cache site `site` (`SetFunctionName`).
    SetFunctionName { atom: u32, site: u16 },
    /// Name the function (`first`) by the key (`second`) with the prefix `prefix` (`SetFunctionNameKey`).
    SetFunctionNameKey { prefix: u32 },
    /// Set the debug name of the function (`first`) to `atom` (`SetFunctionDebugName`).
    SetFunctionDebugName { atom: u32 },
    /// Check the heritage (`first`) of a class is a constructor or `null` (`ValidateClassHeritage`).
    ValidateClassHeritage,
    /// Check the frame's `super()` call bound `this` (`SuperCallCheck`).
    SuperCallCheck,
    /// Bind the frame's `this` to the value (`first`) (`InitializeThis`).
    InitializeThis,
    /// The function the frame runs (`LoadCallee`), defined into the destination.
    LoadCallee,
    /// The import-meta object of the program, defined into the destination (`LoadImportMeta`).
    LoadImportMeta,
}

#[cfg(not(stencil_build))]
/// The step `instruction` denotes among these opcodes, with the register it defines and its (at
/// most two) operand registers, which `Vm::call_out` reads; `None` for any other opcode.
#[inline(always)]
pub(super) fn decode(instruction: WideInstruction) -> Option<(Protocol, Option<Register>, [Option<Register>; 2])> {
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

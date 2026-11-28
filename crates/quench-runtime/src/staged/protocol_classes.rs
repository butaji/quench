//! The class, private-name and function-naming opcodes as staged call-out rows (`Operation::Protocol`): each is one
//! definition every tier runs, and its step is the interpreter's own helper, which the host makes
//! (`Vm::call_out` -> `Vm::class_step`). The decode of a step from its record is in `protocol_classes/glue.rs`.
use crate::bytecode::Register;

/// A class, private-name and function-naming step the host makes by the interpreter's own definition.
///
/// The operand registers the decoder names are read by `Vm::call_out`: `first` is the first operand's value and
/// `second` the second's. Any further register a step needs is named in the variant and read by the host.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ClassStep {
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
mod glue;
#[cfg(not(stencil_build))]
pub use glue::*;

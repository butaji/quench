//! What the JavaScript function tier may take, and the artifact it publishes.
//!
//! The tier writes no opcode meaning of its own. A function compiles through one of two
//! projections of the interpreter: a numeric function through the semantic SSA specializer
//! that frameless JIT bodies and linked AOT regions share (`compile_js_typed_numeric_region`),
//! any other through the call-threaded specializer over the interpreter's per-opcode semantic
//! entries (`compile_js_general_dispatch`). Neither matches a program's shape: an interpreter
//! change reaches both.

use crate::bytecode::{Function, Op, ProgramKind, ResidualProgram};

/// Native code and its result interpretation for a JavaScript function.
pub(super) struct JsJitArtifact {
    /// Machine code using the ABI selected by `result`.
    pub(super) code: Vec<u8>,
    /// The number of formal parameters read from the argument array.
    pub(super) params: usize,
    pub(super) result: JsJitResultKind,
    /// Directly emitted bytecode PCs for the shared general JS dispatcher.
    pub(super) general_dispatch_native_pcs: Vec<u8>,
}

pub(super) enum JsJitResultKind {
    /// Numeric bytecode regions emit the common VM native-entry ABI.
    SharedNumberEntry(super::compiled::NativeEntryMode, super::compiled::NativeNumberResult),
    /// A full function CFG whose operation blocks call the shared VM step
    /// helper and branch directly to the next validated bytecode PC.
    GeneralDispatch,
}

// Keep native regions short so an interpreter interrupt cannot be delayed by a large body.
const MAX_INSTRUCTIONS: usize = 256;
const MAX_REGISTERS: usize = 256;
pub(crate) const MAX_CODE_BYTES: usize = 64 * 1024;
const MAX_PARAMETERS: usize = 8;

/// Compile `function_id` as a numeric function through the shared semantic SSA specializer;
/// any function it does not cover stays with the interpreter and the general tier.
pub(super) fn compile(program: &ResidualProgram, function_id: u32) -> Option<JsJitArtifact> {
    if program.kind == ProgramKind::Wasm || function_id == 0 {
        return None;
    }
    let function = program.function_get(function_id as usize)?;
    if !function_candidate(function, program) || function.code.iter().any(|instruction| instruction.is_wide()) {
        return None;
    }
    super::compiled::compile_js_typed_numeric_region(program, function_id)
}

pub(super) fn candidate_rejection_reason(program: &ResidualProgram, function_id: u32) -> &'static str {
    let Some(function) = program.function_get(function_id as usize) else {
        return "invalid-function-id";
    };
    if function.code.is_empty() {
        return "empty-function";
    }
    if function.code.len() > MAX_INSTRUCTIONS {
        return "bytecode-limit";
    }
    if usize::from(function.registers) > MAX_REGISTERS {
        return "register-limit";
    }
    if usize::from(function.params) > MAX_PARAMETERS {
        return "parameter-limit";
    }
    if !eligible(function, program) {
        return "unsupported-activation-or-function-kind";
    }
    "unsupported-lowering-shape-or-codegen-failure"
}

pub(super) fn function_candidate(function: &Function, program: &ResidualProgram) -> bool {
    *function.js_tier_candidate.get_or_init(|| scanned_function_candidate(function, program))
}

fn scanned_function_candidate(function: &Function, program: &ResidualProgram) -> bool {
    !function.code.is_empty()
        && function.code.len() <= MAX_INSTRUCTIONS
        && function.wide.is_empty()
        && usize::from(function.registers) <= MAX_REGISTERS
        && usize::from(function.params) <= MAX_PARAMETERS
        && eligible(function, program)
}

pub(super) fn eligible(function: &Function, program: &crate::bytecode::ResidualProgram) -> bool {
    // An arrow's `this` is its environment's, which this tier does not bind.
    if function.parent.is_none()
        || function.is_arrow
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || function.class_initializer
        || function.instance_initializer.is_some()
        || function.rest
        || !function.simple_parameters
        || !function.handlers.is_empty()
        || (!function.plain_activation(program)
            && !activation_is_irrelevant(function))
    {
        return false;
    }

    let params = usize::from(function.params);
    if params > usize::from(function.registers) || params > function.local_registers.len() {
        return false;
    }
    function
        .local_registers
        .iter()
        .take(params)
        .enumerate()
        .all(|(index, binding)| {
            usize::from(binding.local) == index && usize::from(binding.register) == index
        })
}

/// Some closures need a non-plain environment only because their source
/// nesting supplies one; a pure numeric body that never reads it has no
/// observable dependency on that activation. Keep this proof deliberately
/// narrow so calls, captures, dynamic names, `this`, and closure creation stay
/// on the interpreter path.
fn activation_is_irrelevant(function: &Function) -> bool {
    function.arguments_slot.is_none()
        && !function.inherited_with_scope
        && function.wide.is_empty()
        && function.code.iter().all(|packed| {
            matches!(
                packed.as_wide().op(),
                Op::LoadConst
                    | Op::Move
                    | Op::Unary
                    | Op::Binary
                    | Op::NumericAdd
                    | Op::NumericMultiply
                    | Op::LoadLocal
                    | Op::LoadLocalPlain
                    | Op::StoreLocal
                    | Op::StoreLocalPlain
                    | Op::Jump
                    | Op::JumpFalse
                    | Op::JumpTrue
                    | Op::JumpBinaryFalse
                    | Op::Return
            )
        })
}

/// What the tier finds in a function at its entries. Each fact depends only on the function and
/// its own program, which never change, so it is derived once per function
/// (`ProgramStore::js_entry_facts`).
#[derive(Default)]
pub(super) struct JsEntryFacts {
    /// Whether the function is a numeric tier candidate (`function_candidate`); only such a
    /// function enters native code at its entry before the general tier holds it.
    pub(super) candidate: bool,
    /// Calls whose cost a frameless unit can remove by specializing their targets.
    pub(super) has_calls: bool,
    /// Whether the general baseline admits it, and without backward jumps
    /// (`js_general_dispatch_shape`).
    pub(super) general: bool,
    pub(super) general_acyclic: bool,
}

impl JsEntryFacts {
    /// Whether every fact is unset (`Self::default`).
    pub(super) const fn is_unset(&self) -> bool {
        !self.candidate && !self.general
    }

    pub(super) fn of(program: &ResidualProgram, function: &Function, priced: bool) -> Self {
        let has_calls = function.instructions().any(|instruction|
            instruction.op().control_flow_layout() == crate::bytecode::ControlFlowLayout::Call);
        let candidate = function_candidate(function, program) && (!priced || has_calls || super::compiled::pays_native_entry(function));
        let shape = super::compiled::js_general_dispatch_shape(function);
        Self {
            candidate,
            has_calls,
            general: shape.is_some(),
            general_acyclic: shape == Some(true),
        }
    }
}

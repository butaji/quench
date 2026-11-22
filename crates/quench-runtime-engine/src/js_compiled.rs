//! What the JavaScript function tier may take, and the artifact it publishes.
//!
//! The tier writes no opcode meaning of its own. A numeric function compiles through the
//! semantic SSA specializer that frameless JIT bodies and linked AOT regions share
//! (`compile_js_typed_numeric_region`): an interpreter change reaches it.

use quench_runtime::bytecode::{Function, LazyFlag, ProgramKind, ResidualProgram, Traits};

/// Native code and its result interpretation for a JavaScript function.
pub(super) struct JsJitArtifact {
    /// Machine code of the lane body ABI.
    pub(super) code: Vec<u8>,
    /// The number of formal parameters read from the argument array.
    pub(super) params: usize,
    pub(super) result: crate::compiled::NativeNumberResult,
}

use crate::compiled::tier_policy::{JS_COMPILED_MAX_INSTRUCTIONS, JS_COMPILED_MAX_REGISTERS};
const MAX_PARAMETERS: usize = 8;

/// Compile `function_id` as a numeric function through the shared semantic SSA specializer;
/// any function it does not cover stays with the interpreter and the lane.
#[cfg(test)]
pub(super) fn compile(program: &ResidualProgram, function_id: u32) -> Option<JsJitArtifact> {
    admitted(program, function_id)?;
    crate::compiled::compile_js_typed_numeric_region(program, function_id)
}

/// The numeric function's image, compiling on the code generator's thread (`NumericImage::start`);
/// `None` where `compile` would decline before compiling.
pub(super) fn start(program: &ResidualProgram, function_id: u32) -> Option<quench_runtime::vm::jit_backend::OffThread<Option<crate::compiled::NumericImage>>> {
    admitted(program, function_id)?;
    crate::compiled::NumericImage::start(program, function_id)
}

fn admitted(program: &ResidualProgram, function_id: u32) -> Option<()> {
    if program.kind == ProgramKind::Wasm || function_id == 0 {
        return None;
    }
    let function = program.function_get(function_id as usize)?;
    (function_candidate(function, program) && !function.code.iter().any(|instruction| instruction.is_wide())).then_some(())
}

pub(super) fn candidate_rejection_reason(program: &ResidualProgram, function_id: u32) -> &'static str {
    let Some(function) = program.function_get(function_id as usize) else {
        return "invalid-function-id";
    };
    if function.code.is_empty() {
        return "empty-function";
    }
    if function.code.len() > JS_COMPILED_MAX_INSTRUCTIONS {
        return "bytecode-limit";
    }
    if usize::from(function.registers) > JS_COMPILED_MAX_REGISTERS {
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
    function.lazy_flag(LazyFlag::JsTierCandidate, || scanned_function_candidate(function, program))
}

fn scanned_function_candidate(function: &Function, program: &ResidualProgram) -> bool {
    !function.code.is_empty()
        && function.code.len() <= JS_COMPILED_MAX_INSTRUCTIONS
        && function.wide.is_empty()
        && usize::from(function.registers) <= JS_COMPILED_MAX_REGISTERS
        && usize::from(function.params) <= MAX_PARAMETERS
        && eligible(function, program)
}

pub(super) fn eligible(function: &Function, program: &quench_runtime::bytecode::ResidualProgram) -> bool {
    // An arrow's `this` is its environment's, which this tier does not bind.
    if function.parent.is_none()
        || function.is_arrow
        || function.is_async
        || function.is_generator
        || function.is_class_constructor
        || function.derived_constructor
        || function.class_initializer
        || function.instance_initializer().is_some()
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
        && function.code.iter().all(|packed| packed.as_wide().op().traits().has(Traits::ACTIVATION_FREE))
}

pub(super) use quench_runtime::vm::engine_types::JsEntryFacts;

#[quench_macros::ext(JsEntryFactsOps1)]
impl JsEntryFacts {
    /// Whether every fact is unset (`Self::default`).
    pub(super) const fn is_unset(&self) -> bool {
        !self.candidate
    }

    pub(super) fn of(program: &ResidualProgram, function: &Function, priced: bool) -> Self {
        let has_calls = function.instructions().any(|instruction|
            instruction.op().control_flow_layout() == quench_runtime::bytecode::ControlFlowLayout::Call);
        Self { candidate: function_candidate(function, program) && (!priced || has_calls || crate::compiled::pays_native_entry(function)) }
    }
}

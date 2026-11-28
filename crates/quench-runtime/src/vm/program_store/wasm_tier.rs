use std::rc::Rc;

/// A Wasm function's root tier: its one compile attempt, claimed at its first call, or the artifact
/// that attempt made. A function with no state has not been attempted. Retiring the artifact leaves it
/// `Attempted`.
#[derive(Clone)]
pub(in crate::vm) enum WasmTierState {
    Attempted,
    Compiled(Rc<crate::vm::engine_types::CompiledProgram>),
}

impl WasmTierState {
    pub(in crate::vm) fn compiled(&self) -> Option<&Rc<crate::vm::engine_types::CompiledProgram>> {
        match self {
            Self::Compiled(compiled) => Some(compiled),
            Self::Attempted => None,
        }
    }
}

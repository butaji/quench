use std::rc::Rc;

/// A Wasm function's selective tier: no compile attempt yet, the one attempt claimed, or its
/// artifact. Retiring the artifact leaves it `Attempted`.
#[derive(Clone)]
pub(in crate::vm) enum WasmTierState {
    Untried,
    Attempted,
    Compiled(Rc<crate::vm::engine_types::CompiledProgram>),
}

impl Default for WasmTierState {
    fn default() -> Self {
        Self::Untried
    }
}

impl WasmTierState {
    pub(in crate::vm) fn compiled(&self) -> Option<&Rc<crate::vm::engine_types::CompiledProgram>> {
        match self {
            Self::Compiled(compiled) => Some(compiled),
            Self::Untried | Self::Attempted => None,
        }
    }

    pub(in crate::vm) fn take_compiled(&mut self) -> Option<Rc<crate::vm::engine_types::CompiledProgram>> {
        match std::mem::replace(self, Self::Attempted) {
            Self::Compiled(compiled) => Some(compiled),
            other => {
                *self = other;
                None
            }
        }
    }
}

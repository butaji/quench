use std::rc::Rc;

/// A Wasm function's selective tier: counting entries toward its one compile attempt, the
/// attempt claimed, or its artifact. Retiring the artifact leaves it `Attempted`.
#[derive(Clone)]
pub(in crate::vm) enum WasmTierState {
    Counting(u8),
    Attempted,
    Compiled(Rc<crate::vm::compiled::CompiledProgram>),
}

impl Default for WasmTierState {
    fn default() -> Self {
        Self::Counting(0)
    }
}

impl WasmTierState {
    pub(in crate::vm) fn compiled(&self) -> Option<&Rc<crate::vm::compiled::CompiledProgram>> {
        match self {
            Self::Compiled(compiled) => Some(compiled),
            Self::Counting(_) | Self::Attempted => None,
        }
    }

    pub(in crate::vm) fn take_compiled(&mut self) -> Option<Rc<crate::vm::compiled::CompiledProgram>> {
        match std::mem::replace(self, Self::Attempted) {
            Self::Compiled(compiled) => Some(compiled),
            other => {
                *self = other;
                None
            }
        }
    }
}

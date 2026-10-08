//! Exception tag declarations; live identity belongs to the shared heap.

use super::{WasmImportName, WasmTypes};
use crate::Diagnostic;

#[derive(Clone, Debug)]
pub struct WasmTag {
    pub ty: u32,
    pub import: Option<WasmImportName>,
}

impl WasmTag {
    pub(crate) fn validate(&self, name: &str, types: &WasmTypes) -> Result<(), Diagnostic> {
        let signature = types.function(name, self.ty as usize)?;
        if !signature.results().is_empty() {
            return Err(Diagnostic::unsupported(
                name,
                "non-empty Wasm tag result type",
            ));
        }
        Ok(())
    }
}

/// Complete exception-construction window: tag followed by its payload.
#[repr(u16)]
pub(crate) enum ExceptionInput {
    Tag,
    Payload,
}
impl ExceptionInput {
    pub(crate) const MIN_COUNT: u16 = Self::Payload as u16;
}

//! Exception tag declarations; live identity belongs to the shared heap.

use super::WasmImportName;

#[derive(Clone, Debug)]
pub struct WasmTag {
    pub ty: u32,
    pub import: Option<WasmImportName>,
}

/// Complete exception-construction window: tag followed by its payload.
#[repr(u16)]
pub enum ExceptionInput {
    Tag,
    Payload,
}
impl ExceptionInput {
    pub(crate) const MIN_COUNT: u16 = Self::Payload as u16;
}

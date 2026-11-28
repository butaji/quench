//! Test-only views of the heap's storage that production code reaches through `staged`.

use super::Heap;
use crate::Value;

impl Heap {
    /// The arena's value base, which the interpreter's staged reads address runs from
    /// (`staged::Table::PropertyValues`).
    #[quench_macros::test_visible]
    pub(crate) fn property_values_base(&self) -> *const Value {
        self.properties.values_base()
    }
}

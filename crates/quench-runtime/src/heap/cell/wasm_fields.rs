//! The storage of a Wasm GC object's fields and of a Wasm table's references, each behind
//! one word so their cells stay compact.

use crate::value::Value;

/// A Wasm GC object's field values in one allocation behind one word. A struct with a
/// descriptor keeps it after its fields and marks the slice; `Deref` reads the fields.
#[derive(Clone, Debug, Default)]
pub(crate) struct WasmGcFields(crate::heap::thin_slice::ThinBoxSlice<Value>);

impl WasmGcFields {
    /// `len` fields made by `field(index)`, then `descriptor`; `None` when one is missing
    /// or no room is left for them.
    pub(crate) fn try_new(len: usize, descriptor: Option<Value>, mut field: impl FnMut(usize) -> Option<Value>) -> Option<Self> {
        let total = len.checked_add(usize::from(descriptor.is_some()))?;
        let mut values = crate::heap::thin_slice::ThinBoxSlice::try_from_fn(total, |index| if index < len { field(index) } else { descriptor })?;
        if descriptor.is_some() {
            values.mark();
        }
        Some(Self(values))
    }

    pub(crate) fn descriptor(&self) -> Option<Value> {
        self.0.marked().then(|| self.0[self.0.len() - 1])
    }

    /// Every value the object holds, its descriptor included: the edges a trace follows.
    pub(crate) fn edges(&self) -> &[Value] {
        &self.0
    }

    pub(crate) fn allocated_bytes(&self) -> usize {
        self.0.allocated_bytes()
    }
}

impl From<Vec<Value>> for WasmGcFields {
    fn from(values: Vec<Value>) -> Self {
        Self(values.into())
    }
}

impl std::ops::Deref for WasmGcFields {
    type Target = [Value];
    #[inline(always)]
    fn deref(&self) -> &[Value] {
        let values = &*self.0;
        &values[..values.len() - usize::from(self.0.marked())]
    }
}

impl std::ops::DerefMut for WasmGcFields {
    fn deref_mut(&mut self) -> &mut [Value] {
        let fields = self.len();
        &mut self.0[..fields]
    }
}

/// A Wasm table's references and optional maximum, boxed together so the cell keeps four
/// words; element access reads the references through `Deref`.
#[derive(Clone, Debug, Default)]
pub(crate) struct WasmTableElements {
    pub(crate) values: Vec<Value>,
    pub(crate) maximum: Option<u64>,
}

macro_rules! deref_to_values {
    ($($owner:ty),*) => {$(
        impl std::ops::Deref for $owner {
            type Target = Vec<Value>;
            fn deref(&self) -> &Vec<Value> {
                &self.values
            }
        }

        impl std::ops::DerefMut for $owner {
            fn deref_mut(&mut self) -> &mut Vec<Value> {
                &mut self.values
            }
        }
    )*};
}
deref_to_values!(WasmTableElements);

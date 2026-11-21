use super::cell::{Cell, Object};

impl Cell {
    pub(crate) fn typed_array_backing(&self) -> Option<(&Object, crate::Value)> {
        match self {
            Self::TypedArray { object, buffer, .. } => Some((object, *buffer)),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn object(&self) -> Option<&Object> {
        match self {
            // The rest box their object so the cell stays small.
            Self::RegExp { object, .. } => Some(object),
            Self::ArrayBuffer { object, .. }
            | Self::ShadowRealm { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::FinalizationRegistry { object, .. }
            | Self::Date { object, .. }
            | Self::Proxy { object, .. }
            | Self::TemporalDuration { object, .. }
            | Self::TemporalPlainDate { object, .. }
            | Self::TemporalPlainDateTime { object, .. }
            | Self::TemporalPlainMonthDay { object, .. }
            | Self::TemporalPlainYearMonth { object, .. }
            | Self::TemporalZonedDateTime { object, .. }
            | Self::TemporalInstant { object, .. }
            | Self::WasmObject { object, .. }
            | Self::WasmTag { object, .. }
            | Self::WasmTable { object, .. } => Some(object),
            Self::Promise(promise) => Some(&promise.object),
            Self::WasmGlobal { header, .. } => Some(&header.object),
            Self::WasmMemory { header, .. } => Some(&header.object),
            Self::TypedArray { object, .. } | Self::DataView { object, .. } => Some(&object.object),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn object_mut(&mut self) -> Option<&mut Object> {
        match self {
            // The rest box their object so the cell stays small.
            Self::RegExp { object, .. } => Some(object),
            Self::ArrayBuffer { object, .. }
            | Self::ShadowRealm { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::FinalizationRegistry { object, .. }
            | Self::Date { object, .. }
            | Self::Proxy { object, .. }
            | Self::TemporalDuration { object, .. }
            | Self::TemporalPlainDate { object, .. }
            | Self::TemporalPlainDateTime { object, .. }
            | Self::TemporalPlainMonthDay { object, .. }
            | Self::TemporalPlainYearMonth { object, .. }
            | Self::TemporalZonedDateTime { object, .. }
            | Self::TemporalInstant { object, .. }
            | Self::WasmObject { object, .. }
            | Self::WasmTag { object, .. }
            | Self::WasmTable { object, .. } => Some(object),
            Self::Promise(promise) => Some(&mut promise.object),
            Self::WasmGlobal { header, .. } => Some(&mut header.object),
            Self::WasmMemory { header, .. } => Some(&mut header.object),
            Self::TypedArray { object, .. } | Self::DataView { object, .. } => {
                Some(&mut object.object)
            }
            _ => None,
        }
    }
}

impl super::Heap {
    /// The object part of a builtin whose named properties are all ordinary: a collection,
    /// date, regexp, promise, iterator, DataView, buffer, weak ref or finalization registry.
    /// No proxy, exotic or integer-indexed object.
    #[inline(always)]
    pub(crate) fn ordinary_builtin_object(&self, value: crate::Value) -> Option<&Object> {
        // A record family reads its record at once, past `object`'s tests of the others.
        if let Some(record_id) = value.direct_iterator_index() {
            // SAFETY: a direct-iterator Value names an initialized iterator record.
            return Some(&unsafe { self.iterator_records.get_live(record_id) }.object);
        }
        if let Some(record_id) = value.direct_promise_index() {
            // SAFETY: a direct-promise Value names an initialized promise record.
            return Some(&unsafe { self.promise_records.get_live(record_id) }.object);
        }
        if let Some(record_id) = value.direct_collection_index() {
            // SAFETY: a direct-collection Value names an initialized collection record.
            return Some(&unsafe { self.collection_records.get_live(record_id) }.object);
        }
        self.ordinary_builtin_cell_object(value)
    }

    /// `ordinary_builtin_object` of a value no record family names: a heap cell's object, out of
    /// line so the record families' reads stay inline at every caller.
    #[inline(never)]
    fn ordinary_builtin_cell_object(&self, value: crate::Value) -> Option<&Object> {
        let cell = self.get(value)?;
        // A DataView's and a buffer's geometry is accessors of their prototypes, so their own
        // names are their shape's too. A typed array is not here: every canonical numeric
        // string, not only an index, is its own element.
        matches!(cell, Cell::WeakMap { .. } | Cell::WeakSet { .. }
            | Cell::Date { .. } | Cell::RegExp { .. } | Cell::Promise(_)
            | Cell::DataView { .. } | Cell::ArrayBuffer { .. } | Cell::WeakRef { .. } | Cell::FinalizationRegistry { .. })
            .then(|| cell.object()).flatten()
    }
}

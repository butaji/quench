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
            // The hot object kinds keep their object inline; the rest box it so the
            // cell stays small.
            Self::Object(object) => Some(object),
            Self::Array { object } => Some(object),
            Self::ArrayBuffer { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::ShadowRealm { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::FinalizationRegistry { object, .. }
            | Self::Date { object, .. }
            | Self::RegExp { object, .. }
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
            Self::Function(function) => Some(&function.object),
            Self::Iterator(iterator) => Some(&iterator.object),
            Self::TypedArray { object, .. } | Self::DataView { object, .. } => Some(&object.object),
            _ => None,
        }
    }

    #[inline]
    pub(crate) fn object_mut(&mut self) -> Option<&mut Object> {
        match self {
            // The hot object kinds keep their object inline; the rest box it so the
            // cell stays small.
            Self::Object(object) => Some(object),
            Self::Array { object } => Some(object),
            Self::ArrayBuffer { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::ShadowRealm { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::FinalizationRegistry { object, .. }
            | Self::Date { object, .. }
            | Self::RegExp { object, .. }
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
            Self::Function(function) => Some(&mut function.object),
            Self::Iterator(iterator) => Some(&mut iterator.object),
            Self::TypedArray { object, .. } | Self::DataView { object, .. } => {
                Some(&mut object.object)
            }
            _ => None,
        }
    }
}

impl super::Heap {
    /// The object part of a builtin whose named properties are all ordinary: a collection,
    /// date, regexp, promise or iterator. No proxy, exotic or integer-indexed object.
    #[inline(never)]
    pub(crate) fn ordinary_builtin_object(&self, value: crate::Value) -> Option<&Object> {
        if value.direct_promise_index().is_some() || value.direct_iterator_index().is_some() {
            return self.object(value);
        }
        let cell = self.get(value)?;
        matches!(cell, Cell::Map { .. } | Cell::Set { .. } | Cell::WeakMap { .. } | Cell::WeakSet { .. }
            | Cell::Date { .. } | Cell::RegExp { .. } | Cell::Promise(_) | Cell::Iterator(_))
            .then(|| cell.object()).flatten()
    }
}

//! The kinds of cells and the bytes each holds, for the heap census and profiles.
use super::*;

impl Heap {
    pub(super) fn cell_kind(cell: &Cell) -> CellKind {
        match cell {
            Cell::Object(_)
            | Cell::WasmObject { .. }
            | Cell::ArrayBuffer { .. }
            | Cell::TypedArray { .. }
            | Cell::DataView { .. }
            | Cell::ShadowRealm { .. }
            | Cell::Proxy { .. }
            | Cell::PromiseResolvingState { .. }
            | Cell::Promise(_) => CellKind::Object,
            Cell::Array { .. } => CellKind::Array,
            Cell::Map { .. } => CellKind::Map,
            Cell::Set { .. } => CellKind::Set,
            Cell::Iterator { .. } => CellKind::Iterator,
            Cell::WeakMap { .. } => CellKind::WeakMap,
            Cell::WeakSet { .. } => CellKind::WeakSet,
            Cell::WeakRef { .. } | Cell::FinalizationRegistry { .. } => CellKind::WeakRef,
            Cell::Function(_) => CellKind::Function,
            Cell::Environment { .. } | Cell::BindingReference { .. } => CellKind::Environment,
            Cell::BigInt(_) => CellKind::BigInt,
            Cell::WasmElements(_) | Cell::WasmInstance(_) => CellKind::WasmElements,
            Cell::WasmGlobal { .. } => CellKind::WasmGlobal,
            Cell::WasmMemory { .. } => CellKind::WasmMemory,
            Cell::WasmTable { .. } => CellKind::WasmTable,
            Cell::WasmBits64(_) => CellKind::WasmBits64,
            Cell::WasmV128(_) => CellKind::WasmV128,
            Cell::WasmExtern(_) => CellKind::WasmExtern,
            Cell::AccessorPair { .. } => CellKind::Function,
            Cell::WasmTag { .. } => CellKind::WasmTag,
            Cell::WasmException { .. } => CellKind::WasmException,
            Cell::WasmGc { .. } => CellKind::WasmGc,
            Cell::WasmHostFunction { .. } => CellKind::WasmHostFunction,
            Cell::Symbol(_) => CellKind::Symbol,
            Cell::Date { .. } => CellKind::Date,
            Cell::Error(_) => CellKind::Error,
            Cell::RegExp { .. } => CellKind::RegExp,
            Cell::ArrayFromAsyncState(_) => CellKind::ArrayFromAsync,
            Cell::AwaitState(_) => CellKind::AwaitState,
            Cell::TemporalDuration { .. } => CellKind::TemporalDuration,
            Cell::TemporalPlainDate { .. } => CellKind::TemporalPlainDate,
            Cell::TemporalPlainDateTime { .. } => CellKind::TemporalPlainDateTime,
            Cell::TemporalPlainMonthDay { .. } => CellKind::TemporalPlainMonthDay,
            Cell::TemporalPlainYearMonth { .. } => CellKind::TemporalPlainYearMonth,
            Cell::TemporalZonedDateTime { .. } => CellKind::TemporalZonedDateTime,
            Cell::TemporalInstant { .. } => CellKind::TemporalInstant,
        }
    }

    pub(super) fn cell_payload_bytes(cell: &Cell) -> usize {
        let object_extra_bytes = cell
            .object()
            .map_or(0, |object| object.allocated_extra_bytes());
        object_extra_bytes
            + match cell {
                Cell::Object(_)
                | Cell::WasmObject { .. }
                | Cell::ShadowRealm { .. }
                | Cell::Iterator { .. }
                | Cell::ArrayFromAsyncState(_)
                | Cell::AwaitState(_)
                | Cell::Proxy { .. }
                | Cell::Date { .. }
                | Cell::PromiseResolvingState { .. }
                | Cell::BindingReference { .. }
                | Cell::TemporalDuration { .. }
                | Cell::TemporalInstant { .. }
                | Cell::TypedArray { .. }
                | Cell::DataView { .. }
                | Cell::WeakRef { .. }
                | Cell::FinalizationRegistry { .. }
                | Cell::WasmGlobal { .. }
                | Cell::WasmInstance(_)
                | Cell::WasmBits64(_)
                | Cell::WasmV128(_)
                | Cell::WasmExtern(_)
                | Cell::AccessorPair { .. }
                | Cell::WasmTag { .. } => 0,
                Cell::Promise(promise) => size_of::<PromiseCell>() + promise.record.reaction_heap_bytes(),
                Cell::WasmException { payload, .. } => payload.capacity() * size_of::<Value>(),
                Cell::WasmHostFunction { target, signature } => {
                    size_of::<crate::WasmSignature>()
                        + (signature.params.capacity() + signature.results.capacity())
                            * size_of::<crate::WasmType>()
                        + if matches!(target, crate::heap::WasmHostTarget::JsEmscriptenMemcpy(_)) {
                            size_of::<crate::heap::WasmHostMemcpyTarget>()
                        } else {
                            0
                        }
                }
                Cell::WasmElements(elements) => elements.capacity() * size_of::<Value>(),
                Cell::WasmTable { elements, .. } => elements.external_bytes(),
                Cell::WasmGc { fields, .. } => fields.allocated_bytes(),
                Cell::TemporalPlainDate { calendar, .. }
                | Cell::TemporalPlainDateTime { calendar, .. }
                | Cell::TemporalPlainMonthDay { calendar, .. }
                | Cell::TemporalPlainYearMonth { calendar, .. } => calendar.capacity(),
                Cell::TemporalZonedDateTime {
                    instant, calendar, ..
                } => instant.time_zone.capacity() + calendar.capacity(),
                Cell::RegExp { meta, .. } => meta.source.capacity(),
                Cell::Array { object } => object.elements().capacity() * size_of::<Value>(),
                Cell::ArrayBuffer { bytes, .. } => bytes.capacity(),
                Cell::WasmMemory { bytes, .. } => bytes.capacity(),
                Cell::Map { entries, .. } => entries.capacity() * size_of::<(Value, Value)>(),
                Cell::Set { entries, .. } => entries.capacity() * size_of::<Value>(),
                Cell::WeakMap { entries, .. } => entries.allocated_bytes(),
                Cell::WeakSet { entries, .. } => entries.allocated_bytes(),
                Cell::Function(_) => size_of::<FunctionRecord>(),
                Cell::Environment { slots, scope, .. } => {
                    let with_objects = scope.with_objects();
                    slots.len() * size_of::<EnvironmentSlot>()
                        + std::mem::size_of_val(with_objects)
                }
                Cell::BigInt(value) => value.spilled_bytes(),
                Cell::Error(value) => value.capacity(),
                Cell::Symbol(value) => value.as_ref().map_or(0, |description| description.len()),
            }
    }
}

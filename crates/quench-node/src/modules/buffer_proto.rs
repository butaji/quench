//! Buffer prototype and view construction.
//!
//! `PROTOTYPE_METHODS` is the data table; `buffer_prototype` lowers
//! it once into a shared (non-enumerable) method object whose own
//! prototype is `Uint8Array.prototype`.

use std::cell::RefCell;
use std::rc::Rc;

use quench_runtime::value::{ArrayBufferData, Uint8ArrayData, Value};

use crate::registry as r;
use crate::registry::NodeSpec;

thread_local! {
    /// Shared `Buffer.prototype` stand-in: an object whose own
    /// prototype is `Uint8Array.prototype`, so Buffers differ from
    /// plain Uint8Arrays under `Object.getPrototypeOf` while
    /// inheriting typed-array lookups.
    static BUFFER_PROTOTYPE: RefCell<Option<Value>> = const { RefCell::new(None) };
}

/// Methods installed (non-enumerable) on `Buffer.prototype`.
const PROTOTYPE_METHODS: &[(&str, NodeSpec)] = &[
    ("toString", r::SPEC_BUFFER_TOSTRING),
    ("write", r::SPEC_BUFFER_WRITE),
    ("equals", r::SPEC_BUFFER_EQUALS),
    ("compare", r::SPEC_BUFFER_COMPARE),
    ("copy", r::SPEC_BUFFER_COPY),
    ("fill", r::SPEC_BUFFER_FILL),
    ("slice", r::SPEC_BUFFER_SLICE),
    ("swap16", r::SPEC_BUFFER_SWAP16),
    ("swap32", r::SPEC_BUFFER_SWAP32),
    ("swap64", r::SPEC_BUFFER_SWAP64),
    ("toJSON", r::SPEC_BUFFER_TOJSON),
    ("indexOf", r::SPEC_BUFFER_INDEX_OF),
    ("lastIndexOf", r::SPEC_BUFFER_LAST_INDEX_OF),
    ("includes", r::SPEC_BUFFER_INCLUDES),
    ("toLocaleString", r::SPEC_BUFFER_TOSTRING),
    ("inspect", r::SPEC_BUFFER_INSPECT),
    ("readUInt8", r::SPEC_BUF_READ_UINT8),
    ("readUint8", r::SPEC_BUF_READ_UINT8),
    ("readUInt16LE", r::SPEC_BUF_READ_UINT16_LE),
    ("readUint16LE", r::SPEC_BUF_READ_UINT16_LE),
    ("readUInt16BE", r::SPEC_BUF_READ_UINT16_BE),
    ("readUint16BE", r::SPEC_BUF_READ_UINT16_BE),
    ("readUInt32LE", r::SPEC_BUF_READ_UINT32_LE),
    ("readUint32LE", r::SPEC_BUF_READ_UINT32_LE),
    ("readUInt32BE", r::SPEC_BUF_READ_UINT32_BE),
    ("readUint32BE", r::SPEC_BUF_READ_UINT32_BE),
    ("readInt8", r::SPEC_BUF_READ_INT8),
    ("readInt16LE", r::SPEC_BUF_READ_INT16_LE),
    ("readInt16BE", r::SPEC_BUF_READ_INT16_BE),
    ("readInt32LE", r::SPEC_BUF_READ_INT32_LE),
    ("readInt32BE", r::SPEC_BUF_READ_INT32_BE),
    ("readFloatLE", r::SPEC_BUF_READ_FLOAT_LE),
    ("readFloatBE", r::SPEC_BUF_READ_FLOAT_BE),
    ("readDoubleLE", r::SPEC_BUF_READ_DOUBLE_LE),
    ("readDoubleBE", r::SPEC_BUF_READ_DOUBLE_BE),
    ("readBigInt64LE", r::SPEC_BUF_READ_BIGINT64_LE),
    ("readBigInt64BE", r::SPEC_BUF_READ_BIGINT64_BE),
    ("readBigUInt64LE", r::SPEC_BUF_READ_BIGUINT64_LE),
    ("readBigUint64LE", r::SPEC_BUF_READ_BIGUINT64_LE),
    ("readBigUInt64BE", r::SPEC_BUF_READ_BIGUINT64_BE),
    ("readBigUint64BE", r::SPEC_BUF_READ_BIGUINT64_BE),
    ("readUIntLE", r::SPEC_BUF_READ_UINT_LE),
    ("readUintLE", r::SPEC_BUF_READ_UINT_LE),
    ("readUIntBE", r::SPEC_BUF_READ_UINT_BE),
    ("readUintBE", r::SPEC_BUF_READ_UINT_BE),
    ("readIntLE", r::SPEC_BUF_READ_INT_LE),
    ("readIntBE", r::SPEC_BUF_READ_INT_BE),
    ("writeUInt8", r::SPEC_BUF_WRITE_UINT8),
    ("writeUint8", r::SPEC_BUF_WRITE_UINT8),
    ("writeUInt16LE", r::SPEC_BUF_WRITE_UINT16_LE),
    ("writeUint16LE", r::SPEC_BUF_WRITE_UINT16_LE),
    ("writeUInt16BE", r::SPEC_BUF_WRITE_UINT16_BE),
    ("writeUint16BE", r::SPEC_BUF_WRITE_UINT16_BE),
    ("writeUInt32LE", r::SPEC_BUF_WRITE_UINT32_LE),
    ("writeUint32LE", r::SPEC_BUF_WRITE_UINT32_LE),
    ("writeUInt32BE", r::SPEC_BUF_WRITE_UINT32_BE),
    ("writeUint32BE", r::SPEC_BUF_WRITE_UINT32_BE),
    ("writeInt8", r::SPEC_BUF_WRITE_INT8),
    ("writeInt16LE", r::SPEC_BUF_WRITE_INT16_LE),
    ("writeInt16BE", r::SPEC_BUF_WRITE_INT16_BE),
    ("writeInt32LE", r::SPEC_BUF_WRITE_INT32_LE),
    ("writeInt32BE", r::SPEC_BUF_WRITE_INT32_BE),
    ("writeFloatLE", r::SPEC_BUF_WRITE_FLOAT_LE),
    ("writeFloatBE", r::SPEC_BUF_WRITE_FLOAT_BE),
    ("writeDoubleLE", r::SPEC_BUF_WRITE_DOUBLE_LE),
    ("writeDoubleBE", r::SPEC_BUF_WRITE_DOUBLE_BE),
    ("writeBigInt64LE", r::SPEC_BUF_WRITE_BIGINT64_LE),
    ("writeBigInt64BE", r::SPEC_BUF_WRITE_BIGINT64_BE),
    ("writeBigUInt64LE", r::SPEC_BUF_WRITE_BIGUINT64_LE),
    ("writeBigUint64LE", r::SPEC_BUF_WRITE_BIGUINT64_LE),
    ("writeBigUInt64BE", r::SPEC_BUF_WRITE_BIGUINT64_BE),
    ("writeBigUint64BE", r::SPEC_BUF_WRITE_BIGUINT64_BE),
    ("writeUIntLE", r::SPEC_BUF_WRITE_UINT_LE),
    ("writeUintLE", r::SPEC_BUF_WRITE_UINT_LE),
    ("writeUIntBE", r::SPEC_BUF_WRITE_UINT_BE),
    ("writeUintBE", r::SPEC_BUF_WRITE_UINT_BE),
    ("writeIntLE", r::SPEC_BUF_WRITE_INT_LE),
    ("writeIntBE", r::SPEC_BUF_WRITE_INT_BE),
];
const ITERATOR_METHODS: &[(&str, quench_runtime::ops::Builtin)] = &[
    ("Symbol.iterator", quench_runtime::ops::Builtin::ArrayIterator),
    ("values", quench_runtime::ops::Builtin::ArrayIterator),
    ("keys", quench_runtime::ops::Builtin::ArrayKeys),
    ("entries", quench_runtime::ops::Builtin::ArrayEntries),
];

pub fn buffer_prototype() -> Value {
    BUFFER_PROTOTYPE.with(|slot| {
        if let Some(prototype) = &*slot.borrow() {
            return prototype.clone();
        }
        let mut to_string_fn: Option<Value> = None;
        let methods: Vec<(&str, Value)> = PROTOTYPE_METHODS
            .iter()
            .map(|(name, spec)| {
                // `toLocaleString` must be the same function object as
                // `toString` (identity is observable).
                let value = if spec.cap == r::SPEC_BUFFER_TOSTRING.cap {
                    to_string_fn
                        .get_or_insert_with(|| crate::host::capability(*spec))
                        .clone()
                } else {
                    crate::host::capability(*spec)
                };
                (*name, value)
            })
            .collect();
        let mut prototype = crate::host::namespace_object(methods)
            .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()));
        for (name, builtin) in ITERATOR_METHODS {
            prototype = quench_runtime::execute::set_property(
                prototype,
                name,
                Value::Builtin(*builtin),
            );
        }
        let _ = quench_runtime::execute::set_prototype_of(
            &prototype,
            &Value::Builtin(quench_runtime::ops::Builtin::Uint8ArrayPrototype),
        );
        slot.borrow_mut().replace(prototype.clone());
        prototype
    })
}

/// Attach Buffer identity to a view: shared prototype + `parent` and
/// `offset` own data properties.
fn finish_view(view: Value, buffer: &Rc<ArrayBufferData>, byte_offset: usize) -> Value {
    quench_runtime::execute::set_property(view.clone(), "\0prototype", buffer_prototype());
    quench_runtime::execute::set_property(
        view.clone(),
        "parent",
        Value::ArrayBuffer(buffer.clone()),
    );
    quench_runtime::execute::set_property(
        view.clone(),
        "offset",
        Value::Number(byte_offset as f64),
    );
    view
}

/// Construct a Buffer value from raw bytes (Node's `Buffer.from(bytes)`).
pub(crate) fn make_buffer(bytes: &[u8]) -> Value {
    let buf = Rc::new(ArrayBufferData::new(bytes.len()));
    buf.bytes.borrow_mut().copy_from_slice(bytes);
    make_view(buf, 0, bytes.len())
}

/// Construct a Buffer value over an existing ArrayBuffer (shared).
pub(crate) fn make_view(buffer: Rc<ArrayBufferData>, offset: usize, length: usize) -> Value {
    let view = Value::Uint8Array(Rc::new(Uint8ArrayData::new(buffer.clone(), offset, length)));
    finish_view(view, &buffer, offset)
}

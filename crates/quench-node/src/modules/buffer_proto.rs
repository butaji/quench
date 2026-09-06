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
    static BUFFER_POOL: RefCell<Option<(Rc<ArrayBufferData>, usize)>> = const { RefCell::new(None) };
}

/// Methods installed (non-enumerable) on `Buffer.prototype`.
const PROTOTYPE_METHODS: &[(&str, NodeSpec)] = &[
    ("toString", r::SPEC_BUFFER_TOSTRING),
    // Legacy encoding entry points share the canonical codec paths.
    ("asciiSlice", r::SPEC_BUFFER_TOSTRING),
    ("base64Slice", r::SPEC_BUFFER_TOSTRING),
    ("base64urlSlice", r::SPEC_BUFFER_TOSTRING),
    ("latin1Slice", r::SPEC_BUFFER_TOSTRING),
    ("hexSlice", r::SPEC_BUFFER_TOSTRING),
    ("ucs2Slice", r::SPEC_BUFFER_TOSTRING),
    ("utf8Slice", r::SPEC_BUFFER_TOSTRING),
    ("write", r::SPEC_BUFFER_WRITE),
    ("asciiWrite", r::SPEC_BUFFER_ASCII_WRITE),
    ("base64Write", r::SPEC_BUFFER_WRITE),
    ("base64urlWrite", r::SPEC_BUFFER_WRITE),
    ("hexWrite", r::SPEC_BUFFER_WRITE),
    ("ucs2Write", r::SPEC_BUFFER_WRITE),
    ("latin1Write", r::SPEC_BUFFER_LATIN1_WRITE),
    ("utf8Write", r::SPEC_BUFFER_UTF8_WRITE),
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
pub fn buffer_prototype() -> Value {
    BUFFER_PROTOTYPE.with(|slot| {
        if let Some(prototype) = &*slot.borrow() {
            return prototype.clone();
        }
        let mut capabilities = std::collections::HashMap::<u16, Value>::new();
        let methods: Vec<(&str, Value)> = PROTOTYPE_METHODS
            .iter()
            .map(|(name, spec)| {
                let value = capabilities
                    .entry(spec.cap)
                    .or_insert_with(|| crate::host::capability(*spec))
                    .clone();
                (*name, value)
            })
            .collect();
        let mut prototype = crate::host::namespace_object(methods)
            .unwrap_or_else(|_| quench_runtime::host_api::object(Vec::new()));
        let constructor = crate::host::capability(r::SPEC_BUFFER_NEW);
        prototype = quench_runtime::execute::set_property(
            prototype,
            "constructor",
            constructor.clone(),
        );
        quench_runtime::execute::set_property_in_place(
            &prototype,
            "\0quench:buffer:constructor",
            constructor,
        );
        prototype = quench_runtime::execute::set_property(
            prototype,
            "subarray",
            crate::host::capability(r::SPEC_BUFFER_SUBARRAY),
        );
        // `util.inspect.custom` is a global symbol. Store the same function
        // object as the named inspect method so identity and symbol lookup
        // follow Node's Buffer prototype contract.
        let inspect = quench_runtime::execute::get_property(&prototype, "inspect");
        prototype = quench_runtime::execute::set_property(
            prototype,
            "Symbol.for.nodejs.util.inspect.custom\0",
            inspect,
        );
        let _ = quench_runtime::execute::set_prototype_of(
            &prototype,
            &Value::Builtin(quench_runtime::ops::Builtin::Uint8ArrayPrototype),
        );
        slot.borrow_mut().replace(prototype.clone());
        prototype
    })
}

/// Stable constructor fact used by host-object serializers. The public
/// `constructor` property remains mutable like Node's, while this hidden
/// value lets native code distinguish that mutation from the original Buffer
/// identity without comparing freshly allocated capability wrappers.
pub(crate) fn canonical_buffer_constructor() -> Value {
    quench_runtime::execute::get_property(
        &buffer_prototype(),
        "\0quench:buffer:constructor",
    )
}

/// Attach Buffer identity to a view: shared prototype + `parent` and `offset` own data properties.
fn finish_view(mut view: Value, buffer: &Rc<ArrayBufferData>, byte_offset: usize) -> Value {
    // Typed-array prototype updates return a replacement value in the runtime
    // (the old value is not mutated in place). Keep that replacement so host
    // constructed Buffers actually expose Buffer.prototype methods.
    view = quench_runtime::execute::set_prototype_of(&view, &buffer_prototype()).unwrap_or(view);
    view =
        quench_runtime::execute::set_property(view, "parent", Value::ArrayBuffer(buffer.clone()));
    // Keep the callable entry point directly on host-created views as well;
    // this remains visible if typed-array prototype chaining is represented
    // by a proxy/replacement value.
    // Keep the callable entry point directly on host-created views.  Defining
    // it through the ordinary setter can return the backing ArrayBuffer for
    // host-created aliases, losing the view as the method receiver.
    let _ = quench_runtime::builtins::define_own_property_public(
        &view,
        "toString",
        &[
            (
                "value".into(),
                crate::host::capability(r::SPEC_BUFFER_TOSTRING),
            ),
            ("writable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(false)),
            ("configurable".into(), Value::Boolean(true)),
        ],
    );
    let _ = quench_runtime::builtins::define_own_property_public(
        &view,
        "offset",
        &[
            ("value".into(), Value::Number(byte_offset as f64)),
            ("writable".into(), Value::Boolean(true)),
            ("enumerable".into(), Value::Boolean(true)),
            ("configurable".into(), Value::Boolean(true)),
        ],
    );
    view
}

pub(crate) fn finish_view_for_methods(view: Value) -> Value {
    let Value::Uint8Array(data) = &view else {
        return view;
    };
    let buffer = data.buffer.clone();
    let offset = data.byte_offset;
    finish_view(view, &buffer, offset)
}

/// Construct a Buffer value from raw bytes (Node's `Buffer.from(bytes)`).
pub(crate) fn make_buffer(bytes: &[u8]) -> Value {
    let buf = Rc::new(ArrayBufferData::new(bytes.len()));
    buf.bytes.borrow_mut().copy_from_slice(bytes);
    make_view(buf, 0, bytes.len())
}

/// Small `Buffer.from(string)` allocations share Node's 8 KiB pool. The pool
/// is an identity fact, while the views retain independent offsets and lengths.
pub(crate) fn make_pooled_buffer(bytes: &[u8]) -> Value {
    make_pooled_buffer_aligned(bytes, 8)
}

pub(crate) fn make_pooled_buffer_aligned(bytes: &[u8], alignment: usize) -> Value {
    const POOL_SIZE: usize = 8192;
    const POOL_THRESHOLD: usize = POOL_SIZE / 2;
    if bytes.is_empty() || bytes.len() > POOL_THRESHOLD || alignment > 64 {
        return make_buffer(bytes);
    }
    BUFFER_POOL.with(|pool| {
        let mut pool = pool.borrow_mut();
        let (buffer, offset) = match pool.as_mut() {
            Some((buffer, offset)) => {
                let aligned = offset.saturating_add(alignment - 1) & !(alignment - 1);
                if aligned + bytes.len() <= POOL_SIZE {
                    (buffer.clone(), aligned)
                } else {
                    let mut pooled = ArrayBufferData::new(POOL_SIZE);
                    pooled.untransferable = true;
                    let buffer = Rc::new(pooled);
                    *pool = Some((buffer.clone(), 0));
                    (buffer, 0)
                }
            }
            _ => {
                let mut pooled = ArrayBufferData::new(POOL_SIZE);
                pooled.untransferable = true;
                let buffer = Rc::new(pooled);
                *pool = Some((buffer.clone(), 0));
                (buffer, 0)
            }
        };
        buffer.bytes.borrow_mut()[offset..offset + bytes.len()].copy_from_slice(bytes);
        if let Some((_, next)) = pool.as_mut() {
            *next = offset + bytes.len();
        }
        make_view(buffer, offset, bytes.len())
    })
}

/// Construct a Buffer value over an existing ArrayBuffer (shared).
pub fn make_view(buffer: Rc<ArrayBufferData>, offset: usize, length: usize) -> Value {
    let view = Value::Uint8Array(Rc::new(Uint8ArrayData::new(buffer.clone(), offset, length)));
    finish_view(view, &buffer, offset)
}

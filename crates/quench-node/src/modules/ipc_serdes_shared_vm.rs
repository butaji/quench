//! The `advanced` child-process IPC codec (lib/internal/child_process/serialization.js).
//! A frame is a 4-byte big-endian payload length, then a V8 value payload in which array
//! buffer views are host objects tagged by type (Node Buffers keep their class) and other
//! host objects travel as shallow copies.

use crate::host::NodeHost;
use quench_runtime::{
    BuiltinBrand, DeserializerDelegate, HostObjectRead, NativeContext, RootId, RootedError, SerializerDelegate,
    SharedWireInput, ValueDeserializer, ValueSerializer, WireInput, WireOutput,
};
use std::cell::RefCell;
use std::rc::Rc;

type Context<'a> = NativeContext<'a, NodeHost>;

/// The frame's payload-length prefix.
pub(crate) const FRAME_HEADER_BYTES: usize = 4;
/// Tags before each host object.
const ARRAY_BUFFER_VIEW_TAG: u32 = 0;
const NOT_ARRAY_BUFFER_VIEW_TAG: u32 = 1;
/// lib/v8.js `arrayBufferViewTypeToIndex`; Node Buffers have their own index.
const VIEW_TYPES: [(BuiltinBrand, u32, usize); 13] = [
    (BuiltinBrand::Int8Array, 0, 1),
    (BuiltinBrand::Uint8Array, 1, 1),
    (BuiltinBrand::Uint8ClampedArray, 2, 1),
    (BuiltinBrand::Int16Array, 3, 2),
    (BuiltinBrand::Uint16Array, 4, 2),
    (BuiltinBrand::Int32Array, 5, 4),
    (BuiltinBrand::Uint32Array, 6, 4),
    (BuiltinBrand::Float32Array, 7, 4),
    (BuiltinBrand::Float64Array, 8, 8),
    (BuiltinBrand::DataView, 9, 1),
    (BuiltinBrand::BigInt64Array, 11, 8),
    (BuiltinBrand::BigUint64Array, 12, 8),
    (BuiltinBrand::Float16Array, 13, 2),
];
const BUFFER_INDEX: u32 = 10;

/// View constructors by type index, captured from the startup intrinsics.
const VIEWS_JS: &str = quench_js_check::checked_js!(
    r#"(FastBuffer) => {
  'use strict';
  const constructors = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array,
    Uint32Array, Float32Array, Float64Array, DataView, FastBuffer, BigInt64Array, BigUint64Array, Float16Array];
  return (index, buffer, offset, length) => new constructors[index](buffer, offset, length);
}"#
);

fn invalid_state(c: &mut Context<'_>, message: &str) -> RootedError {
    let error = c.error_rooted(message).and_then(|error| {
        let key = c.string_rooted("code");
        let code = c.string_rooted("ERR_INVALID_STATE");
        c.define_data_property_rooted(error, key, code, true, true, true)?;
        Ok(error)
    });
    match error {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

fn plain_error(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.error_rooted(message) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

struct IpcSerializer {
    buffer_constructor: RootId,
}

impl SerializerDelegate<NodeHost> for IpcSerializer {
    fn write_host_object(&mut self, c: &mut Context<'_>, output: &WireOutput, object: RootId) -> Result<Option<RootId>, RootedError> {
        let storage = c.buffer_storage_rooted(object)?.filter(|storage| storage.kind != BuiltinBrand::ArrayBuffer && storage.kind != BuiltinBrand::SharedArrayBuffer);
        let Some(storage) = storage else {
            // A shallow copy of the object's own enumerable string-keyed properties.
            quench_runtime::write_uint32(output, NOT_ARRAY_BUFFER_VIEW_TAG);
            let copy = c.object_rooted()?;
            for key in c.enumerable_own_keys_rooted(object, false, true)? {
                let value = c.get_property_rooted(object, key)?;
                c.define_data_property_rooted(copy, key, value, true, true, true)?;
            }
            return Ok(Some(copy));
        };
        quench_runtime::write_uint32(output, ARRAY_BUFFER_VIEW_TAG);
        let key = c.string_rooted("constructor");
        let constructor = c.get_property_rooted(object, key)?;
        let index = if c.same_value_rooted(constructor, self.buffer_constructor)? {
            BUFFER_INDEX
        } else {
            match VIEW_TYPES.iter().find(|(brand, _, _)| *brand == storage.kind) {
                Some((_, index, _)) => *index,
                None => return Err(invalid_state(c, "Unserializable host object")),
            }
        };
        let bytes = c.view_bytes_rooted(object).unwrap_or_default();
        quench_runtime::write_uint32(output, index);
        quench_runtime::write_uint32(output, bytes.len() as u32);
        quench_runtime::write_raw_bytes(output, &bytes);
        Ok(None)
    }

    fn shared_array_buffer_id(&mut self, c: &mut Context<'_>, buffer: RootId) -> Result<u32, RootedError> {
        let message = format!("{} could not be cloned.", c.clone_description_rooted(buffer));
        Err(plain_error(c, &message))
    }

    fn data_clone_error(&mut self, c: &mut Context<'_>, message: &str) -> RootedError {
        plain_error(c, message)
    }
}

struct IpcDeserializer {
    /// The frame's ArrayBuffer and the frame's offset in it.
    buffer: RootId,
    frame_offset: usize,
    views: RootId,
}

impl DeserializerDelegate<NodeHost> for IpcDeserializer {
    fn read_host_object(&mut self, c: &mut Context<'_>, input: &SharedWireInput) -> Result<HostObjectRead<RootId>, RootedError> {
        let tag = input.borrow_mut().read_uint32();
        match tag {
            Some(NOT_ARRAY_BUFFER_VIEW_TAG) => return Ok(HostObjectRead::NextValue),
            Some(ARRAY_BUFFER_VIEW_TAG) => {}
            Some(_) => return Err(invalid_state(c, "Invalid host object tag")),
            None => return Err(plain_error(c, "Unable to deserialize cloned data.")),
        }
        let (index, byte_length) = {
            let mut input = input.borrow_mut();
            (input.read_uint32(), input.read_uint32())
        };
        let (Some(index), Some(byte_length)) = (index, byte_length) else {
            return Err(plain_error(c, "Unable to deserialize cloned data."));
        };
        let element_size = if index == BUFFER_INDEX {
            1
        } else {
            match VIEW_TYPES.iter().find(|(_, candidate, _)| *candidate == index) {
                Some((_, _, size)) => *size,
                None => return Err(invalid_state(c, "Invalid host object type index")),
            }
        };
        let byte_length = byte_length as usize;
        let Some(offset) = input.borrow_mut().read_raw(byte_length) else {
            return Err(plain_error(c, "Unable to deserialize cloned data."));
        };
        // Views over aligned bytes share the frame's buffer; others get a copy.
        let offset_in_buffer = self.frame_offset + offset;
        let (buffer, offset_in_buffer) = if offset_in_buffer % element_size == 0 {
            (self.buffer, offset_in_buffer)
        } else {
            let bytes = c.bytes_prefix_rooted(self.buffer, offset_in_buffer + byte_length).unwrap_or_default();
            let copy = c.uint8_array_rooted(&bytes[offset_in_buffer.min(bytes.len())..])?;
            let key = c.string_rooted("buffer");
            (c.get_property_rooted(copy, key)?, 0)
        };
        let index = c.number(f64::from(index));
        let offset = c.number(offset_in_buffer as f64);
        let length = c.number((byte_length / element_size) as f64);
        let undefined = c.undefined();
        c.call_rooted(self.views, undefined, &[index, buffer, offset, length]).map(HostObjectRead::Object)
    }

    fn shared_array_buffer(&mut self, c: &mut Context<'_>, _: u32) -> Result<RootId, RootedError> {
        Err(plain_error(c, "Unable to deserialize cloned data."))
    }
}

fn views(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:ipc_serdes:views", |c| {
        let fast_buffer = crate::modules::buffer_constructor_shared_vm::fast_buffer(c)?;
        c.with_startup_intrinsics(|c| {
            let factory = c.evaluate_script_rooted(VIEWS_JS, "node:internal/ipc_serdes.js")?;
            let undefined = c.undefined();
            c.call_rooted(factory, undefined, &[fast_buffer])
        })
    })
}

/// One framed message: the length prefix, then the V8 payload.
pub(crate) fn serialize(c: &mut Context<'_>, message: RootId, buffer_constructor: RootId) -> Result<Vec<u8>, RootedError> {
    let output = WireOutput::default();
    quench_runtime::write_raw_bytes(&output, &[0; FRAME_HEADER_BYTES]);
    let mut serializer = ValueSerializer::new(output.clone());
    serializer.set_treat_array_buffer_views_as_host_objects(true);
    serializer.write_header();
    let written = c.serializer_write_value(&mut serializer, message, &mut IpcSerializer { buffer_constructor });
    c.serializer_release(&mut serializer);
    written?;
    let mut bytes = output.take();
    let payload = (bytes.len() - FRAME_HEADER_BYTES) as u32;
    bytes[..FRAME_HEADER_BYTES].copy_from_slice(&payload.to_be_bytes());
    Ok(bytes)
}

/// Read one frame's payload (without its length prefix) from a view.
pub(crate) fn deserialize(c: &mut Context<'_>, view: RootId) -> Result<RootId, RootedError> {
    let Some(storage) = c.buffer_storage_rooted(view)? else {
        return Err(plain_error(c, "Unable to deserialize cloned data."));
    };
    let bytes = c.view_bytes_rooted(view).unwrap_or_default();
    let input: SharedWireInput = Rc::new(RefCell::new(WireInput::new(bytes)));
    let mut deserializer = ValueDeserializer::new(input);
    let views = views(c)?;
    let mut delegate = IpcDeserializer { buffer: storage.buffer, frame_offset: storage.byte_offset, views };
    let result = c
        .deserializer_read_header(&mut deserializer)
        .and_then(|()| c.deserializer_read_value(&mut deserializer, &mut delegate));
    c.deserializer_release(&mut deserializer);
    result
}

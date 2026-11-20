//! The `advanced` child-process IPC codec (lib/internal/child_process/serialization.js).
//! A frame is a 4-byte big-endian payload length, then a V8 value payload in which array
//! buffer views are host objects tagged by type (Node Buffers keep their class) and other
//! host objects travel as shallow copies.

use crate::host::NodeHost;
use quench_runtime::{
    DeserializerDelegate, HostObjectRead, NativeContext, RootId, RootedError, SerializerDelegate,
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

use super::serdes_shared_vm::throw_error as plain_error;

struct IpcSerializer;

impl SerializerDelegate<NodeHost> for IpcSerializer {
    fn write_host_object(&mut self, c: &mut Context<'_>, output: &WireOutput, object: RootId) -> Result<Option<RootId>, RootedError> {
        if super::serdes_shared_vm::view_bytes(c, object)?.is_none() {
            // A shallow copy of the object's own enumerable string-keyed properties.
            quench_runtime::write_uint32(output, NOT_ARRAY_BUFFER_VIEW_TAG);
            let copy = c.object_rooted()?;
            for key in c.enumerable_own_keys_rooted(object, false, true)? {
                let value = c.get_property_rooted(object, key)?;
                c.define_data_property_rooted(copy, key, value, true, true, true)?;
            }
            return Ok(Some(copy));
        }
        quench_runtime::write_uint32(output, ARRAY_BUFFER_VIEW_TAG);
        if !super::serdes_shared_vm::write_view(c, output, object)? {
            return Err(invalid_state(c, "Unserializable host object"));
        }
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
        match super::serdes_shared_vm::read_view(c, input, self.buffer, self.frame_offset)? {
            Some(view) => Ok(HostObjectRead::Object(view)),
            None => Err(invalid_state(c, "Invalid host object type index")),
        }
    }

    fn shared_array_buffer(&mut self, c: &mut Context<'_>, _: u32) -> Result<RootId, RootedError> {
        Err(plain_error(c, "Unable to deserialize cloned data."))
    }
}

/// `internalBinding('ipc_serdes')`, the `advanced` codec of
/// lib/internal/child_process/serialization.js: `serialize(message, Buffer)` frames one
/// message into a Buffer, and `deserialize(view)` reads a frame's payload.
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:ipc_serdes", |c| {
        let module = c.object_rooted()?;
        for (name, operation) in [("serialize", "ipcSerdesSerialize"), ("deserialize", "ipcSerdesDeserialize")] {
            let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(module, key, function, true, true, true)?;
        }
        Ok(module)
    })
}

pub(crate) fn serialize_native(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let undefined = c.undefined();
    let message = args.first().copied().unwrap_or(undefined);
    let bytes = serialize(c, message)?;
    super::buffer_shared_vm::from_vec(c, bytes)
}

pub(crate) fn deserialize_native(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = args.first().copied().unwrap_or_else(|| c.undefined());
    deserialize(c, view)
}

/// One framed message: the length prefix, then the V8 payload.
pub(crate) fn serialize(c: &mut Context<'_>, message: RootId) -> Result<Vec<u8>, RootedError> {
    let output = WireOutput::default();
    quench_runtime::write_raw_bytes(&output, &[0; FRAME_HEADER_BYTES]);
    let mut serializer = ValueSerializer::new(output.clone());
    serializer.set_treat_array_buffer_views_as_host_objects(true);
    serializer.write_header();
    let written = c.serializer_write_value(&mut serializer, message, &mut IpcSerializer);
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
    let mut delegate = IpcDeserializer { buffer: storage.buffer, frame_offset: storage.byte_offset };
    let result = c
        .deserializer_read_header(&mut deserializer)
        .and_then(|()| c.deserializer_read_value(&mut deserializer, &mut delegate));
    c.deserializer_release(&mut deserializer);
    result
}

//! `internalBinding('serdes')` (src/node_serdes.cc): scripted `Serializer` and
//! `Deserializer` classes over the runtime's V8 value serializer. Their delegates are
//! the objects' own `_writeHostObject`, `_getSharedArrayBufferId`, `_getDataCloneError`
//! and `_readHostObject` methods, as Node's `SerializerContext` looks them up per call.

use crate::host::NodeHost;
use quench_runtime::{
    DeserializerDelegate, HostObjectRead, NativeContext, RootId, RootedError, SerializerDelegate, SharedWireInput, ValueDeserializer,
    ValueSerializer, Value, WireInput, WireOutput,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

type Context<'a> = NativeContext<'a, NodeHost>;

const BINDING_JS: &str = quench_js_check::checked_js!(
    r#"(native) => {
  'use strict';
  const handles = new WeakMap();
  const released = new FinalizationRegistry((id) => native.release(id));
  const handle = (object) => {
    const id = handles.get(object);
    if (id === undefined) throw new TypeError('Illegal invocation');
    return id;
  };
  const register = (object, id) => {
    handles.set(object, id);
    released.register(object, id);
  };
  const invalidArgument = (message) => {
    const error = new TypeError(message);
    error.code = 'ERR_INVALID_ARG_TYPE';
    return error;
  };
  class Serializer {
    constructor() { register(this, native.serializer()); }
    writeHeader() { native.writeHeader(handle(this)); }
    writeValue(value) { return native.writeValue(handle(this), this, value); }
    releaseBuffer() { return native.releaseBuffer(handle(this)); }
    transferArrayBuffer(id, arrayBuffer) {
      if (!native.transferArrayBuffer(handle(this), id, arrayBuffer)) throw invalidArgument('arrayBuffer must be an ArrayBuffer');
    }
    writeUint32(value) { native.writeUint32(handle(this), value); }
    writeUint64(hi, lo) { native.writeUint64(handle(this), hi, lo); }
    writeDouble(value) { native.writeDouble(handle(this), value); }
    writeRawBytes(source) {
      if (!ArrayBuffer.isView(source)) throw invalidArgument('source must be a TypedArray or a DataView');
      native.writeRawBytes(handle(this), source);
    }
    _setTreatArrayBufferViewsAsHostObjects(value) { native.treatViewsAsHostObjects(handle(this), !!value); }
  }
  class Deserializer {
    constructor(buffer) {
      if (!ArrayBuffer.isView(buffer)) throw invalidArgument('buffer must be a TypedArray or a DataView');
      register(this, native.deserializer(buffer));
      this.buffer = buffer;
    }
    readHeader() { return native.readHeader(handle(this)); }
    readValue() { return native.readValue(handle(this), this); }
    transferArrayBuffer(id, arrayBuffer) {
      if (!native.receiveArrayBuffer(handle(this), id, arrayBuffer)) {
        throw invalidArgument('arrayBuffer must be an ArrayBuffer or SharedArrayBuffer');
      }
    }
    getWireFormatVersion() { return native.wireFormatVersion(handle(this)); }
    readUint32() { return native.readUint32(handle(this)); }
    readUint64() { return native.readUint64(handle(this)); }
    readDouble() { return native.readDouble(handle(this)); }
    _readRawBytes(length) { return native.readRawBytes(handle(this), length); }
  }
  return { Serializer, Deserializer };
}"#
);

/// One serializer: the runtime state, absent while a `writeValue` runs, and its output.
struct SerializerEntry {
    serializer: Option<ValueSerializer>,
    output: WireOutput,
}

struct DeserializerEntry {
    deserializer: Option<ValueDeserializer>,
    input: SharedWireInput,
}

/// This VM's live scripted serializers and deserializers by handle id.
#[derive(Default)]
pub(crate) struct State {
    next_id: u64,
    serializers: HashMap<u64, SerializerEntry>,
    deserializers: HashMap<u64, DeserializerEntry>,
}

impl State {
    fn allocate(&mut self) -> u64 {
        self.next_id += 1;
        self.next_id
    }

    /// Forget runtime roots when the VM heap is replaced.
    pub(crate) fn reset(&mut self) {
        *self = Self::default();
    }
}

fn state(c: &mut Context<'_>) -> Rc<RefCell<crate::host::SharedNodeState>> {
    c.host_mut().shared_state()
}

fn argument(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    args.get(index).copied().unwrap_or_else(|| c.undefined())
}

fn handle(c: &mut Context<'_>, args: &[RootId]) -> Result<u64, RootedError> {
    let value = argument(c, args, 0);
    c.rooted_value(value)
        .and_then(Value::as_number)
        .map(|id| id as u64)
        .ok_or_else(|| RootedError::host("serdes handle"))
}

/// ECMAScript ToUint32.
fn uint32(c: &mut Context<'_>, value: RootId) -> Result<u32, RootedError> {
    const TWO_TO_THE_32: f64 = 4_294_967_296.0;
    let number = c.to_number(value)?;
    Ok(if number.is_finite() { number.trunc().rem_euclid(TWO_TO_THE_32) as u32 } else { 0 })
}

fn output(c: &mut Context<'_>, args: &[RootId]) -> Result<WireOutput, RootedError> {
    let id = handle(c, args)?;
    let shared = state(c);
    let shared = shared.borrow();
    shared.serdes.serializers.get(&id).map(|entry| entry.output.clone()).ok_or_else(|| RootedError::host("released serializer"))
}

fn input(c: &mut Context<'_>, args: &[RootId]) -> Result<SharedWireInput, RootedError> {
    let id = handle(c, args)?;
    let shared = state(c);
    let shared = shared.borrow();
    shared.serdes.deserializers.get(&id).map(|entry| entry.input.clone()).ok_or_else(|| RootedError::host("released deserializer"))
}

fn throw_error(c: &mut Context<'_>, message: &str) -> RootedError {
    match c.error_rooted(message) {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

fn method(c: &mut Context<'_>, receiver: RootId, name: &str) -> Result<Option<RootId>, RootedError> {
    let key = c.string_rooted(name);
    let value = c.get_property_rooted(receiver, key)?;
    Ok(c.is_callable_rooted(value)?.then_some(value))
}

/// `SerializerContext` as V8's delegate.
struct ScriptedSerializer {
    receiver: RootId,
}

impl SerializerDelegate<NodeHost> for ScriptedSerializer {
    fn write_host_object(&mut self, c: &mut Context<'_>, _: &WireOutput, object: RootId) -> Result<Option<RootId>, RootedError> {
        match method(c, self.receiver, "_writeHostObject")? {
            Some(write) => c.call_rooted(write, self.receiver, &[object]).map(|_| None),
            // V8's default delegate.
            None => {
                let message = format!("{} could not be cloned.", c.clone_description_rooted(object));
                Err(throw_error(c, &message))
            }
        }
    }

    fn shared_array_buffer_id(&mut self, c: &mut Context<'_>, buffer: RootId) -> Result<u32, RootedError> {
        match method(c, self.receiver, "_getSharedArrayBufferId")? {
            Some(get) => {
                let id = c.call_rooted(get, self.receiver, &[buffer])?;
                uint32(c, id)
            }
            None => {
                let message = format!("{} could not be cloned.", c.clone_description_rooted(buffer));
                Err(throw_error(c, &message))
            }
        }
    }

    fn data_clone_error(&mut self, c: &mut Context<'_>, message: &str) -> RootedError {
        let message = c.string_rooted(message);
        let error = method(c, self.receiver, "_getDataCloneError")
            .and_then(|get| get.ok_or_else(|| RootedError::host("_getDataCloneError is not a function")))
            .and_then(|get| c.call_rooted(get, self.receiver, &[message]));
        match error {
            Ok(error) => c.throw(error),
            Err(error) => error,
        }
    }
}

/// `DeserializerContext` as V8's delegate.
struct ScriptedDeserializer {
    receiver: RootId,
}

const DESERIALIZATION_ERROR: &str = "Unable to deserialize cloned data.";

impl DeserializerDelegate<NodeHost> for ScriptedDeserializer {
    fn read_host_object(&mut self, c: &mut Context<'_>, _: &SharedWireInput) -> Result<HostObjectRead<RootId>, RootedError> {
        let Some(read) = method(c, self.receiver, "_readHostObject")? else {
            return Err(throw_error(c, DESERIALIZATION_ERROR));
        };
        let object = c.call_rooted(read, self.receiver, &[])?;
        let is_object = c.rooted_value(object).is_some_and(|value| value.is_heap()) && c.string_text(object)?.is_none();
        if !is_object {
            let error = c.type_error_rooted("readHostObject must return an object")?;
            return Err(c.throw(error));
        }
        Ok(HostObjectRead::Object(object))
    }

    fn shared_array_buffer(&mut self, c: &mut Context<'_>, _: u32) -> Result<RootId, RootedError> {
        Err(throw_error(c, DESERIALIZATION_ERROR))
    }
}

/// `internalBinding('serdes')`.
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let native = c.object_rooted()?;
    for (name, operation) in [
        ("serializer", "serdesSerializer"),
        ("deserializer", "serdesDeserializer"),
        ("release", "serdesRelease"),
        ("writeHeader", "serdesWriteHeader"),
        ("writeValue", "serdesWriteValue"),
        ("releaseBuffer", "serdesReleaseBuffer"),
        ("transferArrayBuffer", "serdesTransferArrayBuffer"),
        ("writeUint32", "serdesWriteUint32"),
        ("writeUint64", "serdesWriteUint64"),
        ("writeDouble", "serdesWriteDouble"),
        ("writeRawBytes", "serdesWriteRawBytes"),
        ("treatViewsAsHostObjects", "serdesTreatViewsAsHostObjects"),
        ("readHeader", "serdesReadHeader"),
        ("readValue", "serdesReadValue"),
        ("receiveArrayBuffer", "serdesReceiveArrayBuffer"),
        ("wireFormatVersion", "serdesWireFormatVersion"),
        ("readUint32", "serdesReadUint32"),
        ("readUint64", "serdesReadUint64"),
        ("readDouble", "serdesReadDouble"),
        ("readRawBytes", "serdesReadRawBytes"),
    ] {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(native, key, function, true, true, true)?;
    }
    let factory = c.evaluate_script_rooted(BINDING_JS, "node:internal/serdes.js")?;
    let undefined = c.undefined();
    c.call_rooted(factory, undefined, &[native])
}

pub(crate) fn serializer(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let shared = state(c);
    let mut shared = shared.borrow_mut();
    let id = shared.serdes.allocate();
    let output = WireOutput::default();
    let serializer = ValueSerializer::new(output.clone());
    shared.serdes.serializers.insert(id, SerializerEntry { serializer: Some(serializer), output });
    Ok(c.number(id as f64))
}

pub(crate) fn deserializer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let view = argument(c, args, 0);
    let bytes = c.view_bytes_rooted(view).unwrap_or_default();
    let input: SharedWireInput = Rc::new(RefCell::new(WireInput::new(bytes)));
    let shared = state(c);
    let mut shared = shared.borrow_mut();
    let id = shared.serdes.allocate();
    let deserializer = ValueDeserializer::new(input.clone());
    shared.serdes.deserializers.insert(id, DeserializerEntry { deserializer: Some(deserializer), input });
    Ok(c.number(id as f64))
}

/// The handle's object was collected: release what its runtime state roots.
pub(crate) fn release(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = handle(c, args)?;
    let (serializer, deserializer) = {
        let shared = state(c);
        let mut shared = shared.borrow_mut();
        (shared.serdes.serializers.remove(&id), shared.serdes.deserializers.remove(&id))
    };
    if let Some(mut serializer) = serializer.and_then(|entry| entry.serializer) {
        c.serializer_release(&mut serializer);
    }
    if let Some(mut deserializer) = deserializer.and_then(|entry| entry.deserializer) {
        c.deserializer_release(&mut deserializer);
    }
    Ok(c.undefined())
}

/// Run `operation` with the handle's serializer taken out of the table, so hooks that
/// re-enter the binding see it busy rather than borrowed.
fn with_serializer<T>(
    c: &mut Context<'_>,
    args: &[RootId],
    operation: impl FnOnce(&mut Context<'_>, &mut ValueSerializer) -> Result<T, RootedError>,
) -> Result<T, RootedError> {
    let id = handle(c, args)?;
    let taken = state(c).borrow_mut().serdes.serializers.get_mut(&id).and_then(|entry| entry.serializer.take());
    let Some(mut serializer) = taken else {
        return Err(throw_error(c, "Serializer is busy or released"));
    };
    let result = operation(c, &mut serializer);
    match state(c).borrow_mut().serdes.serializers.get_mut(&id) {
        Some(entry) => entry.serializer = Some(serializer),
        None => drop(serializer),
    }
    result
}

fn with_deserializer<T>(
    c: &mut Context<'_>,
    args: &[RootId],
    operation: impl FnOnce(&mut Context<'_>, &mut ValueDeserializer) -> Result<T, RootedError>,
) -> Result<T, RootedError> {
    let id = handle(c, args)?;
    let taken = state(c).borrow_mut().serdes.deserializers.get_mut(&id).and_then(|entry| entry.deserializer.take());
    let Some(mut deserializer) = taken else {
        return Err(throw_error(c, "Deserializer is busy or released"));
    };
    let result = operation(c, &mut deserializer);
    match state(c).borrow_mut().serdes.deserializers.get_mut(&id) {
        Some(entry) => entry.deserializer = Some(deserializer),
        None => drop(deserializer),
    }
    result
}

pub(crate) fn write_header(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    with_serializer(c, args, |_, serializer| {
        serializer.write_header();
        Ok(())
    })?;
    Ok(c.undefined())
}

/// `writeValue(value)`: true once written; exceptions propagate.
pub(crate) fn write_value(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let receiver = argument(c, args, 1);
    let value = argument(c, args, 2);
    with_serializer(c, args, |c, serializer| {
        c.serializer_write_value(serializer, value, &mut ScriptedSerializer { receiver })
    })?;
    Ok(c.boolean(true))
}

/// `releaseBuffer()`: the bytes written so far as a Buffer; the serializer starts over.
pub(crate) fn release_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = output(c, args)?;
    let bytes = output.take();
    crate::modules::buffer_shared_vm::from_bytes(c, &bytes)
}

pub(crate) fn transfer_array_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = argument(c, args, 1);
    let id = uint32(c, id)?;
    let buffer = argument(c, args, 2);
    let accepted = with_serializer(c, args, |c, serializer| c.serializer_transfer_array_buffer(serializer, id, buffer))?;
    Ok(c.boolean(accepted))
}

pub(crate) fn write_uint32(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = output(c, args)?;
    let value = argument(c, args, 1);
    let value = uint32(c, value)?;
    quench_runtime::write_uint32(&output, value);
    Ok(c.undefined())
}

pub(crate) fn write_uint64(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = output(c, args)?;
    let (hi, lo) = (argument(c, args, 1), argument(c, args, 2));
    let (hi, lo) = (uint32(c, hi)?, uint32(c, lo)?);
    quench_runtime::write_uint64(&output, (u64::from(hi) << u32::BITS) | u64::from(lo));
    Ok(c.undefined())
}

pub(crate) fn write_double(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = output(c, args)?;
    let value = argument(c, args, 1);
    let value = c.to_number(value)?;
    quench_runtime::write_double(&output, value);
    Ok(c.undefined())
}

pub(crate) fn write_raw_bytes(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = output(c, args)?;
    let view = argument(c, args, 1);
    let bytes = c.view_bytes_rooted(view).unwrap_or_default();
    quench_runtime::write_raw_bytes(&output, &bytes);
    Ok(c.undefined())
}

pub(crate) fn treat_views_as_host_objects(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mode = argument(c, args, 1);
    let mode = c.truthy_rooted(mode)?;
    with_serializer(c, args, |_, serializer| {
        serializer.set_treat_array_buffer_views_as_host_objects(mode);
        Ok(())
    })?;
    Ok(c.undefined())
}

pub(crate) fn read_header(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    with_deserializer(c, args, |c, deserializer| c.deserializer_read_header(deserializer))?;
    Ok(c.boolean(true))
}

pub(crate) fn read_value(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let receiver = argument(c, args, 1);
    with_deserializer(c, args, |c, deserializer| {
        c.deserializer_read_value(deserializer, &mut ScriptedDeserializer { receiver })
    })
}

pub(crate) fn receive_array_buffer(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = argument(c, args, 1);
    let id = uint32(c, id)?;
    let buffer = argument(c, args, 2);
    let accepted = with_deserializer(c, args, |c, deserializer| c.deserializer_transfer_array_buffer(deserializer, id, buffer))?;
    Ok(c.boolean(accepted))
}

pub(crate) fn wire_format_version(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input(c, args)?;
    let version = input.borrow().version();
    Ok(c.number(f64::from(version)))
}

pub(crate) fn read_uint32(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input(c, args)?;
    let value = input.borrow_mut().read_uint32();
    match value {
        Some(value) => Ok(c.number(f64::from(value))),
        None => Err(throw_error(c, "ReadUint32() failed")),
    }
}

/// `readUint64()`: `[hi, lo]`.
pub(crate) fn read_uint64(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input(c, args)?;
    let value = input.borrow_mut().read_uint64();
    let Some(value) = value else {
        return Err(throw_error(c, "ReadUint64() failed"));
    };
    let hi = c.number(f64::from((value >> u32::BITS) as u32));
    let lo = c.number(f64::from(value as u32));
    c.array_rooted(&[hi, lo])
}

pub(crate) fn read_double(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input(c, args)?;
    let value = input.borrow_mut().read_double();
    match value {
        Some(value) => Ok(c.number(value)),
        None => Err(throw_error(c, "ReadDouble() failed")),
    }
}

/// `_readRawBytes(length)`: the bytes' offset in the deserializer's buffer.
pub(crate) fn read_raw_bytes(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = input(c, args)?;
    let length = argument(c, args, 1);
    let length = c.to_number(length)?;
    let offset = if length.is_finite() && length >= 0.0 { input.borrow_mut().read_raw(length as usize) } else { None };
    match offset {
        Some(offset) => Ok(c.number(offset as f64)),
        None => Err(throw_error(c, "ReadRawBytes() failed")),
    }
}

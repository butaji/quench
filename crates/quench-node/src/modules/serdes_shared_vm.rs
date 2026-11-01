//! `internalBinding('serdes')` (src/node_serdes.cc): the `Serializer` and `Deserializer`
//! classes over the runtime's V8 value serializer. Their delegates are the objects' own
//! `_writeHostObject`, `_getSharedArrayBufferId`, `_getDataCloneError` and
//! `_readHostObject` methods, as Node's `SerializerContext` looks them up per call. Each
//! object keeps its handle id in a private slot; a collected object's handle is released
//! after the collection.

use crate::host::NodeHost;
use quench_runtime::{
    BuiltinBrand, DeserializerDelegate, HostObjectRead, HostWeakRoot, NativeContext, RootId, RootedError, SerializerDelegate,
    SharedWireInput, ValueDeserializer, ValueSerializer, Value, WireInput, WireOutput,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

type Context<'a> = NativeContext<'a, NodeHost>;


/// One serializer: the runtime state, absent while a `writeValue` runs, and its output.
struct SerializerEntry {
    identity: HostWeakRoot,
    serializer: Option<ValueSerializer>,
    output: WireOutput,
}

struct DeserializerEntry {
    identity: HostWeakRoot,
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

fn handle(c: &mut Context<'_>, receiver: RootId) -> Result<u64, RootedError> {
    let key = handle_key(c)?;
    let value = if c.is_object_rooted(receiver)? { c.get_property_rooted(receiver, key)? } else { c.undefined() };
    match c.rooted_value(value).and_then(Value::as_number) {
        Some(id) => Ok(id as u64),
        None => {
            let error = c.type_error_rooted("Illegal invocation")?;
            Err(c.throw(error))
        }
    }
}

/// ECMAScript ToUint32.
fn uint32(c: &mut Context<'_>, value: RootId) -> Result<u32, RootedError> {
    const TWO_TO_THE_32: f64 = 4_294_967_296.0;
    let number = c.to_number(value)?;
    Ok(if number.is_finite() { number.trunc().rem_euclid(TWO_TO_THE_32) as u32 } else { 0 })
}

pub(crate) fn serializer_output(c: &mut Context<'_>, receiver: RootId) -> Result<WireOutput, RootedError> {
    let id = handle(c, receiver)?;
    let shared = state(c);
    let shared = shared.borrow();
    shared.serdes.serializers.get(&id).map(|entry| entry.output.clone()).ok_or_else(|| RootedError::host("released serializer"))
}

pub(crate) fn deserializer_input(c: &mut Context<'_>, receiver: RootId) -> Result<SharedWireInput, RootedError> {
    let id = handle(c, receiver)?;
    let shared = state(c);
    let shared = shared.borrow();
    shared.serdes.deserializers.get(&id).map(|entry| entry.input.clone()).ok_or_else(|| RootedError::host("released deserializer"))
}

pub(crate) fn throw_error(c: &mut Context<'_>, message: &str) -> RootedError {
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

pub(crate) const DESERIALIZATION_ERROR: &str = "Unable to deserialize cloned data.";

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

/// A view class lib/v8.js serializes as a host object: its index on the wire, its element
/// size and its global constructor.
pub(crate) struct ViewType {
    brand: BuiltinBrand,
    index: u32,
    element_size: usize,
    constructor: &'static str,
}

/// lib/v8.js `arrayBufferViewTypes`; a Node Buffer travels under `BUFFER_INDEX`.
const VIEW_TYPES: [ViewType; 13] = [
    ViewType { brand: BuiltinBrand::Int8Array, index: 0, element_size: 1, constructor: "Int8Array" },
    ViewType { brand: BuiltinBrand::Uint8Array, index: 1, element_size: 1, constructor: "Uint8Array" },
    ViewType { brand: BuiltinBrand::Uint8ClampedArray, index: 2, element_size: 1, constructor: "Uint8ClampedArray" },
    ViewType { brand: BuiltinBrand::Int16Array, index: 3, element_size: 2, constructor: "Int16Array" },
    ViewType { brand: BuiltinBrand::Uint16Array, index: 4, element_size: 2, constructor: "Uint16Array" },
    ViewType { brand: BuiltinBrand::Int32Array, index: 5, element_size: 4, constructor: "Int32Array" },
    ViewType { brand: BuiltinBrand::Uint32Array, index: 6, element_size: 4, constructor: "Uint32Array" },
    ViewType { brand: BuiltinBrand::Float32Array, index: 7, element_size: 4, constructor: "Float32Array" },
    ViewType { brand: BuiltinBrand::Float64Array, index: 8, element_size: 8, constructor: "Float64Array" },
    ViewType { brand: BuiltinBrand::DataView, index: 9, element_size: 1, constructor: "DataView" },
    ViewType { brand: BuiltinBrand::BigInt64Array, index: 11, element_size: 8, constructor: "BigInt64Array" },
    ViewType { brand: BuiltinBrand::BigUint64Array, index: 12, element_size: 8, constructor: "BigUint64Array" },
    ViewType { brand: BuiltinBrand::Float16Array, index: 13, element_size: 2, constructor: "Float16Array" },
];
pub(crate) const BUFFER_INDEX: u32 = 10;
const BUFFER_ELEMENT_SIZE: usize = 1;

/// Write `view` as lib/v8.js's `_writeHostObject` does: its wire index, byte length and
/// bytes. `false`, writing nothing, when it is no view of a serializable class.
pub(crate) fn write_view(c: &mut Context<'_>, output: &WireOutput, view: RootId) -> Result<bool, RootedError> {
    let Some(storage) = c.buffer_storage_rooted(view)? else {
        return Ok(false);
    };
    let key = c.string_rooted("constructor");
    let constructor = c.get_property_rooted(view, key)?;
    let buffer = crate::modules::buffer_shared_vm::fast_buffer(c)?;
    let index = if c.same_value_rooted(constructor, buffer)? {
        BUFFER_INDEX
    } else {
        match VIEW_TYPES.iter().find(|view_type| view_type.brand == storage.kind) {
            Some(view_type) => view_type.index,
            None => return Ok(false),
        }
    };
    let bytes = c.view_bytes_rooted(view).unwrap_or_default();
    quench_runtime::write_uint32(output, index);
    quench_runtime::write_uint32(output, bytes.len() as u32);
    quench_runtime::write_raw_bytes(output, &bytes);
    Ok(true)
}

/// Read a view `write_view` wrote, as lib/v8.js's `_readHostObject` does. The input's
/// bytes start at `base` in `buffer`: an aligned view shares them, a misaligned one gets
/// a copy. `None` for an unknown class index.
pub(crate) fn read_view(c: &mut Context<'_>, input: &SharedWireInput, buffer: RootId, base: usize) -> Result<Option<RootId>, RootedError> {
    let (index, byte_length) = {
        let mut input = input.borrow_mut();
        (input.read_uint32(), input.read_uint32())
    };
    let (Some(index), Some(byte_length)) = (index, byte_length) else {
        return Err(throw_error(c, DESERIALIZATION_ERROR));
    };
    let (element_size, constructor) = if index == BUFFER_INDEX {
        (BUFFER_ELEMENT_SIZE, crate::modules::buffer_shared_vm::fast_buffer(c)?)
    } else {
        let Some(view_type) = VIEW_TYPES.iter().find(|view_type| view_type.index == index) else {
            return Ok(None);
        };
        let constructor = c.with_startup_intrinsics(|c| {
            let global = c.global_root()?;
            c.get_named_property_rooted(global, view_type.constructor)
        })?;
        (view_type.element_size, constructor)
    };
    let byte_length = byte_length as usize;
    let Some(offset) = input.borrow_mut().read_raw(byte_length) else {
        return Err(throw_error(c, DESERIALIZATION_ERROR));
    };
    let start = base + offset;
    let (buffer, start) = if start.is_multiple_of(element_size) {
        (buffer, start)
    } else {
        let bytes = c.bytes_prefix_rooted(buffer, start + byte_length).unwrap_or_default();
        let copy = c.uint8_array_rooted(&bytes[start.min(bytes.len())..])?;
        (c.get_named_property_rooted(copy, "buffer")?, 0)
    };
    let start = c.number(start as f64);
    let length = c.number((byte_length / element_size) as f64);
    c.construct_rooted(constructor, constructor, &[buffer, start, length]).map(Some)
}

/// The prototype methods of `Serializer` and `Deserializer`, in Node's order: `(name,
/// operation)`.
const SERIALIZER_METHODS: [(&str, &str); 9] = [
    ("writeHeader", "serdesWriteHeader"),
    ("writeValue", "serdesWriteValue"),
    ("releaseBuffer", "serdesReleaseBuffer"),
    ("transferArrayBuffer", "serdesTransferArrayBuffer"),
    ("writeUint32", "serdesWriteUint32"),
    ("writeUint64", "serdesWriteUint64"),
    ("writeDouble", "serdesWriteDouble"),
    ("writeRawBytes", "serdesWriteRawBytes"),
    ("_setTreatArrayBufferViewsAsHostObjects", "serdesTreatViewsAsHostObjects"),
];
const DESERIALIZER_METHODS: [(&str, &str); 8] = [
    ("readHeader", "serdesReadHeader"),
    ("readValue", "serdesReadValue"),
    ("getWireFormatVersion", "serdesWireFormatVersion"),
    ("transferArrayBuffer", "serdesReceiveArrayBuffer"),
    ("readUint32", "serdesReadUint32"),
    ("readUint64", "serdesReadUint64"),
    ("readDouble", "serdesReadDouble"),
    ("_readRawBytes", "serdesReadRawBytes"),
];

/// A class whose prototype holds `methods` and then, as V8's templates order it,
/// `constructor`.
fn class(c: &mut Context<'_>, name: &str, constructor: &str, methods: &[(&str, &str)]) -> Result<RootId, RootedError> {
    let class = c.host_constructor_named(crate::host::shared_vm::operation(constructor), name)?;
    let key = c.string_rooted("prototype");
    let prototype = c.get_property_rooted(class, key)?;
    let key = c.string_rooted("constructor");
    c.delete_property_rooted(prototype, key)?;
    for (method, operation) in methods {
        let function = c.host_function_named(crate::host::shared_vm::operation(operation), method)?;
        let key = c.string_rooted(method);
        c.define_data_property_rooted(prototype, key, function, true, true, true)?;
    }
    let key = c.string_rooted("constructor");
    c.define_data_property_rooted(prototype, key, class, true, false, true)?;
    Ok(class)
}

/// `internalBinding('serdes')`: `{ Serializer, Deserializer }`.
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:serdes", |c| {
        let binding = c.object_rooted()?;
        let serializer = class(c, "Serializer", "serdesSerializer", &SERIALIZER_METHODS)?;
        c.assign_named_property_rooted(binding, "Serializer", serializer)?;
        let deserializer = class(c, "Deserializer", "serdesDeserializer", &DESERIALIZER_METHODS)?;
        c.assign_named_property_rooted(binding, "Deserializer", deserializer)?;
        Ok(binding)
    })
}

/// The private symbol a serializer or deserializer keeps its handle id under.
fn handle_key(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:serdes-handle", |c| {
        Ok(c.private_symbol_rooted(Some("serdes handle")))
    })
}

fn construct_call_required(c: &mut Context<'_>, name: &str) -> Result<(), RootedError> {
    if c.new_target_rooted().is_some() {
        return Ok(());
    }
    let error = c.type_error_rooted(&format!("Class constructor {name} cannot be invoked without 'new'"))?;
    super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_CONSTRUCT_CALL_REQUIRED")?;
    Err(c.throw(error))
}

fn bind_handle(c: &mut Context<'_>, receiver: RootId) -> Result<(u64, HostWeakRoot), RootedError> {
    let id = state(c).borrow_mut().serdes.allocate();
    let key = handle_key(c)?;
    let value = c.number(id as f64);
    c.define_data_property_rooted(receiver, key, value, false, false, false)?;
    Ok((id, c.host_weak_root(receiver)?))
}

/// `new Serializer()`.
pub(crate) fn serializer(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    construct_call_required(c, "Serializer")?;
    let (id, identity) = bind_handle(c, receiver)?;
    let output = WireOutput::default();
    let serializer = ValueSerializer::new(output.clone());
    state(c).borrow_mut().serdes.serializers.insert(id, SerializerEntry { identity, serializer: Some(serializer), output });
    Ok(receiver)
}

/// `new Deserializer(buffer)`: reads `buffer`, which it keeps as its `buffer` property.
pub(crate) fn deserializer(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    construct_call_required(c, "Deserializer")?;
    let view = argument(c, args, 0);
    let Some(bytes) = view_bytes(c, view)? else {
        return Err(invalid_argument(c, "buffer must be a TypedArray or a DataView"));
    };
    c.assign_named_property_rooted(receiver, "buffer", view)?;
    let (id, identity) = bind_handle(c, receiver)?;
    let input: SharedWireInput = Rc::new(RefCell::new(WireInput::new(bytes)));
    let deserializer = ValueDeserializer::new(input.clone());
    state(c).borrow_mut().serdes.deserializers.insert(id, DeserializerEntry { identity, deserializer: Some(deserializer), input });
    Ok(receiver)
}

/// The bytes of an ArrayBuffer view; none for any other value.
pub(crate) fn view_bytes(c: &mut Context<'_>, value: RootId) -> Result<Option<Vec<u8>>, RootedError> {
    let storage = c.buffer_storage_rooted(value)?;
    let is_view = storage.is_some_and(|storage| storage.kind != BuiltinBrand::ArrayBuffer && storage.kind != BuiltinBrand::SharedArrayBuffer);
    Ok(if is_view { c.view_bytes_rooted(value) } else { None })
}

fn invalid_argument(c: &mut Context<'_>, message: &str) -> RootedError {
    let error = c.type_error_rooted(message).and_then(|error| {
        super::node_errors_shared_vm::decorate_type_error(c, error, "ERR_INVALID_ARG_TYPE")?;
        Ok(error)
    });
    match error {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

/// Release what the handles of collected serializers and deserializers root.
pub(crate) fn prune_collected(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let shared = state(c);
    let (serializers, deserializers) = {
        let state = shared.borrow();
        let dead = |identity: HostWeakRoot| !c.host_weak_root_is_live(identity);
        let serializers: Vec<u64> = state.serdes.serializers.iter().filter(|(_, entry)| dead(entry.identity)).map(|(id, _)| *id).collect();
        let deserializers: Vec<u64> = state.serdes.deserializers.iter().filter(|(_, entry)| dead(entry.identity)).map(|(id, _)| *id).collect();
        (serializers, deserializers)
    };
    for id in serializers {
        let entry = shared.borrow_mut().serdes.serializers.remove(&id);
        if let Some(mut entry) = entry {
            c.release_host_weak_root(entry.identity);
            if let Some(serializer) = entry.serializer.as_mut() {
                c.serializer_release(serializer);
            }
        }
    }
    for id in deserializers {
        let entry = shared.borrow_mut().serdes.deserializers.remove(&id);
        if let Some(mut entry) = entry {
            c.release_host_weak_root(entry.identity);
            if let Some(deserializer) = entry.deserializer.as_mut() {
                c.deserializer_release(deserializer);
            }
        }
    }
    Ok(c.undefined())
}

/// After a collection: release what collected handles root, through `serdesPrune`.
pub(crate) fn prune(runtime: &mut quench_runtime::Runtime<NodeHost>) -> Result<(), RootedError> {
    let shared = runtime.host_mut().shared_state();
    if shared.borrow().serdes.is_empty() {
        return Ok(());
    }
    let prune = runtime.host_function(crate::host::shared_vm::operation("serdesPrune"))?;
    let result = runtime.call_rooted(prune, prune, &[]);
    runtime.release_root(prune);
    runtime.release_root(result?);
    Ok(())
}

/// Run `operation` with the handle's serializer taken out of the table, so hooks that
/// re-enter the binding see it busy rather than borrowed.
fn with_serializer<T>(
    c: &mut Context<'_>,
    receiver: RootId,
    operation: impl FnOnce(&mut Context<'_>, &mut ValueSerializer) -> Result<T, RootedError>,
) -> Result<T, RootedError> {
    let id = handle(c, receiver)?;
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
    receiver: RootId,
    operation: impl FnOnce(&mut Context<'_>, &mut ValueDeserializer) -> Result<T, RootedError>,
) -> Result<T, RootedError> {
    let id = handle(c, receiver)?;
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

pub(crate) fn write_header(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    with_serializer(c, receiver, |_, serializer| {
        serializer.write_header();
        Ok(())
    })?;
    Ok(c.undefined())
}

/// `writeValue(value)`: true once written; exceptions propagate.
pub(crate) fn write_value(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = argument(c, args, 0);
    with_serializer(c, receiver, |c, serializer| {
        c.serializer_write_value(serializer, value, &mut ScriptedSerializer { receiver })
    })?;
    Ok(c.boolean(true))
}

/// `releaseBuffer()`: the bytes written so far as a Buffer; the serializer starts over.
pub(crate) fn release_buffer(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let output = serializer_output(c, receiver)?;
    crate::modules::buffer_shared_vm::from_vec(c, output.take())
}

pub(crate) fn transfer_array_buffer(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = argument(c, args, 0);
    let id = uint32(c, id)?;
    let buffer = argument(c, args, 1);
    let accepted = with_serializer(c, receiver, |c, serializer| c.serializer_transfer_array_buffer(serializer, id, buffer))?;
    if !accepted {
        return Err(invalid_argument(c, "arrayBuffer must be an ArrayBuffer"));
    }
    Ok(c.undefined())
}

pub(crate) fn write_uint32(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = serializer_output(c, receiver)?;
    let value = argument(c, args, 0);
    let value = uint32(c, value)?;
    quench_runtime::write_uint32(&output, value);
    Ok(c.undefined())
}

pub(crate) fn write_uint64(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = serializer_output(c, receiver)?;
    let (hi, lo) = (argument(c, args, 0), argument(c, args, 1));
    let (hi, lo) = (uint32(c, hi)?, uint32(c, lo)?);
    quench_runtime::write_uint64(&output, (u64::from(hi) << u32::BITS) | u64::from(lo));
    Ok(c.undefined())
}

pub(crate) fn write_double(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = serializer_output(c, receiver)?;
    let value = argument(c, args, 0);
    let value = c.to_number(value)?;
    quench_runtime::write_double(&output, value);
    Ok(c.undefined())
}

pub(crate) fn write_raw_bytes(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let output = serializer_output(c, receiver)?;
    let view = argument(c, args, 0);
    let Some(bytes) = view_bytes(c, view)? else {
        return Err(invalid_argument(c, "source must be a TypedArray or a DataView"));
    };
    quench_runtime::write_raw_bytes(&output, &bytes);
    Ok(c.undefined())
}

pub(crate) fn treat_views_as_host_objects(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let mode = argument(c, args, 0);
    let mode = c.truthy_rooted(mode)?;
    with_serializer(c, receiver, |_, serializer| {
        serializer.set_treat_array_buffer_views_as_host_objects(mode);
        Ok(())
    })?;
    Ok(c.undefined())
}

pub(crate) fn read_header(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    with_deserializer(c, receiver, |c, deserializer| c.deserializer_read_header(deserializer))?;
    Ok(c.boolean(true))
}

pub(crate) fn read_value(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    with_deserializer(c, receiver, |c, deserializer| {
        c.deserializer_read_value(deserializer, &mut ScriptedDeserializer { receiver })
    })
}

pub(crate) fn receive_array_buffer(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = argument(c, args, 0);
    let id = uint32(c, id)?;
    let buffer = argument(c, args, 1);
    let accepted = with_deserializer(c, receiver, |c, deserializer| c.deserializer_transfer_array_buffer(deserializer, id, buffer))?;
    if !accepted {
        return Err(invalid_argument(c, "arrayBuffer must be an ArrayBuffer or SharedArrayBuffer"));
    }
    Ok(c.undefined())
}

pub(crate) fn wire_format_version(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let input = deserializer_input(c, receiver)?;
    let version = input.borrow().version();
    Ok(c.number(f64::from(version)))
}

pub(crate) fn read_uint32(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let input = deserializer_input(c, receiver)?;
    let value = input.borrow_mut().read_uint32();
    match value {
        Some(value) => Ok(c.number(f64::from(value))),
        None => Err(throw_error(c, "ReadUint32() failed")),
    }
}

/// `readUint64()`: `[hi, lo]`.
pub(crate) fn read_uint64(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let input = deserializer_input(c, receiver)?;
    let value = input.borrow_mut().read_uint64();
    let Some(value) = value else {
        return Err(throw_error(c, "ReadUint64() failed"));
    };
    let hi = c.number(f64::from((value >> u32::BITS) as u32));
    let lo = c.number(f64::from(value as u32));
    c.array_rooted(&[hi, lo])
}

pub(crate) fn read_double(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let input = deserializer_input(c, receiver)?;
    let value = input.borrow_mut().read_double();
    match value {
        Some(value) => Ok(c.number(value)),
        None => Err(throw_error(c, "ReadDouble() failed")),
    }
}

/// `_readRawBytes(length)`: the bytes' offset in the deserializer's buffer.
pub(crate) fn read_raw_bytes(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let input = deserializer_input(c, receiver)?;
    let length = argument(c, args, 0);
    let length = c.to_number(length)?;
    let offset = if length.is_finite() && length >= 0.0 { input.borrow_mut().read_raw(length as usize) } else { None };
    match offset {
        Some(offset) => Ok(c.number(offset as f64)),
        None => Err(throw_error(c, "ReadRawBytes() failed")),
    }
}

impl State {
    fn is_empty(&self) -> bool {
        self.serializers.is_empty() && self.deserializers.is_empty()
    }

    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        use crate::host::startup_snapshot::{require_quiescent, HostSnap};
        require_quiescent(self.is_empty(), "a v8 serializer")?;
        self.next_id.write(writer)
    }
    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        self.next_id = crate::host::startup_snapshot::HostSnap::read(reader)?;
        Ok(())
    }
}

//! Structured serialization (HTML StructuredSerialize/Deserialize, as Node's V8 serializer
//! applies it). A value serializes into a `SerializedValue` that holds no heap references,
//! so it can cross to another agent's heap; `structuredClone` deserializes it into the same
//! heap. Guest code runs only while serializing (getters, coercions); deserializing only
//! allocates.

use super::object_descriptors::PropertyDescriptorRecord;
use super::*;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;

/// A serialized value: its root and every object it reaches, numbered in first-reach
/// order so repeated and cyclic references resolve to one deserialized object.
pub struct SerializedValue {
    root: Record,
    objects: Vec<ObjectRecord>,
    /// The contents of transferred ArrayBuffers, moved out when serialization finished.
    transferred: Vec<TransferredBuffer>,
}

impl std::fmt::Debug for SerializedValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SerializedValue")
            .field("objects", &self.objects.len())
            .field("transferred", &self.transferred.len())
            .finish()
    }
}

#[derive(Clone)]
enum Record {
    Undefined,
    Null,
    Boolean(bool),
    Number(f64),
    BigInt(String),
    String(Vec<u16>),
    Object(usize),
}

enum ObjectRecord {
    /// Reached, its contents not yet serialized (or already deserialized).
    Pending,
    Ordinary(Vec<(Vec<u16>, Record)>),
    Array {
        length: usize,
        properties: Vec<(Vec<u16>, Record)>,
    },
    Boxed(Record),
    Date(f64),
    RegExp {
        source: Vec<u16>,
        flags: String,
    },
    Map(Vec<(Record, Record)>),
    Set(Vec<Record>),
    Error {
        native: Native,
        message: Option<Vec<u16>>,
        stack: Option<Vec<u16>>,
        cause: Option<Record>,
    },
    ArrayBuffer {
        bytes: Vec<u8>,
        max_byte_length: usize,
        resizable: bool,
        immutable: bool,
    },
    Transferred(usize),
    Shared {
        block: Arc<crate::heap::ConcurrentBytes>,
        max_byte_length: usize,
        resizable: bool,
    },
    View(ViewRecord),
    /// A host object (such as a MessagePort) the host recreates on the receiving side.
    Host(usize),
}

struct ViewRecord {
    /// `None` for a DataView.
    kind: Option<TypedArrayKind>,
    buffer: usize,
    offset: usize,
    length: usize,
    length_tracking: bool,
    /// The bytes the view required when serialized, checked against its new buffer.
    byte_length: usize,
    backed_by_resizable: bool,
}

#[derive(Default)]
struct TransferredBuffer {
    bytes: Vec<u8>,
    max_byte_length: usize,
    resizable: bool,
}

/// Serialization state: object numbering, and roots keeping each source alive while
/// getters run guest code.
#[derive(Default)]
struct Serializer {
    objects: Vec<ObjectRecord>,
    ids: HashMap<Value, usize>,
    roots: Vec<RootId>,
}

impl Serializer {
    fn reserve<H: Host>(&mut self, vm: &mut Vm<H>, source: Value, record: ObjectRecord) -> usize {
        self.roots.push(vm.heap.root(source));
        let id = self.objects.len();
        self.objects.push(record);
        self.ids.insert(source, id);
        id
    }

    fn root<H: Host>(&mut self, vm: &mut Vm<H>, value: Value) -> RootId {
        let root = vm.heap.root(value);
        self.roots.push(root);
        root
    }

    fn release<H: Host>(&mut self, vm: &mut Vm<H>) {
        for root in self.roots.drain(..) {
            vm.heap.release_root(root);
        }
    }
}

/// A deserialized view awaiting the check that its new buffer can hold it.
struct DeserializedView {
    view: RootId,
    byte_length: usize,
    backed_by_resizable: bool,
}

#[derive(Default)]
struct Deserializer {
    created: Vec<Option<RootId>>,
    roots: Vec<RootId>,
    views: Vec<DeserializedView>,
}

impl Deserializer {
    fn root<H: Host>(&mut self, vm: &mut Vm<H>, value: Value) -> RootId {
        let root = vm.heap.root(value);
        self.roots.push(root);
        root
    }

    fn release<H: Host>(&mut self, vm: &mut Vm<H>) {
        for root in self.roots.drain(..) {
            vm.heap.release_root(root);
        }
    }
}

impl<H: Host> Vm<H> {
    pub(crate) fn embedding_mark_untransferable(&mut self, source: RootId) -> Result<(), JsError> {
        let source = self.embedding_value(source)?;
        if let Some(object) = self.object_data_mut(source) {
            object.forbid_structured_transfer();
        }
        Ok(())
    }

    pub(crate) fn embedding_is_marked_untransferable(
        &self,
        source: RootId,
    ) -> Result<bool, JsError> {
        let source = self.embedding_value(source)?;
        Ok(self
            .object_data(source)
            .is_some_and(Object::is_structured_transfer_forbidden))
    }

    pub(crate) fn embedding_mark_uncloneable(&mut self, source: RootId) -> Result<(), JsError> {
        let source = self.embedding_value(source)?;
        // The host marker applies to ordinary objects. Built-in serialized
        // types (arrays, buffers, maps) retain their own clone protocol.
        if self.heap.object_record(source).is_some() {
            self.heap
                .object_mut(source)
                .expect("ordinary object remains live")
                .forbid_structured_clone();
        }
        Ok(())
    }

    pub(crate) fn embedding_structured_clone(
        &mut self,
        source: RootId,
        options: Option<RootId>,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let mut serializer = Serializer::default();
        let serialized = self
            .structured_clone_transfers(&program, options, &mut serializer)
            .and_then(|transfers| self.serialize_value(&program, source, &transfers, &[], &mut serializer));
        serializer.release(self);
        self.deserialize_value(&program, serialized?, &[])
    }

    pub(crate) fn embedding_structured_clone_with_transfers(
        &mut self,
        source: RootId,
        transfers: &[RootId],
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let serialized = self.embedding_serialize(source, transfers, &[])?;
        self.deserialize_value(&program, serialized, &[])
    }

    /// Serialize `source`, transferring `transfers` (ArrayBuffers) and recording each of
    /// `host_objects` as a reference the receiving host recreates.
    pub(crate) fn embedding_serialize(
        &mut self,
        source: RootId,
        transfers: &[RootId],
        host_objects: &[RootId],
    ) -> Result<SerializedValue, JsError> {
        let program = self.embedding_program()?;
        let mut serializer = Serializer::default();
        let serialized = self.serialize_value(&program, source, transfers, host_objects, &mut serializer);
        serializer.release(self);
        serialized
    }

    /// Deserialize into this heap; `host_objects` replace the sender's host objects in order.
    pub(crate) fn embedding_deserialize(
        &mut self,
        serialized: SerializedValue,
        host_objects: &[RootId],
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let host_objects = host_objects
            .iter()
            .map(|root| self.embedding_value(*root))
            .collect::<Result<Vec<_>, _>>()?;
        self.deserialize_value(&program, serialized, &host_objects)
    }

    fn serialize_value(
        &mut self,
        program: &ResidualProgram,
        source: RootId,
        transfers: &[RootId],
        host_objects: &[RootId],
        serializer: &mut Serializer,
    ) -> Result<SerializedValue, JsError> {
        self.validate_transfer_roots(program, transfers)?;
        for (index, transfer) in transfers.iter().enumerate() {
            let buffer = self.heap.root_value(*transfer).unwrap();
            serializer.reserve(self, buffer, ObjectRecord::Transferred(index));
        }
        for (index, host) in host_objects.iter().enumerate() {
            let object = self.embedding_value(*host)?;
            serializer.reserve(self, object, ObjectRecord::Host(index));
        }
        let source = self.embedding_value(source)?;
        let root = self.serialize_record(program, source, serializer)?;
        // Node serializes first, then transfers the current backing stores.
        // Getters may resize/detach a buffer, or install a detach key meanwhile.
        let mut transferred = Vec::with_capacity(transfers.len());
        for transfer in transfers {
            let buffer = self.heap.root_value(*transfer).unwrap();
            let (bytes, max_byte_length, resizable) = match self.heap.get(buffer) {
                Some(Cell::ArrayBuffer { detached: true, .. }) => (Default::default(), 0, false),
                Some(Cell::ArrayBuffer {
                    bytes,
                    max_byte_length,
                    resizable,
                    ..
                }) => (bytes.clone(), *max_byte_length, *resizable),
                _ => unreachable!("validated transfer remains an ArrayBuffer"),
            };
            // Detaching drops the source's handle, so the bytes move without a copy.
            self.detach_array_buffer_native(program, &[buffer])?;
            let bytes: crate::heap::BufferBytes = bytes;
            transferred.push(TransferredBuffer { bytes: bytes.into_vec(), max_byte_length, resizable });
        }
        Ok(SerializedValue {
            root,
            objects: std::mem::take(&mut serializer.objects),
            transferred,
        })
    }

    fn deserialize_value(
        &mut self,
        program: &ResidualProgram,
        mut serialized: SerializedValue,
        host_objects: &[Value],
    ) -> Result<Value, JsError> {
        let mut deserializer = Deserializer {
            created: vec![None; serialized.objects.len()],
            ..Default::default()
        };
        let root = serialized.root.clone();
        let result = self
            .deserialize_record(program, &root, &mut serialized, host_objects, &mut deserializer)
            .and_then(|value| {
                let value = deserializer.root(self, value);
                self.validate_cloned_views(program, &deserializer.views)?;
                Ok(self.heap.root_value(value).unwrap())
            });
        deserializer.release(self);
        result
    }

    fn serialize_record(
        &mut self,
        program: &ResidualProgram,
        source: Value,
        serializer: &mut Serializer,
    ) -> Result<Record, JsError> {
        if let Some(id) = serializer.ids.get(&source) {
            return Ok(Record::Object(*id));
        }
        if source.is_undefined() {
            return Ok(Record::Undefined);
        }
        if source.is_null() {
            return Ok(Record::Null);
        }
        if let Some(value) = source.as_bool() {
            return Ok(Record::Boolean(value));
        }
        if let Some(value) = source.as_number() {
            return Ok(Record::Number(value));
        }
        if self
            .object_data(source)
            .is_some_and(Object::is_arguments_object)
        {
            return Err(self.data_clone_error(program, "#<Object> could not be cloned."));
        }
        if let Some(object) = self.heap.object_record(source) {
            if object.is_structured_clone_forbidden() {
                return Err(
                    self.data_clone_error(program, "Cannot clone object of unsupported type.")
                );
            }
            if self.realm.promise.records.contains_key(&source) {
                return Err(self.data_clone_error(program, "#<Promise> could not be cloned."));
            }
            if object.has_error_data() {
                return self.serialize_error(program, source, serializer);
            }
            if object.is_module_namespace() {
                return Err(self.data_clone_error(program, "Object cannot be cloned"));
            }
            if let Some((brand, value)) = self.boxed_primitive_data(source) {
                if brand == crate::BuiltinBrand::SymbolObject {
                    return Err(
                        self.data_clone_error(program, "[object Symbol] could not be cloned.")
                    );
                }
                let payload = self.serialize_record(program, value, serializer)?;
                let id = serializer.reserve(self, source, ObjectRecord::Boxed(payload));
                return Ok(Record::Object(id));
            }
            let id = serializer.reserve(self, source, ObjectRecord::Pending);
            let properties = self.serialize_properties(program, source, serializer)?;
            serializer.objects[id] = ObjectRecord::Ordinary(properties);
            return Ok(Record::Object(id));
        }
        if self.heap.array(source).is_some() {
            let length = self.own_array_length(source).unwrap_or_default();
            let id = serializer.reserve(self, source, ObjectRecord::Pending);
            let properties = self.serialize_properties(program, source, serializer)?;
            serializer.objects[id] = ObjectRecord::Array { length, properties };
            return Ok(Record::Object(id));
        }
        let Some(cell) = self.heap.get(source).cloned() else {
            return Ok(Record::Undefined);
        };
        let record = match cell {
            Cell::TypedArray {
                kind,
                object,
                buffer,
                length,
                length_tracking,
            } => {
                serializer.root(self, source);
                let detached = self.array_buffer_detached(buffer);
                if detached && !serializer.ids.contains_key(&buffer) {
                    return Err(self.data_clone_error(
                        program,
                        "An ArrayBuffer is detached and could not be cloned.",
                    ));
                }
                if !detached && self.typed_array_out_of_bounds(source) {
                    return Err(
                        self.data_clone_error(program, "[object Object] could not be cloned.")
                    );
                }
                // Length-tracking views serialize an auto-length, not the
                // source buffer's current derived length.
                let byte_length = if length_tracking {
                    0
                } else {
                    length * kind.width()
                };
                let backed_by_resizable = self.array_buffer_resizable(buffer);
                let Record::Object(buffer) = self.serialize_record(program, buffer, serializer)?
                else {
                    unreachable!("a view's buffer serializes as an object");
                };
                ObjectRecord::View(ViewRecord {
                    kind: Some(kind),
                    buffer,
                    offset: object.offset,
                    length,
                    length_tracking,
                    byte_length,
                    backed_by_resizable,
                })
            }
            Cell::DataView {
                object,
                buffer,
                length,
                length_tracking,
            } => {
                serializer.root(self, source);
                let detached = self.array_buffer_detached(buffer);
                if detached && !serializer.ids.contains_key(&buffer) {
                    return Err(self.data_clone_error(
                        program,
                        "An ArrayBuffer is detached and could not be cloned.",
                    ));
                }
                let required = if length_tracking { 0 } else { length };
                let backed_by_resizable = self.array_buffer_resizable(buffer);
                if (!detached || backed_by_resizable)
                    && self.array_buffer_out_of_bounds(buffer, object.offset, required)
                {
                    return Err(self.data_clone_error(program, "#<DataView> could not be cloned."));
                }
                let Record::Object(buffer) = self.serialize_record(program, buffer, serializer)?
                else {
                    unreachable!("a view's buffer serializes as an object");
                };
                ObjectRecord::View(ViewRecord {
                    kind: None,
                    buffer,
                    offset: object.offset,
                    length,
                    length_tracking,
                    byte_length: required,
                    backed_by_resizable,
                })
            }
            Cell::Map { entries, .. } => {
                // Snapshot and root every entry before serializing can invoke a
                // getter that deletes or replaces entries in the source.
                let entries: Vec<_> = entries
                    .iter()
                    .map(|(key, value)| [serializer.root(self, key), serializer.root(self, value)])
                    .collect();
                let id = serializer.reserve(self, source, ObjectRecord::Pending);
                let mut records = Vec::with_capacity(entries.len());
                for [key, value] in entries {
                    let key = self.heap.root_value(key).unwrap();
                    let key = self.serialize_record(program, key, serializer)?;
                    let value = self.heap.root_value(value).unwrap();
                    let value = self.serialize_record(program, value, serializer)?;
                    records.push((key, value));
                }
                serializer.objects[id] = ObjectRecord::Map(records);
                return Ok(Record::Object(id));
            }
            Cell::Set { entries, .. } => {
                let entries: Vec<_> = entries
                    .iter()
                    .map(|value| serializer.root(self, value))
                    .collect();
                let id = serializer.reserve(self, source, ObjectRecord::Pending);
                let mut records = Vec::with_capacity(entries.len());
                for value in entries {
                    let value = self.heap.root_value(value).unwrap();
                    records.push(self.serialize_record(program, value, serializer)?);
                }
                serializer.objects[id] = ObjectRecord::Set(records);
                return Ok(Record::Object(id));
            }
            Cell::Date { milliseconds, .. } => ObjectRecord::Date(milliseconds),
            Cell::RegExp { meta, .. } => ObjectRecord::RegExp {
                source: meta.source.units().to_vec(),
                flags: meta.flags.clone(),
            },
            Cell::String(value) => return Ok(Record::String(value.units().to_vec())),
            Cell::BigInt(value) => return Ok(Record::BigInt(value)),
            Cell::Symbol(_) => return Err(self.uncloneable(program, source)),
            Cell::ArrayBuffer {
                bytes,
                shared,
                detached,
                max_byte_length,
                resizable,
                immutable,
                ..
            } if !shared && !detached => {
                let mut copied_bytes = Vec::new();
                copied_bytes.try_reserve_exact(bytes.len()).map_err(|_| {
                    self.range_error(program, "ArrayBuffer is too large to clone".into())
                })?;
                copied_bytes.extend_from_slice(&bytes);
                ObjectRecord::ArrayBuffer {
                    max_byte_length: if resizable { max_byte_length } else { bytes.len() },
                    bytes: copied_bytes,
                    resizable,
                    immutable,
                }
            }
            Cell::ArrayBuffer { detached: true, .. } => {
                return Err(self.data_clone_error(
                    program,
                    "An ArrayBuffer is detached and could not be cloned.",
                ));
            }
            // A data block other agents may share is shared again, not copied.
            Cell::ArrayBuffer {
                bytes,
                shared: true,
                max_byte_length,
                resizable,
                ..
            } if bytes.concurrent_block().is_some() => ObjectRecord::Shared {
                block: bytes.concurrent_block().unwrap().clone(),
                max_byte_length,
                resizable,
            },
            Cell::ArrayBuffer { .. } => {
                return Err(self
                    .data_clone_error(program, "Detached or shared ArrayBuffer cannot be cloned"));
            }
            _ => return Err(self.uncloneable(program, source)),
        };
        let id = serializer.reserve(self, source, record);
        Ok(Record::Object(id))
    }

    fn serialize_error(
        &mut self,
        program: &ResidualProgram,
        source: Value,
        serializer: &mut Serializer,
    ) -> Result<Record, JsError> {
        let source_root = serializer.root(self, source);
        // Node snapshots both own descriptors before name/message coercion or
        // stack access can run guest code and mutate the original error.
        let mut fields = [None, None];
        for (index, name) in ["message", "cause"].into_iter().enumerate() {
            let key = self.heap.alloc(Cell::String(name.into()));
            if let Some((value, attributes)) = self.ordinary_own_property(program, source, key)?
                && !attributes.accessor
            {
                fields[index] = Some(serializer.root(self, value));
            }
        }
        let [message, cause] = fields;
        let name_atom = self.intern_atom("name");
        let name = self.get_property(program, source, name_atom)?;
        let name = self.to_string(program, name)?;
        let native = match name.as_str() {
            "EvalError" => Native::EvalError,
            "RangeError" => Native::RangeError,
            "ReferenceError" => Native::ReferenceError,
            "SyntaxError" => Native::SyntaxError,
            "TypeError" => Native::TypeError,
            "URIError" => Native::URIError,
            _ => Native::Error,
        };
        let message = match message {
            Some(root) => {
                let value = self.heap.root_value(root).unwrap();
                Some(self.coerce_js_string(program, value)?.units().to_vec())
            }
            None => None,
        };
        let stack_atom = self.intern_atom("stack");
        let source = self.heap.root_value(source_root).unwrap();
        let stack = self.get_property(program, source, stack_atom)?;
        let stack = match self.heap.get(stack) {
            Some(Cell::String(text)) => Some(text.units().to_vec()),
            _ => None,
        };
        let id = serializer.reserve(self, source, ObjectRecord::Pending);
        let cause = match cause {
            Some(cause) => {
                let cause = self.heap.root_value(cause).unwrap();
                Some(self.serialize_record(program, cause, serializer)?)
            }
            None => None,
        };
        serializer.objects[id] = ObjectRecord::Error {
            native,
            message,
            stack,
            cause,
        };
        Ok(Record::Object(id))
    }

    /// Own enumerable string-keyed properties, read through getters in key order.
    fn serialize_properties(
        &mut self,
        program: &ResidualProgram,
        source: Value,
        serializer: &mut Serializer,
    ) -> Result<Vec<(Vec<u16>, Record)>, JsError> {
        if matches!(self.heap.get(source), Some(Cell::Proxy { .. })) {
            return Err(self.data_clone_error(program, "Proxy objects cannot be cloned"));
        }
        let source = serializer.root(self, source);
        let source_value = self.heap.root_value(source).unwrap();
        let keys = self.object_own_key_values(program, source_value)?;
        let keys: Vec<_> = keys.into_iter().map(|key| serializer.root(self, key)).collect();
        let mut properties = Vec::new();
        for key_root in keys {
            let key = self.heap.root_value(key_root).unwrap();
            let Some(Cell::String(name)) = self.heap.get(key).cloned() else {
                continue;
            };
            let atom = self.intern_js_atom(&name);
            if self.is_private_name(atom)
                || !self
                    .ordinary_own_property(
                        program,
                        self.heap.root_value(source).unwrap(),
                        key,
                    )?
                    .is_some_and(|(_, attributes)| attributes.enumerable)
            {
                continue;
            }
            let source_value = self.heap.root_value(source).unwrap();
            let key = self.heap.root_value(key_root).unwrap();
            let item = self.get_index(program, source_value, key)?;
            let item = self.serialize_record(program, item, serializer)?;
            properties.push((name.units().to_vec(), item));
        }
        Ok(properties)
    }

    fn deserialize_record(
        &mut self,
        program: &ResidualProgram,
        record: &Record,
        serialized: &mut SerializedValue,
        host_objects: &[Value],
        deserializer: &mut Deserializer,
    ) -> Result<Value, JsError> {
        Ok(match record {
            Record::Undefined => Value::UNDEFINED,
            Record::Null => Value::NULL,
            Record::Boolean(value) => if *value { Value::TRUE } else { Value::FALSE },
            Record::Number(value) => Value::number(*value),
            Record::BigInt(value) => self.heap.alloc(Cell::BigInt(value.clone())),
            Record::String(units) => self.heap.alloc(Cell::String(JsString::from_units(units))),
            Record::Object(id) => {
                return self.deserialize_object(program, *id, serialized, host_objects, deserializer);
            }
        })
    }

    fn deserialize_object(
        &mut self,
        program: &ResidualProgram,
        id: usize,
        serialized: &mut SerializedValue,
        host_objects: &[Value],
        deserializer: &mut Deserializer,
    ) -> Result<Value, JsError> {
        if let Some(created) = deserializer.created[id] {
            return Ok(self.heap.root_value(created).unwrap());
        }
        let record = std::mem::replace(&mut serialized.objects[id], ObjectRecord::Pending);
        let created = |vm: &mut Self, deserializer: &mut Deserializer, value: Value| {
            let root = deserializer.root(vm, value);
            deserializer.created[id] = Some(root);
            root
        };
        let target = match record {
            ObjectRecord::Pending => unreachable!("every reached object is serialized"),
            ObjectRecord::Host(index) => {
                let host = host_objects[index];
                created(self, deserializer, host)
            }
            ObjectRecord::Ordinary(properties) => {
                let target = self
                    .heap
                    .alloc(Cell::Object(Self::empty_object(self.object_proto)));
                let target = created(self, deserializer, target);
                self.deserialize_properties(program, target, properties, serialized, host_objects, deserializer)?;
                target
            }
            ObjectRecord::Array { length, properties } => {
                let prototype = self.array_prototype_for_realm(self.realm.globals);
                let target = self.heap.alloc(Cell::Array {
                    object: crate::heap::ArrayObject::new(
                        Self::empty_object(prototype),
                        Rc::new(Vec::new()),
                    ),
                });
                let target = created(self, deserializer, target);
                self.deserialize_properties(program, target, properties, serialized, host_objects, deserializer)?;
                let value = self.heap.root_value(target).unwrap();
                self.set_array_length(program, value, Value::number(length as f64))?;
                target
            }
            ObjectRecord::Boxed(payload) => {
                let payload = self.deserialize_record(program, &payload, serialized, host_objects, deserializer)?;
                let target = self.box_primitive_object(payload)?;
                created(self, deserializer, target)
            }
            ObjectRecord::Date(milliseconds) => {
                let prototype = self.intrinsic_builtin_prototype(Native::Date);
                let target = self.heap.alloc(Cell::Date {
                    milliseconds,
                    object: Box::new(Self::empty_object(prototype)),
                });
                created(self, deserializer, target)
            }
            ObjectRecord::RegExp { source, flags } => {
                let intrinsics = &self.realm.intrinsics.regexp_intrinsics[&self.realm.globals];
                let (prototype, constructor) = (intrinsics.prototype, intrinsics.constructor);
                let target = self.regexp_from_source(
                    prototype,
                    JsString::from_units(&source),
                    flags,
                    crate::heap::RegExpLegacyOwner::Enabled(constructor),
                )?;
                created(self, deserializer, target)
            }
            ObjectRecord::Map(entries) => {
                let prototype = self.intrinsic_builtin_prototype(Native::Map);
                let target = self.heap.alloc(Cell::Map {
                    object: Box::new(Self::empty_object(prototype)),
                    entries: Box::default(),
                });
                let target = created(self, deserializer, target);
                for (key, value) in entries {
                    let key = self.deserialize_record(program, &key, serialized, host_objects, deserializer)?;
                    let key = deserializer.root(self, key);
                    let value = self.deserialize_record(program, &value, serialized, host_objects, deserializer)?;
                    let key = self.heap.root_value(key).unwrap();
                    let map = self.heap.root_value(target).unwrap();
                    self.call_collection_native(program, Native::MapSet, map, &[key, value])?;
                }
                target
            }
            ObjectRecord::Set(values) => {
                let prototype = self.intrinsic_builtin_prototype(Native::Set);
                let target = self.heap.alloc(Cell::Set {
                    object: Box::new(Self::empty_object(prototype)),
                    entries: Box::default(),
                });
                let target = created(self, deserializer, target);
                for value in values {
                    let value = self.deserialize_record(program, &value, serialized, host_objects, deserializer)?;
                    let set = self.heap.root_value(target).unwrap();
                    self.call_collection_native(program, Native::SetAdd, set, &[value])?;
                }
                target
            }
            ObjectRecord::Error {
                native,
                message,
                stack,
                cause,
            } => {
                let prototype = self.intrinsic_builtin_prototype(native);
                let error = self.heap.alloc(Cell::Object(Object::error(prototype)));
                let target = created(self, deserializer, error);
                let stack = match stack {
                    Some(units) => self.heap.alloc(Cell::String(JsString::from_units(&units))),
                    None => Value::UNDEFINED,
                };
                let error = self.heap.root_value(target).unwrap();
                self.install_error_stack(program, error, StackData::Formatted(stack))?;
                if let Some(message) = message {
                    let message = self.heap.alloc(Cell::String(JsString::from_units(&message)));
                    let error = self.heap.root_value(target).unwrap();
                    self.set_builtin_value_named(error, "message", message)?;
                }
                if let Some(cause) = cause {
                    let cause = self.deserialize_record(program, &cause, serialized, host_objects, deserializer)?;
                    let error = self.heap.root_value(target).unwrap();
                    self.set_builtin_value_named(error, "cause", cause)?;
                }
                target
            }
            ObjectRecord::ArrayBuffer {
                bytes,
                max_byte_length,
                resizable,
                immutable,
            } => {
                let target = self.new_array_buffer_cell(bytes.into(), false, max_byte_length, resizable, immutable);
                created(self, deserializer, target)
            }
            ObjectRecord::Transferred(index) => {
                let contents = std::mem::take(&mut serialized.transferred[index]);
                let target = self.new_array_buffer_cell(
                    contents.bytes.into(),
                    false,
                    contents.max_byte_length,
                    contents.resizable,
                    false,
                );
                created(self, deserializer, target)
            }
            ObjectRecord::Shared {
                block,
                max_byte_length,
                resizable,
            } => {
                let bytes = crate::heap::BufferBytes::concurrent(block);
                let target = self.new_array_buffer_cell(bytes, true, max_byte_length, resizable, false);
                created(self, deserializer, target)
            }
            ObjectRecord::View(view) => {
                let buffer = self.deserialize_object(program, view.buffer, serialized, host_objects, deserializer)?;
                let target = match view.kind {
                    Some(kind) => {
                        let prototype = self.typed_array_proto(kind);
                        self.heap.alloc(Cell::TypedArray {
                            kind,
                            object: Box::new(crate::heap::BufferViewObject {
                                object: Self::empty_object(prototype),
                                offset: view.offset,
                            }),
                            buffer,
                            length: view.length,
                            length_tracking: view.length_tracking,
                        })
                    }
                    None => {
                        let prototype = self.intrinsic_builtin_prototype(Native::DataView);
                        self.heap.alloc(Cell::DataView {
                            object: Box::new(crate::heap::BufferViewObject {
                                object: Self::empty_object(prototype),
                                offset: view.offset,
                            }),
                            buffer,
                            length: view.length,
                            length_tracking: view.length_tracking,
                        })
                    }
                };
                let target = created(self, deserializer, target);
                deserializer.views.push(DeserializedView {
                    view: target,
                    byte_length: view.byte_length,
                    backed_by_resizable: view.backed_by_resizable,
                });
                target
            }
        };
        Ok(self.heap.root_value(target).unwrap())
    }

    fn deserialize_properties(
        &mut self,
        program: &ResidualProgram,
        target: RootId,
        properties: Vec<(Vec<u16>, Record)>,
        serialized: &mut SerializedValue,
        host_objects: &[Value],
        deserializer: &mut Deserializer,
    ) -> Result<(), JsError> {
        for (name, record) in properties {
            let item = self.deserialize_record(program, &record, serialized, host_objects, deserializer)?;
            let item = deserializer.root(self, item);
            let key = self.heap.alloc(Cell::String(JsString::from_units(&name)));
            let target_value = self.heap.root_value(target).unwrap();
            let item = self.heap.root_value(item).unwrap();
            if !self.define_own_property_record(
                program,
                target_value,
                key,
                PropertyDescriptorRecord::data(item),
            )? {
                return Err(self.data_clone_error(program, "Unable to create cloned property"));
            }
        }
        Ok(())
    }

    fn new_array_buffer_cell(
        &mut self,
        bytes: crate::heap::BufferBytes,
        shared: bool,
        max_byte_length: usize,
        resizable: bool,
        immutable: bool,
    ) -> Value {
        let native = if shared { Native::SharedArrayBuffer } else { Native::ArrayBuffer };
        let prototype = self.intrinsic_builtin_prototype(native);
        self.heap.alloc(Cell::ArrayBuffer {
            object: Box::new(Self::empty_object(prototype)),
            bytes,
            shared,
            detached: false,
            max_byte_length,
            resizable,
            immutable,
        })
    }

    fn validate_cloned_views(
        &mut self,
        program: &ResidualProgram,
        views: &[DeserializedView],
    ) -> Result<(), JsError> {
        for view in views {
            let value = self.heap.root_value(view.view).unwrap();
            let (buffer, offset, tracking) = match self.heap.get(value) {
                Some(
                    Cell::TypedArray {
                        buffer,
                        object,
                        length_tracking,
                        ..
                    }
                    | Cell::DataView {
                        buffer,
                        object,
                        length_tracking,
                        ..
                    },
                ) => (*buffer, object.offset, *length_tracking),
                _ => unreachable!("serialized view remains live"),
            };
            let Some(Cell::ArrayBuffer {
                bytes,
                resizable,
                shared,
                ..
            }) = self.heap.get(buffer)
            else {
                unreachable!("serialized view retains its backing buffer");
            };
            if offset > bytes.len()
                || view.byte_length > bytes.len() - offset
                || ((view.backed_by_resizable || tracking) && !resizable)
                || (view.backed_by_resizable && *shared)
                || (*resizable && !shared && !view.backed_by_resizable)
            {
                let message = "Unable to deserialize cloned data.";
                let text = self.heap.alloc(Cell::String(message.into()));
                let error = self.construct_error_native(program, Native::Error, &[text])?;
                return Err(JsError::thrown(error, message.into()));
            }
        }
        Ok(())
    }

    fn structured_clone_transfers(
        &mut self,
        program: &ResidualProgram,
        options: Option<RootId>,
        serializer: &mut Serializer,
    ) -> Result<Vec<RootId>, JsError> {
        let Some(options) = options else {
            return Ok(Vec::new());
        };
        let options = self.embedding_value(options)?;
        if options.is_null() || options.is_undefined() {
            return Ok(Vec::new());
        }
        if !self.is_object_like(options) {
            return Err(
                self.type_error(program, "structuredClone options must be an object".into())
            );
        }

        let transfer_atom = self.intern_atom("transfer");
        let transfer = self.get_property(program, options, transfer_atom)?;
        if transfer.is_null() || transfer.is_undefined() {
            return Ok(Vec::new());
        }
        if !self.is_object_like(transfer) {
            return Err(self.type_error(
                program,
                "structuredClone transfer must be an iterable object".into(),
            ));
        }

        let transfer_root = serializer.root(self, transfer);
        let transfer = self.heap.root_value(transfer_root).unwrap();
        let transfer_roots = self.iterable_to_rooted_list(program, transfer)?;
        serializer.roots.extend(transfer_roots.iter().copied());
        Ok(transfer_roots)
    }

    fn validate_transfer_roots(
        &mut self,
        program: &ResidualProgram,
        transfers: &[RootId],
    ) -> Result<(), JsError> {
        let mut seen = HashSet::new();
        for root in transfers.iter().copied() {
            let value = self.heap.root_value(root).unwrap();
            if self
                .object_data(value)
                .is_some_and(Object::is_structured_transfer_forbidden)
                || matches!(self.heap.get(value), Some(Cell::ArrayBuffer { detached, immutable, bytes, .. })
                    if *detached || *immutable || bytes.is_wasm_view())
            {
                return Err(
                    self.data_clone_error(program, "Cannot transfer object of unsupported type.")
                );
            }
            let valid = matches!(
                self.heap.get(value),
                Some(Cell::ArrayBuffer {
                    bytes,
                    shared: false,
                    detached: false,
                    immutable: false,
                    ..
                }) if !bytes.is_wasm_view()
            );
            if !valid {
                return Err(self.data_clone_error(program, "Found invalid value in transferList."));
            }
            if !seen.insert(value) {
                return Err(
                    self.data_clone_error(program, "Transfer list contains duplicate ArrayBuffer")
                );
            }
        }
        Ok(())
    }

    /// V8's DataCloneError for a value it has no serialization for: the value shown
    /// without side effects, "could not be cloned."
    fn uncloneable(&mut self, program: &ResidualProgram, value: Value) -> JsError {
        let shown = if let Some(Cell::Symbol(description)) = self.heap.get(value) {
            format!("Symbol({})", description.as_deref().unwrap_or(""))
        } else if self.is_function(value) {
            let source = self.function_source_value(value).and_then(|source| match self.heap.get(source) {
                Some(Cell::String(text)) => Some(text.to_string()),
                _ => None,
            });
            match source {
                Some(source) => source,
                None => {
                    let name = self.debug_function_name(value).map(|name| name.to_string()).unwrap_or_default();
                    format!("function {name}() {{ [native code] }}")
                }
            }
        } else {
            let name = self.constructor_name(value).map(|name| name.to_string());
            format!("#<{}>", name.as_deref().unwrap_or("Object"))
        };
        self.data_clone_error(program, &format!("{shown} could not be cloned."))
    }

    fn data_clone_error(&mut self, program: &ResidualProgram, message: &str) -> JsError {
        let message_value = self.heap.alloc(Cell::String(message.into()));
        let error = self
            .construct_error_native(program, Native::Error, &[message_value])
            .unwrap_or_else(|_| {
                self.heap
                    .alloc(Cell::Object(Object::error(self.object_proto)))
            });
        let name = self.heap.alloc(Cell::String("DataCloneError".into()));
        let _ = self.set_builtin_value_named(error, "name", name);
        JsError::data_clone(error, message)
    }
}

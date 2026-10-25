//! V8's value serialization (deps/v8/src/objects/value-serializer.cc, wire format 15).
//! One serializer serves `structuredClone`, `postMessage` and an embedder's scripted
//! `v8.Serializer`: a value becomes bytes, with objects numbered in first-reach order so
//! repeated and cyclic references resolve to one object, and embedder hooks writing host
//! objects and naming shared buffers. Guest code runs while serializing (getters,
//! coercions, hooks); deserializing only allocates, apart from host-object hooks.

use super::object_descriptors::PropertyDescriptorRecord;
use super::*;
use crate::shared_backing::SharedBacking;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::rc::Rc;
use std::sync::Arc;

/// The format version this serializer writes (`kLatestVersion`).
pub const WIRE_FORMAT_VERSION: u32 = 15;
/// The oldest format the embedder API reads (`kMinimumNonLegacyVersion` in api.cc; V8's
/// legacy reader stays disabled), and the first version with view flags.
const MINIMUM_NON_LEGACY_VERSION: u32 = 13;
const VERSION_VIEW_FLAGS: u32 = 14;

/// `SerializationTag`.
mod tag {
    pub(super) const VERSION: u8 = 0xFF;
    pub(super) const PADDING: u8 = b'\0';
    pub(super) const VERIFY_OBJECT_COUNT: u8 = b'?';
    pub(super) const THE_HOLE: u8 = b'-';
    pub(super) const UNDEFINED: u8 = b'_';
    pub(super) const NULL: u8 = b'0';
    pub(super) const TRUE: u8 = b'T';
    pub(super) const FALSE: u8 = b'F';
    pub(super) const INT32: u8 = b'I';
    pub(super) const UINT32: u8 = b'U';
    pub(super) const DOUBLE: u8 = b'N';
    pub(super) const BIGINT: u8 = b'Z';
    pub(super) const UTF8_STRING: u8 = b'S';
    pub(super) const ONE_BYTE_STRING: u8 = b'"';
    pub(super) const TWO_BYTE_STRING: u8 = b'c';
    pub(super) const OBJECT_REFERENCE: u8 = b'^';
    pub(super) const BEGIN_JS_OBJECT: u8 = b'o';
    pub(super) const END_JS_OBJECT: u8 = b'{';
    pub(super) const BEGIN_SPARSE_JS_ARRAY: u8 = b'a';
    pub(super) const END_SPARSE_JS_ARRAY: u8 = b'@';
    pub(super) const BEGIN_DENSE_JS_ARRAY: u8 = b'A';
    pub(super) const END_DENSE_JS_ARRAY: u8 = b'$';
    pub(super) const DATE: u8 = b'D';
    pub(super) const TRUE_OBJECT: u8 = b'y';
    pub(super) const FALSE_OBJECT: u8 = b'x';
    pub(super) const NUMBER_OBJECT: u8 = b'n';
    pub(super) const BIGINT_OBJECT: u8 = b'z';
    pub(super) const STRING_OBJECT: u8 = b's';
    pub(super) const REGEXP: u8 = b'R';
    pub(super) const BEGIN_JS_MAP: u8 = b';';
    pub(super) const END_JS_MAP: u8 = b':';
    pub(super) const BEGIN_JS_SET: u8 = b'\'';
    pub(super) const END_JS_SET: u8 = b',';
    pub(super) const ARRAY_BUFFER: u8 = b'B';
    pub(super) const IMMUTABLE_ARRAY_BUFFER: u8 = b'C';
    pub(super) const RESIZABLE_ARRAY_BUFFER: u8 = b'~';
    pub(super) const ARRAY_BUFFER_TRANSFER: u8 = b't';
    pub(super) const ARRAY_BUFFER_VIEW: u8 = b'V';
    pub(super) const SHARED_ARRAY_BUFFER: u8 = b'u';
    pub(super) const HOST_OBJECT: u8 = b'\\';
    pub(super) const ERROR: u8 = b'r';
}

/// `ErrorTag`: error sub-records.
mod error_tag {
    pub(super) const EVAL_ERROR: u8 = b'E';
    pub(super) const RANGE_ERROR: u8 = b'R';
    pub(super) const REFERENCE_ERROR: u8 = b'F';
    pub(super) const SYNTAX_ERROR: u8 = b'S';
    pub(super) const TYPE_ERROR: u8 = b'T';
    pub(super) const URI_ERROR: u8 = b'U';
    pub(super) const MESSAGE: u8 = b'm';
    pub(super) const CAUSE: u8 = b'c';
    pub(super) const STACK: u8 = b's';
    pub(super) const END: u8 = b'.';
}

/// Native error constructors and their prototype tags; other names deserialize as Error.
const ERROR_PROTOTYPES: [(&str, Native, u8); 6] = [
    ("EvalError", Native::EvalError, error_tag::EVAL_ERROR),
    ("RangeError", Native::RangeError, error_tag::RANGE_ERROR),
    (
        "ReferenceError",
        Native::ReferenceError,
        error_tag::REFERENCE_ERROR,
    ),
    ("SyntaxError", Native::SyntaxError, error_tag::SYNTAX_ERROR),
    ("TypeError", Native::TypeError, error_tag::TYPE_ERROR),
    ("URIError", Native::URIError, error_tag::URI_ERROR),
];

/// `ArrayBufferViewTag` per typed array kind; DataView has its own.
const VIEW_TAGS: [(TypedArrayKind, u8); 12] = [
    (TypedArrayKind::Int8, b'b'),
    (TypedArrayKind::Uint8, b'B'),
    (TypedArrayKind::Uint8Clamped, b'C'),
    (TypedArrayKind::Int16, b'w'),
    (TypedArrayKind::Uint16, b'W'),
    (TypedArrayKind::Int32, b'd'),
    (TypedArrayKind::Uint32, b'D'),
    (TypedArrayKind::Float16, b'h'),
    (TypedArrayKind::Float32, b'f'),
    (TypedArrayKind::Float64, b'F'),
    (TypedArrayKind::BigInt64, b'q'),
    (TypedArrayKind::BigUint64, b'Q'),
];
const DATA_VIEW_TAG: u8 = b'?';
/// `JSArrayBufferViewFlags` bits.
const VIEW_LENGTH_TRACKING: u32 = 1 << 0;
const VIEW_BACKED_BY_RESIZABLE: u32 = 1 << 1;

/// `JSRegExp::Flags` bits by flag character (regexp-flags.h).
const REGEXP_FLAG_BITS: [(char, u32); 9] = [
    ('d', 7),
    ('g', 0),
    ('i', 1),
    ('l', 6),
    ('m', 2),
    ('s', 5),
    ('u', 4),
    ('v', 8),
    ('y', 3),
];
/// The linear-engine flag is accepted only behind a V8 flag Node leaves off.
const REGEXP_LINEAR_BIT: u32 = 6;

/// BigInt bitfield: the sign bit, then the digit storage length in bytes.
const BIGINT_SIGN_BIT: u32 = 1;
const BIGINT_LENGTH_SHIFT: u32 = 1;
const BIGINT_DIGIT_BYTES: usize = 8;

/// Base-128 varints: seven payload bits per byte, high bit continues.
const VARINT_PAYLOAD_BITS: u32 = 7;
const VARINT_PAYLOAD_MASK: u8 = 0x7F;
const VARINT_CONTINUATION: u8 = 0x80;

const DESERIALIZATION_ERROR: &str = "Unable to deserialize cloned data.";
const VERSION_ERROR: &str =
    "Unable to deserialize cloned data due to invalid or unsupported version.";
const DETACHED_MESSAGE: &str = "An ArrayBuffer is detached and could not be cloned.";

/// Bytes a serializer appends to. Embedder host-object hooks write to the same output.
pub type WireOutput = Rc<RefCell<Vec<u8>>>;

fn put_varint(output: &WireOutput, mut value: u64) {
    let mut output = output.borrow_mut();
    loop {
        let byte = (value as u8) & VARINT_PAYLOAD_MASK;
        value >>= VARINT_PAYLOAD_BITS;
        if value == 0 {
            output.push(byte);
            return;
        }
        output.push(byte | VARINT_CONTINUATION);
    }
}

fn varint_len(mut value: u64) -> usize {
    let mut length = 1;
    while value >> VARINT_PAYLOAD_BITS != 0 {
        value >>= VARINT_PAYLOAD_BITS;
        length += 1;
    }
    length
}

fn put_tag(output: &WireOutput, tag: u8) {
    output.borrow_mut().push(tag);
}

fn put_zigzag(output: &WireOutput, value: i32) {
    put_varint(
        output,
        u64::from(((value as u32) << 1) ^ ((value >> 31) as u32)),
    );
}

/// Doubles use host byte order, as V8 writes them.
fn put_double(output: &WireOutput, value: f64) {
    output.borrow_mut().extend_from_slice(&value.to_ne_bytes());
}

/// `ValueSerializer::WriteUint32` and friends, for embedder hooks.
pub fn write_uint32(output: &WireOutput, value: u32) {
    put_varint(output, u64::from(value));
}

pub fn write_uint64(output: &WireOutput, value: u64) {
    put_varint(output, value);
}

pub fn write_double(output: &WireOutput, value: f64) {
    put_double(output, value);
}

pub fn write_raw_bytes(output: &WireOutput, bytes: &[u8]) {
    output.borrow_mut().extend_from_slice(bytes);
}

/// Numbers V8 keeps as Smis (any int32 on the 64-bit builds Node ships) serialize as
/// zigzag integers; every other number as a double.
fn smi(value: f64) -> Option<i32> {
    let integer = value as i32;
    (f64::from(integer) == value && !(value == 0.0 && value.is_sign_negative())).then_some(integer)
}

fn put_number(output: &WireOutput, value: f64) {
    match smi(value) {
        Some(integer) => {
            put_tag(output, tag::INT32);
            put_zigzag(output, integer);
        }
        None => {
            put_tag(output, tag::DOUBLE);
            put_double(output, value);
        }
    }
}

/// One-byte strings when every unit fits a byte; otherwise two-byte, padded so the
/// characters start at an even offset.
fn put_string(output: &WireOutput, units: crate::vm::wtf16::Units<'_>) {
    if units.iter().all(|unit| unit <= u16::from(u8::MAX)) {
        put_tag(output, tag::ONE_BYTE_STRING);
        put_varint(output, units.len() as u64);
        output
            .borrow_mut()
            .extend(units.iter().map(|unit| unit as u8));
        return;
    }
    let byte_length = units.len() * std::mem::size_of::<u16>();
    let start = output.borrow().len();
    if (start + 1 + varint_len(byte_length as u64)) & 1 != 0 {
        put_tag(output, tag::PADDING);
    }
    put_tag(output, tag::TWO_BYTE_STRING);
    put_varint(output, byte_length as u64);
    let mut output = output.borrow_mut();
    for unit in units.iter() {
        output.extend_from_slice(&unit.to_ne_bytes());
    }
}

fn put_bigint_contents(output: &WireOutput, value: &num_bigint::BigInt) {
    let (sign, mut magnitude) = value.to_bytes_le();
    if sign == num_bigint::Sign::NoSign {
        magnitude.clear();
    }
    let digits = magnitude.len().div_ceil(BIGINT_DIGIT_BYTES);
    magnitude.resize(digits * BIGINT_DIGIT_BYTES, 0);
    let negative = u32::from(sign == num_bigint::Sign::Minus);
    put_varint(
        output,
        u64::from(negative | ((magnitude.len() as u32) << BIGINT_LENGTH_SHIFT)),
    );
    output.borrow_mut().extend_from_slice(&magnitude);
}

/// Serialized bytes being read; embedder host-object hooks read from the same input.
pub struct WireInput {
    bytes: Vec<u8>,
    position: usize,
    version: u32,
}

pub type SharedWireInput = Rc<RefCell<WireInput>>;

impl WireInput {
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            position: 0,
            version: 0,
        }
    }

    /// The header's format version (0 before `readHeader`).
    pub fn version(&self) -> u32 {
        self.version
    }

    fn peek_tag(&self) -> Option<u8> {
        self.bytes[self.position..]
            .iter()
            .copied()
            .find(|byte| *byte != tag::PADDING)
    }

    fn read_tag(&mut self) -> Option<u8> {
        loop {
            let byte = *self.bytes.get(self.position)?;
            self.position += 1;
            if byte != tag::PADDING {
                return Some(byte);
            }
        }
    }

    /// `ReadVarintLoop`: bits beyond the target width are dropped, and reading stops
    /// once the width is exhausted.
    fn read_varint(&mut self, width_bits: u32) -> Option<u64> {
        let mut value = 0u64;
        let mut shift = 0;
        loop {
            let byte = *self.bytes.get(self.position)?;
            if shift >= width_bits {
                return Some(value);
            }
            value |= u64::from(byte & VARINT_PAYLOAD_MASK) << shift;
            shift += VARINT_PAYLOAD_BITS;
            self.position += 1;
            if byte & VARINT_CONTINUATION == 0 {
                return Some(value);
            }
        }
    }

    pub fn read_uint32(&mut self) -> Option<u32> {
        self.read_varint(u32::BITS).map(|value| value as u32)
    }

    pub fn read_uint64(&mut self) -> Option<u64> {
        self.read_varint(u64::BITS)
    }

    fn read_byte_varint(&mut self) -> Option<u8> {
        self.read_varint(u8::BITS).map(|value| value as u8)
    }

    fn read_zigzag(&mut self) -> Option<i32> {
        let value = self.read_uint32()?;
        Some(((value >> 1) as i32) ^ -((value & 1) as i32))
    }

    pub fn read_double(&mut self) -> Option<f64> {
        let bytes = self
            .bytes
            .get(self.position..self.position + std::mem::size_of::<f64>())?;
        let value = f64::from_ne_bytes(bytes.try_into().ok()?);
        self.position += std::mem::size_of::<f64>();
        Some(if value.is_nan() { f64::NAN } else { value })
    }

    /// Consume `length` raw bytes; their offset in the input.
    pub fn read_raw(&mut self, length: usize) -> Option<usize> {
        let start = self.position;
        if length > self.bytes.len() - start {
            return None;
        }
        self.position += length;
        Some(start)
    }

    fn raw(&mut self, length: usize) -> Option<&[u8]> {
        let start = self.read_raw(length)?;
        Some(&self.bytes[start..start + length])
    }

    fn remaining(&self) -> usize {
        self.bytes.len() - self.position
    }
}

/// Serialization state that persists across `writeValue` calls (V8's `ValueSerializer`):
/// object ids and the transferred-buffer table. Objects it numbers stay rooted until
/// the owner releases it.
pub struct ValueSerializer {
    output: WireOutput,
    ids: HashMap<Value, u32>,
    transfers: HashMap<Value, u32>,
    views_as_host_objects: bool,
    roots: Vec<RootId>,
}

impl ValueSerializer {
    pub fn new(output: WireOutput) -> Self {
        Self {
            output,
            ids: HashMap::new(),
            transfers: HashMap::new(),
            views_as_host_objects: false,
            roots: Vec::new(),
        }
    }

    pub fn output(&self) -> &WireOutput {
        &self.output
    }

    pub fn write_header(&self) {
        put_tag(&self.output, tag::VERSION);
        put_varint(&self.output, u64::from(WIRE_FORMAT_VERSION));
    }

    pub fn set_treat_array_buffer_views_as_host_objects(&mut self, mode: bool) {
        self.views_as_host_objects = mode;
    }
}

/// Deserialization state across `readValue` calls (V8's `ValueDeserializer`).
pub struct ValueDeserializer {
    input: SharedWireInput,
    objects: Vec<Option<RootId>>,
    transfers: HashMap<u32, RootId>,
    roots: Vec<RootId>,
}

impl ValueDeserializer {
    pub fn new(input: SharedWireInput) -> Self {
        Self {
            input,
            objects: Vec::new(),
            transfers: HashMap::new(),
            roots: Vec::new(),
        }
    }

    pub fn input(&self) -> &SharedWireInput {
        &self.input
    }
}

/// The serializer's embedder hooks (V8's `ValueSerializer::Delegate`).
pub(crate) trait WriteHooks<H: Host> {
    /// Write a host object's contents after its tag; a returned value is written next,
    /// through this serializer.
    fn write_host_object(
        &mut self,
        vm: &mut Vm<H>,
        program: &ResidualProgram,
        output: &WireOutput,
        object: Value,
    ) -> Result<Option<Value>, JsError>;
    fn shared_array_buffer_id(
        &mut self,
        vm: &mut Vm<H>,
        program: &ResidualProgram,
        buffer: Value,
    ) -> Result<u32, JsError>;
    /// The exception for a value that cannot be cloned.
    fn data_clone_error(
        &mut self,
        vm: &mut Vm<H>,
        program: &ResidualProgram,
        message: &str,
    ) -> JsError;
}

/// The deserializer's embedder hooks (V8's `ValueDeserializer::Delegate`).
/// What a host-object read produced: the object, or a request to read the next value
/// as the object.
pub enum HostObjectRead<T> {
    Object(T),
    NextValue,
}

pub(crate) trait ReadHooks<H: Host> {
    fn read_host_object(
        &mut self,
        vm: &mut Vm<H>,
        program: &ResidualProgram,
        input: &SharedWireInput,
    ) -> Result<HostObjectRead<Value>, JsError>;
    fn shared_array_buffer(
        &mut self,
        vm: &mut Vm<H>,
        program: &ResidualProgram,
        id: u32,
    ) -> Result<Value, JsError>;
}

/// Why a read stopped: malformed input, or an exception already thrown.
enum ReadFailure {
    Invalid,
    Thrown(JsError),
}

impl From<JsError> for ReadFailure {
    fn from(error: JsError) -> Self {
        Self::Thrown(error)
    }
}

type Read<T> = Result<T, ReadFailure>;

fn valid<T>(value: Option<T>) -> Read<T> {
    value.ok_or(ReadFailure::Invalid)
}

/// A message's serialized form: bytes, plus what crosses outside them — transferred
/// buffer contents (by transfer id) and shared memory blocks (by shared-buffer id).
pub struct SerializedValue {
    bytes: Vec<u8>,
    transferred: Vec<TransferredBuffer>,
    shared: Vec<Arc<SharedBacking>>,
}

impl std::fmt::Debug for SerializedValue {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SerializedValue")
            .field("bytes", &self.bytes.len())
            .field("transferred", &self.transferred.len())
            .field("shared", &self.shared.len())
            .finish()
    }
}

#[derive(Default)]
struct TransferredBuffer {
    bytes: Vec<u8>,
    max_byte_length: usize,
    resizable: bool,
}

/// Hooks for messages within and between agents (Node's messaging delegate): host
/// objects are the message's transferables by index, shared buffers its shared blocks.
struct MessageHooks {
    host_objects: Vec<Value>,
    shared: Vec<Arc<SharedBacking>>,
}

impl<H: Host> WriteHooks<H> for MessageHooks {
    fn write_host_object(
        &mut self,
        vm: &mut Vm<H>,
        program: &ResidualProgram,
        output: &WireOutput,
        object: Value,
    ) -> Result<Option<Value>, JsError> {
        if let Some(index) = self.host_objects.iter().position(|host| *host == object) {
            write_uint32(output, index as u32);
            return Ok(None);
        }
        let transfer_required = vm.heap.object_record(object).is_some_and(|object| {
            object.structured_serialization()
                == crate::heap::StructuredSerialization::TransferRequired
        });
        Err(vm.data_clone_error(
            program,
            if transfer_required {
                "Object that needs transfer was found in message but not listed in transferList"
            } else {
                "Cannot clone object of unsupported type."
            },
        ))
    }

    fn shared_array_buffer_id(
        &mut self,
        vm: &mut Vm<H>,
        _: &ResidualProgram,
        buffer: Value,
    ) -> Result<u32, JsError> {
        let Some(Cell::ArrayBuffer { bytes, .. }) = vm.heap.get(buffer) else {
            unreachable!("a shared buffer remains an ArrayBuffer");
        };
        let backing = bytes
            .shared_backing()
            .expect("shared buffers own a shared backing");
        if let Some(index) = self
            .shared
            .iter()
            .position(|shared| Arc::ptr_eq(shared, &backing))
        {
            return Ok(index as u32);
        }
        self.shared.push(backing);
        Ok(self.shared.len() as u32 - 1)
    }

    fn data_clone_error(
        &mut self,
        vm: &mut Vm<H>,
        program: &ResidualProgram,
        message: &str,
    ) -> JsError {
        vm.data_clone_error(program, message)
    }
}

impl<H: Host> ReadHooks<H> for MessageHooks {
    fn read_host_object(
        &mut self,
        _: &mut Vm<H>,
        _: &ResidualProgram,
        input: &SharedWireInput,
    ) -> Result<HostObjectRead<Value>, JsError> {
        let index = input.borrow_mut().read_uint32();
        let object = index
            .and_then(|index| self.host_objects.get(index as usize).copied())
            .unwrap_or(Value::UNDEFINED);
        Ok(HostObjectRead::Object(object))
    }

    fn shared_array_buffer(
        &mut self,
        vm: &mut Vm<H>,
        _: &ResidualProgram,
        id: u32,
    ) -> Result<Value, JsError> {
        Ok(match self.shared.get(id as usize) {
            Some(backing) => {
                let (maximum, growable) = (backing.maximum(), backing.growable());
                let bytes = crate::heap::BufferBytes::from_shared(backing.clone());
                vm.new_array_buffer_cell(bytes, true, maximum, growable, false)
            }
            None => Value::UNDEFINED,
        })
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
                .set_structured_serialization(crate::heap::StructuredSerialization::Forbidden);
        }
        Ok(())
    }

    /// Mark a host object serializable only by transfer (a MessagePort).
    pub(crate) fn embedding_require_transfer(&mut self, source: RootId) -> Result<(), JsError> {
        let source = self.embedding_value(source)?;
        if let Some(object) = self.heap.object_mut(source) {
            object.set_structured_serialization(
                crate::heap::StructuredSerialization::TransferRequired,
            );
        }
        Ok(())
    }

    pub(crate) fn embedding_structured_clone(
        &mut self,
        source: RootId,
        options: Option<RootId>,
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let mut roots = Vec::new();
        let transfers = self.structured_clone_transfers(&program, options, &mut roots);
        let serialized =
            transfers.and_then(|transfers| self.embedding_serialize(source, &transfers, &[]));
        for root in roots {
            self.heap.release_root(root);
        }
        self.deserialize_message(&program, serialized?, &[])
    }

    pub(crate) fn embedding_structured_clone_with_transfers(
        &mut self,
        source: RootId,
        transfers: &[RootId],
    ) -> Result<Value, JsError> {
        let program = self.embedding_program()?;
        let serialized = self.embedding_serialize(source, transfers, &[])?;
        self.deserialize_message(&program, serialized, &[])
    }

    /// Serialize `source` as a message, transferring `transfers` (ArrayBuffers) and
    /// writing each of `host_objects` as its index for the receiving host to recreate.
    pub(crate) fn embedding_serialize(
        &mut self,
        source: RootId,
        transfers: &[RootId],
        host_objects: &[RootId],
    ) -> Result<SerializedValue, JsError> {
        let program = self.embedding_program()?;
        self.validate_transfer_roots(&program, transfers)?;
        let mut serializer = ValueSerializer::new(WireOutput::default());
        serializer.write_header();
        for (index, transfer) in transfers.iter().enumerate() {
            let buffer = self.heap.root_value(*transfer).unwrap();
            serializer.transfers.insert(buffer, index as u32);
        }
        let mut hooks = MessageHooks {
            host_objects: host_objects
                .iter()
                .map(|root| self.embedding_value(*root))
                .collect::<Result<_, _>>()?,
            shared: Vec::new(),
        };
        let value = self.embedding_value(source)?;
        let written = self.wire_write(&program, &mut serializer, &mut hooks, value);
        self.release_wire_roots(&mut serializer.roots);
        written?;
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
            self.detach_array_buffer_native(&program, &[buffer])?;
            let bytes: crate::heap::BufferBytes = bytes;
            transferred.push(TransferredBuffer {
                bytes: bytes.into_vec(),
                max_byte_length,
                resizable,
            });
        }
        let bytes = std::mem::take(&mut *serializer.output.borrow_mut());
        Ok(SerializedValue {
            bytes,
            transferred,
            shared: hooks.shared,
        })
    }

    /// Deserialize a message into this heap; `host_objects` replace the sender's in order.
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
        self.deserialize_message(&program, serialized, &host_objects)
    }

    fn deserialize_message(
        &mut self,
        program: &ResidualProgram,
        serialized: SerializedValue,
        host_objects: &[Value],
    ) -> Result<Value, JsError> {
        let SerializedValue {
            bytes,
            transferred,
            shared,
        } = serialized;
        let mut deserializer = ValueDeserializer::new(Rc::new(RefCell::new(WireInput::new(bytes))));
        for (index, contents) in transferred.into_iter().enumerate() {
            let buffer = self.new_array_buffer_cell(
                contents.bytes.into(),
                false,
                contents.max_byte_length,
                contents.resizable,
                false,
            );
            self.wire_receive_array_buffer(&mut deserializer, index as u32, buffer);
        }
        let mut hooks = MessageHooks {
            host_objects: host_objects.to_vec(),
            shared,
        };
        let result = self
            .wire_read_header(program, &mut deserializer)
            .and_then(|()| self.wire_read_value(program, &mut deserializer, &mut hooks));
        // The result stays reachable from its own root until the deserializer releases.
        let result = result.map(|value| {
            let root = self.heap.root(value);
            (value, root)
        });
        self.release_wire_deserializer(&mut deserializer);
        result.map(|(value, root)| {
            self.heap.release_root(root);
            value
        })
    }

    fn release_wire_roots(&mut self, roots: &mut Vec<RootId>) {
        for root in roots.drain(..) {
            self.heap.release_root(root);
        }
    }

    pub(crate) fn release_wire_serializer(&mut self, serializer: &mut ValueSerializer) {
        serializer.ids.clear();
        serializer.transfers.clear();
        self.release_wire_roots(&mut serializer.roots);
    }

    pub(crate) fn release_wire_deserializer(&mut self, deserializer: &mut ValueDeserializer) {
        deserializer.objects.clear();
        deserializer.transfers.clear();
        self.release_wire_roots(&mut deserializer.roots);
    }

    pub(crate) fn is_array_buffer(&self, value: Value) -> bool {
        matches!(self.heap.get(value), Some(Cell::ArrayBuffer { .. }))
    }

    pub(crate) fn is_unshared_array_buffer(&self, value: Value) -> bool {
        matches!(
            self.heap.get(value),
            Some(Cell::ArrayBuffer { shared: false, .. })
        )
    }

    /// `ValueSerializer::TransferArrayBuffer`.
    pub(crate) fn wire_transfer_array_buffer(
        &mut self,
        serializer: &mut ValueSerializer,
        id: u32,
        buffer: Value,
    ) {
        serializer.roots.push(self.heap.root(buffer));
        serializer.transfers.insert(buffer, id);
    }

    /// `ValueDeserializer::TransferArrayBuffer`.
    pub(crate) fn wire_receive_array_buffer(
        &mut self,
        deserializer: &mut ValueDeserializer,
        id: u32,
        buffer: Value,
    ) {
        let root = self.heap.root(buffer);
        deserializer.roots.push(root);
        deserializer.transfers.insert(id, root);
    }

    fn uncloneable_with<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        hooks: &mut D,
        value: Value,
    ) -> JsError {
        let message = format!("{} could not be cloned.", self.clone_description(value));
        hooks.data_clone_error(self, program, &message)
    }

    /// `WriteObject`.
    pub(crate) fn wire_write<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        value: Value,
    ) -> Result<(), JsError> {
        let output = serializer.output.clone();
        if value.is_undefined() {
            put_tag(&output, tag::UNDEFINED);
            return Ok(());
        }
        if value.is_null() {
            put_tag(&output, tag::NULL);
            return Ok(());
        }
        if let Some(boolean) = value.as_bool() {
            put_tag(&output, if boolean { tag::TRUE } else { tag::FALSE });
            return Ok(());
        }
        if let Some(number) = value.as_number() {
            put_number(&output, number);
            return Ok(());
        }
        if let Some(text) = self.heap.string(value) {
            put_string(&output, text.units());
            return Ok(());
        }
        match self.heap.get(value) {
            Some(Cell::BigInt(value)) => {
                put_tag(&output, tag::BIGINT);
                put_bigint_contents(&output, value);
                Ok(())
            }
            Some(Cell::TypedArray { buffer, .. } | Cell::DataView { buffer, .. }) => {
                // A view's buffer serializes before the view is numbered.
                let buffer = *buffer;
                if !serializer.ids.contains_key(&value) && !serializer.views_as_host_objects {
                    let root = self.heap.root(value);
                    let written = self.wire_receiver(program, serializer, hooks, buffer);
                    self.heap.release_root(root);
                    written?;
                }
                self.wire_receiver(program, serializer, hooks, value)
            }
            _ if self.is_object_like(value) => {
                self.wire_receiver(program, serializer, hooks, value)
            }
            _ => Err(self.uncloneable_with(program, hooks, value)),
        }
    }

    /// `WriteJSReceiver`: a reference to an earlier object, or a newly numbered one.
    fn wire_receiver<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        value: Value,
    ) -> Result<(), JsError> {
        let output = serializer.output.clone();
        if let Some(id) = serializer.ids.get(&value) {
            put_tag(&output, tag::OBJECT_REFERENCE);
            put_varint(&output, u64::from(*id));
            return Ok(());
        }
        let id = serializer.ids.len() as u32;
        serializer.ids.insert(value, id);
        serializer.roots.push(self.heap.root(value));
        if self.is_function(value) || matches!(self.heap.get(value), Some(Cell::Proxy { .. })) {
            return Err(self.uncloneable_with(program, hooks, value));
        }
        let _stack = self.enter_stack()?;
        if self
            .object_data(value)
            .is_some_and(Object::is_arguments_object)
        {
            return Err(self.uncloneable_with(program, hooks, value));
        }
        if let Some(object) = self.heap.object_record(value) {
            if object.structured_serialization() != crate::heap::StructuredSerialization::Ordinary {
                return self.wire_host_object(program, serializer, hooks, value);
            }
            if object.has_error_data() {
                return self.wire_error(program, serializer, hooks, value);
            }
            if object.is_module_namespace() || self.is_promise(value) {
                return Err(self.uncloneable_with(program, hooks, value));
            }
            if let Some((_, inner)) = self.boxed_primitive_data(value) {
                return self.wire_primitive_wrapper(program, &output, hooks, value, inner);
            }
            put_tag(&output, tag::BEGIN_JS_OBJECT);
            let keys = self.wire_enumerable_keys(program, value, false)?;
            let written = self.wire_properties(program, serializer, hooks, value, &keys)?;
            put_tag(&output, tag::END_JS_OBJECT);
            put_varint(&output, u64::from(written));
            return Ok(());
        }
        if self.heap.array(value).is_some() {
            return self.wire_array(program, serializer, hooks, value);
        }
        enum Kind {
            Date(f64),
            RegExp(JsString, String),
            Map(Vec<Value>),
            Set(Vec<Value>),
            ArrayBuffer,
            View,
            Other,
        }
        let kind = match self.heap.get(value) {
            Some(Cell::Date { milliseconds, .. }) => Kind::Date(*milliseconds),
            Some(Cell::RegExp { meta, .. }) => Kind::RegExp(meta.source.clone(), meta.flags.text()),
            // Copy the entries first: serializing them can run guest code.
            Some(Cell::Map { entries, .. }) => Kind::Map(
                entries
                    .iter()
                    .flat_map(|(key, value)| [key, value])
                    .collect(),
            ),
            Some(Cell::Set { entries, .. }) => Kind::Set(entries.iter().collect()),
            Some(Cell::ArrayBuffer { .. }) => Kind::ArrayBuffer,
            Some(Cell::TypedArray { .. } | Cell::DataView { .. }) => Kind::View,
            _ => Kind::Other,
        };
        match kind {
            Kind::Date(milliseconds) => {
                put_tag(&output, tag::DATE);
                put_double(&output, milliseconds);
                Ok(())
            }
            Kind::RegExp(source, flags) => {
                let bits = flags
                    .chars()
                    .filter_map(|flag| REGEXP_FLAG_BITS.iter().find(|(name, _)| *name == flag))
                    .fold(0u32, |bits, (_, bit)| bits | (1 << bit));
                put_tag(&output, tag::REGEXP);
                put_string(&output, source.units());
                put_varint(&output, u64::from(bits));
                Ok(())
            }
            Kind::Map(entries) | Kind::Set(entries) => {
                let map = matches!(self.heap.get(value), Some(Cell::Map { .. }));
                let roots: Vec<RootId> = entries
                    .into_iter()
                    .map(|entry| self.heap.root(entry))
                    .collect();
                put_tag(
                    &output,
                    if map {
                        tag::BEGIN_JS_MAP
                    } else {
                        tag::BEGIN_JS_SET
                    },
                );
                let written = self.wire_rooted_sequence(program, serializer, hooks, &roots);
                put_tag(
                    &output,
                    if map {
                        tag::END_JS_MAP
                    } else {
                        tag::END_JS_SET
                    },
                );
                put_varint(&output, roots.len() as u64);
                written
            }
            Kind::ArrayBuffer => self.wire_array_buffer(program, serializer, hooks, value),
            Kind::View if serializer.views_as_host_objects => {
                self.wire_host_object(program, serializer, hooks, value)
            }
            Kind::View => self.wire_view(program, &output, hooks, value),
            Kind::Other => Err(self.uncloneable_with(program, hooks, value)),
        }
    }

    /// `WriteHostObject`: the tag, then the embedder's contents.
    fn wire_host_object<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        object: Value,
    ) -> Result<(), JsError> {
        let output = serializer.output.clone();
        put_tag(&output, tag::HOST_OBJECT);
        match hooks.write_host_object(self, program, &output, object)? {
            Some(next) => {
                let root = self.heap.root(next);
                let written = self.wire_write(program, serializer, hooks, next);
                self.heap.release_root(root);
                written
            }
            None => Ok(()),
        }
    }

    /// Write rooted values in order, releasing every root.
    fn wire_rooted_sequence<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        roots: &[RootId],
    ) -> Result<(), JsError> {
        let mut result = Ok(());
        for root in roots {
            if result.is_ok() {
                let value = self.heap.root_value(*root).unwrap();
                result = self.wire_write(program, serializer, hooks, value);
            }
            self.heap.release_root(*root);
        }
        result
    }

    fn wire_primitive_wrapper<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        output: &WireOutput,
        hooks: &mut D,
        wrapper: Value,
        inner: Value,
    ) -> Result<(), JsError> {
        if let Some(boolean) = inner.as_bool() {
            put_tag(
                output,
                if boolean {
                    tag::TRUE_OBJECT
                } else {
                    tag::FALSE_OBJECT
                },
            );
        } else if let Some(number) = inner.as_number() {
            put_tag(output, tag::NUMBER_OBJECT);
            put_double(output, number);
        } else {
            match self.heap.get(inner) {
                Some(Cell::BigInt(value)) => {
                    put_tag(output, tag::BIGINT_OBJECT);
                    put_bigint_contents(output, value);
                }
                _ if let Some(text) = self.heap.string(inner) => {
                    put_tag(output, tag::STRING_OBJECT);
                    put_string(output, text.units());
                }
                _ => return Err(self.uncloneable_with(program, hooks, wrapper)),
            }
        }
        Ok(())
    }

    /// Own enumerable string keys, with array indices as numbers (`GetKeys` with
    /// `kKeepNumbers`); `skip_indices` drops the indices a dense array writes itself.
    fn wire_enumerable_keys(
        &mut self,
        program: &ResidualProgram,
        object: Value,
        skip_indices: bool,
    ) -> Result<Vec<RootId>, JsError> {
        let object_root = self.heap.root(object);
        let keys = self.object_own_key_values(program, object);
        let mut selected = Vec::new();
        let result = keys.and_then(|keys| {
            let keys: Vec<RootId> = keys.into_iter().map(|key| self.heap.root(key)).collect();
            let mut outcome = Ok(());
            for key_root in &keys {
                let key = self.heap.root_value(*key_root).unwrap();
                let Some(name) = self.heap.string(key).cloned() else {
                    continue;
                };
                let atom = self.intern_js_atom(&name);
                if outcome.is_err()
                    || self.is_private_name(atom)
                    || (skip_indices
                        && super::object_static::array_index(&name.to_string()).is_some())
                {
                    continue;
                }
                let object = self.heap.root_value(object_root).unwrap();
                match self.ordinary_own_property(program, object, key) {
                    Ok(Some((_, attributes))) if attributes.enumerable => {
                        selected.push(self.heap.root(key))
                    }
                    Ok(_) => {}
                    Err(error) => outcome = Err(error),
                }
            }
            for key in keys {
                self.heap.release_root(key);
            }
            outcome
        });
        self.heap.release_root(object_root);
        match result {
            Ok(()) => Ok(selected),
            Err(error) => {
                self.release_wire_roots(&mut selected);
                Err(error)
            }
        }
    }

    /// `WriteJSObjectPropertiesSlow`: each key still present, then its value; the count
    /// written. Releases the key roots.
    fn wire_properties<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        object: Value,
        keys: &[RootId],
    ) -> Result<u32, JsError> {
        let object_root = self.heap.root(object);
        let mut written = 0;
        let mut result = Ok(());
        for key_root in keys {
            if result.is_ok() {
                result = (|| {
                    let object = self.heap.root_value(object_root).unwrap();
                    let key = self.heap.root_value(*key_root).unwrap();
                    if self.ordinary_own_property(program, object, key)?.is_none() {
                        return Ok(());
                    }
                    let value = self.get_index(program, object, key)?;
                    let value_root = self.heap.root(value);
                    let key = self.heap.root_value(*key_root).unwrap();
                    let outcome = self
                        .wire_property_key(program, serializer, hooks, key)
                        .and_then(|()| {
                            let value = self.heap.root_value(value_root).unwrap();
                            self.wire_write(program, serializer, hooks, value)
                        });
                    self.heap.release_root(value_root);
                    outcome?;
                    written += 1;
                    Ok(())
                })();
            }
            self.heap.release_root(*key_root);
        }
        self.heap.release_root(object_root);
        result.map(|()| written)
    }

    /// Array index keys are numbers on the wire.
    fn wire_property_key<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        key: Value,
    ) -> Result<(), JsError> {
        if let Some(name) = self.heap.string(key) {
            if let Some(index) = super::object_static::array_index(&name.to_string()) {
                put_number(&serializer.output, f64::from(index));
                return Ok(());
            }
        }
        self.wire_write(program, serializer, hooks, key)
    }

    /// `WriteJSArray`: arrays whose elements are all present data properties are dense,
    /// others sparse key/value lists.
    fn wire_array<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        array: Value,
    ) -> Result<(), JsError> {
        let output = serializer.output.clone();
        let length = self.own_array_length(array).unwrap_or_default();
        let dense = self.heap.sparse_length(array).is_none()
            && self.heap.array_elements(array).is_some_and(|elements| {
                elements.len() == length && elements.iter().all(|element| !element.is_deleted())
            })
            && (0..length).all(|index| self.array_descriptor(array, index).is_none());
        if !dense {
            put_tag(&output, tag::BEGIN_SPARSE_JS_ARRAY);
            put_varint(&output, length as u64);
            let keys = self.wire_enumerable_keys(program, array, false)?;
            let written = self.wire_properties(program, serializer, hooks, array, &keys)?;
            put_tag(&output, tag::END_SPARSE_JS_ARRAY);
            put_varint(&output, u64::from(written));
            put_varint(&output, length as u64);
            return Ok(());
        }
        put_tag(&output, tag::BEGIN_DENSE_JS_ARRAY);
        put_varint(&output, length as u64);
        let elements = self
            .heap
            .array_elements(array)
            .map(|elements| elements.to_vec())
            .unwrap_or_default();
        // A numeric array holding any non-Smi has double elements: all write as doubles.
        let doubles = elements.iter().all(|element| element.as_number().is_some())
            && elements
                .iter()
                .any(|element| element.as_number().and_then(smi).is_none());
        let array_root = self.heap.root(array);
        let mut result = Ok(());
        for index in 0..length {
            // Writing earlier elements can run getters that change this array.
            let array = self.heap.root_value(array_root).unwrap();
            let element = self
                .heap
                .array_elements(array)
                .and_then(|elements| elements.get(index).copied())
                .filter(|element| !element.is_deleted());
            result = match element {
                None if !self.has_own_array_index(array, index) => {
                    put_tag(&output, tag::THE_HOLE);
                    Ok(())
                }
                None => {
                    let key = self.heap.alloc_string(index.to_string().as_str().into());
                    self.get_index(program, array, key)
                        .and_then(|element| self.wire_write(program, serializer, hooks, element))
                }
                Some(element) if doubles && element.as_number().is_some() => {
                    put_tag(&output, tag::DOUBLE);
                    put_double(&output, element.as_number().unwrap());
                    Ok(())
                }
                Some(element) => self.wire_write(program, serializer, hooks, element),
            };
            if result.is_err() {
                break;
            }
        }
        let array = self.heap.root_value(array_root).unwrap();
        self.heap.release_root(array_root);
        result?;
        let keys = self.wire_enumerable_keys(program, array, true)?;
        let written = self.wire_properties(program, serializer, hooks, array, &keys)?;
        put_tag(&output, tag::END_DENSE_JS_ARRAY);
        put_varint(&output, u64::from(written));
        put_varint(&output, length as u64);
        Ok(())
    }

    fn wire_array_buffer<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        buffer: Value,
    ) -> Result<(), JsError> {
        let output = serializer.output.clone();
        let Some(Cell::ArrayBuffer {
            bytes,
            shared,
            detached,
            max_byte_length,
            resizable,
            immutable,
            ..
        }) = self.heap.get(buffer)
        else {
            unreachable!("dispatched as an ArrayBuffer");
        };
        let (shareable, shared, detached, length, max_byte_length, resizable, immutable) = (
            bytes.shared_backing().is_some(),
            *shared,
            *detached,
            bytes.len(),
            *max_byte_length,
            *resizable,
            *immutable,
        );
        if shared {
            if !shareable {
                return Err(self.uncloneable_with(program, hooks, buffer));
            }
            let id = hooks.shared_array_buffer_id(self, program, buffer)?;
            put_tag(&output, tag::SHARED_ARRAY_BUFFER);
            put_varint(&output, u64::from(id));
            return Ok(());
        }
        if let Some(id) = serializer.transfers.get(&buffer) {
            put_tag(&output, tag::ARRAY_BUFFER_TRANSFER);
            put_varint(&output, u64::from(*id));
            return Ok(());
        }
        if detached {
            return Err(hooks.data_clone_error(self, program, DETACHED_MESSAGE));
        }
        if u32::try_from(length).is_err() || (resizable && u32::try_from(max_byte_length).is_err())
        {
            return Err(self.uncloneable_with(program, hooks, buffer));
        }
        put_tag(
            &output,
            if resizable {
                tag::RESIZABLE_ARRAY_BUFFER
            } else if immutable {
                tag::IMMUTABLE_ARRAY_BUFFER
            } else {
                tag::ARRAY_BUFFER
            },
        );
        put_varint(&output, length as u64);
        if resizable {
            put_varint(&output, max_byte_length as u64);
        }
        if let Some(Cell::ArrayBuffer { bytes, .. }) = self.heap.get(buffer) {
            output.borrow_mut().extend_from_slice(&bytes.read());
        }
        Ok(())
    }

    /// `WriteJSArrayBufferView`; its buffer was written just before.
    fn wire_view<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        output: &WireOutput,
        hooks: &mut D,
        view: Value,
    ) -> Result<(), JsError> {
        let (subtag, offset, byte_length, length_tracking, buffer) = match self.heap.get(view) {
            Some(Cell::TypedArray {
                kind,
                object,
                buffer,
                length,
                length_tracking,
            }) => {
                let (kind, offset, length, length_tracking, buffer) =
                    (*kind, object.offset, *length, *length_tracking, *buffer);
                // Only views on live resizable buffers can be out of bounds (V8's
                // `IsOutOfBounds`).
                let out_of_bounds = !self.array_buffer_detached(buffer)
                    && self.array_buffer_resizable(buffer)
                    && !matches!(
                        self.heap.get(buffer),
                        Some(Cell::ArrayBuffer { shared: true, .. })
                    )
                    && self.typed_array_out_of_bounds(view);
                if out_of_bounds {
                    return Err(self.uncloneable_with(program, hooks, view));
                }
                let subtag = VIEW_TAGS
                    .iter()
                    .find(|(candidate, _)| *candidate == kind)
                    .map(|(_, tag)| *tag)
                    .unwrap();
                // Length-tracking views carry no fixed length.
                let byte_length = if length_tracking {
                    0
                } else {
                    length * kind.width()
                };
                (subtag, offset, byte_length, length_tracking, buffer)
            }
            Some(Cell::DataView {
                object,
                buffer,
                length,
                length_tracking,
            }) => {
                let (offset, length, length_tracking, buffer) =
                    (object.offset, *length, *length_tracking, *buffer);
                let required = if length_tracking { 0 } else { length };
                if self.array_buffer_resizable(buffer)
                    && self.array_buffer_out_of_bounds(buffer, offset, required)
                {
                    return Err(self.uncloneable_with(program, hooks, view));
                }
                (DATA_VIEW_TAG, offset, required, length_tracking, buffer)
            }
            _ => unreachable!("dispatched as a view"),
        };
        let backed_by_resizable = self.array_buffer_resizable(buffer)
            && !matches!(
                self.heap.get(buffer),
                Some(Cell::ArrayBuffer { shared: true, .. })
            );
        put_tag(output, tag::ARRAY_BUFFER_VIEW);
        put_varint(output, u64::from(subtag));
        put_varint(output, offset as u64);
        put_varint(output, byte_length as u64);
        let flags = (if length_tracking {
            VIEW_LENGTH_TRACKING
        } else {
            0
        }) | (if backed_by_resizable {
            VIEW_BACKED_BY_RESIZABLE
        } else {
            0
        });
        put_varint(output, u64::from(flags));
        Ok(())
    }

    /// `WriteJSError`: prototype, message and stack, then a cause that may refer back.
    fn wire_error<D: WriteHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        serializer: &mut ValueSerializer,
        hooks: &mut D,
        error: Value,
    ) -> Result<(), JsError> {
        let output = serializer.output.clone();
        let error_root = self.heap.root(error);
        let mut fields = [None, None];
        for (index, name) in ["message", "cause"].into_iter().enumerate() {
            let key = self.heap.alloc_string(name.into());
            let error = self.heap.root_value(error_root).unwrap();
            if let Some((value, attributes)) = self.ordinary_own_property(program, error, key)?
                && !attributes.accessor
            {
                fields[index] = Some(self.heap.root(value));
            }
        }
        let result = (|| {
            put_tag(&output, tag::ERROR);
            let name_atom = self.intern_atom("name");
            let error = self.heap.root_value(error_root).unwrap();
            let name = self.get_property(program, error, name_atom)?;
            let name = self.to_string(program, name)?;
            if let Some((_, _, prototype)) = ERROR_PROTOTYPES
                .iter()
                .find(|(candidate, _, _)| *candidate == name)
            {
                put_varint(&output, u64::from(*prototype));
            }
            if let Some(message) = fields[0] {
                let message = self.heap.root_value(message).unwrap();
                let message = self.coerce_js_string(program, message)?;
                put_varint(&output, u64::from(error_tag::MESSAGE));
                put_string(&output, message.units());
            }
            let stack_atom = self.intern_atom("stack");
            let error = self.heap.root_value(error_root).unwrap();
            let stack = self.get_property(program, error, stack_atom)?;
            if let Some(stack) = self.heap.string(stack).cloned() {
                put_varint(&output, u64::from(error_tag::STACK));
                put_string(&output, stack.units());
            }
            if let Some(cause) = fields[1] {
                put_varint(&output, u64::from(error_tag::CAUSE));
                let cause = self.heap.root_value(cause).unwrap();
                self.wire_write(program, serializer, hooks, cause)?;
            }
            put_varint(&output, u64::from(error_tag::END));
            Ok(())
        })();
        for root in fields.into_iter().flatten().chain([error_root]) {
            self.heap.release_root(root);
        }
        result
    }

    /// `ReadHeader`, rejecting the header-less and pre-13 legacy formats.
    pub(crate) fn wire_read_header(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
    ) -> Result<(), JsError> {
        let mut input = deserializer.input.borrow_mut();
        let version = if input.bytes.get(input.position) == Some(&tag::VERSION) {
            input.position += 1;
            input.read_uint32()
        } else {
            Some(0)
        };
        match version {
            Some(version)
                if (MINIMUM_NON_LEGACY_VERSION..=WIRE_FORMAT_VERSION).contains(&version) =>
            {
                input.version = version;
                Ok(())
            }
            _ => {
                drop(input);
                Err(self.plain_error(program, VERSION_ERROR))
            }
        }
    }

    fn plain_error(&mut self, program: &ResidualProgram, message: &str) -> JsError {
        let text = self.heap.alloc_string(message.into());
        match self.construct_error_native(program, Native::Error, &[text]) {
            Ok(error) => JsError::thrown(error, message.into()),
            Err(error) => error,
        }
    }

    /// `ReadValue`: one value, or "Unable to deserialize cloned data." for bad input.
    pub(crate) fn wire_read_value<D: ReadHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
        hooks: &mut D,
    ) -> Result<Value, JsError> {
        match self.wire_read(program, deserializer, hooks) {
            Ok(value) => Ok(value),
            Err(ReadFailure::Thrown(error)) => Err(error),
            Err(ReadFailure::Invalid) => Err(self.plain_error(program, DESERIALIZATION_ERROR)),
        }
    }

    /// `ReadObject`: a value; an ArrayBuffer followed by a view tag yields the view.
    fn wire_read<D: ReadHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
        hooks: &mut D,
    ) -> Read<Value> {
        let _stack = self.enter_stack()?;
        let value = self.wire_read_internal(program, deserializer, hooks)?;
        let view_follows = matches!(self.heap.get(value), Some(Cell::ArrayBuffer { .. }))
            && deserializer.input.borrow().peek_tag() == Some(tag::ARRAY_BUFFER_VIEW);
        if view_follows {
            deserializer.input.borrow_mut().read_tag();
            return self.wire_read_view(deserializer, value);
        }
        Ok(value)
    }

    fn wire_add_object(
        &mut self,
        deserializer: &mut ValueDeserializer,
        id: u32,
        value: Value,
    ) -> RootId {
        let root = self.heap.root(value);
        deserializer.roots.push(root);
        let index = id as usize;
        if deserializer.objects.len() <= index {
            deserializer.objects.resize(index + 1, None);
        }
        deserializer.objects[index] = Some(root);
        root
    }

    fn wire_next_id(deserializer: &ValueDeserializer) -> u32 {
        deserializer.objects.len() as u32
    }

    /// Reserve the next id before reading contents that may refer to it.
    fn wire_reserve_id(deserializer: &mut ValueDeserializer) -> u32 {
        let id = Self::wire_next_id(deserializer);
        deserializer.objects.push(None);
        id
    }

    fn wire_read_string(&mut self, deserializer: &mut ValueDeserializer, tag: u8) -> Read<Value> {
        let mut input = deserializer.input.borrow_mut();
        let length = valid(input.read_uint32())? as usize;
        let bytes = valid(input.raw(length))?;
        let text = match tag {
            tag::UTF8_STRING => JsString::from_str(&String::from_utf8_lossy(bytes)),
            tag::ONE_BYTE_STRING => {
                let units: Vec<u16> = bytes.iter().map(|byte| u16::from(*byte)).collect();
                JsString::from_units(&units)
            }
            _ => {
                if length % std::mem::size_of::<u16>() != 0 {
                    return Err(ReadFailure::Invalid);
                }
                let units: Vec<u16> = bytes
                    .chunks_exact(2)
                    .map(|pair| u16::from_ne_bytes([pair[0], pair[1]]))
                    .collect();
                JsString::from_units(&units)
            }
        };
        drop(input);
        Ok(self.heap.alloc_string(text))
    }

    /// `ReadString`: a string record.
    fn wire_read_tagged_string<D: ReadHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
        hooks: &mut D,
    ) -> Read<JsString> {
        let value = self.wire_read(program, deserializer, hooks)?;
        self.heap.string(value).cloned().ok_or(ReadFailure::Invalid)
    }

    fn wire_read_bigint(&mut self, deserializer: &mut ValueDeserializer) -> Read<Value> {
        let mut input = deserializer.input.borrow_mut();
        let bitfield = valid(input.read_uint32())?;
        let length = (bitfield >> BIGINT_LENGTH_SHIFT) as usize;
        let digits = valid(input.raw(length))?;
        let magnitude = num_bigint::BigUint::from_bytes_le(digits);
        let sign = if bitfield & BIGINT_SIGN_BIT != 0 {
            num_bigint::Sign::Minus
        } else {
            num_bigint::Sign::Plus
        };
        let value = num_bigint::BigInt::from_biguint(sign, magnitude);
        drop(input);
        Ok(self.heap.alloc(Cell::bigint(value)))
    }

    fn wire_read_internal<D: ReadHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
        hooks: &mut D,
    ) -> Read<Value> {
        let tag = valid(deserializer.input.borrow_mut().read_tag())?;
        match tag {
            tag::VERIFY_OBJECT_COUNT => {
                valid(deserializer.input.borrow_mut().read_uint32())?;
                self.wire_read(program, deserializer, hooks)
            }
            tag::UNDEFINED => Ok(Value::UNDEFINED),
            tag::NULL => Ok(Value::NULL),
            tag::TRUE => Ok(Value::TRUE),
            tag::FALSE => Ok(Value::FALSE),
            tag::INT32 => Ok(Value::number(f64::from(valid(
                deserializer.input.borrow_mut().read_zigzag(),
            )?))),
            tag::UINT32 => Ok(Value::number(f64::from(valid(
                deserializer.input.borrow_mut().read_uint32(),
            )?))),
            tag::DOUBLE => Ok(Value::number(valid(
                deserializer.input.borrow_mut().read_double(),
            )?)),
            tag::BIGINT => self.wire_read_bigint(deserializer),
            tag::UTF8_STRING | tag::ONE_BYTE_STRING | tag::TWO_BYTE_STRING => {
                self.wire_read_string(deserializer, tag)
            }
            tag::OBJECT_REFERENCE => {
                let id = valid(deserializer.input.borrow_mut().read_uint32())? as usize;
                let root = valid(deserializer.objects.get(id).copied().flatten())?;
                Ok(self.heap.root_value(root).unwrap())
            }
            tag::BEGIN_JS_OBJECT => {
                let id = Self::wire_reserve_id(deserializer);
                let object = self
                    .heap
                    .alloc(Cell::Object(Self::empty_object(self.object_proto)));
                let root = self.wire_add_object(deserializer, id, object);
                let count = self.wire_read_properties(
                    program,
                    deserializer,
                    hooks,
                    root,
                    tag::END_JS_OBJECT,
                )?;
                let expected = valid(deserializer.input.borrow_mut().read_uint32())?;
                if count != expected {
                    return Err(ReadFailure::Invalid);
                }
                Ok(self.heap.root_value(root).unwrap())
            }
            tag::BEGIN_SPARSE_JS_ARRAY => {
                let length = valid(deserializer.input.borrow_mut().read_uint32())?;
                let id = Self::wire_reserve_id(deserializer);
                let array = self.new_wire_array();
                let root = self.wire_add_object(deserializer, id, array);
                self.set_array_length(program, array, Value::number(f64::from(length)))?;
                let count = self.wire_read_properties(
                    program,
                    deserializer,
                    hooks,
                    root,
                    tag::END_SPARSE_JS_ARRAY,
                )?;
                self.wire_read_array_trailer(deserializer, count, length)?;
                Ok(self.heap.root_value(root).unwrap())
            }
            tag::BEGIN_DENSE_JS_ARRAY => {
                let length = valid(deserializer.input.borrow_mut().read_uint32())?;
                if length as usize > deserializer.input.borrow().remaining() {
                    return Err(ReadFailure::Invalid);
                }
                let id = Self::wire_reserve_id(deserializer);
                let array = self.new_wire_array();
                let root = self.wire_add_object(deserializer, id, array);
                self.set_array_length(program, array, Value::number(f64::from(length)))?;
                for index in 0..length {
                    if deserializer.input.borrow().peek_tag() == Some(tag::THE_HOLE) {
                        deserializer.input.borrow_mut().read_tag();
                        continue;
                    }
                    let element = self.wire_read(program, deserializer, hooks)?;
                    let key = self.heap.alloc_string(index.to_string().as_str().into());
                    let array = self.heap.root_value(root).unwrap();
                    if !self.define_own_property_record(
                        program,
                        array,
                        key,
                        PropertyDescriptorRecord::data(element),
                    )? {
                        return Err(ReadFailure::Invalid);
                    }
                }
                let count = self.wire_read_properties(
                    program,
                    deserializer,
                    hooks,
                    root,
                    tag::END_DENSE_JS_ARRAY,
                )?;
                self.wire_read_array_trailer(deserializer, count, length)?;
                Ok(self.heap.root_value(root).unwrap())
            }
            tag::DATE => {
                let milliseconds = valid(deserializer.input.borrow_mut().read_double())?;
                let id = Self::wire_reserve_id(deserializer);
                let prototype = self.intrinsic_builtin_prototype(Native::Date);
                let date = self.heap.alloc(Cell::Date {
                    milliseconds,
                    object: Box::new(Self::empty_object(prototype)),
                });
                self.wire_add_object(deserializer, id, date);
                Ok(date)
            }
            tag::TRUE_OBJECT
            | tag::FALSE_OBJECT
            | tag::NUMBER_OBJECT
            | tag::BIGINT_OBJECT
            | tag::STRING_OBJECT => {
                let id = Self::wire_reserve_id(deserializer);
                let inner = match tag {
                    tag::TRUE_OBJECT => Value::TRUE,
                    tag::FALSE_OBJECT => Value::FALSE,
                    tag::NUMBER_OBJECT => {
                        Value::number(valid(deserializer.input.borrow_mut().read_double())?)
                    }
                    tag::BIGINT_OBJECT => self.wire_read_bigint(deserializer)?,
                    _ => {
                        let text = self.wire_read_tagged_string(program, deserializer, hooks)?;
                        self.heap.alloc_string(text)
                    }
                };
                let wrapper = self.box_primitive_object(inner)?;
                self.wire_add_object(deserializer, id, wrapper);
                Ok(wrapper)
            }
            tag::REGEXP => {
                let id = Self::wire_reserve_id(deserializer);
                let source = self.wire_read_tagged_string(program, deserializer, hooks)?;
                let bits = valid(deserializer.input.borrow_mut().read_uint32())?;
                let known = REGEXP_FLAG_BITS
                    .iter()
                    .fold(0u32, |mask, (_, bit)| mask | (1 << bit));
                if bits & !known != 0 || bits & (1 << REGEXP_LINEAR_BIT) != 0 {
                    return Err(ReadFailure::Invalid);
                }
                let mut flags: Vec<char> = REGEXP_FLAG_BITS
                    .iter()
                    .filter(|(_, bit)| bits & (1 << bit) != 0)
                    .map(|(flag, _)| *flag)
                    .collect();
                flags.sort_unstable();
                let intrinsics = &self.realm.intrinsics.regexp_intrinsics[&self.realm.globals];
                let (prototype, constructor) = (intrinsics.prototype, intrinsics.constructor);
                let regexp = self.regexp_from_source(
                    prototype,
                    source,
                    flags.into_iter().collect(),
                    crate::heap::RegExpLegacyOwner::Enabled(constructor),
                )?;
                self.wire_add_object(deserializer, id, regexp);
                Ok(regexp)
            }
            tag::BEGIN_JS_MAP | tag::BEGIN_JS_SET => {
                let map = tag == tag::BEGIN_JS_MAP;
                let id = Self::wire_reserve_id(deserializer);
                let collection = if map {
                    let prototype = self.intrinsic_builtin_prototype(Native::Map);
                    self.heap.alloc(Cell::Map {
                        object: Box::new(Self::empty_object(prototype)),
                        entries: Box::default(),
                    })
                } else {
                    let prototype = self.intrinsic_builtin_prototype(Native::Set);
                    self.heap.alloc(Cell::Set {
                        object: Box::new(Self::empty_object(prototype)),
                        entries: Box::default(),
                    })
                };
                let root = self.wire_add_object(deserializer, id, collection);
                let end = if map {
                    tag::END_JS_MAP
                } else {
                    tag::END_JS_SET
                };
                let mut length = 0u32;
                loop {
                    if valid(deserializer.input.borrow().peek_tag())? == end {
                        deserializer.input.borrow_mut().read_tag();
                        break;
                    }
                    if map {
                        let key = self.wire_read(program, deserializer, hooks)?;
                        let key = self.heap.root(key);
                        let value = self.wire_read(program, deserializer, hooks);
                        let key_value = self.heap.root_value(key).unwrap();
                        self.heap.release_root(key);
                        let value = value?;
                        let collection = self.heap.root_value(root).unwrap();
                        self.call_collection_native(
                            program,
                            Native::MapSet,
                            collection,
                            &[key_value, value],
                        )?;
                        length += 2;
                    } else {
                        let value = self.wire_read(program, deserializer, hooks)?;
                        let collection = self.heap.root_value(root).unwrap();
                        self.call_collection_native(program, Native::SetAdd, collection, &[value])?;
                        length += 1;
                    }
                }
                if valid(deserializer.input.borrow_mut().read_uint32())? != length {
                    return Err(ReadFailure::Invalid);
                }
                Ok(self.heap.root_value(root).unwrap())
            }
            tag::ARRAY_BUFFER | tag::RESIZABLE_ARRAY_BUFFER | tag::IMMUTABLE_ARRAY_BUFFER => {
                let id = Self::wire_reserve_id(deserializer);
                let mut input = deserializer.input.borrow_mut();
                let byte_length = valid(input.read_uint32())? as usize;
                let resizable = tag == tag::RESIZABLE_ARRAY_BUFFER;
                let max_byte_length = if resizable {
                    valid(input.read_uint32())? as usize
                } else {
                    byte_length
                };
                if byte_length > max_byte_length {
                    return Err(ReadFailure::Invalid);
                }
                let bytes = valid(input.raw(byte_length))?.to_vec();
                drop(input);
                let buffer = self.new_array_buffer_cell(
                    bytes.into(),
                    false,
                    max_byte_length,
                    resizable,
                    tag == tag::IMMUTABLE_ARRAY_BUFFER,
                );
                self.wire_add_object(deserializer, id, buffer);
                Ok(buffer)
            }
            tag::ARRAY_BUFFER_TRANSFER => {
                let id = Self::wire_reserve_id(deserializer);
                let transfer = valid(deserializer.input.borrow_mut().read_uint32())?;
                let root = valid(deserializer.transfers.get(&transfer).copied())?;
                let buffer = self.heap.root_value(root).unwrap();
                self.wire_add_object(deserializer, id, buffer);
                Ok(buffer)
            }
            tag::SHARED_ARRAY_BUFFER => {
                let id = Self::wire_reserve_id(deserializer);
                let clone_id = valid(deserializer.input.borrow_mut().read_uint32())?;
                let buffer = hooks.shared_array_buffer(self, program, clone_id)?;
                if !matches!(
                    self.heap.get(buffer),
                    Some(Cell::ArrayBuffer { shared: true, .. })
                ) {
                    return Err(ReadFailure::Invalid);
                }
                self.wire_add_object(deserializer, id, buffer);
                Ok(buffer)
            }
            tag::ERROR => self.wire_read_error(program, deserializer, hooks),
            tag::HOST_OBJECT => self.wire_read_host_object(program, deserializer, hooks),
            _ => Err(ReadFailure::Invalid),
        }
    }

    fn wire_read_host_object<D: ReadHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
        hooks: &mut D,
    ) -> Read<Value> {
        let id = Self::wire_reserve_id(deserializer);
        let input = deserializer.input.clone();
        let object = match hooks.read_host_object(self, program, &input)? {
            HostObjectRead::Object(object) => object,
            HostObjectRead::NextValue => self.wire_read(program, deserializer, hooks)?,
        };
        if !self.is_object_like(object) {
            return Err(ReadFailure::Invalid);
        }
        self.wire_add_object(deserializer, id, object);
        Ok(object)
    }

    fn new_wire_array(&mut self) -> Value {
        let prototype = self.array_prototype_for_realm(self.realm.globals);
        self.heap.alloc(Cell::Array {
            object: crate::heap::ArrayObject::new(Self::empty_object(prototype), Vec::new()),
        })
    }

    fn wire_read_array_trailer(
        &mut self,
        deserializer: &mut ValueDeserializer,
        count: u32,
        length: u32,
    ) -> Read<()> {
        let mut input = deserializer.input.borrow_mut();
        let expected_count = valid(input.read_uint32())?;
        let expected_length = valid(input.read_uint32())?;
        if count != expected_count || length != expected_length {
            return Err(ReadFailure::Invalid);
        }
        Ok(())
    }

    /// `ReadJSObjectProperties`: key/value pairs until `end`; keys are strings or numbers,
    /// each defined once.
    fn wire_read_properties<D: ReadHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
        hooks: &mut D,
        target: RootId,
        end: u8,
    ) -> Read<u32> {
        let mut count = 0;
        loop {
            if valid(deserializer.input.borrow().peek_tag())? == end {
                deserializer.input.borrow_mut().read_tag();
                return Ok(count);
            }
            let key = self.wire_read(program, deserializer, hooks)?;
            if key.as_number().is_none() && !self.heap.is_string(key) {
                return Err(ReadFailure::Invalid);
            }
            let key = self.to_property_key(program, key)?;
            let key = self.heap.root(key);
            let value = self.wire_read(program, deserializer, hooks);
            let key_value = self.heap.root_value(key).unwrap();
            self.heap.release_root(key);
            let value = value?;
            let object = self.heap.root_value(target).unwrap();
            if self
                .ordinary_own_property(program, object, key_value)?
                .is_some()
                || !self.define_own_property_record(
                    program,
                    object,
                    key_value,
                    PropertyDescriptorRecord::data(value),
                )?
            {
                return Err(ReadFailure::Invalid);
            }
            count += 1;
        }
    }

    /// `ReadJSArrayBufferView` over the buffer just read.
    fn wire_read_view(
        &mut self,
        deserializer: &mut ValueDeserializer,
        buffer: Value,
    ) -> Read<Value> {
        let Some(Cell::ArrayBuffer {
            bytes,
            shared,
            resizable,
            ..
        }) = self.heap.get(buffer)
        else {
            return Err(ReadFailure::Invalid);
        };
        let (buffer_length, shared, resizable) = (bytes.len(), *shared, *resizable);
        let mut input = deserializer.input.borrow_mut();
        let subtag = valid(input.read_byte_varint())?;
        let offset = valid(input.read_uint32())? as usize;
        let byte_length = valid(input.read_uint32())? as usize;
        if offset > buffer_length || byte_length > buffer_length - offset {
            return Err(ReadFailure::Invalid);
        }
        let flags = if input.version >= VERSION_VIEW_FLAGS {
            valid(input.read_uint32())?
        } else {
            0
        };
        drop(input);
        let id = Self::wire_reserve_id(deserializer);
        let length_tracking = flags & VIEW_LENGTH_TRACKING != 0;
        let backed_by_resizable = flags & VIEW_BACKED_BY_RESIZABLE != 0;
        if ((backed_by_resizable || length_tracking) && !resizable)
            || (backed_by_resizable && shared)
            || (resizable && !shared && !backed_by_resizable)
        {
            return Err(ReadFailure::Invalid);
        }
        let view = if subtag == DATA_VIEW_TAG {
            let prototype = self.intrinsic_builtin_prototype(Native::DataView);
            self.heap.alloc(Cell::DataView {
                object: Box::new(crate::heap::BufferViewObject {
                    object: Self::empty_object(prototype),
                    offset,
                }),
                buffer,
                length: byte_length,
                length_tracking,
            })
        } else {
            let kind = valid(
                VIEW_TAGS
                    .iter()
                    .find(|(_, tag)| *tag == subtag)
                    .map(|(kind, _)| *kind),
            )?;
            if offset % kind.width() != 0 || byte_length % kind.width() != 0 {
                return Err(ReadFailure::Invalid);
            }
            let prototype = self.typed_array_proto(kind);
            self.heap.alloc(Cell::TypedArray {
                kind,
                object: Box::new(crate::heap::BufferViewObject {
                    object: Self::empty_object(prototype),
                    offset,
                }),
                buffer,
                length: byte_length / kind.width(),
                length_tracking,
            })
        };
        self.wire_add_object(deserializer, id, view);
        Ok(view)
    }

    /// `ReadJSError`.
    fn wire_read_error<D: ReadHooks<H> + ?Sized>(
        &mut self,
        program: &ResidualProgram,
        deserializer: &mut ValueDeserializer,
        hooks: &mut D,
    ) -> Read<Value> {
        let id = Self::wire_reserve_id(deserializer);
        let next = |deserializer: &mut ValueDeserializer| {
            valid(deserializer.input.borrow_mut().read_byte_varint())
        };
        let mut tag = next(deserializer)?;
        let mut native = Native::Error;
        if let Some((_, constructor, _)) = ERROR_PROTOTYPES
            .iter()
            .find(|(_, _, prototype)| *prototype == tag)
        {
            native = *constructor;
            tag = next(deserializer)?;
        }
        let mut message = None;
        if tag == error_tag::MESSAGE {
            message = Some(self.wire_read_tagged_string(program, deserializer, hooks)?);
            tag = next(deserializer)?;
        }
        let mut stack = Value::UNDEFINED;
        if tag == error_tag::STACK {
            let text = self.wire_read_tagged_string(program, deserializer, hooks)?;
            stack = self.heap.alloc_string(text);
            tag = next(deserializer)?;
        }
        let prototype = self.intrinsic_builtin_prototype(native);
        let error = self.heap.alloc(Cell::Object(Object::error(prototype)));
        let root = self.wire_add_object(deserializer, id, error);
        self.install_error_stack(program, error, StackData::Formatted(stack))?;
        if let Some(message) = message {
            let message = self.heap.alloc_string(message);
            let error = self.heap.root_value(root).unwrap();
            self.set_builtin_value_named(error, "message", message)?;
        }
        if tag == error_tag::CAUSE {
            let cause = self.wire_read(program, deserializer, hooks)?;
            let error = self.heap.root_value(root).unwrap();
            self.set_builtin_value_named(error, "cause", cause)?;
            tag = next(deserializer)?;
        }
        if tag != error_tag::END {
            return Err(ReadFailure::Invalid);
        }
        Ok(self.heap.root_value(root).unwrap())
    }

    pub(super) fn new_array_buffer_cell(
        &mut self,
        bytes: crate::heap::BufferBytes,
        shared: bool,
        max_byte_length: usize,
        resizable: bool,
        immutable: bool,
    ) -> Value {
        let native = if shared {
            Native::SharedArrayBuffer
        } else {
            Native::ArrayBuffer
        };
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

    fn structured_clone_transfers(
        &mut self,
        program: &ResidualProgram,
        options: Option<RootId>,
        roots: &mut Vec<RootId>,
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
        let transfer_root = self.heap.root(transfer);
        roots.push(transfer_root);
        let transfer = self.heap.root_value(transfer_root).unwrap();
        let transfer_roots = self.iterable_to_rooted_list(program, transfer)?;
        roots.extend(transfer_roots.iter().copied());
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
                Some(Cell::ArrayBuffer { bytes, shared: false, detached: false, immutable: false, .. }) if !bytes.is_wasm_view()
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

    /// How V8 shows a value in an error without running guest code
    /// (`Object::NoSideEffectsToString`).
    pub(crate) fn clone_description(&mut self, value: Value) -> String {
        /// Longer function sources and symbol descriptions show both ends.
        const SHOWN_LIMIT: usize = 128;
        const SOURCE_HEAD: usize = 111;
        const SOURCE_TAIL: usize = 2;
        const DESCRIPTION_END: usize = 56;
        const OMITTED: &str = "...<omitted>...";
        fn shorten(text: String, head: usize, tail: usize) -> String {
            let units: Vec<u16> = text.encode_utf16().collect();
            if units.len() <= SHOWN_LIMIT {
                return text;
            }
            let start = String::from_utf16_lossy(&units[..head]);
            let end = String::from_utf16_lossy(&units[units.len() - tail..]);
            format!("{start}{OMITTED}{end}")
        }
        let mut value = value;
        while let Some((target, _)) = self.proxy_parts(value) {
            value = target;
        }
        if let Some(Cell::Symbol(description)) = self.heap.get(value) {
            let description = description
                .as_deref()
                .map(|text| shorten(text.to_string(), DESCRIPTION_END, DESCRIPTION_END));
            return format!("Symbol({})", description.unwrap_or_default());
        }
        if self.is_function(value) {
            let source = self
                .function_source_value(value)
                .and_then(|source| self.heap.string(source).map(ToString::to_string));
            let source = source.unwrap_or_else(|| {
                let name = self
                    .debug_function_name(value)
                    .map(|name| name.to_string())
                    .unwrap_or_default();
                format!("function {name}() {{ [native code] }}")
            });
            return shorten(source, SOURCE_HEAD, SOURCE_TAIL);
        }
        let native_function = |vm: &Self, function: Option<Value>, native: Native| {
            function.is_some_and(|function| {
                matches!(vm.heap.function(function), Some(FunctionRecord { kind: FunctionKind::Native(found), .. }) if *found == native)
            })
        };
        let to_string = self.data_property(value, "toString");
        if self.object_data(value).is_some_and(Object::has_error_data)
            || native_function(self, to_string, Native::ErrorToString)
        {
            let text = |vm: &mut Self, name: &str| {
                vm.data_property(value, name)
                    .and_then(|value| vm.heap.string(value).map(ToString::to_string))
            };
            let name = text(self, "name").unwrap_or_else(|| "Error".to_owned());
            let message = text(self, "message").unwrap_or_default();
            return match (name.is_empty(), message.is_empty()) {
                (true, _) => message,
                (_, true) => name,
                _ => format!("{name}: {message}"),
            };
        }
        if native_function(self, to_string, Native::ObjectPrototypeToString) {
            let constructor = self
                .data_property(value, "constructor")
                .filter(|constructor| self.is_function(*constructor));
            let name = constructor.and_then(|constructor| {
                self.original_debug_name(constructor)
                    .or_else(|| self.debug_function_name(constructor))
            });
            if let Some(name) = name {
                if !name.units().is_empty() {
                    return format!("#<{name}>");
                }
            }
        }
        let tag = self
            .well_known_symbols
            .get(WellKnownSymbol::ToStringTag)
            .and_then(|symbol| self.diagnostic_data_property(value, PropertyKey::symbol(symbol)))
            .and_then(|tag| self.heap.string(tag).map(ToString::to_string));
        format!(
            "[object {}]",
            tag.unwrap_or_else(|| self.class_name(value).to_owned())
        )
    }

    /// `JSReceiver::GetDataProperty`: the first property along the prototype chain when
    /// it holds data; accessors and proxies answer nothing.
    fn data_property(&mut self, object: Value, name: &str) -> Option<Value> {
        let atom = self.intern_atom(name);
        self.diagnostic_data_property(object, PropertyKey::string(atom))
    }

    /// `JSReceiver::class_name`: the receiver's built-in kind. Typed arrays on resizable
    /// buffers have no per-type name.
    fn class_name(&self, value: Value) -> &'static str {
        if self
            .object_data(value)
            .is_some_and(Object::is_arguments_object)
        {
            return "Arguments";
        }
        if let Some((brand, _)) = self.boxed_primitive_data(value) {
            return match brand {
                crate::BuiltinBrand::BooleanObject => "Boolean",
                crate::BuiltinBrand::NumberObject => "Number",
                crate::BuiltinBrand::StringObject => "String",
                crate::BuiltinBrand::BigIntObject => "BigInt",
                crate::BuiltinBrand::SymbolObject => "Symbol",
                _ => "Object",
            };
        }
        if self.object_data(value).is_some_and(Object::has_error_data) {
            return "Error";
        }
        if self.is_promise(value) {
            return "Promise";
        }
        if self.heap.array(value).is_some() {
            return "Array";
        }
        match self.heap.get(value) {
            Some(Cell::TypedArray { kind, buffer, .. }) => {
                if self.array_buffer_resizable(*buffer) {
                    return "Object";
                }
                match kind {
                    TypedArrayKind::Int8 => "Int8Array",
                    TypedArrayKind::Uint8 => "Uint8Array",
                    TypedArrayKind::Uint8Clamped => "Uint8ClampedArray",
                    TypedArrayKind::Int16 => "Int16Array",
                    TypedArrayKind::Uint16 => "Uint16Array",
                    TypedArrayKind::Int32 => "Int32Array",
                    TypedArrayKind::Uint32 => "Uint32Array",
                    TypedArrayKind::Float16 => "Float16Array",
                    TypedArrayKind::Float32 => "Float32Array",
                    TypedArrayKind::Float64 => "Float64Array",
                    TypedArrayKind::BigInt64 => "BigInt64Array",
                    TypedArrayKind::BigUint64 => "BigUint64Array",
                }
            }
            Some(Cell::DataView { .. }) => "DataView",
            Some(Cell::ArrayBuffer { shared: true, .. }) => "SharedArrayBuffer",
            Some(Cell::ArrayBuffer { .. }) => "ArrayBuffer",
            Some(Cell::Date { .. }) => "Date",
            Some(Cell::RegExp { .. }) => "RegExp",
            Some(Cell::Map { .. }) => "Map",
            Some(Cell::Set { .. }) => "Set",
            Some(Cell::WeakMap { .. }) => "WeakMap",
            Some(Cell::WeakSet { .. }) => "WeakSet",
            _ if self.is_function(value) => "Function",
            _ => "Object",
        }
    }

    /// The structured-clone DataCloneError: an Error named DataCloneError that hosts
    /// present as their DOMException.
    pub(super) fn data_clone_error(&mut self, program: &ResidualProgram, message: &str) -> JsError {
        let message_value = self.heap.alloc_string(message.into());
        let error = self
            .construct_error_native(program, Native::Error, &[message_value])
            .unwrap_or_else(|_| {
                self.heap
                    .alloc(Cell::Object(Object::error(self.object_proto)))
            });
        let name = self.heap.alloc_string("DataCloneError".into());
        let _ = self.set_builtin_value_named(error, "name", name);
        JsError::data_clone(error, message)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn bytes(write: impl FnOnce(&WireOutput)) -> Vec<u8> {
        let output = WireOutput::default();
        write(&output);
        output.take()
    }

    #[test]
    fn numbers_match_v8_encodings() {
        assert_eq!(
            bytes(|out| put_number(out, f64::from(1 << 30))),
            [b'I', 0x80, 0x80, 0x80, 0x80, 0x08]
        );
        assert_eq!(bytes(|out| put_number(out, -1.0)), [b'I', 0x01]);
        assert_eq!(bytes(|out| put_number(out, 2f64.powi(31)))[0], b'N');
        assert_eq!(bytes(|out| put_number(out, -0.0))[0], b'N');
    }

    #[test]
    fn bigints_carry_sign_and_byte_length() {
        assert_eq!(bytes(|out| put_bigint_contents(out, &0.into())), [0]);
        assert_eq!(
            bytes(|out| put_bigint_contents(out, &(-1).into())),
            [0x11, 1, 0, 0, 0, 0, 0, 0, 0]
        );
    }

    #[test]
    fn varints_round_trip_and_truncate_like_v8() {
        let encoded = bytes(|out| write_uint64(out, u64::MAX));
        let mut input = WireInput::new(encoded.clone());
        assert_eq!(input.read_uint64(), Some(u64::MAX));
        let mut input = WireInput::new(encoded);
        assert_eq!(input.read_uint32(), Some(u32::MAX));
    }
}

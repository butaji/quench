//! `ReadableState` and `WritableState`: the bit layouts of a stream's `kState` field, the
//! accessors projecting it, and the high water mark policy.

use super::errors::{self, StreamError};
use super::{
    arg, call_method, define_accessor, define_methods, get, get_slot, intrinsics, is_nullish, is_undefined, put,
    put_number, put_slot, Class, Context, Key, Outcome, Slot,
};
use quench_runtime::{RootId, RootedError};

/// Bits both states share (`internal/streams/utils`).
pub(crate) const OBJECT_MODE: u32 = 1 << 0;
pub(crate) const ERROR_EMITTED: u32 = 1 << 1;
pub(crate) const AUTO_DESTROY: u32 = 1 << 2;
pub(crate) const EMIT_CLOSE: u32 = 1 << 3;
pub(crate) const DESTROYED: u32 = 1 << 4;
pub(crate) const CLOSED: u32 = 1 << 5;
pub(crate) const CLOSE_EMITTED: u32 = 1 << 6;
pub(crate) const ERRORED: u32 = 1 << 7;
pub(crate) const CONSTRUCTED: u32 = 1 << 8;

/// `ReadableState`'s own bits.
pub(crate) mod readable {
    pub(crate) const ENDED: u32 = 1 << 9;
    pub(crate) const END_EMITTED: u32 = 1 << 10;
    pub(crate) const READING: u32 = 1 << 11;
    pub(crate) const SYNC: u32 = 1 << 12;
    pub(crate) const NEED_READABLE: u32 = 1 << 13;
    pub(crate) const EMITTED_READABLE: u32 = 1 << 14;
    pub(crate) const READABLE_LISTENING: u32 = 1 << 15;
    pub(crate) const RESUME_SCHEDULED: u32 = 1 << 16;
    pub(crate) const MULTI_AWAIT_DRAIN: u32 = 1 << 17;
    pub(crate) const READING_MORE: u32 = 1 << 18;
    pub(crate) const DATA_EMITTED: u32 = 1 << 19;
    pub(crate) const DEFAULT_UTF8_ENCODING: u32 = 1 << 20;
    pub(crate) const DECODER: u32 = 1 << 21;
    pub(crate) const ENCODING: u32 = 1 << 22;
    pub(crate) const HAS_FLOWING: u32 = 1 << 23;
    pub(crate) const FLOWING: u32 = 1 << 24;
    pub(crate) const HAS_PAUSED: u32 = 1 << 25;
    pub(crate) const PAUSED: u32 = 1 << 26;
    pub(crate) const DATA_LISTENING: u32 = 1 << 27;
}

/// `WritableState`'s own bits.
pub(crate) mod writable {
    pub(crate) const SYNC: u32 = 1 << 9;
    pub(crate) const FINAL_CALLED: u32 = 1 << 10;
    pub(crate) const NEED_DRAIN: u32 = 1 << 11;
    pub(crate) const ENDING: u32 = 1 << 12;
    pub(crate) const FINISHED: u32 = 1 << 13;
    pub(crate) const DECODE_STRINGS: u32 = 1 << 14;
    pub(crate) const WRITING: u32 = 1 << 15;
    pub(crate) const BUFFER_PROCESSING: u32 = 1 << 16;
    pub(crate) const PREFINISHED: u32 = 1 << 17;
    pub(crate) const ALL_BUFFERS: u32 = 1 << 18;
    pub(crate) const ALL_NOOP: u32 = 1 << 19;
    pub(crate) const ON_FINISHED: u32 = 1 << 20;
    pub(crate) const HAS_WRITABLE: u32 = 1 << 21;
    pub(crate) const WRITABLE: u32 = 1 << 22;
    pub(crate) const CORKED: u32 = 1 << 23;
    pub(crate) const DEFAULT_UTF8_ENCODING: u32 = 1 << 24;
    pub(crate) const WRITE_CB: u32 = 1 << 25;
    pub(crate) const EXPECT_WRITE_CB: u32 = 1 << 26;
    pub(crate) const AFTER_WRITE_TICK_INFO: u32 = 1 << 27;
    pub(crate) const AFTER_WRITE_PENDING: u32 = 1 << 28;
    pub(crate) const BUFFERED: u32 = 1 << 29;
    pub(crate) const ENDED: u32 = 1 << 30;
}

/// The bit field of a state, or `None` when the state has no `kState` (a state shaped by
/// another library).
pub(crate) fn raw_bits(c: &mut Context<'_>, state: RootId) -> Result<Option<u32>, RootedError> {
    let value = get_slot(c, state, Slot::State)?;
    if is_undefined(c, value) {
        return Ok(None);
    }
    let number = match c.rooted_value(value).and_then(|value| value.as_number()) {
        Some(number) => number,
        None => c.to_number(value)?,
    };
    Ok(Some(number as i64 as u32))
}

/// The bit field as `state[kState] & mask` reads it.
pub(crate) fn bits(c: &mut Context<'_>, state: RootId) -> Result<u32, RootedError> {
    Ok(raw_bits(c, state)?.unwrap_or_default())
}

pub(crate) fn set_bits(c: &mut Context<'_>, state: RootId, value: u32) -> Result<(), RootedError> {
    let value = c.number(f64::from(value));
    put_slot(c, state, Slot::State, value)
}

/// `state[kState] = (state[kState] | set) & ~clear`; the new field.
pub(crate) fn change(c: &mut Context<'_>, state: RootId, set: u32, clear: u32) -> Result<u32, RootedError> {
    let value = (bits(c, state)? | set) & !clear;
    set_bits(c, state, value)?;
    Ok(value)
}

pub(crate) fn has(c: &mut Context<'_>, state: RootId, mask: u32) -> Result<bool, RootedError> {
    Ok(bits(c, state)? & mask != 0)
}

/// What a held-value field reads while its bit is clear.
#[derive(Clone, Copy)]
enum Absent {
    Null,
    Undefined,
    Nop,
    EmptyArray,
}

/// A state property, as the state prototype's accessor projects it.
#[derive(Clone, Copy)]
enum Field {
    Flag(u32),
    /// A value held in `slot` while `bit` is set. `always` stores every assigned value,
    /// not only truthy ones.
    Held { bit: u32, slot: Slot, absent: Absent, always: bool },
    /// `'utf8'` while `bit` is set, otherwise the held encoding.
    DefaultEncoding(u32),
    /// `null`/`undefined` (`absent`) without `has`, otherwise whether `on` is set.
    Tristate { has: u32, on: u32, absent: Absent },
    Paused,
    PipesCount,
    BufferedRequestCount,
}

const READABLE_FIELDS: &[(&str, Field)] = &[
    ("objectMode", Field::Flag(OBJECT_MODE)),
    ("ended", Field::Flag(readable::ENDED)),
    ("endEmitted", Field::Flag(readable::END_EMITTED)),
    ("reading", Field::Flag(readable::READING)),
    ("constructed", Field::Flag(CONSTRUCTED)),
    ("sync", Field::Flag(readable::SYNC)),
    ("needReadable", Field::Flag(readable::NEED_READABLE)),
    ("emittedReadable", Field::Flag(readable::EMITTED_READABLE)),
    ("readableListening", Field::Flag(readable::READABLE_LISTENING)),
    ("resumeScheduled", Field::Flag(readable::RESUME_SCHEDULED)),
    ("errorEmitted", Field::Flag(ERROR_EMITTED)),
    ("emitClose", Field::Flag(EMIT_CLOSE)),
    ("autoDestroy", Field::Flag(AUTO_DESTROY)),
    ("destroyed", Field::Flag(DESTROYED)),
    ("closed", Field::Flag(CLOSED)),
    ("closeEmitted", Field::Flag(CLOSE_EMITTED)),
    ("multiAwaitDrain", Field::Flag(readable::MULTI_AWAIT_DRAIN)),
    ("readingMore", Field::Flag(readable::READING_MORE)),
    ("dataEmitted", Field::Flag(readable::DATA_EMITTED)),
    ("errored", Field::Held { bit: ERRORED, slot: Slot::ErroredValue, absent: Absent::Null, always: false }),
    ("defaultEncoding", Field::DefaultEncoding(readable::DEFAULT_UTF8_ENCODING)),
    ("decoder", Field::Held { bit: readable::DECODER, slot: Slot::DecoderValue, absent: Absent::Null, always: false }),
    ("encoding", Field::Held { bit: readable::ENCODING, slot: Slot::EncodingValue, absent: Absent::Null, always: false }),
    ("flowing", Field::Tristate { has: readable::HAS_FLOWING, on: readable::FLOWING, absent: Absent::Null }),
    ("pipesCount", Field::PipesCount),
    ("paused", Field::Paused),
];

const WRITABLE_FIELDS: &[(&str, Field)] = &[
    ("objectMode", Field::Flag(OBJECT_MODE)),
    ("finalCalled", Field::Flag(writable::FINAL_CALLED)),
    ("needDrain", Field::Flag(writable::NEED_DRAIN)),
    ("ending", Field::Flag(writable::ENDING)),
    ("ended", Field::Flag(writable::ENDED)),
    ("finished", Field::Flag(writable::FINISHED)),
    ("destroyed", Field::Flag(DESTROYED)),
    ("decodeStrings", Field::Flag(writable::DECODE_STRINGS)),
    ("writing", Field::Flag(writable::WRITING)),
    ("sync", Field::Flag(writable::SYNC)),
    ("bufferProcessing", Field::Flag(writable::BUFFER_PROCESSING)),
    ("constructed", Field::Flag(CONSTRUCTED)),
    ("prefinished", Field::Flag(writable::PREFINISHED)),
    ("errorEmitted", Field::Flag(ERROR_EMITTED)),
    ("emitClose", Field::Flag(EMIT_CLOSE)),
    ("autoDestroy", Field::Flag(AUTO_DESTROY)),
    ("closed", Field::Flag(CLOSED)),
    ("closeEmitted", Field::Flag(CLOSE_EMITTED)),
    ("allBuffers", Field::Flag(writable::ALL_BUFFERS)),
    ("allNoop", Field::Flag(writable::ALL_NOOP)),
    ("errored", Field::Held { bit: ERRORED, slot: Slot::ErroredValue, absent: Absent::Null, always: false }),
    ("writable", Field::Tristate { has: writable::HAS_WRITABLE, on: writable::WRITABLE, absent: Absent::Undefined }),
    ("defaultEncoding", Field::DefaultEncoding(writable::DEFAULT_UTF8_ENCODING)),
    ("writecb", Field::Held { bit: writable::WRITE_CB, slot: Slot::WriteCbValue, absent: Absent::Nop, always: true }),
    (
        "afterWriteTickInfo",
        Field::Held { bit: writable::AFTER_WRITE_TICK_INFO, slot: Slot::AfterWriteTickInfoValue, absent: Absent::Null, always: true },
    ),
    ("buffered", Field::Held { bit: writable::BUFFERED, slot: Slot::BufferedValue, absent: Absent::EmptyArray, always: true }),
];

/// Fields with no setter.
fn read_only(field: Field) -> bool {
    matches!(field, Field::PipesCount | Field::BufferedRequestCount)
}

/// The field an accessor's data index names: the readable fields, then the writable ones,
/// then `bufferedRequestCount`.
fn field(index: usize) -> Field {
    READABLE_FIELDS
        .iter()
        .chain(WRITABLE_FIELDS)
        .map(|(_, field)| *field)
        .chain(std::iter::once(Field::BufferedRequestCount))
        .nth(index)
        .expect("accessor index names a field")
}

fn field_index(c: &mut Context<'_>) -> Result<usize, RootedError> {
    let data = c.host_function_data()?;
    Ok(c.to_number(data)? as usize)
}

fn absent_value(c: &mut Context<'_>, absent: Absent) -> Outcome {
    match absent {
        Absent::Null => Ok(c.null()),
        Absent::Undefined => Ok(c.undefined()),
        Absent::Nop => Ok(intrinsics(c)?.nop),
        Absent::EmptyArray => c.array_rooted(&[]),
    }
}

/// A state accessor's getter.
pub(crate) fn field_get(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let field = field(field_index(c)?);
    let bits = bits(c, this)?;
    match field {
        Field::Flag(bit) => Ok(c.boolean(bits & bit != 0)),
        Field::Held { bit, slot, absent, .. } => {
            if bits & bit != 0 { get_slot(c, this, slot) } else { absent_value(c, absent) }
        }
        Field::DefaultEncoding(bit) => {
            if bits & bit != 0 { Ok(c.string_rooted("utf8")) } else { get_slot(c, this, Slot::DefaultEncodingValue) }
        }
        Field::Tristate { has, on, absent } => {
            if bits & has != 0 { Ok(c.boolean(bits & on != 0)) } else { absent_value(c, absent) }
        }
        Field::Paused => Ok(c.boolean(bits & readable::PAUSED != 0)),
        Field::PipesCount => {
            let pipes = get(c, this, "pipes")?;
            get(c, pipes, "length")
        }
        Field::BufferedRequestCount => {
            if bits & writable::BUFFERED == 0 {
                return Ok(c.number(0.0));
            }
            let buffered = get_slot(c, this, Slot::BufferedValue)?;
            let length = super::number(c, buffered, "length")?;
            let index = super::number(c, this, "bufferedIndex")?;
            Ok(c.number(length - index))
        }
    }
}

/// A state accessor's setter.
pub(crate) fn field_set(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let field = field(field_index(c)?);
    let value = arg(c, args, 0);
    let truthy = c.truthy_rooted(value)?;
    match field {
        Field::Flag(bit) => {
            if truthy { change(c, this, bit, 0)? } else { change(c, this, 0, bit)? };
        }
        Field::Held { bit, slot, always, .. } => {
            if always || truthy {
                put_slot(c, this, slot, value)?;
            }
            if truthy { change(c, this, bit, 0)? } else { change(c, this, 0, bit)? };
        }
        Field::DefaultEncoding(bit) => {
            let utf8 = c.string_text(value)?.is_some_and(|name| name == "utf8" || name == "utf-8");
            if utf8 {
                change(c, this, bit, 0)?;
            } else {
                change(c, this, 0, bit)?;
                put_slot(c, this, Slot::DefaultEncodingValue, value)?;
            }
        }
        Field::Tristate { has, on, .. } => {
            if is_nullish(c, value) {
                change(c, this, 0, has | on)?;
            } else if truthy {
                change(c, this, has | on, 0)?;
            } else {
                change(c, this, has, on)?;
            }
        }
        Field::Paused => {
            change(c, this, readable::HAS_PAUSED, 0)?;
            if truthy { change(c, this, readable::PAUSED, 0)? } else { change(c, this, 0, readable::PAUSED)? };
        }
        Field::PipesCount | Field::BufferedRequestCount => {}
    }
    Ok(c.undefined())
}

fn define_fields(c: &mut Context<'_>, prototype: RootId, fields: &[(&str, Field)], first: usize) -> Result<(), RootedError> {
    for (offset, (name, field)) in fields.iter().enumerate() {
        define_field(c, prototype, name, first + offset, read_only(*field))?;
    }
    Ok(())
}

fn define_field(c: &mut Context<'_>, prototype: RootId, name: &str, index: usize, read_only: bool) -> Result<(), RootedError> {
    let data = c.number(index as f64);
    let getter = c.host_function_named_with_data(crate::host::shared_vm::operation("streamStateFieldGet"), "get", data)?;
    let setter = if read_only {
        None
    } else {
        Some(c.host_function_named_with_data(crate::host::shared_vm::operation("streamStateFieldSet"), "set", data)?)
    };
    define_accessor(c, prototype, name, getter, setter)
}

pub(crate) fn build_readable_state(c: &mut Context<'_>) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamReadableState"), "ReadableState")?;
    let prototype = get(c, constructor, "prototype")?;
    define_fields(c, prototype, READABLE_FIELDS, 0)?;
    define_methods(c, prototype, &[(Key::Symbol(Slot::OnConstructed), "streamReadableOnConstructed", "onConstructed")])?;
    Ok(constructor)
}

pub(crate) fn build_writable_state(c: &mut Context<'_>) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation("streamWritableState"), "WritableState")?;
    let prototype = get(c, constructor, "prototype")?;
    define_fields(c, prototype, WRITABLE_FIELDS, READABLE_FIELDS.len())?;
    define_methods(c, prototype, &[(Key::Name("getBuffer"), "streamWritableGetBuffer", "getBuffer")])?;
    define_field(c, prototype, "bufferedRequestCount", READABLE_FIELDS.len() + WRITABLE_FIELDS.len(), true)?;
    define_methods(c, prototype, &[(Key::Symbol(Slot::OnConstructed), "streamWritableOnConstructed", "onConstructed")])?;
    Ok(constructor)
}

/// `options?.[name]`.
pub(crate) fn option(c: &mut Context<'_>, options: RootId, name: &str) -> Outcome {
    if is_nullish(c, options) {
        return Ok(c.undefined());
    }
    get(c, options, name)
}

/// The two defaults `getDefaultHighWaterMark` returns, which `setDefaultHighWaterMark` changes.
#[derive(Clone, Copy)]
pub(crate) struct DefaultHighWaterMarks {
    bytes: f64,
    objects: f64,
}

/// Bytes a byte stream buffers by default (Node's non-Windows default; Windows uses 16 KiB).
const DEFAULT_BYTE_HIGH_WATER_MARK: f64 = (64 * 1024) as f64;
/// Objects an object-mode stream buffers by default.
const DEFAULT_OBJECT_HIGH_WATER_MARK: f64 = 16.0;

crate::host::startup_snapshot::host_snap_struct!(DefaultHighWaterMarks { bytes, objects });

impl Default for DefaultHighWaterMarks {
    fn default() -> Self {
        Self { bytes: DEFAULT_BYTE_HIGH_WATER_MARK, objects: DEFAULT_OBJECT_HIGH_WATER_MARK }
    }
}

pub(crate) fn default_high_water_mark(c: &mut Context<'_>, object_mode: bool) -> f64 {
    let defaults = c.host_mut().shared_state().borrow().stream_high_water_marks;
    if object_mode { defaults.objects } else { defaults.bytes }
}

/// `getHighWaterMark(state, options, duplexKey, isDuplex)`.
pub(crate) fn high_water_mark(
    c: &mut Context<'_>,
    object_mode: bool,
    options: RootId,
    duplex_key: &str,
    duplex: bool,
) -> Result<f64, RootedError> {
    let mut value = get(c, options, "highWaterMark")?;
    if is_nullish(c, value) {
        value = if duplex { get(c, options, duplex_key)? } else { c.null() };
    }
    if is_nullish(c, value) {
        return Ok(default_high_water_mark(c, object_mode));
    }
    let integer = c.rooted_value(value).and_then(|value| value.as_number()).filter(|number| number.fract() == 0.0);
    match integer {
        Some(number) if number >= 0.0 => Ok(number.floor()),
        _ => {
            let name = if duplex { format!("options.{duplex_key}") } else { "options.highWaterMark".to_owned() };
            Err(super::super::util_shared_vm::argument_value_error(c, value, &name, "is invalid")?)
        }
    }
}

/// `getHighWaterMark(state, options, duplexKey, isDuplex)`, as the state module exports it.
pub(crate) fn get_high_water_mark(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (state, options, key, duplex) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2), arg(c, args, 3));
    let object_mode_value = get(c, state, "objectMode")?;
    let object_mode = c.truthy_rooted(object_mode_value)?;
    let key = c.to_string(key)?;
    let duplex = c.truthy_rooted(duplex)?;
    let mark = high_water_mark(c, object_mode, options, &key, duplex)?;
    Ok(c.number(mark))
}

/// `getDefaultHighWaterMark(objectMode)`.
pub(crate) fn get_default_high_water_mark(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let object_mode = arg(c, args, 0);
    let object_mode = c.truthy_rooted(object_mode)?;
    let mark = default_high_water_mark(c, object_mode);
    Ok(c.number(mark))
}

/// `setDefaultHighWaterMark(objectMode, value)`.
pub(crate) fn set_default_high_water_mark(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let (object_mode, value) = (arg(c, args, 0), arg(c, args, 1));
    let maximum = c.number(MAX_SAFE_INTEGER);
    let mark = super::super::util_shared_vm::validate_integer(c, value, "value", 0.0, maximum)?;
    let object_mode = c.truthy_rooted(object_mode)?;
    let shared = c.host_mut().shared_state();
    let mut shared = shared.borrow_mut();
    if object_mode {
        shared.stream_high_water_marks.objects = mark;
    } else {
        shared.stream_high_water_marks.bytes = mark;
    }
    Ok(c.undefined())
}

/// `Number.MAX_SAFE_INTEGER`, `validateInteger`'s default bound.
const MAX_SAFE_INTEGER: f64 = 9_007_199_254_740_991.0;

/// The encoding a state's `defaultEncoding` option names: `None` for UTF-8 (its own bit).
fn default_encoding(c: &mut Context<'_>, options: RootId) -> Result<Option<RootId>, RootedError> {
    let value = option(c, options, "defaultEncoding")?;
    if is_nullish(c, value) {
        return Ok(None);
    }
    let name = c.string_text(value)?;
    if name.as_deref().is_some_and(|name| name == "utf8" || name == "utf-8") {
        return Ok(None);
    }
    if name.as_deref().is_some_and(is_encoding) {
        return Ok(Some(value));
    }
    let shown = c.to_string(value)?;
    Err(errors::throw(c, StreamError::UnknownEncoding(shown)))
}

/// `Buffer.isEncoding(name)`.
pub(crate) fn is_encoding(name: &str) -> bool {
    !name.is_empty() && super::super::buffer_codec::canonical_encoding(name).is_some()
}

/// `new ReadableState(options, stream, isDuplex)`.
pub(crate) fn readable_state(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (options, duplex) = (arg(c, args, 0), arg(c, args, 2));
    let duplex = c.truthy_rooted(duplex)?;
    let mut field = EMIT_CLOSE | AUTO_DESTROY | CONSTRUCTED | readable::SYNC;
    let object_mode = option(c, options, "objectMode")?;
    let mut objects = c.truthy_rooted(object_mode)?;
    if duplex {
        let readable_objects = option(c, options, "readableObjectMode")?;
        objects |= c.truthy_rooted(readable_objects)?;
    }
    if objects {
        field |= OBJECT_MODE;
    }
    set_bits(c, this, field)?;
    let mark = if c.truthy_rooted(options)? {
        high_water_mark(c, objects, options, "readableHighWaterMark", duplex)?
    } else {
        default_high_water_mark(c, false)
    };
    put_number(c, this, "highWaterMark", mark)?;
    let buffer = c.array_rooted(&[])?;
    put(c, this, "buffer", buffer)?;
    put_number(c, this, "bufferIndex", 0.0)?;
    put_number(c, this, "length", 0.0)?;
    let pipes = c.array_rooted(&[])?;
    put(c, this, "pipes", pipes)?;
    clear_disabled(c, this, options)?;
    match default_encoding(c, options)? {
        None => {
            change(c, this, readable::DEFAULT_UTF8_ENCODING, 0)?;
        }
        Some(encoding) => {
            put_slot(c, this, Slot::DefaultEncodingValue, encoding)?;
        }
    }
    let null = c.null();
    put(c, this, "awaitDrainWriters", null)?;
    let encoding = option(c, options, "encoding")?;
    if c.truthy_rooted(encoding)? {
        let decoder = string_decoder(c, encoding)?;
        put_slot(c, this, Slot::DecoderValue, decoder)?;
        put_slot(c, this, Slot::EncodingValue, encoding)?;
        change(c, this, readable::DECODER | readable::ENCODING, 0)?;
    }
    Ok(c.undefined())
}

/// The `emitClose: false` and `autoDestroy: false` options.
fn clear_disabled(c: &mut Context<'_>, state: RootId, options: RootId) -> Result<(), RootedError> {
    for (name, bit) in [("emitClose", EMIT_CLOSE), ("autoDestroy", AUTO_DESTROY)] {
        let value = option(c, options, name)?;
        let no = c.boolean(false);
        if c.strict_equal_rooted(value, no)? {
            change(c, state, 0, bit)?;
        }
    }
    Ok(())
}

/// `new StringDecoder(encoding)`.
pub(crate) fn string_decoder(c: &mut Context<'_>, encoding: RootId) -> Outcome {
    let module = crate::host::shared_vm::commonjs::library_require(c, "string_decoder")?;
    let constructor = get(c, module, "StringDecoder")?;
    c.construct_rooted(constructor, constructor, &[encoding])
}

/// `new WritableState(options, stream, isDuplex)`.
pub(crate) fn writable_state(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (options, stream, duplex) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let duplex = c.truthy_rooted(duplex)?;
    let mut field = writable::SYNC | CONSTRUCTED | EMIT_CLOSE | AUTO_DESTROY;
    let object_mode = option(c, options, "objectMode")?;
    let mut objects = c.truthy_rooted(object_mode)?;
    if duplex {
        let writable_objects = option(c, options, "writableObjectMode")?;
        objects |= c.truthy_rooted(writable_objects)?;
    }
    if objects {
        field |= OBJECT_MODE;
    }
    set_bits(c, this, field)?;
    let mark = if c.truthy_rooted(options)? {
        high_water_mark(c, objects, options, "writableHighWaterMark", duplex)?
    } else {
        default_high_water_mark(c, false)
    };
    put_number(c, this, "highWaterMark", mark)?;
    let decode = option(c, options, "decodeStrings")?;
    let no = c.boolean(false);
    if !c.strict_equal_rooted(decode, no)? {
        change(c, this, writable::DECODE_STRINGS, 0)?;
    }
    clear_disabled(c, this, options)?;
    match default_encoding(c, options)? {
        None => {
            change(c, this, writable::DEFAULT_UTF8_ENCODING, 0)?;
        }
        Some(encoding) => {
            change(c, this, 0, writable::DEFAULT_UTF8_ENCODING)?;
            put_slot(c, this, Slot::DefaultEncodingValue, encoding)?;
        }
    }
    put_number(c, this, "length", 0.0)?;
    put_number(c, this, "corked", 0.0)?;
    let onwrite = super::closure(c, "streamOnWrite", "bound onwrite", &[stream])?;
    put(c, this, "onwrite", onwrite)?;
    put_number(c, this, "writelen", 0.0)?;
    reset_buffer(c, this)?;
    put_number(c, this, "pendingcb", 0.0)?;
    Ok(c.undefined())
}

/// `resetBuffer(state)`.
pub(crate) fn reset_buffer(c: &mut Context<'_>, state: RootId) -> Result<(), RootedError> {
    let null = c.null();
    put_slot(c, state, Slot::BufferedValue, null)?;
    put_number(c, state, "bufferedIndex", 0.0)?;
    change(c, state, writable::ALL_BUFFERS | writable::ALL_NOOP, writable::BUFFERED)?;
    Ok(())
}

/// `WritableState.prototype.getBuffer()`.
pub(crate) fn get_buffer(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    if !has(c, this, writable::BUFFERED)? {
        return c.array_rooted(&[]);
    }
    let buffered = get(c, this, "buffered")?;
    let index = get(c, this, "bufferedIndex")?;
    call_method(c, buffered, "slice", &[index])
}

/// A new state of `class` for `stream`.
pub(crate) fn create(c: &mut Context<'_>, class: Class, options: RootId, stream: RootId, duplex: bool) -> Outcome {
    let constructor = intrinsics(c)?.class(class);
    let duplex = c.boolean(duplex);
    c.construct_rooted(constructor, constructor, &[options, stream, duplex])
}

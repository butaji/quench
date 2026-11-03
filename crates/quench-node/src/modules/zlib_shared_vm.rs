//! The `zlib` module (lib/zlib.js): compression `Transform` streams over the handles of
//! `internalBinding('zlib')`, their one-shot buffer forms, `crc32` and the constant tables.
//!
//! A stream keeps its codec state where Node keeps it: a binding handle in `_handle`, the
//! output window in `_outBuffer`/`_outOffset`, and the handle's progress (`availInBefore`,
//! `availOutBefore`, `inOff`, `flushFlag`) on the handle itself.

use super::stream_shared_vm::{
    arg, call_method, captures, closure, eos, get, is_nullish, is_undefined, next_tick, number, put, put_number,
    set_capture, Context, Outcome,
};
use super::zlib_constants::{self as constants, Mode};
use quench_runtime::{BuiltinBrand, DescriptorFields, RootId, RootedError, ValueType};

const MODULE_CACHE: &str = "\0builtin:zlib";

/// The module's fixed roots, each kept in the builtin cache under its own key.
#[derive(Clone, Copy)]
enum Internal {
    /// `kFlushFlag`: the flush a zero-length chunk from `flush()` carries.
    FlushFlag,
    /// `kError`: the error the handle reported.
    Error,
    /// `kFlushBuffers`: one zero-length chunk per zlib flush flag, indexed by the flag.
    FlushBuffers,
    /// `processCallback`, the handle's write completion.
    ProcessCallback,
    /// `zlibOnError`, the handle's error report.
    OnError,
    BufferOnData,
    BufferOnError,
    BufferOnEnd,
}

impl Internal {
    const ALL: [Self; 8] = [
        Self::FlushFlag,
        Self::Error,
        Self::FlushBuffers,
        Self::ProcessCallback,
        Self::OnError,
        Self::BufferOnData,
        Self::BufferOnError,
        Self::BufferOnEnd,
    ];

    fn key(self) -> &'static str {
        match self {
            Self::FlushFlag => "\0zlib:kFlushFlag",
            Self::Error => "\0zlib:kError",
            Self::FlushBuffers => "\0zlib:kFlushBuffers",
            Self::ProcessCallback => "\0zlib:processCallback",
            Self::OnError => "\0zlib:zlibOnError",
            Self::BufferOnData => "\0zlib:zlibBufferOnData",
            Self::BufferOnError => "\0zlib:zlibBufferOnError",
            Self::BufferOnEnd => "\0zlib:zlibBufferOnEnd",
        }
    }

    fn make(self, c: &mut Context<'_>) -> Outcome {
        let function = |c: &mut Context<'_>, operation: &str, name: &str| {
            c.host_function_named(crate::host::shared_vm::operation(operation), name)
        };
        match self {
            Self::FlushFlag => Ok(c.symbol_rooted(Some("kFlushFlag"))),
            Self::Error => Ok(c.symbol_rooted(Some("kError"))),
            Self::FlushBuffers => flush_buffers(c),
            Self::ProcessCallback => function(c, "zlibProcessCallback", "processCallback"),
            Self::OnError => function(c, "zlibOnError", "zlibOnError"),
            Self::BufferOnData => function(c, "zlibBufferOnData", "zlibBufferOnData"),
            Self::BufferOnError => function(c, "zlibBufferOnError", "zlibBufferOnError"),
            Self::BufferOnEnd => function(c, "zlibBufferOnEnd", "zlibBufferOnEnd"),
        }
    }
}

fn internal(c: &mut Context<'_>, which: Internal) -> Outcome {
    let shared = c.host_mut().shared_state();
    let found = shared.borrow().builtin_cache.get(which.key()).copied();
    found.ok_or_else(|| RootedError::host("zlib module is not initialized"))
}

/// The family a mode belongs to: its handle, flush flags and parameter form.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Family {
    Zlib,
    Brotli,
    Zstd,
}

/// A family's flushes: the default per write, the one ending the stream, the one `flush()`
/// uses, and the bounds an option must lie in.
struct Flushes {
    flush: f64,
    finish: f64,
    full: f64,
    bounds: (f64, f64),
}

impl Family {
    fn of(mode: Mode) -> Self {
        match mode {
            Mode::BrotliCompress | Mode::BrotliDecompress => Self::Brotli,
            Mode::ZstdCompress | Mode::ZstdDecompress => Self::Zstd,
            _ => Self::Zlib,
        }
    }

    fn flushes(self) -> Flushes {
        match self {
            Self::Zlib => Flushes {
                flush: constants::Z_NO_FLUSH,
                finish: constants::Z_FINISH,
                full: constants::Z_FULL_FLUSH,
                bounds: (constants::Z_NO_FLUSH, constants::Z_BLOCK),
            },
            Self::Brotli => Flushes {
                flush: constants::BROTLI_OPERATION_PROCESS,
                finish: constants::BROTLI_OPERATION_FINISH,
                full: constants::BROTLI_OPERATION_FLUSH,
                bounds: (constants::BROTLI_OPERATION_PROCESS, constants::BROTLI_OPERATION_EMIT_METADATA),
            },
            Self::Zstd => Flushes {
                flush: constants::ZSTD_E_CONTINUE,
                finish: constants::ZSTD_E_END,
                full: constants::ZSTD_E_FLUSH,
                bounds: (constants::ZSTD_E_CONTINUE, constants::ZSTD_E_END),
            },
        }
    }
}

/// The zlib flush flags from weakest to strongest (`kFlushFlagList`).
const FLUSH_ORDER: [f64; 6] = [
    constants::Z_NO_FLUSH,
    constants::Z_BLOCK,
    constants::Z_PARTIAL_FLUSH,
    constants::Z_SYNC_FLUSH,
    constants::Z_FULL_FLUSH,
    constants::Z_FINISH,
];

fn flushiness(flag: f64) -> Option<usize> {
    FLUSH_ORDER.iter().position(|candidate| *candidate == flag)
}

/// `maxFlush(a, b)`: the stronger flush; `b` unless `a` is ranked above it.
fn max_flush(a: f64, b: f64) -> f64 {
    match (flushiness(a), flushiness(b)) {
        (Some(first), Some(second)) if first > second => a,
        _ => b,
    }
}

/// The exported constructors, their modes and parents.
const PUBLIC_CLASSES: [(&str, Mode); 11] = [
    ("Deflate", Mode::Deflate),
    ("Inflate", Mode::Inflate),
    ("Gzip", Mode::Gzip),
    ("Gunzip", Mode::Gunzip),
    ("DeflateRaw", Mode::DeflateRaw),
    ("InflateRaw", Mode::InflateRaw),
    ("Unzip", Mode::Unzip),
    ("BrotliCompress", Mode::BrotliCompress),
    ("BrotliDecompress", Mode::BrotliDecompress),
    ("ZstdCompress", Mode::ZstdCompress),
    ("ZstdDecompress", Mode::ZstdDecompress),
];

/// The convenience methods: the asynchronous name, the synchronous one and the class.
const CONVENIENCE: [(&str, &str, &str); 11] = [
    ("deflate", "deflateSync", "Deflate"),
    ("gzip", "gzipSync", "Gzip"),
    ("deflateRaw", "deflateRawSync", "DeflateRaw"),
    ("unzip", "unzipSync", "Unzip"),
    ("inflate", "inflateSync", "Inflate"),
    ("gunzip", "gunzipSync", "Gunzip"),
    ("inflateRaw", "inflateRawSync", "InflateRaw"),
    ("brotliCompress", "brotliCompressSync", "BrotliCompress"),
    ("brotliDecompress", "brotliDecompressSync", "BrotliDecompress"),
    ("zstdCompress", "zstdCompressSync", "ZstdCompress"),
    ("zstdDecompress", "zstdDecompressSync", "ZstdDecompress"),
];

/// The `create*` factories, in Node's order.
const FACTORIES: [(&str, &str); 11] = [
    ("createDeflate", "Deflate"),
    ("createInflate", "Inflate"),
    ("createDeflateRaw", "DeflateRaw"),
    ("createInflateRaw", "InflateRaw"),
    ("createGzip", "Gzip"),
    ("createGunzip", "Gunzip"),
    ("createUnzip", "Unzip"),
    ("createBrotliCompress", "BrotliCompress"),
    ("createBrotliDecompress", "BrotliDecompress"),
    ("createZstdCompress", "ZstdCompress"),
    ("createZstdDecompress", "ZstdDecompress"),
];

/// The return codes `zlib.codes` translates both ways.
const CODES: [&str; 9] = [
    "Z_OK",
    "Z_STREAM_END",
    "Z_NEED_DICT",
    "Z_ERRNO",
    "Z_STREAM_ERROR",
    "Z_DATA_ERROR",
    "Z_MEM_ERROR",
    "Z_BUF_ERROR",
    "Z_VERSION_ERROR",
];

/// The prefixes naming each parameterized family's parameters.
const BROTLI_PARAMETERS: &str = "BROTLI_PARAM_";
const ZSTD_COMPRESS_PARAMETERS: &str = "ZSTD_c_";
const ZSTD_DECOMPRESS_PARAMETERS: &str = "ZSTD_d_";

/// The prefix of the constants the module re-exports as deprecated top-level members.
const UNEXPORTED_CONSTANTS: &str = "BROTLI";

/// The words of a `DEP0108` warning for `bytesRead`'s getter and setter.
const BYTES_READ_DEPRECATION: &str = "DEP0108";
const BYTES_READ_GET_WARNING: &str =
    "zlib.bytesRead is deprecated and will change its meaning in the future. Use zlib.bytesWritten instead.";
const BYTES_READ_SET_WARNING: &str = "Setting zlib.bytesRead is deprecated. This feature will be removed in the future.";

/// An unset slot in a parameter array (`fill(-1)` on a `Uint32Array`).
const UNSET_PARAMETER: u32 = u32::MAX;
/// `2 ** 32`, the modulus of `ToUint32`.
const UINT32_MODULUS: f64 = u32::MAX as f64 + 1.0;
/// The number of slots in `_writeState`: output then input bytes left.
const WRITE_STATE_SLOTS: f64 = 2.0;

const INTERNAL_ASSERTION_NOTE: &str = "This is caused by either a bug in Node.js or incorrect usage of Node.js internals.\nPlease open an issue with this stack trace at https://github.com/nodejs/node/issues\n";

// ---------------------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------------------

fn thrown(c: &mut Context<'_>, built: Result<RootId, RootedError>) -> RootedError {
    match built {
        Ok(error) => c.throw(error),
        Err(error) => error,
    }
}

fn range_error(c: &mut Context<'_>, message: &str, code: &'static str) -> Outcome {
    let error = c.range_error_rooted(message)?;
    super::node_errors_shared_vm::decorate_range_error(c, error, code)?;
    Ok(error)
}

fn plain_error(c: &mut Context<'_>, message: &str, code: &'static str) -> Outcome {
    let error = c.error_rooted(message)?;
    super::node_errors_shared_vm::decorate_error(c, error, code)?;
    Ok(error)
}

/// `ERR_OUT_OF_RANGE(name, range, value)`.
fn out_of_range(c: &mut Context<'_>, name: &str, range: &str, value: RootId) -> RootedError {
    let built = super::util_shared_vm::range_error_received(c, value).and_then(|received| {
        let message = format!("The value of \"{name}\" is out of range. It must be {range}. Received {received}");
        range_error(c, &message, "ERR_OUT_OF_RANGE")
    });
    thrown(c, built)
}

fn invalid_type(c: &mut Context<'_>, value: RootId, name: &str, expected: &str) -> RootedError {
    match super::util_shared_vm::argument_type_error_with_expected(c, value, name, expected) {
        Ok(error) | Err(error) => error,
    }
}

/// `assert(value, message)` from internal/assert: `ERR_INTERNAL_ASSERTION`.
fn internal_assertion(c: &mut Context<'_>, message: Option<&str>) -> RootedError {
    let text = match message {
        Some(message) => format!("{message}\n{INTERNAL_ASSERTION_NOTE}"),
        None => INTERNAL_ASSERTION_NOTE.to_owned(),
    };
    let built = plain_error(c, &text, "ERR_INTERNAL_ASSERTION");
    thrown(c, built)
}

// ---------------------------------------------------------------------------------------
// Validators
// ---------------------------------------------------------------------------------------

/// `checkFiniteNumber(value, name)`: the number, or `None` for `undefined` and `NaN`.
fn finite_number(c: &mut Context<'_>, value: RootId, name: &str) -> Result<Option<f64>, RootedError> {
    if is_undefined(c, value) {
        return Ok(None);
    }
    let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()) else {
        return Err(invalid_type(c, value, name, "of type number"));
    };
    if number.is_finite() {
        Ok(Some(number))
    } else if number.is_nan() {
        Ok(None)
    } else {
        Err(out_of_range(c, name, "a finite number", value))
    }
}

/// `checkRangesOrGetDefault(value, name, lower, upper, default)`.
fn in_range_or(
    c: &mut Context<'_>,
    value: RootId,
    name: &str,
    (lower, upper): (f64, f64),
    default: f64,
) -> Result<f64, RootedError> {
    match finite_number(c, value, name)? {
        None => Ok(default),
        Some(number) if number < lower || number > upper => {
            Err(out_of_range(c, name, &format!(">= {lower} and <= {upper}"), value))
        }
        Some(number) => Ok(number),
    }
}

fn option_in_range(
    c: &mut Context<'_>,
    options: RootId,
    member: &str,
    bounds: (f64, f64),
    default: f64,
) -> Result<f64, RootedError> {
    let value = get(c, options, member)?;
    in_range_or(c, value, &format!("options.{member}"), bounds, default)
}

fn require_function(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if c.is_callable_rooted(value)? {
        Ok(())
    } else {
        Err(invalid_type(c, value, name, "of type function"))
    }
}

fn brand(c: &mut Context<'_>, value: RootId, brand: BuiltinBrand) -> Result<bool, RootedError> {
    c.has_builtin_brand_rooted(value, brand)
}

fn is_function(c: &Context<'_>, value: RootId) -> bool {
    matches!(c.value_type_rooted(value), Some(ValueType::Function))
}

// ---------------------------------------------------------------------------------------
// Buffers
// ---------------------------------------------------------------------------------------

fn buffer_class(c: &mut Context<'_>) -> Outcome {
    super::buffer_shared_vm::fast_buffer(c)
}

/// `Buffer.allocUnsafe(size)`.
fn allocate(c: &mut Context<'_>, size: f64) -> Outcome {
    super::buffer_shared_vm::from_vec(c, vec![0; size as usize])
}

/// `new FastBuffer()`: an empty Buffer.
fn empty_buffer(c: &mut Context<'_>) -> Outcome {
    super::buffer_shared_vm::from_bytes(c, &[])
}

/// `Buffer.from(arrayBuffer, offset, length)`: a Buffer over the same memory.
fn buffer_over(c: &mut Context<'_>, memory: &[RootId]) -> Outcome {
    let class = super::buffer_shared_vm::fast_buffer_constructor(c)?;
    c.construct_rooted(class, class, memory)
}

fn flush_buffers(c: &mut Context<'_>) -> Outcome {
    let flag = internal(c, Internal::FlushFlag)?;
    let buffers = c.array_rooted(&[])?;
    for flush in FLUSH_ORDER {
        let buffer = empty_buffer(c)?;
        let value = c.number(flush);
        c.set_property_rooted(buffer, flag, value, buffer)?;
        let index = c.number(flush);
        c.set_property_rooted(buffers, index, buffer, buffers)?;
    }
    Ok(buffers)
}

// ---------------------------------------------------------------------------------------
// Construction
// ---------------------------------------------------------------------------------------

fn mode_of(value: f64) -> Option<Mode> {
    PUBLIC_CLASSES.iter().map(|(_, mode)| *mode).find(|mode| *mode as i32 as f64 == value)
}

/// `new Deflate(options)` and its siblings: the class closes over `[mode, class]`.
pub(crate) fn construct(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let [mode, class] = captures(c)?;
    let mode = c.to_number(mode)?;
    let mode = mode_of(mode).ok_or_else(|| RootedError::host("zlib class without a mode"))?;
    let options = arg(c, args, 0);
    if mode == Mode::DeflateRaw && c.truthy_rooted(options)? {
        let bits = get(c, options, "windowBits")?;
        if c.rooted_value(bits).and_then(|value| value.as_number()) == Some(constants::Z_MIN_WINDOWBITS) {
            put_number(c, options, "windowBits", constants::Z_MIN_WINDOWBITS + 1.0)?;
        }
    }
    if c.new_target_rooted().is_none() && !(c.is_object_rooted(this)? && c.instance_of_rooted(this, class)?) {
        return c.construct_rooted(class, class, &[options]);
    }
    match Family::of(mode) {
        Family::Zlib => initialize_zlib(c, this, options, mode)?,
        Family::Brotli => initialize_brotli(c, this, options, mode)?,
        Family::Zstd => initialize_zstd(c, this, options, mode)?,
    }
    Ok(c.undefined())
}

/// `ZlibBase`, `Zlib`, `Brotli` and `Zstd` are not constructed directly.
pub(crate) fn construct_abstract(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    Err(internal_assertion(c, None))
}

fn binding_handle(c: &mut Context<'_>, class: &str, mode: Mode) -> Outcome {
    let binding = super::zlib_native_shared_vm::binding(c)?;
    let constructor = get(c, binding, class)?;
    let mode = c.number(mode as i32 as f64);
    c.construct_rooted(constructor, constructor, &[mode])
}

fn write_state(c: &mut Context<'_>) -> Outcome {
    let global = c.global_root()?;
    let class = get(c, global, "Uint32Array")?;
    let slots = c.number(WRITE_STATE_SLOTS);
    c.construct_rooted(class, class, &[slots])
}

/// `Zlib(opts, mode)`: the zlib-backed streams.
fn initialize_zlib(c: &mut Context<'_>, this: RootId, options: RootId, mode: Mode) -> Result<(), RootedError> {
    let mut window_bits = constants::Z_DEFAULT_WINDOWBITS;
    let mut level = constants::Z_DEFAULT_COMPRESSION;
    let mut memory_level = constants::Z_DEFAULT_MEMLEVEL;
    let mut strategy = constants::Z_DEFAULT_STRATEGY;
    let mut dictionary = c.undefined();
    if c.truthy_rooted(options)? {
        let bits = get(c, options, "windowBits")?;
        let unset = is_nullish(c, bits) || c.rooted_value(bits).and_then(|value| value.as_number()) == Some(0.0);
        if unset && matches!(mode, Mode::Inflate | Mode::Gunzip | Mode::Unzip) {
            window_bits = 0.0;
        } else {
            let minimum = constants::Z_MIN_WINDOWBITS + if mode == Mode::Gzip { 1.0 } else { 0.0 };
            window_bits = in_range_or(
                c,
                bits,
                "options.windowBits",
                (minimum, constants::Z_MAX_WINDOWBITS),
                constants::Z_DEFAULT_WINDOWBITS,
            )?;
        }
        level = option_in_range(
            c,
            options,
            "level",
            (constants::Z_MIN_LEVEL, constants::Z_MAX_LEVEL),
            constants::Z_DEFAULT_COMPRESSION,
        )?;
        memory_level = option_in_range(
            c,
            options,
            "memLevel",
            (constants::Z_MIN_MEMLEVEL, constants::Z_MAX_MEMLEVEL),
            constants::Z_DEFAULT_MEMLEVEL,
        )?;
        strategy = option_in_range(
            c,
            options,
            "strategy",
            (constants::Z_DEFAULT_STRATEGY, constants::Z_FIXED),
            constants::Z_DEFAULT_STRATEGY,
        )?;
        dictionary = get(c, options, "dictionary")?;
        if !is_undefined(c, dictionary) && !brand(c, dictionary, BuiltinBrand::ArrayBufferView)? {
            if brand(c, dictionary, BuiltinBrand::AnyArrayBuffer)? {
                dictionary = buffer_over(c, &[dictionary])?;
            } else {
                return Err(invalid_type(
                    c,
                    dictionary,
                    "options.dictionary",
                    "an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
                ));
            }
        }
    }
    let handle = binding_handle(c, "Zlib", mode)?;
    let state = write_state(c)?;
    put(c, this, "_writeState", state)?;
    let callback = internal(c, Internal::ProcessCallback)?;
    let values = [window_bits, level, memory_level, strategy].map(|value| c.number(value));
    call_method(c, handle, "init", &[values[0], values[1], values[2], values[3], state, callback, dictionary])?;
    initialize_base(c, this, options, handle, Family::Zlib)?;
    put_number(c, this, "_level", level)?;
    put_number(c, this, "_strategy", strategy)?;
    put_number(c, this, "_mode", mode as i32 as f64)
}

/// `opts.params` as the `Uint32Array` a Brotli or Zstd handle reads: each slot
/// `ToUint32(value)`, unset slots all ones.
fn parameters(
    c: &mut Context<'_>,
    options: RootId,
    largest: f64,
    invalid: fn(&str) -> String,
    code: &'static str,
) -> Outcome {
    let mut values = vec![UNSET_PARAMETER; largest as usize + 1];
    let given = if c.truthy_rooted(options)? { get(c, options, "params")? } else { c.undefined() };
    if c.truthy_rooted(given)? {
        for key in c.enumerable_own_keys_rooted(given, false, true)? {
            let name = c.to_string(key)?;
            let index = c.to_number(key)?;
            let slot = (index >= 0.0 && index <= largest).then_some(index as usize);
            let Some(slot) = slot.filter(|slot| values[*slot] == UNSET_PARAMETER) else {
                let built = range_error(c, &invalid(&name), code);
                return Err(thrown(c, built));
            };
            let value = c.get_property_rooted(given, key)?;
            values[slot] = match c.rooted_value(value) {
                Some(value) if value.as_bool().is_some() => u32::from(value.as_bool() == Some(true)),
                Some(value) if value.as_number().is_some() => to_uint32(value.as_number().unwrap_or_default()),
                _ => return Err(invalid_type(c, value, "options.params[key]", "of type number")),
            };
        }
    }
    let bytes: Vec<u8> = values.iter().flat_map(|value| value.to_ne_bytes()).collect();
    c.uint8_array_from_rooted(bytes)
}

fn to_uint32(number: f64) -> u32 {
    if number.is_finite() { number.trunc().rem_euclid(UINT32_MODULUS) as u32 } else { 0 }
}

/// `Brotli(opts, mode)`.
fn initialize_brotli(c: &mut Context<'_>, this: RootId, options: RootId, mode: Mode) -> Result<(), RootedError> {
    let largest = constants::largest_with_prefix(BROTLI_PARAMETERS);
    let params = parameters(c, options, largest, |key| format!("{key} is not a valid Brotli parameter"), "ERR_BROTLI_INVALID_PARAM")?;
    let class = if mode == Mode::BrotliDecompress { "BrotliDecoder" } else { "BrotliEncoder" };
    let handle = binding_handle(c, class, mode)?;
    let state = write_state(c)?;
    put(c, this, "_writeState", state)?;
    let callback = internal(c, Internal::ProcessCallback)?;
    call_method(c, handle, "init", &[params, state, callback])?;
    initialize_base(c, this, options, handle, Family::Brotli)
}

/// `new Zstd(opts, mode, ...)`: `_writeState` is set after the base, as a class's
/// `super()` must run first.
fn initialize_zstd(c: &mut Context<'_>, this: RootId, options: RootId, mode: Mode) -> Result<(), RootedError> {
    let prefix = if mode == Mode::ZstdCompress { ZSTD_COMPRESS_PARAMETERS } else { ZSTD_DECOMPRESS_PARAMETERS };
    let largest = constants::largest_with_prefix(prefix);
    let params = parameters(c, options, largest, |key| format!("{key} is not a valid zstd parameter"), "ERR_ZSTD_INVALID_PARAM")?;
    let class = if mode == Mode::ZstdCompress { "ZstdCompress" } else { "ZstdDecompress" };
    let handle = binding_handle(c, class, mode)?;
    let undefined = c.undefined();
    let (pledged, dictionary) = if c.truthy_rooted(options)? {
        let pledged = get(c, options, "pledgedSrcSize")?;
        let pledged = if is_nullish(c, pledged) { undefined } else { pledged };
        let dictionary = get(c, options, "dictionary")?;
        let usable = c.truthy_rooted(dictionary)? && brand(c, dictionary, BuiltinBrand::ArrayBufferView)?;
        (pledged, if usable { dictionary } else { undefined })
    } else {
        (undefined, undefined)
    };
    let state = write_state(c)?;
    let callback = internal(c, Internal::ProcessCallback)?;
    call_method(c, handle, "init", &[params, pledged, state, callback, dictionary])?;
    initialize_base(c, this, options, handle, Family::Zstd)?;
    put(c, this, "_writeState", state)
}

/// `ZlibBase(opts, mode, handle, defaults)`: the Transform over a handle.
fn initialize_base(c: &mut Context<'_>, this: RootId, options: RootId, handle: RootId, family: Family) -> Result<(), RootedError> {
    let Flushes { mut flush, mut finish, full, bounds } = family.flushes();
    let mut chunk_size = constants::Z_DEFAULT_CHUNK;
    let mut max_output = super::buffer_module::MAX_LENGTH;
    let mut options = options;
    let given = c.truthy_rooted(options)?;
    if given {
        let size = get(c, options, "chunkSize")?;
        chunk_size = match finite_number(c, size, "options.chunkSize")? {
            None => constants::Z_DEFAULT_CHUNK,
            Some(small) if small < constants::Z_MIN_CHUNK => {
                let minimum = constants::Z_MIN_CHUNK;
                return Err(out_of_range(c, "options.chunkSize", &format!(">= {minimum}"), size));
            }
            Some(size) => size,
        };
        flush = option_in_range(c, options, "flush", bounds, flush)?;
        finish = option_in_range(c, options, "finishFlush", bounds, finish)?;
        max_output = option_in_range(c, options, "maxOutputLength", (1.0, max_output), max_output)?;
        let mut redirected = false;
        for member in ["encoding", "objectMode", "writableObjectMode"] {
            redirected = redirected || super::stream_shared_vm::truthy_member(c, options, member)?;
        }
        if redirected {
            options = assign(c, &[options])?;
            let null = c.null();
            put(c, options, "encoding", null)?;
            for member in ["objectMode", "writableObjectMode"] {
                super::stream_shared_vm::put_bool(c, options, member, false)?;
            }
        }
    }
    let defaults = c.object_rooted()?;
    super::stream_shared_vm::put_bool(c, defaults, "autoDestroy", true)?;
    let transform_options = assign(c, &[defaults, options])?;
    let stream = super::stream_shared_vm::module(c)?;
    let transform = get(c, stream, "Transform")?;
    c.call_rooted(transform, this, &[transform_options])?;
    let error_key = internal(c, Internal::Error)?;
    let null = c.null();
    c.set_property_rooted(this, error_key, null, this)?;
    put_number(c, this, "bytesWritten", 0.0)?;
    put(c, this, "_handle", handle)?;
    let owner = super::net_reference_shared_vm::owner_symbol(c)?;
    c.set_property_rooted(handle, owner, this, handle)?;
    let on_error = internal(c, Internal::OnError)?;
    put(c, handle, "onerror", on_error)?;
    let buffer = allocate(c, chunk_size)?;
    put(c, this, "_outBuffer", buffer)?;
    put_number(c, this, "_outOffset", 0.0)?;
    put_number(c, this, "_chunkSize", chunk_size)?;
    put_number(c, this, "_defaultFlushFlag", flush)?;
    put_number(c, this, "_finishFlushFlag", finish)?;
    put_number(c, this, "_defaultFullFlushFlag", full)?;
    let (info, reject) = if given {
        let info = get(c, options, "info")?;
        let reject = get(c, options, "rejectGarbageAfterEnd")?;
        let yes = c.boolean(true);
        (info, c.strict_equal_rooted(reject, yes)?)
    } else {
        (c.undefined(), false)
    };
    put(c, this, "_info", info)?;
    put_number(c, this, "_maxOutputLength", max_output)?;
    super::stream_shared_vm::put_bool(c, this, "_rejectGarbageAfterEnd", reject)
}

/// `Object.assign({}, ...sources)` as the object spread copies them.
fn assign(c: &mut Context<'_>, sources: &[RootId]) -> Outcome {
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    let target = c.object_rooted()?;
    let mut args = vec![target];
    args.extend_from_slice(sources);
    call_method(c, object, "assign", &args)
}

// ---------------------------------------------------------------------------------------
// Processing
// ---------------------------------------------------------------------------------------

fn byte_length(c: &mut Context<'_>, chunk: RootId) -> Result<f64, RootedError> {
    number(c, chunk, "byteLength")
}

/// `_close(engine)`: close the handle once; a closed engine has none.
fn close_handle(c: &mut Context<'_>, engine: RootId) -> Result<(), RootedError> {
    let handle = get(c, engine, "_handle")?;
    if !is_nullish(c, handle) {
        call_method(c, handle, "close", &[])?;
    }
    let null = c.null();
    put(c, engine, "_handle", null)
}

/// `processChunkSync(self, chunk, flushFlag)`: run the handle until it leaves output room,
/// collecting what it wrote.
fn process_chunk_sync(c: &mut Context<'_>, engine: RootId, chunk: RootId, flush: RootId) -> Outcome {
    let mut available_in = byte_length(c, chunk)?;
    let chunk_size = number(c, engine, "_chunkSize")?;
    let mut offset = number(c, engine, "_outOffset")?;
    let mut available_out = chunk_size - offset;
    let mut in_offset = 0.0;
    let mut buffers = Vec::new();
    let mut read = 0.0;
    let mut input_read = 0.0;
    let state = get(c, engine, "_writeState")?;
    let handle = get(c, engine, "_handle")?;
    let mut buffer = get(c, engine, "_outBuffer")?;
    let undefined = c.undefined();
    let seen = c.array_rooted(&[undefined])?;
    let listener = c.host_function_named_with_data(crate::host::shared_vm::operation!("zlibSyncOnError"), "onError", seen)?;
    super::stream_shared_vm::on(c, engine, "error", listener)?;
    let error_key = internal(c, Internal::Error)?;
    loop {
        let values = [in_offset, available_in, offset, available_out].map(|value| c.number(value));
        call_method(c, handle, "writeSync", &[flush, chunk, values[0], values[1], buffer, values[2], values[3]])?;
        let error = get(c, seen, "0")?;
        if c.truthy_rooted(error)? {
            return Err(c.throw(error));
        }
        let error = c.get_property_rooted(engine, error_key)?;
        if c.truthy_rooted(error)? {
            return Err(c.throw(error));
        }
        let out_after = number(c, state, "0")?;
        let in_after = number(c, state, "1")?;
        let in_delta = available_in - in_after;
        input_read += in_delta;
        let have = available_out - out_after;
        if have > 0.0 {
            let out = slice(c, buffer, offset, have)?;
            offset += have;
            buffers.push(out);
            read += have;
            let maximum = number(c, engine, "_maxOutputLength")?;
            if read > maximum {
                close_handle(c, engine)?;
                return Err(buffer_too_large(c, maximum));
            }
        }
        if out_after == 0.0 || offset >= chunk_size {
            available_out = chunk_size;
            offset = 0.0;
            buffer = allocate(c, chunk_size)?;
        }
        if out_after != 0.0 {
            break;
        }
        in_offset += in_delta;
        available_in = in_after;
    }
    put_number(c, engine, "bytesWritten", input_read)?;
    close_handle(c, engine)?;
    concat(c, &buffers, read)
}

/// The listener `processChunkSync` keeps for the error it reports.
pub(crate) fn sync_on_error(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let error = arg(c, args, 0);
    set_capture(c, 0, error)?;
    Ok(c.undefined())
}

fn slice(c: &mut Context<'_>, buffer: RootId, offset: f64, length: f64) -> Outcome {
    let bounds = [offset, offset + length].map(|value| c.number(value));
    call_method(c, buffer, "slice", &bounds)
}

/// The output as one Buffer: empty, the only piece, or the pieces joined.
fn concat(c: &mut Context<'_>, buffers: &[RootId], length: f64) -> Outcome {
    match buffers {
        [] => empty_buffer(c),
        [only] => Ok(*only),
        _ => {
            let list = c.array_rooted(buffers)?;
            let length = c.number(length);
            let class = buffer_class(c)?;
            call_method(c, class, "concat", &[list, length])
        }
    }
}

/// `ERR_BUFFER_TOO_LARGE(maximum)`.
fn buffer_too_large(c: &mut Context<'_>, maximum: f64) -> RootedError {
    let built = create_buffer_too_large(c, maximum);
    thrown(c, built)
}

fn create_buffer_too_large(c: &mut Context<'_>, maximum: f64) -> Outcome {
    let maximum = c.number(maximum);
    let maximum = c.to_string(maximum)?;
    range_error(c, &format!("Cannot create a Buffer larger than {maximum} bytes"), "ERR_BUFFER_TOO_LARGE")
}

/// `processChunk(self, chunk, flushFlag, cb)`: start an asynchronous write; the handle
/// calls `processCallback` when it completes.
fn process_chunk(c: &mut Context<'_>, engine: RootId, chunk: RootId, flush: RootId, callback: RootId) -> Result<(), RootedError> {
    let handle = get(c, engine, "_handle")?;
    if !c.truthy_rooted(handle)? {
        return next_tick(c, callback, &[]);
    }
    let chunk_size = number(c, engine, "_chunkSize")?;
    let offset = number(c, engine, "_outOffset")?;
    let available_in = byte_length(c, chunk)?;
    put(c, handle, "buffer", chunk)?;
    put(c, handle, "cb", callback)?;
    put_number(c, handle, "availOutBefore", chunk_size - offset)?;
    put_number(c, handle, "availInBefore", available_in)?;
    put_number(c, handle, "inOff", 0.0)?;
    put(c, handle, "flushFlag", flush)?;
    let buffer = get(c, engine, "_outBuffer")?;
    let values = [0.0, available_in, offset, chunk_size - offset].map(|value| c.number(value));
    call_method(c, handle, "write", &[flush, chunk, values[0], values[1], buffer, values[2], values[3]])?;
    Ok(())
}

/// `handle.write(...)` for the input the handle has left, into the stream's output window.
fn continue_write(c: &mut Context<'_>, handle: RootId, engine: RootId) -> Result<(), RootedError> {
    let flush = get(c, handle, "flushFlag")?;
    let input = get(c, handle, "buffer")?;
    let in_offset = get(c, handle, "inOff")?;
    let available_in = get(c, handle, "availInBefore")?;
    let buffer = get(c, engine, "_outBuffer")?;
    let offset = get(c, engine, "_outOffset")?;
    let chunk_size = get(c, engine, "_chunkSize")?;
    call_method(c, handle, "write", &[flush, input, in_offset, available_in, buffer, offset, chunk_size])?;
    Ok(())
}

fn finish_handle_callback(c: &mut Context<'_>, handle: RootId, args: &[RootId]) -> Outcome {
    call_method(c, handle, "cb", args)?;
    Ok(c.undefined())
}

/// `processCallback()`, called by the handle (`this`) when a write completes.
pub(crate) fn process_callback(c: &mut Context<'_>, handle: RootId, _: &[RootId]) -> Outcome {
    let owner = super::net_reference_shared_vm::owner_symbol(c)?;
    let engine = c.get_property_rooted(handle, owner)?;
    let state = get(c, engine, "_writeState")?;
    let null = c.null();
    if super::stream_shared_vm::truthy_member(c, engine, "destroyed")? {
        put(c, handle, "buffer", null)?;
        return finish_handle_callback(c, handle, &[]);
    }
    let out_after = number(c, state, "0")?;
    let in_after = number(c, state, "1")?;
    let in_delta = number(c, handle, "availInBefore")? - in_after;
    let written = number(c, engine, "bytesWritten")?;
    put_number(c, engine, "bytesWritten", written + in_delta)?;
    let have = number(c, handle, "availOutBefore")? - out_after;
    let mut full = false;
    if have > 0.0 {
        let buffer = get(c, engine, "_outBuffer")?;
        let offset = number(c, engine, "_outOffset")?;
        let out = slice(c, buffer, offset, have)?;
        put_number(c, engine, "_outOffset", offset + have)?;
        let pushed = call_method(c, engine, "push", &[out])?;
        full = !c.truthy_rooted(pushed)?;
    }
    if super::stream_shared_vm::truthy_member(c, engine, "destroyed")? {
        return finish_handle_callback(c, handle, &[]);
    }
    let chunk_size = number(c, engine, "_chunkSize")?;
    if out_after == 0.0 || number(c, engine, "_outOffset")? >= chunk_size {
        put_number(c, handle, "availOutBefore", chunk_size)?;
        put_number(c, engine, "_outOffset", 0.0)?;
        let buffer = allocate(c, chunk_size)?;
        put(c, engine, "_outBuffer", buffer)?;
    }
    if out_after == 0.0 {
        let in_offset = number(c, handle, "inOff")?;
        put_number(c, handle, "inOff", in_offset + in_delta)?;
        put_number(c, handle, "availInBefore", in_after)?;
        if full {
            let old_read = get(c, engine, "_read")?;
            let resume = closure(c, "zlibResumeOnRead", "", &[engine, handle, old_read])?;
            put(c, engine, "_read", resume)?;
        } else {
            continue_write(c, handle, engine)?;
        }
        return Ok(c.undefined());
    }
    if in_after > 0.0 {
        if super::stream_shared_vm::truthy_member(c, engine, "_rejectGarbageAfterEnd")? {
            let error = plain_error(
                c,
                "Trailing junk found after the end of the compressed stream",
                "ERR_TRAILING_JUNK_AFTER_STREAM_END",
            )?;
            call_method(c, engine, "destroy", &[error])?;
            return finish_handle_callback(c, handle, &[error]);
        }
        call_method(c, engine, "push", &[null])?;
    }
    put(c, handle, "buffer", null)?;
    finish_handle_callback(c, handle, &[])
}

/// The `_read` a full readable buffer leaves in place: restore the original, continue the
/// write, then read.
pub(crate) fn resume_on_read(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let [engine, handle, old_read] = captures(c)?;
    put(c, engine, "_read", old_read)?;
    continue_write(c, handle, engine)?;
    let size = arg(c, args, 0);
    call_method(c, engine, "_read", &[size])
}

/// `zlibOnError(message, errno, code)`, called by the handle (`this`).
pub(crate) fn on_error(c: &mut Context<'_>, handle: RootId, args: &[RootId]) -> Outcome {
    let (message, errno, code) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let owner = super::net_reference_shared_vm::owner_symbol(c)?;
    let engine = c.get_property_rooted(handle, owner)?;
    let message = c.to_string(message)?;
    let error = c.error_rooted(&message)?;
    put(c, error, "errno", errno)?;
    put(c, error, "code", code)?;
    call_method(c, engine, "destroy", &[error])?;
    let key = internal(c, Internal::Error)?;
    c.set_property_rooted(engine, key, error, engine)?;
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// ZlibBase.prototype and Zlib.prototype
// ---------------------------------------------------------------------------------------

/// `_transform(chunk, encoding, cb)`: a flush chunk carries its own flag, and the last
/// chunk also the finishing one.
pub(crate) fn transform(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, callback) = (arg(c, args, 0), arg(c, args, 2));
    let mut flush = number(c, this, "_defaultFlushFlag")?;
    let key = internal(c, Internal::FlushFlag)?;
    let carried = c.get_property_rooted(chunk, key)?;
    if let Some(flag) = c.rooted_value(carried).and_then(|value| value.as_number()) {
        flush = flag;
    }
    if super::stream_shared_vm::truthy_member(c, this, "writableEnded")?
        && number(c, this, "writableLength")? == byte_length(c, chunk)?
    {
        let finish = number(c, this, "_finishFlushFlag")?;
        flush = max_flush(flush, finish);
    }
    let flush = c.number(flush);
    process_chunk(c, this, chunk, flush, callback)?;
    Ok(c.undefined())
}

/// `_processChunk(chunk, flushFlag, cb)`: the asynchronous form with a callback, else the
/// synchronous one.
pub(crate) fn process_chunk_method(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (chunk, flush, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if is_function(c, callback) {
        process_chunk(c, this, chunk, flush, callback)?;
        Ok(c.undefined())
    } else {
        process_chunk_sync(c, this, chunk, flush)
    }
}

/// `_flush(callback)`: the last, empty chunk.
pub(crate) fn flush_last(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    let empty = empty_buffer(c)?;
    let encoding = c.string_rooted("");
    call_method(c, this, "_transform", &[empty, encoding, callback])
}

/// `_final(callback)`.
pub(crate) fn final_(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[])
}

/// `process.nextTick(callback)`, which rejects what is not a function.
fn tick_callback(c: &mut Context<'_>, callback: RootId) -> Result<(), RootedError> {
    require_function(c, callback, "callback")?;
    next_tick(c, callback, &[])
}

/// `flush([kind, ]callback)`.
pub(crate) fn flush(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (mut kind, mut callback) = (arg(c, args, 0), arg(c, args, 1));
    if is_function(c, kind) || (is_undefined(c, kind) && !c.truthy_rooted(callback)?) {
        callback = kind;
        kind = get(c, this, "_defaultFullFlushFlag")?;
    }
    if super::stream_shared_vm::truthy_member(c, this, "writableFinished")? {
        if c.truthy_rooted(callback)? {
            tick_callback(c, callback)?;
        }
    } else if super::stream_shared_vm::truthy_member(c, this, "writableEnded")? {
        if c.truthy_rooted(callback)? {
            super::stream_shared_vm::once(c, this, "end", callback)?;
        }
    } else {
        let buffers = internal(c, Internal::FlushBuffers)?;
        let chunk = c.get_property_rooted(buffers, kind)?;
        let encoding = c.string_rooted("");
        call_method(c, this, "write", &[chunk, encoding, callback])?;
    }
    Ok(c.undefined())
}

/// `close([callback])`.
pub(crate) fn close(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let callback = arg(c, args, 0);
    if c.truthy_rooted(callback)? {
        let undefined = c.undefined();
        eos::finished(c, undefined, &[this, callback])?;
    }
    call_method(c, this, "destroy", &[])?;
    Ok(c.undefined())
}

/// `_destroy(err, callback)`.
pub(crate) fn destroy(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (error, callback) = (arg(c, args, 0), arg(c, args, 1));
    close_handle(c, this)?;
    let undefined = c.undefined();
    c.call_rooted(callback, undefined, &[error])
}

fn open_handle(c: &mut Context<'_>, this: RootId) -> Outcome {
    let handle = get(c, this, "_handle")?;
    if c.truthy_rooted(handle)? { Ok(handle) } else { Err(internal_assertion(c, Some("zlib binding closed"))) }
}

/// `reset()`.
pub(crate) fn reset(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = open_handle(c, this)?;
    call_method(c, handle, "reset", &[])
}

/// `params(level, strategy, callback)`: change them after a sync flush.
pub(crate) fn params(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let (level, strategy, callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    in_range_or(c, level, "level", (constants::Z_MIN_LEVEL, constants::Z_MAX_LEVEL), f64::NAN)?;
    in_range_or(c, strategy, "strategy", (constants::Z_DEFAULT_STRATEGY, constants::Z_FIXED), f64::NAN)?;
    let current_level = get(c, this, "_level")?;
    let current_strategy = get(c, this, "_strategy")?;
    if !c.strict_equal_rooted(current_level, level)? || !c.strict_equal_rooted(current_strategy, strategy)? {
        let after = closure(c, "zlibParamsAfterFlush", "bound paramsAfterFlushCallback", &[this, level, strategy, callback])?;
        let kind = c.number(constants::Z_SYNC_FLUSH);
        call_method(c, this, "flush", &[kind, after])?;
    } else {
        tick_callback(c, callback)?;
    }
    Ok(c.undefined())
}

/// `paramsAfterFlushCallback(level, strategy, callback)`, bound to the stream.
pub(crate) fn params_after_flush(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let [engine, level, strategy, callback] = captures(c)?;
    let handle = open_handle(c, engine)?;
    call_method(c, handle, "params", &[level, strategy])?;
    if !super::stream_shared_vm::truthy_member(c, engine, "destroyed")? {
        put(c, engine, "_level", level)?;
        put(c, engine, "_strategy", strategy)?;
        if c.truthy_rooted(callback)? {
            let undefined = c.undefined();
            c.call_rooted(callback, undefined, &[])?;
        }
    }
    Ok(c.undefined())
}

/// The `_closed` getter.
pub(crate) fn closed(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    let handle = get(c, this, "_handle")?;
    let open = c.truthy_rooted(handle)?;
    Ok(c.boolean(!open))
}

/// Warn once that `bytesRead` is deprecated; the accessor closes over `[warned]`.
fn warn_bytes_read(c: &mut Context<'_>, message: &str) -> Result<(), RootedError> {
    let [warned] = captures(c)?;
    if c.truthy_rooted(warned)? {
        return Ok(());
    }
    let yes = c.boolean(true);
    set_capture(c, 0, yes)?;
    let global = c.global_root()?;
    let process = get(c, global, "process")?;
    if super::stream_shared_vm::truthy_member(c, process, "noDeprecation")? {
        return Ok(());
    }
    let message = c.string_rooted(message);
    let kind = c.string_rooted("DeprecationWarning");
    let code = c.string_rooted(BYTES_READ_DEPRECATION);
    call_method(c, process, "emitWarning", &[message, kind, code])?;
    Ok(())
}

/// The deprecated `bytesRead` getter: `bytesWritten`.
pub(crate) fn bytes_read(c: &mut Context<'_>, this: RootId, _: &[RootId]) -> Outcome {
    warn_bytes_read(c, BYTES_READ_GET_WARNING)?;
    get(c, this, "bytesWritten")
}

/// The deprecated `bytesRead` setter.
pub(crate) fn set_bytes_read(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    warn_bytes_read(c, BYTES_READ_SET_WARNING)?;
    let value = arg(c, args, 0);
    put(c, this, "bytesWritten", value)?;
    Ok(c.undefined())
}

// ---------------------------------------------------------------------------------------
// One-shot forms
// ---------------------------------------------------------------------------------------

/// `zlibBufferSync(engine, buffer)`.
fn buffer_sync(c: &mut Context<'_>, engine: RootId, buffer: RootId) -> Outcome {
    let buffer = if super::stream_shared_vm::is_string(c, buffer) {
        let bytes = super::buffer_shared_vm::encode_bytes(c, buffer, None)?;
        super::buffer_shared_vm::from_vec(c, bytes)?
    } else if brand(c, buffer, BuiltinBrand::ArrayBufferView)? {
        buffer
    } else if brand(c, buffer, BuiltinBrand::AnyArrayBuffer)? {
        buffer_over(c, &[buffer])?
    } else {
        return Err(invalid_type(
            c,
            buffer,
            "buffer",
            "of type string or an instance of Buffer, TypedArray, DataView, or ArrayBuffer",
        ));
    };
    let finish = get(c, engine, "_finishFlushFlag")?;
    let result = process_chunk_sync(c, engine, buffer, finish)?;
    with_info(c, engine, result)
}

/// The result, or `{ buffer, engine }` for an engine made with `info: true`.
fn with_info(c: &mut Context<'_>, engine: RootId, buffer: RootId) -> Outcome {
    if !super::stream_shared_vm::truthy_member(c, engine, "_info")? {
        return Ok(buffer);
    }
    let result = c.object_rooted()?;
    put(c, result, "buffer", buffer)?;
    put(c, result, "engine", engine)?;
    Ok(result)
}

/// `syncBufferWrapper(buffer, opts)`, closing over the class.
pub(crate) fn sync_wrapper(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let class = c.host_function_data()?;
    let (buffer, options) = (arg(c, args, 0), arg(c, args, 1));
    let engine = c.construct_rooted(class, class, &[options])?;
    buffer_sync(c, engine, buffer)
}

/// `asyncBufferWrapper(buffer, opts, callback)`, closing over the class.
pub(crate) fn async_wrapper(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let class = c.host_function_data()?;
    let (mut buffer, mut options, mut callback) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    if is_function(c, options) {
        callback = options;
        options = c.object_rooted()?;
    }
    let engine = c.construct_rooted(class, class, &[options])?;
    require_function(c, callback, "callback")?;
    if brand(c, buffer, BuiltinBrand::ArrayBufferView)? && !brand(c, buffer, BuiltinBrand::Uint8Array)? {
        let memory = get(c, buffer, "buffer")?;
        let offset = get(c, buffer, "byteOffset")?;
        let length = get(c, buffer, "byteLength")?;
        buffer = buffer_over(c, &[memory, offset, length])?;
    } else if brand(c, buffer, BuiltinBrand::AnyArrayBuffer)? {
        buffer = buffer_over(c, &[buffer])?;
    }
    let null = c.null();
    put(c, engine, "buffers", null)?;
    put_number(c, engine, "nread", 0.0)?;
    put(c, engine, "cb", callback)?;
    for (event, listener) in [("data", Internal::BufferOnData), ("error", Internal::BufferOnError), ("end", Internal::BufferOnEnd)] {
        let listener = internal(c, listener)?;
        super::stream_shared_vm::on(c, engine, event, listener)?;
    }
    call_method(c, engine, "end", &[buffer])?;
    Ok(c.undefined())
}

fn remove_end_listeners(c: &mut Context<'_>, engine: RootId) -> Result<(), RootedError> {
    let end = c.string_rooted("end");
    call_method(c, engine, "removeAllListeners", &[end]).map(drop)
}

/// `zlibBufferOnData(chunk)`.
pub(crate) fn buffer_on_data(c: &mut Context<'_>, engine: RootId, args: &[RootId]) -> Outcome {
    let chunk = arg(c, args, 0);
    let buffers = get(c, engine, "buffers")?;
    if c.truthy_rooted(buffers)? {
        call_method(c, buffers, "push", &[chunk])?;
    } else {
        let buffers = c.array_rooted(&[chunk])?;
        put(c, engine, "buffers", buffers)?;
    }
    let read = number(c, engine, "nread")? + number(c, chunk, "length")?;
    put_number(c, engine, "nread", read)?;
    let maximum = number(c, engine, "_maxOutputLength")?;
    if read > maximum {
        call_method(c, engine, "close", &[])?;
        remove_end_listeners(c, engine)?;
        let error = create_buffer_too_large(c, maximum)?;
        call_method(c, engine, "cb", &[error])?;
    }
    Ok(c.undefined())
}

/// `zlibBufferOnError(err)`.
pub(crate) fn buffer_on_error(c: &mut Context<'_>, engine: RootId, args: &[RootId]) -> Outcome {
    let error = arg(c, args, 0);
    remove_end_listeners(c, engine)?;
    call_method(c, engine, "cb", &[error])?;
    Ok(c.undefined())
}

/// `zlibBufferOnEnd()`.
pub(crate) fn buffer_on_end(c: &mut Context<'_>, engine: RootId, _: &[RootId]) -> Outcome {
    let read = number(c, engine, "nread")?;
    let buffer = if read == 0.0 {
        empty_buffer(c)?
    } else {
        let buffers = get(c, engine, "buffers")?;
        let buffers = c.array_like_values_rooted(buffers)?;
        concat(c, &buffers, read)?
    };
    call_method(c, engine, "close", &[])?;
    let result = with_info(c, engine, buffer)?;
    let null = c.null();
    call_method(c, engine, "cb", &[null, result])?;
    Ok(c.undefined())
}

/// `value(options)`: a `create*` factory, closing over the class.
pub(crate) fn factory(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let class = c.host_function_data()?;
    let options = arg(c, args, 0);
    c.construct_rooted(class, class, &[options])
}

/// `crc32(data, value = 0)`.
pub(crate) fn crc32(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Outcome {
    let data = arg(c, args, 0);
    if !super::stream_shared_vm::is_string(c, data) && !brand(c, data, BuiltinBrand::ArrayBufferView)? {
        return Err(invalid_type(c, data, "data", "of type string or an instance of Buffer, TypedArray, or DataView"));
    }
    let value = arg(c, args, 1);
    let value = if is_undefined(c, value) { c.number(0.0) } else { value };
    let Some(number) = c.rooted_value(value).and_then(|value| value.as_number()) else {
        return Err(invalid_type(c, value, "value", "of type number"));
    };
    if !number.is_finite() || number.fract() != 0.0 {
        return Err(out_of_range(c, "value", "an integer", value));
    }
    if !(0.0..=f64::from(u32::MAX)).contains(&number) {
        return Err(out_of_range(c, "value", &format!(">= 0 && <= {}", u32::MAX), value));
    }
    let undefined = c.undefined();
    super::zlib_native_shared_vm::crc32(c, undefined, &[data, value])
}

// ---------------------------------------------------------------------------------------
// The module
// ---------------------------------------------------------------------------------------

fn method(c: &mut Context<'_>, target: RootId, name: &str, operation: &str, function_name: &str) -> Result<(), RootedError> {
    let function = c.host_function_named(crate::host::shared_vm::operation(operation), function_name)?;
    let key = c.string_rooted(name);
    c.define_data_property_rooted(target, key, function, true, true, true)?;
    Ok(())
}

fn accessor(c: &mut Context<'_>, target: RootId, name: &str, getter: RootId, setter: Option<RootId>) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    let fields = DescriptorFields {
        get: Some(getter),
        set: Some(setter.unwrap_or_else(|| c.undefined())),
        enumerable: Some(true),
        configurable: Some(true),
        ..DescriptorFields::default()
    };
    c.define_property_fields_rooted(target, key, fields)?;
    Ok(())
}

/// A constructor whose prototype and itself inherit from `parent`.
fn subclass(c: &mut Context<'_>, constructor: RootId, parent: RootId) -> Outcome {
    let prototype = get(c, constructor, "prototype")?;
    let parent_prototype = get(c, parent, "prototype")?;
    c.set_prototype_rooted(prototype, parent_prototype)?;
    c.set_prototype_rooted(constructor, parent)?;
    Ok(prototype)
}

fn abstract_class(c: &mut Context<'_>, name: &str, parent: RootId) -> Outcome {
    let constructor = c.host_constructor_named(crate::host::shared_vm::operation!("zlibAbstract"), name)?;
    subclass(c, constructor, parent)?;
    Ok(constructor)
}

/// `ZlibBase`, with the members every compression stream shares.
fn base_class(c: &mut Context<'_>, transform: RootId) -> Outcome {
    let base = abstract_class(c, "ZlibBase", transform)?;
    let prototype = get(c, base, "prototype")?;
    let getter = c.host_function_named(crate::host::shared_vm::operation!("zlibClosed"), "get")?;
    accessor(c, prototype, "_closed", getter, None)?;
    let warned = c.boolean(false);
    let getter = closure(c, "zlibBytesRead", "deprecated", &[warned])?;
    let setter = closure(c, "zlibSetBytesRead", "deprecated", &[warned])?;
    accessor(c, prototype, "bytesRead", getter, Some(setter))?;
    for (name, operation) in [
        ("reset", "zlibReset"),
        ("_flush", "zlibFlushLast"),
        ("_final", "zlibFinal"),
        ("flush", "zlibFlush"),
        ("close", "zlibClose"),
        ("_destroy", "zlibDestroy"),
        ("_transform", "zlibTransform"),
        ("_processChunk", "zlibProcessChunk"),
    ] {
        method(c, prototype, name, operation, "")?;
    }
    Ok(base)
}

pub(crate) fn module(c: &mut Context<'_>) -> Outcome {
    let shared = c.host_mut().shared_state();
    if let Some(module) = shared.borrow().builtin_cache.get(MODULE_CACHE).copied() {
        return Ok(module);
    }
    for which in Internal::ALL {
        let value = which.make(c)?;
        let value = c.retain(value)?;
        shared.borrow_mut().builtin_cache.insert(which.key().to_owned(), value);
    }
    let module = build(c)?;
    let retained = c.retain(module)?;
    shared.borrow_mut().builtin_cache.insert(MODULE_CACHE.to_owned(), retained);
    Ok(module)
}

fn build(c: &mut Context<'_>) -> Outcome {
    let stream = super::stream_shared_vm::module(c)?;
    let transform = get(c, stream, "Transform")?;
    let base = base_class(c, transform)?;
    let zlib = abstract_class(c, "Zlib", base)?;
    let zlib_prototype = get(c, zlib, "prototype")?;
    method(c, zlib_prototype, "params", "zlibParams", "params")?;
    let brotli = abstract_class(c, "Brotli", zlib)?;
    let zstd = abstract_class(c, "Zstd", base)?;
    let module = c.object_rooted()?;
    method(c, module, "crc32", "zlibCrc32", "crc32")?;
    let mut classes = Vec::with_capacity(PUBLIC_CLASSES.len());
    for (name, mode) in PUBLIC_CLASSES {
        let mode_value = c.number(mode as i32 as f64);
        let data = c.array_rooted(&[mode_value])?;
        let constructor =
            c.host_constructor_named_with_data(crate::host::shared_vm::operation!("zlibConstruct"), name, data)?;
        let index = c.number(1.0);
        c.set_property_rooted(data, index, constructor, data)?;
        let parent = match Family::of(mode) {
            Family::Zlib => zlib,
            Family::Brotli => brotli,
            Family::Zstd => zstd,
        };
        subclass(c, constructor, parent)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(module, key, constructor, true, true, true)?;
        classes.push((name, constructor));
    }
    let class_named = |name: &str| classes.iter().find(|(class, _)| *class == name).map(|(_, constructor)| *constructor);
    for (asynchronous, synchronous, class) in CONVENIENCE {
        let class = class_named(class).ok_or_else(|| RootedError::host("zlib convenience class missing"))?;
        for (name, operation, function_name) in
            [(asynchronous, "zlibAsyncWrapper", "asyncBufferWrapper"), (synchronous, "zlibSyncWrapper", "syncBufferWrapper")]
        {
            let function =
                c.host_function_named_with_data(crate::host::shared_vm::operation(operation), function_name, class)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(module, key, function, true, true, true)?;
        }
    }
    for (name, class) in FACTORIES {
        let class = class_named(class).ok_or_else(|| RootedError::host("zlib factory class missing"))?;
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation!("zlibFactory"), "value", class)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(module, key, function, false, true, true)?;
    }
    let constants = constants::constants(c)?;
    let key = c.string_rooted("constants");
    c.define_data_property_rooted(module, key, constants, false, true, false)?;
    let codes = codes(c, constants)?;
    let key = c.string_rooted("codes");
    c.define_data_property_rooted(module, key, codes, false, true, false)?;
    for key in c.own_keys_rooted(constants)? {
        if c.string_text(key)?.is_some_and(|name| name.starts_with(UNEXPORTED_CONSTANTS)) {
            continue;
        }
        let value = c.get_property_rooted(constants, key)?;
        c.define_data_property_rooted(module, key, value, false, false, false)?;
    }
    Ok(module)
}

/// `zlib.codes`: each return code by name and each name by code, frozen.
fn codes(c: &mut Context<'_>, constants: RootId) -> Outcome {
    let codes = c.object_rooted()?;
    let mut values = Vec::with_capacity(CODES.len());
    for name in CODES {
        let value = get(c, constants, name)?;
        put(c, codes, name, value)?;
        values.push((name, value));
    }
    for (name, value) in values {
        let name = c.string_rooted(name);
        c.set_property_rooted(codes, value, name, codes)?;
    }
    let global = c.global_root()?;
    let object = get(c, global, "Object")?;
    call_method(c, object, "freeze", &[codes])?;
    Ok(codes)
}

//! `Blob` and `File` (`lib/internal/blob.js`, `lib/internal/file.js`), and the object
//! URLs `URL.createObjectURL` registers and `buffer.resolveObjectURL` resolves.
//!
//! A Blob's slots are its binding handle (`internalBinding('blob')`), size and type; the
//! handle's parts own the bytes, so slices share them.

use crate::host::NodeHost;
use crate::modules::blob_binding_shared_vm as binding;
use crate::modules::host_members::{captured, current, function, members};
use crate::modules::webstreams_shared_vm::{
    self as web, arg, custom_inspect, deferred, get, install, is_nullish, is_undefined, literal, put, record,
    rejected_with, throw, Brand, ClassMember, WebError,
};
use quench_runtime::{NativeContext, RootId, RootedError};

type Context<'a> = NativeContext<'a, NodeHost>;
type Outcome = Result<RootId, RootedError>;

members!(Member, "blobMembers", [
    BlobConstruct,
    FileConstruct,
    Size,
    Type,
    Slice,
    ArrayBuffer,
    Text,
    Bytes,
    Stream,
    BlobInspect,
    Name,
    LastModified,
    FileInspect,
    ReadStep,
    TextDecode,
    BytesView,
    StreamStart,
    StreamPull,
    StreamCancel,
    StreamStep,
    CreateObjectUrl,
    RevokeObjectUrl,
    ResolveObjectUrl,
]);

/// The `builtin_cache` key of the realm's `[Blob, File]`.
const CLASSES: &str = "\0blob:classes";
/// The interfaces' names, also their `Symbol.toStringTag`s.
const BLOB: &str = "Blob";
const FILE: &str = "File";

/// A Blob's slots.
mod field {
    pub(super) const HANDLE: &str = "handle";
    pub(super) const LENGTH: &str = "length";
    pub(super) const TYPE: &str = "type";
    pub(super) const FILE: &str = "file";
    pub(super) const NAME: &str = "name";
    pub(super) const LAST_MODIFIED: &str = "lastModified";
    /// A read in progress: the chunks left and those read.
    pub(super) const CHUNKS: &str = "chunks";
    pub(super) const NEXT: &str = "next";
    pub(super) const CONTROLLER: &str = "controller";
    pub(super) const PENDING: &str = "pendingPulls";
}
use field::*;

/// The `endings` values Blob accepts.
const TRANSPARENT: &str = "transparent";
const NATIVE: &str = "native";
/// The platform's line ending, which `endings: 'native'` writes.
#[cfg(windows)]
const EOL: &str = "\r\n";
#[cfg(not(windows))]
const EOL: &str = "\n";
/// The printable ASCII range a MIME type may use (`disallowedTypeCharacters`).
const TYPE_CHARACTERS: std::ops::RangeInclusive<u16> = 0x20..=0x7E;
/// A UUID's bytes, version nibble and RFC 4122 variant bits.
const UUID_BYTES: usize = 16;
const UUID_VERSION_BYTE: usize = 6;
const UUID_VARIANT_BYTE: usize = 8;
const UUID_VERSION_4: u8 = 0x40;
const UUID_VERSION_MASK: u8 = 0x0F;
const UUID_VARIANT_RFC4122: u8 = 0x80;
const UUID_VARIANT_MASK: u8 = 0x3F;
const UUID_GROUP_ENDS: [usize; 4] = [4, 6, 8, 10];

const BLOB_MEMBERS: [ClassMember<Member>; 7] = [
    ClassMember::Getter("type", Member::Type),
    ClassMember::Getter("size", Member::Size),
    ClassMember::Method("slice", Member::Slice, 0),
    ClassMember::Method("arrayBuffer", Member::ArrayBuffer, 0),
    ClassMember::Method("text", Member::Text, 0),
    ClassMember::Method("bytes", Member::Bytes, 0),
    ClassMember::Method("stream", Member::Stream, 0),
];

const FILE_MEMBERS: [ClassMember<Member>; 2] =
    [ClassMember::Getter("name", Member::Name), ClassMember::Getter("lastModified", Member::LastModified)];

/// `[Blob, File]`, built once.
pub(crate) fn classes(c: &mut Context<'_>) -> Result<(RootId, RootId), RootedError> {
    let cached = c.host_mut().shared_state().borrow().builtin_cache.get(CLASSES).copied();
    let table = match cached {
        Some(table) => table,
        None => {
            let table = c.with_startup_intrinsics(build)?;
            let retained = c.retain(table)?;
            c.host_mut().shared_state().borrow_mut().builtin_cache.insert(CLASSES.to_owned(), retained);
            table
        }
    };
    Ok((get(c, table, BLOB)?, get(c, table, FILE)?))
}

fn build(c: &mut Context<'_>) -> Outcome {
    let table = record(c, &[])?;
    let blob = class(c, Member::BlobConstruct, BLOB, 0, Member::BlobInspect)?;
    let prototype = get(c, blob, "prototype")?;
    install(c, prototype, &BLOB_MEMBERS)?;
    let file = class(c, Member::FileConstruct, FILE, 2, Member::FileInspect)?;
    let file_prototype = get(c, file, "prototype")?;
    install(c, file_prototype, &FILE_MEMBERS)?;
    c.set_prototype_rooted(file_prototype, prototype)?;
    c.set_prototype_rooted(file, blob)?;
    put(c, table, BLOB, blob)?;
    put(c, table, FILE, file)?;
    Ok(table)
}

fn class(c: &mut Context<'_>, member: Member, name: &str, length: u32, inspect: Member) -> Outcome {
    let constructor = crate::modules::host_members::constructor(c, member, name)?;
    web::set_length(c, constructor, length)?;
    let prototype = get(c, constructor, "prototype")?;
    c.define_named_data_property_rooted(constructor, "prototype", prototype, false, false, false)?;
    let global = c.global_root()?;
    let symbol = get(c, global, "Symbol")?;
    let tag = get(c, symbol, "toStringTag")?;
    let value = c.string_rooted(name);
    c.define_data_property_rooted(prototype, tag, value, false, false, true)?;
    let registry = get(c, symbol, "for")?;
    let description = c.string_rooted("nodejs.util.inspect.custom");
    let key = c.call_rooted(registry, symbol, &[description])?;
    let function = web::method_function(c, inspect, "[nodejs.util.inspect.custom]", 2)?;
    c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    Ok(constructor)
}

/// The private slot holding a Blob's record.
const SLOT: &str = "\0slot:Blob";

fn slot(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    crate::modules::private_slots::read(c, value, SLOT)
}

pub(crate) fn is_blob(c: &mut Context<'_>, value: RootId) -> Result<bool, RootedError> {
    Ok(slot(c, value)?.is_some())
}

fn this_blob(c: &mut Context<'_>, this: RootId) -> Outcome {
    match slot(c, this)? {
        Some(state) => Ok(state),
        None => Err(throw(c, WebError::InvalidThis(BLOB))),
    }
}

/// A MIME type as Blob keeps it: lowercase, or empty when it has non-printable characters.
fn normalize_type(c: &mut Context<'_>, value: RootId) -> Outcome {
    let units = c.to_string_code_units(value)?;
    if units.iter().any(|unit| !TYPE_CHARACTERS.contains(unit)) {
        return Ok(c.string_rooted(""));
    }
    Ok(c.string_rooted(&String::from_utf16_lossy(&units).to_ascii_lowercase()))
}

/// The Blob slots over `handle`.
fn blob_record(c: &mut Context<'_>, handle: RootId, length: f64, kind: RootId) -> Outcome {
    let length = c.number(length);
    record(c, &[(HANDLE, handle), (LENGTH, length), (TYPE, kind)])
}

/// `createBlob(handle, length, type)`: a new Blob over `handle`.
pub(crate) fn from_handle(c: &mut Context<'_>, handle: RootId, length: f64, kind: RootId) -> Outcome {
    let (blob, _) = classes(c)?;
    let prototype = get(c, blob, "prototype")?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let state = blob_record(c, handle, length, kind)?;
    crate::modules::private_slots::write(c, object, SLOT, state)?;
    Ok(object)
}

/// A Blob of `bytes` with `type`.
pub(crate) fn from_bytes(c: &mut Context<'_>, bytes: Vec<u8>, kind: &str) -> Outcome {
    let length = bytes.len() as f64;
    let view = c.uint8_array_from_rooted(bytes)?;
    let handle = binding::create_handle(c, &[view])?;
    let kind = c.string_rooted(kind);
    from_handle(c, handle, length, kind)
}

/// The bytes of a Blob's parts, in order.
pub(crate) fn chunks(c: &mut Context<'_>, blob: RootId) -> Result<Vec<Vec<u8>>, RootedError> {
    let state = slot(c, blob)?.ok_or_else(|| RootedError::host("Blob expected"))?;
    let handle = get(c, state, HANDLE)?;
    binding::handle_chunks(c, handle)
}

/// A Blob's `type`.
pub(crate) fn type_of(c: &mut Context<'_>, blob: RootId) -> Result<String, RootedError> {
    let state = slot(c, blob)?.ok_or_else(|| RootedError::host("Blob expected"))?;
    let kind = get(c, state, TYPE)?;
    c.to_string(kind)
}

/// `getSource(source, endings)`: the source as handle-ready bytes or a Blob's handle.
fn source(c: &mut Context<'_>, source: RootId, native: bool) -> Result<(f64, RootId), RootedError> {
    if let Some(state) = slot(c, source)? {
        let length = web::num(c, state, LENGTH)?;
        return Ok((length, get(c, state, HANDLE)?));
    }
    let bytes = if c.has_builtin_brand_rooted(source, quench_runtime::BuiltinBrand::AnyArrayBuffer)?
        || c.has_builtin_brand_rooted(source, quench_runtime::BuiltinBrand::ArrayBufferView)?
    {
        let parts = web::view_parts(c, source)?;
        match parts {
            Some(parts) => c
                .with_buffer_bytes_rooted(parts.buffer, |bytes| bytes[parts.offset..parts.offset + parts.length].to_vec())
                .unwrap_or_default(),
            None => c.with_buffer_bytes_rooted(source, <[u8]>::to_vec).unwrap_or_default(),
        }
    } else {
        let units = c.to_string_code_units(source)?;
        let mut text = String::from_utf16_lossy(&units);
        if native {
            text = text.replace("\r\n", "\n").replace('\n', EOL);
        }
        text.into_bytes()
    };
    let length = bytes.len() as f64;
    Ok((length, c.uint8_array_from_rooted(bytes)?))
}

/// `new Blob(sources, options)` on `this`.
fn construct_blob(c: &mut Context<'_>, this: RootId, sources: RootId, options: RootId) -> Result<RootId, RootedError> {
    let sources = if is_undefined(c, sources) { c.array_rooted(&[])? } else { sources };
    let is_string = matches!(c.value_type_rooted(sources), Some(quench_runtime::ValueType::String));
    let iterable = !is_nullish(c, sources) && !is_string && {
        let global = c.global_root()?;
        let symbol = get(c, global, "Symbol")?;
        let iterator = get(c, symbol, "iterator")?;
        let method = c.get_property_rooted(sources, iterator)?;
        c.is_callable_rooted(method)?
    };
    if !iterable {
        let error = crate::modules::util_shared_vm::argument_type_error_with_expected(c, sources, "sources", "a sequence")?;
        return Err(error);
    }
    validate_dictionary(c, options, "options")?;
    let (kind, endings) = if is_nullish(c, options) {
        (c.string_rooted(""), c.string_rooted(TRANSPARENT))
    } else {
        let kind = get(c, options, TYPE)?;
        let endings = get(c, options, "endings")?;
        (
            if is_undefined(c, kind) { c.string_rooted("") } else { kind },
            if is_undefined(c, endings) { c.string_rooted(TRANSPARENT) } else { endings },
        )
    };
    let endings_text = c.to_string(endings)?;
    if endings_text != TRANSPARENT && endings_text != NATIVE {
        let endings = c.string_rooted(&endings_text);
        return Err(throw(c, WebError::ArgValue("options.endings", endings)));
    }
    let mut total = 0.0;
    let mut parts = Vec::new();
    let mut iterator = c.iterator_rooted(sources)?;
    while let Some(item) = c.iterator_step_rooted(&mut iterator)? {
        let (length, part) = source(c, item, endings_text == NATIVE)?;
        total += length;
        parts.push(part);
    }
    let handle = binding::create_handle(c, &parts)?;
    let kind = normalize_type(c, kind)?;
    let state = blob_record(c, handle, total, kind)?;
    crate::modules::private_slots::write(c, this, SLOT, state)?;
    Ok(state)
}

/// Node's `validateDictionary`: `null`, `undefined`, objects and functions.
fn validate_dictionary(c: &mut Context<'_>, value: RootId, name: &str) -> Result<(), RootedError> {
    if is_nullish(c, value) || c.is_object_rooted(value)? || c.is_callable_rooted(value)? {
        return Ok(());
    }
    Err(crate::modules::util_shared_vm::argument_type_error_with_expected(c, value, name, "a dictionary")?)
}

/// A slice bound as `Blob.prototype.slice` clamps it: relative to the end when negative.
fn clamp(c: &mut Context<'_>, value: RootId, length: f64, default: f64) -> Result<f64, RootedError> {
    let number = if is_undefined(c, value) { default } else { c.to_number(value)? };
    let clamped = if number < 0.0 { (length + number).max(0.0) } else { number.min(length) };
    Ok(if clamped.is_nan() { 0.0 } else { clamped.trunc() })
}

fn slice(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let state = this_blob(c, this)?;
    let length = web::num(c, state, LENGTH)?;
    let (start, end, kind) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
    let start = clamp(c, start, length, 0.0)?;
    let end = clamp(c, end, length, length)?;
    let kind = if is_undefined(c, kind) { c.string_rooted("") } else { kind };
    let kind = normalize_type(c, kind)?;
    let span = (end - start).max(0.0);
    let handle = get(c, state, HANDLE)?;
    let sliced = binding::slice_handle(c, handle, start as usize, (start + span) as usize)?;
    from_handle(c, sliced, span, kind)
}

/// `arrayBuffer()`: the parts are read one per microtask, as the binding's reader delivers
/// them, and joined.
fn array_buffer(c: &mut Context<'_>, state: RootId) -> Outcome {
    let promise = deferred(c)?;
    let handle = get(c, state, HANDLE)?;
    let chunks = binding::handle_chunks(c, handle)?;
    let views = chunks.into_iter().map(|chunk| c.uint8_array_from_rooted(chunk)).collect::<Result<Vec<_>, _>>()?;
    let views = c.array_rooted(&views)?;
    let zero = c.number(0.0);
    let read = record(c, &[(CHUNKS, views), (NEXT, zero)])?;
    read_step(c, read, promise)?;
    Ok(promise)
}

fn read_step(c: &mut Context<'_>, read: RootId, promise: RootId) -> Result<(), RootedError> {
    let views = get(c, read, CHUNKS)?;
    let views = c.array_like_values_rooted(views)?;
    let next = web::num(c, read, NEXT)? as usize;
    if next >= views.len() {
        let mut bytes = Vec::new();
        for view in views {
            bytes.extend(c.view_bytes_rooted(view).unwrap_or_default());
        }
        let joined = c.uint8_array_from_rooted(bytes)?;
        let buffer = get(c, joined, "buffer")?;
        return c.resolve_promise_rooted(promise, buffer);
    }
    web::put_num(c, read, NEXT, (next + 1) as f64)?;
    let task = function(c, Member::ReadStep, "", &[read, promise])?;
    c.queue_microtask_rooted(task, &[])
}

/// `stream()`: a byte stream over the parts, pulled one part per microtask while the
/// stream wants more.
fn stream(c: &mut Context<'_>, state: RootId) -> Outcome {
    let handle = get(c, state, HANDLE)?;
    let chunks = binding::handle_chunks(c, handle)?;
    let views = chunks.into_iter().map(|chunk| c.uint8_array_from_rooted(chunk)).collect::<Result<Vec<_>, _>>()?;
    let views = c.array_rooted(&views)?;
    let zero = c.number(0.0);
    let pending = c.array_rooted(&[])?;
    let read = record(c, &[(CHUNKS, views), (NEXT, zero), (PENDING, pending)])?;
    let source = c.object_rooted()?;
    let kind = c.string_rooted("bytes");
    put(c, source, "type", kind)?;
    for (name, member) in [("start", Member::StreamStart), ("pull", Member::StreamPull), ("cancel", Member::StreamCancel)] {
        let method = function(c, member, name, &[read])?;
        put(c, source, name, method)?;
    }
    let strategy = c.object_rooted()?;
    let zero = c.number(0.0);
    put(c, strategy, "highWaterMark", zero)?;
    let classes = web::classes(c)?;
    let readable = get(c, classes, Brand::ReadableStream.name())?;
    c.construct_rooted(readable, readable, &[source, strategy])
}

fn pending_pulls(c: &mut Context<'_>, read: RootId) -> Result<Vec<RootId>, RootedError> {
    let pending = get(c, read, PENDING)?;
    c.array_like_values_rooted(pending)
}

fn set_pending_pulls(c: &mut Context<'_>, read: RootId, pulls: &[RootId]) -> Result<(), RootedError> {
    let pending = c.array_rooted(pulls)?;
    put(c, read, PENDING, pending)
}

/// One `reader.pull` of the stream's `readNext`.
fn stream_step(c: &mut Context<'_>, read: RootId) -> Result<(), RootedError> {
    let mut pulls = pending_pulls(c, read)?;
    if pulls.is_empty() {
        return Ok(());
    }
    let controller = get(c, read, CONTROLLER)?;
    let views = get(c, read, CHUNKS)?;
    let views = c.array_like_values_rooted(views)?;
    let next = web::num(c, read, NEXT)? as usize;
    let Some(&view) = views.get(next) else {
        call(c, controller, "close", &[])?;
        let request = get(c, controller, "byobRequest")?;
        if !is_nullish(c, request) {
            let zero = c.number(0.0);
            call(c, request, "respond", &[zero])?;
        }
        let first = pulls.remove(0);
        set_pending_pulls(c, read, &pulls)?;
        return web::settle_undefined(c, first);
    };
    web::put_num(c, read, NEXT, (next + 1) as f64)?;
    if c.view_bytes_rooted(view).is_some_and(|bytes| !bytes.is_empty()) {
        call(c, controller, "enqueue", &[view])?;
    }
    let task = function(c, Member::StreamStep, "", &[read])?;
    c.queue_microtask_rooted(task, &[])
}

fn call(c: &mut Context<'_>, object: RootId, method: &str, args: &[RootId]) -> Outcome {
    let function = get(c, object, method)?;
    c.call_rooted(function, object, args)
}

fn inspect_blob(c: &mut Context<'_>, this: RootId, args: &[RootId], file: bool) -> Outcome {
    let state = this_blob(c, this)?;
    let size = get(c, state, LENGTH)?;
    let kind = get(c, state, TYPE)?;
    let data = if file {
        let name = get(c, state, NAME)?;
        let modified = get(c, state, LAST_MODIFIED)?;
        literal(c, &[("size", size), ("type", kind), ("name", name), ("lastModified", modified)])?
    } else {
        literal(c, &[("size", size), ("type", kind)])?
    };
    custom_inspect(c, this, if file { FILE } else { BLOB }, data, args)
}

fn file_state(c: &mut Context<'_>, this: RootId) -> Outcome {
    match slot(c, this)? {
        Some(state) if web::flag(c, state, field::FILE)? => Ok(state),
        _ => Err(throw(c, WebError::InvalidThis(FILE))),
    }
}

/// A fresh `blob:nodedata:` id: an RFC 4122 version 4 UUID.
fn random_uuid() -> String {
    let mut bytes = [0u8; UUID_BYTES];
    let filled = std::fs::File::open("/dev/urandom").and_then(|mut source| std::io::Read::read_exact(&mut source, &mut bytes));
    if filled.is_err() {
        use std::hash::{BuildHasher, Hasher};
        for chunk in bytes.chunks_mut(std::mem::size_of::<u64>()) {
            let mut hasher = std::collections::hash_map::RandomState::new().build_hasher();
            hasher.write_usize(chunk.as_ptr() as usize);
            chunk.copy_from_slice(&hasher.finish().to_le_bytes()[..chunk.len()]);
        }
    }
    bytes[UUID_VERSION_BYTE] = (bytes[UUID_VERSION_BYTE] & UUID_VERSION_MASK) | UUID_VERSION_4;
    bytes[UUID_VARIANT_BYTE] = (bytes[UUID_VARIANT_BYTE] & UUID_VARIANT_MASK) | UUID_VARIANT_RFC4122;
    let mut text = String::with_capacity(UUID_BYTES * 2 + UUID_GROUP_ENDS.len());
    for (index, byte) in bytes.iter().enumerate() {
        if UUID_GROUP_ENDS.contains(&index) {
            text.push('-');
        }
        text.push_str(&format!("{byte:02x}"));
    }
    text
}

/// A FormData entry's File for the Blob `value`: the File itself when no `filename` is
/// given, else a File over the same bytes named `filename` (or `blob` for a plain Blob).
pub(crate) fn as_file(c: &mut Context<'_>, value: RootId, filename: Option<RootId>) -> Outcome {
    let state = slot(c, value)?.ok_or_else(|| RootedError::host("Blob expected"))?;
    let is_file = web::flag(c, state, field::FILE)?;
    if is_file && filename.is_none() {
        return Ok(value);
    }
    let name = match filename {
        Some(name) => {
            let units = c.to_string_code_units(name)?;
            c.string_rooted(&String::from_utf16_lossy(&units))
        }
        None => c.string_rooted("blob"),
    };
    let modified = if is_file { get(c, state, LAST_MODIFIED)? } else { c.number(now()) };
    let handle = get(c, state, HANDLE)?;
    let length = web::num(c, state, LENGTH)?;
    let kind = get(c, state, TYPE)?;
    let file = from_handle(c, handle, length, kind)?;
    let (_, file_class) = classes(c)?;
    let prototype = get(c, file_class, "prototype")?;
    c.set_prototype_rooted(file, prototype)?;
    let copy = slot(c, file)?.ok_or_else(|| RootedError::host("File lost its slots"))?;
    put(c, copy, NAME, name)?;
    put(c, copy, LAST_MODIFIED, modified)?;
    web::put_flag(c, copy, field::FILE, true)?;
    Ok(file)
}

/// `Date.now()`.
fn now() -> f64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|time| time.as_millis() as f64).unwrap_or(0.0)
}

/// `structuredClone(blob)`: a Blob over the same immutable handle, or `None` for any
/// other value.
pub(crate) fn clone(c: &mut Context<'_>, value: RootId) -> Result<Option<RootId>, RootedError> {
    let Some(state) = slot(c, value)? else { return Ok(None) };
    let handle = get(c, state, HANDLE)?;
    let length = web::num(c, state, LENGTH)?;
    let kind = get(c, state, TYPE)?;
    let blob = from_handle(c, handle, length, kind)?;
    if web::flag(c, state, field::FILE)? {
        let (_, file) = classes(c)?;
        let prototype = get(c, file, "prototype")?;
        c.set_prototype_rooted(blob, prototype)?;
        let copy = slot(c, blob)?.ok_or_else(|| RootedError::host("cloned Blob lost its slots"))?;
        for name in [NAME, LAST_MODIFIED] {
            let value = get(c, state, name)?;
            put(c, copy, name, value)?;
        }
        web::put_flag(c, copy, field::FILE, true)?;
    }
    Ok(Some(blob))
}

/// `buffer.resolveObjectURL(url)`.
pub(crate) fn resolve_object_url(c: &mut Context<'_>, url: RootId) -> Outcome {
    let url = c.to_string(url)?;
    let Some(id) = binding::object_url_id(&url) else { return Ok(c.undefined()) };
    let id = c.string_rooted(&id);
    let entry = binding::lookup(c, id)?;
    if is_undefined(c, entry) {
        return Ok(entry);
    }
    let handle = get(c, entry, "0")?;
    let length = get(c, entry, "1")?;
    let length = c.to_number(length)?;
    let kind = get(c, entry, "2")?;
    from_handle(c, handle, length, kind)
}

/// Defines `URL.createObjectURL` and `URL.revokeObjectURL` on the URL class.
pub(crate) fn install_url_statics(c: &mut Context<'_>, url: RootId) -> Result<(), RootedError> {
    for (name, member) in [("createObjectURL", Member::CreateObjectUrl), ("revokeObjectURL", Member::RevokeObjectUrl)] {
        let method = web::method_function(c, member, name, 1)?;
        let key = c.string_rooted(name);
        c.define_data_property_rooted(url, key, method, true, true, true)?;
    }
    Ok(())
}

/// `buffer.resolveObjectURL`.
pub(crate) fn resolve_object_url_function(c: &mut Context<'_>) -> Outcome {
    web::method_function(c, Member::ResolveObjectUrl, "resolveObjectURL", 1)
}

/// The lazy `Blob` and `File` globals' initializer.
pub(crate) fn initialize_globals(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Outcome {
    let (blob, file) = classes(c)?;
    let global = c.global_root()?;
    for (name, class) in [(BLOB, blob), (FILE, file)] {
        let key = c.string_rooted(name);
        c.define_data_property_rooted(global, key, class, true, false, true)?;
    }
    Ok(c.undefined())
}

/// The globals `initialize_globals` defines.
pub(crate) const GLOBALS: [&str; 2] = [BLOB, FILE];

pub(crate) fn member(c: &mut Context<'_>, this: RootId, args: &[RootId]) -> Outcome {
    let member: Member = current(c)?;
    match member {
        Member::BlobConstruct => {
            web::require_new(c, BLOB)?;
            let (sources, options) = (arg(c, args, 0), arg(c, args, 1));
            construct_blob(c, this, sources, options)?;
            Ok(c.undefined())
        }
        Member::FileConstruct => {
            web::require_new(c, FILE)?;
            if args.len() < 2 {
                let error = c.type_error_rooted("The \"fileBits\" and \"fileName\" arguments must be specified")?;
                crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_MISSING_ARGS")?;
                return Err(c.throw(error));
            }
            let (bits, name, options) = (arg(c, args, 0), arg(c, args, 1), arg(c, args, 2));
            let state = construct_blob(c, this, bits, options)?;
            let modified = if is_nullish(c, options) { c.undefined() } else { get(c, options, LAST_MODIFIED)? };
            let modified = if is_undefined(c, modified) {
                c.number(now())
            } else {
                let value = c.to_number(modified)?;
                c.number(if value.is_nan() { 0.0 } else { value })
            };
            let units = c.to_string_code_units(name)?;
            let name = c.string_rooted(&String::from_utf16_lossy(&units));
            put(c, state, NAME, name)?;
            put(c, state, LAST_MODIFIED, modified)?;
            web::put_flag(c, state, field::FILE, true)?;
            Ok(c.undefined())
        }
        Member::Size | Member::Type => {
            let state = this_blob(c, this)?;
            get(c, state, if member == Member::Size { LENGTH } else { TYPE })
        }
        Member::Slice => slice(c, this, args),
        Member::ArrayBuffer => match slot(c, this)? {
            Some(state) => array_buffer(c, state),
            None => rejected_with(c, WebError::InvalidThis(BLOB)),
        },
        Member::Text => match slot(c, this)? {
            Some(state) => {
                let buffer = array_buffer(c, state)?;
                web::upon(c, buffer, Some(Member::TextDecode), None, &[])
            }
            None => rejected_with(c, WebError::InvalidThis(BLOB)),
        },
        Member::Bytes => {
            let state = this_blob(c, this)?;
            let buffer = array_buffer(c, state)?;
            web::upon(c, buffer, Some(Member::BytesView), None, &[])
        }
        Member::Stream => {
            let state = this_blob(c, this)?;
            stream(c, state)
        }
        Member::BlobInspect => inspect_blob(c, this, args, false),
        Member::FileInspect => inspect_blob(c, this, args, true),
        Member::Name | Member::LastModified => {
            let state = file_state(c, this)?;
            get(c, state, if member == Member::Name { NAME } else { LAST_MODIFIED })
        }
        Member::ReadStep => {
            let [read, promise] = captured(c)?;
            read_step(c, read, promise)?;
            Ok(c.undefined())
        }
        Member::TextDecode => {
            let buffer = arg(c, args, 0);
            let bytes = c.with_buffer_bytes_rooted(buffer, <[u8]>::to_vec).unwrap_or_default();
            let text = String::from_utf8_lossy(&bytes);
            let text = text.strip_prefix('\u{feff}').unwrap_or(&text);
            Ok(c.string_rooted(text))
        }
        Member::BytesView => {
            let buffer = arg(c, args, 0);
            let length = web::buffer_length(c, buffer);
            web::uint8_view(c, buffer, 0, length)
        }
        Member::StreamStart => {
            let [read] = captured(c)?;
            let controller = arg(c, args, 0);
            put(c, read, CONTROLLER, controller)?;
            Ok(c.undefined())
        }
        Member::StreamPull => {
            let [read] = captured(c)?;
            let promise = deferred(c)?;
            let mut pulls = pending_pulls(c, read)?;
            pulls.push(promise);
            set_pending_pulls(c, read, &pulls)?;
            stream_step(c, read)?;
            Ok(promise)
        }
        Member::StreamCancel => {
            let [read] = captured(c)?;
            let reason = arg(c, args, 0);
            for pending in pending_pulls(c, read)? {
                web::fail(c, pending, reason)?;
            }
            set_pending_pulls(c, read, &[])?;
            Ok(c.undefined())
        }
        Member::StreamStep => {
            let [read] = captured(c)?;
            let controller = get(c, read, CONTROLLER)?;
            let desired = get(c, controller, "desiredSize")?;
            if c.to_number(desired)? < 0.0 {
                let mut pulls = pending_pulls(c, read)?;
                if !pulls.is_empty() {
                    let first = pulls.remove(0);
                    set_pending_pulls(c, read, &pulls)?;
                    web::settle_undefined(c, first)?;
                }
                return Ok(c.undefined());
            }
            stream_step(c, read)?;
            Ok(c.undefined())
        }
        Member::CreateObjectUrl => {
            let object = arg(c, args, 0);
            let Some(state) = slot(c, object)? else {
                return Err(throw(c, WebError::ArgType("obj", BLOB, object)));
            };
            let id = random_uuid();
            let key = c.string_rooted(&id);
            let handle = get(c, state, HANDLE)?;
            let length = get(c, state, LENGTH)?;
            let kind = get(c, state, TYPE)?;
            binding::store(c, key, &[handle, length, kind])?;
            Ok(c.string_rooted(&binding::object_url(&id)))
        }
        Member::RevokeObjectUrl => {
            if args.is_empty() {
                let error = c.type_error_rooted("The \"url\" argument must be specified")?;
                crate::modules::node_errors_shared_vm::decorate_type_error(c, error, "ERR_MISSING_ARGS")?;
                return Err(c.throw(error));
            }
            let url = arg(c, args, 0);
            let url = c.to_string(url)?;
            let url = c.string_rooted(&url);
            let undefined = c.undefined();
            binding::revoke_object_url(c, undefined, &[url])
        }
        Member::ResolveObjectUrl => {
            let url = arg(c, args, 0);
            resolve_object_url(c, url)
        }
    }
}

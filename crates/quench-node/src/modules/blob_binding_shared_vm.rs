//! `internalBinding('blob')` (src/node_blob.cc): the byte handles behind Node's
//! lib/internal/blob.js and the `blob:nodedata:` object URL registry.
//!
//! A handle holds its parts (buffers and the ranges they cover) under a private symbol,
//! so slices share bytes and the collector owns their lifetime. Every part of a
//! Quench Blob is memory-resident, so a reader answers each pull synchronously.
use crate::host::NodeHost;
use quench_runtime::{BuiltinBrand, NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

/// `bob::Status` values a reader reports to its pull callback.
const STATUS_EOS: f64 = 0.0;
const STATUS_CONTINUE: f64 = 1.0;
/// The scheme and opaque-path base of the URLs `URL.createObjectURL` makes.
const BLOB_SCHEME: &str = "blob";
const NODE_DATA: &str = "nodedata";

/// The slots of the binding's private data.
mod data {
    pub(super) const HANDLE_PROTOTYPE: &str = "0";
    pub(super) const READER_PROTOTYPE: &str = "1";
    pub(super) const STORE: &str = "2";
    pub(super) const BYTES: &str = "3";
    pub(super) const MAP_GET: &str = "4";
    pub(super) const MAP_SET: &str = "5";
    pub(super) const MAP_DELETE: &str = "6";
}

/// A handle's private state is its list of parts, each `[buffer, start, end]` (one per
/// source, as a DataQueue keeps its entries); a reader's adds the next part to deliver.
mod state {
    pub(super) const PARTS: &str = "0";
    pub(super) const NEXT: &str = "1";
    pub(super) const BUFFER: &str = "0";
    pub(super) const START: &str = "1";
    pub(super) const END: &str = "2";
}

const NATIVES: &[(&str, &str)] = &[
    ("createBlob", "blobBindingCreate"),
    ("storeDataObject", "blobBindingStoreDataObject"),
    ("getDataObject", "blobBindingGetDataObject"),
    ("revokeObjectURL", "blobBindingRevokeObjectUrl"),
    ("concat", "blobBindingConcat"),
    ("createBlobFromFilePath", "blobBindingCreateFromFilePath"),
];

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

fn put(c: &mut Context<'_>, object: RootId, name: &str, value: RootId) -> Result<(), RootedError> {
    let key = c.string_rooted(name);
    c.define_data_property_rooted(object, key, value, true, true, true)?;
    Ok(())
}

/// A prototype whose `constructor` is an anonymous class named `name` with `methods`.
fn native_prototype(c: &mut Context<'_>, name: &str, methods: &[(&str, &str)], data: RootId) -> Result<RootId, RootedError> {
    let constructor = c.evaluate_script_rooted(&format!("(class {name} {{}})"), "node:internal/blob/handle.js")?;
    let prototype = get(c, constructor, "prototype")?;
    for &(method, operation) in methods {
        let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), method, data)?;
        let key = c.string_rooted(method);
        c.define_data_property_rooted(prototype, key, function, true, false, true)?;
    }
    Ok(prototype)
}

pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:blob", |c| {
        c.with_startup_intrinsics(|c| {
            let data = c.array_rooted(&[])?;
            let handle = native_prototype(c, "Blob", &[("getReader", "blobHandleGetReader"), ("slice", "blobHandleSlice")], data)?;
            put(c, data, data::HANDLE_PROTOTYPE, handle)?;
            let reader = native_prototype(c, "BlobReader", &[("pull", "blobReaderPull"), ("setWakeup", "blobReaderSetWakeup")], data)?;
            put(c, data, data::READER_PROTOTYPE, reader)?;
            let global = c.global_root()?;
            let map = get(c, global, "Map")?;
            let store = c.construct_rooted(map, map, &[])?;
            put(c, data, data::STORE, store)?;
            let map_prototype = get(c, map, "prototype")?;
            for (slot, method) in [(data::MAP_GET, "get"), (data::MAP_SET, "set"), (data::MAP_DELETE, "delete")] {
                let function = get(c, map_prototype, method)?;
                put(c, data, slot, function)?;
            }
            let bytes = c.private_symbol_rooted(Some("node:blob:bytes"));
            put(c, data, data::BYTES, bytes)?;
            let binding = c.object_rooted()?;
            for &(name, operation) in NATIVES {
                let function = c.host_function_named_with_data(crate::host::shared_vm::operation(operation), name, data)?;
                put(c, binding, name, function)?;
            }
            Ok(binding)
        })
    })
}

fn arg(c: &mut Context<'_>, args: &[RootId], index: usize) -> RootId {
    match args.get(index) {
        Some(value) => *value,
        None => c.undefined(),
    }
}

fn number(c: &mut Context<'_>, value: RootId) -> Result<usize, RootedError> {
    let number = c.to_number(value)?;
    Ok(if number.is_finite() && number > 0.0 { number as usize } else { 0 })
}

/// A new object of `prototype` whose private state is `values`.
fn with_state(c: &mut Context<'_>, prototype_slot: &str, values: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let prototype = get(c, data, prototype_slot)?;
    let object = c.object_rooted()?;
    c.set_prototype_rooted(object, prototype)?;
    let state = c.array_rooted(values)?;
    let key = get(c, data, data::BYTES)?;
    c.define_data_property_rooted(object, key, state, false, false, false)?;
    Ok(object)
}

/// The private state of a handle or reader, or `None` for any other value.
fn state_of(c: &mut Context<'_>, object: RootId) -> Result<Option<RootId>, RootedError> {
    if !c.is_object_rooted(object)? {
        return Ok(None);
    }
    let data = c.host_function_data()?;
    let key = get(c, data, data::BYTES)?;
    let state = c.get_property_rooted(object, key)?;
    Ok(c.is_object_rooted(state)?.then_some(state))
}

/// One part: a buffer and the byte range of it the part covers.
struct Part {
    buffer: RootId,
    start: usize,
    end: usize,
}

fn length(c: &mut Context<'_>, array: RootId) -> Result<usize, RootedError> {
    let length = get(c, array, "length")?;
    number(c, length)
}

/// A handle's parts.
fn parts(c: &mut Context<'_>, state: RootId) -> Result<Vec<Part>, RootedError> {
    let list = get(c, state, state::PARTS)?;
    let count = length(c, list)?;
    let mut parts = Vec::with_capacity(count);
    for index in 0..count {
        let part = get(c, list, &index.to_string())?;
        let buffer = get(c, part, state::BUFFER)?;
        let start = get(c, part, state::START)?;
        let start = number(c, start)?;
        let end = get(c, part, state::END)?;
        let end = number(c, end)?;
        parts.push(Part { buffer, start, end });
    }
    Ok(parts)
}

/// A handle over `parts`, dropping empty ones.
fn handle(c: &mut Context<'_>, parts: &[Part]) -> Result<RootId, RootedError> {
    let mut list = Vec::with_capacity(parts.len());
    for part in parts.iter().filter(|part| part.end > part.start) {
        let start = c.number(part.start as f64);
        let end = c.number(part.end as f64);
        list.push(c.array_rooted(&[part.buffer, start, end])?);
    }
    let list = c.array_rooted(&list)?;
    with_state(c, data::HANDLE_PROTOTYPE, &[list])
}

fn part_bytes(c: &mut Context<'_>, part: &Part) -> Vec<u8> {
    c.with_buffer_bytes_rooted(part.buffer, |bytes| bytes.get(part.start..part.end).unwrap_or_default().to_vec()).unwrap_or_default()
}

/// `createBlob(sources, length)`: a handle over the sources' parts, each source a
/// Uint8Array the library copied or another handle.
pub(crate) fn create(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let sources = arg(c, args, 0);
    let count = length(c, sources)?;
    let mut all = Vec::new();
    for index in 0..count {
        let source = get(c, sources, &index.to_string())?;
        if let Some(state) = state_of(c, source)? {
            all.extend(parts(c, state)?);
        } else if c.has_builtin_brand_rooted(source, BuiltinBrand::ArrayBufferView)? {
            let buffer = get(c, source, "buffer")?;
            let offset = get(c, source, "byteOffset")?;
            let start = number(c, offset)?;
            let size = get(c, source, "byteLength")?;
            let end = start + number(c, size)?;
            all.push(Part { buffer, start, end });
        } else {
            return Err(RootedError::host("Incorrect Blob initialization type"));
        }
    }
    handle(c, &all)
}

/// `handle.slice(start, end)`: a handle over the same bytes between those offsets.
pub(crate) fn slice(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state_of(c, receiver)?.ok_or_else(|| RootedError::host("Blob handle expected"))?;
    let start = arg(c, args, 0);
    let start = number(c, start)?;
    let end = arg(c, args, 1);
    let end = number(c, end)?.max(start);
    let mut offset = 0;
    let mut sliced = Vec::new();
    for part in parts(c, state)? {
        let size = part.end - part.start;
        let from = start.saturating_sub(offset).min(size);
        let to = end.saturating_sub(offset).min(size);
        if to > from {
            sliced.push(Part { buffer: part.buffer, start: part.start + from, end: part.start + to });
        }
        offset += size;
    }
    handle(c, &sliced)
}

/// `handle.getReader()`: a reader over this handle's parts.
pub(crate) fn get_reader(c: &mut Context<'_>, receiver: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let state = state_of(c, receiver)?.ok_or_else(|| RootedError::host("Blob handle expected"))?;
    let list = get(c, state, state::PARTS)?;
    let next = c.number(0.0);
    with_state(c, data::READER_PROTOTYPE, &[list, next])
}

/// `reader.pull(callback)`: the next part with STATUS_CONTINUE, then STATUS_EOS.
pub(crate) fn pull(c: &mut Context<'_>, receiver: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let state = state_of(c, receiver)?.ok_or_else(|| RootedError::host("Blob reader expected"))?;
    let callback = arg(c, args, 0);
    let all = parts(c, state)?;
    let next = get(c, state, state::NEXT)?;
    let next = number(c, next)?;
    let undefined = c.undefined();
    let Some(part) = all.get(next) else {
        let status = c.number(STATUS_EOS);
        c.call_rooted(callback, undefined, &[status, undefined])?;
        return Ok(status);
    };
    let following = c.number((next + 1) as f64);
    put(c, state, state::NEXT, following)?;
    let bytes = part_bytes(c, part);
    let view = c.uint8_array_rooted(&bytes)?;
    let buffer = get(c, view, "buffer")?;
    let status = c.number(STATUS_CONTINUE);
    c.call_rooted(callback, undefined, &[status, buffer])?;
    Ok(status)
}

/// `reader.setWakeup(fn)`: a memory-resident reader never blocks, so it never wakes.
pub(crate) fn set_wakeup(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.undefined())
}

/// `concat(buffers)`: one ArrayBuffer of the buffers' bytes.
pub(crate) fn concat(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let parts = arg(c, args, 0);
    let count = length(c, parts)?;
    let mut bytes = Vec::new();
    for index in 0..count {
        let part = get(c, parts, &index.to_string())?;
        if c.has_builtin_brand_rooted(part, BuiltinBrand::ArrayBufferView)? || c.has_builtin_brand_rooted(part, BuiltinBrand::AnyArrayBuffer)? {
            c.with_buffer_bytes_rooted(part, |part| bytes.extend_from_slice(part));
        }
    }
    let view = c.uint8_array_rooted(&bytes)?;
    get(c, view, "buffer")
}

fn store_call(c: &mut Context<'_>, method: &str, args: &[RootId]) -> Result<RootId, RootedError> {
    let data = c.host_function_data()?;
    let store = get(c, data, data::STORE)?;
    let function = get(c, data, method)?;
    c.call_rooted(function, store, args)
}

/// `storeDataObject(id, handle, length, type)`.
pub(crate) fn store_data_object(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = arg(c, args, 0);
    let entry = args.get(1..4).unwrap_or_default();
    let entry = c.array_rooted(entry)?;
    store_call(c, data::MAP_SET, &[id, entry])?;
    Ok(c.undefined())
}

/// `getDataObject(id)`: `[handle, length, type]`, or undefined.
pub(crate) fn get_data_object(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let id = arg(c, args, 0);
    store_call(c, data::MAP_GET, &[id])
}

/// `revokeObjectURL(url)`: forget the object a `blob:nodedata:` URL names.
pub(crate) fn revoke_object_url(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let url = arg(c, args, 0);
    let url = c.to_string(url)?;
    let id = url::Url::parse(&url).ok().filter(|url| url.scheme() == BLOB_SCHEME).and_then(|url| {
        let (base, id) = url.path().split_once(':')?;
        (base == NODE_DATA && !id.contains(':')).then(|| id.to_owned())
    });
    if let Some(id) = id {
        let id = c.string_rooted(&id);
        store_call(c, data::MAP_DELETE, &[id])?;
    }
    Ok(c.undefined())
}

/// `createBlobFromFilePath(path)`: `[handle, length]`, or undefined when the file cannot
/// be read. The file's bytes are read when the Blob is made.
pub(crate) fn create_from_file_path(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let path = arg(c, args, 0);
    let path = c.to_string(path)?;
    let Ok(bytes) = std::fs::read(&path) else {
        return Ok(c.undefined());
    };
    let view = c.uint8_array_rooted(&bytes)?;
    let buffer = get(c, view, "buffer")?;
    let handle = handle(c, &[Part { buffer, start: 0, end: bytes.len() }])?;
    let length = c.number(bytes.len() as f64);
    c.array_rooted(&[handle, length])
}

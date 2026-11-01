//! `node:buffer` is Node's lib/buffer.js over `internalBinding('buffer')`; this module
//! gives host code its canonical `Buffer`, `FastBuffer` and prototype.
use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
type Context<'a> = NativeContext<'a, NodeHost>;

const FAST_BUFFER_CACHE: &str = "\0builtin:buffer/FastBuffer";

/// The script name Node's frames give `internal/buffer.js`.
const INTERNAL_BUFFER_SCRIPT: &str = "node:internal/buffer";
/// Internal requests the projected library files make for host-held state; only Node's
/// library code can make them.
pub(crate) const FAST_BUFFER_REQUEST: &str = "internal/quench/fast_buffer";
pub(crate) const BUFFER_POOL_REQUEST: &str = "internal/quench/buffer_pool";

/// `buffer.js`'s `Buffer.poolSize = 64 * 1024` and `kPoolAlignment = 64`, which the host's
/// pool follows until `buffer.js` loads (no script can change them before).
const POOL_SIZE: usize = 64 * 1024;
const POOL_ALIGNMENT: usize = 64;
/// `internal/buffer.js`'s `alignmentPadding(alignment)`: a multiple of 8, at least 8.
const MIN_ALIGNMENT_PADDING: usize = 8;
/// `createUnsafeBuffer` keeps a buffer this small on the heap instead of a backing store.
const HEAP_BUFFER_LIMIT: usize = 64;
/// `alignPool` keeps pooled slices 8-byte aligned.
const POOL_SLICE_ALIGNMENT: usize = 8;


/// Who owns Buffer's allocation pool: nobody yet, the host (which allocated before
/// `buffer.js` loaded), or `buffer.js` (which adopted the host's pool or made its own when its
/// load reached `createPool()`).
#[derive(Default)]
pub(crate) enum PoolOwner {
    #[default]
    Unborn,
    /// `[poolSize, poolOffset, poolBase, allocPool, allocBuffer]`, `buffer.js`'s pool state.
    Host(RootId),
    Library,
}

/// The positions of `PoolOwner::Host`'s state.
#[repr(usize)]
enum PoolSlot {
    Size,
    Offset,
    Base,
    Pool,
    Buffer,
}

impl PoolSlot {
    const COUNT: usize = Self::Buffer as usize + 1;
}

fn get(c: &mut Context<'_>, object: RootId, name: &str) -> Result<RootId, RootedError> {
    c.get_named_property_rooted(object, name)
}

/// Load lib/buffer.js. Its `FastBuffer`, which every host-created Buffer shares, is the host's
/// class (`project_internal_buffer`).
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_module(c, "buffer.js", "buffer.js")
}

/// Installs `globalThis.Buffer` as Node's accessor pair (non-enumerable getter `get` and
/// setter `set` over one binding) and the canonical `NodeBuffer` alias. The `buffer` module is
/// built on the first read of either, not at start-up; the getter then holds the constructor
/// that read loaded, as Node's holds the one its bootstrap loaded.
pub(crate) fn install_global(c: &mut Context<'_>) -> Result<(), RootedError> {
    let load = c.host_function(crate::host::shared_vm::operation("bufferConstructor"))?;
    let installer = crate::host::removed_javascript("node:buffer/install-global.js")?;
    let undefined = c.undefined();
    c.call_rooted(installer, undefined, &[load])?;
    Ok(())
}

/// The canonical FastBuffer class, made on first use by the host or by `internal/buffer.js`
/// (which Node's library loads without `buffer.js` too, for `TextDecoder` or streams). Its
/// prototype gets `Buffer.prototype`'s members as pending properties whose first
/// observation loads `buffer.js`; when `buffer.js` is the loader, its first write to the
/// prototype materializes them.
fn fast_buffer_class(_: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::removed_javascript(INTERNAL_BUFFER_SCRIPT)
}


/// Loads `buffer.js`, which defines a pending prototype's members (`define_pending_prototype`).
/// Its load may be what first wrote to the prototype; the library cache then returns it as
/// it stands.
pub(crate) fn initialize_prototype(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_module(c, "buffer.js", "buffer.js")?;
    Ok(c.undefined())
}

/// `require('internal/quench/fast_buffer')` from the projected `internal/buffer.js`.
pub(crate) fn fast_buffer_request(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    fast_buffer_class(c)
}

/// `require('internal/quench/buffer_pool')` from the projected `buffer.js`: the host's pool
/// state for the library to adopt, or `undefined` for the library to make its own. Either
/// way the library owns the pool from then on.
pub(crate) fn buffer_pool_request(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    let owner = std::mem::replace(&mut shared.borrow_mut().buffer_pool, PoolOwner::Library);
    match owner {
        PoolOwner::Host(state) => Ok(state),
        PoolOwner::Unborn | PoolOwner::Library => Ok(c.undefined()),
    }
}




/// `Buffer.allocUnsafe(size)` before `buffer.js` has loaded: Node's `allocate(size)` over a pool
/// the host holds for the library to adopt. `None` once the library owns the pool.
pub(crate) fn allocate_before_library(c: &mut Context<'_>, size: usize) -> Result<Option<RootId>, RootedError> {
    if matches!(c.host_mut().shared_state().borrow().buffer_pool, PoolOwner::Library) {
        return Ok(None);
    }
    let fast = fast_buffer_class(c)?;
    c.with_startup_intrinsics(|c| {
        if size < POOL_SIZE >> 1 {
            let state = match c.host_mut().shared_state().borrow().buffer_pool {
                PoolOwner::Host(state) => Some(state),
                _ => None,
            };
            let mut state = match state {
                Some(state) => state,
                None => create_pool(c, fast)?,
            };
            if size > POOL_SIZE - pool_slot(c, state, PoolSlot::Offset)? {
                state = create_pool(c, fast)?;
            }
            let pool = get_slot(c, state, PoolSlot::Pool)?;
            let start = pool_slot(c, state, PoolSlot::Base)? + pool_slot(c, state, PoolSlot::Offset)?;
            let start = c.number(start as f64);
            let length = c.number(size as f64);
            let buffer = c.construct_rooted(fast, fast, &[pool, start, length])?;
            let offset = pool_slot(c, state, PoolSlot::Offset)? + size;
            // `alignPool`.
            let offset = offset.next_multiple_of(POOL_SLICE_ALIGNMENT);
            let offset = c.number(offset as f64);
            set_slot(c, state, PoolSlot::Offset, offset)?;
            return Ok(Some(buffer));
        }
        let length = c.number(size as f64);
        if size <= HEAP_BUFFER_LIMIT {
            return c.construct_rooted(fast, fast, &[length]).map(Some);
        }
        let backing = unsafe_array_buffer(c, length)?;
        c.construct_rooted(fast, fast, &[backing]).map(Some)
    })
}

/// `createPool()`: a pool-sized FastBuffer aligned inside a padded, untransferable ArrayBuffer.
fn create_pool(c: &mut Context<'_>, fast: RootId) -> Result<RootId, RootedError> {
    let padded = c.number((POOL_SIZE + POOL_ALIGNMENT.max(MIN_ALIGNMENT_PADDING)) as f64);
    let pool = unsafe_array_buffer(c, padded)?;
    let base = c.array_buffer_aligned_offset_rooted(pool, POOL_ALIGNMENT)?;
    let start = c.number(base as f64);
    let length = c.number(POOL_SIZE as f64);
    let buffer = c.construct_rooted(fast, fast, &[pool, start, length])?;
    // `markAsUntransferable(allocPool)`.
    let util = crate::modules::util_shared_vm::key_binding(c)?;
    let symbols = get(c, util, "privateSymbols")?;
    let untransferable = get(c, symbols, "untransferable_object_private_symbol")?;
    let marked = c.boolean(true);
    c.assign_property_rooted(pool, untransferable, marked)?;
    c.mark_untransferable_rooted(pool)?;
    let size = c.number(POOL_SIZE as f64);
    let offset = c.number(0.0);
    let mut slots = [size; PoolSlot::COUNT];
    slots[PoolSlot::Size as usize] = size;
    slots[PoolSlot::Offset as usize] = offset;
    slots[PoolSlot::Base as usize] = start;
    slots[PoolSlot::Pool as usize] = pool;
    slots[PoolSlot::Buffer as usize] = buffer;
    let state = c.array_rooted(&slots)?;
    let retained = c.retain(state)?;
    let shared = c.host_mut().shared_state();
    let previous = std::mem::replace(&mut shared.borrow_mut().buffer_pool, PoolOwner::Host(retained));
    if let PoolOwner::Host(previous) = previous {
        c.release_root(previous);
    }
    Ok(retained)
}

/// `createUnsafeArrayBuffer(size)`.
fn unsafe_array_buffer(c: &mut Context<'_>, length: RootId) -> Result<RootId, RootedError> {
    let global = c.global_root()?;
    let constructor = get(c, global, "ArrayBuffer")?;
    c.construct_rooted(constructor, constructor, &[length])
}

fn get_slot(c: &mut Context<'_>, state: RootId, slot: PoolSlot) -> Result<RootId, RootedError> {
    get(c, state, &(slot as usize).to_string())
}

fn pool_slot(c: &mut Context<'_>, state: RootId, slot: PoolSlot) -> Result<usize, RootedError> {
    let value = get_slot(c, state, slot)?;
    Ok(c.to_number(value)? as usize)
}

fn set_slot(c: &mut Context<'_>, state: RootId, slot: PoolSlot, value: RootId) -> Result<(), RootedError> {
    c.assign_named_property_rooted(state, &(slot as usize).to_string(), value)
}

/// The `buffer` module's `Buffer`, building the module on first use.
pub(crate) fn constructor(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let module = crate::host::shared_vm::commonjs::buffer_module(c)?;
    get(c, module, "Buffer")
}

/// The canonical FastBuffer, loading the buffer module on first use.
pub(crate) fn fast_buffer(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    let shared = c.host_mut().shared_state();
    if !shared.borrow().builtin_cache.contains_key(FAST_BUFFER_CACHE) {
        crate::host::shared_vm::commonjs::buffer_module(c)?;
    }
    let fast = shared.borrow().builtin_cache.get(FAST_BUFFER_CACHE).copied();
    fast.ok_or_else(|| RootedError::host("canonical FastBuffer missing"))
}

/// A new Buffer holding a copy of `bytes`. A class's `prototype` is non-writable and
/// non-configurable, so FastBuffer's is read rather than stored again.
pub(crate) fn from_bytes(c: &mut Context<'_>, bytes: &[u8]) -> Result<RootId, RootedError> {
    from_vec(c, bytes.to_vec())
}

/// A Buffer whose backing store takes ownership of `bytes`.
pub(crate) fn from_vec(c: &mut Context<'_>, bytes: Vec<u8>) -> Result<RootId, RootedError> {
    let fast = fast_buffer(c)?;
    let prototype = get(c, fast, "prototype")?;
    let value = c.uint8_array_from_rooted(bytes)?;
    if !c.set_prototype_rooted(value, prototype)? {
        return Err(RootedError::host("New Buffer view rejected its intrinsic prototype"));
    }
    Ok(value)
}

fn type_error(c: &mut Context<'_>, message: &str, code: &str) -> RootedError {
    let error = match c.type_error_rooted(message) {
        Ok(error) => error,
        Err(error) => return error,
    };
    let key = c.string_rooted("code");
    let value = c.string_rooted(code);
    if let Err(error) = c.set_property_rooted(error, key, value, error) {
        return error;
    }
    c.throw(error)
}

/// A string's bytes under a requested encoding (`Buffer.from(string, encoding)`'s policy):
/// undefined means UTF-8, and only Node's encoding names are accepted.
pub(crate) fn encode_bytes(c: &mut Context<'_>, input: RootId, encoding: Option<RootId>) -> Result<Vec<u8>, RootedError> {
    let Some(units) = c.string_code_units(input)? else {
        return Err(type_error(c, "The \"string\" argument must be of type string", "ERR_INVALID_ARG_TYPE"));
    };
    let name = match encoding.filter(|encoding| !c.rooted_value(*encoding).is_some_and(|value| value.is_undefined())) {
        None => "utf8".to_owned(),
        Some(encoding) => match c.string_text(encoding)? {
            Some(name) => name,
            None => return Err(type_error(c, "The \"encoding\" argument must be of type string", "ERR_INVALID_ARG_TYPE")),
        },
    };
    let Some(canonical) = super::buffer_codec::canonical_encoding(&name) else {
        return Err(type_error(c, &format!("Unknown encoding: {name}"), "ERR_UNKNOWN_ENCODING"));
    };
    Ok(super::buffer_codec::encode_units(&units, canonical))
}

/// `PoolOwner` tags, in declaration order.
const POOL_UNBORN: u8 = 0;
const POOL_HOST: u8 = 1;
const POOL_LIBRARY: u8 = 2;

impl PoolOwner {
    pub(crate) fn write_snapshot(&self, writer: &mut quench_runtime::SnapshotWriter<'_>) -> Result<(), String> {
        match *self {
            Self::Unborn => writer.u8(POOL_UNBORN),
            Self::Host(root) => {
                writer.u8(POOL_HOST);
                writer.root(root)?;
            }
            Self::Library => writer.u8(POOL_LIBRARY),
        }
        Ok(())
    }
    pub(crate) fn read_snapshot(&mut self, reader: &mut quench_runtime::SnapshotReader<'_, '_>) -> Result<(), String> {
        *self = match reader.u8()? {
            POOL_UNBORN => Self::Unborn,
            POOL_HOST => Self::Host(reader.root()?),
            POOL_LIBRARY => Self::Library,
            _ => return Err("invalid snapshot buffer pool".into()),
        };
        Ok(())
    }
}

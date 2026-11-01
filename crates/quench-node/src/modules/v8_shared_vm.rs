//! `node:v8` is Node's own lib/v8.js. Its native bindings (src/node_v8.cc,
//! src/heap_utils.cc) report Quench's heap: one space, sized by the runtime's heap
//! accounting, under Node's default heap limit. Heap snapshots and V8 profiles have no
//! Quench equivalent and fail as unavailable features.

use crate::host::NodeHost;
use quench_runtime::{NativeContext, RootId, RootedError};
use std::sync::OnceLock;

type Context<'a> = NativeContext<'a, NodeHost>;


/// V8's space names, in `kHeapSpaces` order; Quench's single heap reports as old space.
const HEAP_SPACES: [&str; 13] = [
    "read_only_space",
    "new_space",
    "old_space",
    "code_space",
    "shared_space",
    "trusted_space",
    "shared_trusted_space",
    "new_large_object_space",
    "large_object_space",
    "code_large_object_space",
    "shared_large_object_space",
    "shared_trusted_large_object_space",
    "trusted_large_object_space",
];
const HEAP_SPACE: &str = "old_space";

const MIB: u64 = 1024 * 1024;
const GIB: u64 = 1024 * MIB;
/// V8's heap sizing defaults for 64-bit hosts (src/heap/heap.cc): the old generation is
/// a quarter of physical memory times the pointer multiplier, between the minimum and a
/// maximum doubled on hosts with at least 15 GiB; three semi-space-sized young regions
/// sit beside it.
const HEAP_LIMIT_MULTIPLIER: u64 = (u64::BITS / 8 / 4) as u64;
const PHYSICAL_MEMORY_TO_OLD_GENERATION_RATIO: u64 = 4;
const DEFAULT_MAX_HEAP_SIZE: u64 = 1024 * MIB * HEAP_LIMIT_MULTIPLIER;
const DEFAULT_MIN_HEAP_SIZE: u64 = 128 * MIB * HEAP_LIMIT_MULTIPLIER;
const LARGE_HOST_MEMORY_GIB: u64 = 15;
const NORMAL_PAGE_SIZE: u64 = 256 * 1024;
const MAX_SEMI_SPACE_SIZE: u64 = 32 * MIB;
const YOUNG_GENERATION_SEMI_SPACES: u64 = 3;
const MAX_OLD_SPACE_SIZE_OPTION: &str = "--max-old-space-size=";

/// `heap_size_limit`: `--max-old-space-size` or V8's physical-memory default for the old
/// generation, plus the young generation's reservation.
fn heap_size_limit(exec_argv: &[String]) -> u64 {
    let configured = exec_argv
        .iter()
        .rev()
        .find_map(|argument| argument.strip_prefix(MAX_OLD_SPACE_SIZE_OPTION))
        .and_then(|megabytes| megabytes.parse::<u64>().ok())
        .map(|megabytes| megabytes * MIB);
    let old_generation = configured.unwrap_or_else(|| {
        let physical = crate::modules::os_facts::total_memory_bytes();
        let mut maximum = DEFAULT_MAX_HEAP_SIZE;
        if physical / GIB >= LARGE_HOST_MEMORY_GIB {
            maximum *= 2;
        }
        let proportional = physical / PHYSICAL_MEMORY_TO_OLD_GENERATION_RATIO * HEAP_LIMIT_MULTIPLIER;
        proportional.min(maximum).max(DEFAULT_MIN_HEAP_SIZE).div_ceil(NORMAL_PAGE_SIZE) * NORMAL_PAGE_SIZE
    });
    old_generation + YOUNG_GENERATION_SEMI_SPACES * MAX_SEMI_SPACE_SIZE
}

/// `[used, reserved, limit]` bytes.
fn heap_sizes(c: &mut Context<'_>) -> [f64; 3] {
    let composition = c.heap_memory_composition();
    let limit = heap_size_limit(&c.host_mut().shared_state().borrow().exec_argv);
    [composition.used_bytes() as f64, composition.reserved_bytes() as f64, limit as f64]
}

/// The bindings lib/v8.js reads: `[v8, heap_utils, internal_only_v8]`, built once.
fn bindings(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::cached_native_binding(c, "\0binding:v8-family", |c| {
        let native = c.object_rooted()?;
        for (name, operation) in [
            ("heapSpaces", "v8HeapSpaces"),
            ("heapStatistics", "v8HeapStatistics"),
            ("heapSpaceStatistics", "v8HeapSpaceStatistics"),
            ("cachedDataVersionTag", "v8CachedDataVersionTag"),
            ("isStringOneByteRepresentation", "v8IsStringOneByteRepresentation"),
            ("hashSeed", "v8HashSeed"),
            ("queryObjects", "v8QueryObjects"),
        ] {
            let function = c.host_function_named(crate::host::shared_vm::operation(operation), name)?;
            let key = c.string_rooted(name);
            c.define_data_property_rooted(native, key, function, true, true, true)?;
        }
        let factory = crate::host::removed_javascript("node:internal/v8_binding.js")?;
        let undefined = c.undefined();
        c.call_rooted(factory, undefined, &[native])
    })
}

fn member(c: &mut Context<'_>, name: &str) -> Result<RootId, RootedError> {
    let bindings = bindings(c)?;
    let key = c.string_rooted(name);
    c.get_property_rooted(bindings, key)
}

/// `internalBinding('v8')`.
pub(crate) fn binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    member(c, "binding")
}

/// `internalBinding('heap_utils')`.
pub(crate) fn heap_utils_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    member(c, "heapUtils")
}

/// `internalBinding('internal_only_v8')`.
pub(crate) fn internal_only_binding(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    member(c, "internalOnly")
}

/// `node:v8`, Node's own library module.
pub(crate) fn module(c: &mut Context<'_>) -> Result<RootId, RootedError> {
    crate::host::shared_vm::commonjs::node_library_module(c, "v8.js", "v8.js")
}

pub(crate) fn heap_spaces(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    let names = HEAP_SPACES.iter().map(|name| c.string_rooted(name)).collect::<Vec<_>>();
    c.array_rooted(&names)
}

/// Values in `HEAP_STATISTICS_PROPERTIES` order.
pub(crate) fn heap_statistics(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    const NATIVE_CONTEXTS: f64 = 1.0;
    let [used, reserved, limit] = heap_sizes(c);
    let values = [
        reserved,
        0.0,
        reserved,
        (limit - reserved).max(0.0),
        used,
        limit,
        0.0,
        0.0,
        0.0,
        NATIVE_CONTEXTS,
        0.0,
        0.0,
        0.0,
        0.0,
        used,
    ];
    let values = values.map(|value| c.number(value));
    c.array_rooted(&values)
}

/// Values in `HEAP_SPACE_STATISTICS_PROPERTIES` order for one space.
pub(crate) fn heap_space_statistics(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let index = match args.first() {
        Some(index) => c.to_number(*index)?,
        None => f64::NAN,
    };
    let heap_space = HEAP_SPACES.iter().position(|name| *name == HEAP_SPACE).map(|index| index as f64);
    let values = if Some(index) == heap_space {
        let [used, reserved, limit] = heap_sizes(c);
        [reserved, used, (limit - reserved).max(0.0), reserved]
    } else {
        [0.0; 4]
    };
    let values = values.map(|value| c.number(value));
    c.array_rooted(&values)
}

/// `ScriptCompiler::CachedDataVersionTag()`: Quench's code cache format is its own, so the
/// tag names this engine build rather than V8's.
pub(crate) fn cached_data_tag() -> u32 {
    const FNV_OFFSET: u32 = 0x811c_9dc5;
    const FNV_PRIME: u32 = 0x0100_0193;
    let identity = concat!("quench-", env!("CARGO_PKG_VERSION"));
    identity.bytes().fold(FNV_OFFSET, |hash, byte| (hash ^ u32::from(byte)).wrapping_mul(FNV_PRIME))
}

pub(crate) fn cached_data_version_tag(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    Ok(c.number(f64::from(cached_data_tag())))
}

pub(crate) fn is_string_one_byte_representation(c: &mut Context<'_>, _: RootId, args: &[RootId]) -> Result<RootId, RootedError> {
    let value = match args.first() {
        Some(value) => c.string_code_units(*value)?,
        None => None,
    };
    let one_byte = value.is_some_and(|units| units.iter().all(|unit| *unit <= u16::from(u8::MAX)));
    Ok(c.boolean(one_byte))
}

/// The process's string hash seed, chosen once (V8 seeds per isolate group at startup).
pub(crate) fn hash_seed(c: &mut Context<'_>, _: RootId, _: &[RootId]) -> Result<RootId, RootedError> {
    static SEED: OnceLock<u64> = OnceLock::new();
    let seed = SEED.get_or_init(|| {
        let mut bytes = [0u8; 8];
        let _ = openssl::rand::rand_bytes(&mut bytes);
        u64::from_ne_bytes(bytes)
    });
    Ok(c.string_rooted(&seed.to_string()))
}

/// `internalBinding('internal_only_v8').queryObjects(prototype)`: the heap's objects whose
/// prototype is `prototype`.
pub(crate) fn query_objects(
    context: &mut NativeContext<'_, NodeHost>,
    _receiver: RootId,
    args: &[RootId],
) -> Result<RootId, RootedError> {
    let prototype = args.first().copied().unwrap_or_else(|| context.undefined());
    let objects = context.objects_with_prototype_rooted(prototype)?;
    context.array_rooted(&objects)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn max_old_space_size_sets_the_old_generation() {
        let limit = heap_size_limit(&["--max-old-space-size=512".to_owned()]);
        assert_eq!(limit, 512 * MIB + YOUNG_GENERATION_SEMI_SPACES * MAX_SEMI_SPACE_SIZE);
    }
}

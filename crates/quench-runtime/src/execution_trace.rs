//! Opt-in deterministic attribution of VM work.
//!
//! `QUENCH_EXEC_TRACE=1` enables counters. Disabled execution performs one
//! cached boolean branch and owns no counter state. The report is emitted by
//! the CLI after execution, keeping measurement I/O outside VM semantics.

#[cfg(feature = "execution-trace")]
use std::{cell::RefCell, collections::HashMap, hash::Hash, sync::OnceLock};

macro_rules! heap_lifecycles {
    ($($kind:ident => ($allocated:ident, $dropped:ident, $wire:literal)),+ $(,)?) => {
        #[cfg(feature = "execution-trace")]
        mod heap_lifecycle {
            $(pub(super) static $allocated: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);
            pub(super) static $dropped: std::sync::atomic::AtomicU64 =
                std::sync::atomic::AtomicU64::new(0);)+
        }
        $(
            #[inline]
            pub(crate) fn $kind(allocated: bool) {
                let _ = allocated;
                if allocated {
                    allocation(if $wire == "environment" { "environment" } else { "other" });
                }
                #[cfg(feature = "execution-trace")]
                if enabled() {
                    let counter = if allocated {
                        &heap_lifecycle::$allocated
                    } else {
                        &heap_lifecycle::$dropped
                    };
                    // Values can be dropped while this thread's TLS values
                    // are themselves being destroyed. `with` would panic in
                    // that teardown window and turn optional accounting into
                    // a process abort; a failed `try_with` simply omits a
                    // counter update, which is the only safe outcome then.
                    let _ = COUNTERS.try_with(|counters| {
                        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        let _ = counters;
                    });
                }
            }
        )+
        #[cfg(feature = "execution-trace")]
        fn heap_lifecycle_snapshot() -> serde_json::Value {
            serde_json::json!({$($wire: {
                "allocated": heap_lifecycle::$allocated.load(std::sync::atomic::Ordering::Relaxed),
                "dropped": heap_lifecycle::$dropped.load(std::sync::atomic::Ordering::Relaxed),
            }),+})
        }
    };
}

heap_lifecycles! {
    environment_lifecycle => (ENV_ALLOCATED, ENV_DROPPED, "environment"),
    function_lifecycle => (FUNCTION_ALLOCATED, FUNCTION_DROPPED, "function"),
    object_lifecycle => (OBJECT_ALLOCATED, OBJECT_DROPPED, "object"),
    array_lifecycle => (ARRAY_ALLOCATED, ARRAY_DROPPED, "array"),
}

macro_rules! execution_events {
    ($($name:ident => $wire:literal),+ $(,)?) => {
        #[derive(Clone, Copy)]
        #[repr(usize)]
        pub(crate) enum Event { $($name),+ }
        const EVENT_NAMES: &[&str] = &[$($wire),+];
    };
}

execution_events! {
    LoopEntry => "loop_entry",
    LoopIteration => "loop_iteration",
    FragmentEntry => "fragment_entry",
    LeafAttempt => "leaf_attempt",
    LeafHit => "leaf_hit",
    LeafReject => "leaf_reject",
    LeafRejectLength => "leaf_reject_length",
    LeafRejectOpcode => "leaf_reject_opcode",
    LeafRejectRegister => "leaf_reject_register",
    LeafRejectCall => "leaf_reject_call",
    LeafRejectControl => "leaf_reject_control",
    LeafRejectDepth => "leaf_reject_depth",
    RegExpCacheHit => "regexp_cache_hit",
    RegExpCacheMiss => "regexp_cache_miss",
    BindingLoad => "binding_load",
    DynamicBindingLoad => "dynamic_binding_load",
    ValueDecode => "value_decode",
    FixedWordRead => "fixed_word_read",
    LocalWordRead => "local_word_read",
    RegisterFileRead => "register_file_read",
    OwnedWordRead => "owned_word_read",
    ShapeKernelHit => "shape_kernel_hit",
    CountedForAttempt => "counted_for_attempt",
    CountedForHit => "counted_for_hit",
    CountedForDeopt => "counted_for_deopt",
    CountedForRecognized => "counted_for_recognized",
    CountedForPerIteration => "counted_for_per_iteration",
    RegisterWordCopy => "register_word_copy",
    PackedArrayGet => "packed_array_get",
    PackedArraySet => "packed_array_set",
    PackedArrayMiss => "packed_array_miss",
    NamedPropertyHit => "named_property_hit",
    NamedPropertyMiss => "named_property_miss",
    NamedGetReplacement => "named_get_replacement",
    NamedGetCacheEmpty => "named_get_cache_empty",
    NamedGetLayoutMismatch => "named_get_layout_mismatch",
    NamedGetPrototypeMiss => "named_get_prototype_miss",
    NamedGetSlotMissing => "named_get_slot_missing",
    EqualityWordHit => "equality_word_hit",
    EqualityWordMiss => "equality_word_miss",
    NamedPropertySetHit => "named_property_set_hit",
    NamedPropertySetMiss => "named_property_set_miss",
    NamedSetReplacement => "named_set_replacement",
    NamedSetCacheEmpty => "named_set_cache_empty",
    NamedSetLayoutMismatch => "named_set_layout_mismatch",
    NamedSetSlotNotCell => "named_set_slot_not_cell",
    NamedSetPromoteCell => "named_set_promote_cell",
}

#[cfg(feature = "execution-trace")]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct OperandKey {
    name: &'static str,
    a: u16,
    b: u16,
    c: u16,
}

#[cfg(feature = "execution-trace")]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct CompactSiteKey {
    store: usize,
    code: u32,
    pc: u32,
    source: u32,
    opcode: u8,
    window_len: u8,
    window: [u8; 7],
}

#[cfg(feature = "execution-trace")]
#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct StencilKey {
    code: u32,
    pc: u32,
    kind: &'static str,
}

const MAX_STENCIL_SITES: usize = 256;

#[cfg(feature = "execution-trace")]
fn record_bounded<K: Eq + Hash>(map: &mut HashMap<K, u64>, key: K, capacity: usize) {
    if admits_bounded(map, &key, capacity) {
        *map.entry(key).or_default() += 1;
    }
}

#[cfg(feature = "execution-trace")]
fn admits_bounded<K: Eq + Hash, V>(map: &HashMap<K, V>, key: &K, capacity: usize) -> bool {
    map.len() < capacity || map.contains_key(key)
}

#[cfg(feature = "execution-trace")]
fn record_stencil_rejection(counters: &mut Counters, key: StencilKey, reason: &'static str) {
    record_bounded(
        &mut counters.stencil_rejections,
        (key, reason),
        MAX_STENCIL_SITES,
    );
}

#[cfg(feature = "execution-trace")]
struct Counters {
    compact: [u64; crate::ir::Opcode::COUNT as usize + 1],
    leaf_compact: [u64; crate::ir::Opcode::COUNT as usize + 1],
    slow: HashMap<&'static str, u64>,
    binary: HashMap<&'static str, u64>,
    constant: HashMap<&'static str, u64>,
    environment_children: HashMap<(usize, usize), u64>,
    leaf_rejections: HashMap<&'static str, u64>,
    call_shapes: HashMap<(usize, bool, bool), u64>,
    call_targets: HashMap<&'static str, u64>,
    events: Vec<u64>,
    transitions: HashMap<(&'static str, &'static str), u64>,
    previous: Option<&'static str>,
    operand_transitions: HashMap<(OperandKey, OperandKey), u64>,
    previous_operand: Option<OperandKey>,
    regexp: HashMap<String, (u64, u128, u128)>,
    object_shapes: HashMap<String, u64>,
    function_shapes: HashMap<(usize, usize), (u64, u64)>,
    function_call_shapes: HashMap<(u16, usize, usize), u64>,
    function_opcode_shapes: HashMap<(u64, u16, u8, [u8; 32]), u64>,
    descriptor_objects: HashMap<&'static str, u64>,
    descriptor_views_by_op: HashMap<&'static str, u64>,
    named_property_results: HashMap<&'static str, u64>,
    named_property_misses: HashMap<String, u64>,
    loop_shapes: HashMap<u64, (u64, u64, Vec<&'static str>)>,
    value_decode_by_site: HashMap<&'static str, u64>,
    value_decode_other_by_op: HashMap<&'static str, u64>,
    owned_word_read_by_site: HashMap<&'static str, u64>,
    owned_word_read_by_op: HashMap<&'static str, u64>,
    packed_miss_by: HashMap<&'static str, u64>,
    packed_miss_kind: HashMap<&'static str, u64>,
    allocations: HashMap<&'static str, u64>,
    last_index: HashMap<&'static str, u64>,
    kernels: HashMap<&'static str, (u64, u64)>,
    quickening: HashMap<&'static str, (u64, u64)>,
    stencils: HashMap<StencilKey, (u64, u64)>,
    stencil_rejections: HashMap<(StencilKey, &'static str), u64>,
    stencil_outcomes: HashMap<(StencilKey, &'static str), u64>,
    stencil_iterations: HashMap<StencilKey, u64>,
    stencil_storage: HashMap<StencilKey, crate::stencil_arena::ExecutableResourceSnapshot>,
    compact_sites: HashMap<CompactSiteKey, u64>,
    compact_site_dropped: u64,
}

#[cfg(feature = "execution-trace")]
impl Default for Counters {
    fn default() -> Self {
        Self {
            compact: [0; crate::ir::Opcode::COUNT as usize + 1],
            leaf_compact: [0; crate::ir::Opcode::COUNT as usize + 1],
            slow: HashMap::new(),
            binary: HashMap::new(),
            constant: HashMap::new(),
            environment_children: HashMap::new(),
            leaf_rejections: HashMap::new(),
            call_shapes: HashMap::new(),
            call_targets: HashMap::new(),
            events: Vec::new(),
            transitions: HashMap::new(),
            previous: None,
            operand_transitions: HashMap::new(),
            previous_operand: None,
            regexp: HashMap::new(),
            object_shapes: HashMap::new(),
            function_shapes: HashMap::new(),
            function_call_shapes: HashMap::new(),
            function_opcode_shapes: HashMap::new(),
            descriptor_objects: HashMap::new(),
            descriptor_views_by_op: HashMap::new(),
            named_property_results: HashMap::new(),
            named_property_misses: HashMap::new(),
            loop_shapes: HashMap::new(),
            value_decode_by_site: HashMap::new(),
            value_decode_other_by_op: HashMap::new(),
            owned_word_read_by_site: HashMap::new(),
            owned_word_read_by_op: HashMap::new(),
            packed_miss_by: HashMap::new(),
            packed_miss_kind: HashMap::new(),
            allocations: HashMap::new(),
            last_index: HashMap::new(),
            kernels: HashMap::new(),
            quickening: HashMap::new(),
            stencils: HashMap::new(),
            stencil_rejections: HashMap::new(),
            stencil_outcomes: HashMap::new(),
            stencil_iterations: HashMap::new(),
            stencil_storage: HashMap::new(),
            compact_sites: HashMap::new(),
            compact_site_dropped: 0,
        }
    }
}

#[cfg(feature = "execution-trace")]
thread_local! {
    static NAMED_GET_MISS_REASON: std::cell::Cell<&'static str> = const {
        std::cell::Cell::new("unknown")
    };
}

#[derive(Clone, Copy)]
#[allow(dead_code)]
pub(crate) enum DecodeSite {
    GetN,
    SetN,
    Load,
    LoadChecked,
    Move,
    Call,
    LeafGetN,
    LeafLoad,
    LeafLoadChecked,
    LeafOther,
    BindingBorrow,
    EnvLoad,
    Other,
}

impl DecodeSite {
    const fn name(self) -> &'static str {
        match self {
            Self::GetN => "getn",
            Self::SetN => "setn",
            Self::Load => "load",
            Self::LoadChecked => "load_checked",
            Self::Move => "move",
            Self::Call => "call",
            Self::LeafGetN => "leaf_getn",
            Self::LeafLoad => "leaf_load",
            Self::LeafLoadChecked => "leaf_load_checked",
            Self::LeafOther => "leaf_other",
            Self::BindingBorrow => "binding_borrow",
            Self::EnvLoad => "env_load",
            Self::Other => "other",
        }
    }
}

#[cfg(feature = "execution-trace")]
impl Counters {
    #[inline]
    fn retire(&mut self, name: &'static str) {
        if let Some(previous) = self.previous.replace(name) {
            *self.transitions.entry((previous, name)).or_default() += 1;
        }
    }

    #[inline]
    fn retire_operands(&mut self, key: OperandKey) {
        if let Some(previous) = self.previous_operand.replace(key) {
            *self.operand_transitions.entry((previous, key)).or_default() += 1;
        }
    }
}

#[cfg(feature = "execution-trace")]
fn lane_profile(counters: &Counters, compact_total: u64, slow_total: u64) -> serde_json::Value {
    let leaf_total = counters.leaf_compact.iter().sum::<u64>();
    let compact_slow = counters.compact[crate::ir::Opcode::Slow as usize];
    let leaf_slow = counters.leaf_compact[crate::ir::Opcode::Slow as usize];
    let l2 = compact_total.saturating_sub(compact_slow) + leaf_total.saturating_sub(leaf_slow);
    let l3 = slow_total + leaf_slow;
    let vm_total = l2 + l3;
    serde_json::json!({
        "l0": l0_profile(counters), "l1": l1_profile(counters),
        "l2": {"handlers": l2, "vm_share_ppm": ratio_ppm(l2, vm_total),
            "slow_gateways": {"main": compact_slow, "leaf": leaf_slow},
            "top_compact": top_opcodes(&counters.compact, false),
            "top_compact_sites": top_compact_sites(&counters.compact_sites),
            "compact_site_dropped": counters.compact_site_dropped,
            "top_leaf_compact": top_opcodes(&counters.leaf_compact, false)},
        "l3": {"handlers": l3, "vm_share_ppm": ratio_ppm(l3, vm_total),
            "top_slow": top_map(&counters.slow, 8),
            "descriptor_objects": counters.descriptor_objects,
            "descriptor_views_by_op": top_map(&counters.descriptor_views_by_op, 32),
            "alloc": named_buckets(&counters.allocations,
                &["match_result", "descriptor_view", "environment", "other"]),
            "alloc_detail": top_map(&counters.allocations, 32),
            "last_index": named_buckets(&counters.last_index,
                &["header", "getn", "binding_cell"])},
        "l4": host_profile(counters),
    })
}

#[cfg(feature = "execution-trace")]
fn top_compact_sites(values: &HashMap<CompactSiteKey, u64>) -> Vec<serde_json::Value> {
    let mut values = values.iter().collect::<Vec<_>>();
    values.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(**count));
    values
        .into_iter()
        .take(64)
        .map(|(site, count)| {
            let window = site.window[..usize::from(site.window_len)]
                .iter()
                .filter_map(|opcode| {
                    crate::ir::Opcode::from_u8(*opcode).map(crate::ir::Opcode::name)
                })
                .collect::<Vec<_>>();
            serde_json::json!({
                "store": format!("{:x}", site.store),
                "code": site.code,
                "pc": site.pc,
                "source": (site.source != u32::MAX).then_some(site.source),
                "opcode": crate::ir::Opcode::from_u8(site.opcode).map(crate::ir::Opcode::name),
                "window": window,
                "count": count,
            })
        })
        .collect()
}

#[cfg(feature = "execution-trace")]
fn l0_profile(counters: &Counters) -> serde_json::Value {
    let event = |event: Event| {
        counters
            .events
            .get(event as usize)
            .copied()
            .unwrap_or_default()
    };
    serde_json::json!({
            "word_reads": {
                "fixed": event(Event::FixedWordRead),
                "local": event(Event::LocalWordRead),
                "register": event(Event::RegisterFileRead),
                "owned": event(Event::OwnedWordRead),
            },
            "owned_word_read_by_site": named_buckets(&counters.owned_word_read_by_site,
                &["getn", "setn", "load", "load_checked", "move", "call", "leaf_getn",
                  "leaf_load", "leaf_load_checked", "leaf_other",
                  "binding_borrow", "env_load", "other"]),
            "owned_word_read_by_op": top_map(&counters.owned_word_read_by_op, 16),
            "word_copies": event(Event::RegisterWordCopy),
            "value_decode": event(Event::ValueDecode),
            "value_decode_by_site": named_buckets(&counters.value_decode_by_site,
                &["getn", "setn", "load", "load_checked", "move", "call", "leaf_getn",
                  "leaf_load", "leaf_load_checked", "leaf_other",
                  "binding_borrow", "env_load", "other"]),
            "value_decode_other_by_op": top_map(&counters.value_decode_other_by_op, 16),
            "property_hit": event(Event::NamedPropertyHit),
            "property_miss": event(Event::NamedPropertyMiss),
            "property_payload": property_payload(&counters.named_property_results),
            "packed_get": event(Event::PackedArrayGet),
            "packed_set": event(Event::PackedArraySet),
            "packed_miss": event(Event::PackedArrayMiss),
            "packed_miss_by": named_buckets(&counters.packed_miss_by,
                &["kind", "hole", "oob", "other"]),
            "packed_miss_kind": named_buckets(&counters.packed_miss_kind,
                &["packed_limb28", "packed_int", "packed_double", "packed_value",
                  "holey", "sparse", "stale", "non_array", "other"]),
    })
}

#[cfg(feature = "execution-trace")]
fn l1_profile(counters: &Counters) -> serde_json::Value {
    let event = |event: Event| {
        counters
            .events
            .get(event as usize)
            .copied()
            .unwrap_or_default()
    };
    let mut kernels = counters.kernels.iter().collect::<Vec<_>>();
    kernels.sort_unstable_by_key(|(_, counts)| std::cmp::Reverse(counts.0));
    let kernels = kernels
        .into_iter()
        .take(32)
        .map(|(id, counts)| serde_json::json!({"id": id, "hits": counts.0, "deopts": counts.1}))
        .collect::<Vec<_>>();
    serde_json::json!({
            "shape_hits": event(Event::ShapeKernelHit),
            "counted": {"attempts": event(Event::CountedForAttempt),
                "hits": event(Event::CountedForHit), "deopts": event(Event::CountedForDeopt),
                "recognized": event(Event::CountedForRecognized),
                "per_iteration_rejects": event(Event::CountedForPerIteration)},
            "leaf": leaf_profile(event),
            "kernels": kernels,
    })
}

#[cfg(feature = "execution-trace")]
fn host_profile(counters: &Counters) -> serde_json::Value {
    let targets = counters
        .call_targets
        .iter()
        .filter(|(name, _)| **name != "Function");
    let host_calls = targets.clone().map(|(_, count)| *count).sum::<u64>();
    let by_target = targets
        .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
        .collect::<serde_json::Map<_, _>>();
    serde_json::json!({"host_calls": host_calls, "by_target": by_target})
}

#[cfg(feature = "execution-trace")]
fn top_opcodes(counts: &[u64], include_slow: bool) -> Vec<serde_json::Value> {
    let mut rows = (1..=crate::ir::Opcode::COUNT)
        .filter_map(|id| {
            let opcode = crate::ir::Opcode::from_u8(id)?;
            (counts[id as usize] != 0 && (include_slow || !opcode.is_slow()))
                .then_some((opcode.name(), counts[id as usize]))
        })
        .collect::<Vec<_>>();
    rows.sort_unstable_by_key(|row| std::cmp::Reverse(row.1));
    rows.into_iter()
        .take(8)
        .map(|(opcode, count)| serde_json::json!({"opcode": opcode, "count": count}))
        .collect()
}

#[cfg(feature = "execution-trace")]
fn top_map(map: &HashMap<&'static str, u64>, limit: usize) -> Vec<serde_json::Value> {
    let mut rows = map.iter().collect::<Vec<_>>();
    rows.sort_unstable_by_key(|row| std::cmp::Reverse(*row.1));
    rows.into_iter()
        .take(limit)
        .map(|(name, count)| serde_json::json!({"op": name, "count": count}))
        .collect()
}

#[cfg(feature = "execution-trace")]
fn top_string_map(map: &HashMap<String, u64>, limit: usize) -> Vec<serde_json::Value> {
    let mut rows = map.iter().collect::<Vec<_>>();
    rows.sort_unstable_by_key(|row| std::cmp::Reverse(*row.1));
    rows.into_iter()
        .take(limit)
        .map(|(name, count)| serde_json::json!({"name": name, "count": count}))
        .collect()
}

#[cfg(feature = "execution-trace")]
fn named_buckets(map: &HashMap<&'static str, u64>, names: &[&str]) -> serde_json::Value {
    serde_json::Value::Object(
        names
            .iter()
            .map(|name| {
                (
                    (*name).to_owned(),
                    serde_json::json!(map.get(name).copied().unwrap_or(0)),
                )
            })
            .collect(),
    )
}

#[cfg(feature = "execution-trace")]
fn property_payload(results: &HashMap<&'static str, u64>) -> serde_json::Value {
    let mut out = HashMap::<&str, u64>::new();
    for (name, count) in results {
        let kind = if name.contains("binding_cell") {
            "binding_cell"
        } else if name.ends_with(":number") {
            "number"
        } else if name.ends_with(":object") {
            "object"
        } else if name.ends_with(":function") {
            "function"
        } else {
            "other"
        };
        *out.entry(kind).or_default() += count;
    }
    serde_json::json!({
        "number": out.get("number").copied().unwrap_or(0),
        "object": out.get("object").copied().unwrap_or(0),
        "function": out.get("function").copied().unwrap_or(0),
        "binding_cell": out.get("binding_cell").copied().unwrap_or(0),
        "other": out.get("other").copied().unwrap_or(0),
    })
}

#[cfg(feature = "execution-trace")]
fn leaf_profile(event: impl Fn(Event) -> u64) -> serde_json::Value {
    serde_json::json!({"attempt": event(Event::LeafAttempt), "hit": event(Event::LeafHit),
        "reject_length": event(Event::LeafRejectLength),
        "reject_opcode": event(Event::LeafRejectOpcode) + event(Event::LeafRejectRegister),
        "reject_call": event(Event::LeafRejectCall),
        "reject_control": event(Event::LeafRejectControl) + event(Event::LeafRejectDepth)})
}

#[cfg(feature = "execution-trace")]
fn ratio_ppm(part: u64, total: u64) -> u64 {
    part.saturating_mul(1_000_000)
        .checked_div(total)
        .unwrap_or(0)
}

#[cfg(feature = "execution-trace")]
pub(crate) fn regexp(source: &str, compile_ns: u128, match_ns: u128) {
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let sample = counters.regexp.entry(source.to_string()).or_default();
            sample.0 += 1;
            sample.1 += compile_ns;
            sample.2 += match_ns;
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn regexp(_: &str, _: u128, _: u128) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn object_shape(properties: &crate::value::ObjectProperties) {
    if enabled() {
        let shape = properties
            .names()
            .map(|name| name.as_str())
            .collect::<Vec<_>>()
            .join("|");
        COUNTERS.with(|counters| {
            *counters
                .borrow_mut()
                .object_shapes
                .entry(shape)
                .or_default() += 1;
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn object_shape(_: &crate::value::ObjectProperties) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn function_shape(captures: usize, code_len: usize, allocated: bool) {
    if enabled() {
        let _ = COUNTERS.try_with(|counters| {
            let mut counters = counters.borrow_mut();
            let counts = counters
                .function_shapes
                .entry((captures, code_len))
                .or_default();
            if allocated {
                counts.0 += 1;
            } else {
                counts.1 += 1;
            }
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn function_shape(_: usize, _: usize, _: bool) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn function_call_shape(
    params: u16,
    captures: usize,
    code: Option<crate::machine::CodeView<'_>>,
) {
    if enabled() {
        if let Some(code) = code {
            dump_function_shape(params, captures, code);
        }
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let code_len = code.map_or(0, crate::machine::CodeView::len);
            *counters
                .function_call_shapes
                .entry((params, captures, code_len))
                .or_default() += 1;
            let Some(code) = code.filter(|code| code.len() <= 32) else {
                return;
            };
            let fingerprint = function_fingerprint(params, captures, code);
            let mut opcodes = [0; 32];
            for (pc, opcode) in opcodes.iter_mut().enumerate().take(code.len()) {
                *opcode = code
                    .instruction(pc)
                    .map_or(0, |instruction| instruction.opcode as u8);
            }
            *counters
                .function_opcode_shapes
                .entry((fingerprint, params, code.len() as u8, opcodes))
                .or_default() += 1;
        });
    }
}

#[cfg(feature = "execution-trace")]
fn dump_function_shape(params: u16, captures: usize, code: crate::machine::CodeView<'_>) {
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static SEEN: OnceLock<std::sync::Mutex<std::collections::HashSet<u64>>> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("QUENCH_DUMP_FUNCTION_SHAPES").is_some()) {
        return;
    }
    let fingerprint = function_fingerprint(params, captures, code);
    if !SEEN
        .get_or_init(Default::default)
        .lock()
        .expect("function shape trace lock")
        .insert(fingerprint)
    {
        return;
    }
    let (store, code_id) = code.trace_identity();
    eprintln!(
        "FUNCTION_SHAPE store={store:x} code={code_id} params={params} captures={captures} len={} hash={fingerprint}",
        code.len()
    );
    for pc in 0..code.len() {
        let instruction = code.instruction(pc).expect("valid function instruction");
        let cold = code.cold(instruction).map(crate::ops::Op::variant_name);
        eprintln!("  {pc}: {instruction:?} cold={cold:?}");
        if let Some(crate::ops::Op::Loop {
            init,
            test,
            body,
            update,
            ..
        }) = code.cold_at(pc)
        {
            dump_function_fragment("init", init.code());
            dump_function_fragment("test", test.code());
            dump_function_fragment("body", body.code());
            dump_function_fragment("update", update.code());
        }
        if let Some(crate::ops::Op::Branch {
            then_ops, else_ops, ..
        }) = code.cold_at(pc)
        {
            dump_function_fragment("then", then_ops.code());
            dump_function_fragment("else", else_ops.code());
        }
        if let Some(crate::ops::Op::Conditional {
            consequent,
            alternate,
            ..
        }) = code.cold_at(pc)
        {
            dump_function_fragment("consequent", consequent.code());
            dump_function_fragment("alternate", alternate.code());
        }
    }
}

#[cfg(feature = "execution-trace")]
fn function_fingerprint(params: u16, captures: usize, code: crate::machine::CodeView<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    params.hash(&mut hasher);
    captures.hash(&mut hasher);
    hash_code_facts(code, &mut hasher);
    hasher.finish()
}

#[cfg(feature = "execution-trace")]
fn hash_code_facts(code: crate::machine::CodeView<'_>, hasher: &mut impl std::hash::Hasher) {
    use std::hash::Hash;
    code.len().hash(hasher);
    for pc in 0..code.len() {
        let instruction = code.instruction(pc).expect("valid function instruction");
        (instruction.opcode as u8).hash(&mut *hasher);
        instruction.flags.hash(&mut *hasher);
        instruction.a.hash(&mut *hasher);
        instruction.b.hash(&mut *hasher);
        instruction.c.hash(&mut *hasher);
        code.metadata_at(pc)
            .and_then(|metadata| metadata.name.as_deref())
            .hash(hasher);
        format!("{:?}", code.constant_at(pc)).hash(hasher);
        let cold = code.cold(instruction);
        cold.map(crate::ops::Op::variant_name).hash(hasher);
        hash_nested_code(cold, hasher);
    }
}

#[cfg(feature = "execution-trace")]
fn hash_nested_code(op: Option<&crate::ops::Op>, hasher: &mut impl std::hash::Hasher) {
    use crate::ops::Op;
    match op {
        Some(Op::Loop {
            init,
            test,
            body,
            update,
            ..
        }) => [init, test, body, update]
            .into_iter()
            .filter_map(|fragment| fragment.code())
            .for_each(|code| hash_code_facts(code, hasher)),
        Some(Op::Branch {
            then_ops, else_ops, ..
        }) => [then_ops, else_ops]
            .into_iter()
            .filter_map(|fragment| fragment.code())
            .for_each(|code| hash_code_facts(code, hasher)),
        Some(Op::Conditional {
            consequent,
            alternate,
            ..
        }) => [consequent, alternate]
            .into_iter()
            .filter_map(|fragment| fragment.code())
            .for_each(|code| hash_code_facts(code, hasher)),
        _ => {}
    }
}

#[cfg(feature = "execution-trace")]
fn dump_function_fragment(label: &str, code: Option<crate::machine::CodeView<'_>>) {
    let Some(code) = code else { return };
    eprintln!("    {label} len={}", code.len());
    for pc in 0..code.len() {
        let instruction = code.instruction(pc).expect("valid function fragment");
        let cold = code.cold(instruction).map(crate::ops::Op::variant_name);
        let name = code
            .metadata_at(pc)
            .and_then(|metadata| metadata.name.as_deref());
        eprintln!("      {pc}: {instruction:?} cold={cold:?} name={name:?}");
        if let Some(crate::ops::Op::Loop {
            init,
            test,
            body,
            update,
            ..
        }) = code.cold(instruction)
        {
            dump_function_fragment("init", init.code());
            dump_function_fragment("test", test.code());
            dump_function_fragment("body", body.code());
            dump_function_fragment("update", update.code());
        }
        if let Some(crate::ops::Op::Branch {
            then_ops, else_ops, ..
        }) = code.cold(instruction)
        {
            dump_function_fragment("then", then_ops.code());
            dump_function_fragment("else", else_ops.code());
        }
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn function_call_shape(_: u16, _: usize, _: Option<crate::machine::CodeView<'_>>) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn descriptor_object(origin: &'static str) {
    if enabled() {
        if origin == "view" {
            allocation("descriptor_view");
            CURRENT_OP.with(|current| {
                COUNTERS.with(|state| {
                    *state
                        .borrow_mut()
                        .descriptor_views_by_op
                        .entry(current.get())
                        .or_default() += 1;
                });
            });
        }
        COUNTERS.with(|state| {
            *state
                .borrow_mut()
                .descriptor_objects
                .entry(origin)
                .or_default() += 1;
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn descriptor_object(_: &'static str) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn named_property_result(tier: &'static str, value: &crate::value::Value) {
    if enabled() {
        let binding_kind = |cell: &std::rc::Rc<crate::value::BindingCell>| match &*cell.borrow() {
            crate::value::Value::Number(_) => "number",
            crate::value::Value::Object(_) => "object",
            crate::value::Value::Function(_) => "function",
            crate::value::Value::Array(_) => "array",
            crate::value::Value::Boolean(_) => "boolean",
            crate::value::Value::String(_) => "string",
            _ => "other",
        };
        let kind = match (tier, value) {
            ("word", crate::value::Value::BindingCell(_)) => "word:binding_cell",
            ("prototype", crate::value::Value::Number(_)) => "prototype:number",
            ("prototype", crate::value::Value::Object(_)) => "prototype:object",
            ("prototype", crate::value::Value::Function(_)) => "prototype:function",
            ("prototype", crate::value::Value::BindingCell(cell)) => match binding_kind(cell) {
                "number" => "prototype:binding_cell:number",
                "object" => "prototype:binding_cell:object",
                "function" => "prototype:binding_cell:function",
                "array" => "prototype:binding_cell:array",
                "boolean" => "prototype:binding_cell:boolean",
                "string" => "prototype:binding_cell:string",
                _ => "prototype:binding_cell:other",
            },
            ("prototype", crate::value::Value::String(_)) => "prototype:string",
            ("prototype", _) => "prototype:other",
            ("own", crate::value::Value::Number(_)) => "own:number",
            ("own", crate::value::Value::Object(_)) => "own:object",
            ("own", crate::value::Value::Function(_)) => "own:function",
            ("own", crate::value::Value::BindingCell(cell)) => match binding_kind(cell) {
                "number" => "own:binding_cell:number",
                "object" => "own:binding_cell:object",
                "function" => "own:binding_cell:function",
                "array" => "own:binding_cell:array",
                "boolean" => "own:binding_cell:boolean",
                "string" => "own:binding_cell:string",
                _ => "own:binding_cell:other",
            },
            ("own", crate::value::Value::String(_)) => "own:string",
            ("own", _) => "own:other",
            _ => "unknown",
        };
        COUNTERS.with(|counters| {
            *counters
                .borrow_mut()
                .named_property_results
                .entry(kind)
                .or_default() += 1;
        });
    }
}

#[cfg(feature = "execution-trace")]
pub(crate) fn named_call(_key: &str) {}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn named_call(_: &str) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn named_property_miss(key: &str) {
    if enabled() {
        let reason = NAMED_GET_MISS_REASON.with(std::cell::Cell::get);
        let key = format!("{reason}:{key}");
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            if let Some(count) = counters.named_property_misses.get_mut(&key) {
                *count += 1;
            } else if counters.named_property_misses.len() < 64 {
                counters.named_property_misses.insert(key, 1);
            } else {
                *counters
                    .named_property_misses
                    .entry("other".into())
                    .or_default() += 1;
            }
        });
    }
}

#[cfg(feature = "execution-trace")]
pub(crate) fn named_get_miss_reason(reason: &'static str) {
    if enabled() {
        NAMED_GET_MISS_REASON.with(|current| current.set(reason));
    }
}

#[cfg(feature = "execution-trace")]
pub(crate) fn named_property_word(tier: &'static str, payload: &'static str) {
    if enabled() {
        let kind = match (tier, payload) {
            ("own", "number") => "own:number",
            ("own", "object") => "own:object",
            ("own", "function") => "own:function",
            ("own", "binding_cell") => "own:binding_cell:other",
            ("own", _) => "own:other",
            ("prototype", "number") => "prototype:number",
            ("prototype", "object") => "prototype:object",
            ("prototype", "function") => "prototype:function",
            ("prototype", "binding_cell") => "prototype:binding_cell:other",
            ("prototype", _) => "prototype:other",
            _ => "unknown",
        };
        COUNTERS.with(|counters| {
            *counters
                .borrow_mut()
                .named_property_results
                .entry(kind)
                .or_default() += 1;
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn named_property_result(_: &'static str, _: &crate::value::Value) {}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn named_property_miss(_: &str) {}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn named_get_miss_reason(_: &'static str) {}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
pub(crate) fn named_property_word(_: &'static str, _: &'static str) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn loop_shape(body: crate::machine::CodeView<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    if !loop_trace_enabled() {
        return 0;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    let mut names = Vec::with_capacity(body.len());
    for pc in 0..body.len() {
        let instruction = body.instruction(pc).expect("valid compact loop body");
        (instruction.opcode as u8).hash(&mut hasher);
        instruction.flags.hash(&mut hasher);
        let name = body
            .cold(instruction)
            .map_or_else(|| instruction.opcode.name(), crate::ops::Op::variant_name);
        name.hash(&mut hasher);
        names.push(name);
    }
    let fingerprint = hasher.finish();
    COUNTERS.with(|counters| {
        let mut counters = counters.borrow_mut();
        let shape = counters
            .loop_shapes
            .entry(fingerprint)
            .or_insert((0, 0, names));
        shape.0 += 1;
    });
    fingerprint
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn loop_shape(_: crate::machine::CodeView<'_>) -> u64 {
    0
}

#[cfg(feature = "execution-trace")]
#[inline(always)]
pub(crate) fn loop_shape_iteration(fingerprint: u64) {
    if fingerprint != 0 {
        COUNTERS.with(|counters| {
            if let Some(shape) = counters.borrow_mut().loop_shapes.get_mut(&fingerprint) {
                shape.1 += 1;
            }
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
pub(crate) fn loop_shape_iteration(_: u64) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn loop_shape_entries(fingerprint: u64, entries: usize) {
    if fingerprint != 0 && entries != 0 {
        COUNTERS.with(|counters| {
            if let Some(shape) = counters.borrow_mut().loop_shapes.get_mut(&fingerprint) {
                shape.0 += entries as u64;
            }
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
pub(crate) fn loop_shape_entries(_: u64, _: usize) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn counted_loop_iterations(fingerprint: u64, iterations: usize) {
    if !loop_trace_enabled() || iterations == 0 {
        return;
    }
    let iterations = iterations as u64;
    COUNTERS.with(|counters| {
        let mut counters = counters.borrow_mut();
        if counters.events.is_empty() {
            counters.events.resize(EVENT_NAMES.len(), 0);
        }
        counters.events[Event::LoopIteration as usize] += iterations;
        counters.events[Event::CountedForHit as usize] += iterations;
        if let Some(shape) = counters.loop_shapes.get_mut(&fingerprint) {
            shape.1 += iterations;
        }
    });
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
pub(crate) fn counted_loop_iterations(_: u64, _: usize) {}

#[cfg(feature = "execution-trace")]
static ENABLED: OnceLock<bool> = OnceLock::new();
#[cfg(feature = "execution-trace")]
static LOOP_TRACE_ENABLED: OnceLock<bool> = OnceLock::new();
#[cfg(feature = "execution-trace")]
thread_local! {
    static COUNTERS: RefCell<Counters> = RefCell::new(Counters {
        events: vec![0; EVENT_NAMES.len()],
        ..Counters::default()
    });
    static DECODE_SITE: std::cell::Cell<DecodeSite> = const { std::cell::Cell::new(DecodeSite::Other) };
    static CURRENT_OP: std::cell::Cell<&'static str> = const { std::cell::Cell::new("outside_vm") };
}

#[cfg(feature = "execution-trace")]
pub(crate) struct DecodeGuard(DecodeSite, &'static str);

#[cfg(feature = "execution-trace")]
impl Drop for DecodeGuard {
    fn drop(&mut self) {
        DECODE_SITE.with(|site| site.set(self.0));
        CURRENT_OP.with(|name| name.set(self.1));
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) struct DecodeGuard;

#[inline(always)]
fn enter_decode(site: DecodeSite, name: &'static str) -> DecodeGuard {
    #[cfg(feature = "execution-trace")]
    {
        let previous_site = DECODE_SITE.with(|current| current.replace(site));
        let previous_name = CURRENT_OP.with(|current| current.replace(name));
        DecodeGuard(previous_site, previous_name)
    }
    #[cfg(not(feature = "execution-trace"))]
    {
        let _ = (site, name);
        DecodeGuard
    }
}

#[inline(always)]
pub(crate) fn attribution_scope(origin: &'static str) -> DecodeGuard {
    enter_decode(DecodeSite::Other, origin)
}

/// Attribute execute-word traffic performed inside an admitted native lane to
/// that lane instead of whichever VM opcode happened to invoke its guard.
/// L0/L1 traffic overlaps VM handlers, but its origin must remain truthful.
#[inline(always)]
pub(crate) fn kernel_scope(id: &'static str) -> DecodeGuard {
    attribution_scope(id)
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("QUENCH_EXEC_TRACE").is_some())
}

#[cfg(feature = "execution-trace")]
#[inline(always)]
fn loop_trace_enabled() -> bool {
    enabled() || *LOOP_TRACE_ENABLED.get_or_init(|| std::env::var_os("QUENCH_LOOP_TRACE").is_some())
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) const fn enabled() -> bool {
    false
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn compact(opcode: crate::ir::Opcode) -> DecodeGuard {
    let guard = enter_decode(decode_site_for_opcode(opcode, false), opcode.name());
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            counters.compact[opcode as usize] += 1;
            // Slow is a storage gateway, not a semantic operation. Its cold Op
            // is recorded below so a transition contains each retirement once.
            if !opcode.is_slow() {
                counters.retire(opcode.name());
            }
        });
    }
    guard
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) const fn compact(_: crate::ir::Opcode) -> DecodeGuard {
    DecodeGuard
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn leaf_compact(opcode: crate::ir::Opcode) -> DecodeGuard {
    let guard = enter_decode(decode_site_for_opcode(opcode, true), opcode.name());
    if enabled() {
        COUNTERS.with(|counters| counters.borrow_mut().leaf_compact[opcode as usize] += 1);
    }
    guard
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) const fn leaf_compact(_: crate::ir::Opcode) -> DecodeGuard {
    DecodeGuard
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn operands(instruction: crate::ir::Instruction) {
    if enabled() && !instruction.opcode.is_slow() {
        COUNTERS.with(|counters| {
            counters.borrow_mut().retire_operands(OperandKey {
                name: instruction.opcode.name(),
                a: instruction.a,
                b: instruction.b,
                c: instruction.c,
            });
        });
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn operands(_: crate::ir::Instruction) {}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn compact_site(code: crate::machine::CodeView<'_>, pc: usize) {
    if !enabled() {
        return;
    }
    let Some(instruction) = code.instruction(pc) else {
        return;
    };
    let start = pc.saturating_sub(3);
    let end = code.len().min(pc.saturating_add(4));
    let mut window = [0; 7];
    for (index, offset) in (start..end).enumerate() {
        window[index] = code.instruction(offset).map_or(0, |op| op.opcode as u8);
    }
    let (store, code_id) = code.trace_identity();
    let key = CompactSiteKey {
        store,
        code: code_id,
        pc: pc as u32,
        source: code
            .metadata_at(pc)
            .and_then(|metadata| metadata.source)
            .unwrap_or(u32::MAX),
        opcode: instruction.opcode as u8,
        window_len: (end - start) as u8,
        window,
    };
    COUNTERS.with(|counters| record_compact_site(&mut counters.borrow_mut(), key));
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn compact_site(_: crate::machine::CodeView<'_>, _: usize) {}

#[cfg(feature = "execution-trace")]
fn record_compact_site(counters: &mut Counters, key: CompactSiteKey) {
    if let Some(count) = counters.compact_sites.get_mut(&key) {
        *count += 1;
        return;
    }
    if counters.compact_sites.len() < 4096 {
        counters.compact_sites.insert(key, 1);
        return;
    }
    counters.compact_site_dropped += 1;
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn slow(op: &crate::ops::Op) -> DecodeGuard {
    let guard = enter_decode(decode_site_for_slow(op.variant_name()), op.variant_name());
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let name = op.variant_name();
            *counters.slow.entry(name).or_default() += 1;
            if let crate::ops::Op::Binary { operator, .. } = op {
                *counters.binary.entry(binary_name(*operator)).or_default() += 1;
            }
            if let crate::ops::Op::Const { value, .. } = op {
                *counters.constant.entry(constant_name(value)).or_default() += 1;
            }
            counters.retire(name);
            let (a, b, c) = match op {
                crate::ops::Op::LoadBinding {
                    dst, slot, dynamic, ..
                } => (*dst, *slot, u16::from(*dynamic)),
                crate::ops::Op::Binary { dst, lhs, rhs, .. } => (*dst, *lhs, *rhs),
                crate::ops::Op::Const { dst, .. } => (*dst, 0, 0),
                _ => (0, 0, 0),
            };
            counters.retire_operands(OperandKey { name, a, b, c });
        });
    }
    guard
}

#[cfg(feature = "execution-trace")]
fn binary_name(operator: crate::ops::BinaryOp) -> &'static str {
    use crate::ops::BinaryOp::*;
    match operator {
        Add => "Add",
        Subtract => "Subtract",
        Multiply => "Multiply",
        Divide => "Divide",
        Remainder => "Remainder",
        Exponentiate => "Exponentiate",
        NumericAdd => "NumericAdd",
        NumericSubtract => "NumericSubtract",
        Equal => "Equal",
        NotEqual => "NotEqual",
        StrictEqual => "StrictEqual",
        StrictNotEqual => "StrictNotEqual",
        LessThan => "LessThan",
        LessEqual => "LessEqual",
        GreaterThan => "GreaterThan",
        GreaterEqual => "GreaterEqual",
        BitwiseOr => "BitwiseOr",
        BitwiseXor => "BitwiseXor",
        BitwiseAnd => "BitwiseAnd",
        ShiftLeft => "ShiftLeft",
        ShiftRight => "ShiftRight",
        ShiftRightZeroFill => "ShiftRightZeroFill",
        Instanceof => "Instanceof",
    }
}

#[cfg(feature = "execution-trace")]
fn constant_name(value: &crate::ops::Constant) -> &'static str {
    match value {
        crate::ops::Constant::Number(_) => "Number",
        crate::ops::Constant::Boolean(_) => "Boolean",
        crate::ops::Constant::String(_) => "String",
        crate::ops::Constant::StringUnits(_) => "StringUnits",
        crate::ops::Constant::BigInt(_) => "BigInt",
        crate::ops::Constant::Null => "Null",
        crate::ops::Constant::Undefined => "Undefined",
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) const fn slow(_: &crate::ops::Op) -> DecodeGuard {
    DecodeGuard
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn environment_child(captures: usize, locals: usize) {
    if enabled() {
        COUNTERS.with(|counters| {
            *counters
                .borrow_mut()
                .environment_children
                .entry((captures, locals))
                .or_default() += 1;
        });
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn environment_child(_: usize, _: usize) {}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn leaf_rejection(reason: &'static str) {
    if enabled() {
        COUNTERS.with(|counters| {
            *counters
                .borrow_mut()
                .leaf_rejections
                .entry(reason)
                .or_default() += 1;
        });
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn leaf_rejection(_: &'static str) {}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn call_method(
    args: usize,
    spread: bool,
    registered_callee: bool,
    target: &'static str,
) {
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            *counters
                .call_shapes
                .entry((args, spread, registered_callee))
                .or_default() += 1;
            *counters.call_targets.entry(target).or_default() += 1;
        });
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn call_method(_: usize, _: bool, _: bool, _: &'static str) {}

/// Classify a callable for diagnostic call-shape counters.  Trace builds use
/// generated builtin metadata so a single `Builtin` bucket does not hide the
/// dominant gateway; scored builds retain the cheap coarse classification.
#[cfg(feature = "execution-trace")]
#[inline(always)]
pub(crate) fn call_target_name(value: &crate::value::Value) -> &'static str {
    match value {
        crate::value::Value::Function(_) => "Function",
        crate::value::Value::Builtin(builtin) => {
            let name = crate::builtins::builtin_name(*builtin);
            if name.is_empty() {
                "Builtin"
            } else {
                name
            }
        }
        crate::value::Value::BoundFunction(_) => "BoundFunction",
        crate::value::Value::Undefined => "Undefined",
        _ => "Other",
    }
}

#[cfg(not(feature = "execution-trace"))]
#[inline(always)]
pub(crate) fn call_target_name(value: &crate::value::Value) -> &'static str {
    match value {
        crate::value::Value::Function(_) => "Function",
        crate::value::Value::Builtin(_) => "Builtin",
        crate::value::Value::BoundFunction(_) => "BoundFunction",
        crate::value::Value::Undefined => "Undefined",
        _ => "Other",
    }
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn event(event: Event) {
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            if counters.events.is_empty() {
                counters.events.resize(EVENT_NAMES.len(), 0);
            }
            counters.events[event as usize] += 1;
            if matches!(event, Event::OwnedWordRead) {
                let site = DECODE_SITE.with(std::cell::Cell::get);
                let op = CURRENT_OP.with(std::cell::Cell::get);
                *counters
                    .owned_word_read_by_site
                    .entry(site.name())
                    .or_default() += 1;
                count_named(&mut counters.owned_word_read_by_op, op);
            }
        });
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn event(_: Event) {}

#[cfg(feature = "execution-trace")]
fn record_value_decode(counters: &mut Counters, site: DecodeSite, op: &'static str) {
    if counters.events.is_empty() {
        counters.events.resize(EVENT_NAMES.len(), 0);
    }
    counters.events[Event::ValueDecode as usize] += 1;
    *counters
        .value_decode_by_site
        .entry(site.name())
        .or_default() += 1;
    if matches!(site, DecodeSite::Other | DecodeSite::LeafOther) {
        count_named(&mut counters.value_decode_other_by_op, op);
    }
}

#[inline(always)]
pub(crate) fn value_decode_current() {
    #[cfg(feature = "execution-trace")]
    {
        let site = DECODE_SITE.with(std::cell::Cell::get);
        if enabled() {
            let op = CURRENT_OP.with(std::cell::Cell::get);
            COUNTERS.with(|counters| record_value_decode(&mut counters.borrow_mut(), site, op));
        }
    }
}

#[cfg(feature = "execution-trace")]
fn decode_site_for_opcode(opcode: crate::ir::Opcode, leaf: bool) -> DecodeSite {
    if leaf {
        return match opcode {
            crate::ir::Opcode::GetN => DecodeSite::LeafGetN,
            crate::ir::Opcode::LoadLocal => DecodeSite::LeafLoad,
            crate::ir::Opcode::LoadLocalChecked => DecodeSite::LeafLoadChecked,
            _ => DecodeSite::LeafOther,
        };
    }
    match opcode {
        crate::ir::Opcode::GetN => DecodeSite::GetN,
        crate::ir::Opcode::SetN => DecodeSite::SetN,
        crate::ir::Opcode::Move => DecodeSite::Move,
        crate::ir::Opcode::LoadLocal => DecodeSite::Load,
        crate::ir::Opcode::LoadLocalChecked => DecodeSite::LoadChecked,
        crate::ir::Opcode::CallN => DecodeSite::Call,
        _ => DecodeSite::Other,
    }
}

#[cfg(feature = "execution-trace")]
fn decode_site_for_slow(name: &str) -> DecodeSite {
    match name {
        "LoadBinding" | "ResolveBinding" | "ResolveBindingTarget" => DecodeSite::EnvLoad,
        "LoadLocal" => DecodeSite::Load,
        "LoadLocalChecked" | "CheckInitialized" => DecodeSite::LoadChecked,
        "Move" => DecodeSite::Move,
        "GetProperty" | "GetPropertyDynamic" => DecodeSite::GetN,
        "SetProperty" | "SetPropertyDynamic" => DecodeSite::SetN,
        "Call" | "CallMethod" | "Construct" => DecodeSite::Call,
        _ => DecodeSite::Other,
    }
}

#[cfg(feature = "execution-trace")]
fn count_named(map: &mut HashMap<&'static str, u64>, name: &'static str) {
    if map.len() < 64 || map.contains_key(name) {
        *map.entry(name).or_default() += 1;
    } else {
        *map.entry("other").or_default() += 1;
    }
}

#[inline(always)]
pub(crate) fn packed_miss(reason: &'static str) {
    event(Event::PackedArrayMiss);
    #[cfg(feature = "execution-trace")]
    if enabled() {
        COUNTERS.with(|counters| count_named(&mut counters.borrow_mut().packed_miss_by, reason));
    }
    let _ = reason;
}

#[inline(always)]
pub(crate) fn packed_kind_miss(kind: crate::value::ArrayKind) {
    packed_miss("kind");
    #[cfg(feature = "execution-trace")]
    if enabled() {
        let kind = array_kind_name(kind);
        COUNTERS.with(|counters| count_named(&mut counters.borrow_mut().packed_miss_kind, kind));
    }
    let _ = kind;
}

#[inline(always)]
pub(crate) fn packed_kind_reason(reason: &'static str) {
    packed_miss("kind");
    #[cfg(feature = "execution-trace")]
    if enabled() {
        COUNTERS.with(|counters| count_named(&mut counters.borrow_mut().packed_miss_kind, reason));
    }
    let _ = reason;
}

#[cfg(feature = "execution-trace")]
const fn array_kind_name(kind: crate::value::ArrayKind) -> &'static str {
    match kind {
        crate::value::ArrayKind::PackedLimb28 => "packed_limb28",
        crate::value::ArrayKind::PackedInt => "packed_int",
        crate::value::ArrayKind::PackedDouble => "packed_double",
        crate::value::ArrayKind::PackedValue => "packed_value",
        crate::value::ArrayKind::Holey => "holey",
        crate::value::ArrayKind::Sparse => "sparse",
    }
}

#[inline(always)]
pub(crate) fn allocation(kind: &'static str) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        COUNTERS.with(|counters| count_named(&mut counters.borrow_mut().allocations, kind));
    }
    let _ = kind;
}

#[inline(always)]
pub(crate) fn last_index(kind: &'static str) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        COUNTERS.with(|counters| count_named(&mut counters.borrow_mut().last_index, kind));
    }
    let _ = kind;
}

#[inline(always)]
pub(crate) fn kernel(id: &'static str, deopt: bool) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let counts = counters.kernels.entry(id).or_default();
            if deopt {
                counts.1 += 1
            } else {
                counts.0 += 1
            }
        });
    }
    let _ = (id, deopt);
}

/// Record a generic quickening observation and expose a narrow hit-dominance
/// hint to bounded caches. The hint is runtime shape data only; it carries no
/// fixture, source, or persistent profile identity.
#[inline(always)]
pub(crate) fn quickening_observation(opcode: crate::ir::Opcode, hit: bool) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let counts = counters.quickening.entry(opcode.name()).or_default();
            if hit {
                counts.0 += 1;
            } else {
                counts.1 += 1;
            }
        });
    }
    let _ = (opcode, hit);
}

#[inline(always)]
pub(crate) fn quickening_prefers_hot(opcode: crate::ir::Opcode) -> bool {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        return COUNTERS.with(|counters| {
            counters
                .borrow()
                .quickening
                .get(opcode.name())
                .is_some_and(|(hits, misses)| *hits > *misses)
        });
    }
    let _ = opcode;
    false
}

#[cfg(feature = "execution-trace")]
fn quickening_profile(counters: &Counters) -> serde_json::Map<String, serde_json::Value> {
    counters
        .quickening
        .iter()
        .map(|(name, &(hits, misses))| {
            (
                (*name).to_owned(),
                serde_json::json!({ "hits": hits, "misses": misses }),
            )
        })
        .collect()
}

/// Record whether an admitted stencil site executed native bytes or fell
/// through to its complete canonical handler. The key is code identity plus
/// bytecode offset and plan kind, so the fact remains reusable across
/// fixtures without retaining source names or benchmark identity.
#[inline(always)]
pub(crate) fn stencil_observation(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    kind: &'static str,
    native: bool,
) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        let (_, code_id) = code.trace_identity();
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let key = StencilKey {
                code: code_id,
                pc: pc as u32,
                kind,
            };
            if admits_bounded(&counters.stencils, &key, MAX_STENCIL_SITES) {
                let counts = counters.stencils.entry(key).or_default();
                if native {
                    counts.0 += 1;
                } else {
                    counts.1 += 1;
                }
            }
        });
    }
    let _ = (code, pc, kind, native);
}

/// Attribute the number of machine-level loop iterations retired by a native
/// region.  This is deliberately separate from the hit/miss pair consumed by
/// the ledger: an entry hit proves only that bytes were entered, while this
/// fact proves the native body did useful repeated work.
#[inline(always)]
pub(crate) fn stencil_iterations(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    kind: &'static str,
    iterations: usize,
) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        let (_, code_id) = code.trace_identity();
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let key = StencilKey {
                code: code_id,
                pc: pc as u32,
                kind,
            };
            if admits_bounded(&counters.stencil_iterations, &key, MAX_STENCIL_SITES) {
                *counters.stencil_iterations.entry(key).or_default() += iterations as u64;
            }
        });
    }
    let _ = (code, pc, kind, iterations);
}

/// Attribute an entry rejection without conflating it with an executed miss.
/// Reasons are static categories, so the optional map remains bounded by the
/// generated region identities and does not retain source or fixture data.
#[inline(always)]
pub(crate) fn stencil_rejection(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    kind: &'static str,
    reason: &'static str,
) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        let (_, code_id) = code.trace_identity();
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            record_stencil_rejection(
                &mut counters,
                StencilKey {
                    code: code_id,
                    pc: pc as u32,
                    kind,
                },
                reason,
            );
        });
    }
    let _ = (code, pc, kind, reason);
}

/// Record the exact post-entry outcome of a selected region. Outcomes are
/// separate from hit/miss and rejection facts, so a committed exit cannot be
/// mistaken for a retryable admission miss.
#[inline(always)]
pub(crate) fn stencil_outcome(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    kind: &'static str,
    outcome: &'static str,
) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        let (_, code_id) = code.trace_identity();
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            record_bounded(
                &mut counters.stencil_outcomes,
                (
                    StencilKey {
                        code: code_id,
                        pc: pc as u32,
                        kind,
                    },
                    outcome,
                ),
                MAX_STENCIL_SITES,
            );
        });
    }
    let _ = (code, pc, kind, outcome);
}

/// Record the bounded physical storage observed by a selected region.  This
/// is optional attribution only; uninstrumented execution does not touch the
/// map. Resident, used, retired-live, cache and lease facts are one derived
/// snapshot of the owning pool, not independently updated telemetry state.
#[inline(always)]
pub(crate) fn stencil_storage(
    code: crate::machine::CodeView<'_>,
    pc: usize,
    kind: &'static str,
    pool: &crate::stencil_arena::SharedStencilSlab,
) {
    #[cfg(feature = "execution-trace")]
    if enabled() {
        let (_, code_id) = code.trace_identity();
        let snapshot = pool.resource_snapshot();
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let key = StencilKey {
                code: code_id,
                pc: pc as u32,
                kind,
            };
            if admits_bounded(&counters.stencil_storage, &key, MAX_STENCIL_SITES) {
                counters.stencil_storage.insert(key, snapshot);
            }
        });
    }
    let _ = (code, pc, kind, pool);
}

#[cfg(feature = "execution-trace")]
fn stencil_profile(counters: &Counters) -> serde_json::Map<String, serde_json::Value> {
    counters
        .stencils
        .iter()
        .map(|(key, &(hits, misses))| {
            (
                format!("code={}:pc={}:{}", key.code, key.pc, key.kind),
                serde_json::json!({
                    "hits": hits,
                    "misses": misses,
                    "iterations": counters.stencil_iterations.get(key).copied().unwrap_or_default(),
                }),
            )
        })
        .collect()
}

#[cfg(feature = "execution-trace")]
fn stencil_rejection_profile(counters: &Counters) -> serde_json::Map<String, serde_json::Value> {
    counters
        .stencil_rejections
        .iter()
        .map(|(&(key, reason), &count)| {
            (
                format!("code={}:pc={}:{}:{}", key.code, key.pc, key.kind, reason),
                serde_json::json!(count),
            )
        })
        .collect()
}

#[cfg(feature = "execution-trace")]
fn stencil_outcome_profile(counters: &Counters) -> serde_json::Map<String, serde_json::Value> {
    counters
        .stencil_outcomes
        .iter()
        .map(|(&(key, outcome), &count)| {
            (
                format!("code={}:pc={}:{}:{}", key.code, key.pc, key.kind, outcome),
                serde_json::json!(count),
            )
        })
        .collect()
}

#[cfg(feature = "execution-trace")]
pub fn snapshot() -> Option<serde_json::Value> {
    loop_trace_enabled().then(|| {
        COUNTERS.with(|counters| {
            let counters = counters.borrow();
            let compact = (1..=crate::ir::Opcode::COUNT)
                .filter_map(|id| {
                    let count = counters.compact[id as usize];
                    (count != 0).then(|| {
                        let opcode = crate::ir::Opcode::from_u8(id).expect("declared opcode");
                        (format!("{opcode:?}"), serde_json::json!(count))
                    })
                })
                .collect::<serde_json::Map<_, _>>();
            let leaf_compact = (1..=crate::ir::Opcode::COUNT)
                .filter_map(|id| {
                    let count = counters.leaf_compact[id as usize];
                    (count != 0).then(|| {
                        let opcode = crate::ir::Opcode::from_u8(id).expect("declared opcode");
                        (format!("{opcode:?}"), serde_json::json!(count))
                    })
                })
                .collect::<serde_json::Map<_, _>>();
            let mut slow: Vec<_> = counters.slow.iter().collect();
            slow.sort_unstable_by_key(|(name, _)| *name);
            let slow = slow
                .into_iter()
                .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
                .collect::<serde_json::Map<_, _>>();
            let binary = counters
                .binary
                .iter()
                .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
                .collect::<serde_json::Map<_, _>>();
            let constant = counters
                .constant
                .iter()
                .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
                .collect::<serde_json::Map<_, _>>();
            let events = EVENT_NAMES
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    (
                        (*name).to_owned(),
                        serde_json::json!(counters.events.get(index).copied().unwrap_or_default()),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            let environment_children = counters
                .environment_children
                .iter()
                .map(|(&(captures, locals), count)| {
                    (format!("{captures}:{locals}"), serde_json::json!(count))
                })
                .collect::<serde_json::Map<_, _>>();
            let leaf_rejections = counters
                .leaf_rejections
                .iter()
                .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
                .collect::<serde_json::Map<_, _>>();
            let call_shapes = counters
                .call_shapes
                .iter()
                .map(|(&(args, spread, registered), count)| {
                    (
                        format!("{args}:{}:{}", u8::from(spread), u8::from(registered)),
                        serde_json::json!(count),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            let call_targets = counters
                .call_targets
                .iter()
                .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
                .collect::<serde_json::Map<_, _>>();
            let quickening = quickening_profile(&counters);
            let compact_total: u64 = counters.compact.iter().sum();
            let slow_total: u64 = counters.slow.values().sum();
            let mut transitions: Vec<_> = counters.transitions.iter().collect();
            transitions.sort_unstable_by(|left, right| right.1.cmp(left.1));
            let transitions = transitions
                .into_iter()
                .take(64)
                .map(|(&(from, to), &count)| {
                    serde_json::json!({
                        "from": from, "to": to, "count": count
                    })
                })
                .collect::<Vec<_>>();
            let mut operand_transitions: Vec<_> = counters.operand_transitions.iter().collect();
            operand_transitions.sort_unstable_by(|left, right| right.1.cmp(left.1));
            let operand_transitions = operand_transitions
                .into_iter()
                .take(128)
                .map(|(&(from, to), &count)| {
                    serde_json::json!({
                        "from": { "name": from.name, "a": from.a, "b": from.b, "c": from.c },
                        "to": { "name": to.name, "a": to.a, "b": to.b, "c": to.c },
                        "count": count
                    })
                })
                .collect::<Vec<_>>();
            let mut regexp: Vec<_> = counters.regexp.iter().collect();
            regexp.sort_unstable_by(|left, right| right.1 .2.cmp(&left.1 .2));
            let regexp = regexp
                .into_iter()
                .take(64)
                .map(|(source, &(calls, compile_ns, match_ns))| {
                    serde_json::json!({
                        "source": source,
                        "calls": calls,
                        "compile_ns": compile_ns,
                        "match_ns": match_ns,
                    })
                })
                .collect::<Vec<_>>();
            let mut object_shapes: Vec<_> = counters.object_shapes.iter().collect();
            object_shapes.sort_unstable_by(|left, right| right.1.cmp(left.1));
            let object_shapes = object_shapes
                .into_iter()
                .take(64)
                .map(|(shape, count)| serde_json::json!({"shape": shape, "count": count}))
                .collect::<Vec<_>>();
            let mut function_shapes: Vec<_> = counters.function_shapes.iter().collect();
            function_shapes.sort_unstable_by_key(|(_, counts)| {
                std::cmp::Reverse(counts.0.saturating_sub(counts.1))
            });
            let function_shapes = function_shapes.into_iter().take(64).map(|(&(captures, code_len), &(allocated, dropped))| {
                serde_json::json!({"captures": captures, "code_len": code_len, "allocated": allocated, "dropped": dropped})
            }).collect::<Vec<_>>();
            let mut function_call_shapes: Vec<_> = counters.function_call_shapes.iter().collect();
            function_call_shapes.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(**count));
            let function_call_shapes = function_call_shapes.into_iter().take(64).map(
                |(&(params, captures, code_len), &count)| {
                    serde_json::json!({
                        "params": params,
                        "captures": captures,
                        "code_len": code_len,
                        "count": count,
                    })
                },
            ).collect::<Vec<_>>();
            let mut function_opcode_shapes: Vec<_> = counters.function_opcode_shapes.iter().collect();
            function_opcode_shapes.sort_unstable_by_key(|(_, count)| std::cmp::Reverse(**count));
            let function_opcode_shapes = function_opcode_shapes.into_iter().take(64).map(
                |(&(fingerprint, params, len, opcodes), &count)| {
                    let opcodes = opcodes[..usize::from(len)].iter().filter_map(|opcode| {
                        crate::ir::Opcode::from_u8(*opcode).map(crate::ir::Opcode::name)
                    }).collect::<Vec<_>>();
                    serde_json::json!({
                        "fingerprint": fingerprint.to_string(),
                        "params": params,
                        "opcodes": opcodes,
                        "count": count,
                    })
                },
            ).collect::<Vec<_>>();
            let mut loop_shapes = counters.loop_shapes.iter().collect::<Vec<_>>();
            loop_shapes.sort_unstable_by_key(|(_, shape)| std::cmp::Reverse(shape.1));
            let loop_shapes = loop_shapes.into_iter().map(|(&fingerprint, shape)| {
                serde_json::json!({
                    "fingerprint": fingerprint.to_string(),
                    "entries": shape.0,
                    "iterations": shape.1,
                    "ops": shape.2,
                })
            }).collect::<Vec<_>>();
            let lanes = lane_profile(&counters, compact_total, slow_total);
            serde_json::json!({
                "schema": 5,
                "compact_total": compact_total,
                "slow_total": slow_total,
                "guest_total": compact_total,
                "handler_total": compact_total + slow_total,
                "compact": compact,
                "leaf_compact": leaf_compact,
                "slow": slow,
                "binary": binary,
                "constant": constant,
                "environment_children": environment_children,
                "leaf_rejections": leaf_rejections,
                "call_shapes": call_shapes,
                "call_targets": call_targets,
                "quickening": quickening,
                "stencil": stencil_profile(&counters),
                "stencil_rejections": stencil_rejection_profile(&counters),
                "stencil_outcomes": stencil_outcome_profile(&counters),
                "stencil_storage": counters
                    .stencil_storage
                    .iter()
                    .map(|(key, snapshot)| (
                        format!("code={}:pc={}:{}", key.code, key.pc, key.kind),
                        serde_json::json!({
                            "used_bytes": snapshot.used_bytes,
                            "resident_bytes": snapshot.resident_bytes,
                            "retired_live_bytes": snapshot.retired_live_bytes,
                            "cache_rows": snapshot.cache_rows,
                            "active_leases": snapshot.active_leases,
                            "retired_owners": snapshot.retired_owners,
                            "process_resident_bytes": snapshot.process_resident_bytes,
                        }),
                    ))
                    .collect::<serde_json::Map<_, _>>(),
                "events": events,
                "transitions": transitions,
                "operand_transitions": operand_transitions,
                "regexp": regexp,
                "object_shapes": object_shapes,
                "function_shapes": function_shapes,
                "function_call_shapes": function_call_shapes,
                "function_opcode_shapes": function_opcode_shapes,
                "descriptor_objects": counters.descriptor_objects,
                "named_property_results": counters.named_property_results,
                "named_property_misses": top_string_map(&counters.named_property_misses, 32),
                "loop_shapes": loop_shapes,
                "lanes": lanes,
                "heap_lifecycle": heap_lifecycle_snapshot(),
            })
        })
    })
}

#[cfg(not(feature = "execution-trace"))]
pub fn snapshot() -> Option<serde_json::Value> {
    None
}

#[cfg(feature = "execution-trace")]
pub fn emit() {
    if let Some(snapshot) = snapshot() {
        eprintln!("QUENCH_EXEC_TRACE {snapshot}");
    }
}

#[cfg(all(test, feature = "execution-trace"))]
mod lane_profile_tests {
    use super::*;

    #[test]
    fn reports_vm_shares_and_native_deopts_from_one_event_table() {
        let mut counters = Counters {
            events: vec![0; EVENT_NAMES.len()],
            ..Counters::default()
        };
        counters.compact[crate::ir::Opcode::Move as usize] = 8;
        counters.compact[crate::ir::Opcode::Slow as usize] = 2;
        counters.events[Event::CountedForHit as usize] = 4;
        counters.events[Event::CountedForDeopt as usize] = 1;
        let profile = lane_profile(&counters, 10, 2);
        assert_eq!(profile["l1"]["counted"]["hits"], 4);
        assert_eq!(profile["l1"]["counted"]["deopts"], 1);
        assert_eq!(profile["l2"]["handlers"], 8);
        assert_eq!(profile["l3"]["handlers"], 2);
        assert_eq!(profile["l2"]["vm_share_ppm"], 800_000);
    }

    #[test]
    fn exposes_quickening_profile_facts_for_consumers() {
        let mut counters = Counters::default();
        counters.quickening.insert("GetProperty", (9, 2));
        let profile = quickening_profile(&counters);
        assert_eq!(profile["GetProperty"]["hits"], 9);
        assert_eq!(profile["GetProperty"]["misses"], 2);
    }

    #[test]
    fn stencil_profile_keeps_entry_outcomes_and_retired_iterations() {
        let mut counters = Counters::default();
        let key = StencilKey {
            code: 7,
            pc: 11,
            kind: "array_numeric_loop",
        };
        counters.stencils.insert(key, (3, 1));
        counters.stencil_iterations.insert(key, 12);
        let profile = stencil_profile(&counters);
        assert_eq!(profile["code=7:pc=11:array_numeric_loop"]["hits"], 3);
        assert_eq!(profile["code=7:pc=11:array_numeric_loop"]["iterations"], 12);
    }

    #[test]
    fn stencil_storage_keeps_retired_live_and_cache_facts_together() {
        let mut counters = Counters::default();
        let key = StencilKey {
            code: 3,
            pc: 4,
            kind: "array_numeric_loop",
        };
        let expected = crate::stencil_arena::ExecutableResourceSnapshot {
            resident_bytes: 8192,
            used_bytes: 76,
            retired_live_bytes: 4096,
            cache_rows: 1,
            active_leases: 1,
            retired_owners: 1,
            process_resident_bytes: 12288,
        };
        counters.stencil_storage.insert(key, expected);
        let value = counters
            .stencil_storage
            .get(&key)
            .copied()
            .expect("storage fact");
        assert_eq!(value, expected);
    }

    #[test]
    fn stencil_rejection_profile_keeps_reason_separate_from_misses() {
        let mut counters = Counters::default();
        let key = StencilKey {
            code: 5,
            pc: 8,
            kind: "composed_region",
        };
        counters
            .stencil_rejections
            .insert((key, "window_validation"), 2);
        let profile = stencil_rejection_profile(&counters);
        assert_eq!(profile["code=5:pc=8:composed_region:window_validation"], 2);
    }

    #[test]
    fn stencil_rejection_facts_have_a_fixed_capacity() {
        let mut counters = Counters::default();
        for code in 0..MAX_STENCIL_SITES as u32 {
            record_stencil_rejection(
                &mut counters,
                StencilKey {
                    code,
                    pc: 0,
                    kind: "region",
                },
                "guard",
            );
        }
        assert_eq!(counters.stencil_rejections.len(), MAX_STENCIL_SITES);
    }

    #[test]
    fn stencil_outcome_profile_distinguishes_native_and_fallback_completion() {
        let mut counters = Counters::default();
        let key = StencilKey {
            code: 4,
            pc: 6,
            kind: "region",
        };
        record_bounded(
            &mut counters.stencil_outcomes,
            (key, "native_completed"),
            MAX_STENCIL_SITES,
        );
        record_bounded(
            &mut counters.stencil_outcomes,
            (key, "fallback_completed"),
            MAX_STENCIL_SITES,
        );
        let profile = stencil_outcome_profile(&counters);
        assert_eq!(profile["code=4:pc=6:region:native_completed"], 1);
        assert_eq!(profile["code=4:pc=6:region:fallback_completed"], 1);
    }

    #[test]
    fn attributes_getn_decode_to_its_site() {
        let mut counters = Counters::default();
        record_value_decode(
            &mut counters,
            decode_site_for_opcode(crate::ir::Opcode::GetN, false),
            "GetN",
        );
        assert_eq!(counters.events[Event::ValueDecode as usize], 1);
        assert_eq!(counters.value_decode_by_site.get("getn"), Some(&1));
    }

    #[test]
    fn attributes_owned_word_reads_to_site_and_opcode() {
        let mut counters = Counters {
            events: vec![0; EVENT_NAMES.len()],
            ..Counters::default()
        };
        counters.events[Event::OwnedWordRead as usize] += 1;
        *counters
            .owned_word_read_by_site
            .entry(DecodeSite::GetN.name())
            .or_default() += 1;
        count_named(&mut counters.owned_word_read_by_op, "GetN");
        let profile = l0_profile(&counters);
        assert_eq!(profile["owned_word_read_by_site"]["getn"], 1);
        assert_eq!(profile["owned_word_read_by_op"][0]["op"], "GetN");
    }

    #[test]
    fn attributes_calln_traffic_to_call_site() {
        assert!(matches!(
            decode_site_for_opcode(crate::ir::Opcode::CallN, false),
            DecodeSite::Call
        ));
    }

    #[test]
    fn attribution_scope_replaces_and_restores_stale_opcode_attribution() {
        CURRENT_OP.with(|current| current.set("ResolveName"));
        {
            let _scope = attribution_scope("SetN:ordinary");
            assert_eq!(CURRENT_OP.with(std::cell::Cell::get), "SetN:ordinary");
        }
        assert_eq!(CURRENT_OP.with(std::cell::Cell::get), "ResolveName");
    }

    #[test]
    fn packed_kind_miss_names_every_physical_array_layout() {
        use crate::value::ArrayKind::*;
        assert_eq!(array_kind_name(PackedLimb28), "packed_limb28");
        assert_eq!(array_kind_name(PackedInt), "packed_int");
        assert_eq!(array_kind_name(PackedDouble), "packed_double");
        assert_eq!(array_kind_name(PackedValue), "packed_value");
        assert_eq!(array_kind_name(Holey), "holey");
        assert_eq!(array_kind_name(Sparse), "sparse");
    }

    #[test]
    fn ranks_compact_sites_with_patchable_context() {
        let mut counters = Counters {
            events: vec![0; EVENT_NAMES.len()],
            ..Counters::default()
        };
        counters.compact_sites.insert(
            CompactSiteKey {
                store: 1,
                code: 7,
                pc: 2,
                source: 41,
                opcode: crate::ir::Opcode::LoadLocalChecked as u8,
                window_len: 3,
                window: [8, 24, 20, 0, 0, 0, 0],
            },
            99,
        );
        let profile = lane_profile(&counters, 0, 0);
        let site = &profile["l2"]["top_compact_sites"][0];
        assert_eq!(site["code"], 7);
        assert_eq!(site["pc"], 2);
        assert_eq!(site["source"], 41);
        assert_eq!(site["opcode"], "LoadLocalChecked");
        assert_eq!(
            site["window"],
            serde_json::json!(["LoadLocal", "LoadLocalChecked", "GetN"])
        );
        assert_eq!(site["count"], 99);
    }

    #[test]
    fn keeps_descriptor_events_independent_from_view_allocations() {
        let mut counters = Counters {
            events: vec![0; EVENT_NAMES.len()],
            ..Counters::default()
        };
        counters.descriptor_objects.insert("view", 3);
        counters.descriptor_views_by_op.insert("DefineProperty", 3);
        counters.allocations.insert("descriptor_view", 2);
        let profile = lane_profile(&counters, 0, 0);
        assert_eq!(profile["l3"]["descriptor_objects"]["view"], 3);
        assert_eq!(
            profile["l3"]["descriptor_views_by_op"][0],
            serde_json::json!({"op": "DefineProperty", "count": 3})
        );
        assert_eq!(profile["l3"]["alloc"]["descriptor_view"], 2);
    }
}

#[cfg(not(feature = "execution-trace"))]
pub fn emit() {}

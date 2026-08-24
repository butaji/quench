//! Opt-in deterministic attribution of VM work.
//!
//! `QUENCH_EXEC_TRACE=1` enables counters. Disabled execution performs one
//! cached boolean branch and owns no counter state. The report is emitted by
//! the CLI after execution, keeping measurement I/O outside VM semantics.

#[cfg(feature = "execution-trace")]
use std::{cell::RefCell, collections::HashMap, sync::OnceLock};

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
    BindingLoad => "binding_load",
    DynamicBindingLoad => "dynamic_binding_load",
    ValueDecode => "value_decode",
    RegisterWordCopy => "register_word_copy",
    PackedArrayGet => "packed_array_get",
    PackedArraySet => "packed_array_set",
    PackedArrayMiss => "packed_array_miss",
}

#[cfg(feature = "execution-trace")]
#[derive(Default)]
struct Counters {
    compact: [u64; crate::ir::Opcode::COUNT as usize + 1],
    slow: HashMap<&'static str, u64>,
    events: [u64; EVENT_NAMES.len()],
}

#[cfg(feature = "execution-trace")]
static ENABLED: OnceLock<bool> = OnceLock::new();
#[cfg(feature = "execution-trace")]
thread_local! {
    static COUNTERS: RefCell<Counters> = RefCell::new(Counters::default());
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("QUENCH_EXEC_TRACE").is_some())
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) const fn enabled() -> bool {
    false
}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn compact(opcode: crate::ir::Opcode) {
    if enabled() {
        COUNTERS.with(|counters| counters.borrow_mut().compact[opcode as usize] += 1);
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn compact(_: crate::ir::Opcode) {}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn slow(op: &crate::ops::Op) {
    if enabled() {
        COUNTERS.with(|counters| {
            *counters
                .borrow_mut()
                .slow
                .entry(op.variant_name())
                .or_default() += 1;
        });
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn slow(_: &crate::ops::Op) {}

#[inline(always)]
#[cfg(feature = "execution-trace")]
pub(crate) fn event(event: Event) {
    if enabled() {
        COUNTERS.with(|counters| counters.borrow_mut().events[event as usize] += 1);
    }
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn event(_: Event) {}

#[cfg(feature = "execution-trace")]
pub fn snapshot() -> Option<serde_json::Value> {
    enabled().then(|| {
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
            let mut slow: Vec<_> = counters.slow.iter().collect();
            slow.sort_unstable_by_key(|(name, _)| *name);
            let slow = slow
                .into_iter()
                .map(|(name, count)| ((*name).to_owned(), serde_json::json!(count)))
                .collect::<serde_json::Map<_, _>>();
            let events = EVENT_NAMES
                .iter()
                .enumerate()
                .map(|(index, name)| {
                    (
                        (*name).to_owned(),
                        serde_json::json!(counters.events[index]),
                    )
                })
                .collect::<serde_json::Map<_, _>>();
            let compact_total: u64 = counters.compact.iter().sum();
            let slow_total: u64 = counters.slow.values().sum();
            serde_json::json!({
                "schema": 1,
                "compact_total": compact_total,
                "slow_total": slow_total,
                "guest_total": compact_total,
                "handler_total": compact_total + slow_total,
                "compact": compact,
                "slow": slow,
                "events": events,
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

#[cfg(not(feature = "execution-trace"))]
pub fn emit() {}

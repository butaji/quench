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
    NamedPropertyHit => "named_property_hit",
    NamedPropertyMiss => "named_property_miss",
    EqualityWordHit => "equality_word_hit",
    EqualityWordMiss => "equality_word_miss",
    NamedPropertySetHit => "named_property_set_hit",
    NamedPropertySetMiss => "named_property_set_miss",
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
#[derive(Default)]
struct Counters {
    compact: [u64; crate::ir::Opcode::COUNT as usize + 1],
    slow: HashMap<&'static str, u64>,
    binary: HashMap<&'static str, u64>,
    events: [u64; EVENT_NAMES.len()],
    transitions: HashMap<(&'static str, &'static str), u64>,
    previous: Option<&'static str>,
    operand_transitions: HashMap<(OperandKey, OperandKey), u64>,
    previous_operand: Option<OperandKey>,
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
}

#[inline(always)]
#[cfg(not(feature = "execution-trace"))]
pub(crate) fn compact(_: crate::ir::Opcode) {}

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
pub(crate) fn slow(op: &crate::ops::Op) {
    if enabled() {
        COUNTERS.with(|counters| {
            let mut counters = counters.borrow_mut();
            let name = op.variant_name();
            *counters.slow.entry(name).or_default() += 1;
            if let crate::ops::Op::Binary { operator, .. } = op {
                *counters.binary.entry(binary_name(*operator)).or_default() += 1;
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
            let binary = counters
                .binary
                .iter()
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
            serde_json::json!({
                "schema": 2,
                "compact_total": compact_total,
                "slow_total": slow_total,
                "guest_total": compact_total,
                "handler_total": compact_total + slow_total,
                "compact": compact,
                "slow": slow,
                "binary": binary,
                "events": events,
                "transitions": transitions,
                "operand_transitions": operand_transitions,
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

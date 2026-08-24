//! Opt-in deterministic attribution of VM work.
//!
//! `QUENCH_EXEC_TRACE=1` enables counters. Disabled execution performs one
//! cached boolean branch and owns no counter state. The report is emitted by
//! the CLI after execution, keeping measurement I/O outside VM semantics.

#[cfg(feature = "execution-trace")]
use std::{cell::RefCell, collections::HashMap, sync::OnceLock};

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
                #[cfg(feature = "execution-trace")]
                if enabled() {
                    let counter = if allocated {
                        &heap_lifecycle::$allocated
                    } else {
                        &heap_lifecycle::$dropped
                    };
                    counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
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
    NamedSetReplacement => "named_set_replacement",
    NamedSetCacheEmpty => "named_set_cache_empty",
    NamedSetLayoutMismatch => "named_set_layout_mismatch",
    NamedSetSlotNotCell => "named_set_slot_not_cell",
    NamedSetPromoteCell => "named_set_promote_cell",
    CryptoKernelShape => "crypto_kernel_shape",
    CryptoKernelPrefix => "crypto_kernel_prefix",
    CryptoKernelProduct => "crypto_kernel_product",
    CryptoKernelStores => "crypto_kernel_stores",
    CryptoKernelHeader => "crypto_kernel_header",
    CryptoKernelInputs => "crypto_kernel_inputs",
    CryptoKernelInputStorage => "crypto_kernel_input_storage",
    CryptoKernelOutputStorage => "crypto_kernel_output_storage",
    CryptoKernelStorage => "crypto_kernel_storage",
    CryptoKernelBounds => "crypto_kernel_bounds",
    CryptoKernelHit => "crypto_kernel_hit",
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
    constant: HashMap<&'static str, u64>,
    environment_children: HashMap<(usize, usize), u64>,
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
    function_opcode_shapes: HashMap<(u16, u8, [u8; 16]), u64>,
    descriptor_objects: HashMap<&'static str, u64>,
    named_property_results: HashMap<&'static str, u64>,
    crypto_direct_iterations: u64,
    loop_shapes: HashMap<u64, (u64, u64, Vec<&'static str>)>,
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
            .iter()
            .map(|(name, _)| name.as_str())
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
        COUNTERS.with(|counters| {
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
            let Some(code) = code.filter(|code| code.len() <= 16) else {
                return;
            };
            let mut opcodes = [0; 16];
            for (pc, opcode) in opcodes.iter_mut().enumerate().take(code.len()) {
                *opcode = code
                    .instruction(pc)
                    .map_or(0, |instruction| instruction.opcode as u8);
            }
            *counters
                .function_opcode_shapes
                .entry((params, code.len() as u8, opcodes))
                .or_default() += 1;
        });
    }
}

#[cfg(feature = "execution-trace")]
fn dump_function_shape(params: u16, captures: usize, code: crate::machine::CodeView<'_>) {
    use std::hash::{Hash, Hasher};
    static ENABLED: OnceLock<bool> = OnceLock::new();
    static SEEN: OnceLock<std::sync::Mutex<std::collections::HashSet<u64>>> = OnceLock::new();
    if !*ENABLED.get_or_init(|| std::env::var_os("QUENCH_DUMP_FUNCTION_SHAPES").is_some()) {
        return;
    }
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    params.hash(&mut hasher);
    captures.hash(&mut hasher);
    for pc in 0..code.len() {
        let instruction = code.instruction(pc).expect("valid function instruction");
        (instruction.opcode as u8).hash(&mut hasher);
        instruction.flags.hash(&mut hasher);
        instruction.a.hash(&mut hasher);
        instruction.b.hash(&mut hasher);
        instruction.c.hash(&mut hasher);
        code.cold(instruction)
            .map(crate::ops::Op::variant_name)
            .hash(&mut hasher);
    }
    let fingerprint = hasher.finish();
    if !SEEN
        .get_or_init(Default::default)
        .lock()
        .expect("function shape trace lock")
        .insert(fingerprint)
    {
        return;
    }
    eprintln!(
        "FUNCTION_SHAPE params={params} captures={captures} len={} hash={fingerprint}",
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
        let binding_kind =
            |cell: &std::rc::Rc<std::cell::RefCell<crate::value::Value>>| match &*cell.borrow() {
                crate::value::Value::Number(_) => "number",
                crate::value::Value::Object(_) => "object",
                crate::value::Value::Function(_) => "function",
                crate::value::Value::Array(_) => "array",
                crate::value::Value::Boolean(_) => "boolean",
                crate::value::Value::String(_) => "string",
                _ => "other",
            };
        let kind = match (tier, value) {
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

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn named_property_result(_: &'static str, _: &crate::value::Value) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn crypto_direct_iterations(count: usize) {
    if enabled() {
        COUNTERS.with(|counters| {
            counters.borrow_mut().crypto_direct_iterations += count as u64;
        });
    }
}

#[cfg(not(feature = "execution-trace"))]
pub(crate) fn crypto_direct_iterations(_: usize) {}

#[cfg(feature = "execution-trace")]
pub(crate) fn loop_shape(body: crate::machine::CodeView<'_>) -> u64 {
    use std::hash::{Hash, Hasher};
    if !enabled() {
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
pub(crate) fn loop_shape_iteration(_: u64) {}

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
pub(crate) fn slow(_: &crate::ops::Op) {}

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
        });
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
                        serde_json::json!(counters.events[index]),
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
                |(&(params, len, opcodes), &count)| {
                    let opcodes = opcodes[..usize::from(len)].iter().filter_map(|opcode| {
                        crate::ir::Opcode::from_u8(*opcode).map(crate::ir::Opcode::name)
                    }).collect::<Vec<_>>();
                    serde_json::json!({"params": params, "opcodes": opcodes, "count": count})
                },
            ).collect::<Vec<_>>();
            let mut loop_shapes = counters.loop_shapes.iter().collect::<Vec<_>>();
            loop_shapes.sort_unstable_by_key(|(_, shape)| std::cmp::Reverse(shape.1));
            let loop_shapes = loop_shapes.into_iter().map(|(&fingerprint, shape)| {
                serde_json::json!({
                    "fingerprint": fingerprint,
                    "entries": shape.0,
                    "iterations": shape.1,
                    "ops": shape.2,
                })
            }).collect::<Vec<_>>();
            serde_json::json!({
                "schema": 2,
                "compact_total": compact_total,
                "slow_total": slow_total,
                "guest_total": compact_total,
                "handler_total": compact_total + slow_total,
                "compact": compact,
                "slow": slow,
                "binary": binary,
                "constant": constant,
                "environment_children": environment_children,
                "call_shapes": call_shapes,
                "call_targets": call_targets,
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
                "crypto_direct_iterations": counters.crypto_direct_iterations,
                "loop_shapes": loop_shapes,
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
    let _ = snapshot();
}

#[cfg(not(feature = "execution-trace"))]
pub fn emit() {}

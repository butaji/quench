//! The cell enum: every heap cell's variant and the methods that name, count and visit them.

use super::*;

#[derive(Clone, Debug)]
#[rustfmt::skip]
pub enum Cell {
    ArrayBuffer {
        object: Box<Object>,
        bytes: super::BufferBytes,
        shared: bool,
        detached: bool,
        max_byte_length: usize,
        resizable: bool,
        immutable: bool,
    },
    TypedArray {
        object: Box<BufferViewObject>,
        buffer: Value,
        length: ViewLength,
    },
    DataView {
        object: Box<BufferViewObject>,
        buffer: Value,
        length: ViewLength,
    },
    ShadowRealm {
        object: Box<Object>,
        caller_global: Value,
        realm_global: Value,
    },
    WeakMap {
        object: Box<Object>,
        entries: Box<WeakMapEntries>,
    },
    WeakSet {
        object: Box<Object>,
        entries: Box<WeakSetEntries>,
    },
    WeakRef {
        object: Box<Object>,
        target: Option<WeakHandle>,
    },
    FinalizationRegistry {
        object: Box<Object>,
        callback: Value,
        entries: Box<FinalizationEntries>,
    },
    ArrayFromAsyncState(Box<ArrayFromAsyncState>),
    AwaitState(Box<AwaitState>),
    Proxy {
        object: Box<Object>,
        kind: ProxyKind,
        target: Value,
        handler: Value,
    },
    BindingReference {
        environment: Value,
        slot: u16,
        kind: crate::bytecode::LexicalBindingKind,
    },
    // Immutable raw 64-bit Wasm scalars cannot fit the tagged Value payload.
    WasmBits64(u64),
    WasmV128([u8; crate::wasm::V128_BYTES]),
    /// Opaque external payload in the internal anyref hierarchy, outside eqref.
    WasmExtern(Value),
    /// The getter and setter of an accessor property whose shape keeps them in its slot
    /// (`ShapeAttributes::SLOT_ACCESSOR_PAIR`), so objects that each define their own pair, as
    /// an object literal's `get x() {}` and `set x(v) {}` do, share one shape. Only that slot
    /// holds one; no guest code sees it as a value.
    AccessorPair { getter: Value, setter: Value },
    /// One exception identity owns its original tag and traced payload.
    WasmException {
        tag: Value,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        payload: Box<Vec<Value>>,
    },
    /// A tag retains its original declaration; identity is independent of type equality.
    WasmTag { object: Box<Object>, declarations: crate::WasmTypes, ty: u32 },
    /// The native callable's immutable host operation and structural signature.
    WasmHostFunction {
        target: WasmHostTarget,
        signature: Rc<crate::wasm::WasmHostSignature>,
    },
    /// Immutable references available until an element segment drops.
    WasmElements(Vec<Value>),
    /// A `WebAssembly.Module` or `WebAssembly.Instance`.
    WasmObject { object: Box<Object>, slot: WasmObjectSlot },
    /// A global, memory or table is also its `WebAssembly.Global`, `.Memory` or `.Table`
    /// object, so every export of it is the same JavaScript object. `bits` is its one native
    /// slot, which compiled code reads and writes in place: the raw payload of an i32, i64, f32
    /// or f64 (a 32-bit one zero-extended), else the bits of the reference or vector handle,
    /// the only kind the collector follows (`Storage`).
    WasmGlobal { header: Box<WasmGlobalHeader>, bits: u64, ty: crate::WasmType, mutable: bool },
    /// The persistent context of a Wasm instance, which its environment holds in a slot of its
    /// own: native code reads the instance's words in place and rows name the instance by it.
    WasmInstance(crate::vm::InstanceRecord),
    /// One memory identity owns its bytes and original optional maximum. `buffer` is the
    /// ArrayBuffer currently viewing the bytes, or undefined until one is requested; growth
    /// detaches it.
    WasmMemory { header: Box<WasmMemoryHeader>, bytes: std::rc::Rc<crate::wasm::memory::MemoryStorage>, buffer: Value },
    /// Typed references owned by a Wasm instance, traced like other heap edges.
    WasmTable {
        object: Box<Object>,
        table64: bool,
        elements: Box<WasmTableElements>,
        element_type: wasmparser::RefType,
        declarations: crate::WasmTypes,
    },
    /// Digits inline up to `bigint::INLINE_DIGITS`, within the slot (`cells_stay_compact`).
    BigInt(crate::bigint::JsBigInt),
    Date { milliseconds: f64, object: Box<Object> },
    /// `meta` is shared: every evaluation of one literal site takes the site's record
    /// (`ProgramStore::regexp_literal_meta`), and `compile` replaces the receiver's copy.
    RegExp {
        object: Object,
        meta: Rc<RegExpMeta>,
    },
    Error(String),
    /// A promise: an ordinary object with its [[PromiseState]] and reactions.
    Promise(Box<super::PromiseCell>),
    PromiseResolvingState {
        promise: Value,
        already_resolved: bool,
    },
    TemporalDuration {
        object: Box<Object>,
        fields: Box<[f64; 10]>,
    },
    TemporalPlainDate {
        object: Box<Object>,
        year: i32,
        month: u32,
        day: u32,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
    },
    TemporalPlainDateTime {
        object: Box<Object>,
        date_time: Box<IsoDateTimeFields>,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
    },
    TemporalPlainMonthDay {
        object: Box<Object>,
        month: u32,
        day: u32,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
        reference_iso_year: i32,
    },
    TemporalPlainYearMonth {
        object: Box<Object>,
        year: i32,
        month: u32,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
        reference_iso_day: u32,
    },
    TemporalZonedDateTime {
        object: Box<Object>,
        instant: Box<ZonedInstant>,
        // Boxed so this payload keeps `Cell` narrow; the heap stores cells inline.
        #[allow(clippy::box_collection)]
        calendar: Box<String>,
    },
    TemporalInstant {
        object: Box<Object>,
        epoch_nanoseconds: Box<i128>,
    },
}

impl Cell {
    pub fn bigint(value: num_bigint::BigInt) -> Self {
        Self::BigInt(crate::bigint::JsBigInt::from(&value))
    }

    /// Whether a collection prunes this cell's entries by the liveness of their targets
    /// (`Heap::prune_weak_cell`).
    pub(crate) fn has_weak_entries(&self) -> bool {
        matches!(
            self,
            Self::WeakMap { .. }
                | Self::WeakSet { .. }
                | Self::WeakRef { .. }
                | Self::FinalizationRegistry { .. }
        )
    }

    /// The built-in class this cell's object is, by its kind alone: the one table behind
    /// inspection, embedder diagnostics and the structured-clone error text. Map and Set
    /// live outside `Cell` (`Heap::is_map` / `is_set`); callers check them, and keep their
    /// own spec-required exceptions in front of this table.
    pub(crate) fn class_name(&self) -> Option<&'static str> {
        Some(match self {
            Self::ArrayBuffer { shared: true, .. } => "SharedArrayBuffer",
            Self::ArrayBuffer { .. } => "ArrayBuffer",
            Self::TypedArray { object, .. } => object.native.kind().name(),
            Self::DataView { .. } => "DataView",
            Self::WeakMap { .. } => "WeakMap",
            Self::WeakSet { .. } => "WeakSet",
            Self::WeakRef { .. } => "WeakRef",
            Self::FinalizationRegistry { .. } => "FinalizationRegistry",
            Self::Date { .. } => "Date",
            Self::RegExp { .. } => "RegExp",
            Self::Error(_) => "Error",
            _ => return None,
        })
    }

    pub(crate) fn profile_variant_name(&self) -> &'static str {
        match self {
            Self::WasmObject { .. } => "wasm_object",
            Self::ArrayBuffer { .. } => "array_buffer",
            Self::TypedArray { .. } => "typed_array",
            Self::DataView { .. } => "data_view",
            Self::ShadowRealm { .. } => "shadow_realm",
            Self::WeakMap { .. } => "weak_map",
            Self::WeakSet { .. } => "weak_set",
            Self::WeakRef { .. } => "weak_ref",
            Self::FinalizationRegistry { .. } => "finalization_registry",
            Self::ArrayFromAsyncState(_) => "array_from_async_state",
            Self::AwaitState(_) => "await_state",
            Self::Proxy { .. } => "proxy",
            Self::BindingReference { .. } => "binding_reference",
            Self::WasmBits64(_) => "wasm_bits64",
            Self::WasmInstance(_) => "wasm_instance",
            Self::WasmV128(_) => "wasm_v128",
            Self::WasmExtern(_) => "wasm_extern",
            Self::AccessorPair { .. } => "accessor_pair",
            Self::WasmException { .. } => "wasm_exception",
            Self::WasmTag { .. } => "wasm_tag",
            Self::WasmHostFunction { .. } => "wasm_host_function",
            Self::WasmElements(_) => "wasm_elements",
            Self::WasmGlobal { .. } => "wasm_global",
            Self::WasmMemory { .. } => "wasm_memory",
            Self::WasmTable { .. } => "wasm_table",
            Self::BigInt(_) => "bigint",
            Self::Date { .. } => "date",
            Self::RegExp { .. } => "regexp",
            Self::Error(_) => "error",
            Self::PromiseResolvingState { .. } => "promise_resolving_state",
            Self::Promise(_) => "promise",
            Self::TemporalDuration { .. } | Self::TemporalPlainDate { .. } | Self::TemporalPlainDateTime { .. }
            | Self::TemporalPlainMonthDay { .. } | Self::TemporalPlainYearMonth { .. }
            | Self::TemporalZonedDateTime { .. } | Self::TemporalInstant { .. } => self.temporal_variant_name(),
        }
    }

    /// `profile_variant_name` of a Temporal cell.
    pub(super) fn temporal_variant_name(&self) -> &'static str {
        match self {
            Self::TemporalDuration { .. } => "temporal_duration",
            Self::TemporalPlainDate { .. } => "temporal_plain_date",
            Self::TemporalPlainDateTime { .. } => "temporal_plain_date_time",
            Self::TemporalPlainMonthDay { .. } => "temporal_plain_month_day",
            Self::TemporalPlainYearMonth { .. } => "temporal_plain_year_month",
            Self::TemporalZonedDateTime { .. } => "temporal_zoned_date_time",
            _ => "temporal_instant",
        }
    }
}

impl Cell {
    /// Each program whose code this cell can still run or resolve names in:
    /// a user function's code, an environment's scope, a suspended generator.
    pub(crate) fn program_references(&self, mut visit: impl FnMut(ProgramId)) {
        match self {
            Self::AwaitState(state) => state.program_references(&mut visit),
            _ => {}
        }
    }
}

/// The union of two ascending index sequences, ascending and without repeats.
pub(super) fn merge_ascending(
    left: impl Iterator<Item = usize>,
    right: impl Iterator<Item = usize>,
) -> Vec<usize> {
    let (mut left, mut right) = (left.peekable(), right.peekable());
    let mut merged = Vec::with_capacity(left.size_hint().0 + right.size_hint().0);
    loop {
        let next = match (left.peek().copied(), right.peek().copied()) {
            (Some(l), Some(r)) if l <= r => {
                left.next();
                l
            }
            (_, Some(r)) => {
                right.next();
                r
            }
            (Some(l), None) => {
                left.next();
                l
            }
            (None, None) => return merged,
        };
        if merged.last() != Some(&next) {
            merged.push(next);
        }
    }
}

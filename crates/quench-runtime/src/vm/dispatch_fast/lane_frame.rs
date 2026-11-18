//! The interpreter's register frame: `staged` moves and constants over the live file.

use super::*;
use super::lane_reads::Reads;
use crate::bytecode::{Operand, Register};
use crate::typed_ir::Type;
use crate::bytecode::RegisterWindow;
use crate::staged::{Allocation, CallOut, Comparison, Concrete, Constant, Edge, Frame, GlobalBinding, NumericFrame, Operation, Protocol, Step, WasmRep, read_initialized_local, write_initialized_local, write_local};

/// The maker of a frame's heap values: allocates in the lane context it is given and runs no
/// guest code; `None` leaves the instruction to the general path.
type AllocationMaker = unsafe fn(*mut (), Registers, Allocation) -> Option<Value>;

/// The definer of a frame's array literal element (`lane_define_element`): the VM's definition of
/// `value` at `index` into the literal array `array`; false leaves the definition to the general path.
type ElementDefiner = unsafe fn(*mut (), u64, u32, u64) -> bool;

/// The deleter of a frame's own `delete base[key]` hit: the `true` of a deletion that runs no
/// guest code; `None` leaves the instruction to the general path.
type PropertyDeleter = unsafe fn(*mut (), Value, Value) -> Option<Value>;

/// The reader of a frame's by-name load: the value of `atom` under a binding the record's first
/// run resolved, which runs no guest code; `None` leaves the instruction to the residual.
type NameReader = unsafe fn(*mut (), NameRead, u32) -> Option<Value>;

/// The fast answer of a `Names` row's iteration or for-in call-out over its operands (`fast_names_step`),
/// which runs no guest code; `None` leaves the call to the row's miss.
type NamesStep = unsafe fn(*mut (), CallOut, [Option<Value>; 2]) -> Option<Value>;

/// How a by-name load's record reads its name once resolved (`lane_name_resolution`).
#[derive(Clone, Copy)]
pub(super) enum NameRead {
    /// The active realm's function for the native the name stands for (`Vm::intrinsic_natives`).
    Native,
    /// An intrinsic name's value (`Vm::intrinsic_name_value`).
    Intrinsic,
    /// The global object's own data field the record's hint names.
    Field(crate::vm::FieldCache),
}

/// What a by-name load's record resolved its name to: the global `undefined`, a constant no
/// binding changes, or a read of the VM's heap.
#[derive(Clone, Copy)]
pub(super) enum NameBinding {
    Undefined,
    Read(NameRead),
}

/// The lane context (type-erased, so the frame is one type for every host) and what it makes
/// for a staged row: an allocation, a property deletion or a by-name load's value.
#[derive(Clone, Copy)]
struct LaneHost {
    cx: *mut (),
    allocate: AllocationMaker,
    define: ElementDefiner,
    delete: PropertyDeleter,
    read_name: NameReader,
    /// What the by-name load the frame runs resolved its name to, if it runs one.
    name: Option<NameBinding>,
}

/// The lane's environment clone for a `CloneEnv` row (`clone_environment_of`): the lane context, the
/// register count of the environment window the record names (0 where it names none), and the clone.
#[derive(Clone, Copy)]
struct EnvironmentClones {
    context: *mut (),
    window: u16,
    run: unsafe fn(*mut (), Registers, u16, u32) -> bool,
}

/// The lane context's capture stores, erased of its host so the frame names no `H`
/// (`lane_capture_store`).
#[derive(Clone, Copy)]
struct CaptureStores {
    context: *mut (),
    store: fn(*mut (), u16, u16, Value) -> bool,
}

impl CaptureStores {
    /// The stores of the running lane's `cx`.
    fn of<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { context: cx.cast(), store: |context, depth, slot, value| lane_capture_store(context.cast::<LaneContext<H>>(), depth, slot, value) }
    }

    /// The running function's own initialized slot written directly through its dense slots
    /// (`lane_store_own_capture`), over the register file `r`; any other store is the row's miss
    /// (`row_miss`: the lane's walking store).
    #[inline(always)]
    fn own(r: Registers) -> Self {
        Self { context: r.0.cast(), store: |context, depth, slot, value| depth == 0
            && super::capture_stores(Registers(context.cast()).capture())
                // SAFETY: the running function's dense capture slots, which admit this write
                // (`CAPTURE_STORES`); no collection runs in the lane.
                .and_then(|slots| unsafe { crate::heap::EnvironmentSlots::write_storage(slots, usize::from(slot), value) })
                .is_some() }
    }
}

/// The lane context's steady decisions of a pair the words leave, erased of its host so the frame
/// names no `H` (`lane_decision`).
#[derive(Clone, Copy)]
struct Decisions {
    context: *mut (),
    decide: fn(*mut (), u32, Value, Value) -> Option<bool>,
    /// The encoded `prototype` link of a constructor the VM proved Stable (`Vm::instanceof_fact`).
    prototype: fn(*mut (), Value) -> Option<u32>,
    /// The base of the object record table.
    records: fn(*mut ()) -> Option<u64>,
}

impl Decisions {
    /// The decisions of the running lane's `cx`.
    fn of<H: Host>(cx: *mut LaneContext<H>) -> Self {
        Self { context: cx.cast(), decide: |context, operator, left, right|
            super::vm_steps::lane_decision(unsafe { vm(context.cast::<LaneContext<H>>()) }, operator, left, right),
            prototype: |context, constructor| unsafe { vm(context.cast::<LaneContext<H>>()) }.instanceof_fact(constructor)
                .map(|fact| fact.link),
            records: |context| unsafe { vm(context.cast::<LaneContext<H>>()) }.heap.object_record_base().map(|base| base as u64) }
    }
}

/// The interpreter's frame: the live register file and the accumulator view a producer
/// carries. `ACC` names the record's accumulator field (`read`).
pub(in crate::vm::dispatch_fast) struct LaneFrame<const ACC: u8> {
    registers: Registers,
    view: Carried,
    /// The activation's `this` (`LaneContext::this`): `Value::DELETED` where the lane does
    /// not hold it.
    this: Value,
    /// The activation's local slots and their count, for a staged local operation.
    locals: Option<(*mut Value, usize)>,
    /// The lane context and what it makes, for a staged allocation or deletion row.
    host: Option<LaneHost>,
    /// The capture stores of the running lane, for a staged capture store.
    captures: Option<CaptureStores>,
    /// The lane context's string-case index for a `StringCaseIndex` row (`case_index_in_lane`): the record's
/// filter, null where the record has none.
#[derive(Clone, Copy)]
struct StringCases {
    context: *mut (),
    filter: *const super::string_case_filter::StringCaseFilter,
    index: unsafe fn(*mut (), *const super::string_case_filter::StringCaseFilter, Value, usize, usize) -> u32,
}

/// The lane context's suspension of an `Await` row (`generator_yields::await_in_lane`), erased of its
/// host: the record the row runs (its position names the resume) and the suspend itself.
#[derive(Clone, Copy)]
struct Suspension {
    context: *mut (),
    record: *const LaneInstruction,
    suspend: unsafe fn(*mut (), *const LaneInstruction, Registers, Register, Register) -> Option<()>,
}

/// The lane context's transfer of a `Yield` or `YieldStar` row, erased of its host: the record the row runs
/// and the window's resume, which answers `None` where no generator runs the lane.
#[derive(Clone, Copy)]
struct YieldSuspension {
    context: *mut (),
    record: *const LaneInstruction,
    resume: unsafe fn(*mut (), *const LaneInstruction, Registers, Register) -> Option<Resumed>,
}

/// The running lane's steady decisions, for a staged comparison jump.
    decisions: Option<Decisions>,
    /// The slots the running function's captures read from (`capture_reads`), for a staged capture read.
    capture_slots: Option<*const usize>,
    /// The heap whose records a row reads in place (`Frame::heap_records`), where the row is one
    /// that does (`Decoder::Names`).
    heap: Option<*const crate::heap::Heap>,
    /// The running frame's `this` where the lane's context word does not hold it (`lane_this`),
    /// read only by a row that asks for `this`.
    frame_this: Option<(*mut (), unsafe fn(*mut ()) -> Option<Value>)>,
    /// The running function's `arguments` count in a local slot (`arguments_count`), read only
    /// by the row that asks for it.
    arguments: Option<(*mut (), unsafe fn(*mut (), u16) -> Option<Value>)>,
    /// The running frame's captured environment, for a staged environment row (`Decoder::Names`).
    environment: Option<EnvironmentReads>,
    /// The global object's own data field a global-name store writes through the record's hint,
    /// where the row is a `StoreName` (`global_name_store`).
    global_store: Option<GlobalStore>,
    /// The lane's pure binary operation (`lane_binary_values`), which answers a `Binary` or an
    /// `Equality` call-out a row makes where its words do not decide it (`vm_step`).
    binary: Option<(*mut (), unsafe fn(*mut (), u32, Value, Value) -> Option<Value>)>,
    /// The lane context and the fast answers of an iteration or for-in call-out (`fast_names_step`), for
    /// the row that makes one (`Decoder::Names`).
    names: Option<(*mut (), NamesStep)>,
    /// The environment clone a `CloneEnv` row makes (`clone_environment_of`).
    clones: Option<EnvironmentClones>,
    /// The iterator cleanup stack an `IteratorCleanupPush`/`Pop` row keeps (`iterator_push_in_lane`).
    cleanups: Option<IteratorCleanups>,
    /// The string-case index a `StringCaseIndex` row answers (`case_index_in_lane`).
    string_cases: Option<StringCases>,
    /// The suspension an `Await` row makes (`await_in_lane`).
    suspension: Option<Suspension>,
    /// The window transfer a `Yield` or `YieldStar` row makes (`generator_yields::yield_in_lane`,
    /// `yield_star_in_lane`), where the row is one.
    yields: Option<YieldSuspension>,
    /// The resumption a transfer hands the lane (`Edge::Transfer`), taken by `lane_row`.
    transfer: Option<Resumed>,
    /// The running function, which a `LoadCallee` row reads (`running_callee`), where the row is one.
    callee: Option<(*mut (), unsafe fn(*mut ()) -> Option<Value>)>,
    /// The lane context and the VM's Wasm global load and store, for a staged instance-state row
    /// (`Decoder::WasmState`).
    globals: Option<WasmGlobals>,
    /// The function's linear memory a direct access row addresses (`Decoder::WasmMemory`): the base
    /// and length the view resolved (`MemoryView::extent`).
    memory: Option<(*mut u8, usize)>,
    /// The lane context's resolver of a Wasm memory value, for a generic memory row that names its
    /// memory by register (`Frame::wasm_memory`), where the row is one (`Decoder::WasmMemory`).
    memories: Option<WasmMemories>,
    /// The proof span of the array a proved element row (`FLAG_SET`) reads or stores through, with the
    /// lane context it proves in (`proof_span`).
    element_span: Option<(*mut u8, ElementSpan)>,
    /// The constants, local slots and outer captures the staged row's operands name.
    reads: Reads,
    /// The window of an environment activation's run, where a window record reads its locals
    /// (`FLAG_WINDOW`): the register at its base, whose locals follow its environment and slots
    /// words (`WINDOW_LOCALS`). `None` in a frame's run.
    window: Option<*mut Value>,
}

/// The proof span of an array a proved element row names (`dispatch_fast::proof_span`): the lane
/// context's bytes, the base register, its value, and whether a store writes it.
type ElementSpan = unsafe fn(*mut u8, u16, Value, bool) -> Option<(std::ptr::NonNull<Value>, usize)>;

/// The lane context's resolver of a Wasm memory value for a generic memory row: the bytes of the
/// memory the value names, and whether its index is an i64 (`wasm_memory_of`).
#[derive(Clone, Copy)]
struct WasmMemories {
    context: *mut (),
    resolve: unsafe fn(*mut (), Value) -> Option<(*mut u8, usize, bool)>,
}

/// `WasmMemories::resolve`: the bytes of the Wasm memory `value` names, as `direct_memory` gives
/// them, and whether its index type is i64. `None` where the memory is not addressed directly.
///
/// # Safety
/// `context` is the running lane's context.
unsafe fn wasm_memory_of<H: Host>(context: *mut (), value: Value) -> Option<(*mut u8, usize, bool)> {
    let vm = unsafe { vm(context.cast::<LaneContext<H>>()) };
    let (bytes, length) = direct_memory(vm, value)?;
    let wide = vm.wasm_memory_index_type(value).ok()? == crate::WasmType::I64;
    Some((bytes, length, wide))
}

/// The lane context's Wasm global rules for a staged instance-state row: the VM's load of the
/// global bound in a binding and its store (`wasm_global_load`, `wasm_global_store`).
#[derive(Clone, Copy)]
struct WasmGlobals {
    context: *mut (),
    load: unsafe fn(*mut (), Value) -> Option<Value>,
    store: unsafe fn(*mut (), Value, Value) -> bool,
}

/// The running frame's captured environment for a staged environment row: the storage of its
/// slots, which `read` finds for a read and `write` for a store (it takes the environment's barrier).
#[derive(Clone, Copy)]
struct EnvironmentReads {
    context: *mut (),
    read: unsafe fn(*mut ()) -> Option<*mut crate::heap::EnvironmentSlots>,
    write: unsafe fn(*mut (), u16) -> Option<*mut crate::heap::EnvironmentSlots>,
}

/// The lane context's iterator cleanup stack for an `IteratorCleanupPush`/`Pop` row (`iterator_push_in_lane`,
/// `iterator_pop_in_lane`), erased of its host so the frame names no `H`.
#[derive(Clone, Copy)]
struct IteratorCleanups {
    context: *mut (),
    push: unsafe fn(*mut (), Register, Register),
    pop: unsafe fn(*mut ()) -> bool,
}

/// A row's global-name store: the lane context and the record's own hint, which the store
/// writes through (`global_name_store`).
#[derive(Clone, Copy)]
struct GlobalStore {
    cx: *mut (),
    hint: crate::vm::FieldCache,
    store: unsafe fn(*mut (), crate::vm::FieldCache, u32, Value) -> bool,
}

impl<const ACC: u8> LaneFrame<ACC> {
    /// The transfer of a `Yield` or `YieldStar` row whose operand is `operand`: the window's resumption,
    /// held for `lane_row` to take (`Edge::Transfer`); `None` where the window declines.
    fn transfer_yield(&mut self, operand: Register) -> Option<Edge> {
        let yields = self.yields?;
        // SAFETY: the running lane's context, record and register file.
        let resumed = unsafe { (yields.resume)(yields.context, yields.record, self.registers, operand) }?;
        self.transfer = Some(resumed);
        Some(Edge::Transfer)
    }

    /// The resumption the last transfer made (`Edge::Transfer`).
    fn take_transfer(&mut self) -> Resumed {
        self.transfer.take().unwrap_or_else(|| unreachable!("a transfer edge carries its resumption"))
    }

    /// A frame for an operation that reads no `this`.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, this: Value::DELETED, locals: None, host: None, captures: None, decisions: None,
            capture_slots: None, heap: None, frame_this: None, arguments: None, environment: None, global_store: None, binary: None, names: None, clones: None, cleanups: None, string_cases: None, suspension: None, yields: None, transfer: None, callee: None, globals: None, memory: None, memories: None, element_span: None, reads: Reads::NONE }
            capture_slots: None, heap: None, frame_this: None, arguments: None, environment: None, global_store: None, binary: None, names: None, clones: None, cleanups: None, string_cases: None, suspension: None, callee: None, globals: None, memory: None, memories: None, element_span: None, reads: Reads::NONE, window: None }
    }

    /// A frame whose allocation and deletion rows make their values in the lane context `cx`.
    #[inline(always)]
    fn hosted<H: Host>(registers: Registers, view: Carried, cx: *mut LaneContext<H>) -> Self {
        let host = LaneHost { cx: cx.cast(), allocate: lane_allocation::<H>, define: lane_define_element::<H>, delete: lane_delete_value::<H>,
            read_name: lane_name_value::<H>, name: None };
        Self { host: Some(host), ..Self::new(registers, view) }
    }

    /// This frame running a by-name load whose record resolved its name to `binding`.
    #[inline(always)]
    fn named(mut self, binding: NameBinding) -> Self {
        if let Some(host) = self.host.as_mut() {
            host.name = Some(binding);
        }
        self
    }

    /// A frame that may read and write the activation's local slots.
    fn with_locals(registers: Registers, view: Carried, this: Value,
        locals: *mut Value, len: usize) -> Self {
        Self { registers, view, this, locals: Some((locals, len)), host: None, captures: None, decisions: None,
            capture_slots: None, heap: None, frame_this: None, arguments: None, environment: None, global_store: None, binary: None, names: None, clones: None, cleanups: None, string_cases: None, suspension: None, yields: None, transfer: None, callee: None, globals: None, memory: None, memories: None, element_span: None, reads: Reads::NONE }
            capture_slots: None, heap: None, frame_this: None, arguments: None, environment: None, global_store: None, binary: None, names: None, clones: None, cleanups: None, string_cases: None, suspension: None, callee: None, globals: None, memory: None, memories: None, element_span: None, reads: Reads::NONE, window: None }
    }
}

/// The operands and operator of a binary call-out the lane's pure binary answers: a `Binary`, or an
/// `Equality` as the operator of its strictness and negation.
fn binary_call(operation: CallOut) -> Option<(Operand, Operand, u32)> {
    match operation {
        CallOut::Binary { left, right, operator } => Some((left, right, operator)),
        CallOut::Equality { left, right, strict, negated } =>
            Some((left, right, crate::vm::operations::equality_binary_operator(strict, negated))),
        _ => None,
    }
}

/// The lane frame's fact about a word: how it becomes a register's `Value`. Staged rows hand an
/// int32 result already boxed (`NumericFrame::int_result`) and a binary64 result as its raw bits
/// (`number_fact`), which the write boxes.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(in crate::vm::dispatch_fast) enum Boxing {
    Value,
    Double,
}

impl<const ACC: u8> Frame<Concrete> for LaneFrame<ACC> {
    type Fact = Boxing;
    type Control = Edge;

    #[inline(always)]
    fn fact(&self, _register: Register) -> Self::Fact { Boxing::Value }

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        // The carried accumulator stands for field B, the one register a single-operand row reads;
        // a row that reads several (one with heap records) reads the file, which every producer writes.
        if self.heap.is_some() { return self.registers.get(register).to_bits(); }
        read::<ACC, ACC_B>(self.registers, register, self.view).to_bits()
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, fact: Self::Fact) {
        let value = match fact {
            Boxing::Value => Value::from_bits(value),
            Boxing::Double => Value::number(f64::from_bits(value)),
        };
        self.registers.set(register, value);
    }

    #[inline(always)]
    fn wide_read(&mut self, stage: &mut Concrete, register: Register, raw: bool) -> Option<u64> {
        let word = self.read(stage, register);
        if raw { Some(word) } else { Value::from_bits(word).bits64() }
    }

    #[inline(always)]
    fn wide_write(&mut self, _stage: &mut Concrete, register: Register, value: u64, raw: bool) -> Option<()> {
        let value = if raw { Value::from_bits(value) } else { Value::from_bits64(value)? };
        self.registers.set(register, value);
        Some(())
    }

    #[inline(always)]
    fn this(&mut self, _stage: &mut Concrete) -> u64 {
        if self.this.is_deleted() && let Some((context, read)) = self.frame_this {
            // SAFETY: the running lane's context, live for the row's step.
            return unsafe { read(context) }.unwrap_or(Value::DELETED).to_bits();
        }
        self.this.to_bits()
    }

    #[inline(always)]
    fn define_this(&mut self, _stage: &mut Concrete, destination: Register, this: u64) {
        self.registers.set(destination, Value::from_bits(this));
    }

    #[inline(always)]
    fn constant(&mut self, _stage: &mut Concrete, constant: Constant) -> (u64, Self::Fact) {
        (constant.value.to_bits(), Boxing::Value)
    }

    #[inline(always)]
    fn truthiness(&mut self, stage: &mut Concrete, register: Register) -> Option<bool> {
        Value::from_bits(self.read(stage, register)).truthiness()
    }

    #[inline(always)]
    fn branch(&mut self, _stage: &mut Concrete, condition: bool) -> Edge {
        if condition { Edge::Target } else { Edge::Following }
    }

    #[inline(always)]
    fn jump(&mut self, _stage: &mut Concrete) -> Edge {
        Edge::Target
    }

    #[inline(always)]
    fn branch_table(&mut self, _stage: &mut Concrete, index: u64, entries: u32) -> Option<Edge> {
        // The selector is an int32 word, zero-extended; any selector past the cases takes the default.
        Some(Edge::Entry((index as u32).min(entries - 1)))
    }

    #[inline(always)]
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, Boxing) {
        (Value::boolean(condition).to_bits(), Boxing::Value)
    }

    /// The local `slot` of the running activation: its window's where the run holds one
    /// (`window`), else the frame's. An uninitialized binding (the deleted marker) is the row's miss.
    #[inline(always)]
    fn load_local(&mut self, _stage: &mut Concrete, slot: u16, _miss: Option<()>)
        -> Option<(u64, Boxing)> {
        let value = match self.window {
            // SAFETY: a window record's slot is one of its function's locals, which the window holds
            // after its environment and slots words (`lane_activate_environment`).
            Some(window) => unsafe { *window.add(usize::from(super::WINDOW_LOCALS) + usize::from(slot)) },
            None => {
                let (locals, len) = self.locals?;
                if usize::from(slot) >= len {
                    return None;
                }
                // SAFETY: `slot` is inside the activation's live local slots.
                unsafe { *locals.add(usize::from(slot)) }
            }
        };
        (!value.is_deleted()).then_some((value.to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn allocate_value(&mut self, _stage: &mut Concrete, destination: Register, allocation: Allocation, _proof: ()) -> Option<u64> {
        let host = self.host?;
        // SAFETY: the maker was made for the lane context this frame runs in.
        let value = unsafe { (host.allocate)(host.cx, self.registers, allocation) }?;
        self.registers.set(destination, value);
        Some(value.to_bits())
    }

    #[inline(always)]
    fn define_element(&mut self, _stage: &mut Concrete, array: u64, index: u32, value: u64) -> Option<()> {
        let host = self.host?;
        // SAFETY: the definer was made for the lane context this frame runs in.
        unsafe { (host.define)(host.cx, array, index, value) }.then_some(())
    }

    /// Writes local `slot` of the running activation, its window's where the run holds one (`window`).
    /// The window's environment is young (`window_environment`) or remembered, so a plain local
    /// needs no barrier: no collection runs in the lane.
    #[inline(always)]
    fn suspend_await(&mut self, _stage: &mut Concrete, destination: Register, source: Register) -> Option<Edge> {
        let suspension = self.suspension?;
        // SAFETY: the running lane's context, record and register file.
        unsafe { (suspension.suspend)(suspension.context, suspension.record, self.registers, destination, source) }?;
        Some(Edge::Leave)
    }

    #[inline(always)]
    fn suspend_yield(&mut self, _stage: &mut Concrete, value: Register) -> Option<Edge> {
        self.transfer_yield(value)
    }

    #[inline(always)]
    fn suspend_yield_star(&mut self, _stage: &mut Concrete, iterable: Register) -> Option<Edge> {
        self.transfer_yield(iterable)
    }

    #[inline(always)]
    fn case_index(&mut self, stage: &mut Concrete, source: Register, start: u32, count: u16) -> Option<(u64, Self::Fact)> {
        let cases = self.string_cases?;
        let value = Value::from_bits(self.read(stage, source));
        // SAFETY: the running lane's context, and the filter the record's operand names (null where none).
        let index = unsafe { (cases.index)(cases.context, cases.filter, value, start as usize, usize::from(count)) };
        Some((Value::integer(index as i32).to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn iterator_push(&mut self, _stage: &mut Concrete, iterator: Register, done: Register) -> Option<()> {
        let cleanups = self.cleanups?;
        // SAFETY: the context the cleanup stack was made for is the running lane's.
        unsafe { (cleanups.push)(cleanups.context, iterator, done) };
        Some(())
    }

    #[inline(always)]
    fn iterator_pop(&mut self, _stage: &mut Concrete) -> Option<()> {
        let cleanups = self.cleanups?;
        // SAFETY: as `iterator_push`.
        unsafe { (cleanups.pop)(cleanups.context) }.then_some(())
    }

    #[inline(always)]
    fn store_local(&mut self, _stage: &mut Concrete, slot: u16, value: u64, _miss: Option<()>) -> bool {
        match self.window {
            // SAFETY: as `load_local`'s read of the window's local.
            Some(window) => unsafe { *window.add(usize::from(super::WINDOW_LOCALS) + usize::from(slot)) = Value::from_bits(value) },
            None => {
                let Some((locals, len)) = self.locals else { return false };
                if usize::from(slot) >= len { return false; }
                // SAFETY: `slot` is inside the activation's live local slots.
                unsafe { *locals.add(usize::from(slot)) = Value::from_bits(value) };
            }
        }
        true
    }

    #[inline(always)]
    fn heap_records(&mut self, _stage: &mut Concrete) -> Option<(u64, u64, u64)> {
        // SAFETY: the running lane's heap, live for the row's step.
        let heap = unsafe { &*self.heap? };
        Some((heap.array_record_base()? as u64, heap.object_record_base()? as u64, heap.collection_epoch()))
    }

    #[inline(always)]
    fn vm_step(&mut self, stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, Boxing)> {
        if let Some((context, binary)) = self.binary
            && let Some((left, right, operator)) = binary_call(operation)
        {
            let (left, right) = (self.operand_word(stage, left)?, self.operand_word(stage, right)?);
            let (left, right) = (Value::from_bits(left), Value::from_bits(right));
            // SAFETY: the running lane's context, live for the row's step.
            return unsafe { binary(context, operator, left, right) }.map(|value| (value.to_bits(), Boxing::Value));
        }
        match operation {
            CallOut::Decide { left, right, operator } => {
                let Decisions { context, decide, .. } = self.decisions?;
                let (left, right) = (Value::from_bits(self.read(stage, left)), Value::from_bits(self.read(stage, right)));
                decide(context, operator, left, right).map(|holds| (Value::boolean(holds).to_bits(), Boxing::Value))
            }
            CallOut::Protocol { .. } | CallOut::ForInNext { .. } => {
                let (context, step) = self.names?;
                let operands = operation.operands().fixed;
                let operands = operands.map(|operand| operand.and_then(|operand| self.operand_word(stage, operand)).map(Value::from_bits));
                // SAFETY: the running lane's context, live for the row's step.
                unsafe { step(context, operation, operands) }.map(|value| (value.to_bits(), Boxing::Value))
            }
            // The clone defines no register; its word is a placeholder the row does not write.
            CallOut::CloneEnv { plan } => {
                let EnvironmentClones { context, window, run } = self.clones?;
                // SAFETY: the running lane's context and register file, live for the row's step.
                unsafe { run(context, self.registers, window, plan) }.then_some((Value::UNDEFINED.to_bits(), Boxing::Value))
            }
            _ => None,
        }
    }

    #[inline(always)]
    fn arguments_length(&mut self, _stage: &mut Concrete, slot: u16) -> Option<(u64, Boxing)> {
        let (context, count) = self.arguments?;
        // SAFETY: the running lane's context, live for the row's step.
        Some((unsafe { count(context, slot) }?.to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn callee(&mut self, _stage: &mut Concrete) -> Option<(u64, Boxing)> {
        let (context, read) = self.callee?;
        // SAFETY: the running lane's context, live for the row's step.
        Some((unsafe { read(context) }?.to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn capture_slot(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, root: bool) -> Option<(u64, Boxing)> {
        // The own scope's slot first, each test its own branch: the read is the hot capture.
        if depth == 0 && !root
            && let Some(slots) = self.capture_slots
            // SAFETY: the running function's dense storage, found this run
            // (`LaneContext::capture`); the scoped read retains the bounds and marker checks.
            && let Some(value) = unsafe { crate::heap::EnvironmentSlots::read_storage(slots, slot) }
        {
            return Some((value.to_bits(), Boxing::Value));
        }
        Some((self.reads.capture(depth, slot, root)?.to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn store_capture(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, value: u64) -> Option<()> {
        let CaptureStores { context, store } = self.captures?;
        store(context, depth, slot, Value::from_bits(value)).then_some(())
    }

    #[inline(always)]
    fn environment(&mut self, _stage: &mut Concrete) -> Option<(u64, Boxing)> {
        let EnvironmentReads { context, read, .. } = self.environment?;
        // SAFETY: the running lane's context, live for the row's step.
        let storage = unsafe { read(context) }?;
        Some((storage as u64, Boxing::Value))
    }

    #[inline(always)]
    fn environment_write(&mut self, _stage: &mut Concrete, slot: u16, value: u64) -> Option<()> {
        let EnvironmentReads { context, write, .. } = self.environment?;
        // SAFETY: as `environment`; `write` took the environment's barrier, and no collection runs in the lane.
        let storage = unsafe { write(context, slot) }?;
        unsafe { crate::staged::environment::concrete_environment_slot_store(storage, slot, Value::from_bits(value)) }
            .then_some(())
    }

    /// A Wasm scalar read straight from the register file (never the carried accumulator, which
    /// stands for one operand only). An i32 or f32 is its encoded integer; a 64-bit payload is held
    /// in place where untagged; a raw f64 is its word.
    #[inline(always)]
    fn wasm_read(&mut self, _stage: &mut Concrete, register: Register, rep: WasmRep) -> Option<u64> {
        let value = self.registers.get(register);
        Some(match rep {
            WasmRep::Int32 => u64::from(value.as_int()? as u32),
            WasmRep::Bits64 => value.bits64()?,
            WasmRep::RawBits64 | WasmRep::Tagged => value.to_bits(),
        })
    }

    /// Define `register` with a Wasm scalar `word` in `rep`'s representation: an i32 or f32 as an
    /// encoded integer, a boxed 64-bit payload only where it is untagged (else the row leaves), a
    /// raw f64 or a tagged word as it is.
    #[inline(always)]
    fn wasm_write(&mut self, _stage: &mut Concrete, register: Register, word: u64, rep: WasmRep) -> Option<u64> {
        let value = match rep {
            WasmRep::Int32 => self.view.integers.encode(word as u32 as i32),
            WasmRep::Bits64 => Value::from_bits64(word)?,
            WasmRep::RawBits64 | WasmRep::Tagged => Value::from_bits(word),
        };
        self.registers.set(register, value);
        Some(value.to_bits())
    }

    /// The VM's load of the global bound in `binding`, where the frame holds the rules (`globals`).
    #[inline(always)]
    fn wasm_global_load(&mut self, _stage: &mut Concrete, binding: u64) -> Option<u64> {
        let globals = self.globals?;
        // SAFETY: the context is the running lane's, which `row_frame` took it from.
        let value = unsafe { (globals.load)(globals.context, Value::from_bits(binding))? };
        Some(value.to_bits())
    }

    /// The VM's store of `value` to the mutable global bound in `binding`, where the frame holds the
    /// rules (`globals`); a binding that is no such global leaves the row to its miss.
    #[inline(always)]
    fn wasm_global_store(&mut self, _stage: &mut Concrete, binding: u64, value: u64) -> Option<()> {
        let globals = self.globals?;
        // SAFETY: as for `wasm_global_load`.
        let stored = unsafe { (globals.store)(globals.context, Value::from_bits(binding), Value::from_bits(value)) };
        stored.then_some(())
    }

    /// The bytes of the function's memory, where the row addresses them directly (`memory`).
    #[inline(always)]
    fn direct_memory(&mut self) -> Option<&mut [u8]> {
        let (bytes, length) = self.memory?;
        if length == 0 {
            return Some(Default::default());
        }
        // SAFETY: `bytes` is the memory the view resolved in this lane run, `length` bytes long; the
        // frame holds no other borrow of it.
        Some(unsafe { std::slice::from_raw_parts_mut(bytes, length) })
    }

    /// The bytes of the Wasm memory value in `register`, resolved through the lane context
    /// (`wasm_memory_of`), and its index type. A memory the lane cannot address leaves the row.
    #[inline(always)]
    fn wasm_memory(&mut self, _stage: &mut Concrete, register: Register) -> Option<(&mut [u8], bool)> {
        let WasmMemories { context, resolve } = self.memories?;
        // SAFETY: the context is the running lane's, live for the row's step.
        let (bytes, length, wide) = unsafe { resolve(context, self.registers.get(register)) }?;
        if length == 0 {
            return Some((Default::default(), wide));
        }
        // SAFETY: `bytes` is the memory the value names in this lane run, `length` bytes long; the
        // frame holds no other borrow of it.
        Some((unsafe { std::slice::from_raw_parts_mut(bytes, length) }, wide))
    }
}

/// The running frame's environment storage and the environment that holds it, where a staged
/// environment row may read its slots: a captured frame of a non-root function whose every local
/// lives in its environment and whose `arguments` alias no parameter, in a run that holds no
/// environment window. `store` names the slot a write targets, which the function's self binding
/// keeps from the row.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn environment_of<H: Host>(cx: *mut LaneContext<H>, store: Option<u16>)
    -> Option<(*mut crate::heap::EnvironmentSlots, Value)> {
    if !unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let storage = super::lane_frame_storage(cx)?;
    let vm = unsafe { vm(cx) };
    let frame = &vm.frames[unsafe { (*cx).frame() }];
    let function = unsafe { &*(*cx).code }.function(frame.function as usize);
    if function.selective_capture_slots.is_some() || function.arguments_alias_parameters()
        || store.is_some_and(|slot| function.is_self_binding_slot(usize::from(slot))) {
        return None;
    }
    Some((std::ptr::from_mut(storage), frame.env))
}

/// `EnvironmentReads::read`: the storage `environment_of` finds for a read.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn environment_read<H: Host>(cx: *mut ()) -> Option<*mut crate::heap::EnvironmentSlots> {
    unsafe { environment_of::<H>(cx.cast(), None) }.map(|(storage, _)| storage)
}

/// `EnvironmentReads::write`: the storage `environment_of` finds for a store to `slot`, after the
/// environment's write barrier (`Heap::remember_cell`), as `store_local_miss` takes it.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn environment_write_storage<H: Host>(cx: *mut (), slot: u16) -> Option<*mut crate::heap::EnvironmentSlots> {
    let (storage, environment) = unsafe { environment_of::<H>(cx.cast(), Some(slot)) }?;
    unsafe { vm(cx.cast::<LaneContext<H>>()) }.heap.remember_cell(environment);
    Some(storage)
}

/// The by-name load's binding as the row asks for it: the global `undefined` is a constant; any
/// other is the lane's own read (`GlobalFrame::global_read`) of what its record resolved.
impl<const ACC: u8> crate::staged::GlobalFrame<Concrete> for LaneFrame<ACC> {
    #[inline(always)]
    fn global_binding(&mut self, _proof: ()) -> GlobalBinding {
        match self.host.and_then(|host| host.name) {
            Some(NameBinding::Undefined) => GlobalBinding::Constant(Value::UNDEFINED),
            _ => GlobalBinding::Dynamic,
        }
    }

    #[inline(always)]
    fn global_constant(&mut self, _stage: &mut Concrete, destination: Register, value: Value) -> Option<()> {
        self.registers.set(destination, value);
        Some(())
    }

    #[inline(always)]
    fn global_read(&mut self, _stage: &mut Concrete, destination: Register, atom: u32, _site: u16, _ty: Type, _proof: ()) -> Option<()> {
        let host = self.host?;
        let Some(NameBinding::Read(read)) = host.name else { return None };
        // SAFETY: the reader was made for the lane context this frame runs in.
        let value = unsafe { (host.read_name)(host.cx, read, atom) }?;
        self.registers.set(destination, value);
        Some(())
    }

    /// `LoadNameTypeof`'s read of the name: the binding's value, where the binding answers it. An
    /// absent name answers nothing, which the row's miss settles (`typeof` reads it as `undefined`).
    #[inline(always)]
    fn global_typeof(&mut self, _stage: &mut Concrete, destination: Register, atom: u32, _site: u16) -> Option<()> {
        let host = self.host?;
        let value = match host.name? {
            NameBinding::Undefined => Value::UNDEFINED,
            // SAFETY: the reader was made for the lane context this frame runs in.
            NameBinding::Read(read) => unsafe { (host.read_name)(host.cx, read, atom) }?,
        };
        self.registers.set(destination, value);
        Some(())
    }
    #[inline(always)]
    fn global_write(&mut self, _stage: &mut Concrete, _source: Register, atom: u32, _site: u16, value: u64, _initialize: bool) -> Option<()> {
        let GlobalStore { cx, hint, store } = self.global_store?;
        // SAFETY: the store was made for the lane context this frame runs in.
        unsafe { store(cx, hint, atom, Value::from_bits(value)) }.then_some(())
    }
}

/// The store of `value` to the global object's own writable data field `atom` that `hint` names
/// (`GlobalFrame::global_write`): it runs no guest code, and `false` where the hint misses.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn global_name_store<H: Host>(cx: *mut (), hint: crate::vm::FieldCache, atom: u32, value: Value) -> bool {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    vm.hinted_own_field_store_hit(hint, vm.realm.globals, atom, value)
}

impl<const ACC: u8> crate::staged::CallFrame<Concrete> for LaneFrame<ACC> {}

impl<const ACC: u8> crate::staged::PropertyFrame<Concrete> for LaneFrame<ACC> {
    type Plans = crate::staged::NoPlans;

    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }

    #[inline(always)]
    fn delete_property(&mut self, _stage: &mut Concrete, base: u64, key: u64, _strict: bool, _miss: ()) -> Option<(u64, Boxing)> {
        let host = self.host?;
        // SAFETY: the deleter was made for the lane context this frame runs in.
        let deleted = unsafe { (host.delete)(host.cx, Value::from_bits(base), Value::from_bits(key)) }?;
        Some((deleted.to_bits(), Boxing::Value))
    }

    /// The live object record table, which an `instanceof` walk addresses.
    fn object_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        let Decisions { context, records, .. } = self.decisions?;
        records(context)
    }
}

impl<const ACC: u8> crate::staged::ElementFrame<Concrete> for LaneFrame<ACC> {
    /// The array records of the heap an element row reads (`row_frame` binds it).
    #[inline(always)]
    fn array_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        // SAFETY: the heap is the lane's, live for the row.
        self.heap.and_then(|heap| unsafe { (*heap).array_record_base() }).map(|base| base as u64)
    }

    /// The lane keeps no views: a read takes the element of any backing in place.
    #[inline(always)]
    fn reads_any_backing(&self) -> bool { true }

    /// The array a proof of the lane's span gives (`proof_span`), where the row is one a proof names;
    /// `None` where the slot does not prove the array, which proves it on the access's own check.
    #[inline(always)]
    fn proved_view(&mut self, stage: &mut Concrete, base: Register, writable: bool) -> Option<crate::staged::elements::DenseView<u64>> {
        let (context, span) = self.element_span?;
        let object = Value::from_bits(self.read(stage, base));
        // SAFETY: the context and its proofs are the running lane's, live for the row.
        let (values, length) = unsafe { span(context, base, object, writable) }?;
        Some(crate::staged::elements::DenseView { values: values.as_ptr() as u64, length: length as u64, packed: false, within: false })
    }

    /// A store of `value` into the array needs no barrier log where `value` is no heap reference
    /// and no hole (`concrete_append_in_room`'s test): the in-place replacement of the row answers
    /// it; any other value leaves to the VM's store.
    #[inline(always)]
    fn array_store_gate(&mut self, _stage: &mut Concrete, _base: u64, value: u64, _miss: ()) -> Option<()> {
        let value = Value::from_bits(value);
        (value.heap_index().is_none() && !value.is_deleted()).then_some(())
    }
}

impl<const ACC: u8> NumericFrame<Concrete> for LaneFrame<ACC> {
    /// A register's word, or the constant or local the record names.
    #[inline(always)]
    fn operand_word(&mut self, stage: &mut Concrete, operand: Operand) -> Option<u64> {
        match operand.register_index() {
            Some(register) => Some(self.read(stage, register)),
            None => self.reads.word(operand).map(Value::to_bits),
        }
    }

    #[inline(always)]
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        // A register no accumulator stands for: its tag half-word tested in memory
        // (`Registers::int`), not the whole word loaded and masked.
        if ACC == ACC_NONE && let Some(register) = operand.register_index() {
            return self.registers.int(register).map(|int| u64::from(int as u32));
        }
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_int().map(|int| u64::from(int as u32))
    }

    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let word = self.operand_word(stage, operand)?;
        Value::from_bits(word).as_number().map(f64::to_bits)
    }

    /// The constructor's live `prototype` link, where the VM proved its `instanceof` steady: the
    /// interpreter reads it from the objects each time, so it needs no dependency.
    fn stable_prototype(&mut self, stage: &mut Concrete, constructor: Register) -> Option<u64> {
        let Decisions { context, prototype, .. } = self.decisions?;
        prototype(context, Value::from_bits(self.read(stage, constructor))).map(u64::from)
    }

    /// ToInt32 of a Number operand, as the bitwise operators read it.
    fn int32_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> Option<u64> {
        let number = Value::from_bits(self.operand_word(stage, operand)?).as_number()?;
        Some(u64::from(crate::value::number_to_u32(number)))
    }

    #[inline(always)]
    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }

    #[inline(always)]
    fn int_result(&mut self, _stage: &mut Concrete, word: u64) -> u64 {
        Value::integer(word as u32 as i32).to_bits()
    }

    #[inline(always)]
    fn miss(&self) {}
    #[inline(always)]
    fn int_fact(&self) -> Boxing { Boxing::Value }
    #[inline(always)]
    fn number_fact(&self) -> Boxing { Boxing::Double }
}

#[path = "lane_frame/staged_ops.rs"]
mod staged_ops;
pub(super) use staged_ops::*;

/// A lane record read as the staged decoder's fields (`staged::Fields`), its opcode the
/// handler's const generic. The lane admits a register-defining record only without result flags
/// (a returning one takes `returning_handler`), so its `a` is the result register as it is.
#[derive(Clone, Copy)]
struct LaneFields<'a, const OP: u16, const FLAG: u8 = FLAG_UNREAD>(&'a LaneInstruction);

impl<const OP: u16, const FLAG: u8> LaneFields<'_, OP, FLAG> {
    const OPCODE: Op = match Op::from_index(OP as usize) { Some(op) => op, None => panic!("no such opcode") };
    const FLAG_ROLE: bool = matches!(Self::OPCODE.immediate_role(), crate::bytecode::ImmediateRole::BooleanFlag);
    const REGISTER_B: bool = Self::OPCODE.field_layout(crate::bytecode::InstructionField::B).is_register_field();
    const REGISTER_C: bool = Self::OPCODE.field_layout(crate::bytecode::InstructionField::C).is_register_field();
    const RESULT_A: bool = matches!(Self::OPCODE.field_layout(crate::bytecode::InstructionField::A),
        crate::bytecode::FieldLayout::ResultRegister);
    const REGISTER_A: bool = Self::OPCODE.field_layout(crate::bytecode::InstructionField::A).is_register_field();
}

impl<const OP: u16, const FLAG: u8> crate::staged::Fields for LaneFields<'_, OP, FLAG> {
    #[inline(always)]
    fn op(self) -> Op {
        // A constant of the instantiation, so the decode's match folds to the one arm.
        Self::OPCODE
    }
    #[inline(always)] fn result_register(self) -> u16 { self.0.a }
    #[inline(always)] fn register_a(self) -> u16 { self.0.a }
    #[inline(always)] fn register_b(self) -> u16 { self.0.b }
    #[inline(always)]
    fn constant(self, _: crate::staged::DecodeContext<'_>) -> Option<Constant> {
        Some(Constant::untyped(self.0.constant()))
    }
    #[inline(always)]
    fn wide(self) -> WideInstruction {
        // SAFETY: the view admits a record only with its register fields inside the function's
        // registers (`lane_fields_fit`), which `REGISTER_MASK` bounds; stated so a row's operand
        // kind test folds.
        unsafe {
            // A record whose result register carries flags runs the general step instead
            // (`derive_lane_view`), and an input register field carries none.
            if Self::RESULT_A || Self::REGISTER_A { std::hint::assert_unchecked(self.0.a <= crate::bytecode::REGISTER_MASK) }
            if Self::REGISTER_B { std::hint::assert_unchecked(self.0.b <= crate::bytecode::REGISTER_MASK) }
            if Self::REGISTER_C { std::hint::assert_unchecked(self.0.c <= crate::bytecode::REGISTER_MASK) }
        }
        // A flag the compiler writes is 0 or 1; any other word reads as set, so the flag's decode
        // is the one test.
        let imm = match (Self::FLAG_ROLE, FLAG) {
            (true, FLAG_CLEAR) => 0,
            (true, FLAG_SET) => 1,
            (true, _) => self.0.imm().min(1),
            // The view proved the immediate's high half zero (`row_near`): a capture of the running
            // function's own scope, whose depth then folds.
            (false, FLAG_SET) => self.0.imm() & u32::from(u16::MAX),
            (false, _) => self.0.imm(),
        };
        WideInstruction::new(self.op(), self.0.a, self.0.b, self.0.c, imm)
    }
}

/// `lane_row`'s `FLAG`: a row whose immediate is a boolean flag (`ImmediateRole::BooleanFlag`) reads it
/// from the record until its first run, which requickens the record to the instantiation of its flag
/// (`FLAG_CLEAR`, `FLAG_SET`), so the flag is a static operand the row folds (an increment and a
/// decrement are two handlers of one row). For any other row, `FLAG_SET` states the fact the view
/// proved of the record (`row_near`): a capture's immediate has a zero high half (its own scope),
/// an `arguments` count reads its function's kept arguments.
pub(in crate::vm::dispatch_fast) const FLAG_UNREAD: u8 = 0;
pub(in crate::vm::dispatch_fast) const FLAG_CLEAR: u8 = 1;
pub(in crate::vm::dispatch_fast) const FLAG_SET: u8 = 2;
/// `lane_row`'s `FLAG` for a by-name load (`NameLoad`) whose first run resolved its name to a
/// native an internal name stands for (`NameRead::Native`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_NATIVE: u8 = 3;
/// `FLAG` of a by-name load resolved to the global `undefined` (`NameBinding::Undefined`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_UNDEFINED: u8 = 4;
/// `FLAG` of a by-name load resolved to the global object's own data field its record's hint
/// names (`NameRead::Field`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_GLOBAL: u8 = 5;
/// `FLAG` of a by-name load resolved to an intrinsic name (`NameRead::Intrinsic`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_INTRINSIC: u8 = 6;
/// `FLAG` of a by-name load a probe settled onto the global object's own data field its record's
/// hint names, in a scope that may declare the name (`settle_name_record`): as `FLAG_NAME_GLOBAL`,
/// but a refresh checks the scopes' declarations first (`lane_load_global_name_refresh`, `SCOPED`).
pub(in crate::vm::dispatch_fast) const FLAG_NAME_GLOBAL_SCOPED: u8 = 7;
/// `FLAG` of a `StringCaseIndex` record its tier filtered (`string_case_filter::specialize`): the row
/// answers from the filter the record's operand names. Not a flag role, so its immediate is whole.
pub(in crate::vm::dispatch_fast) const FLAG_CASE_FILTER: u8 = 8;
/// `FLAG` of a local-access record of a plain slot (`plain_frame_local`) in an environment window's
/// function (`specialize_environment_window`): its immediate names the window's register base above
/// its slot (`staged::window_base`). In a window activation's run the row reads the window's locals;
/// in a frame's run, the frame's own locals, as an unflagged plain record does.
pub(in crate::vm::dispatch_fast) const FLAG_WINDOW: u8 = 9;

/// Whether opcode `op`'s lane handler is its staged row (`lane_row`): a row that falls through,
/// of the flow family or one the catalog marks `ROW_HANDLER`. Derived from the catalog, so a row
/// that joins gets its handler with no line written here.
/// Whether opcode `op`'s row defines the carried accumulator: its record's `a` is its result
/// register, and its miss takes the general step, which carries the result on
/// (`Carried::stepped`). A capture read's miss walks the scopes and requickens its record to
/// handlers that carry none, and a local row defines none.
pub(in crate::vm::dispatch_fast) const fn row_produces(op: Op) -> bool {
    matches!(op.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister)
        && !matches!(op.staged(), Some(crate::bytecode::Decoder::Local))
        && matches!(crate::staged::miss(op), crate::staged::Miss::General)
}

pub(in crate::vm::dispatch_fast) const fn lane_runs_row(op: Op) -> bool {
    matches!(op.control_flow_layout(), crate::bytecode::ControlFlowLayout::Fallthrough)
        && (matches!(op.staged(), Some(crate::bytecode::Decoder::Flow))
            || op.traits().has(crate::bytecode::Traits::ROW_HANDLER))
}

/// The frame `lane_row` steps opcode `OP`'s row over: the register file and the carried view, and
/// the running frame's local slots where the row is a local one (`Decoder::Local`).
#[inline(always)]
fn row_frame<H: Host, const OP: u16, const ACC: u8, const FLAG: u8>(cx: *mut LaneContext<H>, r: Registers, i: &LaneInstruction, view: Carried) -> LaneFrame<ACC> {
    let locals = const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Local)) };
    if const { crate::staged::name_load(LaneFields::<OP>::OPCODE).is_some() } {
        // A by-name load reads the binding its record's first run resolved (`FLAG`, `name_row`);
        // an unresolved record holds none, so its row misses to the resolve (`name_load_miss`).
        let binding = match FLAG {
            FLAG_NAME_NATIVE => Some(NameBinding::Read(NameRead::Native)),
            FLAG_NAME_UNDEFINED => Some(NameBinding::Undefined),
            FLAG_NAME_GLOBAL | FLAG_NAME_GLOBAL_SCOPED => Some(NameBinding::Read(NameRead::Field(i.field_hint()))),
            FLAG_NAME_INTRINSIC => Some(NameBinding::Read(NameRead::Intrinsic)),
            _ => None,
        };
        let frame = LaneFrame::<ACC>::hosted::<H>(r, view, cx);
        return match binding {
            Some(binding) => frame.named(binding),
            None => frame,
        };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
        let heap: *const crate::heap::Heap = &unsafe { vm(cx) }.heap;
        // A count the view proved to read its function's kept arguments (`FLAG_SET`) takes a
        // keeping activation's count without finding the function.
        let count: unsafe fn(*mut (), u16) -> Option<Value> = if FLAG == FLAG_SET {
            |context, slot| unsafe { super::arguments_count::<H, true>(context.cast(), slot) }
        } else {
            |context, slot| unsafe { super::arguments_count::<H, false>(context.cast(), slot) }
        };
        let environment = EnvironmentReads { context: cx.cast(), read: environment_read::<H>, write: environment_write_storage::<H> };
        // A global-name store writes through the record's own hint (`global_name_store`).
        let global_store = if const { OP == crate::staged::GLOBAL_WRITE_ROW } {
            Some(GlobalStore { cx: cx.cast(), hint: i.field_hint(), store: global_name_store::<H> })
        } else {
            None
        };
        let names: NamesStep = fast_names_step::<H>;
        // A clone names the window its record carries above its plan (`specialize_environment_window`).
        let clones = if const { OP == crate::staged::CLONE_ENV_ROW } {
            Some(EnvironmentClones { context: cx.cast(), window: (i.imm() >> super::ENVIRONMENT_WINDOW_SHIFT) as u16,
                run: super::clone_environment_of::<H> })
        } else {
            None
        };
        // The iterator cleanup stack of the running lane (`iterator_push_in_lane`, `iterator_pop_in_lane`).
        let cleanups = IteratorCleanups { context: cx.cast(), push: iterator_push_in_lane::<H>, pop: iterator_pop_in_lane::<H> };
        return LaneFrame::<ACC> { heap: Some(heap), arguments: Some((cx.cast(), count)), environment: Some(environment), global_store, names: Some((cx.cast(), names)),
            clones, cleanups: Some(cleanups), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Lane)) } {
        // A string-case index reads its operand from the register file (`LaneFrame::read` stands for field B),
        // and answers from the case table of the record's filter (`FLAG_CASE_FILTER`), else the VM's selector.
        let string_case = const { OP == crate::staged::STRING_CASE_INDEX_ROW };
        let string_cases = if string_case {
            let filter = if FLAG == FLAG_CASE_FILTER { unsafe { i.operand.string_case_filter } } else { std::ptr::null() };
            Some(StringCases { context: cx.cast(), filter, index: super::string_case_filter::case_index_in_lane::<H> })
        } else {
            None
        };
        let heap: Option<*const crate::heap::Heap> = if string_case {
            let heap: *const crate::heap::Heap = &unsafe { vm(cx) }.heap;
            Some(heap)
        } else {
            None
        };
        // An `await` suspends the promise job the run is rooted in, from the record it runs (`await_in_lane`).
        let suspension = if const { OP == crate::staged::AWAIT_ROW } {
            Some(Suspension { context: cx.cast(), record: i, suspend: super::generator_yields::await_in_lane::<H> })
        } else {
            None
        };
        // A `yield` or `yield*` transfers to the running generator's window (`generator_yields::yield_in_lane`,
        // `yield_star_in_lane`), which declines where no generator runs the lane.
        let yields = if const { OP == crate::staged::YIELD_ROW } {
            Some(YieldSuspension { context: cx.cast(), record: i, resume: super::generator_yields::yield_in_lane::<H> })
        } else if const { OP == crate::staged::YIELD_STAR_ROW } {
            Some(YieldSuspension { context: cx.cast(), record: i, resume: super::generator_yields::yield_star_in_lane::<H> })
        } else {
            None
        };
        return LaneFrame::<ACC> { heap, string_cases, suspension, yields, ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
        // A JavaScript view carries its context (`script_handler!`), so no thread-local load.
        let context = view.memory.bytes;
        let heap: *const crate::heap::Heap = &unsafe { vm(context.cast::<LaneContext<H>>()) }.heap;
        // A row a proof names (`FLAG_SET`) reads and stores its array through the proof's span.
        let element_span = if const { FLAG == FLAG_SET } { Some((context, proof_span::<H> as ElementSpan)) } else { None };
        return LaneFrame::<ACC> { heap: Some(heap), element_span, ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Value)) } {
        // A value row may read `this`: the context word, else the frame's (`lane_this`), read lazily.
        let read: unsafe fn(*mut ()) -> Option<Value> = |context| {
            let cx = context.cast::<LaneContext<H>>();
            unsafe { super::lane_this(cx, (*cx).registers) }
        };
        // A capture row reads the running function's dense slots (its miss walks: `row_miss`) and
        // stores through the lane's own store.
        let (capture_slots, captures) = (Some(capture_reads(r.capture())), Some(CaptureStores::own(r)));
        // A `LoadCapture` past the running function's own scope reads through the lane's remembered
        // walks (`Reads::of_captures`) before its walk miss.
        let reads = if const { OP == crate::staged::LOAD_CAPTURE_ROW } { Reads::of_captures::<H>(cx) } else { Reads::NONE };
        // A `LoadCallee` row reads the running function (`running_callee`).
        let callee: Option<(*mut (), unsafe fn(*mut ()) -> Option<Value>)> = if const { OP == crate::staged::LOAD_CALLEE_ROW } {
            let read: unsafe fn(*mut ()) -> Option<Value> = super::running_callee::<H>;
            Some((cx.cast(), read))
        } else {
            None
        };
        return LaneFrame::<ACC> { this: r.this(), frame_this: Some((cx.cast(), read)), capture_slots, captures, callee, reads,
            ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
        // A generic row's word-decided answers, and the pure binary of its call-outs (`lane_binary_values`).
        let binary: unsafe fn(*mut (), u32, Value, Value) -> Option<Value> =
            |context, operator, left, right| unsafe { super::lane_binary_values::<H>(context.cast(), operator, left, right) };
        // Its operands may name a constant, a plain local or a capture, and a comparison pair its words
        // leave is the VM's steady decision (`staged::step` over the comparison).
        return LaneFrame::<ACC> { binary: Some((cx.cast(), binary)), decisions: Some(Decisions::of(cx)), reads: Reads::of_jump::<H>(cx, None),
            ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Literal)) } {
        // An allocation or an element definition makes its value in the lane context, as a staged
        // allocation does (`LaneHost::allocate`, `LaneHost::define`).
        return LaneFrame::<ACC>::hosted::<H>(r, view, cx);
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmInt)) } {
        // An integer row reads its operands from the register file: the carried accumulator stands for
        // one field only, and a row reads two registers (`LaneFrame::read`).
        let heap: *const crate::heap::Heap = &unsafe { vm(cx) }.heap;
        return LaneFrame::<ACC> { heap: Some(heap), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmMemory)) } {
        // A direct access addresses the memory its view resolved, as the hand handler did.
        // SAFETY: the view resolved this memory in the current lane run (`MemoryView::extent`).
        let memory = unsafe { view.memory.extent() };
        // A generic access names its memory by register, which `wasm_memory_of` resolves.
        let memories = WasmMemories { context: cx.cast(), resolve: wasm_memory_of::<H> };
        return LaneFrame::<ACC> { memory: Some(memory), memories: Some(memories), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmState)) } {
        // An instance-state row reads and writes the VM's globals through the lane context.
        let globals = WasmGlobals { context: cx.cast(), load: super::wasm_instance::wasm_global_load::<H>,
            store: super::wasm_instance::wasm_global_store::<H> };
        return LaneFrame::<ACC> { globals: Some(globals), ..LaneFrame::<ACC>::new(r, view) };
    }
    if !locals {
        return LaneFrame::<ACC>::new(r, view);
    }
    // A local access holds the frame's locals only in an activation without its own environment, and
    // not a root body's frame. A window record (`FLAG_WINDOW`) in an activation that holds one reads the
    // window's locals instead. Any other leaves to its miss (`row_miss`).
    let environment_free = unsafe { (*cx).registers }.env().is_deleted();
    if FLAG == FLAG_WINDOW && !environment_free {
        return LaneFrame::<ACC> { window: Some(r.0.wrapping_add(usize::from(crate::staged::window_base(i.imm())))),
            ..LaneFrame::<ACC>::new(r, view) };
    }
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame() }];
    if !environment_free || frame.function == super::ROOT_FUNCTION_ID {
        return LaneFrame::<ACC>::new(r, view);
    }
    let (slots, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, slots, len)
}

/// The hand handler of checked local access `op` (`row_miss`), which the lane takes for a record
/// whose slot its frame's locals do not hold (an environment's, a mapped `arguments`' or a self
/// binding's), as the row would only leave there.
pub(in crate::vm::dispatch_fast) fn checked_local_hand<H: Host>(op: Op) -> Handler {
    row_miss::<H, FLAG_UNREAD>(op)
}

/// Where opcode `op`'s row leaves when its words do not decide it: the VM's general step, or for
/// a capture access the lane's own slow arm, which walks the scopes and requickens the record to
/// the root binding it finds (`lane_load_capture_walk`, `store_outer_capture_miss`), or for a
/// `ToPropertyKey` the lane's integer and symbol answers (`lane_to_property_key`).
pub(in crate::vm::dispatch_fast) const fn row_miss<H: Host, const FLAG: u8>(op: Op) -> Handler {
    match crate::staged::miss(op) {
        crate::staged::Miss::WalkScopes => super::lane_load_capture_walk::<H>,
        crate::staged::Miss::StoreThroughScopes => super::store_outer_capture_miss::<H>,
        crate::staged::Miss::ArgumentsBinding => super::lane_frame_arguments::<H, false>,
        crate::staged::Miss::EnvironmentLoad => super::load_local_miss::<H>,
        crate::staged::Miss::EnvironmentStore => super::store_local_miss::<H>,
        crate::staged::Miss::StoreNameFirst => super::store_name_first_miss::<H>,
        crate::staged::Miss::PropertyKeyValue => super::lane_to_property_key::<H>,
        crate::staged::Miss::Binary => super::lane_binary_general::<H>,
        crate::staged::Miss::WasmMemoryLoad => super::lane_wasm_memory_load::<H>,
        crate::staged::Miss::WasmMemoryStore => super::lane_wasm_memory_store::<H>,
        crate::staged::Miss::WasmMemoryAddress => super::lane_wasm_memory_address::<H>,
        crate::staged::Miss::LocalLoad => super::load_local_miss::<H>,
        crate::staged::Miss::LocalStore => super::store_local_miss::<H>,
        crate::staged::Miss::LocalInitialize => super::lane_initialize_tdz::<H>,
        crate::staged::Miss::Unary => super::denoted::lane_unary::<H>,
        crate::staged::Miss::Concat => super::denoted::string_concat_miss::<H>,
        crate::staged::Miss::Yield => super::generator_yields::yield_miss::<H>,
        crate::staged::Miss::YieldStar => super::generator_yields::yield_star_miss::<H>,
        crate::staged::Miss::NameLoad(load) => name_load_miss::<H, FLAG>(load),
        crate::staged::Miss::General | crate::staged::Miss::CellTruth { .. }
            | crate::staged::Miss::CompareChain => lane_general_step::<H>,
    }
}

/// The row handler of by-name load `load` for a record in binding state `FLAG` (`lane_row`'s
/// instantiation over the load's opcode, with an accumulator-free record, since a load answers
/// no accumulator).
pub(in crate::vm::dispatch_fast) const fn name_row<H: Host, const FLAG: u8>(load: crate::staged::NameLoad) -> Handler {
    match load {
        crate::staged::NameLoad::Read => lane_row::<H, { crate::staged::LOAD_NAME_ROW }, ACC_NONE, FLAG> as Handler,
        crate::staged::NameLoad::Call => lane_row::<H, { crate::staged::LOAD_NAME_CALL_ROW }, ACC_NONE, FLAG> as Handler,
        crate::staged::NameLoad::Typeof => lane_row::<H, { crate::staged::LOAD_NAME_TYPEOF_ROW }, ACC_NONE, FLAG> as Handler,
    }
}

/// Where a by-name load's row leaves when its binding does not answer it: the first run (and a
/// native's, which is resolved again) resolves the name (`lane_load_name_resolve`); a global's
/// read takes its refresh (`lane_load_global_name_refresh`); an undefined or intrinsic read takes
/// the general path, which the lane exits to.
const fn name_load_miss<H: Host, const FLAG: u8>(load: crate::staged::NameLoad) -> Handler {
    match load {
        crate::staged::NameLoad::Read => name_miss_kind::<H, FLAG, { super::LOAD_NAME }>(),
        crate::staged::NameLoad::Call => name_miss_kind::<H, FLAG, { super::LOAD_NAME_CALL }>(),
        crate::staged::NameLoad::Typeof => name_miss_kind::<H, FLAG, { super::LOAD_NAME_TYPEOF }>(),
    }
}

/// `name_load_miss` for the by-name load of kind `KIND`.
const fn name_miss_kind<H: Host, const FLAG: u8, const KIND: u8>() -> Handler {
    match FLAG {
        FLAG_NAME_GLOBAL => super::lane_load_global_name_refresh::<H, KIND, false> as Handler,
        FLAG_NAME_GLOBAL_SCOPED => super::lane_load_global_name_refresh::<H, KIND, true> as Handler,
        FLAG_NAME_UNDEFINED | FLAG_NAME_INTRINSIC => super::lane_exit::<H> as Handler,
        _ => super::lane_load_name_resolve::<H, KIND> as Handler,
    }
}

/// Opcode `OP`'s row handler for a record of which the view proved its row's fact (`FLAG_SET`),
/// where `handler` is the plain row (`FLAG_UNREAD`, no accumulator field): a capture access at
/// depth 0 then reads its slot without testing the depth.
pub(in crate::vm::dispatch_fast) fn row_near<H: Host, const OP: u16>(handler: *const ()) -> Option<Handler> {
    (handler == lane_row::<H, OP, ACC_NONE, FLAG_UNREAD> as Handler as *const ())
        .then_some(lane_row::<H, OP, ACC_NONE, FLAG_SET> as Handler)
}

/// Whether `handler` is opcode `OP`'s generated row handler (`lane_row`), at any accumulator field:
/// the passes that fuse a record by its handler find the row whichever field the view carried.
pub(in crate::vm::dispatch_fast) fn is_row<H: Host, const OP: u16>(handler: *const ()) -> bool {
    is_row_flagged::<H, OP, FLAG_UNREAD>(handler) || is_row_flagged::<H, OP, FLAG_CLEAR>(handler)
        || is_row_flagged::<H, OP, FLAG_SET>(handler)
}

/// `is_row` at one `FLAG`.
fn is_row_flagged<H: Host, const OP: u16, const FLAG: u8>(handler: *const ()) -> bool {
    let rows: [Handler; ACC_POSITIONS] = [lane_row::<H, OP, ACC_NONE, FLAG>, lane_row::<H, OP, ACC_A, FLAG>,
        lane_row::<H, OP, ACC_B, FLAG>, lane_row::<H, OP, ACC_C, FLAG>, lane_row::<H, OP, ACC_PAIR, FLAG>];
    rows.iter().any(|row| *row as *const () == handler)
}

/// `staged::step` of opcode `$op`'s row, compiled as only the part the row reaches
/// (`staged::row_step`): each family is an `if const` branch, which rustc compiles only when taken.
macro_rules! row_step {
    ($op:expr, $stage:expr, $frame:expr, $operation:expr) => {{
        use crate::staged::RowStep as R;
        if const { matches!(crate::staged::row_step($op), R::Flow) } { crate::staged::step_flow($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Local) } { crate::staged::step_local($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Number) } { crate::staged::step_number($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Value) } { crate::staged::step_value($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Names) } { crate::staged::step_names($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Property) } { crate::staged::step_property($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Keyed) } { crate::staged::step_keyed($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::Generic) } { crate::staged::step_generic($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmInt) } { crate::staged::step_wasm_int($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmFloat) } { crate::staged::step_wasm_float($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmJump) } { crate::staged::step_wasm_jump($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmState) } { crate::staged::step_wasm_state($stage, $frame, $operation) }
        else if const { matches!(crate::staged::row_step($op), R::WasmMemory) } { crate::staged::step_wasm_access($stage, $frame, $operation) }
        else { crate::staged::step($stage, $frame, $operation) }
    }};
}

/// Where opcode `op`'s jump row leaves when its words do not decide the edge: a truth test of a
/// heap value asks its cell (`jump_test_cell_miss`); a comparison the frame leaves takes the
/// lane's comparison chain (`lane_jump_binary_false_chain`); any other takes the VM's general step.
const fn jump_row_miss<H: Host, const BACKWARD: bool>(op: Op) -> Handler {
    match crate::staged::miss(op) {
        crate::staged::Miss::CellTruth { taken: false } => super::jump_test_cell_miss::<H, BACKWARD, false>,
        crate::staged::Miss::CellTruth { taken: true } => super::jump_test_cell_miss::<H, BACKWARD, true>,
        crate::staged::Miss::CompareChain => super::lane_jump_binary_false_chain::<H, BACKWARD>,
        _ => lane_general_step::<H>,
    }
}

/// Whether opcode `op` runs its generated jump row (`row_jump`): a jump the flow decoder
/// stages (`Decoder::Flow`), a loop's exit (`LANE_LOOP_EXIT`), whose names row makes its step and
/// its edge, a jump whose row answers its edge from a comparison (`staged::compare_jump`), or a
/// Wasm branch table, whose edge is the entry its index picks (`Edge::Entry`).
pub(in crate::vm::dispatch_fast) const fn jumps_by_row(op: Op) -> bool {
    crate::staged::wasm_branch_table(op)
        || (op.control_flow_layout().jumps()
            && (matches!(op.staged(), Some(crate::bytecode::Decoder::Flow)) || op.traits().has(crate::bytecode::Traits::LANE_LOOP_EXIT)
                || crate::staged::compare_jump(op) || crate::staged::wasm_i32_jump(op)))
}

/// Opcode `OP`'s jump row in direction `backward`.
pub(in crate::vm::dispatch_fast) const fn jump_row<H: Host, const OP: u16>(backward: bool) -> Handler {
    if backward { row_jump::<H, OP, true> } else { row_jump::<H, OP, false> }
}

/// Opcode `$op`'s staged operation decoded from `$fields` by its row family's one decoder. Each
/// decoder is an `if const` branch: rustc compiles only the taken one (it compiles every arm of a
/// `match` on a constant).
macro_rules! row_decode {
    ($op:ident, $flag:expr, $fields:ident) => {
        if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Flow)) } {
            Operation::decode_flow($fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Local)) } {
            Operation::decode_local(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Value)) } {
            Operation::decode_value(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
            Operation::decode_generic(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmInt)) } {
            Operation::decode_wasm_int(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
            Operation::decode_names(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
            Operation::decode_element(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmFloat)) } {
            Operation::decode_wasm_float(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmState)) } {
            Operation::decode_wasm_state($fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmMemory)) } {
            Operation::decode_wasm_access($fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Literal)) } {
            Operation::decode_literal(crate::staged::Fields::wide($fields), ())
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Lane)) } {
            Operation::decode_lane(crate::staged::Fields::wide($fields))
        } else if const { matches!(LaneFields::<$op, $flag>::OPCODE.staged(), Some(crate::bytecode::Decoder::Proved)) } {
            // A by-name load's proof of its global is the lane's own binding (`row_frame`), so the
            // proof is the unit.
            Operation::decode_load_name(crate::staged::Fields::wide($fields), ())
        } else {
            None
        }
    };
}

lane_handler! {
    /// The lane handler of every jump `jumps_by_row` admits, generated from its staged row as
    /// `lane_row` is: the record's decoder (flow, or generic for a comparison) reads it, the row
    /// steps on `Concrete` over the lane's frame, and its edge picks the continuation. A jump to
    /// its target runs the lane's jump (`lane_jump`), whose backward edge (`BACKWARD`, which the
    /// view fixes per record) is a safepoint.
    pub(super) fn row_jump<OP: u16, BACKWARD: bool>(cx, r, ip, i, view) {
        let fields = LaneFields::<OP>(i);
        // A loop's exit decodes by its names row (`ForInNext`), any other jump by the flow decoder.
        let decoded = if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
            Operation::decode_names(crate::staged::Fields::wide(fields))
        } else if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
            Operation::decode_generic(crate::staged::Fields::wide(fields))
        } else if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmJump)) } {
            Operation::decode_wasm_jump(crate::staged::Fields::wide(fields))
        } else {
            Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY)
        };
        let Some(operation) = decoded else {
            sibling!(lane_general_step::<H>, r, ip, view)
        };
        let mut stage = Concrete::new(std::ptr::null());
        // A loop's exit writes its result register, so no producer's accumulator carries past it.
        let carried = if const { LaneFields::<OP>::RESULT_A } { Carried::script_in(cx, view) } else { view };
        let mut frame = row_frame::<H, OP, ACC_NONE, FLAG_UNREAD>(cx, r, i, view);
        match row_step!(LaneFields::<OP>::OPCODE, &mut stage, &mut frame, operation) {
            Step::Control(Edge::Target) => resume!(cx, super::lane_jump::<H>(cx, r, ip, BACKWARD, carried, None)),
            // A table's entry is the jump record `k` places after the table record (validation places
            // `entries` of them after it), and the lane jumps from that record.
            Step::Control(Edge::Entry(k)) => resume!(cx, super::lane_jump::<H>(cx, r, unsafe { ip.add(1 + k as usize) }, BACKWARD, carried, None)),
            Step::Control(Edge::Leave) => unreachable!("a jump leaves no lane run"),
            Step::Control(Edge::Transfer) => unreachable!("a jump transfers no control"),
            Step::Control(Edge::Following) | Step::Value(_) => next!(cx, r, following(ip), carried),
            Step::Unresolved => sibling!(const { jump_row_miss::<H, BACKWARD>(LaneFields::<OP>::OPCODE) }, r, ip, view),
        }
    }
}

lane_handler! {
    /// The lane handler of every opcode `lane_runs_row` admits, generated from its staged row
    /// rather than written: the record decodes by the row family's one decoder
    /// (`Operation::decode_flow`), the row steps on `Concrete` over the lane's frame, and the
    /// step picks the continuation. `OP` and `ACC` are static, so rustc folds the decode and the
    /// row to the residual a hand-written handler would be (Futamura's first projection).
    pub(super) fn lane_row<OP: u16, ACC: u8, FLAG: u8>(cx, r, ip, i, view) {
        if LaneFields::<OP, FLAG>::FLAG_ROLE && FLAG == FLAG_UNREAD {
            let flagged: Handler = if i.imm() != 0 { lane_row::<H, OP, ACC, FLAG_SET> } else { lane_row::<H, OP, ACC, FLAG_CLEAR> };
            requicken::<H>(ip, flagged);
            sibling!(flagged, r, ip, view)
        }
        // The row family's decoder, a constant of the instantiation, so only its arm is compiled.
        let fields = LaneFields::<OP, FLAG>(i);
        if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
            // A keyed read's row is installed for a register base and key without result flags
            // (`element_read_row`): stated so the decode's operand tests fold.
            unsafe {
                std::hint::assert_unchecked(i.a <= crate::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.b <= crate::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.c <= crate::bytecode::REGISTER_MASK);
            }
        }
        // `if const`, not `match const`: rustc compiles only a taken `if const` branch, so each
        // instantiation holds its one decoder (a `match` on a constant compiles every arm).
        let decoded = row_decode!(OP, FLAG, fields);
        let Some(operation) = decoded else { sibling!(lane_general_step::<H>, r, ip, view) };
        // A row that reads records in place (`Decoder::Names`) may read a slot of the property arena.
        let property_values = if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
            unsafe { vm(cx) }.heap.arena_values_base()
        } else { std::ptr::null() };
        let mut stage = Concrete::new(property_values);
        let mut frame = row_frame::<H, OP, ACC, FLAG>(cx, r, i, view);
        match row_step!(LaneFields::<OP, FLAG>::OPCODE, &mut stage, &mut frame, operation) {
            Step::Value(value) => {
                // A row that answers its value without writing its register (`row_writes_result`):
                // the register is written here, as every producer writes it beside the carried value.
                if const { !crate::staged::row_writes_result(LaneFields::<OP, FLAG>::OPCODE) } {
                    r.set(i.a, Value::from_bits(value));
                }
                if const { row_produces(LaneFields::<OP, FLAG>::OPCODE) } {
                    next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
                }
                // No record reads a non-producer's accumulator (`derive_lane_view`).
                next!(cx, r, following(ip), view)
            }
            // A row that suspended its activation (`Frame::suspend_await`) ends the run, as its hand handler did.
            Step::Control(Edge::Leave) => return exit(ip),
            // A keyed read past its in-place arm: the record's keyed paths (a slot, a getter).
            Step::Unresolved if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element))
                && matches!(LaneFields::<OP, FLAG>::OPCODE.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister) } => {
                let script: *mut LaneContext<H> = view.memory.bytes.cast();
                sibling!(super::lane_get_index_slot::<H>, r, ip, Carried::script_in(script, view))
            }
            // A transfer (`Frame::suspend_yield`) resumes the register file the window switched to.
            Step::Control(Edge::Transfer) => resume!(cx, frame.take_transfer()),
            Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H, FLAG>(LaneFields::<OP, FLAG>::OPCODE) }, r, ip, view),
        }
    }
}

lane_handler! {
    /// Two adjacent rows as one record, generated from their staged rows (`staged::FUSED_ROWS`):
    /// row `A` steps over this record, then row `B` over the next one, with no dispatch between
    /// them. Neither row is written for the pair: each is its own row's decode and step, so the
    /// pair is the composition the specializer would make (Futamura). Only this record changes;
    /// the next keeps its own row for jumps into it. A miss of `A` takes `A`'s miss here; a miss
    /// of `B` takes `B`'s at the next record, after `A`'s effects, as two records would.
    /// `BACKWARD` is `B`'s jump direction where `B` is a jump (`jumps_by_row`). A flagged row
    /// reads its flag from its record at run time. `MODE` is the pair's `staged::FUSED_*`:
    /// `FUSED_DEAD_FIRST` skips `A`'s register write (installed only where liveness proves
    /// `A`'s register dead and `B` does not name it); `FUSED_RAW` runs as `FUSED_PLAIN` here.
    pub(super) fn fused_rows<A: u16, B: u16, BACKWARD: bool, MODE: u8>(cx, r, ip, i, view) {
        // Row `A` at flag `$flag`: a flagged row (`LaneFields::FLAG_ROLE`) reads its flag here,
        // from the record, rather than requickening (`lane_row`).
        macro_rules! step_a { ($flag:expr) => {{
            let fields = LaneFields::<A, { $flag }>(i);
            let Some(operation) = row_decode!(A, { $flag }, fields) else { sibling!(lane_general_step::<H>, r, ip, view) };
            let property_values = if const { matches!(LaneFields::<A>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
                unsafe { vm(cx) }.heap.arena_values_base()
            } else { std::ptr::null() };
            let mut stage = Concrete::new(property_values);
            let mut frame = row_frame::<H, A, ACC_NONE, { $flag }>(cx, r, i, view);
            match row_step!(LaneFields::<A>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !crate::staged::row_writes_result(LaneFields::<A>::OPCODE) && MODE != crate::staged::FUSED_DEAD_FIRST } {
                        r.set(i.a, Value::from_bits(value));
                    }
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H, { $flag }>(LaneFields::<A>::OPCODE) }, r, ip, view),
            }
        }}; }
        if LaneFields::<A>::FLAG_ROLE {
            if i.imm() != 0 { step_a!(FLAG_SET) } else { step_a!(FLAG_CLEAR) }
        } else {
            step_a!(FLAG_UNREAD)
        }
        // `B` over the next record; it reads registers (no accumulator), which `A` wrote.
        let ip = following(ip);
        // SAFETY: the next record of the running view.
        let i = unsafe { &*ip };
        let fields = LaneFields::<B>(i);
        if const { jumps_by_row(LaneFields::<B>::OPCODE) } {
            let decoded = if const { matches!(LaneFields::<B>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
                Operation::decode_names(crate::staged::Fields::wide(fields))
            } else if const { matches!(LaneFields::<B>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
                Operation::decode_generic(crate::staged::Fields::wide(fields))
            } else if const { matches!(LaneFields::<B>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmJump)) } {
                Operation::decode_wasm_jump(crate::staged::Fields::wide(fields))
            } else {
                Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY)
            };
            let Some(operation) = decoded else { sibling!(lane_general_step::<H>, r, ip, view) };
            let mut stage = Concrete::new(std::ptr::null());
            let carried = if const { LaneFields::<B>::RESULT_A } { Carried::script_in(cx, view) } else { view };
            let mut frame = row_frame::<H, B, ACC_NONE, FLAG_UNREAD>(cx, r, i, view);
            match row_step!(LaneFields::<B>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Control(Edge::Target) => resume!(cx, super::lane_jump::<H>(cx, r, ip, BACKWARD, carried, None)),
                Step::Control(Edge::Entry(k)) => resume!(cx, super::lane_jump::<H>(cx, r, unsafe { ip.add(1 + k as usize) }, BACKWARD, carried, None)),
                Step::Control(Edge::Leave) => unreachable!("a jump leaves no lane run"),
                Step::Control(Edge::Transfer) => unreachable!("a jump transfers no control"),
                Step::Control(Edge::Following) | Step::Value(_) => next!(cx, r, following(ip), carried),
                Step::Unresolved => sibling!(const { jump_row_miss::<H, BACKWARD>(LaneFields::<B>::OPCODE) }, r, ip, view),
            }
        }
        macro_rules! step_b { ($flag:expr) => {{
            let fields = LaneFields::<B, { $flag }>(i);
            let Some(operation) = row_decode!(B, { $flag }, fields) else { sibling!(lane_general_step::<H>, r, ip, view) };
            let property_values = if const { matches!(LaneFields::<B>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
                unsafe { vm(cx) }.heap.arena_values_base()
            } else { std::ptr::null() };
            let mut stage = Concrete::new(property_values);
            let mut frame = row_frame::<H, B, ACC_NONE, { $flag }>(cx, r, i, view);
            match row_step!(LaneFields::<B>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !crate::staged::row_writes_result(LaneFields::<B>::OPCODE) } {
                        r.set(i.a, Value::from_bits(value));
                    }
                    if const { row_produces(LaneFields::<B>::OPCODE) } {
                        next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
                    }
                    next!(cx, r, following(ip), view)
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H, { $flag }>(LaneFields::<B>::OPCODE) }, r, ip, view),
            }
        }}; }
        if LaneFields::<B>::FLAG_ROLE {
            if i.imm() != 0 { step_b!(FLAG_SET) } else { step_b!(FLAG_CLEAR) }
        } else {
            step_b!(FLAG_UNREAD)
        }
    }
}

lane_handler! {
    /// Three or four adjacent rows as one record, generated from their staged rows
    /// (`staged::FUSED_CHAINS`): each link is its row's own decode and step over its record,
    /// with no dispatch between links, as `fused_rows` does for two. A link's miss takes its
    /// row's miss at its own record, after the earlier links' effects, as separate records
    /// would. Only the last link may be a jump (`BACKWARD` its direction). `L3` is
    /// `staged::NO_LINK` for a three-link chain.
    pub(super) fn chain_rows<L0: u16, L1: u16, L2: u16, L3: u16, BACKWARD: bool>(cx, r, ip, i, view) {
        macro_rules! link_flag { ($L:ident, $flag:expr) => {{
            let fields = LaneFields::<$L, { $flag }>(i);
            let Some(operation) = row_decode!($L, { $flag }, fields) else { sibling!(lane_general_step::<H>, r, ip, view) };
            let property_values = if const { matches!(LaneFields::<$L>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
                unsafe { vm(cx) }.heap.arena_values_base()
            } else { std::ptr::null() };
            let mut stage = Concrete::new(property_values);
            let mut frame = row_frame::<H, $L, ACC_NONE, { $flag }>(cx, r, i, view);
            match row_step!(LaneFields::<$L>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !crate::staged::row_writes_result(LaneFields::<$L>::OPCODE) } {
                        r.set(i.a, Value::from_bits(value));
                    }
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H, { $flag }>(LaneFields::<$L>::OPCODE) }, r, ip, view),
            }
        }}; }
        macro_rules! value_link { ($L:ident) => {
            if LaneFields::<$L>::FLAG_ROLE {
                if i.imm() != 0 { link_flag!($L, FLAG_SET) } else { link_flag!($L, FLAG_CLEAR) }
            } else {
                link_flag!($L, FLAG_UNREAD)
            }
        }; }
        macro_rules! last_flag { ($L:ident, $flag:expr) => {{
            let fields = LaneFields::<$L, { $flag }>(i);
            let Some(operation) = row_decode!($L, { $flag }, fields) else { sibling!(lane_general_step::<H>, r, ip, view) };
            let property_values = if const { matches!(LaneFields::<$L>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
                unsafe { vm(cx) }.heap.arena_values_base()
            } else { std::ptr::null() };
            let mut stage = Concrete::new(property_values);
            let mut frame = row_frame::<H, $L, ACC_NONE, { $flag }>(cx, r, i, view);
            match row_step!(LaneFields::<$L>::OPCODE, &mut stage, &mut frame, operation) {
                Step::Value(value) => {
                    if const { !crate::staged::row_writes_result(LaneFields::<$L>::OPCODE) } {
                        r.set(i.a, Value::from_bits(value));
                    }
                    if const { row_produces(LaneFields::<$L>::OPCODE) } {
                        next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
                    }
                    next!(cx, r, following(ip), view)
                }
                Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H, { $flag }>(LaneFields::<$L>::OPCODE) }, r, ip, view),
            }
        }}; }
        macro_rules! last_link { ($L:ident) => {{
            let i = unsafe { &*ip };
        let fields = LaneFields::<$L>(i);
            if const { jumps_by_row(LaneFields::<$L>::OPCODE) } {
                let decoded = if const { matches!(LaneFields::<$L>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
                    Operation::decode_names(crate::staged::Fields::wide(fields))
                } else if const { matches!(LaneFields::<$L>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
                    Operation::decode_generic(crate::staged::Fields::wide(fields))
                } else if const { matches!(LaneFields::<$L>::OPCODE.staged(), Some(crate::bytecode::Decoder::WasmJump)) } {
                    Operation::decode_wasm_jump(crate::staged::Fields::wide(fields))
                } else {
                    Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY)
                };
                let Some(operation) = decoded else { sibling!(lane_general_step::<H>, r, ip, view) };
                let mut stage = Concrete::new(std::ptr::null());
                let carried = if const { LaneFields::<$L>::RESULT_A } { Carried::script_in(cx, view) } else { view };
                let mut frame = row_frame::<H, $L, ACC_NONE, FLAG_UNREAD>(cx, r, i, view);
                match row_step!(LaneFields::<$L>::OPCODE, &mut stage, &mut frame, operation) {
                    Step::Control(Edge::Target) => resume!(cx, super::lane_jump::<H>(cx, r, ip, BACKWARD, carried, None)),
                    Step::Control(Edge::Entry(k)) => resume!(cx, super::lane_jump::<H>(cx, r, unsafe { ip.add(1 + k as usize) }, BACKWARD, carried, None)),
                    Step::Control(Edge::Leave) => unreachable!("a jump leaves no lane run"),
                    Step::Control(Edge::Transfer) => unreachable!("a jump transfers no control"),
                    Step::Control(Edge::Following) | Step::Value(_) => next!(cx, r, following(ip), carried),
                    Step::Unresolved => sibling!(const { jump_row_miss::<H, BACKWARD>(LaneFields::<$L>::OPCODE) }, r, ip, view),
                }
            }
            if LaneFields::<$L>::FLAG_ROLE {
                if i.imm() != 0 { last_flag!($L, FLAG_SET) } else { last_flag!($L, FLAG_CLEAR) }
            } else {
                last_flag!($L, FLAG_UNREAD)
            }
        }}; }
        value_link!(L0);
        let ip = following(ip);
        // SAFETY: the next record of the running view (the chain's install checked its length).
        let i = unsafe { &*ip };
        value_link!(L1);
        let ip = following(ip);
        if const { L3 == crate::staged::NO_LINK } {
            last_link!(L2)
        } else {
            let i = unsafe { &*ip };
            value_link!(L2);
            let ip = following(ip);
            last_link!(L3)
        }
    }
}

/// Chain `(L0, L1, L2, L3)`'s record handler for its last link's jump direction `backward`.
pub(in crate::vm::dispatch_fast) const fn chain_row<H: Host, const L0: u16, const L1: u16, const L2: u16, const L3: u16>(backward: bool) -> Handler {
    if backward { chain_rows::<H, L0, L1, L2, L3, true> } else { chain_rows::<H, L0, L1, L2, L3, false> }
}

/// `chains!` index `chain` within `staged::FUSED_CHAINS` (past its end, entry 0 again).
const fn chain_index(chain: usize) -> usize { if chain < crate::staged::FUSED_CHAINS.len() { chain } else { 0 } }

/// Gives each record that starts a run of records whose rows are a chain of
/// `staged::FUSED_CHAINS` the chain's record (`chain_rows`). Runs before `fuse_rows`, so a
/// chain wins over the pair at its head; the later records keep their own rows for jumps
/// into them. A chain whose middle record is a jump target still fuses: the jump enters at
/// that record's own row.
pub(in crate::vm::dispatch_fast) fn fuse_chains<H: Host>(records: &mut [LaneInstruction]) {
    if const { crate::staged::FUSED_CHAINS.is_empty() } { return; }
    macro_rules! link { ($chain:literal, $k:literal) => { crate::staged::FUSED_CHAINS[chain_index($chain)][$k] }; }
    macro_rules! chains {
        ($($chain:literal),*) => {
            [$((
                [
                    (|handler| plain_row::<H, { link!($chain, 0) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| plain_row::<H, { link!($chain, 1) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| plain_row::<H, { link!($chain, 2) }>(handler)) as fn(*const ()) -> bool,
                    (|handler| if const { link!($chain, 3) == crate::staged::NO_LINK } { true } else { plain_row::<H, { link!($chain, 3) }>(handler) }) as fn(*const ()) -> bool,
                ],
                if link!($chain, 3) == crate::staged::NO_LINK { 3usize } else { 4 },
                (|handler| if const { link!($chain, 3) == crate::staged::NO_LINK } { jump_row_backward::<H, { link!($chain, 2) }>(handler) } else { jump_row_backward::<H, { link!($chain, 3) }>(handler) }) as fn(*const ()) -> bool,
                [chain_row::<H, { link!($chain, 0) }, { link!($chain, 1) }, { link!($chain, 2) }, { link!($chain, 3) }>(false),
                    chain_row::<H, { link!($chain, 0) }, { link!($chain, 1) }, { link!($chain, 2) }, { link!($chain, 3) }>(true)],
            )),*]
        };
    }
    let chains = chains!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63);
    const { assert!(crate::staged::FUSED_CHAINS.len() <= 64, "`chains!` covers 64 chains; add indices") };
    for pc in 0..records.len() {
        let Some((_, length, backward, row)) = chains.iter().find(|(links, length, _, _)| {
            pc + length <= records.len() && (0..*length).all(|k| links[k](records[pc + k].handler))
        }) else { continue };
        records[pc].handler = row[usize::from(backward(records[pc + length - 1].handler))] as *const ();
    }
}

/// Opcode pair `(A, B)`'s fused record handler for `B`'s jump direction `backward`.
pub(in crate::vm::dispatch_fast) const fn fused_row<H: Host, const A: u16, const B: u16, const MODE: u8>(backward: bool) -> Handler {
    if backward { fused_rows::<H, A, B, true, MODE> } else { fused_rows::<H, A, B, false, MODE> }
}

/// Whether `handler` is opcode `OP`'s plain row in any direction: its value row with no
/// accumulator field, or its jump row.
pub(in crate::vm::dispatch_fast) fn plain_row<H: Host, const OP: u16>(handler: *const ()) -> bool {
    if const { jumps_by_row(LaneFields::<OP>::OPCODE) } {
        return handler == jump_row::<H, OP>(false) as *const () || handler == jump_row::<H, OP>(true) as *const ();
    }
    is_row_flagged_plain::<H, OP>(handler)
}

/// Whether `handler` is opcode `OP`'s row with no accumulator field at any flag.
fn is_row_flagged_plain<H: Host, const OP: u16>(handler: *const ()) -> bool {
    [lane_row::<H, OP, ACC_NONE, FLAG_UNREAD> as Handler, lane_row::<H, OP, ACC_NONE, FLAG_CLEAR>, lane_row::<H, OP, ACC_NONE, FLAG_SET>]
        .iter().any(|row| *row as *const () == handler)
}

/// `pairs!` index `pair` within `staged::FUSED_ROWS`: indices past its end repeat entry 0, so
/// the table grows without touching the macro (a repeated entry never matches first).
const fn fused_index(pair: usize) -> usize { if pair < crate::staged::FUSED_ROWS.len() { pair } else { 0 } }

/// Gives each record whose row and next record's row are a pair of `staged::FUSED_ROWS` the
/// pair's fused record (`fused_rows`); the next record keeps its own row. A `FUSED_DEAD_FIRST`
/// pair takes its fused record only where `liveness` proves `A`'s result register dead after
/// the pair and `B` names that register in no operand; otherwise it takes the `FUSED_PLAIN` record.
pub(in crate::vm::dispatch_fast) fn fuse_rows<H: Host>(records: &mut [LaneInstruction], live: Option<&crate::compile::liveness::RegisterMasks>, osr_loop: bool) {
    macro_rules! pairs {
        ($($pair:literal),*) => {
            [$((
                (|handler| plain_row::<H, { crate::staged::FUSED_ROWS[fused_index($pair)].0 }>(handler)) as fn(*const ()) -> bool,
                (|handler| plain_row::<H, { crate::staged::FUSED_ROWS[fused_index($pair)].1 }>(handler)) as fn(*const ()) -> bool,
                jump_row_backward::<H, { crate::staged::FUSED_ROWS[fused_index($pair)].1 }> as fn(*const ()) -> bool,
                crate::staged::FUSED_ROWS[fused_index($pair)].2,
                [fused_row::<H, { crate::staged::FUSED_ROWS[fused_index($pair)].0 }, { crate::staged::FUSED_ROWS[fused_index($pair)].1 }, { crate::staged::FUSED_ROWS[fused_index($pair)].2 }>(false),
                    fused_row::<H, { crate::staged::FUSED_ROWS[fused_index($pair)].0 }, { crate::staged::FUSED_ROWS[fused_index($pair)].1 }, { crate::staged::FUSED_ROWS[fused_index($pair)].2 }>(true)],
                [fused_row::<H, { crate::staged::FUSED_ROWS[fused_index($pair)].0 }, { crate::staged::FUSED_ROWS[fused_index($pair)].1 }, { crate::staged::FUSED_PLAIN }>(false),
                    fused_row::<H, { crate::staged::FUSED_ROWS[fused_index($pair)].0 }, { crate::staged::FUSED_ROWS[fused_index($pair)].1 }, { crate::staged::FUSED_PLAIN }>(true)],
            )),*]
        };
    }
    let pairs = pairs!(0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12, 13, 14, 15, 16, 17, 18, 19, 20, 21, 22, 23, 24, 25, 26, 27, 28, 29, 30, 31, 32, 33, 34, 35, 36, 37, 38, 39, 40, 41, 42, 43, 44, 45, 46, 47, 48, 49, 50, 51, 52, 53, 54, 55, 56, 57, 58, 59, 60, 61, 62, 63, 64, 65, 66, 67, 68, 69, 70, 71, 72, 73, 74, 75, 76, 77, 78, 79, 80, 81, 82, 83, 84, 85, 86, 87, 88, 89, 90, 91, 92, 93, 94, 95, 96, 97, 98, 99, 100, 101, 102, 103, 104, 105, 106, 107, 108, 109, 110, 111, 112, 113, 114, 115, 116, 117, 118, 119, 120, 121, 122, 123, 124, 125, 126, 127);
    const { assert!(crate::staged::FUSED_ROWS.len() <= 128, "`pairs!` covers 128 fused pairs; add indices") };
    for pc in 0..records.len().saturating_sub(1) {
        let (first, second) = (records[pc].handler, records[pc + 1].handler);
        let Some((_, _, backward, mode, fused, plain)) = pairs.iter().find(|(a, b, _, _, _, _)| a(first) && b(second)) else {
            continue;
        };
        let direction = usize::from(backward(second));
        // A move or an add of a constant at a hot OSR latch stays its own record, which
        // `install_loop_sites` read as the latch's (`derive_lane_view`'s `then_fuses`).
        if osr_loop && direction == 1 && (plain_row::<H, { crate::staged::MOVE_ROW }>(first) || plain_row::<H, { crate::staged::WASM_I32_ADD_IMMEDIATE_ROW }>(first)) {
            continue;
        }
        // The dead-first form is only sound when `B` never reads `A`'s register, and nothing
        // after the pair reads it: the same liveness the hand pass used, plus `B`'s operands.
        let dead_first = {
            let (first, second) = (&records[pc], &records[pc + 1]);
            second.b != first.a
                && second.c != first.a
                && (second.a == first.a
                    || live
                        .and_then(|masks| masks.get(pc + 2))
                        .is_some_and(|after| crate::compile::liveness::register_dead(after, usize::from(first.a))))
        };
        let handler = if *mode == crate::staged::FUSED_DEAD_FIRST && !dead_first {
            plain[direction]
        } else {
            fused[direction]
        };
        records[pc].handler = handler as *const ();
    }
}

/// Whether `handler` is opcode `OP`'s backward jump row.
fn jump_row_backward<H: Host, const OP: u16>(handler: *const ()) -> bool {
    if const { jumps_by_row(LaneFields::<OP>::OPCODE) } { handler == jump_row::<H, OP>(true) as *const () } else { false }
}

/// The value `allocation` makes in the lane context `cx` (a `*mut LaneContext<H>`): it
/// allocates and runs no guest code, so a frame's own run makes it as the general loop does;
/// `None` where the general path must.
///
/// # Safety
/// `cx` is the running lane's context and `r` its register file.
unsafe fn lane_allocation<H: Host>(cx: *mut (), r: Registers, allocation: Allocation) -> Option<Value> {
    let cx = cx.cast::<LaneContext<H>>();
    match allocation {
        Allocation::Empty => Some(lane_new_object(unsafe { vm(cx) })),
        Allocation::Array { length } => Some(lane_new_hole_array(unsafe { vm(cx) }, length as usize)),
        Allocation::ArrayOf { elements } => Some(lane_literal_array_value(unsafe { vm(cx) }, r, elements)),
        Allocation::RegExp { site } => lane_regexp_literal_value(cx, site),
        Allocation::ConstArray { constant, count } => lane_const_array_value(cx, constant, count),
        Allocation::ObjectPair { site, first, second } => Some(lane_object_pair(cx, r, site, [first, second])),
        Allocation::ObjectLiteral { site, window } => Some(lane_object_literal_value(cx, r, site, window)),
        Allocation::Closure { function } => lane_closure_value_of(cx, function),
        Allocation::Template { template } => lane_template_value(cx, template),
        Allocation::Concat { window } => lane_concat_value(cx, r, window),
    }
}

/// `DefineArrayElement` of `value` at `index` into the literal array `array` in the lane context `cx`'s
/// own run (`Vm::define_literal_element`); false where the general path must define it.
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneHost::define`, a constant function pointer.
#[inline(always)]
unsafe fn lane_define_element<H: Host>(cx: *mut (), array: u64, index: u32, value: u64) -> bool {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    vm.define_literal_element(Value::from_bits(array), index as usize, Value::from_bits(value))
}

/// The value of `atom` under `read` in the lane context `cx`, which allocates nothing and runs
/// no guest code; `None` where the binding does not answer it (a native no first run recorded,
/// an intrinsic name the VM cannot evaluate, a hint the global object's shape no longer fits).
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneHost::read_name`, a constant
/// function pointer in the inlined frame `row_frame` builds for a by-name load, so the call inlines.
#[inline(always)]
unsafe fn lane_name_value<H: Host>(cx: *mut (), read: NameRead, atom: u32) -> Option<Value> {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    match read {
        NameRead::Native => {
            let native = vm.intrinsic_natives.get(atom as usize).copied().flatten()?;
            Some(vm.native_value(native))
        }
        NameRead::Intrinsic => vm.intrinsic_name_value(atom),
        NameRead::Field(hint) => vm.record_field_entry_hit(hint, vm.realm.globals),
    }
}

/// `IteratorCleanupPush` in the running VM frame, as the general loop runs it: the iterator to close if
/// the loop exits abruptly. A frameless activation keeps it on the lane stack (`LaneStack::iterators`),
/// which its frame takes if it materializes.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn iterator_push_in_lane<H: Host>(cx: *mut (), iterator: Register, done: Register) {
    let cx = cx.cast::<LaneContext<H>>();
    let vm = unsafe { vm(cx) };
    let iterator = crate::vm::ActiveIterator { iterator, done };
    if let Some(depth) = vm.lane_stack.activations.len().checked_sub(1) {
        vm.lane_stack.iterators.push((depth, iterator));
    } else {
        let frame = unsafe { (*cx).frame() };
        vm.frames[frame].active_iterators.push(iterator);
    }
}

/// `IteratorCleanupPop` in the running VM frame: the running activation's own entry, the last. `false`
/// where there is none, which the general path raises for.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn iterator_pop_in_lane<H: Host>(cx: *mut ()) -> bool {
    let cx = cx.cast::<LaneContext<H>>();
    let vm = unsafe { vm(cx) };
    match vm.lane_stack.activations.len().checked_sub(1) {
        Some(depth) => vm.lane_stack.iterators.pop_if(|(owner, _)| *owner == depth).is_some(),
        None => {
            let frame = unsafe { (*cx).frame() };
            vm.frames[frame].active_iterators.pop().is_some()
        }
    }
}

/// The `true` of `delete base[key]` of a string key on an ordinary object, which runs no guest
/// code. A refused deletion (a non-configurable property) has changed nothing and, like any
/// other base or key, is the general path's: strict code makes it an error.
///
/// # Safety
/// `cx` is the running lane's context.
unsafe fn lane_delete_value<H: Host>(cx: *mut (), base: Value, key: Value) -> Option<Value> {
    let cx = cx.cast::<LaneContext<H>>();
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let code = unsafe { &*(*cx).code };
    let plain = base.direct_object_index().is_some()
        && vm.heap.string(key).is_some()
        && vm.object_data(base).is_some_and(|data| !data.is_module_namespace() && !data.is_arguments_object());
    (plain && vm.object_delete_property(code, &[base, key]).is_ok_and(|deleted| deleted == Value::TRUE)).then_some(Value::TRUE)
}

/// The fast answer of a `Names` row's iteration or for-in call-out over its operands (`[first, second]`,
/// the registers the call names in order): the VM's fast arm of the step, which runs no guest code and
/// allocates at most what the general step would. `None` where the general step decides it.
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneFrame::names`, a constant function pointer
/// in the inlined frame `row_frame` builds, so the call inlines.
#[inline(always)]
unsafe fn fast_names_step<H: Host>(cx: *mut (), call: CallOut, [first, second]: [Option<Value>; 2]) -> Option<Value> {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    match call {
        // A built-in iterator whose chain lacks `return` closes with `undefined`.
        CallOut::Protocol { step: Protocol::IteratorClose, .. } => {
            vm.iterator_close_absent(second?).then_some(Value::UNDEFINED)
        }
        // The intrinsic values iterator of an array whose iteration is intact, made without looking
        // up or calling `@@iterator`.
        CallOut::Protocol { step: Protocol::GetIterator, .. } => {
            let source = second?;
            if !vm.array_iteration_intact(source) {
                return None;
            }
            let iterator = vm.array_iterator_of(crate::heap::IteratorKind::ArrayValues, source);
            note_allocation(vm);
            Some(iterator)
        }
        // The loop's state over the snapshot of an object whose keys the for-in memo lists.
        CallOut::Protocol { step: Protocol::ForInStart, .. } => {
            let source = second?;
            let keys = vm.listed_for_in_keys(source)?;
            let state = vm.lane_for_in_state(source, keys);
            // A state may be allocated; a due collection runs at the lane's next safepoint.
            note_allocation(vm);
            Some(state)
        }
        // The next key of a listed snapshot, `Value::DELETED` once none is left (which takes the exit).
        CallOut::ForInNext { .. } => vm.for_in_step_listed(second?).map(|key| key.unwrap_or(Value::DELETED)),
        // A spread onto the literal's array with no elisions before it, from a source whose
        // iteration runs no guest code: the append defines no register, so its value is unused.
        CallOut::Protocol { step: Protocol::AppendArray { length: 0, spread: true }, .. } => {
            let (array, source) = (first?, second?);
            if !vm.spread_intact_in_place(array, source) {
                return None;
            }
            note_allocation(vm);
            Some(Value::UNDEFINED)
        }
        _ => None,
    }
}

/// The array of the `count` program constants from `constant` (`Vm::const_array_value`);
/// `None` where the range is outside the program's constants.
#[inline(never)]
fn lane_const_array_value<H: Host>(cx: *mut LaneContext<H>, constant: u32, count: u16) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    // SAFETY: the context's program outlives the lane run.
    let value = vm.const_array_value(unsafe { (*cx).program }, constant as usize, usize::from(count))?;
    note_allocation(vm);
    Some(value)
}

/// The value of literal template `template` in a frame's own lane run (`Vm::literal_template`);
/// `None` inside a lane activation, whose frame is not the context's, or where it fails.
#[inline(never)]
fn lane_template_value<H: Host>(cx: *mut LaneContext<H>, template: u32) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    if !vm.lane_stack.activations.is_empty() {
        return None;
    }
    // SAFETY: the context's program outlives the lane run.
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame()) };
    let value = vm.literal_template(code, frame, template as usize).ok();
    note_allocation(vm);
    value
}

/// The string of the parts in `window`, each converted without guest code
/// (`Vm::pure_string_concat`); `None` where any part needs the general path.
#[inline(never)]
fn lane_concat_value<H: Host>(cx: *mut LaneContext<H>, r: Registers, window: RegisterWindow) -> Option<Value> {
    let vm = unsafe { vm(cx) };
    let base = usize::from(window.base);
    // SAFETY: the context's program outlives the lane run.
    let value = vm.pure_string_concat(unsafe { &*(*cx).code }, usize::from(window.count),
        |_: &Vm<H>, part: usize| r.get((base + part) as Register))?;
    note_allocation(vm);
    Some(value)
}

/// `allocation` into `destination` by the staged operation, in the lane context `cx`'s own run;
/// false, writing nothing, where the general path must make it.
#[inline(always)]
pub(super) fn staged_allocate<H: Host>(cx: *mut LaneContext<H>, r: Registers, destination: u16, allocation: Allocation) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::hosted::<H>(r, Carried::script(cx), cx);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::Allocate { destination, allocation, proof: () }), Step::Value(_))
}

/// `delete base[key]` into `destination` by the staged operation, in the lane context `cx`'s own
/// run; false, writing nothing, where the general path must make it.
#[inline(always)]
pub(super) fn staged_delete<H: Host>(cx: *mut LaneContext<H>, r: Registers, [destination, base, key]: [u16; 3], strict: bool) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::hosted::<H>(r, Carried::script(cx), cx);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::Delete { destination, base, key, strict }), Step::Value(_))
}

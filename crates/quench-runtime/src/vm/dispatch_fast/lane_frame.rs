//! The interpreter's register frame: `staged` moves and constants over the live file.

use super::*;
use super::lane_reads::{RecordOperand, Reads};
use crate::bytecode::{Operand, Register};
use crate::typed_ir::Type;
use crate::bytecode::RegisterWindow;
use crate::staged::{Allocation, CallOut, Comparison, Concrete, Constant, Edge, Frame, GlobalBinding, NumericFrame, Operation, Protocol, Step, read_initialized_local, write_initialized_local, write_local};

/// The maker of a frame's heap values: allocates in the lane context it is given and runs no
/// guest code; `None` leaves the instruction to the general path.
type AllocationMaker = unsafe fn(*mut (), Registers, Allocation) -> Option<Value>;

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
    delete: PropertyDeleter,
    read_name: NameReader,
    /// What the by-name load the frame runs resolved its name to, if it runs one.
    name: Option<NameBinding>,
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
    /// The constants, local slots and outer captures the staged row's operands name.
    reads: Reads,
}

/// The running frame's captured environment for a staged environment row: the storage of its
/// slots, which `read` finds for a read and `write` for a store (it takes the environment's barrier).
#[derive(Clone, Copy)]
struct EnvironmentReads {
    context: *mut (),
    read: unsafe fn(*mut ()) -> Option<*mut crate::heap::EnvironmentSlots>,
    write: unsafe fn(*mut (), u16) -> Option<*mut crate::heap::EnvironmentSlots>,
/// A row's global-name store: the lane context and the record's own hint, which the store
/// writes through (`global_name_store`).
#[derive(Clone, Copy)]
struct GlobalStore {
    cx: *mut (),
    hint: crate::vm::FieldCache,
    store: unsafe fn(*mut (), crate::vm::FieldCache, u32, Value) -> bool,
}

impl<const ACC: u8> LaneFrame<ACC> {
    /// A frame for an operation that reads no `this`.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, this: Value::DELETED, locals: None, host: None, captures: None, decisions: None,
            capture_slots: None, heap: None, frame_this: None, arguments: None, environment: None, global_store: None, binary: None, names: None, reads: Reads::NONE }
    }

    /// A frame whose allocation and deletion rows make their values in the lane context `cx`.
    #[inline(always)]
    fn hosted<H: Host>(registers: Registers, view: Carried, cx: *mut LaneContext<H>) -> Self {
        let host = LaneHost { cx: cx.cast(), allocate: lane_allocation::<H>, delete: lane_delete_value::<H>,
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
            capture_slots: None, heap: None, frame_this: None, arguments: None, environment: None, global_store: None, binary: None, names: None, reads: Reads::NONE }
    }
}

/// The operands and operator of a binary call-out the lane's pure binary answers: a `Binary`, or an
/// `Equality` as the operator of its strictness and negation.
fn binary_call(operation: CallOut) -> Option<(Register, Register, u32)> {
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
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, Boxing) {
        (Value::boolean(condition).to_bits(), Boxing::Value)
    }

    #[inline(always)]
    fn load_local(&mut self, _stage: &mut Concrete, slot: u16, _miss: Option<()>)
        -> Option<(u64, Boxing)> {
        let (locals, len) = self.locals?;
        (usize::from(slot) < len).then(|| {
            // SAFETY: `slot` is inside the activation's live local slots.
            (unsafe { *locals.add(usize::from(slot)) }.to_bits(), Boxing::Value)
        })
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
    fn store_local(&mut self, _stage: &mut Concrete, slot: u16, value: u64, _miss: Option<()>) -> bool {
        let Some((locals, len)) = self.locals else { return false };
        if usize::from(slot) >= len { return false; }
        // SAFETY: `slot` is inside the activation's live local slots.
        unsafe { *locals.add(usize::from(slot)) = Value::from_bits(value) };
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
            let (left, right) = (Value::from_bits(self.read(stage, left)), Value::from_bits(self.read(stage, right)));
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
                let operands = operands.map(|register| register.map(|register| Value::from_bits(self.read(stage, register))));
                // SAFETY: the running lane's context, live for the row's step.
                unsafe { step(context, operation, operands) }.map(|value| (value.to_bits(), Boxing::Value))
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
/// environment's write barrier (`Heap::remember_cell`), as `lane_store_window_local_general` takes it.
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

impl<const ACC: u8> crate::staged::ElementFrame<Concrete> for LaneFrame<ACC> {}

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
        return LaneFrame::<ACC> { heap: Some(heap), arguments: Some((cx.cast(), count)), environment: Some(environment), global_store, names: Some((cx.cast(), names)),
            ..LaneFrame::<ACC>::new(r, view) };
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
        return LaneFrame::<ACC> { this: r.this(), frame_this: Some((cx.cast(), read)), capture_slots, captures,
            ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
        // A generic row's word-decided answers, and the pure binary of its call-outs (`lane_binary_values`).
        let binary: unsafe fn(*mut (), u32, Value, Value) -> Option<Value> =
            |context, operator, left, right| unsafe { super::lane_binary_values::<H>(context.cast(), operator, left, right) };
        return LaneFrame::<ACC> { binary: Some((cx.cast(), binary)), ..LaneFrame::<ACC>::new(r, view) };
    }
    if !locals {
        return LaneFrame::<ACC>::new(r, view);
    }
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame() }];
    let (slots, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, slots, len)
}

/// Where opcode `op`'s row leaves when its words do not decide it: the VM's general step, or for
/// a capture access the lane's own slow arm, which walks the scopes and requickens the record to
/// the root binding it finds (`lane_load_capture_walk`, `lane_store_outer_capture`).
const fn row_miss<H: Host>(op: Op) -> Handler {
    match crate::staged::miss(op) {
        crate::staged::Miss::WalkScopes => super::lane_load_capture_walk::<H>,
        crate::staged::Miss::StoreThroughScopes => super::lane_store_outer_capture::<H>,
        crate::staged::Miss::ArgumentsBinding => super::lane_frame_arguments::<H, false>,
        crate::staged::Miss::EnvironmentLoad => super::lane_load_local::<H>,
        crate::staged::Miss::EnvironmentStore => super::lane_store_local::<H>,
        crate::staged::Miss::StoreNameFirst => super::lane_store_name_first::<H>,
        crate::staged::Miss::General | crate::staged::Miss::CellTruth { .. } => lane_general_step::<H>,
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
        else { crate::staged::step($stage, $frame, $operation) }
    }};
}

/// Where opcode `op`'s jump row leaves when its words do not decide the edge: a truth test of a
/// heap value asks its cell (`lane_jump_test_cell`); any other takes the VM's general step.
const fn jump_row_miss<H: Host, const BACKWARD: bool>(op: Op) -> Handler {
    match crate::staged::miss(op) {
        crate::staged::Miss::CellTruth { taken: false } => super::lane_jump_test_cell::<H, BACKWARD, false>,
        crate::staged::Miss::CellTruth { taken: true } => super::lane_jump_test_cell::<H, BACKWARD, true>,
        _ => lane_general_step::<H>,
    }
}

/// Whether opcode `op` runs its generated jump row (`row_jump`): a jump the flow decoder
/// stages (`Decoder::Flow`).
pub(in crate::vm::dispatch_fast) const fn jumps_by_row(op: Op) -> bool {
    op.control_flow_layout().jumps() && matches!(op.staged(), Some(crate::bytecode::Decoder::Flow))
}

/// Opcode `OP`'s jump row in direction `backward`.
pub(in crate::vm::dispatch_fast) const fn jump_row<H: Host, const OP: u16>(backward: bool) -> Handler {
    if backward { row_jump::<H, OP, true> } else { row_jump::<H, OP, false> }
}

lane_handler! {
    /// The lane handler of every jump `jumps_by_row` admits, generated from its staged row as
    /// `lane_row` is: the flow decoder reads the record, the row steps on `Concrete`, and its
    /// edge picks the continuation. A jump to its target runs the lane's jump (`lane_jump`),
    /// whose backward edge (`BACKWARD`, which the view fixes per record) is a safepoint.
    pub(super) fn row_jump<OP: u16, BACKWARD: bool>(cx, r, ip, i, view) {
        let fields = LaneFields::<OP>(i);
        let Some(operation) = Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY) else {
            sibling!(lane_general_step::<H>, r, ip, view)
        };
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
        match crate::staged::step_flow(&mut stage, &mut frame, operation) {
            Step::Control(Edge::Target) => resume!(cx, super::lane_jump::<H>(r, ip, BACKWARD, view, None)),
            Step::Control(Edge::Following) | Step::Value(_) => next!(cx, r, following(ip), view),
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
        // `if const`, not `match const`: rustc compiles only a taken `if const` branch, so each
        // instantiation holds its one decoder (a `match` on a constant compiles every arm).
        let decoded = if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Flow)) } {
            Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY)
        } else if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Local)) } {
            Operation::decode_local(crate::staged::Fields::wide(fields))
        } else if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Value)) } {
            Operation::decode_value(crate::staged::Fields::wide(fields))
        } else if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Generic)) } {
            Operation::decode_generic(crate::staged::Fields::wide(fields))
        } else if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
            Operation::decode_names(crate::staged::Fields::wide(fields))
        } else {
            None
        };
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
            Step::Control(_) | Step::Unresolved => sibling!(const { row_miss::<H>(LaneFields::<OP, FLAG>::OPCODE) }, r, ip, view),
        }
    }
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

/// The value of `atom` under `read` in the lane context `cx`, which allocates nothing and runs
/// no guest code; `None` where the binding does not answer it (a native no first run recorded,
/// an intrinsic name the VM cannot evaluate, a hint the global object's shape no longer fits).
///
/// # Safety
/// `cx` is the running lane's context. Called through `LaneHost::read_name`, a constant
/// function pointer in the inlined frame `staged_load_name` builds, so the call inlines.
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

/// The by-name load of record `i` (a call's, with a receiver, where `call`) by the staged
/// `LoadName` row over `binding`, in the lane context `cx`'s own run; false, writing nothing,
/// where the binding does not answer it.
#[inline(always)]
pub(super) fn staged_load_name<H: Host>(cx: *mut LaneContext<H>, r: Registers, i: &LaneInstruction, call: bool,
    binding: NameBinding) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::hosted::<H>(r, Carried::script(cx), cx).named(binding);
    let row = Operation::LoadName { destination: i.a, atom: i.imm(), site: i.c, receiver: call.then_some(i.b), proof: () };
    matches!(crate::staged::step(&mut stage, &mut frame, row), Step::Value(_))
}

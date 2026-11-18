//! The interpreter's register frame: `staged` moves and constants over the live file.

use super::*;
use super::lane_reads::{RecordOperand, Reads};
use crate::bytecode::{Operand, Register};
use crate::typed_ir::Type;
use crate::bytecode::RegisterWindow;
use crate::staged::{Allocation, CallOut, Comparison, Concrete, Constant, Edge, Frame, GlobalBinding, NumericFrame, Operation, Step, read_initialized_local, write_initialized_local, write_local};

/// The maker of a frame's heap values: allocates in the lane context it is given and runs no
/// guest code; `None` leaves the instruction to the general path.
type AllocationMaker = unsafe fn(*mut (), Registers, Allocation) -> Option<Value>;

/// The deleter of a frame's own `delete base[key]` hit: the `true` of a deletion that runs no
/// guest code; `None` leaves the instruction to the general path.
type PropertyDeleter = unsafe fn(*mut (), Value, Value) -> Option<Value>;

/// The reader of a frame's by-name load: the value of `atom` under a binding the record's first
/// run resolved, which runs no guest code; `None` leaves the instruction to the residual.
type NameReader = unsafe fn(*mut (), NameRead, u32) -> Option<Value>;

/// The values a keeping activation kept (`kept_values`).
type KeptValues = unsafe fn(*mut ()) -> Option<(u16, u16)>;

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
    /// The values a keeping activation kept (`kept_values`): their count and the register
    /// that holds the first, past the function's registers.
    kept_values: KeptValues,
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
    /// The constants, local slots and outer captures the staged row's operands name.
    reads: Reads,
    /// The plan of the named read the frame runs, as its record holds it (`RecordPlan`).
    plan: Option<RecordPlan>,
}

/// A named read's plan as a lane record holds it: the ops the record's own-field hint proves
/// (`staged::ic`), over the object record table the lane bound. The generated row reads it as a
/// specializer reads the plan its site proved (`PlanSource`), so the record's quickened state is
/// data the one staged row runs, not a handler written for it.
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct RecordPlan {
    ops: [crate::staged::ic::IcOp<'static>; 2],
    /// How many of `ops` the plan runs.
    len: usize,
    records: u64,
    /// The heap whose arrays, functions and ordinary builtins a plan any receiver runs
    /// (`IcOp::Held`) finds the object part of (`Heap::held_other_receiver`); `None` where the
    /// plan runs over direct objects alone.
    other: Option<*const crate::heap::Heap>,
    /// Whether the plan starts from the receiver's word rather than its object record
    /// (`IcOp::HeldString`).
    word: bool,
    /// The own-field store the record's hint proves, `(layout, slot)` (`StorePlan::Layout`);
    /// `None` for a read's plan.
    store: Option<(u32, u16)>,
    /// Whether the plan's receivers are mostly functions (`own_other`): their object part is
    /// found inline, before any other family.
    functions: bool,
}

impl RecordPlan {
    /// The own-slot read of the hint `(layout, slot)`: one compare of the record's layout id,
    /// whose storage form the site fixed where it fixed one (`own_formed_handler`).
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own(layout: u32, slot: u16, records: u64) -> Self {
        use crate::staged::ic::IcOp;
        let load = IcOp::LoadSlot { slot, form: crate::staged::Form::Either, storage_len: usize::from(slot) + 1,
            rep: crate::typed_ir::Representation::Tagged };
        Self { ops: [IcOp::GuardLayout(layout), load], len: 2, records, other: None, word: false, store: None, functions: false }
    }

    /// `own` of a receiver of any object part (`Heap::held_other_receiver`): a function's record
    /// takes the same layout compare.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_other(layout: u32, slot: u16, records: u64, heap: *const crate::heap::Heap) -> Self {
        Self { other: Some(heap), functions: true, ..Self::own(layout, slot, records) }
    }

    /// `own_other` of a layout whose storage form `form` the row's site fixed.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_other_formed(layout: u32, form: u32, slot: u16, records: u64,
        heap: *const crate::heap::Heap) -> Self {
        let mut plan = Self::own_other(layout, slot, records, heap);
        plan.ops[0] = crate::staged::ic::IcOp::GuardFormed { id: layout, form };
        plan
    }

    /// The read through a prototype hint whose operands are `guard`, `watch` and `value`
    /// (`IcOp::Held`).
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn held(guard: u64, watch: u64, value: u64, records: u64,
        other: Option<*const crate::heap::Heap>) -> Self {
        use crate::staged::ic::IcOp;
        let held = IcOp::Held { guard, watch, value };
        Self { ops: [held, held], len: 1, records, other, word: false, store: None, functions: false }
    }
}

impl RecordPlan {
    /// The own-field store of the hint `(layout, slot)`: one compare of the record's layout id,
    /// whose entry names the storage form (`StorePlan::Layout`); what it refuses is the VM's.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn own_store(layout: u32, slot: u16, records: u64) -> Self {
        use crate::staged::ic::IcOp;
        let none = IcOp::GuardLayout(layout);
        Self { ops: [none, none], len: 0, records, other: None, word: false, store: Some((layout, slot)), functions: false }
    }

    /// The read of a string receiver through a `String.prototype` hint whose operands are
    /// `watch` and `value` (`IcOp::HeldString`), from the receiver's word.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn held_string(watch: u64, value: u64) -> Self {
        use crate::staged::ic::IcOp;
        let held = IcOp::HeldString { watch, value };
        Self { ops: [held, held], len: 1, records: 0, other: None, word: true, store: None, functions: false }
    }
}

/// The plans a lane frame holds: the record's own, or none.
#[derive(Clone, Copy)]
pub(in crate::vm::dispatch_fast) struct LanePlans(Option<([crate::staged::ic::IcOp<'static>; 2], usize)>, Option<(u32, u16)>);

impl crate::staged::property::PlanSource for LanePlans {
    #[inline(always)]
    fn field_plan(&self, _lookup: crate::bytecode::FieldLookup) -> Option<crate::staged::property::FieldPlan<'_>> {
        self.0.as_ref().map(|(ops, len)| crate::staged::property::FieldPlan::Ops(&ops[..*len]))
    }
    #[inline(always)]
    fn store_plan(&self, _site: u16) -> Option<crate::staged::property::StorePlan<'_>> {
        // The VM's own-field hit completes what the layout store refuses (`StoreJoin::Host`),
        // which the lane frame answers as a miss: the record's store handlers take it.
        self.1.map(|(id, slot)| crate::staged::property::StorePlan::Layout { id, slot, join: crate::staged::property::StoreJoin::Host })
    }
}

impl<const ACC: u8> LaneFrame<ACC> {
    /// A frame for an operation that reads no `this`.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, this: Value::DELETED, locals: None, host: None, captures: None, decisions: None,
            capture_slots: None, heap: None, frame_this: None, reads: Reads::NONE, plan: None }
    }

    /// A frame whose allocation and deletion rows make their values in the lane context `cx`.
    #[inline(always)]
    fn hosted<H: Host>(registers: Registers, view: Carried, cx: *mut LaneContext<H>) -> Self {
        let host = LaneHost { cx: cx.cast(), allocate: lane_allocation::<H>, delete: lane_delete_value::<H>,
            read_name: lane_name_value::<H>, kept_values: kept_values::<H>, name: None };
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
            capture_slots: None, heap: None, frame_this: None, reads: Reads::NONE, plan: None }
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

    /// A rest view's read from the values the running keeping activation kept past its
    /// registers (`kept_values`), read in place; a frame keeps none of its own.
    #[inline(always)]
    fn rest_view(&mut self, _stage: &mut Concrete, get: bool, fixed: usize, key: Option<Register>) -> Option<(u64, Self::Fact)> {
        let host = self.host?;
        // SAFETY: the reader was made for the lane context this frame runs in.
        let (argc, first) = unsafe { (host.kept_values)(host.cx) }?;
        // A rest view's fixed parameters are a function's (`Function::params`), a `u16`.
        let fixed = fixed as u16;
        let length = argc.saturating_sub(fixed);
        if !get {
            return Some((Value::integer(i32::from(length)).to_bits(), Boxing::Value));
        }
        // A negative index wraps past every length, so one unsigned compare bounds it.
        let index = self.registers.get(key?).as_int().map(|index| index as u32).filter(|index| *index < u32::from(length))?;
        Some((self.registers.get(first + fixed + index as u16).to_bits(), Boxing::Value))
    }

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
        let CallOut::Decide { left, right, operator } = operation else { return None };
        let Decisions { context, decide, .. } = self.decisions?;
        let (left, right) = (Value::from_bits(self.read(stage, left)), Value::from_bits(self.read(stage, right)));
        decide(context, operator, left, right).map(|holds| (Value::boolean(holds).to_bits(), Boxing::Value))
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
}

impl<const ACC: u8> crate::staged::CallFrame<Concrete> for LaneFrame<ACC> {}

impl<const ACC: u8> crate::staged::PropertyFrame<Concrete> for LaneFrame<ACC> {
    type Plans = LanePlans;

    #[inline(always)]
    fn plans(&self) -> LanePlans { LanePlans(self.plan.map(|plan| (plan.ops, plan.len)), self.plan.and_then(|plan| plan.store)) }

    /// The activation's `this` as the lane holds it: an unbound one (`Value::DELETED`) is no
    /// direct object, so the receiver's record test refuses it with no test of its own.
    #[inline(always)]
    fn field_this(&mut self, _stage: &mut Concrete, _miss: ()) -> Option<u64> { Some(self.this.to_bits()) }

    /// A direct object's record in the table, or, where the plan runs over any object part
    /// (`RecordPlan::other`), the object part of an array, function or builtin, out of line.
    #[inline(always)]
    fn receiver_record(&mut self, stage: &mut Concrete, _owner: crate::staged::property::Owner, object: u64, _readable: bool,
        miss: ()) -> Option<crate::staged::ic::Reached<u64, bool>> {
        if let Some(RecordPlan { word: true, .. }) = self.plan {
            return Some(crate::staged::ic::Reached::bare(object));
        }
        let Some(RecordPlan { records, other: Some(heap), functions, .. }) = self.plan else {
            return crate::staged::property::receiver(stage, self, object, miss);
        };
        // SAFETY: the heap is the lane's, live for the read.
        if functions && let Some(record) = unsafe { &*heap }.function_receiver(Value::from_bits(object)) {
            return Some(crate::staged::ic::Reached::bare(record as *const crate::heap::Object as u64));
        }
        if let Some(id) = Value::from_bits(object).direct_object_index() {
            return Some(crate::staged::ic::Reached::bare(crate::staged::object::object_record_at(stage, records, u64::from(id))));
        }
        // SAFETY: as above.
        let record = unsafe { &*heap }.held_other_receiver(Value::from_bits(object))?;
        Some(crate::staged::ic::Reached::bare(record as *const crate::heap::Object as u64))
    }

    #[inline(always)]
    fn delete_property(&mut self, _stage: &mut Concrete, base: u64, key: u64, _strict: bool, _miss: ()) -> Option<(u64, Boxing)> {
        let host = self.host?;
        // SAFETY: the deleter was made for the lane context this frame runs in.
        let deleted = unsafe { (host.delete)(host.cx, Value::from_bits(base), Value::from_bits(key)) }?;
        Some((deleted.to_bits(), Boxing::Value))
    }

    /// The live object record table, which an `instanceof` walk and a planned read address.
    #[inline(always)]
    fn object_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        if let Some(plan) = self.plan { return Some(plan.records); }
        let Decisions { context, records, .. } = self.decisions?;
        records(context)
    }
}

impl<const ACC: u8> crate::staged::ElementFrame<Concrete> for LaneFrame<ACC> {
    /// The array records of the heap an element row reads (`row_frame` binds it).
    #[inline(always)]
    fn array_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        // SAFETY: the heap is the lane's, live for the row.
        // The live table: a direct array receiver proves it reserved, and any other receiver
        // leaves at the access's family test before it is addressed.
        self.heap.map(|heap| unsafe { (*heap).live_array_records() } as u64)
    }

    /// The lane keeps no views: a read takes the element of any backing in place.
    #[inline(always)]
    fn reads_any_backing(&self) -> bool { true }

    /// The array's write barrier admits `value` unlogged (`Heap::array_store_unlogged`).
    #[inline(always)]
    fn array_store_gate(&mut self, _stage: &mut Concrete, base: u64, value: u64, _miss: ()) -> Option<()> {
        // SAFETY: as `array_records`.
        let heap = unsafe { &*self.heap? };
        heap.array_store_unlogged(Value::from_bits(base), Value::from_bits(value)).then_some(())
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
/// decrement are two handlers of one row). For any other row, `FLAG_SET` states a fact the view
/// proved of the record's immediate: its high half is zero (`row_near`).
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
fn row_frame<H: Host, const OP: u16, const ACC: u8>(cx: *mut LaneContext<H>, r: Registers, view: Carried) -> LaneFrame<ACC> {
    let locals = const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Local)) };
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
        // A JavaScript view carries its context (`script_handler!`), so no thread-local load.
        let heap: *const crate::heap::Heap = &unsafe { vm(view.memory.bytes.cast::<LaneContext<H>>()) }.heap;
        return LaneFrame::<ACC> { heap: Some(heap), ..LaneFrame::<ACC>::new(r, view) };
    }
    if const { matches!(LaneFields::<OP>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
        let heap: *const crate::heap::Heap = &unsafe { vm(cx) }.heap;
        return LaneFrame::<ACC> { heap: Some(heap), ..LaneFrame::<ACC>::hosted::<H>(r, view, cx) };
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
        crate::staged::Miss::General => lane_general_step::<H>,
    }
}

/// Opcode `OP`'s row handler for a record whose immediate the view proved to fit its low half
/// (`FLAG_SET`), where `handler` is the plain row (`FLAG_UNREAD`, no accumulator field): a
/// capture access at depth 0 then reads its slot without testing the depth.
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
        // Where a planned named read misses: the own read past its record's hint, which reads
        // through the site's cache and rebinds the hint; a held read, the site's holder entry.
        macro_rules! property_miss { () => {{
            let script: *mut LaneContext<H> = view.memory.bytes.cast();
            if const { super::held_site(FLAG) && !super::held_other_site(FLAG) } {
                // A direct object's miss; the read of any other receiver's object part.
                if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                    sibling!(lane_row::<H, OP, ACC, { super::HELD_OTHER_THIS }>, r, ip, Carried::script_in(script, view))
                }
                sibling!(lane_row::<H, OP, ACC, { super::HELD_OTHER_REGISTER }>, r, ip, Carried::script_in(script, view))
            }
            if const { FLAG == super::STRING_SITE } {
                sibling!(lane_get_field_string_refresh::<H>, r, ip, Carried::script_in(script, view))
            }
            if const { super::cell_own_site(FLAG) } {
                // The record moves on as the site's tier decides, which rebinds the hint.
                sibling!(lane_get_field_miss::<H>, r, ip, Carried::script_in(script, view))
            }
            if const { super::inherited_site(FLAG) && !super::inherited_other_site(FLAG) } {
                // A direct object's miss; the read of any other receiver's object part.
                if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                    sibling!(lane_row::<H, OP, ACC, { super::INHERITED_OTHER_THIS }>, r, ip, Carried::script_in(script, view))
                }
                sibling!(lane_row::<H, OP, ACC, { super::INHERITED_OTHER_REGISTER }>, r, ip, Carried::script_in(script, view))
            }
            if const { super::inherited_other_site(FLAG) } {
                if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                    sibling!(lane_get_field_inherited_walk::<H, true>, r, ip, Carried::script_in(script, view))
                }
                sibling!(lane_get_field_inherited_walk::<H, false>, r, ip, Carried::script_in(script, view))
            }
            if const { FLAG == super::HELD_CELL_REGISTER } {
                sibling!(lane_get_field_holder_cell_refresh::<H>, r, ip, Carried::script_in(script, view))
            }
            if const { super::held_other_site(FLAG) } {
                if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                    sibling!(lane_get_field_holder_refresh::<H, true>, r, ip, Carried::script_in(script, view))
                }
                sibling!(lane_get_field_holder_refresh::<H, false>, r, ip, Carried::script_in(script, view))
            }
            if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                sibling!(lane_get_field_own_refresh::<H, true, true>, r, ip, Carried::script_in(script, view))
            }
            sibling!(lane_get_field_own_refresh::<H, false, true>, r, ip, Carried::script_in(script, view))
        }} }
        let property = const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Property)) };
        // A named store (`SetField` and its `this` and strict forms): its record's own-field hint
        // as a layout store (`RecordPlan::own_store`); whatever it refuses, the store handlers.
        let store = const { LaneFields::<OP, FLAG>::OPCODE.field_store().is_some() };
        let this_store = const { matches!(LaneFields::<OP, FLAG>::OPCODE.field_store(),
            Some(crate::bytecode::FieldStore { receiver: crate::bytecode::StoreReceiver::This, .. })) };
        macro_rules! store_miss { () => {{
            let script: *mut LaneContext<H> = view.memory.bytes.cast();
            if this_store {
                sibling!(super::lane_set_field_own_miss::<H, true, false>, r, ip, Carried::script_in(script, view))
            }
            sibling!(super::lane_set_field_own_miss::<H, false, false>, r, ip, Carried::script_in(script, view))
        }} }
        if property && !store {
            // The site fact names the base (`own_site`), `this` or a register, and the lane admits
            // the record only without result flags: stated so the decode's tests fold.
            unsafe {
                std::hint::assert_unchecked(i.a <= crate::bytecode::REGISTER_MASK);
                if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                    std::hint::assert_unchecked(i.b == crate::bytecode::FieldBase::THIS.0);
                } else {
                    std::hint::assert_unchecked(i.b <= crate::bytecode::REGISTER_MASK);
                }
            }
        }
        if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } {
            // A keyed read's row is installed for a register base and key without result flags
            // (`element_read_row`), a keyed write's operands are three registers, as the
            // handlers these rows replaced read them: stated so the decode's operand tests fold.
            unsafe {
                std::hint::assert_unchecked(i.a <= crate::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.b <= crate::bytecode::REGISTER_MASK);
                std::hint::assert_unchecked(i.c <= crate::bytecode::REGISTER_MASK);
            }
        }
        // The row family's decoder, a constant of the instantiation, so only its arm is compiled.
        let fields = LaneFields::<OP, FLAG>(i);
        let decoded = match const { LaneFields::<OP, FLAG>::OPCODE.staged() } {
            Some(crate::bytecode::Decoder::Flow) => Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY),
            Some(crate::bytecode::Decoder::Local) => Operation::decode_local(crate::staged::Fields::wide(fields)),
            Some(crate::bytecode::Decoder::Value) => Operation::decode_value(crate::staged::Fields::wide(fields)),
            Some(crate::bytecode::Decoder::Names) => Operation::decode_names(crate::staged::Fields::wide(fields)),
            Some(crate::bytecode::Decoder::Property) => Operation::decode_property(crate::staged::Fields::wide(fields)),
            Some(crate::bytecode::Decoder::Element) => Operation::decode_element(crate::staged::Fields::wide(fields)),
            _ => None,
        };
        let Some(operation) = decoded else { sibling!(lane_general_step::<H>, r, ip, view) };
        // A row that reads records in place (`Decoder::Names`) or a planned named read may read a slot of the property arena.
        let property_values = if property && !store {
            // A JavaScript view carries its context (`script_handler!`), so no thread-local load.
            unsafe { vm(view.memory.bytes.cast::<LaneContext<H>>()) }.heap.arena_values_base()
        } else if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names)) } {
            unsafe { vm(cx) }.heap.arena_values_base()
        } else { std::ptr::null() };
        let mut stage = if store {
            // A store's gate reads the heap's barrier state (`Stage::store_gate`).
            Concrete::storing(&raw const unsafe { vm(view.memory.bytes.cast::<LaneContext<H>>()) }.heap)
        } else { Concrete::new(property_values) };
        let mut frame = row_frame::<H, OP, ACC>(cx, r, view);
        if store {
            let script: *mut LaneContext<H> = view.memory.bytes.cast();
            // The live record table: a direct object receiver proves it reserved, and any other
            // receiver leaves at the row's family test before it is addressed.
            let records = unsafe { vm(script) }.heap.live_object_records() as u64;
            // A method cache that may name the atom must see the store (`store_admitted`).
            if unsafe { vm(script) }.method_caches_may_name(i.imm()) { store_miss!() }
            if this_store { frame.this = r.this(); }
            let hint = i.field_hint();
            frame.plan = Some(RecordPlan::own_store(hint.receiver, hint.slot, records));
        } else if property {
            // The record's plan: its own-field hint over the object records the lane bound; an
            // unbound table is the miss's to bind.
            // A JavaScript view carries its context (`script_handler!`); an own-field row is
            // installed only once the lane bound the object records (`lane_get_field_own_refresh`).
            let script: *mut LaneContext<H> = view.memory.bytes.cast();
            debug_assert_eq!(script, cx, "a JavaScript view carries its context");
            if const { super::inherited_site(FLAG) } {
                // A read through the record's inherited hint (`IcOp::Held` over the layout its proof
                // last answered); at the other site, of any receiver's object part.
                let machine = unsafe { vm(script) };
                let others = const { super::inherited_other_site(FLAG) };
                let Some((guard, watch, value)) = machine.inherited_hint_operands(i.call_link.get(), i.imm(), others) else { property_miss!() };
                if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) { frame.this = r.this(); }
                let other = if others { Some(&raw const machine.heap) } else { None };
                frame.plan = Some(RecordPlan::held(guard, watch, value, machine.heap.live_object_records() as u64, other));
            } else if const { super::cell_own_site(FLAG) } {
                // A receiver outside the object records (a function) by its layout id; a direct
                // object, should the site meet one, through the bound records.
                let machine = unsafe { vm(script) };
                // The live record table: a direct object receiver proves it reserved.
                let records = machine.heap.live_object_records() as u64;
                if const { super::own_site_this(FLAG) } { frame.this = r.this(); }
                let hint = i.field_hint();
                // A fixed form is the hint layout's own (the decision chose the row by it).
                let form = const { super::own_site_form(FLAG) };
                frame.plan = Some(if form == HINTED_FORM {
                    RecordPlan::own_other(hint.receiver, hint.slot, records, &raw const machine.heap)
                } else {
                    RecordPlan::own_other_formed(hint.receiver, form & !crate::staged::TAGGED_SLOTS, hint.slot, records, &raw const machine.heap)
                });
            } else if const { FLAG == super::STRING_SITE } {
                // A string receiver's read through the record's `String.prototype` hint.
                let machine = unsafe { vm(script) };
                let Some((watch, value)) = machine.string_hint_operands(i.call_link.get(), i.imm()) else { property_miss!() };
                frame.plan = Some(RecordPlan::held_string(watch, value));
            } else if const { super::held_site(FLAG) } {
                // A read through the record's prototype hint (`IcOp::Held`): the hint's operands,
                // where the lane bound its records and the hint is the read's; else the hint's own
                // read of any receiver, which refreshes it.
                let machine = unsafe { vm(script) };
                let Some((guard, watch, value)) = machine.holder_hint_operands(i.call_link.get(), i.imm()) else { property_miss!() };
                if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) { frame.this = r.this(); }
                // A direct object's record is addressed in the table after its family test; at the
                // held-other site, any other receiver's object part is resolved out of line.
                let other = if const { super::held_other_site(FLAG) } { Some(&raw const machine.heap) } else { None };
                frame.plan = Some(RecordPlan::held(guard, watch, value, machine.heap.live_object_records() as u64, other));
            } else {
            let records = unsafe { (*script).binding.object_base } as u64;
            if const { super::own_site_form(FLAG) == HINTED_FORM } {
                // The form of no particular hint is the row a site starts on, before the lane bound
                // its object records: its first miss binds them.
                if records == 0 {
                    if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) {
                        sibling!(lane_get_field_own_refresh::<H, true, false>, r, ip, Carried::script_in(script, view))
                    }
                    sibling!(lane_get_field_own_refresh::<H, false, false>, r, ip, Carried::script_in(script, view))
                }
            }
            debug_assert_ne!(records, 0, "an own-field row runs over bound object records");
            if (const { super::own_site_this(FLAG) } || (const { super::runtime_base(FLAG) } && i.b == crate::bytecode::FieldBase::THIS.0)) { frame.this = r.this(); }
            let hint = i.field_hint();
            let form = const { super::own_site_form(FLAG) };
            let fixed = (form != HINTED_FORM).then_some(form & !crate::staged::TAGGED_SLOTS);
            // A fixed form is the hint's layout's own (`own_formed_handler` chose the row by it),
            // stated so the read takes it as a constant, not from the layout table.
            if let Some(form) = fixed { unsafe { std::hint::assert_unchecked(crate::staged::ic::layout_form(hint.receiver) == form) } }
            frame.plan = Some(RecordPlan::own(hint.receiver, hint.slot, records));
            }
        }
        match crate::staged::step(&mut stage, &mut frame, operation) {
            Step::Value(_) if store => {
                let script: *mut LaneContext<H> = view.memory.bytes.cast();
                next!(script, r, following(ip), Carried::script_in(script, view))
            }
            Step::Unresolved if store => store_miss!(),
            Step::Value(value) if property => {
                // The register takes the read's value, a slot word decoded where the form may hold a raw double.
                let value = if const { super::own_site_decodes(FLAG) } { crate::value_vec::decode_staged_word(value) } else { Value::from_bits(value) };
                r.set(i.a, value);
                let script: *mut LaneContext<H> = view.memory.bytes.cast();
                next!(script, r, following(ip), Carried::script_in(script, view))
            }
            Step::Unresolved if property => property_miss!(),
            Step::Value(value) => {
                // A names row answers its value without defining its register (its call-out defines it):
                // the register is written here, as every producer writes it beside the carried value.
                if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Names))
                    || (matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element))
                        && matches!(LaneFields::<OP, FLAG>::OPCODE.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister)) } {
                    r.set(i.a, Value::from_bits(value));
                }
                if const { row_produces(LaneFields::<OP, FLAG>::OPCODE) } {
                    next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
                }
                // No record reads a non-producer's accumulator (`derive_lane_view`).
                next!(cx, r, following(ip), view)
            }
            // An element read past its in-place arm: the record's keyed paths (a slot, a getter).
            Step::Unresolved if const { matches!(LaneFields::<OP, FLAG>::OPCODE.staged(), Some(crate::bytecode::Decoder::Element)) } => {
                let script: *mut LaneContext<H> = view.memory.bytes.cast();
                if const { matches!(LaneFields::<OP, FLAG>::OPCODE.field_layout(crate::bytecode::InstructionField::A), crate::bytecode::FieldLayout::ResultRegister) } {
                    sibling!(super::lane_get_index_slot::<H>, r, ip, Carried::script_in(script, view))
                }
                // A keyed write past its in-place replacement: an append, a hole, a typed array, a key.
                sibling!(super::lane_set_index_slot::<H>, r, ip, Carried::script_in(script, view))
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
/// `cx` is the running lane's context and `r` its register file. Called through
/// `LaneHost::allocate`, a constant function pointer in the inlined frame `staged_allocate`
/// builds, so the call inlines and each handler keeps only its allocation's arm.
#[inline(always)]
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

/// The count of the values the running keeping activation kept and the register holding the
/// first (`ScriptActivation::kept`); `None` in a frame or any other activation.
///
/// # Safety
/// `cx` is the running lane's context.
#[inline(always)]
unsafe fn kept_values<H: Host>(cx: *mut ()) -> Option<(u16, u16)> {
    let cx = cx.cast::<LaneContext<H>>();
    if unsafe { (*cx).registers }.env().is_deleted() {
        return None;
    }
    let script = unsafe { vm(cx) }.lane_stack.activations.last()?.script().filter(|script| script.forwards)?;
    Some((script.argc, script.kept))
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

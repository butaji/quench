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
    /// The constants, local slots and outer captures the staged row's operands name.
    reads: Reads,
}

impl<const ACC: u8> LaneFrame<ACC> {
    /// A frame for an operation that reads no `this`.
    #[inline(always)]
    pub(in crate::vm::dispatch_fast) fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, this: Value::DELETED, locals: None, host: None, captures: None, decisions: None,
            capture_slots: None, reads: Reads::NONE }
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
            capture_slots: None, reads: Reads::NONE }
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
    fn vm_step(&mut self, stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, Boxing)> {
        let CallOut::Decide { left, right, operator } = operation else { return None };
        let Decisions { context, decide, .. } = self.decisions?;
        let (left, right) = (Value::from_bits(self.read(stage, left)), Value::from_bits(self.read(stage, right)));
        decide(context, operator, left, right).map(|holds| (Value::boolean(holds).to_bits(), Boxing::Value))
    }

    #[inline(always)]
    fn capture_slot(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, root: bool) -> Option<(u64, Boxing)> {
        let own = self.capture_slots.filter(|_| depth == 0 && !root);
        // SAFETY: null or the running function's dense storage, found this run
        // (`LaneContext::capture`); the scoped read retains the bounds and marker checks.
        let value = own.and_then(|slots| unsafe { crate::heap::EnvironmentSlots::read_storage(slots, slot) })
            .or_else(|| self.reads.capture(depth, slot, root))?;
        Some((value.to_bits(), Boxing::Value))
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
            if Self::REGISTER_B { std::hint::assert_unchecked(self.0.b <= crate::bytecode::REGISTER_MASK) }
            if Self::REGISTER_C { std::hint::assert_unchecked(self.0.c <= crate::bytecode::REGISTER_MASK) }
        }
        // A flag the compiler writes is 0 or 1; any other word reads as set, so the flag's decode
        // is the one test.
        let imm = match (Self::FLAG_ROLE, FLAG) {
            (true, FLAG_CLEAR) => 0,
            (true, FLAG_SET) => 1,
            (true, _) => self.0.imm().min(1),
            (false, _) => self.0.imm(),
        };
        WideInstruction::new(self.op(), self.0.a, self.0.b, self.0.c, imm)
    }
}

/// `lane_row`'s `FLAG`: a row whose immediate is a boolean flag (`ImmediateRole::BooleanFlag`) reads it
/// from the record until its first run, which requickens the record to the instantiation of its flag
/// (`FLAG_CLEAR`, `FLAG_SET`), so the flag is a static operand the row folds (an increment and a
/// decrement are two handlers of one row). Any other row ignores it.
pub(in crate::vm::dispatch_fast) const FLAG_UNREAD: u8 = 0;
pub(in crate::vm::dispatch_fast) const FLAG_CLEAR: u8 = 1;
pub(in crate::vm::dispatch_fast) const FLAG_SET: u8 = 2;

/// Whether opcode `op`'s lane handler is its staged row (`lane_row`): a row that falls through,
/// of the flow family or one the catalog marks `ROW_HANDLER`. Derived from the catalog, so a row
/// that joins gets its handler with no line written here.
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
    if !locals {
        return LaneFrame::<ACC>::new(r, view);
    }
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame() }];
    let (slots, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, slots, len)
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
        // The row family's decoder, a constant of the instantiation, so only its arm is compiled.
        let fields = LaneFields::<OP, FLAG>(i);
        let decoded = match const { LaneFields::<OP, FLAG>::OPCODE.staged() } {
            Some(crate::bytecode::Decoder::Flow) => Operation::decode_flow(fields, crate::staged::DecodeContext::EMPTY),
            Some(crate::bytecode::Decoder::Local) => Operation::decode_local(crate::staged::Fields::wide(fields)),
            Some(crate::bytecode::Decoder::Value) => Operation::decode_value(crate::staged::Fields::wide(fields)),
            _ => None,
        };
        let Some(operation) = decoded else { sibling!(lane_general_step::<H>, r, ip, view) };
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = row_frame::<H, OP, ACC>(cx, r, view);
        match crate::staged::step(&mut stage, &mut frame, operation) {
            Step::Value(value) => next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view }),
            Step::Control(_) | Step::Unresolved => sibling!(lane_general_step::<H>, r, ip, view),
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

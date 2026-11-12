//! The interpreter's register frame: `staged` moves and constants over the live file.

use super::*;
use super::lane_reads::{RecordOperand, Reads};
use crate::bytecode::{Operand, Register};
use crate::staged::{CallOut, Comparison, Concrete, Constant, Edge, Frame, NumberOperation, NumericFrame, Operation, Step};

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
struct LaneFrame<const ACC: u8> {
    registers: Registers,
    view: Carried,
    /// The activation's `this` (`LaneContext::this`): `Value::DELETED` where the lane does
    /// not hold it.
    this: Value,
    /// The activation's local slots and their count, for a staged local operation.
    locals: Option<(*mut Value, usize)>,
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
    fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, this: Value::DELETED, locals: None, captures: None, decisions: None, capture_slots: None,
            reads: Reads::NONE }
    }

    /// A frame that may read and write the activation's local slots.
    fn with_locals(registers: Registers, view: Carried, this: Value,
        locals: *mut Value, len: usize) -> Self {
        Self { registers, view, this, locals: Some((locals, len)), captures: None, decisions: None, capture_slots: None,
            reads: Reads::NONE }
    }
}

impl<const ACC: u8> Frame<Concrete> for LaneFrame<ACC> {
    type Fact = ();
    type Control = Edge;

    #[inline(always)]
    fn fact(&self, _register: Register) -> Self::Fact {}

    #[inline(always)]
    fn read(&mut self, _stage: &mut Concrete, register: Register) -> u64 {
        read::<ACC, ACC_B>(self.registers, register, self.view).to_bits()
    }

    #[inline(always)]
    fn write(&mut self, _stage: &mut Concrete, register: Register, value: u64, _fact: Self::Fact) {
        self.registers.set(register, Value::from_bits(value));
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
        (constant.value.to_bits(), ())
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
    fn boolean(&mut self, _stage: &mut Concrete, condition: bool) -> (u64, ()) {
        (Value::boolean(condition).to_bits(), ())
    }

    #[inline(always)]
    fn load_local(&mut self, _stage: &mut Concrete, slot: u16, _miss: Option<()>)
        -> Option<(u64, ())> {
        let (locals, len) = self.locals?;
        (usize::from(slot) < len).then(|| {
            // SAFETY: `slot` is inside the activation's live local slots.
            (unsafe { *locals.add(usize::from(slot)) }.to_bits(), ())
        })
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
    fn vm_step(&mut self, stage: &mut Concrete, operation: CallOut, _result: Option<Register>) -> Option<(u64, ())> {
        let CallOut::Decide { left, right, operator } = operation else { return None };
        let Decisions { context, decide } = self.decisions?;
        let (left, right) = (Value::from_bits(self.read(stage, left)), Value::from_bits(self.read(stage, right)));
        decide(context, operator, left, right).map(|holds| (Value::boolean(holds).to_bits(), ()))
    }

    #[inline(always)]
    fn capture_slot(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, root: bool) -> Option<(u64, ())> {
        let own = self.capture_slots.filter(|_| depth == 0 && !root);
        // SAFETY: null or the running function's dense storage, found this run
        // (`LaneContext::capture`); the scoped read retains the bounds and marker checks.
        let value = own.and_then(|slots| unsafe { crate::heap::EnvironmentSlots::read_storage(slots, slot) })
            .or_else(|| self.reads.capture(depth, slot, root))?;
        Some((value.to_bits(), ()))
    }

    #[inline(always)]
    fn store_capture(&mut self, _stage: &mut Concrete, depth: u16, slot: u16, value: u64) -> Option<()> {
        let CaptureStores { context, store } = self.captures?;
        store(context, depth, slot, Value::from_bits(value)).then_some(())
    }
}

impl<const ACC: u8> crate::staged::GlobalFrame<Concrete> for LaneFrame<ACC> {}

impl<const ACC: u8> crate::staged::PropertyFrame<Concrete> for LaneFrame<ACC> {
    type Plans = crate::staged::NoPlans;

    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }

    /// The live object record table, which an `instanceof` walk addresses.
    fn object_records(&mut self, _stage: &mut Concrete) -> Option<u64> {
        let Decisions { context, records, .. } = self.decisions?;
        records(context)
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

    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        let word = self.operand_word(stage, operand).expect("a staged numeric operand is one the frame reads");
        u64::from(Value::from_bits(word).as_int().expect("a proved int operand") as u32)
    }

    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        let word = self.operand_word(stage, operand).expect("a staged numeric operand is one the frame reads");
        Value::from_bits(word).as_number().expect("a proved Number operand").to_bits()
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

    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }

    fn miss(&self) {}
    fn int_fact(&self) {}
    fn number_fact(&self) {}
}

/// The edge a staged jump test takes, or `None` where the value's heap cell decides it.
#[inline(always)]
pub(super) fn staged_jump_if<const TAKEN: bool>(r: Registers, view: Carried, register: u16) -> Option<Edge> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::JumpIf { register, taken: TAKEN }) {
        Step::Control(edge) => Some(edge),
        Step::Unresolved => None,
        Step::Value(_) => unreachable!("a jump produces no value"),
    }
}

/// Whether `register` is coercible to an object (neither `null` nor `undefined`), by the
/// staged RequireObjectCoercible.
#[inline(always)]
pub(super) fn staged_require_object_coercible(r: Registers, view: Carried, register: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::RequireObjectCoercible { register }) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a coercibility test transfers no control"),
    }
}

/// ToNumeric of a Number register `source` into `destination`, by the staged operation;
/// false, writing nothing, where `source` is not a Number.
#[inline(always)]
pub(super) fn staged_to_numeric(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::Number(NumberOperation::ToNumeric { destination, source })) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a conversion transfers no control"),
    }
}

/// ToPropertyKey of the direct string in `source` into `destination`, by the staged operation;
/// false, writing nothing, where the value is no direct string.
#[inline(always)]
pub(super) fn staged_property_key(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::PropertyKey { destination, source }), Step::Value(_))
}

/// The activation's `this` into `destination` by the staged LoadThis; false, writing
/// nothing, where the lane does not hold it (`this` is `Value::DELETED`).
#[inline(always)]
pub(super) fn staged_load_this(r: Registers, view: Carried, this: Value, destination: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.this = this;
    match crate::staged::step(&mut stage, &mut frame, Operation::LoadThis { destination }) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a load transfers no control"),
    }
}

/// `LoadLocalPlain` of `slot` into `destination` by the staged operation; false, writing
/// nothing, where the activation has no such local slot.
#[inline(always)]
pub(super) fn staged_load_local_plain<H: Host, const ACC: u8>(cx: *mut LaneContext<H>,
    r: Registers, view: Carried, destination: u16, slot: u16) -> bool {
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame }];
    let (locals, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, locals, len);
    matches!(crate::staged::step(&mut stage, &mut frame,
        Operation::LoadLocalPlain { destination, slot }), Step::Value(_))
}

/// `StoreLocalPlain` of `source` to `slot`, and to `result` where the instruction has one,
/// by the staged operation; false, writing nothing, where the activation has no such slot.
#[inline(always)]
pub(super) fn staged_store_local_plain<H: Host, const ACC: u8>(cx: *mut LaneContext<H>,
    r: Registers, view: Carried, source: u16, slot: u16, result: Option<u16>) -> bool {
    let vm = unsafe { vm(cx) };
    let frame = &mut vm.frames[unsafe { (*cx).frame }];
    let (locals, len) = (frame.locals.as_mut_ptr(), frame.locals.len());
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC>::with_locals(r, view, Value::DELETED, locals, len);
    matches!(crate::staged::step(&mut stage, &mut frame,
        Operation::StoreLocalPlain { source, slot, result }), Step::Value(_))
}

/// `StoreCapture` of `source` to the capture at `depth` and `slot` past the running function's
/// dense slots, by the staged operation and the lane's own store (`lane_capture_store`); false,
/// storing nothing, where the binding does not take the direct store.
#[inline(always)]
pub(super) fn staged_store_capture<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    source: u16, (depth, slot): (u16, u16)) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.captures = Some(CaptureStores::of(cx));
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::StoreCapture { source, depth, slot }), Step::Value(_))
}

/// The edge the staged comparison jump of registers `left` and `right` takes, or `None` where the
/// interpreter's own path decides it. The jump's backward poll is `lane_jump`'s, the one copy
/// every jump takes through `jump_on_edge!`.
#[inline(always)]
pub(super) fn staged_compare_jump<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    comparison: Comparison, [left, right]: [u16; 2]) -> Option<Edge> {
    staged_comparison_jump::<H>(cx, r, view, comparison, [RecordOperand::Register(left), RecordOperand::Register(right)])
}

/// The comparison `operator` makes of the observed `values`: two Numbers decide a relation as
/// binary64 (the static arm the handler selects), and any other relation is no Number relation,
/// which the handler's cold path decides.
#[inline(always)]
fn observed(operator: u32, [left, right]: [Value; 2]) -> Comparison {
    match Comparison::of_operator(operator).by_vm() {
        Comparison::Relation { .. } if left.as_number().is_none() || right.as_number().is_none() => Comparison::Operator(operator),
        comparison => comparison,
    }
}

/// The edge the staged jump of `operator` over the record's `operands` takes: the comparison the
/// observed operands select (`observed`), or `None` where the interpreter's cold path decides it.
#[inline(always)]
pub(super) fn staged_record_jump<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried, operator: u32,
    operands: [RecordOperand; 2]) -> Option<Edge> {
    let values = [operands[0].value(cx, r)?, operands[1].value(cx, r)?];
    staged_comparison_jump::<H>(cx, r, view, observed(operator, values), operands)
}

/// The edge the staged jump of `comparison` over the record's `operands` takes, or `None` where
/// the interpreter's own path decides it.
#[inline(always)]
pub(super) fn staged_comparison_jump<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried, comparison: Comparison,
    operands: [RecordOperand; 2]) -> Option<Edge> {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.decisions = Some(Decisions::of(cx));
    let ([left, right], reads) = RecordOperand::staged_pair::<H>(cx, operands);
    frame.reads = reads;
    let operation = Operation::CompareJump { comparison, left, right, negate: true };
    match crate::staged::step(&mut stage, &mut frame, operation) {
        Step::Control(edge) => Some(edge),
        Step::Unresolved => None,
        Step::Value(_) => unreachable!("a jump produces no value"),
    }
}

/// `LoadCapture` of the capture at `depth` and `slot` into `destination` (of the root's projected
/// binding where `root`), by the staged operation: the running function's own slots, else the lane's
/// remembered walks; false, writing nothing, where they hold no such value.
#[inline(always)]
pub(super) fn staged_load_capture<H: Host>(cx: *mut LaneContext<H>, r: Registers, view: Carried,
    destination: u16, (depth, slot): (u16, u16), root: bool) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    frame.capture_slots = Some(capture_reads(unsafe { (*cx).capture }));
    frame.reads = Reads::of_captures::<H>(cx);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::LoadCapture { destination, depth, slot, root }), Step::Value(_))
}

/// The edge a staged unconditional jump takes.
#[inline(always)]
pub(super) fn staged_jump(r: Registers, view: Carried) -> Edge {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::Jump) {
        Step::Control(edge) => edge,
        _ => unreachable!("a jump produces no value"),
    }
}

lane_handler! {
    pub(super) fn lane_move<ACC: u8>(cx, r, ip, i, view) {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = LaneFrame::<ACC>::new(r, view);
        let Step::Value(value) = crate::staged::step(&mut stage, &mut frame, Operation::Move {
            destination: i.a, source: i.b }) else { unreachable!("a move produces a value") };
        next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
    }
}

lane_handler! {
    pub(super) fn lane_load_const<>(cx, r, ip, i, view) {
        let mut stage = Concrete::new(std::ptr::null());
        let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
        let Step::Value(value) = crate::staged::step(&mut stage, &mut frame, Operation::LoadConst {
            destination: i.a, constant: Constant::untyped(i.constant()) })
            else { unreachable!("a constant produces a value") };
        next!(cx, r, following(ip), Carried { acc: Slot::of(Value::from_bits(value)), ..view })
    }
}

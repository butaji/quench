//! The interpreter's register frame: `staged` moves and constants over the live file.

use super::*;
use crate::bytecode::{Operand, Register};
use crate::bytecode::RegisterWindow;
use crate::staged::{Allocation, Concrete, Constant, Edge, Frame, NumberOperation, NumericFrame, Operation, Step};

/// The maker of a frame's heap values: allocates in the lane context it is given and runs no
/// guest code; `None` leaves the instruction to the general path.
type AllocationMaker = unsafe fn(*mut (), Registers, Allocation) -> Option<Value>;

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
    /// The lane context (type-erased, so the frame is one type for every host) and its maker,
    /// for a staged allocation row.
    allocator: Option<(*mut (), AllocationMaker)>,
}

impl<const ACC: u8> LaneFrame<ACC> {
    /// A frame for an operation that reads no `this`.
    #[inline(always)]
    fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, this: Value::DELETED, locals: None, allocator: None }
    }

    /// A frame whose allocation rows make their heap values in the lane context `cx`.
    fn allocating<H: Host>(registers: Registers, view: Carried, cx: *mut LaneContext<H>) -> Self {
        let make: AllocationMaker = lane_allocation::<H>;
        Self { allocator: Some((cx.cast(), make)), ..Self::new(registers, view) }
    }

    /// A frame that may read and write the activation's local slots.
    fn with_locals(registers: Registers, view: Carried, this: Value,
        locals: *mut Value, len: usize) -> Self {
        Self { registers, view, this, locals: Some((locals, len)), allocator: None }
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
    fn allocate_value(&mut self, _stage: &mut Concrete, destination: Register, allocation: Allocation, _proof: ()) -> Option<u64> {
        let (cx, make) = self.allocator?;
        // SAFETY: the maker was made for the lane context this frame runs in.
        let value = unsafe { make(cx, self.registers, allocation) }?;
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
}

impl<const ACC: u8> crate::staged::GlobalFrame<Concrete> for LaneFrame<ACC> {}

impl<const ACC: u8> NumericFrame<Concrete> for LaneFrame<ACC> {
    fn int_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        let register = operand.register_index().expect("a staged numeric operand is a register");
        u64::from(Value::from_bits(self.read(stage, register)).as_int().expect("a proved int operand") as u32)
    }

    fn number_operand(&mut self, stage: &mut Concrete, operand: Operand, _miss: ()) -> u64 {
        let register = operand.register_index().expect("a staged numeric operand is a register");
        Value::from_bits(self.read(stage, register)).as_number().expect("a proved Number operand").to_bits()
    }

    fn prove_number(&mut self, stage: &mut Concrete, register: Register, _miss: ()) -> Option<()> {
        Value::from_bits(self.read(stage, register)).as_number().map(|_| ())
    }

    fn int_result(&mut self, _stage: &mut Concrete, word: u64) -> u64 {
        Value::integer(word as u32 as i32).to_bits()
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

/// The `typeof` name index of `source` into `destination` by the staged operation; false,
/// writing nothing, where a heap value's cell decides the name.
#[inline(always)]
pub(super) fn staged_typeof_index(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    match crate::staged::step(&mut stage, &mut frame, Operation::TypeofIndex { destination, source }) {
        Step::Value(_) => true,
        Step::Unresolved => false,
        Step::Control(_) => unreachable!("a typeof transfers no control"),
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

/// The activation's `this` into `destination` by the staged LoadThis; false, writing
/// nothing, where the lane does not hold it (`this` is `Value::DELETED`).
#[inline(always)]
pub(super) fn staged_load_this(r: Registers, view: Carried, this: Value, destination: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE> { registers: r, view, this, locals: None, allocator: None };
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

/// The value `allocation` makes in the lane context `cx` (a `*mut LaneContext<H>`): it
/// allocates and runs no guest code, so a frame's own run makes it as the general loop does;
/// `None` where the general path must.
///
/// # Safety
/// `cx` is the running lane's context and `r` its register file.
unsafe fn lane_allocation<H: Host>(cx: *mut (), r: Registers, allocation: Allocation) -> Option<Value> {
    let cx = cx.cast::<LaneContext<H>>();
    match allocation {
        Allocation::ObjectLiteral { site, window } => Some(lane_object_literal_value(cx, r, site, window)),
        Allocation::Closure { function } => lane_closure_value_of(cx, function),
        Allocation::Template { template } => lane_template_value(cx, template),
        Allocation::Concat { window } => lane_concat_value(cx, r, window),
    }
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
    let (code, frame) = unsafe { (&*(*cx).code, (*cx).frame) };
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
    let mut frame = LaneFrame::<ACC_NONE>::allocating::<H>(r, Carried::script(cx), cx);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::Allocate { destination, allocation, proof: () }), Step::Value(_))
}

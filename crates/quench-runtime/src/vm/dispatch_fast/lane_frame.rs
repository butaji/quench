//! The interpreter's register frame: `staged` moves and constants over the live file.

use super::*;
use crate::bytecode::{Operand, Register};
use crate::bytecode::RegisterWindow;
use crate::staged::{Allocation, Concrete, Constant, Edge, Frame, GlobalBinding, NumberOperation, NumericFrame, Operation, Step};

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
    /// The lane context and what it makes, for a staged allocation or deletion row.
    host: Option<LaneHost>,
}

impl<const ACC: u8> LaneFrame<ACC> {
    /// A frame for an operation that reads no `this`.
    #[inline(always)]
    fn new(registers: Registers, view: Carried) -> Self {
        Self { registers, view, this: Value::DELETED, locals: None, host: None }
    }

    /// A frame whose allocation and deletion rows make their values in the lane context `cx`.
    fn hosted<H: Host>(registers: Registers, view: Carried, cx: *mut LaneContext<H>) -> Self {
        let host = LaneHost { cx: cx.cast(), allocate: lane_allocation::<H>, delete: lane_delete_value::<H>,
            read_name: lane_name_value::<H>, name: None };
        Self { host: Some(host), ..Self::new(registers, view) }
    }

    /// This frame running a by-name load whose record resolved its name to `binding`.
    fn named(mut self, binding: NameBinding) -> Self {
        if let Some(host) = self.host.as_mut() {
            host.name = Some(binding);
        }
        self
    }

    /// A frame that may read and write the activation's local slots.
    fn with_locals(registers: Registers, view: Carried, this: Value,
        locals: *mut Value, len: usize) -> Self {
        Self { registers, view, this, locals: Some((locals, len)), host: None }
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
}

/// The by-name load's binding as the row asks for it: the global `undefined` is a constant; any
/// other is the lane's own read (`GlobalFrame::global_read`) of what its record resolved.
impl<const ACC: u8> crate::staged::GlobalFrame<Concrete> for LaneFrame<ACC> {
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
    fn global_read(&mut self, _stage: &mut Concrete, destination: Register, atom: u32, _site: u16, _proof: ()) -> Option<()> {
        let host = self.host?;
        let Some(NameBinding::Read(read)) = host.name else { return None };
        // SAFETY: the reader was made for the lane context this frame runs in.
        let value = unsafe { (host.read_name)(host.cx, read, atom) }?;
        self.registers.set(destination, value);
        Some(())
    }
}

impl<const ACC: u8> crate::staged::PropertyFrame<Concrete> for LaneFrame<ACC> {
    type Plans = crate::staged::NoPlans;

    fn plans(&self) -> crate::staged::NoPlans { crate::staged::NoPlans }

    #[inline(always)]
    fn delete_property(&mut self, _stage: &mut Concrete, base: u64, key: u64, _strict: bool, _miss: ()) -> Option<(u64, ())> {
        let host = self.host?;
        // SAFETY: the deleter was made for the lane context this frame runs in.
        let deleted = unsafe { (host.delete)(host.cx, Value::from_bits(base), Value::from_bits(key)) }?;
        Some((deleted.to_bits(), ()))
    }
}

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

/// `typeof source`'s name index into `destination`, by the staged operation; false, writing
/// nothing, where the value is a heap value, whose cell the VM reads.
#[inline(always)]
pub(super) fn staged_typeof(r: Registers, view: Carried, destination: u16, source: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE>::new(r, view);
    matches!(crate::staged::step(&mut stage, &mut frame, Operation::Typeof { destination, source }), Step::Value(_))
}

/// The activation's `this` into `destination` by the staged LoadThis; false, writing
/// nothing, where the lane does not hold it (`this` is `Value::DELETED`).
#[inline(always)]
pub(super) fn staged_load_this(r: Registers, view: Carried, this: Value, destination: u16) -> bool {
    let mut stage = Concrete::new(std::ptr::null());
    let mut frame = LaneFrame::<ACC_NONE> { registers: r, view, this, locals: None, host: None };
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
/// `cx` is the running lane's context.
#[inline(never)]
unsafe fn lane_name_value<H: Host>(cx: *mut (), read: NameRead, atom: u32) -> Option<Value> {
    let vm = unsafe { vm(cx.cast::<LaneContext<H>>()) };
    match read {
        NameRead::Native => {
            let &(_, native) = vm.intrinsic_natives.iter().find(|&&(named, _)| named == atom)?;
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

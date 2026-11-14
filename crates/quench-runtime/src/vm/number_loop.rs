//! Counted JavaScript loops whose bodies are C: one lane record runs the whole loop over
//! unboxed numbers.
//!
//! A rotated loop `T: if !(i OP n) goto X; body; U: i++; goto T` whose straight body only
//! reads and replaces elements of plain arrays, computes numbers and copies registers runs
//! from its update record `U` as a Rust loop (`lane_number_loop`). At entry the loop reads
//! the kinds of the registers it reads before writing (int32, double, array) and infers
//! every other register's kind over the body (`plan`); each register then lives in an
//! unboxed slot for the whole run. Arrays the body never redefines are resolved once to
//! their values and length: an element read is a bound compare, a load and its kind test.
//! The body runs as monomorphic nodes (`nodes`, `ints`, `compile`) under one loop driver
//! (`driver`): values flow in machine registers within a statement, every operator runs
//! the interpreter's own Number semantics on raw values, and nothing is re-canonicalized
//! until it leaves the loop.
//!
//! A step that meets anything else (a hole, an element that is not a Number, a key that is
//! no index below the length, an int32 overflow) leaves before changing anything: every
//! register the loop wrote is boxed back and the general path runs that step's record.
//! The steps change no heap value but Numbers stored over Numbers in place, and allocate
//! nothing, so no collection runs and the resolved values stay valid for the run.

use super::*;
use crate::bytecode::OperandKind;
use oxc_ast::ast::BinaryOperator;

#[path = "number_loop/compile.rs"]
mod compile;
#[path = "number_loop/driver.rs"]
mod driver;
#[path = "number_loop/ints.rs"]
mod ints;
#[path = "number_loop/nodes.rs"]
mod nodes;
#[path = "number_loop/plan.rs"]
mod plan;

/// Slots a run keeps on its stack: a loop naming a register at or past this is no number loop.
const SLOTS: usize = u64::BITS as usize;
/// Arrays a run resolves.
const ARRAYS: usize = 8;
/// Body instructions a loop admits.
const MAX_STEPS: usize = 32;
/// Plans a loop builds for new entry kinds before it keeps its fused handler for good.
const MAX_PLANS: u8 = 3;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Kind {
    Int,
    Double,
    Array,
}

/// An operand as the bytecode names it.
#[derive(Clone, Copy, Debug)]
enum Operand {
    Register(u16),
    Constant(Value),
}

/// One body instruction, before kinds.
#[derive(Clone, Copy, Debug)]
enum Instruction {
    Read { dst: u16, base: u16, key: Operand },
    Binary { dst: u16, operator: BinaryOperator, left: Operand, right: Operand },
    Write { base: u16, key: Operand, value: u16 },
    Move { dst: u16, src: Operand },
}

/// The body planned for one assignment of entry kinds.
struct Plan {
    /// Kinds of the registers read before written, as the plan assumes them.
    entry: Box<[(u16, Kind)]>,
    /// Every register's slot kind (`None`: not used).
    kinds: [Option<Kind>; SLOTS],
    /// The array registers, by the index nodes name them with, and whether a node writes them.
    arrays: Box<[(u16, bool)]>,
    driver: Box<dyn driver::Driver>,
}

/// A loop `lane_number_loop` runs, which the view owns.
pub(super) struct NumberLoop {
    /// The update record (`U`) and the test record (`T`) by their PCs.
    update: u32,
    test: u32,
    counter: (u16, u16),
    decrement: bool,
    operator: u32,
    test_operands: (Operand, Operand),
    body: Box<[(u32, Instruction)]>,
    /// Registers the loop writes, which every run boxes back.
    written: Box<[u16]>,
    /// The handler the record ran before, which a loop whose plans failed keeps.
    fallback: Handler,
    plan: std::cell::UnsafeCell<Option<Plan>>,
    plans: std::cell::Cell<u8>,
}

/// What a run did.
pub(super) enum Ran {
    /// The test failed: the loop continues at its exit.
    Done,
    /// A step met a case the run does not take: the general path runs the record at this PC.
    Leave(u32),
    /// The entry kinds fit no plan.
    Decline,
}

/// A rotated counted loop's records: test `T`, update `U`, the test's operator.
struct Shape {
    test: usize,
    counter: (u16, u16),
    decrement: bool,
    operator: u32,
    test_operands: (Operand, Operand),
}

impl NumberLoop {
    pub(super) fn update(&self) -> u32 {
        self.update
    }

    /// The PC the loop leaves to when its test fails: past the back edge.
    pub(super) fn exit(&self) -> u32 {
        self.update + 2
    }

    pub(super) fn fallback(&self) -> Handler {
        self.fallback
    }

    /// The loops of a body: each rotated counted loop whose update record runs `looped`'s
    /// handler (`update_loop_handler`) and whose body the run admits.
    pub(super) fn find(
        decoded: &[WideInstruction],
        constants: &[Value],
        jump_targets: &InstructionSet,
        handlers: &[*const ()],
        looped: impl Fn(u32, bool, bool) -> Handler,
    ) -> Vec<NumberLoop> {
        (0..decoded.len().saturating_sub(1))
            .filter_map(|update| {
                let shape = shape(decoded, constants, update)?;
                let fallback = looped(shape.operator, shape.decrement, decoded[shape.test].operand_b().register_index() == Some(shape.counter.0));
                // Control enters the body only at its top, from the test.
                let entered = (shape.test + 1..update).any(|pc| jump_targets.contains(pc));
                (handlers[update] == fallback as *const () && !entered).then_some(())?;
                Self::admit(decoded, constants, update, shape, fallback)
            })
            .collect()
    }

    fn admit(decoded: &[WideInstruction], constants: &[Value], update: usize, shape: Shape, fallback: Handler) -> Option<NumberLoop> {
        if update - shape.test - 1 > MAX_STEPS {
            return None;
        }
        let body = (shape.test + 1..update)
            .map(|pc| instruction(decoded[pc], constants).map(|admitted| (pc as u32, admitted)))
            .collect::<Option<Box<[_]>>>()?;
        let mut written: Vec<u16> = body.iter().filter_map(|(_, instruction)| instruction.defined()).chain([shape.counter.0]).collect();
        written.sort_unstable();
        written.dedup();
        let mut used = body.iter().flat_map(|(_, instruction)| instruction.registers()).chain([shape.counter.0, shape.counter.1]);
        if used.any(|register| usize::from(register) >= SLOTS) {
            return None;
        }
        Some(NumberLoop {
            update: update as u32,
            test: shape.test as u32,
            counter: shape.counter,
            decrement: shape.decrement,
            operator: shape.operator,
            test_operands: shape.test_operands,
            body,
            written: written.into_boxed_slice(),
            fallback,
            plan: std::cell::UnsafeCell::new(None),
            plans: std::cell::Cell::new(0),
        })
    }

    /// Registers the loop reads before it writes them, in the order a run starts at `U`:
    /// the update, the test, then the body.
    fn live_in(&self) -> Vec<u16> {
        let mut defined = 0u64;
        let mut live = Vec::new();
        let mut read = |register: u16, defined: u64| {
            if defined & (1 << register) == 0 && !live.contains(&register) {
                live.push(register);
            }
        };
        read(self.counter.1, defined);
        defined |= 1 << self.counter.0;
        for operand in [self.test_operands.0, self.test_operands.1] {
            operand.register().into_iter().for_each(|register| read(register, defined));
        }
        for (_, instruction) in self.body.iter() {
            instruction.reads().for_each(|register| read(register, defined));
            defined |= instruction.defined().map_or(0, |dst| 1 << dst);
        }
        live
    }

    /// The plan for the entry kinds `entry`, or `None` where the body needs a kind they
    /// cannot give (an array register redefined or computed with, a Number used as a base).
    fn plan(&self, entry: Vec<(u16, Kind)>) -> Option<Plan> {
        let kinds = plan::infer(self, &entry)?;
        let built = plan::Builder::new(self, &kinds).build()?;
        let arrays = built.arrays.into_boxed_slice();
        let body = built.statements.into_iter().map(compile::statement).collect();
        let operand = |operand: Operand| match operand {
            Operand::Constant(value) => Some(plan::Tree::Constant(value.as_number()?)),
            Operand::Register(register) => match kinds[usize::from(register)]? {
                Kind::Int => Some(plan::Tree::Int(register)),
                Kind::Double => Some(plan::Tree::Double(register)),
                Kind::Array => None,
            },
        };
        let driver = driver::driver(driver::Parts {
            counter: self.counter,
            int_counter: kinds[usize::from(self.counter.1)]? == Kind::Int,
            decrement: self.decrement,
            operator: self.operator,
            test: (operand(self.test_operands.0)?, operand(self.test_operands.1)?),
            body,
            update: self.update,
            test_pc: self.test,
        });
        Some(Plan { entry: entry.into_boxed_slice(), kinds, arrays, driver })
    }

    /// The plan whose entry kinds `r` holds, built when none does and plans remain.
    fn planned<H: Host>(&self, vm: &Vm<H>, r: Registers) -> Option<&Plan> {
        // SAFETY: the view's thread alone runs its records, and no reference into the plan
        // outlives the run that took it.
        let slot = unsafe { &mut *self.plan.get() };
        let fits = |plan: &Plan| plan.entry.iter().all(|&(register, kind)| kind_of(vm, r.get(register)) == Some(kind));
        if !slot.as_ref().is_some_and(fits) {
            if self.plans.get() >= MAX_PLANS {
                return None;
            }
            self.plans.set(self.plans.get() + 1);
            let entry = self.live_in().into_iter().map(|register| Some((register, kind_of(vm, r.get(register))?))).collect::<Option<_>>()?;
            *slot = self.plan(entry);
        }
        slot.as_ref()
    }

    /// Runs the loop from its update record over `r`, the frame's registers.
    pub(super) fn run<H: Host>(&self, cx: *mut LaneContext<H>, vm: &mut Vm<H>, r: Registers) -> Ran {
        let Some(plan) = self.planned(vm, r) else { return Ran::Decline };
        let mut state = nodes::Ctx { slots: [0; SLOTS], values: [std::ptr::null_mut(); ARRAYS], lengths: [0; ARRAYS], at: 0 };
        if !resolve_arrays(vm, r, plan, &mut state) {
            return Ran::Decline;
        }
        let Some(unset) = unbox(r, plan, &mut state) else { return Ran::Decline };
        // SAFETY: the run's floor cell, which its VM holds for the whole run.
        let floor = unsafe { &*(*cx).floor };
        let outcome = match plan.driver.run(&mut state, floor) {
            driver::Exit::Done => Ran::Done,
            driver::Exit::Leave(at) => Ran::Leave(at),
        };
        self.write_back(r, plan, &state, unset);
        outcome
    }

    /// Boxes every register the loop wrote back into `r`. A slot that started unset
    /// (`unbox`) and still holds its marker was never written: its register keeps its value.
    fn write_back(&self, r: Registers, plan: &Plan, state: &nodes::Ctx, unset: u64) {
        for &register in self.written.iter() {
            let bits = state.slot(register);
            if unset & (1 << register) != 0 && bits == nodes::UNSET {
                continue;
            }
            match plan.kinds[usize::from(register)] {
                Some(Kind::Int) => r.set(register, Value::integer(bits as i32)),
                Some(Kind::Double) => r.set(register, Value::number(f64::from_bits(bits))),
                _ => {}
            }
        }
    }
}

/// The rotated counted loop whose update record is `update`.
fn shape(decoded: &[WideInstruction], constants: &[Value], update: usize) -> Option<Shape> {
    let (increment, back) = (decoded[update], decoded[update + 1]);
    if increment.op().class() != Class::IncDec || back.op().class() != Class::Jump {
        return None;
    }
    let test = back.jump_target() as usize;
    let check = decoded.get(test).filter(|_| test < update)?;
    if check.op().class() != Class::JumpBinaryFalse || check.jump_target() as usize != update + 2 {
        return None;
    }
    Some(Shape {
        test,
        counter: (increment.register_a(), increment.register_b()),
        decrement: increment.boolean_flag()?,
        operator: check.binary_operator_field(),
        test_operands: (operand(check.operand_b(), constants)?, operand(check.operand_c(), constants)?),
    })
}

fn operand(operand: crate::bytecode::Operand, constants: &[Value]) -> Option<Operand> {
    match operand.kind()? {
        OperandKind::Register => Some(Operand::Register(operand.payload())),
        OperandKind::Constant => {
            let value = *constants.get(usize::from(operand.payload()))?;
            value.as_number().map(|_| Operand::Constant(value))
        }
        _ => None,
    }
}

/// A body instruction the run admits.
fn instruction(instruction: WideInstruction, constants: &[Value]) -> Option<Instruction> {
    let operand = |field| operand(field, constants);
    Some(match instruction.op().class() {
        Class::GetIndex => Instruction::Read {
            dst: instruction.register_a(),
            base: operand(instruction.operand_b())?.register()?,
            key: operand(instruction.operand_c())?,
        },
        Class::Binary if !instruction.returns_from_frame() => Instruction::Binary {
            dst: instruction.register_a(),
            operator: crate::vm::operations::numeric_operator(instruction.binary_operator())?,
            left: operand(instruction.operand_b())?,
            right: operand(instruction.operand_c())?,
        },
        Class::SetIndex => Instruction::Write {
            base: instruction.register_b(),
            key: Operand::Register(instruction.register_c()),
            value: instruction.register_a(),
        },
        Class::Move => Instruction::Move { dst: instruction.register_a(), src: Operand::Register(instruction.register_b()) },
        Class::LoadConst => {
            let value = *constants.get(instruction.constant_index())?;
            value.as_number()?;
            Instruction::Move { dst: instruction.register_a(), src: Operand::Constant(value) }
        }
        _ => return None,
    })
}

/// The kind of a register's value at entry, as a slot or an array holds it.
fn kind_of<H: Host>(vm: &Vm<H>, value: Value) -> Option<Kind> {
    if value.as_int().is_some() {
        Some(Kind::Int)
    } else if value.as_number().is_some() {
        Some(Kind::Double)
    } else {
        vm.heap.record_array(value).filter(|array| array.holds_ordinary_elements()).map(|_| Kind::Array)
    }
}

/// Resolves the plan's arrays to their values and lengths: unique and unlogged where a
/// node writes them, and no prototype, so an element store changes nothing else.
fn resolve_arrays<H: Host>(vm: &mut Vm<H>, r: Registers, plan: &Plan, state: &mut nodes::Ctx) -> bool {
    for (index, &(register, writes)) in plan.arrays.iter().enumerate() {
        let array = r.get(register);
        let resolved = if writes {
            vm.heap
                .record_array_store_mut_unlogged(array, Value::UNDEFINED)
                .filter(|object| object.holds_ordinary_elements() && !object.was_used_as_prototype())
                .and_then(|object| Some((object.elements().len(), object.unique_values()?.as_ptr())))
        } else {
            vm.heap
                .record_array(array)
                .filter(|object| object.holds_ordinary_elements())
                .map(|object| (object.elements().len(), object.elements().as_slice().as_ptr().cast_mut()))
        };
        let Some((length, values)) = resolved else { return false };
        (state.lengths[index], state.values[index]) = (length, values);
    }
    true
}

/// Unboxes every slot the plan uses into `state`; `None` where an entry register no longer
/// has its kind. A register the body writes before reading may hold anything (a fused
/// record leaves a dead temporary unwritten): its slot starts unset, marked `UNSET`, and
/// its bit is in the returned mask.
fn unbox(r: Registers, plan: &Plan, state: &mut nodes::Ctx) -> Option<u64> {
    let mut unset = 0u64;
    for (register, kind) in plan.kinds.iter().enumerate() {
        let value = r.get(register as u16);
        let bits = match kind {
            Some(Kind::Int) => value.as_int().map(|int| int as i64 as u64),
            Some(Kind::Double) => value.as_number().map(f64::to_bits),
            _ => continue,
        };
        match bits {
            Some(bits) => state.slots[register] = bits,
            None if plan.entry.iter().any(|&(entry, _)| usize::from(entry) == register) => return None,
            None => {
                state.slots[register] = nodes::UNSET;
                unset |= 1 << register;
            }
        }
    }
    Some(unset)
}

impl Operand {
    fn register(self) -> Option<u16> {
        match self {
            Operand::Register(register) => Some(register),
            Operand::Constant(_) => None,
        }
    }
}

impl Instruction {
    fn defined(self) -> Option<u16> {
        match self {
            Instruction::Read { dst, .. } | Instruction::Binary { dst, .. } | Instruction::Move { dst, .. } => Some(dst),
            Instruction::Write { .. } => None,
        }
    }

    /// The registers the instruction reads as values, in the order it evaluates them: the
    /// operands a temporary may fold into.
    fn operands(self) -> impl Iterator<Item = u16> {
        let operands: [Option<u16>; 2] = match self {
            Instruction::Binary { left, right, .. } => [left.register(), right.register()],
            Instruction::Write { value, .. } => [Some(value), None],
            Instruction::Move { src, .. } => [src.register(), None],
            Instruction::Read { .. } => [None, None],
        };
        operands.into_iter().flatten()
    }

    fn reads(self) -> impl Iterator<Item = u16> {
        let operands: [Option<u16>; 3] = match self {
            Instruction::Read { base, key, .. } => [Some(base), key.register(), None],
            Instruction::Binary { left, right, .. } => [left.register(), right.register(), None],
            Instruction::Write { base, key, value } => [Some(base), key.register(), Some(value)],
            Instruction::Move { src, .. } => [src.register(), None, None],
        };
        operands.into_iter().flatten()
    }

    fn registers(self) -> impl Iterator<Item = u16> {
        self.reads().chain(self.defined())
    }
}

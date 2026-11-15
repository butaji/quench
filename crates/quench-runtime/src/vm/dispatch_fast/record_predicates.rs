//! Predicates on lane view records: whether a record of a function may run in the lane, by its operands and its neighbours.

use super::*;

/// Whether the lane may run `instruction` from its view record: every
/// register it names lies inside the register file and carries no flag.
pub(in crate::vm) fn lane_fields_fit(instruction: WideInstruction, function: &Function) -> bool {
    let registers = function.registers;
    let numeric_local = instruction.writes_numeric_local()
        && instruction.a() & crate::bytecode::RETURN_REGISTER == 0
        && instruction.result_register() < function.locals;
    let fields = [
        (InstructionField::A, instruction.a()),
        (InstructionField::B, instruction.b()),
        (InstructionField::C, instruction.c()),
    ];
    fields.iter().all(|&(field, value)| field_register_fits(instruction, (field, value), (registers, numeric_local)))
        && window_and_pair_fit(instruction, registers)
}

/// Whether the register that `field`, holding `value`, names in `instruction` lies inside the
/// `registers` of its function, as the field's layout reads it. The result of a numeric local's
/// write (`numeric_local`) needs no register of the file.
fn field_register_fits(instruction: WideInstruction, (field, value): (InstructionField, u16), (registers, numeric_local): (u16, bool)) -> bool {
    let op = instruction.op();
    match op.field_layout(field) {
        FieldLayout::ResultRegister if field == InstructionField::A && numeric_local => true,
        // A lane call reads its return flag from the field (`lane_call_fits`).
        FieldLayout::ResultRegister
            if field == InstructionField::A
                && (op.traits().has(crate::bytecode::Traits::FRAME_RETURNABLE)
                    || (op.is_binary() && instruction.returns_from_frame())) =>
        {
            instruction.result_register() < registers
        }
        FieldLayout::FieldBase => {
            value == crate::bytecode::FieldBase::NESTED
                || crate::bytecode::FieldBase(value)
                    .register_index()
                    .is_none_or(|register| register < registers)
        }
        FieldLayout::OptionalRegister | FieldLayout::OptionalReadRegister => value
            .checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS)
            .is_none_or(|register| register < registers),
        FieldLayout::Operand => {
            let operand = crate::bytecode::Operand(value);
            operand.kind() != Some(crate::bytecode::OperandKind::Register) || operand.payload() < registers
        }
        layout => !layout.is_register_field() || value < registers,
    }
}

/// Whether the window of a register-window instruction and the register pair of its immediate
/// lie inside the `registers` of its function.
fn window_and_pair_fit(instruction: WideInstruction, registers: u16) -> bool {
    let op = instruction.op();
    let window_fits = op.field_layout(InstructionField::B) != FieldLayout::RegisterWindowBase
        || u32::from(instruction.b()) + u32::from(instruction.c()) <= u32::from(registers);
    let pair_fits = op.immediate_layout() != ImmediateLayout::RegisterPair || {
        let (first, second) = ImmediateLayout::register_pair(instruction.imm());
        first < registers && second < registers
    };
    window_fits && pair_fits
}

/// The record operand of `instruction` at `pc`, or `None` when its jump or
/// constant cannot be resolved.
pub(super) fn lane_operand(
    instruction: WideInstruction,
    pc: usize,
    constants: &[Value],
) -> Option<LaneOperand> {
    let record = std::mem::size_of::<LaneInstruction>() as isize;
    Some(match instruction.op().immediate_role() {
        ImmediateRole::JumpTarget => {
            let distance = isize::try_from(instruction.imm()).ok()? - isize::try_from(pc).ok()?;
            LaneOperand {
                jump: distance.checked_mul(record)?,
            }
        }
        // A case selector reads its run of constants from the context by index, as a constant
        // array reads its elements' (`lane_make_const_array`).
        ImmediateRole::ConstantIndex
            if instruction.op().is_string_case_index() || instruction.op().class() == Class::MakeConstArray =>
        {
            let start = instruction.imm() as usize;
            constants.get(start..start.checked_add(usize::from(instruction.b()))?)?;
            LaneOperand { imm: instruction.imm() }
        }
        ImmediateRole::ConstantIndex => LaneOperand {
            constant: *constants.get(instruction.imm() as usize)?,
        },
        _ if holds_field_hint(instruction.op()) => LaneOperand {
            field: FieldOperand { imm: instruction.imm(), shape: super::NO_FIELD_RECEIVER },
        },
        _ if instruction.op().makes_script_call() => LaneOperand {
            call: CallOperand { imm: instruction.imm(), memo: NO_READ_MEMO },
        },
        _ => LaneOperand {
            imm: instruction.imm(),
        },
    })
}

/// A `Call` runs in the lane only as the general loop pushes one in place: it evaluates no
/// direct or parameter-scope `eval`, which a window activation's call record runs apart (`lane_eval`). It, `GetField`, `Unary` and `Binary` may return their
/// result from the frame (`lane_returns`, `lane_returning`); a returning record runs no
/// other result flag.
pub(super) fn lane_call_fits(instruction: WideInstruction) -> bool {
    match instruction.op().class() {
        Class::Call | Class::CallArray | Class::CallSpread => {
            lane_returns(instruction)
                && !instruction.direct_eval()
                && !instruction.parameter_eval()
        }
        Class::Construct => {
            lane_returns(instruction)
                && !instruction.is_super_construct()
                && !ImmediateLayout::construct_array_arguments(instruction.imm())
        }
        _ => lane_completion_fits(instruction),
    }
}

/// Whether a record of a class that returns a value (`lane_call_fits`'s other classes) returns
/// from its frame only where the lane may complete it.
fn lane_completion_fits(instruction: WideInstruction) -> bool {
    match instruction.op().class() {
        Class::CallForwardingArguments => lane_returns(instruction),
        Class::GetField => lane_returns(instruction) || lane_sinks_field(instruction),
        Class::Unary | Class::MakeObject2 | Class::SuperConstArrayObject2 | Class::MakeObjectLiteral => {
            lane_returns(instruction)
        }
        Class::Binary if instruction.returns_from_frame() => lane_returns(instruction),
        _ => true,
    }
}

/// The record offset, from a call, of the record after its `Await`, whose live registers show
/// the awaited result dead.
const AFTER_AWAIT: usize = 2;

/// A `Call` record's operand, flagged when only the `Await` right after the call reads its
/// result: the await overwrites the result register, or liveness (`live`) shows it dead after
/// the await. A settling call there makes its promise only where one becomes observable
/// (`ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG`). Liveness is derived only for a call
/// followed by an `Await`, of a function whose suspension derives it anyway
/// (`Vm::suspend_activation`).
pub(super) fn lane_call_awaited_marked<'live>(
    operand: LaneOperand,
    decoded: &[WideInstruction],
    pc: usize,
    live: &dyn Fn() -> Option<&'live std::rc::Rc<crate::compile::liveness::RegisterMasks>>,
) -> LaneOperand {
    let call = decoded[pc];
    let Some(&awaited) = decoded.get(pc + 1) else {
        return operand;
    };
    if !call.op().is_call() || call.result_flags() != 0 || !awaited.op().is_await() {
        return operand;
    }
    let result = call.result_register();
    let feeds = awaited.register_b() == result
        && (awaited.result_register() == result
            || live()
                .and_then(|masks| masks.get(pc + AFTER_AWAIT))
                .is_some_and(|live| crate::compile::liveness::register_dead(live, usize::from(result))));
    if !feeds {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`lane_operand`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_AWAITED_FLAG }
}

/// A `Call` record's operand, flagged when liveness (`live`, the registers live before each
/// pc) shows its result register dead after the call: nothing reads the value the callee
/// returns (`ImmediateLayout::lane_call_result_used`).
pub(super) fn lane_call_result_marked(
    operand: LaneOperand,
    instruction: WideInstruction,
    function: &Function,
    pc: usize,
) -> LaneOperand {
    if !instruction.op().is_call() || !function.register_roots.call_result_dead(pc) {
        return operand;
    }
    // SAFETY: a `Call`'s operand is its immediate (`lane_operand`).
    let imm = unsafe { operand.imm };
    LaneOperand { imm: imm | ImmediateLayout::LANE_CALL_RESULT_UNUSED_FLAG }
}

/// Whether the `Call` at `pc` is an iteration step: no arguments, its result checked by the
/// `RequireIteratorResult` after it, as the compiler emits a for-of or destructuring step,
/// whose result register only the step's own `done` and `value` reads take
/// (`lane_iterator_step`).
pub(super) fn lane_iteration_step(decoded: &[WideInstruction], pc: usize) -> bool {
    let call = decoded[pc];
    call.result_flags() == 0
        && ImmediateLayout::argument_count(call.imm()) == 0
        && decoded.get(pc + 1).is_some_and(|next| {
            next.op().is_require_iterator_result() && next.register_b() == call.result_register()
        })
}

/// The records of a for-of step after its iteration `Call` (`lane_for_of_step`), by offset
/// from it: the `done` read of the step's result, then its test.
pub(super) const FOR_OF_DONE_READ: usize = 2;
pub(super) const FOR_OF_DONE_TEST: usize = 3;

/// How a for-of step's records test its `done` read (`lane_for_of_step`). As `for_of` emits
/// it, a `JumpFalse` into the body jumps over the `Jump` that leaves the loop; the rewrite
/// that threads a jump over a jump makes them one `JumpTrue` that leaves it.
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum ForOfTest {
    /// `JumpFalse` to the `value` read, past a `Jump` out of the loop.
    Emitted,
    /// `JumpTrue` out of the loop, the `value` read after it.
    Threaded,
}

impl ForOfTest {
    /// The offset of the record that leaves the loop: the `Jump` after the test, or the
    /// threaded test itself.
    pub(super) const fn exit(self) -> usize {
        match self {
            Self::Emitted => FOR_OF_DONE_TEST + 1,
            Self::Threaded => FOR_OF_DONE_TEST,
        }
    }

    /// The offset of the `value` read, which follows the record that leaves the loop.
    pub(super) const fn value_read(self) -> usize {
        self.exit() + 1
    }

    /// The records the step spans, its `Call` through its `value` read.
    pub(super) const fn records(self) -> usize {
        self.value_read() + 1
    }
}

/// How the iteration step at `pc` (`lane_iteration_step`) begins a for-of step: after its
/// `RequireIteratorResult`, a plain read of the result into a register, a test of that
/// register (`ForOfTest`), and a second plain read of the result where the body begins.
/// `lane_iterator_step_reads` then makes the reads from the step itself; their atoms it
/// tests as it runs.
pub(super) fn lane_for_of_step(decoded: &[WideInstruction], pc: usize) -> Option<ForOfTest> {
    let result = decoded[pc].result_register();
    let record = |offset: usize| decoded.get(pc + offset).copied();
    let reads_result = |offset: usize| {
        record(offset).is_some_and(|read| read.op().is_get_field() && read.result_flags() == 0 && read.b() == result)
    };
    let done = record(FOR_OF_DONE_READ).filter(|_| reads_result(FOR_OF_DONE_READ))?.result_register();
    let test = record(FOR_OF_DONE_TEST).filter(|test| test.a() == done)?;
    let form = match test.op().class() {
        Class::JumpFalse => ForOfTest::Emitted,
        Class::JumpTrue => ForOfTest::Threaded,
        _ => return None,
    };
    let exit = record(form.exit())?;
    let leaves = exit.op().class().eq(if form == ForOfTest::Emitted { Class::Jump } else { Class::JumpTrue })
        && exit.jump_target() as usize > pc + form.value_read();
    let enters = form == ForOfTest::Threaded || test.jump_target() as usize == pc + form.value_read();
    (leaves && enters && reads_result(form.value_read())).then_some(form)
}

/// The records of a destructuring step after its iteration `Call` (`lane_binding_step`), by
/// offset from it: the `done` read, its move to the pattern's done flag, the threaded test
/// leaving the step, and the `value` read.
pub(super) const BINDING_DONE_READ: usize = 2;
pub(super) const BINDING_DONE_MOVE: usize = 3;
pub(super) const BINDING_DONE_TEST: usize = 4;
pub(super) const BINDING_VALUE_READ: usize = 5;

/// Whether the iteration step at `pc` (`lane_iteration_step`) is a destructuring step as
/// `binding_iterator_step` emits it once its test is threaded: a plain read of the result's
/// `done`, a move of it to another register, a `JumpTrue` of it past the step, and a plain
/// read of the result's `value`. `lane_iterator_step_binding` then makes the reads and the
/// move from the step itself; their atoms it tests as it runs.
pub(super) fn lane_binding_step(decoded: &[WideInstruction], pc: usize) -> bool {
    let result = decoded[pc].result_register();
    let record = |offset: usize| decoded.get(pc + offset).copied();
    let reads_result = |offset: usize| {
        record(offset).is_some_and(|read| read.op().is_get_field() && read.result_flags() == 0 && read.b() == result)
    };
    let (Some(done), Some(flag), Some(test), Some(value)) =
        (record(BINDING_DONE_READ), record(BINDING_DONE_MOVE), record(BINDING_DONE_TEST), record(BINDING_VALUE_READ))
    else {
        return false;
    };
    reads_result(BINDING_DONE_READ)
        && reads_result(BINDING_VALUE_READ)
        && binding_move_and_test(decoded, pc, result, done.result_register(), (flag, test, value))
}

/// The move of the done value (`flag`), the `JumpTrue` past the step (`test`) and the value
/// read (`value`) of a binding step at `pc` whose result is `result` and whose done value is in
/// `done`, in the shape `lane_binding_step` requires.
pub(super) fn binding_move_and_test(decoded: &[WideInstruction], pc: usize, result: Register, done: Register,
    (flag, test, value): (WideInstruction, WideInstruction, WideInstruction)) -> bool {
    binding_done_move(flag, result, done)
        && test.op().class().eq(Class::JumpTrue)
        && test.a() == done
        && test.jump_target() as usize > pc + BINDING_VALUE_READ
        && value.result_register() != done
        && value.result_register() != flag.a()
        && pc + BINDING_VALUE_READ + 1 < decoded.len()
}

/// Whether `flag` moves the done value (`done`) to another register than the step's result or
/// its own source.
pub(super) fn binding_done_move(flag: WideInstruction, result: Register, done: Register) -> bool {
    flag.op().is_move() && flag.result_flags() == 0 && flag.b() == done && flag.a() != done && flag.a() != result
}

/// A nested `GetField` whose only result flag stores its value to a field of `this` too
/// (`lane_get_field_sink`).
pub(super) fn lane_sinks_field(instruction: WideInstruction) -> bool {
    instruction.result_flags() == crate::bytecode::SET_THIS_REGISTER
        && instruction.writes_current_this()
}

/// Whether the instruction's only result flag, if any, returns the result from the frame.
pub(super) fn lane_returns(instruction: WideInstruction) -> bool {
    instruction.result_flags() & !crate::bytecode::RETURN_REGISTER == 0
}

/// The register every compiled linear-memory access of `function` goes
/// through, when there is exactly one: the direct accesses', or in a body with none, its
/// vector accesses' (`WasmMemoryAddress`) when the prologue binds it, as lowering binds a
/// memory32 memory once per call.
pub(in crate::vm) fn memory_register(function: &Function) -> Option<u16> {
    let instructions = || function.code.iter().map(|packed| decoded_instruction(function, *packed));
    let direct = instructions().filter_map(|instruction| match instruction.op() {
        op if MemoryLoad::from_direct_op(op).is_some()
            || MemoryStore::from_direct_op(op).is_some() => Some(instruction.b()),
        op if op.is_wasm_memory_size() => Some(instruction.b()),
        op if op.is_wasm_memory_fill() => Some(instruction.a()),
        _ => None,
    });
    single_register(direct).or_else(|| {
        let vector = single_register(
            instructions().filter(|instruction| instruction.op().is_wasm_memory_address()).map(|instruction| instruction.c()),
        )?;
        instructions()
            .take_while(|instruction| {
                matches!(instruction.op().class(), Class::WasmFillRegisters | Class::WasmInstanceBinding)
            })
            .any(|instruction| instruction.op().is_wasm_instance_binding() && instruction.a() == vector)
            .then_some(vector)
    })
}

/// The one register `registers` names, when they name exactly one.
pub(super) fn single_register(mut registers: impl Iterator<Item = u16>) -> Option<u16> {
    let first = registers.next()?;
    registers.all(|register| register == first).then_some(first)
}

/// Whether a lane run of `instruction` could write `register`.
pub(super) fn lane_writes(instruction: WideInstruction, register: u16) -> bool {
    let op = instruction.op();
    let result = matches!(
        op.field_layout(InstructionField::A),
        FieldLayout::ResultRegister | FieldLayout::WriteRegister | FieldLayout::ReadWriteRegister
    ) && instruction.a() == register;
    let optional = op.field_layout(InstructionField::B) == FieldLayout::OptionalRegister
        && instruction
            .b()
            .checked_sub(crate::bytecode::OPTIONAL_REGISTER_BIAS)
            == Some(register);
    let filled = op.is_wasm_fill_registers()
        && (instruction.b()..instruction.b().saturating_add(instruction.c())).contains(&register);
    result || optional || filled
}

/// The shortest straight-line lane run the general loop enters. Entering
/// builds the lane context and memory view, which one or two lane
/// instructions do not repay: on V8-v7 fixed work, three keeps every
/// JavaScript benchmark within one percent of the general path or better
/// (one and two cost DeltaBlue three percent), and leaves Wasm loops,
/// whose runs are long, unchanged.
pub(super) const LANE_ENTRY_MIN_RUN: usize = 3;

/// `LANE_ENTRY_MIN_RUN` in a function with no loop the lane runs whole: each call the
/// general loop makes into it enters the lane anew, so a run must repay a fresh lane
/// context.
pub(super) const LANE_ENTRY_MIN_CALL_RUN: usize = 8;

/// Whether records may read the accumulator. Without sibling calls every
/// handler returns to the lane loop, which carries no accumulator.
pub(super) const ACCUMULATOR: bool = cfg!(quench_lane_tail_calls);

/// The handler of a returning record of `kind` (`returning_kind`) in a view that has no leaf form.
pub(super) fn returning_handler<H: Host>(kind: u8) -> Handler {
    match kind {
        RETURNING_FIELD => lane_returning_field_first::<H, NOT_LEAF>,
        RETURNING_UNARY => lane_returning::<H, RETURNING_UNARY, NOT_LEAF>,
        RETURNING_BINARY => lane_returning::<H, RETURNING_BINARY, NOT_LEAF>,
        RETURNING_OBJECT => lane_returning::<H, RETURNING_OBJECT, NOT_LEAF>,
        RETURNING_OBJECT_LITERAL => lane_returning::<H, RETURNING_OBJECT_LITERAL, NOT_LEAF>,
        _ => lane_returning::<H, RETURNING_LITERAL, NOT_LEAF>,
    }
}

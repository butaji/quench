//! The lane's Number arithmetic (`+`, `-`, `*`, `/` of Numbers): the representation the
//! operands' proved facts choose for it. Operands proved int32 stay int32 as C keeps them, an
//! integer result an index is made of is computed in 64-bit words (`index_math`), any other
//! operand takes binary64, and an operand only a tag types is taken for an int32 and tested.

#[allow(unused_imports)]
use crate::ext_traits::*;
use super::*;
use super::super::operations::NumberArithmetic;
use quench_runtime::staged::number_machine;
use quench_runtime::staged::{Domain, NegativeZero, NumberOperation, Operation, Overflow};
use quench_runtime::vm::global_facts::GlobalRead;

/// What a Number arithmetic step reads and writes: its result register and its operands.
type Operands = (Register, [Operand; 2]);

impl Lowering<'_, '_> {
    /// The arithmetic `instruction` at `pc` runs, into its result register; the exit it leaves at
    /// is `exit`. `None` where an operand has no proof the lowering can make.
    pub(crate) fn arithmetic(&mut self, state: &mut State, pc: usize, instruction: WideInstruction,
        operation: NumberArithmetic, exit: ir::Block) -> Option<()> {
        if self.fold_static(state, pc, instruction, Some(operation)) { return Some(()); }
        let operands = (instruction.result_register(), [instruction.operand_b(), instruction.operand_c()]);
        let words = if operation == NumberArithmetic::Divide { None } else { self.index_math(state, pc, instruction) };
        if let Some(range) = words {
            self.index_arithmetic(state, operands, (operation, range), exit)?;
        } else if operation == NumberArithmetic::Divide || self.float_operand(state, operands.1) {
            self.arithmetic_step(state, (operands, operation), (Domain::Float64, Overflow::Exit, NegativeZero::Exit), exit);
        } else if self.proved_int(state, operands.1) {
            self.int32_arithmetic(state, pc, operands, (operation, exit));
        } else if operation.int32_projection_is_wrapping() && self.truncated(pc, operands.0) {
            self.truncated_sum(state, pc, operands, (operation, exit))?;
        } else {
            self.checked_arithmetic(state, pc, operands, (operation, exit))?;
        }
        Some(())
    }

    /// The one fold point of the lane's static results. Number arithmetic (`operation` is `Some`): operands whose values are
    /// exact (a constant, or a register whose fact or whole-program solve pins it to one integer) fold the operation to its
    /// value, which the interpreter's own operator computes (`static_number_binary`). An identity the operands make exact
    /// defines the destination as its other operand (`copy`): `x * 1`, `x / 1` and `x - 0` for any Number, and `x + 0`
    /// only for an integer `x`, since `-0 + 0` is `+0`. A call (`operation` is `None`) of a Math function whose arguments
    /// are exact folds to the value the runtime's own function returns (`fold_math_call`). Returns whether it defined the
    /// destination; nothing else is emitted.
    pub(crate) fn fold_static(&mut self, state: &mut State, pc: usize, instruction: WideInstruction, operation: Option<NumberArithmetic>) -> bool {
        if self.left_here { return false; }
        let Some(operation) = operation else { return self.fold_math_call(state, pc, instruction) };
        let destination = instruction.result_register();
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        let known = [self.known_number(state, pc, left), self.known_number(state, pc, right)];
        if let [Some(left_value), Some(right_value)] = known
            && let Some(quench_runtime::bytecode::Constant::Number(value)) =
                quench_runtime::vm::operations::static_number_binary(instruction.binary_operator(), left_value, right_value)
        {
            self.fold_constant(state, destination, Value::number(value));
            return true;
        }
        let is = |index: usize, number: f64| known[index] == Some(number);
        let other = match operation {
            NumberArithmetic::Multiply if is(0, 1.0) => right,
            NumberArithmetic::Multiply if is(1, 1.0) => left,
            NumberArithmetic::Divide if is(1, 1.0) => left,
            NumberArithmetic::Subtract if is(1, 0.0) => left,
            NumberArithmetic::Add if is(1, 0.0) && self.is_integer(state, pc, left) => left,
            NumberArithmetic::Add if is(0, 0.0) && self.is_integer(state, pc, right) => right,
            _ => return false,
        };
        // The identity's other operand must be a register the lane already proves a Number.
        let Some(register) = other.register_index().filter(|_| other.kind() == Some(OperandKind::Register)) else { return false };
        if !state.values[usize::from(register)].value.ty.is_subtype(Type::NUMBER) { return false; }
        self.copy(state, destination, register);
        true
    }

    /// The Number a constant operand holds, or the one integer a register is proved to hold before the unit instruction at
    /// `pc` (`integer_range` pins it to a single value). `None` where the operand is not exactly known.
    fn known_number(&self, state: &State, pc: usize, operand: Operand) -> Option<f64> {
        match operand.kind()? {
            OperandKind::Constant => super::super::facts::constant_number(&self.program.constants, operand),
            OperandKind::Register => {
                let range = self.integer_range(state, pc, operand.register_index()?)?;
                (range.min == range.max).then_some(range.min as f64)
            }
            _ => None,
        }
    }

    /// The Math call at `pc` of exact Number arguments (`fold_static`), folded to the value `concrete_math_call` returns: the
    /// function the runtime runs for the native. The callee is the native its by-name read named (`GlobalRead::NativeMember`
    /// or `NativeSlot`, as `intrinsic_calls` finds it), and the fold guards its identity and holds its holder, so the fold
    /// leaves where the callee is another function. Returns whether it defined the destination.
    fn fold_math_call(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> bool {
        if !quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::Call)
            || instruction.returns_from_frame() || instruction.direct_eval() || instruction.parameter_eval()
            || instruction.result_flags() != 0 || !self.holds_holders() { return false; }
        let callee = instruction.register_b();
        let Some(defined) = (0..pc).rev().find(|&at| self.unit.instructions[at].written_registers().any(|written| written == callee)) else { return false };
        let (value, native, holder) = match self.global_reads.get(&defined) {
            Some(GlobalRead::NativeMember { value, native, holder, .. } | GlobalRead::NativeSlot { value, native, holder, .. }) => (*value, *native, *holder),
            _ => return false,
        };
        let Some(call) = quench_runtime::vm::number::math_call(native) else { return false };
        let argc = quench_runtime::bytecode::ImmediateLayout::argument_count(instruction.imm());
        if argc != call.arity() { return false; }
        let window = quench_runtime::bytecode::ImmediateLayout::call_window_base(instruction.imm());
        let Some(args) = (0..argc).map(|index| self.exact_argument(state, pc, window + index)).collect::<Option<Vec<Value>>>() else { return false };
        let Some(result) = quench_runtime::staged::call_native::concrete_math_call(call, &args) else { return false };
        let exit = self.guard_exit_for(state, pc as u32, ExitFact::Callee);
        self.hold_holder_any(state, exit, holder);
        // A builtin the closed world proves is the callee (`callee_is_builtin`) is no compare of its identity.
        if !self.callee_is_builtin(pc, callee) {
            let held = self.tagged(state, callee);
            let same = self.is_identity(held, value);
            emit_js_guard(&mut self.builder, same, exit);
        }
        self.fold_constant(state, instruction.result_register(), result);
        true
    }

    /// The exact value of the argument in `register` before the unit instruction at `pc`: the one integer its range pins
    /// (`integer_range`), or the constant the one load of the same block writes (`LoadConst`). `None` otherwise.
    fn exact_argument(&self, state: &State, pc: usize, register: Register) -> Option<Value> {
        if let Some(range) = self.integer_range(state, pc, register).filter(|range| range.min == range.max) {
            return Some(Value::number(range.min as f64));
        }
        let at = (0..pc).rev().find(|&at| self.unit.instructions[at].written_registers().any(|written| written == register))?;
        if ((at + 1)..pc).any(|later| self.blocks.get(later).is_some_and(|block| block.is_some())) { return None; }
        let instruction = self.unit.instructions[at];
        if !quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::LoadConst) { return None; }
        super::super::facts::immediate(self.program.constants.get(instruction.constant_index())?)
    }

    /// Defines `destination` as the folded `value`: its constant word, with the fact of its one value. An int32 carries
    /// its singleton range, so a later fold reads it as exact (`integer_range`).
    fn fold_constant(&mut self, state: &mut State, destination: Register, value: Value) {
        let ty = match value.as_int() {
            Some(int) => Type::int(IntRange { min: i64::from(int), max: i64::from(int) }),
            None => Type::immediate(value),
        };
        let constant = self.identity_constant(value);
        self.define(state, destination, constant, RegisterFact::definition(ty, Representation::Tagged));
    }

    /// Whether `operand` is a register proved to hold an integer before the unit instruction at `pc`, or an integer
    /// constant: an integer holds no negative zero.
    fn is_integer(&self, state: &State, pc: usize, operand: Operand) -> bool {
        match operand.kind() {
            Some(OperandKind::Register) => operand.register_index().is_some_and(|register| self.integer_range(state, pc, register).is_some()),
            Some(OperandKind::Constant) => super::super::facts::constant_number(&self.program.constants, operand)
                .is_some_and(|number| number.fract() == 0.0 && number.is_finite()),
            _ => false,
        }
    }

    /// The integer range `register` is proved to hold before the unit instruction at `pc`: its current fact, narrowed by
    /// the whole-program solve (`cell_before`) where both know it. `None` where neither proves an integer.
    fn integer_range(&self, state: &State, pc: usize, register: Register) -> Option<IntRange> {
        let fact = state.values[usize::from(register)].value.ty.int_range();
        let solved = self.cell_before(pc, register).range();
        fact.zip(solved).and_then(|(fact, solved)| fact.intersect(solved)).or(fact).or(solved)
    }

    /// Whether an operand is held as a binary64 or a wide integer, or is a constant that is no
    /// int32: the operation then runs in binary64, or is a loop's own (`left_here`).
    fn float_operand(&self, state: &State, operands: [Operand; 2]) -> bool {
        operands.iter().any(|operand| match operand.kind() {
            Some(OperandKind::Register) => matches!(state.values[usize::from(operand.payload())].value.representation,
                Representation::Float64 | Representation::Int64),
            Some(OperandKind::Constant) => self.program.constants.get(usize::from(operand.payload()))
                .and_then(super::super::facts::immediate).is_some_and(|value| !super::super::facts::is_int32(value)),
            _ => false,
        }) || self.left_here
    }

    /// Operands proved int32: the int32 result, which leaves where it overflows. A result the
    /// unit's ranges keep inside int32, or a sum or difference only `| 0` reads (its ToInt32), is
    /// the wrapping int32 result: no overflow exit.
    fn int32_arithmetic(&mut self, state: &mut State, pc: usize, operands: Operands,
        (operation, exit): (NumberArithmetic, ir::Block)) {
        let truncated = match operation {
            NumberArithmetic::Add | NumberArithmetic::Subtract => self.truncated_through_sums(pc, operands.0),
            NumberArithmetic::Multiply if self.truncated_through_sums(pc, operands.0) || self.truncated_through_int32_sum(state, pc, operands.0) => {
                if !self.small_int32_factor(operands.1) && !self.ranges.overflow_free(pc) {
                    self.truncated_product(state, self.ranges.product_exact(pc), operands, exit);
                    return;
                }
                true
            }
            _ => false,
        };
        let wrapping = self.ranges.overflow_free(pc) || truncated;
        let overflow = if wrapping { Overflow::Wrap } else { Overflow::Exit };
        // ToInt32 reads -0 as 0, the wrapping product's own zero.
        let zero = if truncated || self.ranges.ignores_negative_zero(pc) { NegativeZero::Ignore } else { NegativeZero::Exit };
        self.arithmetic_step(state, (operands, operation), (Domain::Int32, overflow, zero), exit);
    }

    /// A product of int32 operands only whose ToInt32 is read (`truncated_through_sums`), neither
    /// factor known small: the exact 64-bit product, whose low 32 bits are that ToInt32 while the
    /// product's magnitude is under 2^52 (its double is then exact, and so are the chain's sums
    /// of int32 constants). A larger product rounds as a double, and leaves. Where the unit's
    /// ranges prove the exact product within that bound (`product_exact`), no test is made.
    fn truncated_product(&mut self, state: &mut State, exact_proven: bool, (destination, [left, right]): Operands, exit: ir::Block) {
        let left = self.operand_int(state, left, exit).expect("proved int32 operand");
        let right = self.operand_int(state, right, exit).expect("proved int32 operand");
        let product = number_machine::int32_product(&mut emit(&mut self.builder), left, right);
        if !exact_proven {
            let exact = number_machine::product_exact(&mut emit(&mut self.builder), product);
            emit_js_guard(&mut self.builder, exact, exit);
        }
        let low = number_machine::product_low_int32(&mut emit(&mut self.builder), product);
        self.set_int(state, destination, low);
    }

    /// Whether a product of int32 `operands` is exact in binary64: one is an int32 constant of
    /// magnitude at most 2^21, so the product's magnitude is under 2^52 (`truncated_through_sums`).
    fn small_int32_factor(&self, operands: [Operand; 2]) -> bool {
        operands.iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
            && self.program.constants.get(usize::from(operand.payload())).and_then(super::super::facts::immediate)
                .and_then(|value| value.as_int()).is_some_and(|factor| factor.unsigned_abs() <= 1 << 21))
    }

    /// The staged Number arithmetic of `operation` over `operands`, in a domain with its overflow
    /// and negative-zero policies; the operands' guards and the step's leave at `miss`. The step's
    /// word, where it produced one.
    fn arithmetic_step(&mut self, state: &mut State, ((destination, [left, right]), operation): (Operands, NumberArithmetic),
        (domain, overflow, zero): (Domain, Overflow, NegativeZero), miss: ir::Block) -> Option<ir::Value> {
        let operation = Operation::Number(NumberOperation::Arithmetic { destination, left, right, operation, domain, overflow, zero });
        match self.step(state, frame::Edges::ValueGuarded { miss }, operation) {
            quench_runtime::staged::Step::Value(word) => Some(word),
            _ => None,
        }
    }

    /// A sum or difference only its ToInt32 is read of: int32 operands wrap (the staged row), any
    /// other numbers take ToInt32 of their binary64 result. Each arm defines the destination as an
    /// int32, so the join keeps it.
    fn truncated_sum(&mut self, state: &mut State, pc: usize, operands: Operands,
        (operation, exit): (NumberArithmetic, ir::Block)) -> Option<()> {
        let (destination, [left, right]) = operands;
        let float = super::cold_block(&mut self.builder);
        let mut float_state = state.clone();
        let join = self.builder.create_block();
        self.arithmetic_step(state, (operands, operation), (Domain::Int32, Overflow::Wrap, NegativeZero::Ignore), float)?;
        self.goto(join, &[]);
        self.builder.switch_to_block(float);
        let left_number = self.operand_number(&mut float_state, left, exit)?;
        let right_number = self.operand_number(&mut float_state, right, exit)?;
        let number = number_machine::float_arithmetic(&mut emit(&mut self.builder), operation, left_number, right_number);
        let truncated = self.number_to_int32(number, exit);
        self.set_int(&mut float_state, destination, truncated);
        self.goto(join, &[]);
        self.builder.switch_to_block(join);
        self.join_states(state, &float_state, pc);
        Some(())
    }

    /// Operands a tag only types: the staged int32 row where both are int32, which widens an
    /// overflow or a product's negative zero to binary64 itself; else the binary64 arm. Each arm
    /// defines the destination as a Number, so the join keeps it.
    fn checked_arithmetic(&mut self, state: &mut State, pc: usize, operands: Operands,
        (operation, exit): (NumberArithmetic, ir::Block)) -> Option<()> {
        let (destination, [left, right]) = operands;
        let float = super::cold_block(&mut self.builder);
        let mut float_state = state.clone();
        let join = self.builder.create_block();
        self.arithmetic_step(state, (operands, operation), (Domain::Int32, Overflow::Widen, NegativeZero::Exit), float)?;
        self.goto(join, &[]);
        self.builder.switch_to_block(float);
        let left_number = self.operand_number(&mut float_state, left, exit)?;
        let right_number = self.operand_number(&mut float_state, right, exit)?;
        let result = number_machine::float_arithmetic(&mut emit(&mut self.builder), operation, left_number, right_number);
        self.set_number(&mut float_state, destination, result);
        self.goto(join, &[]);
        self.builder.switch_to_block(join);
        self.join_states(state, &float_state, pc);
        Some(())
    }
}

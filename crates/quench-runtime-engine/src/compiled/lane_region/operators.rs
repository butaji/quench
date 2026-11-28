use super::*;

impl Lowering<'_, '_> {
    /// A relation's predicate: `instanceof` by the prototype chain (`instance_of`),
    /// (in)equality of operands not both Numbers (`equality`), every other relation of Numbers.
    fn relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        let numbers = [left, right].iter().all(|operand| super::facts::operand_type(
            &self.program.constants, *operand, |register| state.values[usize::from(register)].value.ty)
            .is_subtype(Type::NUMBER));
        // A frameless Number region has no VM helper capability (`LaneHelpers::Numeric`).
        let helpers = !self.helpers.is_numeric();
        if operator == oxc_ast::ast::BinaryOperator::Instanceof as u32 {
            return helpers.then(|| self.instance_of(state, pc, left, right)).flatten();
        }
        let typed = |operand: Operand| super::facts::operand_type(&self.program.constants, operand,
            |register| state.values[usize::from(register)].value.ty).is_subtype(Type::NUMBER);
        match super::operations::equality_operator(operator) {
            Some((true, negated)) if !numbers && (typed(left) || typed(right)) && self.operand_supported(left) && self.operand_supported(right) => {
                let (number, other) = if typed(left) { (left, right) } else { (right, left) };
                let equal = self.number_strict_equality(state, pc, number, other)?;
                Some(if negated { self.raw_binary_imm(Binary::Xor, equal, 1) } else { equal })
            }
            Some((strict, negated)) if !numbers && helpers && self.operand_supported(left) && self.operand_supported(right) => {
                let equal = self.equality(state, pc, left, right, strict)?;
                Some(if negated { self.raw_binary_imm(Binary::Xor, equal, 1) } else { equal })
            }
            // A relation of operands not both Numbers (strings, `in`): the interpreter's own
            // rule (`lane_decision`) through the pure `Decide` helper; where only the full step
            // decides (a coercion that may run guest code), the region leaves at the test.
            None if !numbers && helpers && self.operand_supported(left) && self.operand_supported(right) =>
                self.decided_relation(state, pc, operator, left, right),
            _ => self.number_relation(state, pc, operator, left, right),
        }
    }

    /// A relation the `Decide` helper answers by the interpreter's rule (`lane_decision`): its
    /// boolean, or a leave at the test where only the full step decides.
    pub(super) fn decided_relation(&mut self, state: &mut State, pc: u32, operator: u32, left: Operand, right: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        // A relational operator of operands no fact types (a field's value, a call's result): two
        // int-tagged words compare as int32 in place, as the interpreter's integer path does
        // (`operations::integer_binary`); any other pair is the helper's.
        let Some(relation) = super::operations::NumberRelation::from_operator(operator) else {
            return Some(self.decided_words(left, right, operator, exit));
        };
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.raw_binary(Binary::And, left_int, right_int);
        let (int_block, other, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.branch_if(ints, (int_block, &[]), (other, &[]));
        self.builder.switch_to_block(int_block);
        let (left_bits, right_bits) = (self.raw_unary(Unary::Narrow(Lane::I32), left), self.raw_unary(Unary::Narrow(Lane::I32), right));
        let int_holds = quench_runtime::staged::number_machine::number_relation(&mut super::object_slots::emit(&mut self.builder), relation, left_bits, right_bits, NumberRelationDomain::Int32);
        self.goto(join, &[int_holds]);
        self.builder.switch_to_block(other);
        let decided = self.decided_words(left, right, operator, exit);
        self.goto(join, &[decided]);
        self.builder.switch_to_block(join);
        Some(holds)
    }

    /// The `Decide` helper's Boolean of `left operator right`, leaving at `exit` where only the
    /// full algorithm decides it (`comparison_decision`).
    fn decided_words(&mut self, left: ir::Value, right: ir::Value, operator: u32, exit: ir::Block) -> ir::Value {
        let operator = self.raw_const(Lane::I64, i64::from(operator));
        let holds = self.helper_call(LaneHelper::Decide, &[left, right, operator]);
        let decided = self.raw_compare_imm(Relation::NotEqual, holds, Value::DELETED.to_bits() as i64);
        emit_js_guard(&mut self.builder, decided, exit);
        self.raw_unary(Unary::Narrow(Lane::I8), holds)
    }

    /// `number === other` where `number` is a Number: equal only where `other` is a Number too
    /// and the two compare equal as Numbers; nothing leaves.
    pub(super) fn number_strict_equality(&mut self, state: &mut State, pc: u32, number: Operand, other: Operand) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let number = self.operand_number(state, number, exit)?;
        let bits = self.operand_tagged(state, other)?;
        Some(quench_runtime::staged::number_strict_equal(&mut emit(&mut self.builder), number, bits))
    }

    /// `===` or `==` of two operands: a strict pair of strings the loop holds compares their held
    /// facts (`held_strings_equal`), and any other pair is decided by `equality_words`.
    pub(super) fn equality(&mut self, state: &mut State, pc: u32, left: Operand, right: Operand, strict: bool) -> Option<ir::Value> {
        let Some(pair) = self.held_pair(pc as usize, left, right, strict) else {
            return self.equality_words(state, pc, left, right, strict);
        };
        let bits = [self.operand_tagged(state, left)?, self.operand_tagged(state, right)?];
        Some(self.held_strings_equal(pair, bits))
    }

    /// `Vm::strict_equal` (`strict`) or `==` of two tagged operands, by the staged equality row's
    /// word decision (`staged::equality_decided`). A pair its words leave: loosely, a leave at `pc`;
    /// strictly, two heap values compare by contents where they are strings or legacy cells
    /// (BigInts) of one family (`strict_equal_cells_in_lane`), and are unequal otherwise.
    fn equality_words(&mut self, state: &mut State, pc: u32, left: Operand, right: Operand, strict: bool) -> Option<ir::Value> {
        let exit = self.guard_exit(state, pc);
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let (decided, equal) = quench_runtime::staged::equality_decided(&mut emit(&mut self.builder), [left, right], strict);
        if !strict {
            emit_js_guard(&mut self.builder, decided, exit);
            return Some(equal);
        }
        let [open, cells, compare, join] = std::array::from_fn(|_| self.builder.create_block());
        let joined = self.builder.append_block_param(join, types::I8);
        self.builder.set_cold_block(open);
        self.branch_if(decided, (join, &[equal]), (open, &[]));
        self.builder.switch_to_block(open);
        // The open pair's contents decide it (`content_pair`); two short strings by their records' words, any other the VM's.
        let compared = quench_runtime::staged::string_length::content_pair(&mut emit(&mut self.builder), [left, right]);
        let unequal = self.raw_const(Lane::I8, 0);
        self.branch_if(compared, (cells, &[]), (join, &[unequal]));
        self.builder.switch_to_block(cells);
        let base = emit_js_payload_word(&mut self.builder, self.payload, std::mem::offset_of!(JsStepDispatch, head.string_record_base));
        let same_text = quench_runtime::staged::string_length::short_strings_equal(&mut emit(&mut self.builder), base, [left, right], compare)
            .expect("the emit stage continues past its guards");
        self.goto(join, &[same_text]);
        self.builder.switch_to_block(compare);
        let result = self.helper_call(LaneHelper::StrictEqual, &[left, right]);
        let cells_equal = self.raw_unary(Unary::Narrow(Lane::I8), result);
        self.goto(join, &[cells_equal]);
        self.builder.switch_to_block(join);
        Some(joined)
    }

    fn number_relation(&mut self, state: &mut State, pc: u32, operator: u32,
        left: Operand, right: Operand) -> Option<ir::Value> {
        let relation = super::operations::NumberRelation::from_operator(operator)?;
        if !self.operand_supported(left) || !self.operand_supported(right) || !self.numbers_supported(left, right) {
            return None;
        }
        let exit = self.guard_exit(state, pc);
        if let Some(domain) = self.number_domain(state, left, right) {
            return Some(self.number_relation_step(state, exit, relation, left, right, domain));
        }
        // Operands only their tags type compare as int32 where both tags say so, else as
        // Numbers; neither path proves a fact the other does not.
        let (left, right) = (self.operand_tagged(state, left)?, self.operand_tagged(state, right)?);
        let (left_int, right_int) = (is_int_tagged(&mut self.builder, left), is_int_tagged(&mut self.builder, right));
        let ints = self.raw_binary(Binary::And, left_int, right_int);
        let (int_block, number_block, join) = (self.builder.create_block(), self.builder.create_block(), self.builder.create_block());
        let holds = self.builder.append_block_param(join, types::I8);
        self.branch_if(ints, (int_block, &[]), (number_block, &[]));
        self.builder.switch_to_block(int_block);
        let (left_bits, right_bits) = (self.raw_unary(Unary::Narrow(Lane::I32), left), self.raw_unary(Unary::Narrow(Lane::I32), right));
        let relation = super::operations::NumberRelation::from_operator(operator)?;
        let int_holds = quench_runtime::staged::number_machine::number_relation(&mut super::object_slots::emit(&mut self.builder), relation, left_bits, right_bits, NumberRelationDomain::Int32);
        self.goto(join, &[int_holds]);
        self.builder.switch_to_block(number_block);
        let left = frame::number_word(&mut self.builder, state, left, exit);
        let right = frame::number_word(&mut self.builder, state, right, exit);
        let number_holds = quench_runtime::staged::number_machine::number_relation(&mut super::object_slots::emit(&mut self.builder), relation, left, right, NumberRelationDomain::Float64);
        self.goto(join, &[number_holds]);
        self.builder.switch_to_block(join);
        Some(holds)
    }

    /// A Number relation of `domain`, whose operands its staged step proves.
    fn number_relation_step(&mut self, state: &mut State, miss: ir::Block,
        relation: super::operations::NumberRelation, left: Operand, right: Operand,
        domain: quench_runtime::staged::Domain) -> ir::Value {
        let operation = quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::Relation { left, right, relation, domain });
        match self.step(state, frame::Edges::ValueGuarded { miss }, operation) {
            quench_runtime::staged::Step::Value(value) => value,
            _ => unreachable!("a relation produces a predicate"),
        }
    }

    /// `+source` or `-source` of a Number: the shared Number unary denotation, with the
    /// source's representation. A non-Number source leaves at its exact PC for the general
    /// coercion, and an int32 negation that overflows widens and leaves.
    pub(super) fn unary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        // A returning result completes the activation as a returning binary's does.
        let returns = instruction.returns_from_frame();
        if instruction.writes_current_this() || (instruction.result_flags() != 0 && !returns)
            || (returns && (self.completion.numeric() || !matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))))) {
            return None;
        }
        let operation = crate::compiled::number_unary(instruction, instruction.unary_operator())?;
        let source = instruction.register_b();
        let domain = if state.values[usize::from(source)].value.ty.is_subtype(Type::INT32) {
            quench_runtime::staged::Domain::Int32
        } else {
            quench_runtime::staged::Domain::Float64
        };
        let exit = self.guard_exit(state, pc as u32);
        let operation = quench_runtime::staged::Operation::Number(quench_runtime::staged::NumberOperation::Unary {
            destination: instruction.result_register(),
            source: quench_runtime::bytecode::Operand::register(source),
            operation, domain,
        });
        self.step(state, frame::Edges::ValueGuarded { miss: exit }, operation);
        if returns {
            self.complete_result(state, pc, instruction.result_register());
            return Some(false);
        }
        Some(true)
    }

    pub(super) fn binary(&mut self, state: &mut State, pc: usize, instruction: WideInstruction) -> Option<bool> {
        let returns = instruction.returns_from_frame();
        if instruction.writes_current_this() || (instruction.result_flags() != 0 && !returns) {
            return None;
        }
        // A returning result completes the activation from its own register, at its caller
        // record (`Lowering::complete`); an inlined callee's is the unit's binary, move and
        // jump (`lane_unit`). A jump to the function's dead terminal `Return` instead would
        // leave there: no block starts at it.
        if returns && !matches!(self.unit.origins.get(pc), Some(super::lane_unit::Origin::Caller(_))) {
            return None;
        }
        let destination = instruction.result_register();
        let (left, right) = (instruction.operand_b(), instruction.operand_c());
        if !self.operand_supported(left) || !self.operand_supported(right) {
            return None;
        }
        let operator = instruction.binary_operator();
        let arithmetic = crate::compiled::number_arithmetic(instruction, operator);
        let bitwise = crate::compiled::number_bitwise(instruction, operator);
        // `%` is the shared Number remainder; the opcode's denotation is the catalog's, so the
        // meaning is not a second hand-written `Binary` arm.
        let remainder = quench_runtime::staged::meaning::means(instruction, quench_runtime::staged::meaning::Meaning::NumericBinary)
            && operator == super::operations::REMAINDER;
        // Arithmetic takes Number constants; a relation decides its own (`relation`).
        if (arithmetic.is_some() || bitwise.is_some() || remainder) && [left, right].iter().any(|operand| operand.kind() == Some(OperandKind::Constant)
            && self.program.constants.get(usize::from(operand.payload())).and_then(super::facts::immediate).and_then(Value::as_number).is_none()) {
            return None;
        }
        let exit = self.guard_exit(state, pc as u32);
        if let Some(arithmetic) = arithmetic
            && arithmetic.int32_projection_is_wrapping() && !self.left_here
            && instruction.result_flags() == 0
            && let Some(next) = self.unit.instructions.get(pc + 1).copied()
            && next.result_flags() == 0
            && quench_runtime::staged::meaning::means(next, quench_runtime::staged::meaning::Meaning::Binary)
            && crate::compiled::number_bitwise(next, next.binary_operator())
                == Some(super::operations::NumberBitwise::Or)
            && next.operand_b().register_index() == Some(destination)
            && super::facts::constant_number(&self.program.constants, next.operand_c()) == Some(0.0)
            && self.blocks[pc + 1].is_none()
            && (next.result_register() == destination || !self.is_live(pc + 2, destination))
        {
            // No source edge or observer can see the intermediate Number.
            // A failed input proof resumes before the arithmetic, and the
            // consumer cannot fail once these signed-int32 inputs are proved.
            let left = self.operand_int(state, left, exit)?;
            let right = self.operand_int(state, right, exit)?;
            let bits = quench_runtime::staged::number_machine::wrapping_int32_arithmetic(&mut super::object_slots::emit(&mut self.builder), arithmetic, left, right);
            self.set_int(state, next.result_register(), bits);
            self.consumed.insert(pc + 1);
            return Some(true);
        }
        match (arithmetic, bitwise) {
            _ if remainder => { if !self.remainder(state, pc, destination, [left, right], exit) { return Some(false); } }
            (Some(arithmetic), _) => { self.arithmetic(state, pc, instruction, arithmetic, exit)?; }
            (_, Some(bitwise)) => {
                let left_int = self.operand_to_int32(state, left, exit)?;
                let right_int = self.operand_to_int32(state, right, exit)?;
                let right_constant = super::facts::constant_number(&self.program.constants, right);
                if !bitwise.result_is_int32(right_constant) {
                    let number = quench_runtime::staged::number_machine::number_bitwise(&mut super::object_slots::emit(&mut self.builder), bitwise, left_int, right_int);
                    self.set_number(state, destination, number);
                } else {
                    let result = quench_runtime::staged::number_machine::int32_bitwise(&mut super::object_slots::emit(&mut self.builder), bitwise, left_int, right_int);
                    self.set_int(state, destination, result);
                }
            }
            // A relation's Boolean, from the predicate a branch on it would test.
            _ => {
                let holds = self.relation(state, pc as u32, operator, left, right)?;
                let boolean = self.boolean(holds);
                self.set_boolean(state, destination, boolean);
            }
        }
        if returns && self.completion.numeric() {
            self.return_number(state, destination, pc as u32);
            return Some(false);
        }
        if returns {
            self.complete_result(state, pc, destination);
            self.native += 1;
            return Some(false);
        }
        Some(true)
    }
}

use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn coalesce(&mut self, value: &oxc_ast::ast::LogicalExpression<'_>, debug_name: Option<Atom>) -> Register {
        let left = self.expression_with_debug_name(&value.left, debug_name);
        let result = self.literal(Constant::Undefined);
        self.emit(Op::Move, result, left, 0, 0);

        let null = self.literal(Constant::Null);
        let is_null = self.emit_binary(
            BinaryOperator::StrictEquality as u32,
            Operand::register(left),
            Operand::register(null),
        );
        let check_undefined = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
        let use_right_from_null = self.emit(Op::Jump, 0, 0, 0, 0);

        self.patch_instruction(check_undefined, self.code.len() as u32);
        let undefined = self.literal(Constant::Undefined);
        let is_undefined = self.emit_binary(
            BinaryOperator::StrictEquality as u32,
            Operand::register(left),
            Operand::register(undefined),
        );
        let use_left = self.emit(Op::JumpFalse, is_undefined, 0, 0, 0);

        self.patch_instruction(use_right_from_null, self.code.len() as u32);
        let right = self.expression_with_debug_name(&value.right, debug_name);
        self.emit(Op::Move, result, right, 0, 0);
        self.patch_instruction(use_left, self.code.len() as u32);
        result
    }

    pub(super) fn sequence_expression(
        &mut self,
        sequence: &oxc_ast::ast::SequenceExpression<'_>,
        debug_name: Option<Atom>,
    ) -> Register {
        let mut result = self.literal(Constant::Undefined);
        for (index, expression) in sequence.expressions.iter().enumerate() {
            if index + 1 == sequence.expressions.len() {
                result = self.expression_with_debug_name(expression, debug_name);
                continue;
            }
            // A discarded element's temporaries are dead once it has run, as a statement's
            // are (`release_temporaries`): a minified body's long comma chains otherwise keep
            // every element's registers, past what liveness and frame registers cover
            // (`ROOT_MAP_REGISTERS`).
            let temporaries = self.next_reg;
            self.effect_expression(expression);
            self.next_reg = temporaries;
        }
        result
    }
}

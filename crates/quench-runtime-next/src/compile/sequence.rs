use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn coalesce(&mut self, value: &oxc_ast::ast::LogicalExpression<'_>) -> Register {
        let left = self.expression(&value.left);
        let result = self.literal(Constant::Undefined);
        self.emit(Op::Move, result, left, 0, 0);

        let null = self.literal(Constant::Null);
        let is_null = self.emit_binary(0, Operand::register(left), Operand::register(null));
        let check_undefined = self.emit(Op::JumpFalse, is_null, 0, 0, 0);
        let use_right_from_null = self.emit(Op::Jump, 0, 0, 0, 0);

        self.patch_instruction(check_undefined, self.code.len() as u32);
        let undefined = self.literal(Constant::Undefined);
        let is_undefined =
            self.emit_binary(0, Operand::register(left), Operand::register(undefined));
        let use_left = self.emit(Op::JumpFalse, is_undefined, 0, 0, 0);

        self.patch_instruction(use_right_from_null, self.code.len() as u32);
        let right = self.expression(&value.right);
        self.emit(Op::Move, result, right, 0, 0);
        self.patch_instruction(use_left, self.code.len() as u32);
        result
    }

    pub(super) fn sequence_expression(
        &mut self,
        sequence: &oxc_ast::ast::SequenceExpression<'_>,
    ) -> Register {
        let mut result = self.literal(Constant::Undefined);
        for expression in &sequence.expressions {
            result = self.expression(expression);
        }
        result
    }
}

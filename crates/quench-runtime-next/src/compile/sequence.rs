use super::*;

impl FunctionCompiler<'_, '_> {
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

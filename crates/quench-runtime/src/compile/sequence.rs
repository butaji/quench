use super::*;

impl FunctionCompiler<'_, '_> {
    /// The register a value-joining operator (`a || b`, `a && b`, `a ?? b`) leaves its
    /// result in, given its left operand's register `left`, lowered from register `start` on.
    /// A left operand computed into a temporary of its own (`left >= start`) hands that
    /// temporary to the operator alone (`expression_with_debug_name`), so the result
    /// overwrites it in place and the left value needs no copy. Any other register (a local,
    /// the shared `undefined`, a temporary of an enclosing expression) keeps its value, so
    /// the result takes a copy in a fresh register.
    pub(super) fn join_result(&mut self, left: Register, start: Register) -> Register {
        if left >= start {
            return left;
        }
        let result = self.reg();
        self.emit(Op::Move, result, left, 0, 0);
        result
    }

    pub(super) fn coalesce(
        &mut self,
        value: &oxc_ast::ast::LogicalExpression<'_>,
        debug_name: Option<Atom>,
    ) -> Register {
        let start = self.next_reg;
        let left = self.expression_with_debug_name(&value.left, debug_name);
        let result = self.join_result(left, start);
        let use_right_from_null = self.jump_strictly(left, Constant::Null, true);
        let use_left = self.jump_strictly(left, Constant::Undefined, false);
        self.patch(use_right_from_null);
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

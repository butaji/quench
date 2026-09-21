use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn string_literal(&mut self, value: &oxc_ast::ast::StringLiteral<'_>) -> Register {
        self.literal(Constant::String(value.value.to_string()))
    }

    pub(super) fn bigint_literal(&mut self, value: &oxc_ast::ast::BigIntLiteral<'_>) -> Register {
        self.literal(Constant::BigInt(value.value.to_string()))
    }

    pub(super) fn template_literal(
        &mut self,
        template: &oxc_ast::ast::TemplateLiteral<'_>,
    ) -> Register {
        let Some(first) = template.quasis.first() else {
            return self.literal(Constant::String(String::new()));
        };
        let Some(cooked) = first.value.cooked.as_ref() else {
            self.owner
                .reject(first.span, "invalid template escape sequence");
            return self.literal(Constant::Undefined);
        };
        let mut result = self.literal(Constant::String(cooked.to_string()));
        for (index, expression) in template.expressions.iter().enumerate() {
            let expression_value = self.expression(expression);
            result = self.emit_binary(
                8,
                Operand::register(result),
                Operand::register(expression_value),
            );
            let Some(quasi) = template.quasis.get(index + 1) else {
                break;
            };
            let Some(cooked) = quasi.value.cooked.as_ref() else {
                self.owner
                    .reject(quasi.span, "invalid template escape sequence");
                return self.literal(Constant::Undefined);
            };
            let tail = self.literal(Constant::String(cooked.to_string()));
            result = self.emit_binary(8, Operand::register(result), Operand::register(tail));
        }
        result
    }
}

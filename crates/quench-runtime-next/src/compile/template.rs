use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn tagged_template(
        &mut self,
        value: &oxc_ast::ast::TaggedTemplateExpression<'_>,
    ) -> Register {
        let tag = self.expression(&value.tag);
        let strings = self.reg();
        self.emit(
            Op::MakeArray,
            strings,
            0,
            0,
            value.quasi.quasis.len() as u32,
        );
        for (index, quasi) in value.quasi.quasis.iter().enumerate() {
            let key = self.literal(Constant::Number(index as f64));
            let string = self.literal(super::string::template_constant(&quasi.value));
            self.emit(Op::SetIndex, string, strings, key, 0);
        }
        let this = self.literal(Constant::Undefined);
        let base = self.next_reg;
        let strings_arg = self.reg();
        self.emit(Op::Move, strings_arg, strings, 0, 0);
        for expression in &value.quasi.expressions {
            let argument = self.reg();
            let value = self.expression(expression);
            self.emit(Op::Move, argument, value, 0, 0);
        }
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            tag,
            this,
            (u32::from(base) << 16) | (1 + value.quasi.expressions.len() as u32),
        );
        result
    }

    pub(super) fn string_literal(&mut self, value: &oxc_ast::ast::StringLiteral<'_>) -> Register {
        self.literal(super::string::constant(value))
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
        let Some(_) = first.value.cooked.as_ref() else {
            self.owner
                .reject(first.span, "invalid template escape sequence");
            return self.literal(Constant::Undefined);
        };
        let mut result = self.literal(super::string::template_constant(&first.value));
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
            let Some(_) = quasi.value.cooked.as_ref() else {
                self.owner
                    .reject(quasi.span, "invalid template escape sequence");
                return self.literal(Constant::Undefined);
            };
            let tail = self.literal(super::string::template_constant(&quasi.value));
            result = self.emit_binary(8, Operand::register(result), Operand::register(tail));
        }
        result
    }
}

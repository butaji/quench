use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn tagged_template(
        &mut self,
        value: &oxc_ast::ast::TaggedTemplateExpression<'_>,
    ) -> Register {
        let (tag, this) = self.callee(&value.tag);
        let strings = self.reg();
        let site = self.owner.template_site();
        self.emit(Op::LoadCachedTemplateObject, strings, 0, 0, site);
        let undefined = self.literal(Constant::Undefined);
        let missing = self.emit_binary(
            BinaryOperator::StrictEquality as u32,
            Operand::register(strings),
            Operand::register(undefined),
        );
        let cached_template = self.emit(Op::JumpFalse, missing, 0, 0, 0);
        self.emit(
            Op::MakeArray,
            strings,
            0,
            0,
            value.quasi.quasis.len() as u32,
        );
        let raw_strings = self.reg();
        self.emit(
            Op::MakeArray,
            raw_strings,
            0,
            0,
            value.quasi.quasis.len() as u32,
        );
        for (index, quasi) in value.quasi.quasis.iter().enumerate() {
            let key = self.literal(Constant::Number(index as f64));
            let string = quasi
                .value
                .cooked
                .as_ref()
                .map(|cooked| self.literal(Constant::String(cooked.as_str().into())))
                .unwrap_or_else(|| self.literal(Constant::Undefined));
            self.emit(Op::SetIndex, string, strings, key, 0);
            let raw = self.literal(Constant::String(quasi.value.raw.as_str().into()));
            self.emit(Op::SetIndex, raw, raw_strings, key, 0);
        }
        let descriptor = self.reg();
        self.emit(Op::MakeObject, descriptor, 0, 0, 0);
        self.set_template_descriptor_field(descriptor, "value", raw_strings);
        for field in ["writable", "enumerable", "configurable"] {
            let value = self.literal(Constant::Boolean(false));
            self.set_template_descriptor_field(descriptor, field, value);
        }
        let raw_key = self.literal(Constant::String("raw".into()));
        self.call_template_intrinsic(
            "\0rqj:object-define-property",
            &[strings, raw_key, descriptor],
        );
        self.call_template_intrinsic("\0rqj:object-freeze", &[raw_strings]);
        self.call_template_intrinsic("\0rqj:object-freeze", &[strings]);
        self.emit(Op::CacheTemplateObject, strings, 0, 0, site);
        self.patch(cached_template);
        // Evaluate substitutions before reserving the contiguous call-argument
        // window. Each expression may allocate temporaries; reserving a slot
        // first would let those temporaries occupy the next argument register.
        let substitutions = value
            .quasi
            .expressions
            .iter()
            .map(|expression| self.expression(expression))
            .collect::<Vec<_>>();
        let base = self.next_reg;
        let strings_arg = self.reg();
        self.emit(Op::Move, strings_arg, strings, 0, 0);
        for value in substitutions {
            let argument = self.reg();
            self.emit(Op::Move, argument, value, 0, 0);
        }
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            tag,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(
                base,
                (1 + value.quasi.expressions.len()) as u16,
                false,
                false,
            ),
        );
        result
    }

    fn set_template_descriptor_field(&mut self, descriptor: Register, name: &str, value: Register) {
        let atom = self.owner.atom(name);
        let cache = self.owner.cache_site();
        self.emit(Op::SetField, value, descriptor, cache, atom);
    }

    fn call_template_intrinsic(&mut self, name: &str, arguments: &[Register]) -> Register {
        let callee = self.load_name(name);
        let this = self.literal(Constant::Undefined);
        let base = self.next_reg;
        for argument in arguments {
            let slot = self.reg();
            self.emit(Op::Move, slot, *argument, 0, 0);
        }
        let result = self.reg();
        self.emit(
            Op::Call,
            result,
            callee,
            this,
            crate::bytecode::ImmediateLayout::call_immediate(
                base,
                arguments.len() as u16,
                false,
                false,
            ),
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

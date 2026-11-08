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
            "\0quench:object-define-property",
            &[strings, raw_key, descriptor],
        );
        self.call_template_intrinsic("\0quench:object-freeze", &[raw_strings]);
        self.call_template_intrinsic("\0quench:object-freeze", &[strings]);
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
        self.literal(Constant::BigInt(crate::bigint::decimal_literal(&value.value)))
    }

    pub(super) fn template_literal(
        &mut self,
        template: &oxc_ast::ast::TemplateLiteral<'_>,
    ) -> Register {
        if let Some(invalid) = template.quasis.iter().find(|quasi| quasi.value.cooked.is_none()) {
            self.owner.reject(invalid.span, "invalid template escape sequence");
            return self.literal(Constant::Undefined);
        }
        let Some(last) = template.expressions.len().checked_sub(1) else {
            let text = template.quasis.first().map(|quasi| super::string::template_constant(&quasi.value));
            return self.literal(text.unwrap_or_else(|| Constant::String("".into())));
        };
        // Every part becomes a string before the parts are joined, so an empty text adds
        // nothing (`"" + s` is `s`) and is left out of the window.
        let texts = template.quasis.iter().map(|quasi| &quasi.value).filter(|text| text.cooked.as_ref().is_some_and(|text| !text.is_empty()));
        let parts = texts.count() + template.expressions.len();
        let base = self.next_reg;
        for _ in 0..parts {
            self.reg();
        }
        let mut slot = base;
        for (index, text) in template.quasis.iter().map(|quasi| &quasi.value).enumerate() {
            if text.cooked.as_ref().is_some_and(|text| !text.is_empty()) {
                let constant = self.owner.constant(super::string::template_constant(text));
                self.emit(Op::LoadConst, slot, 0, 0, constant);
                slot += 1;
            }
            let Some(expression) = template.expressions.get(index) else {
                break;
            };
            let value = self.expression(expression);
            // The last substitution is converted by `StringConcat` itself: no evaluation
            // follows it, so nothing can observe when its ToString runs.
            if index == last {
                self.emit(Op::Move, slot, value, 0, 0);
            } else {
                self.emit(Op::Unary, slot, value, 0, crate::bytecode::UNARY_TO_STRING_PRIMITIVE);
            }
            slot += 1;
        }
        debug_assert_eq!(usize::from(slot - base), parts);
        self.emit(Op::StringConcat, base, base, parts as Register, 0);
        base
    }
}

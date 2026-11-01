use super::*;

impl FunctionCompiler<'_, '_> {
    pub(super) fn array_expression(&mut self, value: &ArrayExpression<'_>) -> Register {
        if value
            .elements
            .iter()
            .any(|item| matches!(item, ArrayExpressionElement::SpreadElement(_)))
        {
            return self.array_expression_with_spread(value);
        }
        let dst = self.reg();
        let constants = value
            .elements
            .iter()
            .map(|item| {
                item.as_expression()
                    .and_then(|value| binding_time::expression(value).static_value())
            })
            .collect::<Option<Vec<_>>>();
        if let Some(constants) = constants
            && !constants.is_empty()
            && constants.len() <= u16::MAX as usize
        {
            let start = self.owner.constant_run(constants);
            self.emit(
                Op::MakeConstArray,
                dst,
                value.elements.len() as u16,
                0,
                start,
            );
            return dst;
        }
        self.emit(Op::MakeArray, dst, 0, 0, value.elements.len() as u32);
        let element_temporaries = self.next_reg;
        for (index, item) in value.elements.iter().enumerate() {
            let Some(expr) = item.as_expression() else {
                continue;
            };
            let item = self.expression(expr);
            self.emit(Op::DefineArrayElement, item, dst, 0, index as u32);
            self.next_reg = element_temporaries;
        }
        dst
    }

    fn array_expression_with_spread(&mut self, value: &ArrayExpression<'_>) -> Register {
        let elements = value
            .elements
            .iter()
            .map(|item| match item {
                ArrayExpressionElement::SpreadElement(spread) => {
                    AccumulatedElement::Spread(&spread.argument)
                }
                ArrayExpressionElement::Elision(_) => AccumulatedElement::Hole,
                item => AccumulatedElement::Value(item.as_expression().expect("array element")),
            })
            .collect::<Vec<_>>();
        self.accumulated_array(&elements)
    }

    /// ArrayAccumulation into a fresh array: the elements before the first spread have static
    /// indices; the rest append at the array's length, which tracks the next index.
    pub(super) fn accumulated_array(
        &mut self,
        elements: &[AccumulatedElement<'_, '_>],
    ) -> Register {
        let dst = self.reg();
        let prefix = elements
            .iter()
            .position(|element| matches!(element, AccumulatedElement::Spread(_)))
            .unwrap_or(elements.len());
        self.emit(Op::MakeArray, dst, 0, 0, prefix as u32);
        let element_temporaries = self.next_reg;
        let mut holes = 0u32;
        for (index, element) in elements.iter().enumerate() {
            let (value, spread) = match *element {
                AccumulatedElement::Hole => {
                    holes += u32::from(index >= prefix);
                    continue;
                }
                AccumulatedElement::Value(expression) => (self.expression(expression), false),
                AccumulatedElement::Spread(source) => (self.expression(source), true),
            };
            if index < prefix {
                self.emit(Op::DefineArrayElement, value, dst, 0, index as u32);
            } else {
                self.emit(
                    Op::AppendArray,
                    dst,
                    crate::bytecode::encode_optional_register(Some(value)),
                    u16::from(spread),
                    holes,
                );
                holes = 0;
            }
            self.next_reg = element_temporaries;
        }
        if holes > 0 {
            self.emit(
                Op::AppendArray,
                dst,
                crate::bytecode::encode_optional_register(None),
                u16::from(false),
                holes,
            );
        }
        dst
    }
}

/// One item of ArrayAccumulation, shared by array literals and spread argument lists.
#[derive(Clone, Copy)]
pub(super) enum AccumulatedElement<'e, 'a> {
    Hole,
    Value(&'e Expression<'a>),
    Spread(&'e Expression<'a>),
}

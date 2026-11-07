use super::*;
use smallvec::SmallVec;

/// Elements a literal evaluates into registers before making its array.
const LITERAL_ELEMENT_WINDOW: usize = 8;

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
                    .and_then(binding_time::expression)
            })
            .collect::<Option<Vec<_>>>();
        if let Some(constants) = constants
            && !constants.is_empty()
            && constants.len() <= u16::MAX as usize
        {
            let start = self.owner.constant_run(constants);
            self.emit(Op::MakeConstArray, dst, value.elements.len() as u16, 0, start);
            return dst;
        }
        if self.short_array_literal(dst, value) {
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

    /// A short literal without holes (`literal_element_values`) made into `dst`: its
    /// element definitions follow `MakeArray` back to back. `false` emits nothing.
    fn short_array_literal(&mut self, dst: Register, value: &ArrayExpression<'_>) -> bool {
        let element_temporaries = self.next_reg;
        let Some(values) = self.literal_element_values(value) else {
            return false;
        };
        self.emit(Op::MakeArray, dst, 0, 0, values.len() as u32);
        for (index, item) in values.into_iter().enumerate() {
            self.emit(Op::DefineArrayElement, item, dst, 0, index as u32);
        }
        self.next_reg = element_temporaries;
        true
    }

    /// The registers holding a short literal's elements, every one evaluated before the
    /// array is made, which no code can observe: its definitions then follow `MakeArray`
    /// back to back, which the lane makes as one literal (`specialize_array_literals`).
    /// A value read from a register that a later element could assign is copied first; the
    /// last element's is read as it stands.
    /// `None` for a literal with holes or past the window, which define as they go.
    fn literal_element_values(&mut self, value: &ArrayExpression<'_>) -> Option<SmallVec<[Register; LITERAL_ELEMENT_WINDOW]>> {
        if value.elements.len() > LITERAL_ELEMENT_WINDOW {
            return None;
        }
        let expressions = value.elements.iter().map(ArrayExpressionElement::as_expression).collect::<Option<SmallVec<[_; LITERAL_ELEMENT_WINDOW]>>>()?;
        let mut values = SmallVec::new();
        let last = expressions.len().saturating_sub(1);
        for (index, expression) in expressions.into_iter().enumerate() {
            let start = self.next_reg;
            let mut item = self.expression(expression);
            if item < start && index < last {
                let copy = self.reg();
                self.emit(Op::Move, copy, item, 0, 0);
                item = copy;
            }
            values.push(item);
        }
        Some(values)
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

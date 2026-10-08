use super::Op;

macro_rules! numeric_operator_rows {
    ($callback:ident) => {
        $callback! {
            NumericAdd => (8, add),
            NumericMultiply => (10, multiply),
        }
    };
}

macro_rules! define_selector {
    ($($opcode:ident => ($operator:literal, $semantic:ident),)+) => {
        pub(crate) const fn specialized_numeric_op(operator: u32) -> Option<Op> {
            match operator {
                $($operator => Some(Op::$opcode),)+
                _ => None,
            }
        }
    };
}

numeric_operator_rows!(define_selector);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selector_is_derived_from_operator_rows() {
        assert_eq!(specialized_numeric_op(8), Some(Op::NumericAdd));
        assert_eq!(specialized_numeric_op(10), Some(Op::NumericMultiply));
        assert_eq!(specialized_numeric_op(9), None);
    }
}

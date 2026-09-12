// Shared operator facts consumed by the runtime catalog and the build-time
// physical-key generator. Keep semantic names, compact ids and physical leaf
// families in one data declaration.
macro_rules! with_binary_operator_catalog {
    ($callback:ident) => {
        $callback! {
            Add = 0 => None,
            Subtract = 1 => None,
            Multiply = 2 => None,
            Divide = 3 => None,
            Remainder = 4 => None,
            Exponentiate = 5 => None,
            NumericAdd = 6 => Some("increment"),
            NumericSubtract = 7 => Some("decrement"),
            Equal = 8 => Some("compare_equal"),
            NotEqual = 9 => Some("compare_not_equal"),
            StrictEqual = 10 => Some("compare_equal"),
            StrictNotEqual = 11 => Some("compare_not_equal"),
            LessThan = 12 => Some("compare_less"),
            LessEqual = 13 => Some("compare_less_equal"),
            GreaterThan = 14 => Some("compare_greater"),
            GreaterEqual = 15 => Some("compare_greater_equal"),
            BitwiseOr = 16 => Some("bitwise_or"),
            BitwiseXor = 17 => Some("bitwise_xor"),
            BitwiseAnd = 18 => Some("bitwise_and"),
            ShiftLeft = 19 => Some("shift_left"),
            ShiftRight = 20 => Some("shift_right"),
            ShiftRightZeroFill = 21 => Some("shift_right_zero"),
            Instanceof = 22 => None,
        }
    };
}

#[allow(unused_macros)]
macro_rules! with_unary_operator_catalog {
    ($callback:ident) => {
        $callback! {
            Plus = 0,
            Minus = 1,
            Not = 2,
            BitwiseNot = 3,
            Void = 4,
            Typeof = 5,
            ToString = 6,
            ToNumeric = 7,
            Delete = 8,
            IsNullish = 9,
        }
    };
}

// Canonical operation names that intentionally use a different compact
// physical spelling.  Build-time matrix generation consumes this table so
// aliases cannot drift from the shared opcode catalog.
#[allow(unused_macros)]
macro_rules! with_canonical_physical_alias_catalog {
    ($callback:ident) => {
        $callback! {
            Const => LoadConst,
            Call => Call,
            CallMethod => CallN,
            Loop => ForI,
            LoadBinding => LoadLocalChecked,
            GetProperty => GetN,
            GetPropertyDynamic => AGetI,
            SetProperty => SetN,
            SetPropertyDynamic => ASetI,
        }
    };
}

// Canonical operations whose typed cold marker deliberately differs from the
// primary physical spelling.  The ordinary cold rows are derived from the
// physical catalog; only these semantic aliases need an explicit relation.
#[allow(unused_macros)]
macro_rules! with_canonical_cold_alias_catalog {
    ($callback:ident) => {
        $callback! {
            Call => CallSlow,
            Loop => ForI,
        }
    };
}

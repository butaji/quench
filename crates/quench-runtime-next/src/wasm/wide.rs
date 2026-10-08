use wasmparser::Operator;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u32)]
pub(crate) enum WideArithmeticOperator {
    Add128,
    Sub128,
    MulWideSigned,
    MulWideUnsigned,
}

impl WideArithmeticOperator {
    pub(crate) fn from_wasm(operator: &Operator<'_>) -> Option<Self> {
        Some(match operator {
            Operator::I64Add128 => Self::Add128,
            Operator::I64Sub128 => Self::Sub128,
            Operator::I64MulWideS => Self::MulWideSigned,
            Operator::I64MulWideU => Self::MulWideUnsigned,
            _ => return None,
        })
    }

    pub(crate) fn from_tag(tag: u32) -> Option<Self> {
        Some(match tag {
            0 => Self::Add128,
            1 => Self::Sub128,
            2 => Self::MulWideSigned,
            3 => Self::MulWideUnsigned,
            _ => return None,
        })
    }

    pub(crate) const fn input_count(self) -> u16 {
        match self {
            Self::Add128 | Self::Sub128 => 4,
            Self::MulWideSigned | Self::MulWideUnsigned => 2,
        }
    }

    pub(crate) fn apply(
        self,
        left_low: i64,
        left_high: Option<i64>,
        right_low: i64,
        right_high: Option<i64>,
    ) -> (i64, i64) {
        let result = match self {
            Self::Add128 | Self::Sub128 => {
                let (left, right) =
                    wide_value(left_low, left_high.unwrap(), right_low, right_high.unwrap());
                if self == Self::Add128 {
                    left.wrapping_add(right)
                } else {
                    left.wrapping_sub(right)
                }
            }
            Self::MulWideSigned => (i128::from(left_low) * i128::from(right_low)) as u128,
            Self::MulWideUnsigned => u128::from(left_low as u64) * u128::from(right_low as u64),
        };
        (result as u64 as i64, (result >> 64) as u64 as i64)
    }
}

fn wide_value(low: i64, high: i64, other_low: i64, other_high: i64) -> (u128, u128) {
    (
        (u128::from(high as u64) << 64) | u128::from(low as u64),
        (u128::from(other_high as u64) << 64) | u128::from(other_low as u64),
    )
}

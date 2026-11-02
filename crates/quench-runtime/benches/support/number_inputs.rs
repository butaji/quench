use std::hint::black_box;

pub const BATCH_VALUES: usize = 64;
pub const INPUT_SEED: i32 = 104729;
const FRACTION_DIVISOR: f64 = 31.0;
const UNIT_FRACTION_SCALE: f64 = 2.0;

#[derive(Clone, Copy)]
// The private VM and public Value harnesses use different subsets of these inputs.
#[allow(dead_code)]
pub enum Inputs {
    Integers,
    WideIntegers,
    LargeInt32,
    Fractions,
    UnitFractions,
    Dyadic,
    Mixed,
    Boundaries,
}

impl Inputs {
    pub fn numbers(self) -> [f64; BATCH_VALUES] {
        let seed = black_box(INPUT_SEED);
        let edges = [
            0.0,
            -0.0,
            f64::from(i32::MIN),
            f64::from(i32::MAX),
            f64::from(i32::MIN) - 1.0,
            f64::from(i32::MAX) + 1.0,
            f64::MIN_POSITIVE,
            f64::from_bits(1),
            f64::INFINITY,
            f64::NEG_INFINITY,
            f64::NAN,
        ];
        std::array::from_fn(|index| {
            let integer = f64::from(seed * (index as i32 - BATCH_VALUES as i32 / 2));
            match self {
                Self::Integers => integer,
                Self::WideIntegers => integer * f64::from(u32::MAX),
                Self::LargeInt32 => f64::from(i32::MAX - seed * index as i32),
                Self::Fractions => integer + 1.0 / FRACTION_DIVISOR,
                // An affine dependency chain with factor 1/2 stays below magnitude one.
                Self::UnitFractions => {
                    integer / (f64::from(seed) * BATCH_VALUES as f64 * UNIT_FRACTION_SCALE)
                        + 1.0 / FRACTION_DIVISOR
                }
                Self::Dyadic => integer + 0.5,
                Self::Mixed if index % 2 == 0 => integer,
                Self::Mixed => integer + 1.0 / FRACTION_DIVISOR,
                Self::Boundaries => edges[index % edges.len()],
            }
        })
    }
}

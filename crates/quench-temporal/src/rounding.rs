const ROUNDING_TIE_FACTOR: i128 = 2;

/// Rounds `value` to a multiple of a positive `quantum` using Temporal's
/// rounding modes, returning the rounded quotient.
///
/// Callers validate the quantum as part of option processing before invoking
/// this pure arithmetic operation.
pub fn round_temporal_nanoseconds(value: i128, quantum: i128, mode: &str) -> i128 {
    let quotient = value / quantum;
    let remainder = value % quantum;
    if remainder == 0 {
        return quotient;
    }
    let sign = value.signum();
    let distance = remainder.abs();
    let tie = distance * ROUNDING_TIE_FACTOR == quantum;
    let above_tie = distance * ROUNDING_TIE_FACTOR > quantum;
    let adjust = match mode {
        "trunc" => false,
        "floor" => sign < 0,
        "ceil" => sign > 0,
        "expand" => true,
        "halfTrunc" => above_tie,
        "halfExpand" => above_tie || tie,
        "halfFloor" => above_tie || tie && sign < 0,
        "halfCeil" => above_tie || tie && sign > 0,
        "halfEven" => above_tie || tie && quotient % ROUNDING_TIE_FACTOR != 0,
        _ => false,
    };
    quotient + if adjust { sign } else { 0 }
}

#[cfg(test)]
mod tests {
    use super::round_temporal_nanoseconds;

    #[test]
    fn rounding_modes_define_positive_and_negative_ties() {
        let cases = [
            ("trunc", 0, 0),
            ("floor", 0, -1),
            ("ceil", 1, 0),
            ("expand", 1, -1),
            ("halfTrunc", 0, 0),
            ("halfExpand", 1, -1),
            ("halfFloor", 0, -1),
            ("halfCeil", 1, 0),
            ("halfEven", 0, 0),
        ];

        for (mode, positive, negative) in cases {
            assert_eq!(round_temporal_nanoseconds(5, 10, mode), positive, "{mode}");
            assert_eq!(round_temporal_nanoseconds(-5, 10, mode), negative, "{mode}");
        }
    }

    #[test]
    fn half_even_uses_the_quotient_parity_on_both_sides_of_zero() {
        assert_eq!(round_temporal_nanoseconds(15, 10, "halfEven"), 2);
        assert_eq!(round_temporal_nanoseconds(25, 10, "halfEven"), 2);
        assert_eq!(round_temporal_nanoseconds(-15, 10, "halfEven"), -2);
        assert_eq!(round_temporal_nanoseconds(-25, 10, "halfEven"), -2);
    }
}

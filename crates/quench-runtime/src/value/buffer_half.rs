pub(crate) fn f16_round(value: f64) -> f64 {
    half_to_f64(f64_to_half(value))
}

fn round_half(value: f64) -> u16 {
    let lower = value.floor() as u64;
    let fraction = value - lower as f64;
    let rounded = lower + u64::from(fraction > 0.5 || (fraction == 0.5 && lower & 1 != 0));
    rounded as u16
}

fn sign_factor(sign: u64) -> f64 {
    if sign == 0 {
        1.0
    } else {
        -1.0
    }
}

const SCIENTIFIC_NOTATION_MIN_MAGNITUDE: f64 = 1e-6;
const SCIENTIFIC_NOTATION_MAX_MAGNITUDE: f64 = 1e21;

pub(crate) fn format(value: f64) -> String {
    if value.is_nan() {
        return "NaN".into();
    }
    if value.is_infinite() {
        return if value.is_sign_negative() {
            "-Infinity"
        } else {
            "Infinity"
        }
        .into();
    }
    if value == 0.0 {
        return "0".into();
    }
    let magnitude = value.abs();
    if magnitude >= SCIENTIFIC_NOTATION_MAX_MAGNITUDE
        || magnitude < SCIENTIFIC_NOTATION_MIN_MAGNITUDE
    {
        let scientific = format!("{value:e}");
        let (mantissa, exponent) = scientific.split_once('e').expect("scientific notation");
        let mantissa = mantissa.trim_end_matches('0').trim_end_matches('.');
        let exponent = exponent.parse::<i32>().expect("numeric exponent");
        return format!("{mantissa}e{exponent:+}");
    }
    if value.fract() == 0.0 {
        format!("{value:.0}")
    } else {
        value.to_string()
    }
}

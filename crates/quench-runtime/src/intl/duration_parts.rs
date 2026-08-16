use crate::value::Value;

use super::slot_value;

pub(super) fn append_subsecond_parts(
    parts: &mut Vec<String>,
    slots: &[(String, Value)],
    milliseconds: i64,
    microseconds: i64,
    nanoseconds: i64,
) {
    if slot_value(slots, "microseconds") == Some("numeric")
        && (milliseconds != 0 || microseconds != 0 || nanoseconds != 0)
    {
        let value = format!(
            "{:03}.{:03}{:03}",
            milliseconds.abs(),
            microseconds.abs(),
            nanoseconds.abs()
        );
        let formatted = value.parse::<f64>().map_or(value, |value| {
            crate::intl::number_format::format_number_rounded(value, 9, 1, "trunc")
        });
        parts.push(format!("{formatted} ms"));
    } else if slot_value(slots, "nanoseconds") == Some("numeric") {
        if milliseconds != 0 {
            parts.push(format!("{milliseconds} ms"));
        }
        if microseconds != 0 || nanoseconds != 0 {
            parts.push(format!(
                "{:02}.{:03} μs",
                microseconds.abs(),
                nanoseconds.abs()
            ));
        }
    }
}

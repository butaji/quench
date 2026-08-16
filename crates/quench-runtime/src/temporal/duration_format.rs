use super::duration::{duration_field, duration_receiver};
use crate::{execute::VmError, value::Value};

pub(super) fn to_json(receiver: Option<&Value>) -> Result<Value, VmError> {
    let object = duration_receiver(receiver)?;
    Ok(Value::String(format_iso_duration(object)))
}

pub(super) fn to_string(receiver: Option<&Value>) -> Result<Value, VmError> {
    to_json(receiver)
}

fn format_iso_duration(object: &crate::value::ObjectData) -> String {
    let names = [
        "years",
        "months",
        "weeks",
        "days",
        "hours",
        "minutes",
        "seconds",
        "milliseconds",
        "microseconds",
        "nanoseconds",
    ];
    let fields = names.map(|name| duration_field(object, name));
    let negative = fields.iter().any(|value| *value < 0);
    let fields = fields.map(|value| value.abs());
    let date = format_date_fields(&fields);
    let time = format_time_fields(&fields);
    let body = if date.is_empty() && time.is_empty() {
        "T0S".to_string()
    } else {
        format!("{date}{time}")
    };
    format!("{}P{body}", if negative { "-" } else { "" })
}

fn format_date_fields(fields: &[i128; 10]) -> String {
    ["Y", "M", "W", "D"]
        .iter()
        .enumerate()
        .filter(|(index, _)| fields[*index] != 0)
        .map(|(index, suffix)| format!("{}{}", fields[index], suffix))
        .collect()
}

fn format_time_fields(fields: &[i128; 10]) -> String {
    let mut result = String::new();
    append_time_field(&mut result, fields[4], "H");
    append_time_field(&mut result, fields[5], "M");
    let subseconds = fields[7] * 1_000_000 + fields[8] * 1_000 + fields[9];
    let seconds = fields[6] + subseconds / 1_000_000_000;
    let remainder = subseconds % 1_000_000_000;
    if seconds != 0 || remainder != 0 {
        let fraction = format!("{remainder:09}").trim_end_matches('0').to_string();
        if fraction.is_empty() {
            append_time_field(&mut result, seconds, "S");
        } else {
            result.push_str(&format!("{seconds}.{fraction}S"));
        }
    }
    if result.is_empty() {
        String::new()
    } else {
        format!("T{result}")
    }
}

fn append_time_field(result: &mut String, value: i128, suffix: &str) {
    if value != 0 {
        result.push_str(&format!("{value}{suffix}"));
    }
}

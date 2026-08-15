pub(crate) mod duration;
pub(crate) mod instant;
pub(crate) mod plain_date;
pub(crate) mod plain_date_time;
pub(crate) mod plain_time;

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    if builtin == crate::ops::Builtin::TemporalZonedDateTime {
        return Some(zoned_date_time_construct(arguments));
    }
    duration::execute(builtin, receiver, arguments)
        .or_else(|| instant::execute(builtin, receiver, arguments))
        .or_else(|| plain_date::execute(builtin, receiver, arguments))
        .or_else(|| plain_date_time::execute(builtin, receiver, arguments))
        .or_else(|| plain_time::execute(builtin, receiver, arguments))
}

fn zoned_date_time_construct(
    arguments: &[crate::value::Value],
) -> Result<crate::value::Value, crate::execute::VmError> {
    let epoch = arguments
        .first()
        .cloned()
        .unwrap_or(crate::value::Value::BigInt("0".into()));
    let time_zone = arguments
        .get(1)
        .cloned()
        .unwrap_or(crate::value::Value::String("UTC".into()));
    let calendar = arguments
        .get(2)
        .cloned()
        .unwrap_or(crate::value::Value::String("iso8601".into()));
    Ok(crate::value::Value::Object(std::rc::Rc::new(
        crate::value::ObjectData::new(vec![
            ("epochNanoseconds".into(), epoch),
            ("timeZoneId".into(), time_zone),
            ("calendarId".into(), calendar),
            (
                "\0prototype".into(),
                crate::value::Value::Builtin(crate::ops::Builtin::TemporalZonedDateTimePrototype),
            ),
        ]),
    )))
}

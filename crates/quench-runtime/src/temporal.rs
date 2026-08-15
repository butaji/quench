pub(crate) mod duration;
pub(crate) mod plain_date;
pub(crate) mod plain_date_time;
pub(crate) mod plain_time;

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    duration::execute(builtin, receiver, arguments)
        .or_else(|| plain_date::execute(builtin, receiver, arguments))
        .or_else(|| plain_date_time::execute(builtin, receiver, arguments))
        .or_else(|| plain_time::execute(builtin, receiver, arguments))
}

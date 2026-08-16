pub(crate) mod duration;
mod duration_arithmetic;
mod duration_construct;
mod duration_format;
mod duration_parse;
pub(crate) mod plain_date;

pub(crate) fn execute(
    builtin: crate::ops::Builtin,
    receiver: Option<&crate::value::Value>,
    arguments: &[crate::value::Value],
) -> Option<Result<crate::value::Value, crate::execute::VmError>> {
    duration::execute(builtin, receiver, arguments)
        .or_else(|| plain_date::execute(builtin, receiver, arguments))
}

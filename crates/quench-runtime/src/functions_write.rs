use crate::{
    functions::FunctionMetadata,
    ops::{FunctionKind, Op},
};

pub(crate) fn write_op(registers: &mut Vec<crate::value::Value>, op: &Op) {
    match op {
        Op::MakeFunction {
            dst,
            body,
            params,
            length,
            captures,
            strictness,
            is_async,
            mapped_arguments,
        } => write_ordinary(
            registers,
            (*dst, body, *params, *length, *captures),
            *strictness,
            *is_async,
            *mapped_arguments,
        ),
        Op::MakeFunctionWithKind {
            dst,
            body,
            params,
            length,
            captures,
            kind,
            strictness,
            is_async,
            mapped_arguments,
        } => write_non_ordinary(
            registers,
            (*dst, body, *params, *captures),
            (*kind, *length, *strictness, *is_async, *mapped_arguments),
        ),
        _ => {}
    }
}

fn write_non_ordinary(
    registers: &mut Vec<crate::value::Value>,
    function: (u16, &[Op], u16, u16),
    metadata: (
        crate::ops::FunctionKind,
        u16,
        crate::ops::FunctionStrictness,
        bool,
        bool,
    ),
) {
    let (kind, length, strictness, is_async, mapped_arguments) = metadata;
    write_kind(
        registers,
        function,
        FunctionMetadata {
            kind,
            length,
            strictness,
            is_async,
            mapped_arguments,
        },
    );
}

fn write_kind(
    registers: &mut Vec<crate::value::Value>,
    function: (u16, &[Op], u16, u16),
    metadata: FunctionMetadata,
) {
    let (dst, body, params, captures) = function;
    crate::functions::write(registers, dst, body, params, captures, metadata);
}

fn write_ordinary(
    registers: &mut Vec<crate::value::Value>,
    function: (u16, &[Op], u16, u16, u16),
    strictness: crate::ops::FunctionStrictness,
    is_async: bool,
    mapped_arguments: bool,
) {
    let (dst, body, params, length, captures) = function;
    crate::functions::write(
        registers,
        dst,
        body,
        params,
        captures,
        FunctionMetadata {
            kind: FunctionKind::Ordinary,
            strictness,
            is_async,
            mapped_arguments,
            length,
        },
    );
}

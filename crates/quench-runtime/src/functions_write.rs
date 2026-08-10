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
            captures,
            strictness,
            is_async,
        } => {
            write_ordinary(
                registers,
                (*dst, body, *params, *captures),
                *strictness,
                *is_async,
            );
        }
        Op::MakeFunctionWithKind {
            dst,
            body,
            params,
            captures,
            kind,
            strictness,
            is_async,
        } => write_kind(
            registers,
            (*dst, body, *params, *captures),
            FunctionMetadata {
                kind: *kind,
                strictness: *strictness,
                is_async: *is_async,
            },
        ),
        _ => {}
    }
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
    function: (u16, &[Op], u16, u16),
    strictness: crate::ops::FunctionStrictness,
    is_async: bool,
) {
    let (dst, body, params, captures) = function;
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
        },
    );
}

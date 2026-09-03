use crate::{
    functions::FunctionMetadata,
    ops::{FunctionKind, Op},
};

pub(crate) fn write_op(registers: &mut crate::register_file::RegisterFile, op: &Op) {
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
            source,
        } => {
            write_non_ordinary(
                registers,
                (*dst, body, *params, *captures),
                (*kind, *length, *strictness, *is_async, *mapped_arguments),
            );
            if let Some(source) = source {
                if let Ok(crate::value::Value::Function(function)) =
                    crate::execute::read_register(registers, *dst)
                {
                    function.properties.borrow_mut().push((
                        "\0dynamic_source".into(),
                        crate::value::Value::String(source.clone()),
                    ));
                }
            }
        }
        _ => {}
    }
}

fn write_non_ordinary(
    registers: &mut crate::register_file::RegisterFile,
    function: (u16, &crate::machine::FunctionCode, u16, u16),
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
            direct_constructor: function.1.facts().direct_constructor.clone(),
            composed_constructor: function.1.facts().composed_constructor.clone(),
            strictness,
            is_async,
            mapped_arguments,
        },
    );
}

fn write_kind(
    registers: &mut crate::register_file::RegisterFile,
    function: (u16, &crate::machine::FunctionCode, u16, u16),
    metadata: FunctionMetadata,
) {
    let (dst, body, params, captures) = function;
    crate::functions::write(registers, dst, body, params, captures, metadata);
}

fn write_ordinary(
    registers: &mut crate::register_file::RegisterFile,
    function: (u16, &crate::machine::FunctionCode, u16, u16, u16),
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
            direct_constructor: body.facts().direct_constructor.clone(),
            composed_constructor: body.facts().composed_constructor.clone(),
        },
    );
}

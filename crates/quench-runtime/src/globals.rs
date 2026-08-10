use std::collections::HashMap;

use crate::{
    facts::{Constant as FactConstant, ConstantFact, ProgramDb},
    ops::{Constant, Op},
};

pub(crate) fn reduce(
    name: &str,
    ops: &mut Vec<Op>,
    facts: &mut ProgramDb,
    next_register: &mut u16,
    locals: &HashMap<String, u16>,
) -> Option<u16> {
    if locals.contains_key(name) {
        return None;
    }
    if let Some(builtin) = builtin(name) {
        let register = *next_register;
        *next_register = next_register.saturating_add(1);
        ops.push(Op::MakeBuiltin {
            dst: register,
            builtin,
        });
        return Some(register);
    }
    let (fact, value) = match name {
        "undefined" => (FactConstant::Undefined, Constant::Undefined),
        "NaN" => (FactConstant::Number(f64::NAN), Constant::Number(f64::NAN)),
        "Infinity" => (
            FactConstant::Number(f64::INFINITY),
            Constant::Number(f64::INFINITY),
        ),
        _ => return None,
    };
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    facts.constants.push(ConstantFact { value: fact });
    ops.push(Op::Const {
        dst: register,
        value,
    });
    Some(register)
}

fn builtin(name: &str) -> Option<crate::ops::Builtin> {
    match name {
        "Array" => Some(crate::ops::Builtin::Array),
        "ArrayBuffer" => Some(crate::ops::Builtin::ArrayBuffer),
        "Float64Array" => Some(crate::ops::Builtin::Float64Array),
        "Float32Array" => Some(crate::ops::Builtin::Float32Array),
        "Int8Array" => Some(crate::ops::Builtin::Int8Array),
        "Int16Array" => Some(crate::ops::Builtin::Int16Array),
        "Int32Array" => Some(crate::ops::Builtin::Int32Array),
        "Uint8Array" => Some(crate::ops::Builtin::Uint8Array),
        "Uint32Array" => Some(crate::ops::Builtin::Uint32Array),
        "Uint8ClampedArray" => Some(crate::ops::Builtin::Uint8ClampedArray),
        "DataView" => Some(crate::ops::Builtin::DataView),
        "Boolean" => Some(crate::ops::Builtin::Boolean),
        "eval" => Some(crate::ops::Builtin::Eval),
        "escape" => Some(crate::ops::Builtin::Escape),
        "isFinite" => Some(crate::ops::Builtin::IsFinite),
        "isNaN" => Some(crate::ops::Builtin::IsNaN),
        "Number" => Some(crate::ops::Builtin::Number),
        "Object" => Some(crate::ops::Builtin::Object),
        "parseFloat" => Some(crate::ops::Builtin::ParseFloat),
        "parseInt" => Some(crate::ops::Builtin::ParseInt),
        "String" => Some(crate::ops::Builtin::String),
        "Symbol" => Some(crate::ops::Builtin::Symbol),
        "unescape" => Some(crate::ops::Builtin::Unescape),
        "Math" => Some(crate::ops::Builtin::Math),
        "Function" => Some(crate::ops::Builtin::Function),
        "TypeError" => Some(crate::ops::Builtin::TypeError),
        "Error" => Some(crate::ops::Builtin::Error),
        "RangeError" => Some(crate::ops::Builtin::RangeError),
        "ReferenceError" => Some(crate::ops::Builtin::ReferenceError),
        "SyntaxError" => Some(crate::ops::Builtin::SyntaxError),
        "EvalError" => Some(crate::ops::Builtin::EvalError),
        "URIError" => Some(crate::ops::Builtin::URIError),
        "AggregateError" => Some(crate::ops::Builtin::AggregateError),
        "Date" => Some(crate::ops::Builtin::Date),
        "Promise" => Some(crate::ops::Builtin::Promise),
        "print" => Some(crate::ops::Builtin::Print),
        "Reflect" => Some(crate::ops::Builtin::Reflect),
        "RegExp" => Some(crate::ops::Builtin::RegExp),
        "Intl" => Some(crate::ops::Builtin::Intl),
        _ => None,
    }
}

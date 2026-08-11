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
        return Some(emit_builtin(ops, next_register, builtin));
    }
    let (fact, value) = global_constant(name)?;
    let register = take_register(next_register);
    facts.constants.push(ConstantFact { value: fact });
    ops.push(Op::Const {
        dst: register,
        value,
    });
    Some(register)
}

pub(crate) fn is_defined(name: &str) -> bool {
    name == "$262" || builtin(name).is_some() || global_constant(name).is_some()
}

pub(crate) fn immutable_value(name: &str) -> Option<crate::value::Value> {
    Some(match name {
        "undefined" => crate::value::Value::Undefined,
        "NaN" => crate::value::Value::Number(f64::NAN),
        "Infinity" => crate::value::Value::Number(f64::INFINITY),
        _ => return None,
    })
}

pub(crate) fn script_properties(ops: &mut Vec<Op>, next_register: &mut u16) -> Vec<(String, u16)> {
    let names = [
        "Object",
        "Function",
        "Array",
        "Promise",
        "RegExp",
        "Date",
        "Error",
        "TypeError",
        "RangeError",
        "ReferenceError",
        "SyntaxError",
        "EvalError",
        "URIError",
        "WeakMap",
        "WeakSet",
        "SharedArrayBuffer",
        "WeakRef",
    ];
    names
        .into_iter()
        .filter_map(|name| {
            let register = emit_builtin(ops, next_register, builtin(name)?);
            Some((name.to_string(), register))
        })
        .collect()
}

fn global_constant(name: &str) -> Option<(FactConstant, Constant)> {
    Some(match name {
        "undefined" => (FactConstant::Undefined, Constant::Undefined),
        "NaN" => (FactConstant::Number(f64::NAN), Constant::Number(f64::NAN)),
        "Infinity" => (
            FactConstant::Number(f64::INFINITY),
            Constant::Number(f64::INFINITY),
        ),
        _ => return None,
    })
}

fn emit_builtin(ops: &mut Vec<Op>, next_register: &mut u16, builtin: crate::ops::Builtin) -> u16 {
    let register = take_register(next_register);
    ops.push(Op::MakeBuiltin {
        dst: register,
        builtin,
    });
    register
}

fn take_register(next_register: &mut u16) -> u16 {
    let register = *next_register;
    *next_register = next_register.saturating_add(1);
    register
}

fn builtin(name: &str) -> Option<crate::ops::Builtin> {
    typed_array_builtin(name)
        .or_else(|| (name == "Proxy").then_some(crate::ops::Builtin::Proxy))
        .or_else(|| builtin_core(name))
}

fn typed_array_builtin(name: &str) -> Option<crate::ops::Builtin> {
    match name {
        "Float64Array" => Some(crate::ops::Builtin::Float64Array),
        "Float32Array" => Some(crate::ops::Builtin::Float32Array),
        "Int8Array" => Some(crate::ops::Builtin::Int8Array),
        "Int16Array" => Some(crate::ops::Builtin::Int16Array),
        "Int32Array" => Some(crate::ops::Builtin::Int32Array),
        "Uint8Array" => Some(crate::ops::Builtin::Uint8Array),
        "Uint16Array" => Some(crate::ops::Builtin::Uint16Array),
        "Uint32Array" => Some(crate::ops::Builtin::Uint32Array),
        "Uint8ClampedArray" => Some(crate::ops::Builtin::Uint8ClampedArray),
        "BigInt64Array" => Some(crate::ops::Builtin::BigInt64Array),
        "BigUint64Array" => Some(crate::ops::Builtin::BigUint64Array),
        _ => None,
    }
}

fn builtin_core(name: &str) -> Option<crate::ops::Builtin> {
    if let Some(builtin) = collection_builtin(name) {
        return Some(builtin);
    }
    if let Some(builtin) = standard_builtin(name) {
        return Some(builtin);
    }
    match name {
        "Array" => Some(crate::ops::Builtin::Array),
        "ArrayBuffer" => Some(crate::ops::Builtin::ArrayBuffer),
        "SharedArrayBuffer" => Some(crate::ops::Builtin::SharedArrayBuffer),
        "DataView" => Some(crate::ops::Builtin::DataView),
        "Boolean" => Some(crate::ops::Builtin::Boolean),
        "BigInt" => Some(crate::ops::Builtin::BigInt),
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
        _ => None,
    }
}

fn standard_builtin(name: &str) -> Option<crate::ops::Builtin> {
    match name {
        "Date" => Some(crate::ops::Builtin::Date),
        "Promise" => Some(crate::ops::Builtin::Promise),
        "print" => Some(crate::ops::Builtin::Print),
        "Reflect" => Some(crate::ops::Builtin::Reflect),
        "JSON" => Some(crate::ops::Builtin::Json),
        "RegExp" => Some(crate::ops::Builtin::RegExp),
        "Intl" => Some(crate::ops::Builtin::Intl),
        _ => None,
    }
}

fn collection_builtin(name: &str) -> Option<crate::ops::Builtin> {
    match name {
        "Map" => Some(crate::ops::Builtin::Map),
        "Set" => Some(crate::ops::Builtin::Set),
        "WeakMap" => Some(crate::ops::Builtin::WeakMap),
        "WeakSet" => Some(crate::ops::Builtin::WeakSet),
        "WeakRef" => Some(crate::ops::Builtin::WeakRef),
        _ => None,
    }
}

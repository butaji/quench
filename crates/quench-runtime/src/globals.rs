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
        "Boolean" => Some(crate::ops::Builtin::Boolean),
        "eval" => Some(crate::ops::Builtin::Eval),
        "isFinite" => Some(crate::ops::Builtin::IsFinite),
        "isNaN" => Some(crate::ops::Builtin::IsNaN),
        "Number" => Some(crate::ops::Builtin::Number),
        "Object" => Some(crate::ops::Builtin::Object),
        "parseFloat" => Some(crate::ops::Builtin::ParseFloat),
        "parseInt" => Some(crate::ops::Builtin::ParseInt),
        "String" => Some(crate::ops::Builtin::String),
        _ => None,
    }
}

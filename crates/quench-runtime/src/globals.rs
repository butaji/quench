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

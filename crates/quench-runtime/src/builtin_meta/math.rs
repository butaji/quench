//! Math builtin metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::MathAbs => Some("Math.abs"),
        Builtin::MathFloor => Some("Math.floor"),
        Builtin::MathCeil => Some("Math.ceil"),
        Builtin::MathRound => Some("Math.round"),
        Builtin::MathTrunc => Some("Math.trunc"),
        Builtin::MathMax => Some("Math.max"),
        Builtin::MathMin => Some("Math.min"),
        Builtin::MathSign => Some("Math.sign"),
        Builtin::MathSqrt => Some("Math.sqrt"),
        Builtin::MathCbrt => Some("Math.cbrt"),
        Builtin::MathHypot => Some("Math.hypot"),
        Builtin::MathImul => Some("Math.imul"),
        Builtin::MathLog => Some("Math.log"),
        Builtin::MathLog10 => Some("Math.log10"),
        Builtin::MathLog2 => Some("Math.log2"),
        Builtin::MathExp => Some("Math.exp"),
        Builtin::MathSin => Some("Math.sin"),
        Builtin::MathCos => Some("Math.cos"),
        Builtin::MathTan => Some("Math.tan"),
        Builtin::MathAsin => Some("Math.asin"),
        Builtin::MathAcos => Some("Math.acos"),
        Builtin::MathAtan => Some("Math.atan"),
        Builtin::MathAtan2 => Some("Math.atan2"),
        Builtin::MathPow => Some("Math.pow"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::MathAtan2 | Builtin::MathImul => Some(2.0),
        _ => Some(1.0),
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::MathAbs => Some("abs"),
        Builtin::MathFloor => Some("floor"),
        Builtin::MathCeil => Some("ceil"),
        Builtin::MathRound => Some("round"),
        Builtin::MathTrunc => Some("trunc"),
        Builtin::MathMax => Some("max"),
        Builtin::MathMin => Some("min"),
        Builtin::MathSign => Some("sign"),
        Builtin::MathSqrt => Some("sqrt"),
        Builtin::MathCbrt => Some("cbrt"),
        Builtin::MathHypot => Some("hypot"),
        Builtin::MathImul => Some("imul"),
        Builtin::MathLog => Some("log"),
        Builtin::MathLog10 => Some("log10"),
        Builtin::MathLog2 => Some("log2"),
        Builtin::MathExp => Some("exp"),
        Builtin::MathSin => Some("sin"),
        Builtin::MathCos => Some("cos"),
        Builtin::MathTan => Some("tan"),
        Builtin::MathAsin => Some("asin"),
        Builtin::MathAcos => Some("acos"),
        Builtin::MathAtan => Some("atan"),
        Builtin::MathAtan2 => Some("atan2"),
        Builtin::MathPow => Some("pow"),
        _ => None,
    }
}

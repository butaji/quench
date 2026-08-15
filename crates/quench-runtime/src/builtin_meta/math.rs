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
        _ => fn_name_tail(builtin),
    }
}

const fn fn_name_tail(builtin: Builtin) -> Option<&'static str> {
    match builtin {
        Builtin::MathAtan2 => Some("Math.atan2"),
        Builtin::MathAcosh => Some("Math.acosh"),
        Builtin::MathAsinh => Some("Math.asinh"),
        Builtin::MathAtanh => Some("Math.atanh"),
        Builtin::MathClz32 => Some("Math.clz32"),
        Builtin::MathCosh => Some("Math.cosh"),
        Builtin::MathExpm1 => Some("Math.expm1"),
        Builtin::MathFround => Some("Math.fround"),
        Builtin::MathLog1p => Some("Math.log1p"),
        Builtin::MathSinh => Some("Math.sinh"),
        Builtin::MathTanh => Some("Math.tanh"),
        Builtin::MathF16Round => Some("Math.f16round"),
        Builtin::MathRandom => Some("Math.random"),
        Builtin::MathSumPrecise => Some("Math.sumPrecise"),
        Builtin::MathPow => Some("Math.pow"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::MathAtan2
        | Builtin::MathHypot
        | Builtin::MathImul
        | Builtin::MathMax
        | Builtin::MathMin
        | Builtin::MathPow => Some(2.0),
        Builtin::MathAbs
        | Builtin::MathFloor
        | Builtin::MathCeil
        | Builtin::MathRound
        | Builtin::MathTrunc
        | Builtin::MathSign
        | Builtin::MathSqrt
        | Builtin::MathCbrt
        | Builtin::MathLog
        | Builtin::MathLog10
        | Builtin::MathLog2
        | Builtin::MathExp
        | Builtin::MathSin
        | Builtin::MathCos
        | Builtin::MathTan
        | Builtin::MathAsin
        | Builtin::MathAcos
        | Builtin::MathAtan
        | Builtin::MathAcosh
        | Builtin::MathAsinh
        | Builtin::MathAtanh
        | Builtin::MathClz32
        | Builtin::MathCosh
        | Builtin::MathExpm1
        | Builtin::MathFround
        | Builtin::MathLog1p
        | Builtin::MathSinh
        | Builtin::MathTanh => Some(1.0),
        _ => fn_len_tail(b),
    }
}

const fn fn_len_tail(b: Builtin) -> Option<f64> {
    match b {
        Builtin::MathF16Round | Builtin::MathSumPrecise => Some(1.0),
        Builtin::MathRandom => Some(0.0),
        _ => None,
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
        Builtin::MathAcosh => Some("acosh"),
        Builtin::MathAsinh => Some("asinh"),
        Builtin::MathAtanh => Some("atanh"),
        Builtin::MathClz32 => Some("clz32"),
        Builtin::MathCosh => Some("cosh"),
        Builtin::MathExpm1 => Some("expm1"),
        Builtin::MathFround => Some("fround"),
        Builtin::MathLog1p => Some("log1p"),
        Builtin::MathSinh => Some("sinh"),
        Builtin::MathTanh => Some("tanh"),
        Builtin::MathF16Round => Some("f16round"),
        Builtin::MathRandom => Some("random"),
        Builtin::MathSumPrecise => Some("sumPrecise"),
        Builtin::MathPow => Some("pow"),
        _ => None,
    }
}

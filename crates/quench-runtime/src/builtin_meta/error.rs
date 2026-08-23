//! Error builtin metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ErrorIsError => Some("Error.isError"),
        Builtin::ErrorCaptureStackTrace => Some("Error.captureStackTrace"),
        Builtin::CallSiteGetFileName => Some("getFileName"),
        Builtin::CallSiteGetLineNumber => Some("getLineNumber"),
        Builtin::CallSiteGetColumnNumber => Some("getColumnNumber"),
        Builtin::CallSiteGetFunctionName => Some("getFunctionName"),
        Builtin::CallSiteIsEval => Some("isEval"),
        Builtin::CallSiteGetEvalOrigin => Some("getEvalOrigin"),
        Builtin::ErrorPrototypeToString => Some("Error.prototype.toString"),
        Builtin::ErrorPrototypeNameGetter => Some("get name"),
        Builtin::ErrorPrototypeMessageGetter => Some("get message"),
        Builtin::ErrorPrototypeCauseGetter => Some("get cause"),
        Builtin::ErrorPrototypeStackGetter => Some("get stack"),
        Builtin::ErrorPrototypeStackSetter => Some("set stack"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::ErrorIsError => Some(1.0),
        Builtin::ErrorCaptureStackTrace => Some(2.0),
        Builtin::CallSiteGetFileName
        | Builtin::CallSiteGetLineNumber
        | Builtin::CallSiteGetColumnNumber
        | Builtin::CallSiteGetFunctionName
        | Builtin::CallSiteIsEval
        | Builtin::CallSiteGetEvalOrigin => Some(0.0),
        Builtin::ErrorPrototypeToString
        | Builtin::ErrorPrototypeNameGetter
        | Builtin::ErrorPrototypeMessageGetter
        | Builtin::ErrorPrototypeCauseGetter
        | Builtin::ErrorPrototypeStackGetter => Some(0.0),
        Builtin::ErrorPrototypeStackSetter => Some(1.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ErrorIsError => Some("isError"),
        Builtin::ErrorCaptureStackTrace => Some("captureStackTrace"),
        Builtin::CallSiteGetFileName => Some("getFileName"),
        Builtin::CallSiteGetLineNumber => Some("getLineNumber"),
        Builtin::CallSiteGetColumnNumber => Some("getColumnNumber"),
        Builtin::CallSiteGetFunctionName => Some("getFunctionName"),
        Builtin::CallSiteIsEval => Some("isEval"),
        Builtin::CallSiteGetEvalOrigin => Some("getEvalOrigin"),
        Builtin::ErrorPrototypeToString => Some("toString"),
        Builtin::ErrorPrototypeNameGetter => Some("get name"),
        Builtin::ErrorPrototypeMessageGetter => Some("get message"),
        Builtin::ErrorPrototypeCauseGetter => Some("get cause"),
        Builtin::ErrorPrototypeStackGetter => Some("get stack"),
        Builtin::ErrorPrototypeStackSetter => Some("set stack"),
        _ => None,
    }
}

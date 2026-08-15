//! DataView method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::DataViewBufferGetter => Some("get buffer"),
        Builtin::DataViewByteLengthGetter => Some("get byteLength"),
        Builtin::DataViewByteOffsetGetter => Some("get byteOffset"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::DataViewBufferGetter
        | Builtin::DataViewByteLengthGetter
        | Builtin::DataViewByteOffsetGetter => Some(0.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    fn_name(b)
}

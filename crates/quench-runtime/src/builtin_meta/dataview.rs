//! DataView method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ArrayBufferByteLengthGetter => Some("get byteLength"),
        Builtin::DataViewBufferGetter => Some("get buffer"),
        Builtin::DataViewByteLengthGetter => Some("get byteLength"),
        Builtin::DataViewByteOffsetGetter => Some("get byteOffset"),
        Builtin::SharedArrayBufferByteLengthGetter => Some("get byteLength"),
        Builtin::SharedArrayBufferGrowableGetter => Some("get growable"),
        Builtin::SharedArrayBufferMaxByteLengthGetter => Some("get maxByteLength"),
        Builtin::SharedArrayBufferGrow => Some("grow"),
        Builtin::SharedArrayBufferSlice => Some("slice"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::DataViewBufferGetter
        | Builtin::DataViewByteLengthGetter
        | Builtin::DataViewByteOffsetGetter
        | Builtin::SharedArrayBufferByteLengthGetter => Some(0.0),
        Builtin::ArrayBufferByteLengthGetter => Some(0.0),
        Builtin::SharedArrayBufferGrowableGetter
        | Builtin::SharedArrayBufferMaxByteLengthGetter => Some(0.0),
        Builtin::SharedArrayBufferGrow => Some(1.0),
        Builtin::SharedArrayBufferSlice => Some(2.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    fn_name(b)
}

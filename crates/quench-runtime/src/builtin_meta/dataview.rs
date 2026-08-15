//! DataView method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::DataViewBufferGetter => Some("get buffer"),
        Builtin::DataViewByteLengthGetter => Some("get byteLength"),
        Builtin::ArrayBufferByteLengthGetter => Some("get byteLength"),
        Builtin::ArrayBufferDetachedGetter => Some("get detached"),
        Builtin::ArrayBufferMaxByteLengthGetter => Some("get maxByteLength"),
        Builtin::ArrayBufferResizableGetter => Some("get resizable"),
        Builtin::ArrayBufferImmutableGetter => Some("get immutable"),
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
        | Builtin::ArrayBufferByteLengthGetter
        | Builtin::ArrayBufferDetachedGetter
        | Builtin::ArrayBufferMaxByteLengthGetter
        | Builtin::ArrayBufferResizableGetter
        | Builtin::ArrayBufferImmutableGetter
        | Builtin::DataViewByteOffsetGetter => Some(0.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    fn_name(b)
}

//! DataView method metadata.

use crate::ops::Builtin;

pub const fn fn_name(b: Builtin) -> Option<&'static str> {
    match b {
        Builtin::ArrayBufferByteLengthGetter => Some("get byteLength"),
        Builtin::ArrayBufferDetachedGetter => Some("get detached"),
        Builtin::ArrayBufferImmutableGetter => Some("get immutable"),
        Builtin::ArrayBufferMaxByteLengthGetter => Some("get maxByteLength"),
        Builtin::ArrayBufferResizableGetter => Some("get resizable"),
        Builtin::ArrayBufferResize => Some("resize"),
        Builtin::ArrayBufferTransferToImmutable => Some("transferToImmutable"),
        Builtin::DataViewBufferGetter => Some("get buffer"),
        Builtin::DataViewByteLengthGetter => Some("get byteLength"),
        Builtin::DataViewByteOffsetGetter => Some("get byteOffset"),
        Builtin::SharedArrayBufferByteLengthGetter => Some("get byteLength"),
        Builtin::SharedArrayBufferGrowableGetter => Some("get growable"),
        Builtin::SharedArrayBufferMaxByteLengthGetter => Some("get maxByteLength"),
        Builtin::SharedArrayBufferGrow => Some("grow"),
        Builtin::ArrayBufferSlice | Builtin::SharedArrayBufferSlice => Some("slice"),
        Builtin::ArrayBufferSliceToImmutable => Some("sliceToImmutable"),
        _ => None,
    }
}

pub const fn fn_len(b: Builtin) -> Option<f64> {
    match b {
        Builtin::ArrayBufferIsView => Some(1.0),
        Builtin::DataViewBufferGetter
        | Builtin::DataViewByteLengthGetter
        | Builtin::DataViewByteOffsetGetter
        | Builtin::SharedArrayBufferByteLengthGetter => Some(0.0),
        Builtin::ArrayBufferByteLengthGetter => Some(0.0),
        Builtin::ArrayBufferDetachedGetter => Some(0.0),
        Builtin::ArrayBufferImmutableGetter => Some(0.0),
        Builtin::ArrayBufferMaxByteLengthGetter => Some(0.0),
        Builtin::ArrayBufferResizableGetter => Some(0.0),
        Builtin::ArrayBufferResize => Some(1.0),
        Builtin::ArrayBufferTransferToImmutable => Some(0.0),
        Builtin::SharedArrayBufferGrowableGetter
        | Builtin::SharedArrayBufferMaxByteLengthGetter => Some(0.0),
        Builtin::SharedArrayBufferGrow => Some(1.0),
        Builtin::ArrayBufferSlice | Builtin::SharedArrayBufferSlice => Some(2.0),
        Builtin::ArrayBufferSliceToImmutable => Some(2.0),
        _ => None,
    }
}

pub const fn short_name(b: Builtin) -> Option<&'static str> {
    fn_name(b)
}

use crate::ops::Builtin;

pub(crate) fn data_view_name(builtin: Builtin) -> Option<&'static str> {
    Some(match builtin {
        Builtin::DataView => "DataView",
        Builtin::DataViewGetInt8 => "getInt8",
        Builtin::DataViewGetUint8 => "getUint8",
        Builtin::DataViewGetInt16 => "getInt16",
        Builtin::DataViewGetUint16 => "getUint16",
        Builtin::DataViewGetInt32 => "getInt32",
        Builtin::DataViewGetUint32 => "getUint32",
        Builtin::DataViewGetFloat16 => "getFloat16",
        Builtin::DataViewGetFloat32 => "getFloat32",
        Builtin::DataViewGetFloat64 => "getFloat64",
        Builtin::DataViewSetInt8 => "setInt8",
        Builtin::DataViewSetUint8 => "setUint8",
        Builtin::DataViewSetInt16 => "setInt16",
        Builtin::DataViewSetUint16 => "setUint16",
        Builtin::DataViewSetInt32 => "setInt32",
        Builtin::DataViewSetUint32 => "setUint32",
        Builtin::DataViewSetFloat16 => "setFloat16",
        Builtin::DataViewSetFloat32 => "setFloat32",
        Builtin::DataViewSetFloat64 => "setFloat64",
        _ => return None,
    })
}

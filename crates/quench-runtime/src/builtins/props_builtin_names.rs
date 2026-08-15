pub(crate) fn builtin_name(builtin: Builtin) -> &'static str {
    use Builtin::*;
    if let Some(name) = metadata_builtin_name(builtin) {
        return name;
    }
    match builtin {
        Eval => "eval", Escape => "escape", Unescape => "unescape", EncodeURI => "encodeURI", EncodeURIComponent => "encodeURIComponent", DecodeURI => "decodeURI", DecodeURIComponent => "decodeURIComponent", Array => "Array", ArrayBuffer => "ArrayBuffer",
        ArrayBufferIsView => "isView", Object => "Object", String => "String", Symbol => "Symbol",
        Number => "Number", Date => "Date", DateGetYear => "getYear", DateSetYear => "setYear",
        RegExp => "RegExp", RegExpTest => "test", RegExpExec => "exec", _ => "",
    }
}

fn metadata_builtin_name(builtin: Builtin) -> Option<&'static str> {
    crate::builtin_meta::methods::short_name(builtin)
        .or_else(|| crate::builtin_meta::methods::function_name(builtin))
        .or_else(|| data_view_name::data_view_name(builtin))
        .or_else(|| error_name(builtin))
        .or_else(|| generator_name(builtin))
        .or_else(|| typed_array_name(builtin))
        .or_else(|| crate::builtin_meta::constructor_name(builtin))
}

use super::*;

pub(crate) type NativeSemantic = fn(&mut Vm, Value, &[Value]) -> JsResult<Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuiltinOwner {
    Global,
    Math,
    Reflect,
    Console,
    ArrayPrototype,
    StringPrototype,
    NumberPrototype,
    BooleanPrototype,
    DatePrototype,
    RegExpPrototype,
    ObjectPrototype,
    FunctionPrototype,
    Assert,
    Process,
    StringConstructor,
    NumberConstructor,
    DateConstructor,
    ObjectConstructor,
    ArrayConstructor,
    FunctionConstructor,
}

impl BuiltinOwner {
    /// Return the constructor whose prototype owns methods for this namespace.
    ///
    /// Keeping this relationship beside the owner declaration means builtin
    /// installation can lower one catalog entry uniformly instead of carrying
    /// a second, hand-maintained prototype match in the VM.
    pub(crate) const fn prototype_constructor(self) -> Option<BuiltinId> {
        match self {
            Self::ArrayPrototype => Some(BuiltinId::ArrayConstructor),
            Self::StringPrototype => Some(BuiltinId::StringConstructor),
            Self::NumberPrototype => Some(BuiltinId::NumberConstructor),
            Self::BooleanPrototype => Some(BuiltinId::BooleanConstructor),
            Self::DatePrototype => Some(BuiltinId::DateConstructor),
            Self::RegExpPrototype => Some(BuiltinId::RegExpConstructor),
            Self::ObjectPrototype => Some(BuiltinId::ObjectConstructor),
            Self::FunctionPrototype => Some(BuiltinId::FunctionConstructor),
            _ => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuiltinSignature {
    Generic,
    UnaryNumber,
    BinaryNumber,
    VariadicNumber,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct BuiltinEffects(u8);

impl BuiltinEffects {
    const PURE: Self = Self(0);
    const MAY_ALLOCATE: Self = Self(1 << 0);
    const MAY_MUTATE: Self = Self(1 << 1);
    const MAY_CALL_JS: Self = Self(1 << 2);
    const EFFECTFUL: Self = Self(Self::MAY_ALLOCATE.0 | Self::MAY_MUTATE.0 | Self::MAY_CALL_JS.0);
}

#[derive(Clone, Copy)]
pub(crate) struct BuiltinRecipe {
    pub id: BuiltinId,
    pub owner: BuiltinOwner,
    pub key: &'static str,
    pub signature: BuiltinSignature,
    pub effects: BuiltinEffects,
    pub semantic: NativeSemantic,
}

macro_rules! builtin_catalog {
    ($( $id:ident, $owner:ident, $key:literal, $semantic:ident, $signature:ident, $effects:ident; )+) => {
        #[derive(Clone, Copy, Debug, PartialEq, Eq)]
        #[repr(usize)]
        pub(crate) enum BuiltinId {
            $( $id, )+
        }

        impl BuiltinId {
            pub(crate) const ALL: &'static [Self] = &[$(Self::$id),+];

            pub(crate) fn recipe(self) -> &'static BuiltinRecipe {
                &BUILTIN_RECIPES[self as usize]
            }
        }

        pub(crate) static BUILTIN_RECIPES: &[BuiltinRecipe] = &[
            $( BuiltinRecipe {
                id: BuiltinId::$id,
                owner: BuiltinOwner::$owner,
                key: $key,
                signature: BuiltinSignature::$signature,
                effects: BuiltinEffects::$effects,
                semantic: $semantic,
            }, )+
        ];

        pub(crate) fn lookup(owner: BuiltinOwner, key: &str) -> Option<BuiltinId> {
            match (owner, key) {
                $( (BuiltinOwner::$owner, $key) => Some(BuiltinId::$id), )+
                _ => None,
            }
        }
    };

}

builtin_catalog! {
    ParseInt, Global, "parseInt", native_parse_int, Generic, EFFECTFUL;
    ParseFloat, Global, "parseFloat", native_parse_float, Generic, EFFECTFUL;
    IsNaN, Global, "isNaN", native_is_nan, UnaryNumber, PURE;
    IsFinite, Global, "isFinite", native_is_finite, UnaryNumber, PURE;
    DecodeURI, Global, "decodeURI", native_decode_uri, Generic, MAY_ALLOCATE;
    DecodeURIComponent, Global, "decodeURIComponent", native_decode_uri_component, Generic, MAY_ALLOCATE;
    EncodeURI, Global, "encodeURI", native_encode_uri, Generic, MAY_ALLOCATE;
    EncodeURIComponent, Global, "encodeURIComponent", native_encode_uri_component, Generic, MAY_ALLOCATE;
    MathPow, Math, "pow", native_math_pow, BinaryNumber, PURE;
    MathFloor, Math, "floor", native_math_floor, UnaryNumber, PURE;
    MathCeil, Math, "ceil", native_math_ceil, UnaryNumber, PURE;
    MathSqrt, Math, "sqrt", native_math_sqrt, UnaryNumber, PURE;
    MathAcos, Math, "acos", native_math_acos, UnaryNumber, PURE;
    MathAsin, Math, "asin", native_math_asin, UnaryNumber, PURE;
    MathAtan, Math, "atan", native_math_atan, UnaryNumber, PURE;
    MathAtan2, Math, "atan2", native_math_atan2, BinaryNumber, PURE;
    MathCbrt, Math, "cbrt", native_math_cbrt, UnaryNumber, PURE;
    MathCosh, Math, "cosh", native_math_cosh, UnaryNumber, PURE;
    MathSinh, Math, "sinh", native_math_sinh, UnaryNumber, PURE;
    MathTanh, Math, "tanh", native_math_tanh, UnaryNumber, PURE;
    MathAcosh, Math, "acosh", native_math_acosh, UnaryNumber, PURE;
    MathAsinh, Math, "asinh", native_math_asinh, UnaryNumber, PURE;
    MathAtanh, Math, "atanh", native_math_atanh, UnaryNumber, PURE;
    MathExpm1, Math, "expm1", native_math_expm1, UnaryNumber, PURE;
    MathLog1p, Math, "log1p", native_math_log1p, UnaryNumber, PURE;
    MathAbs, Math, "abs", native_math_abs, UnaryNumber, PURE;
    MathMin, Math, "min", native_math_min, VariadicNumber, PURE;
    MathMax, Math, "max", native_math_max, VariadicNumber, PURE;
    MathLog, Math, "log", native_math_log, UnaryNumber, MAY_CALL_JS;
    MathRound, Math, "round", native_math_round, UnaryNumber, PURE;
    MathTrunc, Math, "trunc", native_math_trunc, UnaryNumber, PURE;
    MathSign, Math, "sign", native_math_sign, UnaryNumber, PURE;
    MathSin, Math, "sin", native_math_sin, UnaryNumber, PURE;
    MathCos, Math, "cos", native_math_cos, UnaryNumber, PURE;
    MathTan, Math, "tan", native_math_tan, UnaryNumber, PURE;
    MathExp, Math, "exp", native_math_exp, UnaryNumber, PURE;
    MathLog10, Math, "log10", native_math_log10, UnaryNumber, PURE;
    MathLog2, Math, "log2", native_math_log2, UnaryNumber, PURE;
    MathHypot, Math, "hypot", native_math_hypot, VariadicNumber, PURE;
    MathClz32, Math, "clz32", native_math_clz32, UnaryNumber, PURE;
    MathImul, Math, "imul", native_math_imul, BinaryNumber, PURE;
    MathFround, Math, "fround", native_math_fround, UnaryNumber, PURE;
    MathF16Round, Math, "f16round", native_math_f16round, UnaryNumber, PURE;
    MathSumPrecise, Math, "sumPrecise", native_math_sum_precise, Generic, MAY_ALLOCATE;
    MathRandom, Math, "random", native_random, Generic, PURE;
    ReflectApply, Reflect, "apply", native_reflect_apply, Generic, MAY_CALL_JS;
    ReflectConstruct, Reflect, "construct", native_reflect_construct, Generic, MAY_ALLOCATE;
    ReflectDefineProperty, Reflect, "defineProperty", native_reflect_define_property, Generic, MAY_MUTATE;
    ReflectDeleteProperty, Reflect, "deleteProperty", native_reflect_delete_property, Generic, MAY_MUTATE;
    ReflectGet, Reflect, "get", native_reflect_get, Generic, MAY_CALL_JS;
    ReflectGetOwnPropertyDescriptor, Reflect, "getOwnPropertyDescriptor", native_reflect_get_own_property_descriptor, Generic, MAY_ALLOCATE;
    ReflectGetPrototypeOf, Reflect, "getPrototypeOf", native_reflect_get_prototype_of, Generic, PURE;
    ReflectHas, Reflect, "has", native_reflect_has, Generic, PURE;
    ReflectIsExtensible, Reflect, "isExtensible", native_reflect_is_extensible, Generic, PURE;
    ReflectOwnKeys, Reflect, "ownKeys", native_reflect_own_keys, Generic, MAY_ALLOCATE;
    ReflectPreventExtensions, Reflect, "preventExtensions", native_reflect_prevent_extensions, Generic, MAY_MUTATE;
    ReflectSet, Reflect, "set", native_reflect_set, Generic, MAY_MUTATE;
    ReflectSetPrototypeOf, Reflect, "setPrototypeOf", native_reflect_set_prototype_of, Generic, MAY_MUTATE;
    ObjectConstructor, Global, "Object", native_object, Generic, MAY_ALLOCATE;
    ObjectGetOwnPropertyDescriptor, ObjectConstructor, "getOwnPropertyDescriptor", native_object_get_own_property_descriptor, Generic, MAY_ALLOCATE;
    ObjectGetPrototypeOf, ObjectConstructor, "getPrototypeOf", native_object_get_prototype_of, Generic, PURE;
    ObjectKeys, ObjectConstructor, "keys", native_object_keys, Generic, MAY_ALLOCATE;
    ObjectGetOwnPropertyNames, ObjectConstructor, "getOwnPropertyNames", native_object_get_own_property_names, Generic, MAY_ALLOCATE;
    ObjectGetOwnPropertySymbols, ObjectConstructor, "getOwnPropertySymbols", native_object_get_own_property_symbols, Generic, MAY_ALLOCATE;
    ObjectGetOwnPropertyDescriptors, ObjectConstructor, "getOwnPropertyDescriptors", native_object_get_own_property_descriptors, Generic, MAY_ALLOCATE;
    ObjectCreate, ObjectConstructor, "create", native_object_create, Generic, MAY_ALLOCATE;
    ObjectDefineProperties, ObjectConstructor, "defineProperties", native_object_define_properties, Generic, MAY_MUTATE;
    ObjectDefineProperty, ObjectConstructor, "defineProperty", native_object_define_property, Generic, MAY_MUTATE;
    ObjectPreventExtensions, ObjectConstructor, "preventExtensions", native_object_prevent_extensions, Generic, MAY_MUTATE;
    ObjectIsExtensible, ObjectConstructor, "isExtensible", native_object_is_extensible, Generic, PURE;
    ObjectAssign, ObjectConstructor, "assign", native_object_assign, Generic, MAY_MUTATE;
    ObjectValues, ObjectConstructor, "values", native_object_values, Generic, MAY_ALLOCATE;
    ObjectEntries, ObjectConstructor, "entries", native_object_entries, Generic, MAY_ALLOCATE;
    ObjectFromEntries, ObjectConstructor, "fromEntries", native_object_from_entries, Generic, MAY_ALLOCATE;
    ObjectHasOwn, ObjectConstructor, "hasOwn", native_object_has_own, Generic, PURE;
    ObjectIs, ObjectConstructor, "is", native_object_is, Generic, PURE;
    ObjectSetPrototypeOf, ObjectConstructor, "setPrototypeOf", native_object_set_prototype_of, Generic, MAY_MUTATE;
    ObjectSeal, ObjectConstructor, "seal", native_object_seal, Generic, MAY_MUTATE;
    ObjectFreeze, ObjectConstructor, "freeze", native_object_freeze, Generic, MAY_MUTATE;
    ObjectIsSealed, ObjectConstructor, "isSealed", native_object_is_sealed, Generic, PURE;
    ObjectIsFrozen, ObjectConstructor, "isFrozen", native_object_is_frozen, Generic, PURE;
    ArrayConstructor, Global, "Array", native_array, Generic, MAY_ALLOCATE;
    ArrayIsArray, ArrayConstructor, "isArray", native_array_is_array, Generic, PURE;
    ArrayFrom, ArrayConstructor, "from", native_array_from, Generic, MAY_ALLOCATE;
    ArrayOf, ArrayConstructor, "of", native_array_of, Generic, MAY_ALLOCATE;
    FunctionConstructor, Global, "Function", native_function_constructor, Generic, MAY_ALLOCATE;
    StringConstructor, Global, "String", native_string, Generic, MAY_ALLOCATE;
    NumberConstructor, Global, "Number", native_number, Generic, PURE;
    BooleanConstructor, Global, "Boolean", native_boolean, Generic, PURE;
    BooleanValueOf, BooleanPrototype, "valueOf", native_boolean_value_of, Generic, PURE;
    BooleanToString, BooleanPrototype, "toString", native_boolean_to_string, Generic, PURE;
    DateConstructor, Global, "Date", native_date, Generic, PURE;
    DateGetTime, DatePrototype, "getTime", native_date_get_time, Generic, PURE;
    DateValueOf, DatePrototype, "valueOf", native_date_value_of, Generic, PURE;
    DateToISOString, DatePrototype, "toISOString", native_date_to_iso_string, Generic, MAY_ALLOCATE;
    DateToJSON, DatePrototype, "toJSON", native_date_to_json, Generic, MAY_ALLOCATE;
    DateGetUTCFullYear, DatePrototype, "getUTCFullYear", native_date_get_utc_full_year, Generic, PURE;
    DateGetUTCMonth, DatePrototype, "getUTCMonth", native_date_get_utc_month, Generic, PURE;
    DateGetUTCDate, DatePrototype, "getUTCDate", native_date_get_utc_date, Generic, PURE;
    DateGetUTCDay, DatePrototype, "getUTCDay", native_date_get_utc_day, Generic, PURE;
    DateGetUTCHours, DatePrototype, "getUTCHours", native_date_get_utc_hours, Generic, PURE;
    DateGetUTCMinutes, DatePrototype, "getUTCMinutes", native_date_get_utc_minutes, Generic, PURE;
    DateGetUTCSeconds, DatePrototype, "getUTCSeconds", native_date_get_utc_seconds, Generic, PURE;
    DateGetUTCMilliseconds, DatePrototype, "getUTCMilliseconds", native_date_get_utc_milliseconds, Generic, PURE;
    DateGetFullYear, DatePrototype, "getFullYear", native_date_get_utc_full_year, Generic, PURE;
    DateGetYear, DatePrototype, "getYear", native_date_get_year, Generic, PURE;
    DateGetMonth, DatePrototype, "getMonth", native_date_get_utc_month, Generic, PURE;
    DateGetDate, DatePrototype, "getDate", native_date_get_utc_date, Generic, PURE;
    DateGetDay, DatePrototype, "getDay", native_date_get_utc_day, Generic, PURE;
    DateGetHours, DatePrototype, "getHours", native_date_get_utc_hours, Generic, PURE;
    DateGetMinutes, DatePrototype, "getMinutes", native_date_get_utc_minutes, Generic, PURE;
    DateGetSeconds, DatePrototype, "getSeconds", native_date_get_utc_seconds, Generic, PURE;
    DateGetMilliseconds, DatePrototype, "getMilliseconds", native_date_get_utc_milliseconds, Generic, PURE;
    DateSetTime, DatePrototype, "setTime", native_date_set_time, Generic, MAY_MUTATE;
    DateSetUTCFullYear, DatePrototype, "setUTCFullYear", native_date_set_utc_full_year, Generic, MAY_MUTATE;
    DateSetUTCMonth, DatePrototype, "setUTCMonth", native_date_set_utc_month, Generic, MAY_MUTATE;
    DateSetUTCDate, DatePrototype, "setUTCDate", native_date_set_utc_date, Generic, MAY_MUTATE;
    DateSetUTCHours, DatePrototype, "setUTCHours", native_date_set_utc_hours, Generic, MAY_MUTATE;
    DateSetUTCMinutes, DatePrototype, "setUTCMinutes", native_date_set_utc_minutes, Generic, MAY_MUTATE;
    DateSetUTCSeconds, DatePrototype, "setUTCSeconds", native_date_set_utc_seconds, Generic, MAY_MUTATE;
    DateSetUTCMilliseconds, DatePrototype, "setUTCMilliseconds", native_date_set_utc_milliseconds, Generic, MAY_MUTATE;
    DateSetFullYear, DatePrototype, "setFullYear", native_date_set_utc_full_year, Generic, MAY_MUTATE;
    DateSetYear, DatePrototype, "setYear", native_date_set_year, Generic, MAY_MUTATE;
    DateSetMonth, DatePrototype, "setMonth", native_date_set_utc_month, Generic, MAY_MUTATE;
    DateSetDate, DatePrototype, "setDate", native_date_set_utc_date, Generic, MAY_MUTATE;
    DateSetHours, DatePrototype, "setHours", native_date_set_utc_hours, Generic, MAY_MUTATE;
    DateSetMinutes, DatePrototype, "setMinutes", native_date_set_utc_minutes, Generic, MAY_MUTATE;
    DateSetSeconds, DatePrototype, "setSeconds", native_date_set_utc_seconds, Generic, MAY_MUTATE;
    DateSetMilliseconds, DatePrototype, "setMilliseconds", native_date_set_utc_milliseconds, Generic, MAY_MUTATE;
    DateToString, DatePrototype, "toString", native_date_to_string, Generic, MAY_ALLOCATE;
    DateToDateString, DatePrototype, "toDateString", native_date_to_date_string, Generic, MAY_ALLOCATE;
    DateToUTCString, DatePrototype, "toUTCString", native_date_to_utc_string, Generic, MAY_ALLOCATE;
    DateToGMTString, DatePrototype, "toGMTString", native_date_to_utc_string, Generic, MAY_ALLOCATE;
    DateToTimeString, DatePrototype, "toTimeString", native_date_to_time_string, Generic, MAY_ALLOCATE;
    DateToLocaleString, DatePrototype, "toLocaleString", native_date_to_string, Generic, MAY_ALLOCATE;
    DateToLocaleDateString, DatePrototype, "toLocaleDateString", native_date_to_date_string, Generic, MAY_ALLOCATE;
    DateToLocaleTimeString, DatePrototype, "toLocaleTimeString", native_date_to_time_string, Generic, MAY_ALLOCATE;
    DateGetTimezoneOffset, DatePrototype, "getTimezoneOffset", native_date_get_timezone_offset, Generic, PURE;
    DateToTemporalInstant, DatePrototype, "toTemporalInstant", native_date_to_temporal_instant, Generic, MAY_ALLOCATE;
    DateToPrimitive, DatePrototype, "Symbol(Symbol.toPrimitive)", native_date_to_primitive, Generic, MAY_ALLOCATE;
    DateNow, DateConstructor, "now", native_date_now, Generic, PURE;
    DateParse, DateConstructor, "parse", native_date_parse, Generic, MAY_ALLOCATE;
    DateUTC, DateConstructor, "UTC", native_date_utc, Generic, MAY_ALLOCATE;
    RegExpConstructor, Global, "RegExp", native_regexp, Generic, MAY_ALLOCATE;
    RegExpCompile, RegExpPrototype, "compile", native_regexp_compile, Generic, MAY_MUTATE;
    RegExpToString, RegExpPrototype, "toString", native_regexp_to_string, Generic, MAY_ALLOCATE;
    RegExpMatchAll, RegExpPrototype, "Symbol(Symbol.matchAll)", native_regexp_match_all, Generic, MAY_ALLOCATE;
    RegExpSymbolMatch, RegExpPrototype, "Symbol(Symbol.match)", native_regexp_symbol_match, Generic, MAY_ALLOCATE;
    RegExpSymbolSearch, RegExpPrototype, "Symbol(Symbol.search)", native_regexp_symbol_search, Generic, MAY_ALLOCATE;
    RegExpSymbolReplace, RegExpPrototype, "Symbol(Symbol.replace)", native_regexp_symbol_replace, Generic, MAY_ALLOCATE;
    RegExpSymbolSplit, RegExpPrototype, "Symbol(Symbol.split)", native_regexp_symbol_split, Generic, MAY_ALLOCATE;
    ErrorConstructor, Global, "Error", native_error, Generic, MAY_ALLOCATE;
    TypeErrorConstructor, Global, "TypeError", native_error, Generic, MAY_ALLOCATE;
    RangeErrorConstructor, Global, "RangeError", native_error, Generic, MAY_ALLOCATE;
    URIErrorConstructor, Global, "URIError", native_error, Generic, MAY_ALLOCATE;
    SyntaxErrorConstructor, Global, "SyntaxError", native_error, Generic, MAY_ALLOCATE;
    ReferenceErrorConstructor, Global, "ReferenceError", native_error, Generic, MAY_ALLOCATE;
    EvalErrorConstructor, Global, "EvalError", native_error, Generic, MAY_ALLOCATE;
    AggregateErrorConstructor, Global, "AggregateError", native_error, Generic, MAY_ALLOCATE;
    Eval, Global, "eval", native_eval, Generic, EFFECTFUL;
    Alert, Global, "alert", native_noop, Generic, EFFECTFUL;
    Print, Global, "print", native_print, Generic, EFFECTFUL;
    Load, Global, "load", native_load, Generic, EFFECTFUL;
    Require, Global, "require", native_require, Generic, EFFECTFUL;
    Assert, Global, "assert", native_assert, Generic, EFFECTFUL;
    AssertStrictEqual, Assert, "strictEqual", native_assert_strict_equal, Generic, EFFECTFUL;
    AssertThrows, Assert, "throws", native_assert_throws, Generic, MAY_CALL_JS;
    SetTimeout, Global, "setTimeout", native_set_timeout, Generic, EFFECTFUL;
    ClearTimeout, Global, "clearTimeout", native_clear_timeout, Generic, EFFECTFUL;
    SetImmediate, Global, "setImmediate", native_set_immediate, Generic, EFFECTFUL;
    ProcessNextTick, Process, "nextTick", native_process_next_tick, Generic, EFFECTFUL;
    ConsoleLog, Console, "log", native_print, Generic, EFFECTFUL;
    StringFromCharCode, StringConstructor, "fromCharCode", native_string_from_char_code, Generic, MAY_ALLOCATE;
    StringFromCodePoint, StringConstructor, "fromCodePoint", native_string_from_code_point, Generic, MAY_ALLOCATE;
    NumberIsFinite, NumberConstructor, "isFinite", native_number_is_finite, Generic, PURE;
    NumberIsInteger, NumberConstructor, "isInteger", native_number_is_integer, Generic, PURE;
    NumberIsNaN, NumberConstructor, "isNaN", native_number_is_nan, Generic, PURE;
    NumberIsSafeInteger, NumberConstructor, "isSafeInteger", native_number_is_safe_integer, Generic, PURE;
    NumberParseFloat, NumberConstructor, "parseFloat", native_parse_float, Generic, PURE;
    NumberParseInt, NumberConstructor, "parseInt", native_parse_int, Generic, EFFECTFUL;
    ArrayPush, ArrayPrototype, "push", native_array_push, Generic, MAY_MUTATE;
    ArrayPop, ArrayPrototype, "pop", native_array_pop, Generic, MAY_MUTATE;
    ArrayShift, ArrayPrototype, "shift", native_array_shift, Generic, MAY_MUTATE;
    ArrayUnshift, ArrayPrototype, "unshift", native_array_unshift, Generic, MAY_MUTATE;
    ArraySlice, ArrayPrototype, "slice", native_array_slice, Generic, MAY_ALLOCATE;
    ArrayJoin, ArrayPrototype, "join", native_array_join, Generic, MAY_ALLOCATE;
    ArrayToString, ArrayPrototype, "toString", native_array_to_string, Generic, MAY_ALLOCATE;
    ArrayConcat, ArrayPrototype, "concat", native_array_concat, Generic, MAY_ALLOCATE;
    ArrayForEach, ArrayPrototype, "forEach", native_array_for_each, Generic, MAY_CALL_JS;
    ArrayMap, ArrayPrototype, "map", native_array_map, Generic, MAY_ALLOCATE;
    ArrayFilter, ArrayPrototype, "filter", native_array_filter, Generic, MAY_ALLOCATE;
    ArraySome, ArrayPrototype, "some", native_array_some, Generic, MAY_CALL_JS;
    ArrayEvery, ArrayPrototype, "every", native_array_every, Generic, MAY_CALL_JS;
    ArrayIndexOf, ArrayPrototype, "indexOf", native_array_index_of, Generic, PURE;
    ArrayLastIndexOf, ArrayPrototype, "lastIndexOf", native_array_last_index_of, Generic, PURE;
    ArrayIncludes, ArrayPrototype, "includes", native_array_includes, Generic, PURE;
    ArrayReduce, ArrayPrototype, "reduce", native_array_reduce, Generic, MAY_CALL_JS;
    ArrayReduceRight, ArrayPrototype, "reduceRight", native_array_reduce_right, Generic, MAY_CALL_JS;
    ArrayFind, ArrayPrototype, "find", native_array_find, Generic, MAY_CALL_JS;
    ArrayFindIndex, ArrayPrototype, "findIndex", native_array_find_index, Generic, MAY_CALL_JS;
    ArraySplice, ArrayPrototype, "splice", native_array_splice, Generic, MAY_MUTATE;
    ArrayReverse, ArrayPrototype, "reverse", native_array_reverse, Generic, MAY_MUTATE;
    ArraySort, ArrayPrototype, "sort", native_array_sort, Generic, MAY_MUTATE;
    ArrayFlat, ArrayPrototype, "flat", native_array_flat, Generic, MAY_ALLOCATE;
    ArrayFlatMap, ArrayPrototype, "flatMap", native_array_flat_map, Generic, MAY_ALLOCATE;
    ArrayAt, ArrayPrototype, "at", native_array_at, Generic, PURE;
    ArrayFill, ArrayPrototype, "fill", native_array_fill, Generic, MAY_MUTATE;
    ArrayCopyWithin, ArrayPrototype, "copyWithin", native_array_copy_within, Generic, MAY_MUTATE;
    ArrayKeys, ArrayPrototype, "keys", native_array_keys, Generic, MAY_ALLOCATE;
    ArrayValues, ArrayPrototype, "values", native_array_values, Generic, MAY_ALLOCATE;
    ArrayEntries, ArrayPrototype, "entries", native_array_entries, Generic, MAY_ALLOCATE;
    ArrayToReversed, ArrayPrototype, "toReversed", native_array_to_reversed, Generic, MAY_ALLOCATE;
    ArrayToSorted, ArrayPrototype, "toSorted", native_array_to_sorted, Generic, MAY_ALLOCATE;
    ArrayToSpliced, ArrayPrototype, "toSpliced", native_array_to_spliced, Generic, MAY_ALLOCATE;
    ArrayWith, ArrayPrototype, "with", native_array_with, Generic, MAY_ALLOCATE;
    StringSubstring, StringPrototype, "substring", native_string_substring, Generic, MAY_ALLOCATE;
    StringSlice, StringPrototype, "slice", native_string_slice, Generic, MAY_ALLOCATE;
    StringCharCodeAt, StringPrototype, "charCodeAt", native_string_char_code_at, Generic, PURE;
    StringCharAt, StringPrototype, "charAt", native_string_char_at, Generic, MAY_ALLOCATE;
    StringSubstr, StringPrototype, "substr", native_string_substr, Generic, MAY_ALLOCATE;
    StringToLowerCase, StringPrototype, "toLowerCase", native_string_lower, Generic, MAY_ALLOCATE;
    StringToUpperCase, StringPrototype, "toUpperCase", native_string_upper, Generic, MAY_ALLOCATE;
    StringToString, StringPrototype, "toString", native_string_to_string, Generic, PURE;
    StringValueOf, StringPrototype, "valueOf", native_string_value_of, Generic, PURE;
    StringConcat, StringPrototype, "concat", native_string_concat, Generic, MAY_ALLOCATE;
    StringReplace, StringPrototype, "replace", native_string_replace, Generic, MAY_ALLOCATE;
    StringReplaceAll, StringPrototype, "replaceAll", native_string_replace_all, Generic, MAY_ALLOCATE;
    StringSplit, StringPrototype, "split", native_string_split, Generic, MAY_ALLOCATE;
    StringMatch, StringPrototype, "match", native_string_match, Generic, MAY_ALLOCATE;
    StringMatchAll, StringPrototype, "matchAll", native_string_match_all, Generic, MAY_ALLOCATE;
    StringSearch, StringPrototype, "search", native_string_search, Generic, MAY_ALLOCATE;
    StringIterator, StringPrototype, "Symbol(Symbol.iterator)", native_string_iterator, Generic, MAY_ALLOCATE;
    StringAnchor, StringPrototype, "anchor", native_string_anchor, Generic, MAY_ALLOCATE;
    StringBig, StringPrototype, "big", native_string_big, Generic, MAY_ALLOCATE;
    StringBlink, StringPrototype, "blink", native_string_blink, Generic, MAY_ALLOCATE;
    StringBold, StringPrototype, "bold", native_string_bold, Generic, MAY_ALLOCATE;
    StringFixed, StringPrototype, "fixed", native_string_fixed, Generic, MAY_ALLOCATE;
    StringFontcolor, StringPrototype, "fontcolor", native_string_fontcolor, Generic, MAY_ALLOCATE;
    StringFontsize, StringPrototype, "fontsize", native_string_fontsize, Generic, MAY_ALLOCATE;
    StringItalics, StringPrototype, "italics", native_string_italics, Generic, MAY_ALLOCATE;
    StringLink, StringPrototype, "link", native_string_link, Generic, MAY_ALLOCATE;
    StringSmall, StringPrototype, "small", native_string_small, Generic, MAY_ALLOCATE;
    StringStrike, StringPrototype, "strike", native_string_strike, Generic, MAY_ALLOCATE;
    StringSub, StringPrototype, "sub", native_string_sub, Generic, MAY_ALLOCATE;
    StringSup, StringPrototype, "sup", native_string_sup, Generic, MAY_ALLOCATE;
    StringTrim, StringPrototype, "trim", native_string_trim, Generic, MAY_ALLOCATE;
    StringTrimStart, StringPrototype, "trimStart", native_string_trim_left, Generic, MAY_ALLOCATE;
    StringTrimEnd, StringPrototype, "trimEnd", native_string_trim_right, Generic, MAY_ALLOCATE;
    StringTrimLeft, StringPrototype, "trimLeft", native_string_trim_left, Generic, MAY_ALLOCATE;
    StringTrimRight, StringPrototype, "trimRight", native_string_trim_right, Generic, MAY_ALLOCATE;
    Escape, Global, "escape", native_escape, Generic, MAY_ALLOCATE;
    Unescape, Global, "unescape", native_unescape, Generic, MAY_ALLOCATE;
    StringIndexOf, StringPrototype, "indexOf", native_string_index_of, Generic, PURE;
    StringLastIndexOf, StringPrototype, "lastIndexOf", native_string_last_index_of, Generic, PURE;
    StringIncludes, StringPrototype, "includes", native_string_includes, Generic, PURE;
    StringStartsWith, StringPrototype, "startsWith", native_string_starts_with, Generic, PURE;
    StringEndsWith, StringPrototype, "endsWith", native_string_ends_with, Generic, PURE;
    StringRepeat, StringPrototype, "repeat", native_string_repeat, Generic, MAY_ALLOCATE;
    StringPadStart, StringPrototype, "padStart", native_string_pad_start, Generic, MAY_ALLOCATE;
    StringPadEnd, StringPrototype, "padEnd", native_string_pad_end, Generic, MAY_ALLOCATE;
    StringAt, StringPrototype, "at", native_string_at, Generic, MAY_ALLOCATE;
    StringCodePointAt, StringPrototype, "codePointAt", native_string_code_point_at, Generic, PURE;
    StringNormalize, StringPrototype, "normalize", native_string_normalize, Generic, MAY_ALLOCATE;
    StringToLocaleLowerCase, StringPrototype, "toLocaleLowerCase", native_string_lower, Generic, MAY_ALLOCATE;
    StringToLocaleUpperCase, StringPrototype, "toLocaleUpperCase", native_string_upper, Generic, MAY_ALLOCATE;
    StringIsWellFormed, StringPrototype, "isWellFormed", native_string_is_well_formed, Generic, PURE;
    StringToWellFormed, StringPrototype, "toWellFormed", native_string_to_well_formed, Generic, MAY_ALLOCATE;
    StringLocaleCompare, StringPrototype, "localeCompare", native_string_locale_compare, Generic, MAY_ALLOCATE;
    NumberToFixed, NumberPrototype, "toFixed", native_number_to_fixed, Generic, MAY_ALLOCATE;
    NumberToPrecision, NumberPrototype, "toPrecision", native_number_to_precision, Generic, MAY_ALLOCATE;
    NumberToExponential, NumberPrototype, "toExponential", native_number_to_exponential, Generic, MAY_ALLOCATE;
    NumberToLocaleString, NumberPrototype, "toLocaleString", native_number_to_string, Generic, MAY_ALLOCATE;
    NumberValueOf, NumberPrototype, "valueOf", native_number_value_of, Generic, PURE;
    NumberToString, NumberPrototype, "toString", native_number_to_string, Generic, MAY_ALLOCATE;
    RegExpTest, RegExpPrototype, "test", native_regexp_test, Generic, PURE;
    RegExpExec, RegExpPrototype, "exec", native_regexp_exec, Generic, MAY_ALLOCATE;
    ObjectInheritsFrom, ObjectPrototype, "inheritsFrom", native_inherits_from, Generic, MAY_MUTATE;
    ObjectToString, ObjectPrototype, "toString", native_object_to_string, Generic, MAY_ALLOCATE;
    ObjectToLocaleString, ObjectPrototype, "toLocaleString", native_object_to_string, Generic, MAY_ALLOCATE;
    ObjectValueOf, ObjectPrototype, "valueOf", native_object_value_of, Generic, PURE;
    ObjectHasOwnProperty, ObjectPrototype, "hasOwnProperty", native_object_has_own_property, Generic, PURE;
    ObjectPropertyIsEnumerable, ObjectPrototype, "propertyIsEnumerable", native_object_property_is_enumerable, Generic, PURE;
    ObjectIsPrototypeOf, ObjectPrototype, "isPrototypeOf", native_object_is_prototype_of, Generic, PURE;
    ObjectDefineGetter, ObjectPrototype, "__defineGetter__", native_object_define_getter, Generic, MAY_MUTATE;
    ObjectDefineSetter, ObjectPrototype, "__defineSetter__", native_object_define_setter, Generic, MAY_MUTATE;
    ObjectLookupGetter, ObjectPrototype, "__lookupGetter__", native_object_lookup_getter, Generic, PURE;
    ObjectLookupSetter, ObjectPrototype, "__lookupSetter__", native_object_lookup_setter, Generic, PURE;
    FunctionCall, FunctionPrototype, "call", native_function_call, Generic, MAY_CALL_JS;
    FunctionApply, FunctionPrototype, "apply", native_function_apply, Generic, MAY_CALL_JS;
    FunctionBind, FunctionPrototype, "bind", native_function_bind, Generic, MAY_ALLOCATE;
}

pub(crate) fn instantiate(vm: &Vm) -> Box<[Value]> {
    BuiltinId::ALL
        .iter()
        .copied()
        .map(|id| {
            let name = id
                .recipe()
                .key
                .strip_prefix("Symbol(Symbol.")
                .and_then(|name| name.strip_suffix(')'))
                .map_or_else(
                    || id.recipe().key.to_owned(),
                    |name| format!("[Symbol.{name}]"),
                );
            let function = Rc::new(FunctionValue {
                kind: FunctionKind::Builtin(id),
                strict: false,
                prototype: vm.allocate_object(Object::ordinary(None)),
                props: Rc::new(RefCell::new(IndexMap::from([
                    ("name".to_string(), Value::string_value(name)),
                    (
                        "length".to_string(),
                        Value::Number(builtin_length(id) as f64),
                    ),
                ]))),
                attributes: Rc::new(RefCell::new(HashMap::new())),
                dyn_jit: RefCell::new(None),
                numeric_jit: RefCell::new(None),
                source_id: None,
            });
            Value::Function(function)
        })
        .collect()
}

fn builtin_length(id: BuiltinId) -> usize {
    match id {
        BuiltinId::StringFromCharCode
        | BuiltinId::StringFromCodePoint
        | BuiltinId::StringAt
        | BuiltinId::StringCodePointAt
        | BuiltinId::StringIncludes
        | BuiltinId::StringStartsWith
        | BuiltinId::StringEndsWith
        | BuiltinId::StringRepeat => 1,
        BuiltinId::ArrayPush
        | BuiltinId::ArrayUnshift
        | BuiltinId::ArrayJoin
        | BuiltinId::ArrayConcat
        | BuiltinId::ArrayForEach
        | BuiltinId::ArrayMap
        | BuiltinId::ArrayFilter
        | BuiltinId::ArraySome
        | BuiltinId::ArrayEvery
        | BuiltinId::ArrayIndexOf
        | BuiltinId::ArrayIncludes
        | BuiltinId::ArrayReduce
        | BuiltinId::ArrayReduceRight
        | BuiltinId::ArrayFind
        | BuiltinId::ArrayFindIndex => 1,
        BuiltinId::ArraySlice => 2,
        BuiltinId::ObjectDefineProperty => 3,
        BuiltinId::ObjectGetOwnPropertyDescriptor
        | BuiltinId::ObjectGetOwnPropertySymbols
        | BuiltinId::ObjectGetOwnPropertyDescriptors
        | BuiltinId::ObjectGetPrototypeOf
        | BuiltinId::ObjectKeys
        | BuiltinId::ObjectGetOwnPropertyNames
        | BuiltinId::ObjectPreventExtensions
        | BuiltinId::ObjectIsExtensible
        | BuiltinId::ObjectValues
        | BuiltinId::ObjectEntries
        | BuiltinId::ObjectIsSealed
        | BuiltinId::ObjectIsFrozen
        | BuiltinId::ObjectHasOwn
        | BuiltinId::ObjectIs
        | BuiltinId::ObjectSeal
        | BuiltinId::ObjectFreeze
        | BuiltinId::NumberIsFinite
        | BuiltinId::NumberIsInteger
        | BuiltinId::NumberIsNaN
        | BuiltinId::NumberIsSafeInteger
        | BuiltinId::NumberParseFloat
        | BuiltinId::NumberParseInt
        | BuiltinId::ObjectConstructor
        | BuiltinId::ArrayIsArray
        | BuiltinId::ArrayFrom
        | BuiltinId::ArrayConstructor
        | BuiltinId::StringConstructor
        | BuiltinId::NumberConstructor
        | BuiltinId::BooleanConstructor
        | BuiltinId::ErrorConstructor
        | BuiltinId::TypeErrorConstructor
        | BuiltinId::RangeErrorConstructor
        | BuiltinId::URIErrorConstructor
        | BuiltinId::SyntaxErrorConstructor
        | BuiltinId::ReferenceErrorConstructor
        | BuiltinId::EvalErrorConstructor
        | BuiltinId::AggregateErrorConstructor => 1,
        BuiltinId::FunctionConstructor | BuiltinId::FunctionBind | BuiltinId::FunctionCall => 1,
        BuiltinId::FunctionApply => 2,
        BuiltinId::ObjectCreate => 2,
        BuiltinId::ObjectAssign => 2,
        BuiltinId::ObjectDefineProperties => 2,
        BuiltinId::ObjectFromEntries => 1,
        BuiltinId::ObjectSetPrototypeOf => 2,
        BuiltinId::DateNow => 0,
        BuiltinId::DateGetYear => 0,
        BuiltinId::DateSetYear => 1,
        BuiltinId::DateParse => 1,
        BuiltinId::DateUTC => 7,
        BuiltinId::DateToJSON => 1,
        BuiltinId::ReflectApply => 3,
        BuiltinId::ReflectConstruct => 2,
        BuiltinId::ReflectDefineProperty => 3,
        BuiltinId::ReflectGetPrototypeOf
        | BuiltinId::ReflectIsExtensible
        | BuiltinId::ReflectOwnKeys
        | BuiltinId::ReflectPreventExtensions => 1,
        BuiltinId::ReflectDeleteProperty
        | BuiltinId::ReflectGetOwnPropertyDescriptor
        | BuiltinId::ReflectHas => 2,
        BuiltinId::ReflectGet => 2,
        BuiltinId::ReflectSet => 3,
        BuiltinId::ReflectSetPrototypeOf => 2,
        BuiltinId::MathAtan2 => 2,
        BuiltinId::MathSumPrecise => 1,
        BuiltinId::NumberToLocaleString => 0,
        BuiltinId::NumberValueOf => 0,
        BuiltinId::ParseInt
        | BuiltinId::MathPow
        | BuiltinId::MathMin
        | BuiltinId::MathMax
        | BuiltinId::MathHypot
        | BuiltinId::MathImul
        | BuiltinId::AssertStrictEqual
        | BuiltinId::AssertThrows
        | BuiltinId::SetTimeout
        | BuiltinId::SetImmediate => 2,
        BuiltinId::ParseFloat
        | BuiltinId::IsNaN
        | BuiltinId::IsFinite
        | BuiltinId::Eval
        | BuiltinId::DecodeURI
        | BuiltinId::DecodeURIComponent
        | BuiltinId::EncodeURI
        | BuiltinId::EncodeURIComponent
        | BuiltinId::MathFloor
        | BuiltinId::MathCeil
        | BuiltinId::MathSqrt
        | BuiltinId::MathAbs
        | BuiltinId::MathLog
        | BuiltinId::MathRound
        | BuiltinId::MathTrunc
        | BuiltinId::MathSign
        | BuiltinId::MathSin
        | BuiltinId::MathCos
        | BuiltinId::MathTan
        | BuiltinId::MathExp
        | BuiltinId::MathLog10
        | BuiltinId::MathLog2
        | BuiltinId::MathClz32
        | BuiltinId::MathFround
        | BuiltinId::MathF16Round
        | BuiltinId::MathAcos
        | BuiltinId::MathAsin
        | BuiltinId::MathAtan
        | BuiltinId::MathCbrt
        | BuiltinId::MathCosh
        | BuiltinId::MathSinh
        | BuiltinId::MathTanh
        | BuiltinId::MathAcosh
        | BuiltinId::MathAsinh
        | BuiltinId::MathAtanh
        | BuiltinId::MathExpm1
        | BuiltinId::MathLog1p => 1,
        BuiltinId::ArraySplice => 2,
        BuiltinId::ArrayFlatMap => 1,
        BuiltinId::NumberToFixed
        | BuiltinId::NumberToPrecision
        | BuiltinId::NumberToExponential
        | BuiltinId::NumberToString => 1,
        BuiltinId::ClearTimeout
        | BuiltinId::ProcessNextTick
        | BuiltinId::ArrayPop
        | BuiltinId::ArrayShift
        | BuiltinId::ArrayToString
        | BuiltinId::StringToLowerCase
        | BuiltinId::StringToUpperCase
        | BuiltinId::StringToString
        | BuiltinId::StringBig
        | BuiltinId::StringBlink
        | BuiltinId::StringBold
        | BuiltinId::StringFixed
        | BuiltinId::StringItalics
        | BuiltinId::StringSmall
        | BuiltinId::StringStrike
        | BuiltinId::StringSub
        | BuiltinId::StringSup
        | BuiltinId::StringTrim
        | BuiltinId::StringTrimStart
        | BuiltinId::StringTrimEnd
        | BuiltinId::StringTrimLeft
        | BuiltinId::StringTrimRight
        | BuiltinId::StringToLocaleLowerCase
        | BuiltinId::StringToLocaleUpperCase
        | BuiltinId::RegExpTest
        | BuiltinId::RegExpExec
        | BuiltinId::RegExpToString
        | BuiltinId::ObjectToString
        | BuiltinId::ObjectToLocaleString
        | BuiltinId::ObjectValueOf => 0,
        BuiltinId::StringSubstr => 2,
        BuiltinId::StringSubstring | BuiltinId::StringSlice => 2,
        BuiltinId::StringCharCodeAt | BuiltinId::StringCharAt => 1,
        BuiltinId::StringIndexOf | BuiltinId::StringLastIndexOf => 1,
        BuiltinId::StringReplace => 2,
        BuiltinId::StringSplit => 2,
        BuiltinId::StringMatch => 1,
        BuiltinId::StringConcat => 1,
        BuiltinId::StringReplaceAll => 2,
        BuiltinId::StringPadStart | BuiltinId::StringPadEnd => 2,
        BuiltinId::StringMatchAll | BuiltinId::StringSearch | BuiltinId::RegExpMatchAll => 1,
        BuiltinId::StringLocaleCompare => 1,
        BuiltinId::ArrayAt => 1,
        BuiltinId::ArrayFill => 3,
        BuiltinId::ArrayCopyWithin => 2,
        BuiltinId::ArrayWith => 2,
        BuiltinId::ArrayToSpliced => 2,
        BuiltinId::StringAnchor
        | BuiltinId::StringFontcolor
        | BuiltinId::StringFontsize
        | BuiltinId::StringLink
        | BuiltinId::Escape
        | BuiltinId::Unescape => 1,
        BuiltinId::RegExpCompile => 2,
        BuiltinId::ObjectIsPrototypeOf
        | BuiltinId::ObjectHasOwnProperty
        | BuiltinId::ObjectPropertyIsEnumerable
        | BuiltinId::ObjectLookupGetter
        | BuiltinId::ObjectLookupSetter => 1,
        BuiltinId::ObjectDefineGetter | BuiltinId::ObjectDefineSetter => 2,
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_identity_owner_and_dispatch_are_one_fact() {
        assert_eq!(BuiltinId::ALL.len(), BUILTIN_RECIPES.len());
        for (index, recipe) in BUILTIN_RECIPES.iter().enumerate() {
            assert_eq!(recipe.id as usize, index);
            assert_eq!(lookup(recipe.owner, recipe.key), Some(recipe.id));
            assert!(std::ptr::eq(recipe.id.recipe(), recipe));
        }
    }
}

use crate::{ops::Builtin, value::Value};

/// Lookup a property on a builtin, checking intl first, then special, then callable.
pub(crate) fn lookup(builtin: Builtin, key: &str) -> Value {
    if let Some(value) = crate::intl::property(builtin, key) {
        return value;
    }
    if let Some(value) = special(builtin, key) {
        return value;
    }
    if let Some(value) = callable(builtin, key) {
        return value;
    }
    Value::Undefined
}

fn special(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    if builtin == Math {
        return crate::math::property(key).map(Value::Builtin);
    }
    special_match(builtin, key)
}

fn special_match(builtin: Builtin, key: &str) -> Option<Value> {
    use Builtin::*;
    match (builtin, key) {
        (Symbol, "prototype") => Some(Value::Builtin(SymbolPrototype)),
        (Symbol, k) => crate::builtin_meta::symbol::symbol_prop(k).map(Value::Builtin),
        (MapPrototype | SetPrototype, k) => collections_prop(builtin, k).map(Value::Builtin),
        (DatePrototype, k) => crate::builtin_meta::date::date_prop(k).map(Value::Builtin),
        _ => builtin_method(builtin, key).map(Value::Builtin),
    }
}

fn builtin_method(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Function, "prototype") => Some(FunctionPrototype),
        (FunctionPrototype, "call") => Some(FunctionCall),
        (FunctionPrototype, "bind") => Some(FunctionBind),
        (FunctionCall, "bind") => Some(FunctionBind),
        (ArrayPrototype, "join") => Some(ArrayJoin),
        (ArrayPrototype, "push") => Some(ArrayPush),
        (Object, "prototype") => Some(ObjectPrototype),
        (ObjectPrototype, "hasOwnProperty") => Some(ObjectHasOwnProperty),
        (ObjectPrototype, "propertyIsEnumerable") => Some(ObjectPropertyIsEnumerable),
        (ObjectPrototype, "toString") => Some(ObjectPrototypeToString),
        (ObjectPrototype, "valueOf") => Some(ObjectPrototypeValueOf),
        (Date, "prototype") => Some(DatePrototype),
        (Reflect, "construct") => Some(ReflectConstruct),
        (RegExp, "prototype") => Some(RegExpPrototype),
        (RegExpPrototype, "test") => Some(RegExpTest),
        (RegExpPrototype, "exec") => Some(RegExpExec),
        _ => builtin_method2(builtin, key),
    }
}

fn builtin_method2(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Object, "defineProperty") => Some(ObjectDefineProperty),
        (Object, "getOwnPropertyNames") => Some(ObjectGetOwnPropertyNames),
        (Object, "create") => Some(ObjectCreate),
        (Object, "freeze") => Some(ObjectFreeze),
        (Object, "seal") => Some(ObjectSeal),
        (Object, "preventExtensions") => Some(ObjectPreventExtensions),
        (Object, "isFrozen") => Some(ObjectIsFrozen),
        (Object, "isSealed") => Some(ObjectIsSealed),
        (Object, "isExtensible") => Some(ObjectIsExtensible),
        (Object, "getPrototypeOf") => Some(ObjectGetPrototypeOf),
        (Object, "setPrototypeOf") => Some(ObjectSetPrototypeOf),
        (FunctionPrototype, "toString") => Some(FunctionPrototypeToString),
        (FunctionPrototype, "valueOf") => Some(FunctionPrototypeValueOf),
        _ => builtin_method3(builtin, key),
    }
}

fn builtin_method3(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Number, "prototype") => Some(NumberPrototype),
        (Boolean, "prototype") => Some(BooleanPrototype),
        #[allow(unreachable_patterns)]
        (Symbol, "prototype") => Some(SymbolPrototype),
        (String, "prototype") => Some(StringPrototype),
        (BigInt, "prototype") => Some(BigIntPrototype),
        _ => None,
    }
}

fn collections_prop(builtin: Builtin, key: &str) -> Option<Builtin> {
    crate::builtin_meta::collections::collections_property(builtin, key).and_then(|v| match v {
        Value::Builtin(b) => Some(b),
        _ => None,
    })
}

pub(crate) fn special_property(builtin: Builtin, key: &str) -> Option<Value> {
    special(builtin, key)
}

pub(crate) fn callable(builtin: Builtin, key: &str) -> Option<Value> {
    match key {
        "length" => Some(Value::Number(builtin_length(builtin))),
        "name" => Some(Value::String(builtin_name(builtin).to_string())),
        _ => None,
    }
}

fn builtin_length(builtin: Builtin) -> f64 {
    matches!(
        builtin,
        Builtin::Escape | Builtin::Unescape | Builtin::DateSetYear
    ) as i32 as f64
}

pub(crate) fn builtin_name(builtin: Builtin) -> &'static str {
    use Builtin::*;
    match builtin {
        Escape => "escape",
        Unescape => "unescape",
        Array => "Array",
        Object => "Object",
        String => "String",
        Symbol => "Symbol",
        Number => "Number",
        Date => "Date",
        DateGetYear => "getYear",
        DateSetYear => "setYear",
        RegExp => "RegExp",
        RegExpTest => "test",
        RegExpExec => "exec",
        _ => "",
    }
}

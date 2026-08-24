fn array_method(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "forEach" => Some(ArrayForEach),
        "map" => Some(ArrayMap),
        "filter" => Some(ArrayFilter),
        "some" => Some(ArraySome),
        "every" => Some(ArrayEvery),
        "find" => Some(ArrayFind),
        "findIndex" => Some(ArrayFindIndex),
        "findLast" => Some(ArrayFindLast),
        "findLastIndex" => Some(ArrayFindLastIndex),
        "includes" => Some(ArrayIncludes),
        "indexOf" => Some(ArrayIndexOf),
        "lastIndexOf" => Some(ArrayLastIndexOf),
        "slice" => Some(ArraySlice),
        "concat" => Some(ArrayConcat),
        "flat" => Some(ArrayFlat),
        _ => array_method_tail(key),
    }
}

fn array_method_tail(key: &str) -> Option<Builtin> {
    use Builtin::*;
    match key {
        "flatMap" => Some(ArrayFlatMap),
        "at" => Some(ArrayAt),
        "toReversed" => Some(ArrayToReversed),
        "join" => Some(ArrayJoin),
        "toString" => Some(ArrayToString),
        "reduce" => Some(ArrayReduce),
        "reduceRight" => Some(ArrayReduceRight),
        "toLocaleString" => Some(ArrayToLocaleString),
        "values" => Some(ArrayIterator),
        "Symbol.iterator" => Some(ArrayIterator),
        "keys" => Some(ArrayKeys),
        "entries" => Some(ArrayEntries),
        "push" => Some(ArrayPush),
        "shift" => Some(ArrayShift),
        "reverse" => Some(ArrayReverse),
        "sort" => Some(ArraySort),
        "pop" => Some(ArrayPop),
        "unshift" => Some(ArrayUnshift),
        "fill" => Some(ArrayFill),
        "copyWithin" => Some(ArrayCopyWithin),
        "toSorted" => Some(ArrayToSorted),
        "splice" => Some(ArraySplice),
        _ => None,
    }
}
fn builtin_method2(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Object, "defineProperty") => Some(ObjectDefineProperty),
        (Object, "defineProperties") => Some(ObjectDefineProperties),
        (Object, "getOwnPropertyDescriptor") => Some(ObjectGetOwnPropertyDescriptor),
        (Object, "getOwnPropertyDescriptors") => Some(ObjectGetOwnPropertyDescriptors),
        (Object, "keys") => Some(ObjectKeys),
        (Object, "values") => Some(ObjectValues),
        (Object, "entries") => Some(ObjectEntries),
        (Object, "hasOwn") => Some(ObjectHasOwn),
        (Object, "getOwnPropertyNames") => Some(ObjectGetOwnPropertyNames),
        (Object, "getOwnPropertySymbols") => Some(ObjectGetOwnPropertySymbols),
        (Object, "create") => Some(ObjectCreate),
        (Object, "freeze") => Some(ObjectFreeze),
        (Object, "seal") => Some(ObjectSeal),
        (Object, "preventExtensions") => Some(ObjectPreventExtensions),
        (Object, "isFrozen") => Some(ObjectIsFrozen),
        (Object, "isSealed") => Some(ObjectIsSealed),
        (Object, "isExtensible") => Some(ObjectIsExtensible),
        (Object, "getPrototypeOf") => Some(ObjectGetPrototypeOf),
        (Object, "is") => Some(ObjectIs),
        (Object, "assign") => Some(ObjectAssign),
        (Object, "setPrototypeOf") => Some(ObjectSetPrototypeOf),
        (Map, "prototype") => Some(MapPrototype),
        (Set, "prototype") => Some(SetPrototype),
        (FunctionPrototype, "toString") => Some(FunctionPrototypeToString),
        (FunctionPrototype, "valueOf") => Some(FunctionPrototypeValueOf),
        (FunctionPrototype, "Symbol.hasInstance") => Some(FunctionPrototypeHasInstance),
        (RegExpPrototype, "toString") => Some(RegExpPrototypeToString),
        _ => builtin_method3(builtin, key),
    }
}
fn builtin_method3(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Number, "prototype") => Some(NumberPrototype),
        (NumberPrototype, "toLocaleString") => Some(NumberToLocaleString),
        (NumberPrototype, "toString") => Some(NumberToString),
        (NumberPrototype, "valueOf") => Some(NumberValueOf),
        (NumberPrototype, "toFixed") => Some(NumberToFixed),
        (NumberPrototype, "toPrecision") => Some(NumberToPrecision),
        (NumberPrototype, "toExponential") => Some(NumberToExponential),
        (Number, "isNaN") => Some(IsNaN),
        (Number, "isFinite") => Some(IsFinite),
        (Number, key @ ("isInteger" | "isSafeInteger")) => Some(if key == "isInteger" {
            NumberIsInteger
        } else {
            NumberIsSafeInteger
        }),
        _ => builtin_method3_tail(builtin, key),
    }
}

fn builtin_method3_tail(builtin: Builtin, key: &str) -> Option<Builtin> {
    use Builtin::*;
    match (builtin, key) {
        (Boolean, "prototype") => Some(BooleanPrototype),
        (BooleanPrototype, "valueOf") => Some(BooleanValueOf),
        (BooleanPrototype, "toString") => Some(BooleanToString),
        (BooleanPrototype, "constructor") => Some(Boolean),
        (NumberPrototype, "constructor") => Some(Number),
        (ObjectPrototype, "constructor") => Some(Object),
        (ObjectPrototype, "toLocaleString") => Some(ObjectPrototypeToString),
        (TemporalDurationPrototype, "toJSON") => Some(TemporalDurationToJSON),
        (TemporalPlainDatePrototype, "toString") => Some(TemporalPlainDateToString),
        (TemporalPlainDatePrototype, "toJSON") => Some(TemporalPlainDateToJSON),
        (Symbol, "prototype") => Some(SymbolPrototype),
        (SymbolPrototype, "toString") => Some(SymbolToString),
        (SymbolPrototype, "valueOf") => Some(SymbolValueOf),
        (SymbolPrototype, "Symbol.toPrimitive") => Some(SymbolPrototypeToPrimitive),
        (SymbolPrototype, "constructor") => Some(Symbol),
        (String, "prototype") => Some(StringPrototype),
        (StringPrototype, "valueOf") => Some(StringValueOf),
        (BigInt, "prototype") => Some(BigIntPrototype),
        (BigInt, "asIntN") => Some(BigIntAsIntN),
        (BigInt, "asUintN") => Some(BigIntAsUintN),
        (BigIntPrototype, "valueOf") => Some(BigIntValueOf),
        (BigIntPrototype, "constructor") => Some(BigInt),
        (BigIntPrototype, "toString") => Some(BigIntToString),
        (BigIntPrototype, "toLocaleString") => Some(BigIntToLocaleString),
        _ => None,
    }
}

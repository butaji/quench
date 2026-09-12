use super::*;

pub(crate) type NativeSemantic = fn(&mut Vm, Value, &[Value]) -> JsResult<Value>;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum BuiltinOwner {
    Global,
    Math,
    Console,
    ArrayPrototype,
    StringPrototype,
    NumberPrototype,
    RegExpPrototype,
    ObjectPrototype,
    FunctionPrototype,
    Assert,
    Process,
    StringConstructor,
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
    IsNaN, Global, "isNaN", native_is_nan, UnaryNumber, PURE;
    MathPow, Math, "pow", native_math_pow, BinaryNumber, PURE;
    MathFloor, Math, "floor", native_math_floor, UnaryNumber, PURE;
    MathCeil, Math, "ceil", native_math_ceil, UnaryNumber, PURE;
    MathSqrt, Math, "sqrt", native_math_sqrt, UnaryNumber, PURE;
    MathAbs, Math, "abs", native_math_abs, UnaryNumber, PURE;
    MathMin, Math, "min", native_math_min, VariadicNumber, PURE;
    MathMax, Math, "max", native_math_max, VariadicNumber, PURE;
    MathLog, Math, "log", native_math_log, UnaryNumber, MAY_CALL_JS;
    MathRound, Math, "round", native_math_round, UnaryNumber, PURE;
    MathRandom, Math, "random", native_random, Generic, PURE;
    ObjectConstructor, Global, "Object", native_object, Generic, MAY_ALLOCATE;
    ArrayConstructor, Global, "Array", native_array, Generic, MAY_ALLOCATE;
    StringConstructor, Global, "String", native_string, Generic, MAY_ALLOCATE;
    NumberConstructor, Global, "Number", native_number, Generic, PURE;
    DateConstructor, Global, "Date", native_date, Generic, PURE;
    RegExpConstructor, Global, "RegExp", native_regexp, Generic, MAY_ALLOCATE;
    ErrorConstructor, Global, "Error", native_error, Generic, MAY_ALLOCATE;
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
    ArrayPush, ArrayPrototype, "push", native_array_push, Generic, MAY_MUTATE;
    ArrayPop, ArrayPrototype, "pop", native_array_pop, Generic, MAY_MUTATE;
    ArrayShift, ArrayPrototype, "shift", native_array_shift, Generic, MAY_MUTATE;
    ArrayUnshift, ArrayPrototype, "unshift", native_array_unshift, Generic, MAY_MUTATE;
    ArraySlice, ArrayPrototype, "slice", native_array_slice, Generic, MAY_ALLOCATE;
    ArrayJoin, ArrayPrototype, "join", native_array_join, Generic, MAY_ALLOCATE;
    ArrayConcat, ArrayPrototype, "concat", native_array_concat, Generic, MAY_ALLOCATE;
    StringSubstring, StringPrototype, "substring", native_string_substring, Generic, MAY_ALLOCATE;
    StringSlice, StringPrototype, "slice", native_string_slice, Generic, MAY_ALLOCATE;
    StringCharCodeAt, StringPrototype, "charCodeAt", native_string_char_code_at, Generic, PURE;
    StringCharAt, StringPrototype, "charAt", native_string_char_at, Generic, MAY_ALLOCATE;
    StringSubstr, StringPrototype, "substr", native_string_substr, Generic, MAY_ALLOCATE;
    StringToLowerCase, StringPrototype, "toLowerCase", native_string_lower, Generic, MAY_ALLOCATE;
    StringToUpperCase, StringPrototype, "toUpperCase", native_string_upper, Generic, MAY_ALLOCATE;
    StringToString, StringPrototype, "toString", native_string_to_string, Generic, PURE;
    StringConcat, StringPrototype, "concat", native_string_concat, Generic, MAY_ALLOCATE;
    StringReplace, StringPrototype, "replace", native_string_replace, Generic, MAY_ALLOCATE;
    StringSplit, StringPrototype, "split", native_string_split, Generic, MAY_ALLOCATE;
    StringMatch, StringPrototype, "match", native_string_match, Generic, MAY_ALLOCATE;
    StringIndexOf, StringPrototype, "indexOf", native_string_index_of, Generic, PURE;
    StringLastIndexOf, StringPrototype, "lastIndexOf", native_string_last_index_of, Generic, PURE;
    NumberToFixed, NumberPrototype, "toFixed", native_number_to_fixed, Generic, MAY_ALLOCATE;
    NumberToPrecision, NumberPrototype, "toPrecision", native_number_to_precision, Generic, MAY_ALLOCATE;
    NumberToString, NumberPrototype, "toString", native_number_to_string, Generic, MAY_ALLOCATE;
    RegExpTest, RegExpPrototype, "test", native_regexp_test, Generic, PURE;
    RegExpExec, RegExpPrototype, "exec", native_regexp_exec, Generic, MAY_ALLOCATE;
    ObjectInheritsFrom, ObjectPrototype, "inheritsFrom", native_inherits_from, Generic, MAY_MUTATE;
    ObjectToString, ObjectPrototype, "toString", native_object_to_string, Generic, MAY_ALLOCATE;
    ObjectValueOf, ObjectPrototype, "valueOf", native_object_value_of, Generic, PURE;
    FunctionCall, FunctionPrototype, "call", native_function_call, Generic, MAY_CALL_JS;
    FunctionApply, FunctionPrototype, "apply", native_function_apply, Generic, MAY_CALL_JS;
}

pub(crate) fn instantiate(vm: &Vm) -> Box<[Value]> {
    BuiltinId::ALL
        .iter()
        .copied()
        .map(|id| {
            Value::Function(Rc::new(FunctionValue {
                kind: FunctionKind::Builtin(id),
                prototype: vm.allocate_object(Object::ordinary(None)),
                props: Rc::new(RefCell::new(IndexMap::new())),
                dyn_jit: RefCell::new(None),
                numeric_jit: RefCell::new(None),
                source_id: None,
            }))
        })
        .collect()
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

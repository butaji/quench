//! Every tier's one table of native calls that complete in place: no guest code, no raised
//! error. The lane and the interpreter both read it (`Vm::in_place_native_value`); a case
//! may allocate, and each caller notes that its own way.
use super::*;
use crate::vm::object_get::ordinary_chain::ChainRead;

impl Vm {
    /// A native call's value where its common case completes in place: the steady cases
    /// first (`Vm::steady_native_value`), then Math, the pure string methods, objects, collections
    /// and arrays, and primitives. `None` leaves the call to the native itself.
    #[inline(always)]
    pub(in crate::vm) fn in_place_native_value(&mut self, code: &ResidualProgram, native: Native, callee: Value,
        receiver: Value, args: &[Value]) -> Option<Value> {
        self.in_place_native_value_inline(code, native, callee, receiver, args)
    }

    /// `in_place_native_value` inlined into its caller: the lane's native call
    /// (`string_or_native_call_in_lane`) enters the family function from its own frame.
    #[inline(always)]
    pub(in crate::vm) fn in_place_native_value_inline(&mut self, code: &ResidualProgram, native: Native, callee: Value,
        receiver: Value, args: &[Value]) -> Option<Value> {
        // One table read selects the families that can answer `native` (`IN_PLACE_FAMILIES`),
        // tried in the same order the cascade of all eight tried them.
        let families = IN_PLACE_FAMILIES[native as usize];
        // The steady cases are the common ones: tried here, in the caller, so they do not carry
        // the frame of the whole cascade (`in_place_family_value`).
        if families & STEADY != 0 && let Some(value) = self.steady_native_value(native, receiver, args) {
            return Some(value);
        }
        if (families & !STEADY) == 0 {
            return None;
        }
        in_place_family_value(self, code, native, callee, receiver, args, families)
    }

    /// `value`, the in-place body's answer for `native`. Where that body declined (`None`), the
    /// call goes on to `native`'s general call with `declined_fast` naming it, so the general
    /// native skips the same body a second time.
    #[inline(always)]
    pub(in crate::vm) fn fast_declined(&mut self, native: Native, value: Option<Value>) -> Option<Value> {
        if value.is_none() {
            self.declined_fast = Some(native);
        }
        value
    }

    /// `fast_declined` for `indexOf` and `includes`, whose dense walk (`dense_search_resume`)
    /// can stop at a hole with the general steps still to run: the walk's index is recorded
    /// with `declined_fast`, so the general native resumes there.
    #[inline(always)]
    pub(in crate::vm) fn fast_search_declined(&mut self, native: Native, found: Result<Value, Option<usize>>) -> Option<Value> {
        match found {
            Ok(value) => Some(value),
            Err(resume) => {
                self.declined_fast = Some(native);
                self.declined_resume = resume;
                None
            }
        }
    }
}

/// `in_place_native_value` after its steady case: a native's in-place row (`in_place_row_value`),
/// else the families of `families` other than `STEADY`, in order, each answering or declining.
/// Out of line, so the steady case inlined into the caller does not carry this cascade's frame.
#[inline(never)]
fn in_place_family_value(vm: &mut Vm, code: &ResidualProgram, native: Native, callee: Value,
    receiver: Value, args: &[Value], families: u16) -> Option<Value> {
    // A native with an in-place row answers from its row's body alone (`in_place_row_value`).
    if families & ROW != 0 {
        return super::native_calls::in_place_row_value(vm, code, native, receiver, args);
    }
    let tries = |family: u16| families & family != 0;
    if tries(MATH) && let Some(value) = math_value(native, args) {
        return Some(value);
    }
    if tries(PURE_STRING) && let Some(value) = vm.pure_string_method_call(code, native, receiver, args) {
        return Some(value);
    }
    if tries(OBJECT) && let Some(value) = object_value(vm, code, native, receiver, args) {
        return Some(value);
    }
    if tries(COLLECTION) && let Some(value) = collection_value(vm, code, native, receiver, args) {
        return Some(value);
    }
    if tries(ARRAY) && let Some(value) = array_value(vm, code, native, callee, receiver, args) {
        return Some(value);
    }
    if tries(INTERNAL_SLOT) && let Some(value) = internal_slot_value(vm, native, callee, receiver) {
        return Some(value);
    }
    if tries(PRIMITIVE) && let Some(value) = primitive_value(vm, native, receiver, args) {
        return Some(value);
    }
    if tries(PRIMITIVE_LEAF) {
        return primitive_leaf_value(vm, code, native, receiver, args);
    }
    None
}

// The in-place families, one bit each, in the order `in_place_native_value` tries them.
const STEADY: u16 = 1 << 0;
const MATH: u16 = 1 << 1;
const PURE_STRING: u16 = 1 << 2;
const OBJECT: u16 = 1 << 3;
const COLLECTION: u16 = 1 << 4;
const ARRAY: u16 = 1 << 5;
const INTERNAL_SLOT: u16 = 1 << 6;
const PRIMITIVE: u16 = 1 << 7;
const PRIMITIVE_LEAF: u16 = 1 << 8;
/// A native with an in-place row (`native_calls::in_place_row`): its row's body answers it alone.
const ROW: u16 = 1 << 9;

/// The families whose functions below can answer `native`: each family's arms, listed
/// beside the function that holds them. A native outside every family completes through
/// the native itself, with no in-place attempt. A native with an in-place row is answered by
/// that row alone, so its family arms are not listed here.
const fn in_place_families(native: Native) -> u16 {
    if super::native_calls::in_place_row(native).is_some() {
        return ROW;
    }
    let mut families = 0;
    // `Vm::steady_native_value`.
    if matches!(native, Native::ArrayPush | Native::ArrayPop | Native::StringCharCodeAt | Native::StringCharAt
        | Native::MapGet | Native::MapHas | Native::SetHas | Native::WeakMapGet | Native::WeakMapHas | Native::WeakSetHas
        | Native::MapSet | Native::ArrayIsArray | Native::JsonStringify | Native::DateNow)
    {
        families |= STEADY;
    }
    if native.is_math_function() {
        families |= MATH;
    }
    if native.is_pure_string_method() {
        families |= PURE_STRING;
    }
    if matches!(native, Native::ObjectCreate | Native::ObjectPrototypeHasOwnProperty
        | Native::ObjectPrototypePropertyIsEnumerable | Native::ObjectHasOwn | Native::ObjectIs | Native::ReflectGet
        | Native::ReflectHas | Native::ObjectValues | Native::ObjectEntries
        | Native::ReflectGetPrototypeOf | Native::ObjectPrototypeToString | Native::SymbolFor
        | Native::ObjectGetPrototypeOf)
    {
        families |= OBJECT;
    }
    if matches!(native, Native::MapSet | Native::MapDelete | Native::MapClear | Native::SetAdd | Native::SetDelete
        | Native::SetClear | Native::ArrayBufferIsView | Native::TypedArrayAt | Native::Uint8ArrayIndexOf
        | Native::Uint8ArrayIncludes | Native::Uint8ArrayFill)
        || super::super::data_view::data_view_element(native)
        || super::super::date::date_reads(native)
    {
        families |= COLLECTION;
    }
    if matches!(native, Native::ArraySplice | Native::ArrayUnshift
        | Native::ArrayShift | Native::ArrayIteratorNext | Native::ArrayFrom | Native::ArrayOf | Native::ArrayLastIndexOf
        | Native::ArrayReverse | Native::ArrayToReversed | Native::ArrayWith)
    {
        families |= ARRAY;
    }
    if matches!(native, Native::MapKeys | Native::MapValues | Native::MapEntries | Native::IteratorClose
        | Native::StringToString | Native::StringValueOf | Native::FunctionToString | Native::RegExpLegacyGetter)
    {
        families |= INTERNAL_SLOT;
    }
    if matches!(native, Native::StringFromCharCode | Native::Boolean | Native::String | Native::Number
        | Native::NumberString | Native::StringLocaleCompare | Native::GlobalIsNaN | Native::GlobalIsFinite
        | Native::NumberIsNaN | Native::NumberIsFinite | Native::NumberIsInteger | Native::NumberIsSafeInteger
        | Native::NumberFixed)
    {
        families |= PRIMITIVE;
    }
    if primitive_leaf(native) || string_leaf(native) {
        families |= PRIMITIVE_LEAF;
    }
    families
}

/// Natives of `Vm::call_primitive_native` that, for a primitive receiver and arguments that
/// are not objects, run no guest code, observe no frame and have no effect beyond allocating
/// their result: their conversions of primitives run none.
const fn primitive_leaf(native: Native) -> bool {
    matches!(native, Native::ParseInt | Native::NumberParseFloat | Native::NumberString | Native::NumberValueOf
        | Native::BooleanToString | Native::BooleanValueOf | Native::BigIntValueOf | Native::StringFromCodePoint
        | Native::EncodeUri | Native::EncodeUriComponent | Native::DecodeUri | Native::DecodeUriComponent
        | Native::GlobalEscape | Native::GlobalUnescape)
}

/// `String.prototype` methods of `Vm::call_primitive_native` that, for a string receiver and
/// arguments that are not objects, run no guest code: `replace` and `replaceAll` look
/// `Symbol.replace` up only on an object pattern (ES2025), and a pattern that is not an object
/// makes no RegExp.
const fn string_leaf(native: Native) -> bool {
    matches!(native, Native::StringReplace | Native::StringReplaceAll | Native::StringNormalize
        | Native::StringIsWellFormed | Native::StringToWellFormed)
}

/// A `primitive_leaf` or `string_leaf` native's call made straight into its body, without the
/// general path's native activation and roots: none is observable, since the body runs no guest
/// code and no collection. A call that fails had no effect, so the general path repeats it to
/// raise its error under its activation.
fn primitive_leaf_value(vm: &mut Vm, code: &ResidualProgram, native: Native, receiver: Value,
    args: &[Value]) -> Option<Value> {
    // Immediates, strings and BigInts: values whose conversions run no guest code.
    let primitive = |value: Value| !value.is_heap() || vm.heap.is_string(value) || matches!(vm.heap.get(value), Some(Cell::BigInt(_)));
    if !primitive(receiver) || !args.iter().all(|argument| primitive(*argument))
        || (string_leaf(native) && !vm.heap.is_string(receiver))
    {
        return None;
    }
    let result = vm.call_primitive_native(code, native, receiver, args).ok();
    // A string method reads its receiver as text, which it may make on demand.
    vm.heap.charge_materialized_text();
    result
}

/// The catalog fact the lean native rows derive from (`native_calls::in_place_handler`): whether
/// `native` has an in-place body, which runs no guest code, raises nothing and observes no frame.
pub(super) const fn runs_no_guest_code(native: Native) -> bool {
    IN_PLACE_FAMILIES[native as usize] != 0
}

/// `in_place_families` of every native, by discriminant.
const IN_PLACE_FAMILIES: [u16; Native::COUNT as usize] = {
    let mut table = [0; Native::COUNT as usize];
    let mut index = 0;
    while index < Native::COUNT {
        table[index as usize] = in_place_families(Native::from_discriminant(index).unwrap());
        index += 1;
    }
    table
};

/// A Math function of Numbers, which runs no guest code and observes no frame
/// (`number::math_inline_call`, and `math_numbers` of up to `MATH_INLINE_ARGUMENTS`).
fn math_value(native: Native, args: &[Value]) -> Option<Value> {
    if !native.is_math_function() {
        return None;
    }
    super::super::number::math_inline_call(native, args).or_else(|| {
        // Longer calls leave to the native, which converts them.
        let mut numbers = [0.0; super::super::number::MATH_INLINE_ARGUMENTS];
        let used = numbers.get_mut(..args.len())?;
        for (number, value) in used.iter_mut().zip(args) {
            *number = value.as_number()?;
        }
        super::super::number::math_numbers(native, used).map(Value::number)
    })
}

/// Object queries whose steps observe nothing: `Object.create` without properties, an own
/// property query by a string key (`Vm::plain_own_property_query`), `Object.is`, an object's
/// values or entries its shape lists (`Vm::plain_enumerable_own_properties`), `super`
/// reads (`Reflect.get` and `Reflect.getPrototypeOf` of an ordinary target, whose data
/// property read ignores the receiver), `Symbol.for` of a string and `Object.prototype.toString`
/// (`object_to_string_in_place`). `Object.keys` and `Object.assign` are rows (`native_calls`).
fn object_value(vm: &mut Vm, code: &ResidualProgram, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
    let argument = args.first().copied().unwrap_or(Value::UNDEFINED);
    match native {
        Native::ObjectCreate if args.get(1).is_none_or(|properties| properties.is_undefined()) => vm.object_create(argument),
        Native::ObjectPrototypeHasOwnProperty | Native::ObjectPrototypePropertyIsEnumerable =>
            vm.plain_own_property_query(code, native, receiver, *args.first()?),
        Native::ObjectHasOwn => vm.plain_own_property_query(code, Native::ObjectPrototypeHasOwnProperty, *args.first()?, *args.get(1)?),
        // Pure of any arguments: SameValue.
        Native::ObjectIs => vm.call_object_native(code, Native::ObjectIs, args).ok(),
        Native::ReflectGet | Native::ReflectHas => reflect_value(vm, native, args),
        // An object's own enumerable properties, where its shape lists them.
        _ if let Some(kind) = super::super::object_keys::EnumerableOwnPropertyKind::of(native) => {
            let listed = vm.plain_enumerable_own_properties(argument, kind);
            vm.fast_declined(native, listed)
        }
        Native::ReflectGetPrototypeOf => Some(vm.heap.ordinary_object(*args.first()?)?.proto()),
        Native::ObjectGetPrototypeOf => {
            let prototype = vm.plain_object_get_prototype_of(args);
            vm.fast_declined(native, prototype)
        }

        Native::ObjectPrototypeToString => object_to_string_in_place(vm, code, receiver),
        // The registry lookup of a string key, which ToString leaves as it is.
        Native::SymbolFor if args.first().is_some_and(|key| vm.heap.is_string(*key)) =>
            vm.call_symbol_native(code, Native::SymbolFor, args).ok(),
        _ => None,
    }
}

/// `Reflect.get` and `Reflect.has` of a target whose chain shapes decide the answer
/// (`Vm::reflect_chain_read`); a symbol or index key of an ordinary target reads as a plain
/// keyed read. An accessor is a present property, whose getter only `get` calls.
fn reflect_value(vm: &mut Vm, native: Native, args: &[Value]) -> Option<Value> {
    let (target, key) = (*args.first()?, *args.get(1)?);
    if native == Native::ReflectGet && !vm.heap.is_string(key) {
        vm.heap.ordinary_object(target)?;
        return vm.plain_keyed_get(target, key);
    }
    match (native, vm.reflect_chain_read(target, key)?) {
        (_, ChainRead::Exotic(_)) | (Native::ReflectGet, ChainRead::Getter(_)) => None,
        (Native::ReflectGet, ChainRead::Value(value)) => Some(value),
        (Native::ReflectGet, _) => Some(Value::UNDEFINED),
        (_, ChainRead::Absent) => Some(Value::FALSE),
        _ => Some(Value::TRUE),
    }
}

/// `Object.prototype.toString` of `undefined`, `null`, or an object whose `@@toStringTag`
/// a plain lookup reads (`Vm::plain_keyed_get`, which declines accessors on the chain): its
/// brand and its tag then run no guest code. A proxy, whose brand asks `IsArray` of its
/// target, and a primitive, which the tag reads through its wrapper, take the native.
fn object_to_string_in_place(vm: &mut Vm, code: &ResidualProgram, receiver: Value) -> Option<Value> {
    if receiver.is_heap() {
        if matches!(vm.heap.get(receiver), Some(Cell::Proxy { .. })) || !vm.is_object_like(receiver) {
            return None;
        }
        let tag = vm.well_known_symbols.get(super::super::WellKnownSymbol::ToStringTag)?;
        vm.plain_keyed_get(receiver, tag)?;
    } else if !receiver.is_undefined() && !receiver.is_null() {
        return None;
    }
    vm.object_prototype_to_string(code, receiver).ok()
}

/// Steps of collections and built-in records on their own receivers: the Map and Set
/// mutators (`Vm::plain_collection_mutation`, a dense `Map#set` first), a DataView element
/// get or set that raises nothing, a Date getter of a Date, and `ArrayBuffer.isView`.
fn collection_value(vm: &mut Vm, code: &ResidualProgram, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
    match COLLECTION_ARMS[native as usize] {
        MAP_SET_ARM if vm.map_set(receiver, args) => Some(receiver),
        MAP_SET_ARM | MUTATION_ARM => vm.plain_collection_mutation(code, native, receiver, args),
        ARRAY_BUFFER_IS_VIEW_ARM => Some(Value::boolean(vm.is_array_buffer_view(args.first().copied().unwrap_or(Value::UNDEFINED)))),
        DATA_VIEW_ARM => vm.plain_data_view_access(native, receiver, args),
        DATE_ARM => vm.plain_date_get(native, receiver),
        TYPED_AT_ARM => vm.plain_typed_at(native, receiver, args),
        TYPED_INDEX_OF_ARM => vm.plain_typed_index_of(native, receiver, args),
        TYPED_INCLUDES_ARM => vm.plain_typed_includes(native, receiver, args),
        TYPED_FILL_ARM => vm.plain_typed_fill(native, receiver, args),
        _ => None,
    }
}

// The arms of `collection_value`, one code each, in `COLLECTION_ARMS`.
const NO_COLLECTION_ARM: u8 = 0;
/// `Map.prototype.set`: its dense step first, then the general mutation.
const MAP_SET_ARM: u8 = 1;
/// The Map and Set mutators other than `set` (`plain_collection_mutation`).
const MUTATION_ARM: u8 = 2;
const ARRAY_BUFFER_IS_VIEW_ARM: u8 = 3;
const DATA_VIEW_ARM: u8 = 4;
const DATE_ARM: u8 = 5;
const TYPED_AT_ARM: u8 = 6;
const TYPED_INDEX_OF_ARM: u8 = 7;
const TYPED_INCLUDES_ARM: u8 = 8;
const TYPED_FILL_ARM: u8 = 9;

/// The `collection_value` arm of `native`, or `NO_COLLECTION_ARM`.
const fn collection_arm(native: Native) -> u8 {
    if matches!(native, Native::MapSet) {
        MAP_SET_ARM
    } else if matches!(native, Native::MapDelete | Native::MapClear | Native::SetAdd | Native::SetDelete | Native::SetClear) {
        MUTATION_ARM
    } else if matches!(native, Native::ArrayBufferIsView) {
        ARRAY_BUFFER_IS_VIEW_ARM
    } else if super::super::data_view::data_view_element(native) {
        DATA_VIEW_ARM
    } else if super::super::date::date_reads(native) {
        DATE_ARM
    } else if matches!(native, Native::TypedArrayAt) {
        TYPED_AT_ARM
    } else if matches!(native, Native::Uint8ArrayIndexOf) {
        TYPED_INDEX_OF_ARM
    } else if matches!(native, Native::Uint8ArrayIncludes) {
        TYPED_INCLUDES_ARM
    } else if matches!(native, Native::Uint8ArrayFill) {
        TYPED_FILL_ARM
    } else {
        NO_COLLECTION_ARM
    }
}

/// `collection_arm` of every native, by discriminant: one table read selects the arm.
const COLLECTION_ARMS: [u8; Native::COUNT as usize] = {
    let mut table = [NO_COLLECTION_ARM; Native::COUNT as usize];
    let mut index = 0;
    while index < Native::COUNT {
        table[index as usize] = collection_arm(Native::from_discriminant(index).unwrap());
        index += 1;
    }
    table
};

/// Natives that read the receiver's own record or the realm's state: a string's or function's text
/// (`String.prototype.toString`, `Function.prototype.toString`), a Map's iterators
/// (`Vm::collection_iterator` of a Map, which allocates the iterator), an iterator's close where
/// it has no `return` (`Vm::iterator_close_in_place`), and a legacy `RegExp` static getter
/// (`Vm::regexp_legacy_getter_in_place`). Any other receiver leaves the native to throw.
fn internal_slot_value(vm: &mut Vm, native: Native, callee: Value, receiver: Value) -> Option<Value> {
    use crate::heap::IteratorKind;
    let kind = match native {
        Native::MapKeys => IteratorKind::MapKeys,
        Native::MapValues => IteratorKind::MapValues,
        Native::MapEntries => IteratorKind::MapEntries,
        Native::IteratorClose => return vm.iterator_close_in_place(receiver),
        Native::StringToString | Native::StringValueOf => return vm.heap.is_string(receiver).then_some(receiver),
        Native::FunctionToString => return vm.is_function(receiver).then(|| vm.function_source_value(receiver)).flatten(),
        Native::RegExpLegacyGetter => return vm.regexp_legacy_getter_in_place(callee, receiver),
        _ => return None,
    };
    vm.heap.is_map(receiver).then(|| vm.collection_iterator(receiver, kind).ok()).flatten()
}

/// A dense array's `splice`, `unshift`, `shift` and `lastIndexOf`, and the array-building and
/// reversing methods over a plain array, and an array iterator's step over a plain array.
/// The rows of `native_calls` answer the others (`in_place_row_value`).
fn array_value(vm: &mut Vm, code: &ResidualProgram, native: Native, callee: Value, receiver: Value,
    args: &[Value]) -> Option<Value> {
    match native {
        Native::ArraySplice => vm.plain_array_splice(code, callee, receiver, args),
        Native::ArrayUnshift => vm.plain_array_unshift(receiver, args),
        Native::ArrayShift => vm.plain_array_shift(receiver),
        Native::ArrayIteratorNext => vm.plain_array_iterator_next(callee, receiver),
        Native::ArrayFrom => vm.plain_array_from(receiver, args),
        Native::ArrayOf => vm.plain_array_of(receiver, args),
        Native::ArrayLastIndexOf => vm.plain_array_last_index_of(receiver, args),
        Native::ArrayReverse => vm.plain_array_reverse(receiver, args),
        Native::ArrayToReversed => vm.plain_array_to_reversed(receiver, args),
        Native::ArrayWith => {
            let with = vm.plain_array_with(receiver, args);
            vm.fast_declined(native, with)
        }
        _ => None,
    }
}

/// Conversions and predicates of primitives whose steps read no cell but a string's text:
/// `String.fromCharCode` of numbers, `Boolean`, `String` of a string or an immediate
/// primitive, `Number` of a primitive, decimal `Number.prototype.toString`, the global
/// `isNaN` and `isFinite`, the `Number` predicates (`number::number_predicate`), and a
/// default-collation `localeCompare` of two strings (`default_locale_compare`).
fn primitive_value(vm: &mut Vm, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
    let argument = args.first().copied().unwrap_or(Value::UNDEFINED);
    match native {
        Native::StringFromCharCode => vm.from_char_code_numbers(args),
        Native::Boolean => Some(Value::boolean(vm.truthy(argument))),
        Native::String if vm.heap.string(argument).is_some() => Some(argument),
        Native::String if !args.is_empty() => Some(vm.heap.alloc_string(super::super::coercion::immediate_to_string(argument)?)),
        Native::Number if !args.is_empty() => vm.primitive_to_number(argument).map(Value::number),
        Native::NumberString if args.first().is_none_or(|radix| radix.is_undefined() || radix.as_number() == Some(DECIMAL)) =>
            Some(vm.heap.alloc_string(super::super::wtf16::JsString::from_number(receiver.as_number()?))),
        Native::NumberString => vm.plain_number_to_string(receiver, args),
        Native::NumberFixed => vm.plain_number_to_fixed(receiver, args),
        Native::StringLocaleCompare => default_locale_compare(vm, receiver, args),
        Native::GlobalIsNaN | Native::GlobalIsFinite =>
            super::super::number::global_number_predicate(native, vm.primitive_to_number(argument)?).map(Value::boolean),
        _ => super::super::number::number_predicate(native, argument).map(Value::boolean),
    }
}

/// `String.prototype.localeCompare` of two strings under the default collation, once a call
/// without locales or options has made it (`Vm::locale_compare_collation`): neither string
/// converts and no guest code runs.
fn default_locale_compare(vm: &Vm, receiver: Value, args: &[Value]) -> Option<Value> {
    // Its locales and options, the only arguments it reads past the other string, are absent.
    let [other, settings @ ..] = args else { return None };
    let (locales, options) = (settings.first(), settings.get(1));
    if locales.is_some_and(|value| !value.is_undefined()) || options.is_some_and(|value| !value.is_undefined()) {
        return None;
    }
    let collation = vm.default_collation.as_ref()?;
    let (left, right) = (vm.heap.string(receiver)?, vm.heap.string(*other)?);
    Some(Value::number(collation.compare(left.host_string(), right.host_string())))
}

/// The radix `Number.prototype.toString` writes in place.
const DECIMAL: f64 = 10.0;

#[cfg(test)]
#[path = "../lane_exit_tests.rs"]
mod tests;

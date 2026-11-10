//! Every tier's one table of native calls that complete in place: no guest code, no raised
//! error. The lane and the general tier both read it (`Vm::in_place_native_value`); a case
//! may allocate, and each caller notes that its own way.
use super::*;

impl<H: Host> Vm<H> {
    /// A native call's value where its common case completes in place: the steady cases
    /// first (`Vm::steady_native_value`), then Math, the pure string methods, objects, collections
    /// and arrays, and primitives. `None` leaves the call to the native itself.
    pub(in crate::vm) fn in_place_native_value(&mut self, code: &ResidualProgram, native: Native, callee: Value,
        receiver: Value, args: &[Value]) -> Option<Value> {
        self.steady_native_value(native, receiver, args)
            .or_else(|| math_value(native, args))
            .or_else(|| self.pure_string_method_call(code, native, receiver, args))
            .or_else(|| object_value(self, code, native, receiver, args))
            .or_else(|| collection_value(self, code, native, receiver, args))
            .or_else(|| array_value(self, code, native, callee, receiver, args))
            .or_else(|| primitive_value(self, native, receiver, args))
    }
}

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
/// keys, values or entries its shape lists (`Vm::plain_enumerable_own_properties`), `super`
/// reads (`Reflect.get` and `Reflect.getPrototypeOf` of an ordinary target, whose data
/// property read ignores the receiver), `Symbol.for` of a string and
/// `Object.prototype.toString` (`object_to_string_in_place`).
fn object_value<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
    let argument = args.first().copied().unwrap_or(Value::UNDEFINED);
    match native {
        Native::ObjectCreate if args.get(1).is_none_or(|properties| properties.is_undefined()) => vm.object_create(argument),
        Native::ObjectPrototypeHasOwnProperty | Native::ObjectPrototypePropertyIsEnumerable =>
            vm.plain_own_property_query(code, native, receiver, *args.first()?),
        Native::ObjectHasOwn => vm.plain_own_property_query(code, Native::ObjectPrototypeHasOwnProperty, *args.first()?, *args.get(1)?),
        // Pure of any arguments: SameValue.
        Native::ObjectIs => vm.call_object_native(code, Native::ObjectIs, args).ok(),
        Native::ReflectGet => {
            let target = *args.first()?;
            vm.heap.ordinary_object(target)?;
            vm.plain_keyed_get(target, *args.get(1)?)
        }
        // An object's own enumerable properties, where its shape lists them.
        _ if let Some(kind) = super::super::object_keys::EnumerableOwnPropertyKind::of(native) =>
            vm.plain_enumerable_own_properties(argument, kind),
        Native::ReflectGetPrototypeOf => Some(vm.heap.ordinary_object(*args.first()?)?.proto()),
        Native::ObjectPrototypeToString => object_to_string_in_place(vm, code, receiver),
        // The registry lookup of a string key, which ToString leaves as it is.
        Native::SymbolFor if args.first().is_some_and(|key| vm.heap.is_string(*key)) =>
            vm.call_symbol_native(code, Native::SymbolFor, args).ok(),
        _ => None,
    }
}

/// `Object.prototype.toString` of `undefined`, `null`, or an object whose `@@toStringTag`
/// a plain lookup reads (`Vm::plain_keyed_get`, which declines accessors on the chain): its
/// brand and its tag then run no guest code. A proxy, whose brand asks `IsArray` of its
/// target, and a primitive, which the tag reads through its wrapper, take the native.
fn object_to_string_in_place<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, receiver: Value) -> Option<Value> {
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
fn collection_value<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
    match native {
        Native::MapSet if vm.map_set(receiver, args) => Some(receiver),
        Native::MapSet | Native::MapDelete | Native::MapClear | Native::SetAdd | Native::SetDelete | Native::SetClear =>
            vm.plain_collection_mutation(code, native, receiver, args),
        Native::ArrayBufferIsView => Some(Value::boolean(vm.is_array_buffer_view(args.first().copied().unwrap_or(Value::UNDEFINED)))),
        _ if super::super::data_view::data_view_element(native) => vm.plain_data_view_access(native, receiver, args),
        _ if super::super::date::date_reads(native) => vm.plain_date_get(native, receiver),
        _ => None,
    }
}

/// A dense array's `indexOf`, `splice`, `unshift` and `shift`, and an array iterator's step
/// over a plain array.
fn array_value<H: Host>(vm: &mut Vm<H>, code: &ResidualProgram, native: Native, callee: Value, receiver: Value,
    args: &[Value]) -> Option<Value> {
    match native {
        Native::ArrayIndexOf => vm.dense_index_of(receiver, args),
        Native::ArraySplice => vm.plain_array_splice(code, callee, receiver, args),
        Native::ArrayUnshift => vm.plain_array_unshift(receiver, args),
        Native::ArrayShift => vm.plain_array_shift(receiver),
        Native::ArrayIteratorNext => vm.plain_array_iterator_next(callee, receiver),
        _ => None,
    }
}

/// Conversions and predicates of primitives whose steps read no cell but a string's text:
/// `String.fromCharCode` of numbers, `Boolean`, `String` of a string or an immediate
/// primitive, `Number` of a primitive, decimal `Number.prototype.toString`, the global
/// `isNaN` and `isFinite`, the `Number` predicates (`number::number_predicate`), and a
/// default-collation `localeCompare` of two strings (`default_locale_compare`).
fn primitive_value<H: Host>(vm: &mut Vm<H>, native: Native, receiver: Value, args: &[Value]) -> Option<Value> {
    let argument = args.first().copied().unwrap_or(Value::UNDEFINED);
    match native {
        Native::StringFromCharCode => vm.from_char_code_numbers(args),
        Native::Boolean => Some(Value::boolean(vm.truthy(argument))),
        Native::String if vm.heap.string(argument).is_some() => Some(argument),
        Native::String if !args.is_empty() => Some(vm.heap.alloc_string(super::super::coercion::immediate_to_string(argument)?)),
        Native::Number if !args.is_empty() => vm.primitive_to_number(argument).map(Value::number),
        Native::NumberString if args.first().is_none_or(|radix| radix.is_undefined() || radix.as_number() == Some(DECIMAL)) =>
            Some(vm.heap.alloc_string(super::super::wtf16::JsString::from_number(receiver.as_number()?))),
        Native::StringLocaleCompare => default_locale_compare(vm, receiver, args),
        Native::GlobalIsNaN | Native::GlobalIsFinite =>
            super::super::number::global_number_predicate(native, vm.primitive_to_number(argument)?).map(Value::boolean),
        _ => super::super::number::number_predicate(native, argument).map(Value::boolean),
    }
}

/// `String.prototype.localeCompare` of two strings under the default collation, once a call
/// without locales or options has made it (`Vm::locale_compare_collation`): neither string
/// converts and no guest code runs.
fn default_locale_compare<H: Host>(vm: &Vm<H>, receiver: Value, args: &[Value]) -> Option<Value> {
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

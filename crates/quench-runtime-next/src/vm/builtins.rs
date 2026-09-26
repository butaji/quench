use super::property_key::PropertyKey;
use super::wtf16::JsString;
use super::*;
const MATH_FUNCTIONS: &[(&str, Native)] = &[
    ("abs", Native::MathAbs),
    ("acos", Native::MathAcos),
    ("acosh", Native::MathAcosh),
    ("asin", Native::MathAsin),
    ("asinh", Native::MathAsinh),
    ("atan", Native::MathAtan),
    ("atan2", Native::MathAtan2),
    ("atanh", Native::MathAtanh),
    ("cbrt", Native::MathCbrt),
    ("ceil", Native::MathCeil),
    ("clz32", Native::MathClz32),
    ("cos", Native::MathCos),
    ("cosh", Native::MathCosh),
    ("exp", Native::MathExp),
    ("expm1", Native::MathExpm1),
    ("floor", Native::MathFloor),
    ("f16round", Native::MathF16Round),
    ("fround", Native::MathFround),
    ("hypot", Native::MathHypot),
    ("imul", Native::MathImul),
    ("log", Native::MathLog),
    ("log10", Native::MathLog10),
    ("log1p", Native::MathLog1p),
    ("log2", Native::MathLog2),
    ("max", Native::MathMax),
    ("min", Native::MathMin),
    ("pow", Native::MathPow),
    ("random", Native::MathRandom),
    ("round", Native::MathRound),
    ("sign", Native::MathSign),
    ("sin", Native::MathSin),
    ("sinh", Native::MathSinh),
    ("sqrt", Native::MathSqrt),
    ("sumPrecise", Native::MathSumPrecise),
    ("tan", Native::MathTan),
    ("tanh", Native::MathTanh),
    ("trunc", Native::MathTrunc),
];
#[rustfmt::skip]
const NATIVES: &[Native] = &[
    Native::Print, Native::HostDone, Native::CreateRealm, Native::EvalScript, Native::RealmTypeError, Native::Eval, Native::ToString, Native::Function, Native::FunctionPrototype, Native::FunctionPrototypeHasInstance, Native::FunctionReturnThis, Native::FunctionReturnName, Native::WithEnter, Native::WithExit, Native::Object, Native::AbstractModuleSource, Native::AbstractModuleSourceToStringTag,
    Native::ObjectKeys, Native::ForInKeys, Native::ForInKeyIsEnumerable, Native::ObjectValues, Native::ObjectEntries, Native::ObjectGetOwnPropertyNames, Native::ObjectGetOwnPropertySymbols, Native::ObjectGetOwnPropertyDescriptor, Native::ObjectGetOwnPropertyDescriptors,
    Native::ObjectGroupBy,
    Native::ObjectFromEntries, Native::ObjectIs,
    Native::ObjectCreate, Native::ObjectAssign, Native::ObjectDefineProperty, Native::ObjectDefineProperties, Native::ObjectGetPrototypeOf,
    Native::ObjectSetPrototypeOf, Native::ObjectHasOwn, Native::ObjectPreventExtensions,
    Native::ObjectIsExtensible, Native::ObjectSeal, Native::ObjectIsSealed,
    Native::ObjectFreeze, Native::ObjectIsFrozen,
    Native::ObjectPrototypeHasOwnProperty, Native::ObjectPrototypePropertyIsEnumerable, Native::ObjectPrototypeIsPrototypeOf, Native::ObjectPrototypeLookupGetter, Native::ObjectPrototypeLookupSetter, Native::ObjectPrototypeToLocaleString, Native::ObjectPrototypeToString, Native::ObjectPrototypeValueOf,
    Native::ObjectPrototypeDefineGetter, Native::ObjectPrototypeDefineSetter,
    Native::ObjectPrototypeProtoGetter, Native::ObjectPrototypeProtoSetter,
    Native::ReflectGet, Native::ReflectHas, Native::ReflectApply, Native::ReflectGetOwnPropertyDescriptor, Native::ReflectDefineProperty, Native::ReflectDeleteProperty, Native::ReflectPreventExtensions, Native::ReflectIsExtensible,
    Native::ReflectSet,
    Native::SuperSet,
    Native::ObjectLiteralPrototype,
    Native::ReflectOwnKeys,
    Native::ReflectGetPrototypeOf,
    Native::ReflectSetPrototypeOf,
    Native::ReflectConstruct,
    Native::Proxy,
    Native::ProxyRevocable,
    Native::ProxyRevoke,
    Native::JsonParse,
    Native::JsonStringify,
    Native::JsonRawJson,
    Native::JsonIsRawJson,
    Native::Array, Native::TypedArray,
    Native::ArrayIsArray,
    Native::ArrayPush,
    Native::ArrayPop,
    Native::ArraySlice,
    Native::ArrayIncludes,
    Native::ArrayJoin,
    Native::ArrayConcat,
    Native::ArrayFlat,
    Native::ArrayReverse,
    Native::ArrayShift,
    Native::ArrayUnshift,
    Native::ArraySplice,
    Native::ArrayFill,
    Native::ArrayAt,
    Native::ArrayLastIndexOf,
    Native::ArrayIndexOf,
    Native::ArrayCopyWithin,
    Native::ArrayWith,
    Native::ArrayForEach,
    Native::ArrayMap,
    Native::ArrayFilter,
    Native::ArraySome,
    Native::ArrayEvery,
    Native::ArrayFind,
    Native::ArrayFindIndex,
    Native::ArrayFindLast,
    Native::ArrayFindLastIndex,
    Native::ArrayGroup,
    Native::ArrayGroupToMap,
    Native::ArrayFlatMap,
    Native::ArrayReduce,
    Native::ArrayReduceRight,
    Native::ArrayToReversed,
    Native::ArrayToSpliced,
    Native::ArraySort,
    Native::ArrayToSorted,
    Native::ArrayToString, Native::ArrayToLocaleString, Native::ArraySpecies, Native::ArrayFromAsync,
    Native::ArrayFromAsyncFulfilled, Native::ArrayFromAsyncRejected,
    Native::ArrayKeys,
    Native::ArrayValues,
    Native::ArrayEntries,
    Native::ArrayFrom,
    Native::ArrayOf,
    Native::ArrayBuffer,
    Native::ArrayBufferSpecies,
    Native::ArrayBufferSlice,
    Native::ArrayBufferTransfer,
    Native::ArrayBufferResize,
    Native::ArrayBufferTransferToFixedLength,
    Native::ArrayBufferTransferToImmutable,
    Native::ArrayBufferSliceToImmutable,
    Native::ArrayBufferByteLengthGetter,
    Native::ArrayBufferDetachedGetter,
    Native::ArrayBufferImmutableGetter,
    Native::ArrayBufferMaxByteLengthGetter,
    Native::ArrayBufferResizableGetter,
    Native::SharedArrayBufferByteLengthGetter,
    Native::SharedArrayBufferGrowableGetter,
    Native::SharedArrayBufferMaxByteLengthGetter,
    Native::ArrayBufferIsView,
    Native::DetachArrayBuffer,
    Native::ArrayIteratorNext,
    Native::SharedArrayBuffer,
    Native::SharedArrayBufferGrow,
    Native::AtomicsLoad,
    Native::AtomicsStore,
    Native::AtomicsAdd,
    Native::AtomicsSub,
    Native::AtomicsAnd,
    Native::AtomicsOr,
    Native::AtomicsXor,
    Native::AtomicsExchange,
    Native::AtomicsCompareExchange,
    Native::AtomicsIsLockFree,
    Native::AtomicsNotify,
    Native::AtomicsWait,
    Native::AtomicsWaitAsync,
    Native::AtomicsPause,
    Native::Uint8Array,
    Native::Uint8ClampedArray,
    Native::Uint16Array,
    Native::Uint32Array,
    Native::Int8Array,
    Native::Int16Array,
    Native::Int32Array,
    Native::BigInt64Array,
    Native::BigUint64Array,
    Native::Float32Array,
    Native::Float64Array,
    Native::Uint8ArraySet,
    Native::Uint8ArrayReverse,
    Native::Uint8ArrayFill,
    Native::Uint8ArrayCopyWithin,
    Native::Uint8ArraySubarray,
    Native::Uint8ArraySlice,
    Native::Uint8ArrayIncludes,
    Native::Uint8ArrayIndexOf,
    Native::Uint8ArrayJoin,
    Native::Uint8ArrayToString,
    Native::Uint8ArrayKeys,
    Native::Uint8ArrayValues,
    Native::Uint8ArrayEntries,
    Native::DataView,
    Native::DataViewGetUint8,
    Native::DataViewSetUint8,
    Native::DataViewGetInt8,
    Native::DataViewSetInt8,
    Native::DataViewGetUint16,
    Native::DataViewSetUint16,
    Native::DataViewGetInt16,
    Native::DataViewSetInt16,
    Native::DataViewGetUint32,
    Native::DataViewSetUint32,
    Native::DataViewGetInt32,
    Native::DataViewSetInt32,
    Native::DataViewGetFloat32,
    Native::DataViewSetFloat32,
    Native::DataViewGetFloat64,
    Native::DataViewSetFloat64,
    Native::DataViewGetFloat16,
    Native::DataViewSetFloat16,
    Native::DataViewGetBigInt64,
    Native::DataViewSetBigInt64,
    Native::DataViewGetBigUint64,
    Native::DataViewSetBigUint64,
    Native::DataViewBufferGetter,
    Native::DataViewByteLengthGetter,
    Native::DataViewByteOffsetGetter,
    Native::Map,
    Native::MapGet,
    Native::MapSet,
    Native::MapHas,
    Native::MapDelete,
    Native::MapClear,
    Native::MapKeys,
    Native::MapValues,
    Native::MapEntries,
    Native::MapForEach,
    Native::MapGetOrInsert,
    Native::MapGetOrInsertComputed,
    Native::MapGroupBy,
    Native::MapSizeGetter,
    Native::Set,
    Native::SetAdd,
    Native::SetHas,
    Native::SetDelete,
    Native::SetClear,
    Native::SetKeys,
    Native::SetValues,
    Native::SetEntries,
    Native::SetForEach,
    Native::SetSizeGetter,
    Native::Iterator, Native::IteratorFrom, Native::IteratorConcat, Native::IteratorZip,
    Native::IteratorZipKeyed, Native::IteratorMap, Native::IteratorFilter, Native::IteratorTake,
    Native::IteratorDrop, Native::IteratorFlatMap, Native::IteratorReduce, Native::IteratorToArray,
    Native::IteratorForEach, Native::IteratorEvery, Native::IteratorFind, Native::IteratorSome,
    Native::IteratorDispose, Native::IteratorProtocolNext, Native::IteratorProtocolReturn,
    Native::IteratorHelperNext,
    Native::IteratorHelperReturn,
    Native::IteratorPrototypeConstructorGetter, Native::IteratorPrototypeConstructorSetter,
    Native::IteratorPrototypeToStringTagGetter, Native::IteratorPrototypeToStringTagSetter,
    Native::IteratorNext, Native::IteratorClose, Native::IteratorSelf, Native::AsyncIteratorSelf,
    Native::IteratorReturn, Native::IteratorThrow,
    Native::GeneratorNext, Native::GeneratorReturn, Native::GeneratorThrow,
    Native::AsyncGeneratorNext, Native::AsyncGeneratorReturn, Native::AsyncGeneratorThrow,
    Native::AsyncGeneratorReturnFulfilled, Native::AsyncGeneratorReturnRejected,
    Native::AsyncIteratorDispose, Native::AsyncIteratorDisposeFulfilled,
    Native::Test262Agent,
    Native::WeakMap,
    Native::WeakMapGet,
    Native::WeakMapSet,
    Native::WeakMapHas,
    Native::WeakMapDelete,
    Native::WeakSet,
    Native::WeakSetAdd,
    Native::WeakSetHas,
    Native::WeakSetDelete,
    Native::WeakRef,
    Native::WeakRefDeref,
    Native::FinalizationRegistry,
    Native::FinalizationRegistryRegister,
    Native::FinalizationRegistryUnregister,
    Native::DisposableStack, Native::AsyncDisposableStack, Native::AsyncDisposableStackUse, Native::AsyncDisposableStackAdopt,
    Native::AsyncDisposableStackDefer, Native::AsyncDisposableStackMove, Native::AsyncDisposableStackDisposeAsync,
    Native::AsyncDisposableStackDisposed, Native::DisposableStackMove, Native::DisposableStackDisposed,
    Native::DisposableStackUse, Native::DisposableStackAdopt, Native::DisposableStackDefer, Native::DisposableStackDispose,
    Native::DisposableStackUseAsync, Native::DisposableStackDisposeAsync, Native::DisposableStackDisposeWithCompletion,
    Native::DisposableStackDisposeAsyncWithCompletion, Native::DisposableStackAsyncDisposalFulfilled,
    Native::DisposableStackAsyncDisposalRejected,
    Native::FunctionCall, Native::FunctionApply, Native::FunctionBind, Native::FunctionBoundCall, Native::FunctionToString, Native::FunctionCaller, Native::AsyncFunction, Native::GeneratorFunction, Native::AsyncGeneratorFunction, Native::AsyncGeneratorReturnResult, Native::AsyncGeneratorDelegateReturnStart,
    Native::Date, Native::DateNow, Native::DateGetTime, Native::DateGetFullYear,
    Native::DateGetMonth, Native::DateGetDate, Native::DateGetDay, Native::DateGetHours,
    Native::DateGetMinutes, Native::DateGetSeconds, Native::DateGetMilliseconds,
    Native::DateGetTimezoneOffset, Native::DateGetUTCFullYear, Native::DateGetUTCMonth,
    Native::DateGetUTCDate, Native::DateGetUTCDay, Native::DateGetUTCHours,
    Native::DateGetUTCMinutes, Native::DateGetUTCSeconds, Native::DateGetUTCMilliseconds,
    Native::DateGetYear, Native::DateSetTime, Native::DateSetFullYear, Native::DateSetMonth,
    Native::DateSetUTCMonth, Native::DateSetDate, Native::DateSetUTCDate,
    Native::DateSetUTCFullYear, Native::DateSetHours, Native::DateSetMinutes,
    Native::DateSetSeconds, Native::DateSetMilliseconds, Native::DateSetUTCHours,
    Native::DateSetUTCMinutes, Native::DateSetUTCSeconds, Native::DateSetUTCMilliseconds,
    Native::DateSetYear, Native::DateValueOf, Native::DateToString, Native::DateToUTCString,
    Native::DateToDateString, Native::DateToTimeString, Native::DateToLocaleString,
    Native::DateToLocaleDateString, Native::DateToLocaleTimeString,
    Native::DateToISOString, Native::DateToJSON, Native::DateToPrimitive,
    Native::DateToTemporalInstant, Native::DateParse, Native::DateUTC,
    Native::Error, Native::ErrorToString, Native::ErrorIsError, Native::ErrorStackGetter, Native::ErrorStackSetter,
    Native::AggregateError, Native::SuppressedError, Native::EvalError, Native::RangeError, Native::ReferenceError, Native::SyntaxError, Native::TypeError, Native::URIError, Native::ThrowTypeError,
    Native::RegExp,
    Native::RegExpToString,
    Native::RegExpSymbolMatch,
    Native::RegExpSpecies,
    Native::RegExpExec,
    Native::RegExpTest,
    Native::RegExpGlobal,
    Native::RegExpIgnoreCase,
    Native::RegExpMultiline,
    Native::RegExpDotAll,
    Native::RegExpUnicode,
    Native::RegExpUnicodeSets,
    Native::RegExpSticky,
    Native::RegExpHasIndices,
    Native::RegExpSource,
    Native::RegExpFlags,
    Native::String, Native::Boolean, Native::BooleanToString, Native::BooleanValueOf, Native::BigInt, Native::BigIntValueOf, Native::BigIntToString, Native::BigIntAsIntN, Native::BigIntAsUintN,
    Native::Symbol, Native::SymbolToString, Native::SymbolValueOf,
    Native::SymbolFor,
    Native::SymbolKeyFor,
    Native::StringCharCodeAt, Native::StringSlice,
    Native::StringCharAt,
    Native::StringSubstring,
    Native::StringSubstr,
    Native::StringIncludes,
    Native::StringStartsWith,
    Native::StringEndsWith,
    Native::StringIndexOf, Native::StringLastIndexOf, Native::StringToString, Native::StringValueOf,
    Native::StringReplace, Native::StringSplit, Native::StringTrim, Native::StringTrimStart,
    Native::StringTrimEnd,
    Native::StringRepeat,
    Native::StringPadStart,
    Native::StringPadEnd,
    Native::StringMatch,
    Native::StringSearch,
    Native::StringReplaceAll,
    Native::StringAt, Native::StringCodePointAt, Native::StringToUpperCase, Native::StringToLowerCase, Native::StringToLocaleLowerCase, Native::StringToLocaleUpperCase, Native::StringLocaleCompare, Native::StringConcat, Native::StringNormalize, Native::StringValues,
    Native::EncodeUri,
    Native::EncodeUriComponent,
    Native::DecodeUri,
    Native::DecodeUriComponent,
    Native::StringFromCharCode, Native::StringFromCodePoint,
    Native::ParseInt,
    Native::MathLog,
    Native::MathPow,
    Native::MathFloor,
    Native::MathMin,
    Native::MathMax,
    Native::MathRandom,
    Native::MathAbs, Native::MathCeil, Native::MathRound, Native::MathTrunc,
    Native::MathSqrt, Native::MathSign, Native::MathAcos, Native::MathAsin,
    Native::MathAtan, Native::MathCos, Native::MathExp, Native::MathSin,
    Native::MathTan, Native::MathAtan2,
    Native::MathAcosh, Native::MathAsinh, Native::MathAtanh, Native::MathCbrt,
    Native::MathCosh, Native::MathExpm1, Native::MathFround, Native::MathHypot,
    Native::MathImul, Native::MathLog10, Native::MathLog1p, Native::MathLog2,
    Native::MathSinh, Native::MathTanh, Native::MathClz32, Native::MathF16Round,
    Native::MathSumPrecise,
    Native::NumberString,
    Native::NumberToLocaleString,
    Native::Number, Native::NumberValueOf,
    Native::GlobalIsNaN, Native::GlobalIsFinite, Native::NumberIsNaN,
    Native::NumberIsFinite,
    Native::NumberIsInteger,
    Native::NumberIsSafeInteger,
    Native::NumberParseFloat,
    Native::NumberFixed,
    Native::NumberExponential,
    Native::NumberPrecision,
    Native::Promise,
    Native::PromiseResolve,
    Native::PromiseReject,
    Native::PromiseWithResolvers,
    Native::PromiseCapabilityExecutor,
    Native::PromiseThen,
    Native::PromiseCatch,
    Native::PromiseFinally, Native::PromiseAll, Native::PromiseAllKeyed, Native::PromiseRace, Native::PromiseAllSettled, Native::PromiseAllSettledKeyed, Native::PromiseAny,
    Native::PromiseReactionJob, Native::PromiseThenableJob,
    Native::PromiseFinallyJob, Native::PromiseFinallyContinuationJob, Native::PromiseAggregateJob,
    Native::PromiseAsyncResumeJob, Native::DynamicImport, Native::AsyncFromSyncValue,
    Native::AsyncFromSyncValueRejected, Native::AsyncGeneratorDelegateFulfilled,
    Native::AsyncGeneratorDelegateRejected, ];
impl<H: Host> Vm<H> {
    pub(super) fn install_builtins(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        self.install_prototypes();
        for native in NATIVES {
            let value = self.native(*native);
            self.natives.push((*native, value));
        }
        self.install_object(program)?;
        self.install_console(program)?;
        self.install_array(program)?;
        self.install_array_buffer(program)?;
        self.install_typed_array(program)?;
        self.install_data_view(program)?;
        self.install_atomics(program)?;
        self.install_collections(program)?;
        self.install_weak_collections(program)?;
        self.install_iterators(program)?;
        self.global(
            program,
            "\0rqj:iterator-close",
            self.native_value(Native::IteratorClose),
        )?;
        self.global(program, "undefined", Value::UNDEFINED)?;
        self.global(program, "NaN", Value::number(f64::NAN))?;
        self.global(program, "Infinity", Value::number(f64::INFINITY))?;
        for name in ["undefined", "NaN", "Infinity"] {
            let atom = self.intern_atom(name);
            self.set_property_attributes(
                self.realm.globals,
                property_key::PropertyKey::string(atom),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: false,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        self.global(program, "print", self.native_value(Native::Print))?;
        self.global(
            program,
            "\0rqj:to-string",
            self.native_value(Native::ToString),
        )?;
        self.global(program, "eval", self.native_value(Native::Eval))?;
        self.global(
            program,
            "\0rqj:with-enter",
            self.native_value(Native::WithEnter),
        )?;
        self.global(
            program,
            "\0rqj:with-exit",
            self.native_value(Native::WithExit),
        )?;
        self.install_date(program)?;
        self.install_host_globals(program)?;
        self.install_errors(program)?;
        self.install_regexp(program)?;
        let symbol = self.native_value(Native::Symbol);
        self.set_named(program, symbol, "for", self.native_value(Native::SymbolFor))?;
        self.set_named(
            program,
            symbol,
            "keyFor",
            self.native_value(Native::SymbolKeyFor),
        )?;
        for name in [
            "asyncDispose",
            "asyncIterator",
            "dispose",
            "hasInstance",
            "isConcatSpreadable",
            "iterator",
            "match",
            "matchAll",
            "metadata",
            "replace",
            "search",
            "species",
            "split",
            "toPrimitive",
            "toStringTag",
            "unscopables",
        ] {
            let value = self
                .heap
                .alloc(Cell::Symbol(Some(format!("Symbol.{name}").into())));
            self.well_known_symbols.insert(name.into(), value);
            self.set_named(program, symbol, name, value)?;
        }
        self.install_map_species()?;
        self.install_regexp_symbol_properties(
            self.native_value(Native::RegExp),
            self.regexp_proto,
            self.realm.globals,
        )?;
        let has_instance_symbol = self.well_known_symbols["hasInstance"];
        let has_instance = self.native_value(Native::FunctionPrototypeHasInstance);
        self.set_builtin_function_name(has_instance, "[Symbol.hasInstance]")?;
        self.set_symbol_property(self.function_proto, has_instance_symbol, has_instance)?;
        self.set_property_attributes(
            self.function_proto,
            property_key::PropertyKey::symbol(has_instance_symbol),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        self.install_finalization_registry(program)?;
        let atomics_name = self.intern_atom("Atomics");
        if let Some(atomics) = self.own_property(self.realm.globals, atomics_name) {
            self.install_builtin_to_string_tag(atomics, "Atomics")?;
        }
        let bigint = self.native_value(Native::BigInt);
        let prototype_atom = self.intern_atom("prototype");
        let bigint_prototype = self.get_property(program, bigint, prototype_atom)?;
        self.install_builtin_to_string_tag(bigint_prototype, "BigInt")?;
        let data_view = self.native_value(Native::DataView);
        let data_view_prototype = self.get_property(program, data_view, prototype_atom)?;
        self.install_builtin_to_string_tag(data_view_prototype, "DataView")?;
        let date = self.native_value(Native::Date);
        let date_prototype = self.get_property(program, date, prototype_atom)?;
        self.install_builtin_to_string_tag(date_prototype, "Date")?;
        let date_to_primitive = self.native_value(Native::DateToPrimitive);
        self.set_builtin_function_name(date_to_primitive, "[Symbol.toPrimitive]")?;
        if let Some(symbol) = self.well_known_symbols.get("toPrimitive").copied() {
            self.set_symbol_property(date_prototype, symbol, date_to_primitive)?;
            self.set_property_attributes(
                date_prototype,
                PropertyKey::symbol(symbol),
                PropertyAttributes {
                    writable: false,
                    enumerable: false,
                    configurable: true,
                    accessor: false,
                    getter: None,
                    setter: None,
                },
            );
        }
        for (prototype, tag) in [
            (self.map_proto, "Map"),
            (self.set_proto, "Set"),
            (self.weak_map_proto, "WeakMap"),
            (self.weak_set_proto, "WeakSet"),
        ] {
            self.install_builtin_to_string_tag(prototype, tag)?;
        }
        self.install_array_species()?;
        self.install_array_unscopables()?;
        self.install_abstract_module_source(program)?;
        self.install_array_buffer_species(program)?;
        self.global(program, "Symbol", symbol)?;
        self.install_disposal(program)?;
        let string = self.native_value(Native::String);
        self.install_string(program, string)?;
        self.set_builtin_named(program, string, "fromCharCode", Native::StringFromCharCode)?;
        self.set_builtin_named(
            program,
            string,
            "fromCodePoint",
            Native::StringFromCodePoint,
        )?;
        self.global(program, "String", string)?;
        self.install_iterator_self(program)?;
        self.global(program, "parseInt", self.native_value(Native::ParseInt))?;
        self.global(
            program,
            "parseFloat",
            self.native_value(Native::NumberParseFloat),
        )?;
        self.global(program, "isNaN", self.native_value(Native::GlobalIsNaN))?;
        self.global(
            program,
            "isFinite",
            self.native_value(Native::GlobalIsFinite),
        )?;
        self.install_number(program)?;
        self.global(program, "encodeURI", self.native_value(Native::EncodeUri))?;
        self.global(
            program,
            "encodeURIComponent",
            self.native_value(Native::EncodeUriComponent),
        )?;
        self.global(program, "decodeURI", self.native_value(Native::DecodeUri))?;
        self.global(
            program,
            "decodeURIComponent",
            self.native_value(Native::DecodeUriComponent),
        )?;
        self.install_json(program)?;
        self.install_reflect(program)?;
        self.install_math(program)?;
        self.install_promise(program)
    }
    fn install_prototypes(&mut self) {
        self.object_proto = self
            .heap
            .alloc(Cell::Object(Self::empty_object(Value::NULL)));
        self.function_proto = self.heap.alloc(Cell::Function {
            object: Box::new(Self::empty_object(self.object_proto)),
            kind: FunctionKind::Native(Native::FunctionPrototype),
            env: Value::NULL,
            realm: self.realm.globals,
        });
        self.object_data_mut(self.realm.globals).unwrap().proto = self.object_proto;
    }
    fn install_console(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let console = self.object();
        self.set_named(program, console, "log", self.native_value(Native::Print))?;
        self.global(program, "console", console)
    }
    fn install_json(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let json = self.object();
        self.install_builtin_to_string_tag(json, "JSON")?;
        let parse = self.native_value(Native::JsonParse);
        self.set_builtin_value_named(json, "parse", parse)?;
        self.set_builtin_function_name(parse, "parse")?;
        let stringify = self.native_value(Native::JsonStringify);
        self.set_builtin_value_named(json, "stringify", stringify)?;
        self.set_builtin_function_name(stringify, "stringify")?;
        let raw_json = self.native_value(Native::JsonRawJson);
        self.set_builtin_value_named(json, "rawJSON", raw_json)?;
        self.set_builtin_function_name(raw_json, "rawJSON")?;
        let is_raw_json = self.native_value(Native::JsonIsRawJson);
        self.set_builtin_value_named(json, "isRawJSON", is_raw_json)?;
        self.set_builtin_function_name(is_raw_json, "isRawJSON")?;
        self.global(program, "JSON", json)
    }

    fn install_reflect(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let reflect = self.object();
        for (name, native) in [
            ("get", Native::ReflectGet),
            ("has", Native::ReflectHas),
            ("apply", Native::ReflectApply),
            (
                "getOwnPropertyDescriptor",
                Native::ReflectGetOwnPropertyDescriptor,
            ),
            ("defineProperty", Native::ReflectDefineProperty),
            ("deleteProperty", Native::ReflectDeleteProperty),
            ("preventExtensions", Native::ReflectPreventExtensions),
            ("isExtensible", Native::ReflectIsExtensible),
        ] {
            self.set_named(program, reflect, name, self.native_value(native))?;
        }
        self.set_named(
            program,
            reflect,
            "set",
            self.native_value(Native::ReflectSet),
        )?;
        self.set_named(
            program,
            reflect,
            "ownKeys",
            self.native_value(Native::ReflectOwnKeys),
        )?;
        self.set_named(
            program,
            reflect,
            "getPrototypeOf",
            self.native_value(Native::ReflectGetPrototypeOf),
        )?;
        self.set_named(
            program,
            reflect,
            "setPrototypeOf",
            self.native_value(Native::ReflectSetPrototypeOf),
        )?;
        self.set_named(
            program,
            reflect,
            "construct",
            self.native_value(Native::ReflectConstruct),
        )?;
        self.global(program, "Reflect", reflect)
    }
    fn install_math(&mut self, program: &ResidualProgram) -> Result<(), JsError> {
        let math = self.object();
        self.install_builtin_to_string_tag(math, "Math")?;
        for (name, value) in [
            ("E", std::f64::consts::E),
            ("LN10", std::f64::consts::LN_10),
            ("LN2", std::f64::consts::LN_2),
            ("LOG10E", std::f64::consts::LOG10_E),
            ("LOG2E", std::f64::consts::LOG2_E),
            ("PI", std::f64::consts::PI),
            ("SQRT1_2", std::f64::consts::FRAC_1_SQRT_2),
            ("SQRT2", std::f64::consts::SQRT_2),
        ] {
            self.set_named_constant(program, math, name, Value::number(value))?;
        }
        for (name, native) in MATH_FUNCTIONS {
            self.set_builtin_named(program, math, name, *native)?;
        }
        self.global(program, "Math", math)
    }

    pub(super) fn install_builtin_to_string_tag(
        &mut self,
        object: Value,
        tag: &str,
    ) -> Result<(), JsError> {
        let symbol = self
            .well_known_symbols
            .get("toStringTag")
            .copied()
            .ok_or_else(|| JsError("Symbol.toStringTag is not initialized".into()))?;
        let value = self.heap.alloc(Cell::String(tag.into()));
        self.set_symbol_property(object, symbol, value)?;
        self.set_property_attributes(
            object,
            PropertyKey::symbol(symbol),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn empty_object(proto: Value) -> Object {
        Object {
            proto,
            properties: ValueVec::new(),
            arguments_map: None,
            arguments_object: false,
            module_namespace: false,
            module_bindings: Vec::new(),
            deferred_module: None,
            private_names: Vec::new(),
        }
    }
    fn native(&mut self, kind: Native) -> Value {
        self.native_with_env(kind, Value::NULL)
    }
    pub(super) fn native_value(&self, kind: Native) -> Value {
        self.natives
            .iter()
            .find(|(item, _)| *item == kind)
            .unwrap()
            .1
    }
    pub(super) fn object(&mut self) -> Value {
        self.heap
            .alloc(Cell::Object(Self::empty_object(self.object_proto)))
    }
    pub(super) fn lookup_atom(&self, name: &str) -> Option<Atom> {
        let hash = Self::atom_hash(name);
        let primary = self.atoms.get(&hash).copied()?;
        if self.atom_name(primary) == name {
            return Some(primary);
        }
        self.atom_collisions.get(&hash).and_then(|atoms| {
            atoms
                .iter()
                .copied()
                .find(|atom| self.atom_name(*atom) == name)
        })
    }
    pub(super) fn intern_atom(&mut self, name: &str) -> Atom {
        self.intern_js_atom(&JsString::from_str(name))
    }
    pub(super) fn intern_js_atom(&mut self, name: &JsString) -> Atom {
        if let Some(atom) = self.lookup_js_atom(name) {
            return atom;
        }
        let atom = (self.atom_text.len() + self.dynamic_atoms.len()) as Atom;
        self.dynamic_atoms.push(name.clone());
        self.index_atom(Self::atom_hash_units(name.units()), atom);
        self.profile.dynamic_atom();
        atom
    }
    pub(super) fn atom_hash(name: &str) -> u64 {
        Self::atom_hash_units(&name.encode_utf16().collect::<Vec<_>>())
    }
    pub(super) fn index_atom(&mut self, hash: u64, atom: Atom) {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.atoms.entry(hash) {
            entry.insert(atom);
        } else {
            self.atom_collisions.entry(hash).or_default().push(atom);
        }
    }
    pub(super) fn atom_name(&self, atom: Atom) -> &str {
        let index = atom as usize;
        if index < self.atom_text.len() {
            &self.atom_text[index]
        } else {
            self.dynamic_atoms[index - self.atom_text.len()].host_string()
        }
    }
    pub(super) fn global(
        &mut self,
        _p: &ResidualProgram,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property(self.realm.globals, atom, value)?;
        self.set_property_attributes(
            self.realm.globals,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }
    pub(super) fn set_named(
        &mut self,
        _p: &ResidualProgram,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property(object, atom, value)
    }
    pub(super) fn set_named_constant(
        &mut self,
        program: &ResidualProgram,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        self.set_named(program, object, name, value)?;
        let atom = self.intern_atom(name);
        self.set_property_attributes(
            object,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: false,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn set_builtin_named(
        &mut self,
        _program: &ResidualProgram,
        object: Value,
        name: &str,
        native: Native,
    ) -> Result<(), JsError> {
        let function = self.native_value(native);
        self.set_builtin_function_name(function, name)?;
        self.set_builtin_value_named(object, name, function)
    }

    pub(super) fn set_builtin_function_name(
        &mut self,
        function: Value,
        name: &str,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom("name");
        let current = self.own_property(function, atom);
        if current.is_some_and(|value| {
            !matches!(self.heap.get(value), Some(Cell::String(text)) if text.units().is_empty())
        }) {
            return Ok(());
        }
        let value = self.heap.alloc(Cell::String(JsString::from_str(name)));
        self.set_property(function, atom, value)?;
        self.set_property_attributes(
            function,
            PropertyKey::string(atom),
            PropertyAttributes {
                writable: false,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }

    pub(super) fn set_builtin_value_named(
        &mut self,
        object: Value,
        name: &str,
        value: Value,
    ) -> Result<(), JsError> {
        let atom = self.intern_atom(name);
        self.set_property(object, atom, value)?;
        self.set_property_attributes(
            object,
            property_key::PropertyKey::string(atom),
            PropertyAttributes {
                writable: true,
                enumerable: false,
                configurable: true,
                accessor: false,
                getter: None,
                setter: None,
            },
        );
        Ok(())
    }
}

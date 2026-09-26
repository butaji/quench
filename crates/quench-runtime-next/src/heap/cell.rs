use super::root::WeakHandle;
use crate::bytecode::Atom;
use crate::value::Value;
use crate::value_vec::ValueVec;
use crate::vm::program_store::ProgramId;
use crate::vm::wtf16::JsString;
use std::rc::Rc;
#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Native {
    Print, HostDone, CreateRealm, EvalScript, RealmTypeError, Eval, ToString, Function, FunctionPrototype, FunctionPrototypeHasInstance, FunctionReturnThis, FunctionReturnName, FunctionReturnClass, FunctionCaller, DynamicFunction, DynamicDerivedClass, DynamicImport, AbstractModuleSource, AbstractModuleSourceToStringTag,
    Object,
    ObjectKeys, ForInKeys, ForInKeyIsEnumerable, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectGetOwnPropertySymbols, ObjectGetOwnPropertyDescriptor, ObjectGetOwnPropertyDescriptors, ObjectFromEntries, ObjectIs,
    ObjectCreate, ObjectAssign, ObjectDefineProperty, ObjectDefineProperties, ObjectGetPrototypeOf, ObjectPreventExtensions, ObjectIsExtensible, ObjectSeal, ObjectIsSealed, ObjectFreeze, ObjectIsFrozen, ObjectGroupBy,
    ObjectSetPrototypeOf, ObjectHasOwn, ObjectPrototypeHasOwnProperty, ObjectPrototypePropertyIsEnumerable, ObjectPrototypeIsPrototypeOf, ObjectPrototypeLookupGetter, ObjectPrototypeLookupSetter, ObjectPrototypeDefineGetter, ObjectPrototypeDefineSetter, ObjectPrototypeProtoGetter, ObjectPrototypeProtoSetter, ObjectPrototypeToLocaleString, ObjectPrototypeToString, ObjectPrototypeValueOf,
    ReflectGet, ReflectHas, ReflectApply, ReflectGetOwnPropertyDescriptor, ReflectDefineProperty, ReflectDeleteProperty, ReflectPreventExtensions, ReflectIsExtensible,
    ReflectSet,
    SuperSet,
    ObjectLiteralPrototype,
    ReflectOwnKeys,
    ReflectGetPrototypeOf,
    ReflectSetPrototypeOf,
    ReflectConstruct,
    Proxy, ProxyRevocable, ProxyRevoke,
    JsonParse,
    JsonStringify,
    JsonRawJson,
    JsonIsRawJson,
    Array, TypedArray, ArrayToLocaleString, ArraySpecies, ArrayFromAsync,
    ArrayFromAsyncFulfilled, ArrayFromAsyncRejected,
    ArrayIsArray,
    ArrayPush,
    ArrayPop,
    ArraySlice,
    ArrayIncludes,
    ArrayJoin,
    ArrayConcat,
    ArrayFlat,
    ArrayReverse,
    ArrayShift,
    ArrayUnshift,
    ArraySplice,
    ArrayFill,
    ArrayAt,
    ArrayLastIndexOf,
    ArrayIndexOf,
    ArrayCopyWithin,
    ArrayWith,
    ArrayForEach,
    ArrayMap,
    ArrayFilter,
    ArraySome,
    ArrayEvery,
    ArrayFind,
    ArrayFindIndex,
    ArrayFindLast,
    ArrayFindLastIndex,
    ArrayGroup,
    ArrayGroupToMap,
    ArrayFlatMap,
    ArrayReduce,
    ArrayReduceRight,
    ArrayToReversed,
    ArrayToSpliced,
    ArraySort,
    ArrayToSorted,
    ArrayToString,
    ArrayKeys,
    ArrayValues,
    ArrayEntries,
    ArrayFrom,
    ArrayOf,
    ArrayBuffer,
    ArrayBufferSpecies,
    ArrayBufferSlice,
    ArrayBufferTransfer,
    ArrayBufferResize,
    ArrayBufferTransferToFixedLength,
    ArrayBufferTransferToImmutable,
    ArrayBufferSliceToImmutable,
    ArrayBufferByteLengthGetter,
    ArrayBufferDetachedGetter,
    ArrayBufferImmutableGetter,
    ArrayBufferMaxByteLengthGetter,
    ArrayBufferResizableGetter,
    SharedArrayBufferByteLengthGetter,
    SharedArrayBufferGrowableGetter,
    SharedArrayBufferMaxByteLengthGetter,
    ArrayBufferIsView,
    DetachArrayBuffer,
    ArrayIteratorNext,
    SharedArrayBuffer,
    SharedArrayBufferGrow,
    AtomicsLoad,
    AtomicsStore,
    AtomicsAdd,
    AtomicsSub,
    AtomicsAnd,
    AtomicsOr,
    AtomicsXor,
    AtomicsExchange,
    AtomicsCompareExchange,
    AtomicsIsLockFree,
    AtomicsNotify,
    AtomicsWait,
    AtomicsWaitAsync,
    AtomicsPause,
    Uint8Array,
    Uint8ClampedArray,
    Uint16Array,
    Uint32Array,
    Int8Array,
    Int16Array,
    Int32Array,
    BigInt64Array,
    BigUint64Array,
    Float32Array,
    Float64Array,
    Uint8ArraySet,
    Uint8ArrayReverse,
    Uint8ArrayFill,
    Uint8ArrayCopyWithin,
    Uint8ArraySubarray,
    Uint8ArraySlice,
    Uint8ArrayIncludes,
    Uint8ArrayIndexOf,
    Uint8ArrayJoin,
    Uint8ArrayToString,
    Uint8ArrayKeys,
    Uint8ArrayValues,
    Uint8ArrayEntries,
    DataView,
    DataViewGetUint8,
    DataViewSetUint8,
    DataViewGetInt8,
    DataViewSetInt8,
    DataViewGetUint16,
    DataViewSetUint16,
    DataViewGetInt16,
    DataViewSetInt16,
    DataViewGetUint32,
    DataViewSetUint32,
    DataViewGetInt32,
    DataViewSetInt32,
    DataViewGetFloat32,
    DataViewSetFloat32,
    DataViewGetFloat64,
    DataViewSetFloat64,
    DataViewGetFloat16,
    DataViewSetFloat16,
    DataViewGetBigInt64,
    DataViewSetBigInt64,
    DataViewGetBigUint64,
    DataViewSetBigUint64,
    DataViewBufferGetter,
    DataViewByteLengthGetter,
    DataViewByteOffsetGetter,
    Map,
    MapGet,
    MapSet,
    MapHas,
    MapDelete,
    MapClear,
    MapKeys,
    MapValues,
    MapEntries,
    MapForEach,
    MapGetOrInsert,
    MapGetOrInsertComputed,
    MapGroupBy,
    MapSizeGetter,
    Set,
    SetAdd,
    SetHas,
    SetDelete,
    SetClear,
    SetKeys,
    SetValues,
    SetEntries,
    SetForEach,
    SetSizeGetter,
    Iterator,
    IteratorFrom,
    IteratorConcat,
    IteratorZip,
    IteratorZipKeyed,
    IteratorMap,
    IteratorFilter,
    IteratorTake,
    IteratorDrop,
    IteratorFlatMap,
    IteratorReduce,
    IteratorToArray,
    IteratorForEach,
    IteratorEvery,
    IteratorFind,
    IteratorSome,
    IteratorDispose,
    IteratorProtocolNext,
    IteratorProtocolReturn,
    IteratorHelperNext,
    IteratorHelperReturn,
    IteratorPrototypeConstructorGetter,
    IteratorPrototypeConstructorSetter,
    IteratorPrototypeToStringTagGetter,
    IteratorPrototypeToStringTagSetter,
    IteratorNext, IteratorClose, IteratorSelf, AsyncIteratorSelf, IteratorReturn, IteratorThrow,
    GeneratorNext, GeneratorReturn, GeneratorThrow,
    AsyncGeneratorNext, AsyncGeneratorReturn, AsyncGeneratorThrow,
    AsyncGeneratorReturnFulfilled, AsyncGeneratorReturnRejected,
    AsyncIteratorDispose, AsyncIteratorDisposeFulfilled,
    Test262Agent,
    WeakMap,
    WeakMapGet,
    WeakMapSet,
    WeakMapHas,
    WeakMapDelete,
    WeakSet,
    WeakSetAdd,
    WeakSetHas,
    WeakSetDelete,
    WeakRef,
    WeakRefDeref,
    FinalizationRegistry,
    FinalizationRegistryRegister,
    FinalizationRegistryUnregister,
    DisposableStack,
    AsyncDisposableStack,
    AsyncDisposableStackUse,
    AsyncDisposableStackAdopt,
    AsyncDisposableStackDefer,
    AsyncDisposableStackMove,
    AsyncDisposableStackDisposeAsync,
    AsyncDisposableStackDisposed,
    DisposableStackMove,
    DisposableStackDisposed,
    DisposableStackUse,
    DisposableStackAdopt,
    DisposableStackDefer,
    DisposableStackDispose,
    DisposableStackUseAsync,
    DisposableStackDisposeAsync,
    DisposableStackDisposeWithCompletion,
    DisposableStackDisposeAsyncWithCompletion,
    DisposableStackAsyncDisposalFulfilled,
    DisposableStackAsyncDisposalRejected,
    FunctionCall, FunctionApply, FunctionBind, FunctionBoundCall, FunctionToString, AsyncFunction, GeneratorFunction, AsyncGeneratorFunction, AsyncGeneratorReturnResult, AsyncGeneratorDelegateReturnStart,
    Date,
    DateNow,
    DateGetTime, DateValueOf, DateGetTimezoneOffset, DateGetFullYear, DateGetMonth, DateGetDate, DateGetDay,
    DateGetHours, DateGetMinutes, DateGetSeconds, DateGetMilliseconds, DateGetUTCFullYear,
    DateGetUTCMonth, DateGetUTCDate, DateGetUTCDay, DateGetUTCHours, DateGetUTCMinutes,
    DateGetUTCSeconds, DateGetUTCMilliseconds, DateGetYear,
    DateSetTime, DateSetFullYear, DateSetMonth, DateSetUTCMonth, DateSetDate, DateSetUTCDate,
    DateSetUTCFullYear, DateSetHours, DateSetMinutes, DateSetSeconds, DateSetMilliseconds,
    DateSetUTCHours, DateSetUTCMinutes, DateSetUTCSeconds, DateSetUTCMilliseconds, DateSetYear,
    DateToString, DateToDateString, DateToTimeString, DateToUTCString,
    DateToLocaleString, DateToLocaleDateString, DateToLocaleTimeString, DateToISOString,
    DateToJSON, DateToPrimitive, DateToTemporalInstant, DateParse, DateUTC,
    Error, ErrorToString, ErrorIsError, ErrorStackGetter, ErrorStackSetter, AggregateError, SuppressedError, EvalError, RangeError, ReferenceError, SyntaxError, TypeError, URIError, ThrowTypeError,
    RegExp, RegExpToString, RegExpSymbolMatch, RegExpSymbolReplace, RegExpSpecies,
    RegExpExec,
    RegExpTest,
    RegExpGlobal,
    RegExpIgnoreCase,
    RegExpMultiline,
    RegExpDotAll,
    RegExpUnicode,
    RegExpUnicodeSets,
    RegExpSticky,
    RegExpHasIndices,
    RegExpSource,
    RegExpFlags,
    String, Boolean, BooleanToString, BooleanValueOf,
    Symbol, SymbolToString, SymbolValueOf,
    BigInt, BigIntValueOf, BigIntToString, BigIntAsIntN, BigIntAsUintN,
    SymbolFor,
    SymbolKeyFor,
    StringCharCodeAt, StringSlice,
    StringCharAt,
    StringSubstring,
    StringSubstr,
    StringIncludes,
    StringStartsWith,
    StringEndsWith,
    StringIndexOf, StringLastIndexOf,
    StringToString, StringValueOf, StringLocaleCompare, StringToLocaleLowerCase, StringToLocaleUpperCase,
    StringReplace, StringSplit, StringTrim, StringTrimStart, StringTrimEnd,
    StringRepeat, StringPadStart, StringPadEnd, StringMatch, StringSearch,
    StringReplaceAll, StringAt, StringCodePointAt, StringToUpperCase, StringToLowerCase, StringConcat, StringNormalize, StringValues,
    EncodeUri, EncodeUriComponent,
    DecodeUri, DecodeUriComponent,
    StringFromCharCode, StringFromCodePoint, ParseInt,
    MathLog, MathPow, MathFloor, MathMin, MathMax, MathRandom,
    MathAbs, MathCeil, MathRound, MathTrunc, MathSqrt, MathSign, MathAcos, MathAsin, MathAtan, MathCos, MathExp, MathSin, MathTan, MathAtan2, NumberString, NumberToLocaleString, Number, NumberValueOf,
    MathAcosh, MathAsinh, MathAtanh, MathCbrt, MathCosh, MathExpm1, MathFround,
    MathHypot, MathImul, MathLog10, MathLog1p, MathLog2, MathSinh, MathTanh,
    MathClz32, MathF16Round, MathSumPrecise,
    NumberIsNaN, NumberIsFinite, NumberIsInteger, NumberIsSafeInteger, NumberParseFloat, GlobalIsNaN, GlobalIsFinite,
    NumberFixed,
    NumberExponential,
    NumberPrecision,
    Promise,
    PromiseSpeciesGetter,
    PromiseResolve,
    PromiseReject,
    PromiseTry,
    PromiseWithResolvers,
    PromiseCapabilityExecutor,
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    PromiseFinallyHandler,
    PromiseFinallyContinuationHandler,
    PromiseAll,
    PromiseAllKeyed,
    PromiseRace,
    PromiseAllSettled,
    PromiseAllSettledKeyed,
    PromiseAny,
    PromiseReactionJob,
    PromiseThenableJob,
    PromiseFinallyJob,
    PromiseFinallyContinuationJob,
    PromiseAggregateJob,
    PromiseAsyncResumeJob, AsyncFromSyncValue, AsyncFromSyncValueRejected, AsyncGeneratorDelegateFulfilled, AsyncGeneratorDelegateRejected,
    WithEnter, WithExit,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypedArrayKind {
    Uint8,
    Uint8Clamped,
    Uint16,
    Uint32,
    Int8,
    Int16,
    Int32,
    BigInt64,
    BigUint64,
    Float32,
    Float64,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum ArrayFromAsyncAwait {
    IteratorStep,
    ArrayLikeValue,
    MapperResult,
}

#[derive(Clone, Copy, Debug)]
pub(crate) struct ArrayFromAsyncState {
    pub(crate) output: Value,
    pub(crate) iterator: Option<Value>,
    pub(crate) result: Value,
    pub(crate) mapper: Option<Value>,
    pub(crate) this_arg: Value,
    pub(crate) index: usize,
    pub(crate) array_like: Option<(Value, usize)>,
    pub(crate) awaiting: ArrayFromAsyncAwait,
}
impl TypedArrayKind {
    pub(crate) const fn width(self) -> usize {
        match self {
            Self::Uint8 => 1,
            Self::Uint8Clamped => 1,
            Self::Uint16 => 2,
            Self::Uint32 => 4,
            Self::Int8 => 1,
            Self::Int16 => 2,
            Self::Int32 => 4,
            Self::BigInt64 => 8,
            Self::BigUint64 => 8,
            Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}
impl Native {
    pub(crate) fn is_function_native(self) -> bool {
        matches!(
            self,
            Self::Function
                | Self::FunctionReturnThis
                | Self::FunctionReturnName
                | Self::FunctionReturnClass
                | Self::DynamicFunction
                | Self::AsyncFunction
                | Self::GeneratorFunction
                | Self::AsyncGeneratorFunction
        )
    }

    pub(crate) fn is_host_control_native(self) -> bool {
        matches!(
            self,
            Self::HostDone | Self::CreateRealm | Self::Boolean | Self::BigInt
        ) || self.is_function_native()
    }

    pub(crate) fn is_error_constructor(self) -> bool {
        matches!(
            self,
            Self::Error
                | Self::AggregateError
                | Self::SuppressedError
                | Self::EvalError
                | Self::RangeError
                | Self::ReferenceError
                | Self::SyntaxError
                | Self::TypeError
                | Self::URIError
                | Self::RealmTypeError
        )
    }

    #[rustfmt::skip]
    pub(crate) fn is_object_static(self) -> bool { matches!(self, Native::Object | Native::ObjectKeys | Native::ForInKeys | Native::ForInKeyIsEnumerable | Native::ObjectValues | Native::ObjectEntries | Native::ObjectGetOwnPropertyNames | Native::ObjectGetOwnPropertySymbols | Native::ObjectGetOwnPropertyDescriptor | Native::ObjectGetOwnPropertyDescriptors | Native::ObjectFromEntries | Native::ObjectIs | Native::ObjectCreate | Native::ObjectAssign | Native::ObjectDefineProperty | Native::ObjectDefineProperties | Native::ObjectGetPrototypeOf | Native::ObjectSetPrototypeOf | Native::ObjectHasOwn | Native::ObjectPreventExtensions | Native::ObjectIsExtensible | Native::ObjectSeal | Native::ObjectIsSealed | Native::ObjectFreeze | Native::ObjectIsFrozen | Native::ObjectGroupBy) }
    pub(crate) fn is_typed_array_method(self) -> bool {
        matches!(
            self,
            Self::Uint8ArraySet
                | Self::Uint8ArrayReverse
                | Self::Uint8ArrayFill
                | Self::Uint8ArrayCopyWithin
                | Self::Uint8ArraySubarray
                | Self::Uint8ArraySlice
                | Self::Uint8ArrayIncludes
                | Self::Uint8ArrayIndexOf
                | Self::Uint8ArrayJoin
                | Self::Uint8ArrayToString
                | Self::ArrayBufferIsView
        )
    }
    pub(crate) fn is_typed_array_iterator(self) -> bool {
        matches!(
            self,
            Self::Uint8ArrayKeys | Self::Uint8ArrayValues | Self::Uint8ArrayEntries
        )
    }
    pub(crate) fn is_atomics_native(self) -> bool {
        matches!(
            self,
            Self::AtomicsLoad
                | Self::AtomicsStore
                | Self::AtomicsAdd
                | Self::AtomicsSub
                | Self::AtomicsAnd
                | Self::AtomicsOr
                | Self::AtomicsXor
                | Self::AtomicsExchange
                | Self::AtomicsCompareExchange
                | Self::AtomicsIsLockFree
                | Self::AtomicsNotify
                | Self::AtomicsWait
                | Self::AtomicsWaitAsync
                | Self::AtomicsPause
        )
    }

    pub(crate) fn is_bigint_native(self) -> bool {
        matches!(
            self,
            Self::BigIntValueOf | Self::BigIntToString | Self::BigIntAsIntN | Self::BigIntAsUintN
        )
    }
    pub(crate) fn is_data_view_native(self) -> bool {
        matches!(
            self,
            Self::DataViewGetUint8
                | Self::DataViewSetUint8
                | Self::DataViewGetInt8
                | Self::DataViewSetInt8
                | Self::DataViewGetUint16
                | Self::DataViewSetUint16
                | Self::DataViewGetInt16
                | Self::DataViewSetInt16
                | Self::DataViewGetUint32
                | Self::DataViewSetUint32
                | Self::DataViewGetInt32
                | Self::DataViewSetInt32
                | Self::DataViewGetFloat32
                | Self::DataViewSetFloat32
                | Self::DataViewGetFloat64
                | Self::DataViewSetFloat64
                | Self::DataViewGetFloat16
                | Self::DataViewSetFloat16
                | Self::DataViewGetBigInt64
                | Self::DataViewSetBigInt64
                | Self::DataViewGetBigUint64
                | Self::DataViewSetBigUint64
                | Self::DataViewBufferGetter
                | Self::DataViewByteLengthGetter
                | Self::DataViewByteOffsetGetter
        )
    }

    pub(crate) fn is_promise_native(self) -> bool {
        matches!(
            self,
            Self::Promise
                | Self::PromiseSpeciesGetter
                | Self::PromiseResolve
                | Self::PromiseReject
                | Self::PromiseTry
                | Self::PromiseWithResolvers
                | Self::PromiseCapabilityExecutor
                | Self::PromiseThen
                | Self::PromiseCatch
                | Self::PromiseFinally
                | Self::PromiseFinallyHandler
                | Self::PromiseFinallyContinuationHandler
                | Self::PromiseAll
                | Self::PromiseAllKeyed
                | Self::PromiseRace
                | Self::PromiseAllSettled
                | Self::PromiseAllSettledKeyed
                | Self::PromiseAny
                | Self::PromiseReactionJob
                | Self::PromiseThenableJob
                | Self::PromiseFinallyJob
                | Self::PromiseFinallyContinuationJob
                | Self::PromiseAggregateJob
                | Self::PromiseAsyncResumeJob
                | Self::ArrayFromAsyncFulfilled
                | Self::ArrayFromAsyncRejected
                | Self::DynamicImport
                | Self::AsyncGeneratorDelegateFulfilled
                | Self::AsyncGeneratorDelegateRejected
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum FunctionKind {
    User(ProgramId, u32),
    NumericUser(ProgramId, u32),
    Native(Native),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum IteratorKind {
    Array,
    ArrayKeys,
    ArrayValues,
    ArrayEntries,
    String,
    MapKeys,
    MapValues,
    MapEntries,
    SetValues,
    SetEntries,
    Protocol,
    Map,
    Filter,
    Take,
    Drop,
    FlatMap,
    Concat,
    Zip,
    Generator,
    AsyncFromSync,
    AsyncGenerator,
}
#[derive(Clone, Debug)]
pub(crate) enum IteratorHelper {
    Map {
        callback: Value,
        index: usize,
    },
    Filter {
        callback: Value,
        index: usize,
    },
    Take {
        remaining: f64,
    },
    Drop {
        remaining: f64,
    },
    FlatMap {
        callback: Value,
        index: usize,
        inner: Option<Value>,
    },
    Concat {
        items: Vec<Value>,
        methods: Vec<Value>,
        opened: Vec<Option<Value>>,
        next_item: usize,
        active: Option<Value>,
    },
    Zip {
        iterators: Vec<Value>,
        padding: Vec<Value>,
        mode: IteratorZipMode,
        keys: Option<Vec<Value>>,
        opened: Vec<bool>,
        done: bool,
    },
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum IteratorZipMode {
    Shortest,
    Longest,
    Strict,
}
#[derive(Clone, Copy, PartialEq, Eq)]
pub(crate) enum IteratorConsumer {
    Reduce,
    ToArray,
    ForEach,
    Every,
    Find,
    Some,
}
#[derive(Clone, Debug)]
pub(crate) struct Object {
    pub proto: Value,
    // Property names live once in the VM's immutable shape table; objects keep
    // only the data vector selected by that shape.
    pub properties: ValueVec,
    pub arguments_map: Option<Vec<u16>>,
    pub arguments_object: bool,
    pub module_namespace: bool,
    pub module_bindings: Vec<(Atom, ProgramId, u16)>,
    pub deferred_module: Option<crate::ModuleSource>,
    pub private_names: Vec<PrivateBrand>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) struct PrivateBrand {
    pub home: Value,
    pub name: Atom,
}
#[derive(Clone, Debug)]
pub(crate) struct FinalizationEntry {
    pub target: WeakHandle,
    pub held: Value,
    pub token: Option<WeakHandle>,
}
#[derive(Clone, Debug, Default)]
pub(crate) struct FinalizationEntries(pub(crate) Vec<FinalizationEntry>);
impl std::ops::Deref for FinalizationEntries {
    type Target = Vec<FinalizationEntry>;

    fn deref(&self) -> &Self::Target {
        &self.0
    }
}
impl std::ops::DerefMut for FinalizationEntries {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.0
    }
}
impl Object {
    pub(crate) fn shape(&self) -> u32 {
        self.properties.auxiliary()
    }
    pub(crate) fn set_shape(&mut self, shape: u32) {
        self.properties.set_auxiliary(shape);
    }
    pub(crate) fn is_extensible(&self) -> bool {
        self.properties.is_extensible()
    }
    pub(crate) fn set_extensible(&mut self, value: bool) {
        self.properties.set_extensible(value);
    }
    pub(crate) fn is_frozen(&self) -> bool {
        self.properties.is_frozen()
    }
    pub(crate) fn set_frozen(&mut self, value: bool) {
        self.properties.set_frozen(value);
    }
    pub(crate) fn is_arguments_object(&self) -> bool {
        self.arguments_object
    }
    pub(crate) fn is_module_namespace(&self) -> bool {
        self.module_namespace
    }

    pub(crate) fn module_binding(&self, atom: Atom) -> Option<(ProgramId, u16)> {
        self.module_bindings
            .iter()
            .find_map(|(name, program, slot)| (*name == atom).then_some((*program, *slot)))
    }
}
#[derive(Clone, Debug)]
#[rustfmt::skip]
pub(crate) enum Cell {
    Object(Object),
    Array {
        object: Object,
        elements: Rc<Vec<Value>>,
    },
    ArrayBuffer {
        object: Object,
        bytes: Rc<Vec<u8>>,
        shared: bool,
        detached: bool,
        max_byte_length: usize,
        resizable: bool,
        immutable: bool,
    },
    TypedArray {
        kind: TypedArrayKind,
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    DataView {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Map {
        object: Object,
        entries: Vec<(Value, Value)>,
    },
    Set {
        object: Object,
        entries: Vec<Value>,
    },
    WeakMap {
        object: Object,
        entries: Vec<(Value, Value)>,
    },
    WeakSet {
        object: Object,
        entries: Vec<Value>,
    },
    WeakRef {
        object: Object,
        target: Option<WeakHandle>,
    },
    FinalizationRegistry {
        object: Object,
        callback: Value,
        entries: Box<FinalizationEntries>,
    },
    Iterator {
        object: Object,
        source: Value,
        next_method: Option<Value>,
        helper: Option<Box<IteratorHelper>>,
        helper_running: bool,
        helper_started: bool,
        kind: IteratorKind,
        index: usize,
        done: bool,
        generator: Option<Box<crate::vm::activation::GeneratorRecord>>,
    },
    ArrayFromAsyncState(ArrayFromAsyncState),
    Proxy {
        object: Object,
        target: Value,
        handler: Value,
    },
    Function {
        object: Box<Object>,
        kind: FunctionKind,
        env: Value,
        realm: Value,
    },
    Environment {
        parent: Value,
        program: Option<u32>,
        root_eval_scope: bool,
        function: u32,
        slots: Box<[Value]>,
        dynamic_bindings: Vec<(Atom, Value)>,
        with_objects: Vec<Value>,
    },
    String(JsString), BigInt(String),
    Symbol(Option<String>),
    Date { milliseconds: f64, object: Box<Object> },
    RegExp {
        object: Object,
        source: JsString,
        flags: String,
    },
    Error(String),
}

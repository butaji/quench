use super::root::WeakHandle;
use crate::value::Value;
use crate::value_vec::ValueVec;
use crate::vm::wtf16::JsString;
use std::rc::Rc;
#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Native {
    Print,
    Object,
    ObjectKeys, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectGetOwnPropertySymbols, ObjectGetOwnPropertyDescriptor, ObjectGetOwnPropertyDescriptors, ObjectFromEntries, ObjectIs,
    ObjectCreate, ObjectAssign, ObjectDefineProperty, ObjectDefineProperties, ObjectGetPrototypeOf, ObjectPreventExtensions, ObjectIsExtensible, ObjectSeal, ObjectIsSealed, ObjectFreeze, ObjectIsFrozen,
    ObjectSetPrototypeOf, ObjectHasOwn, ObjectPrototypeHasOwnProperty, ObjectPrototypePropertyIsEnumerable, ObjectPrototypeIsPrototypeOf,
    ReflectGet, ReflectGetOwnPropertyDescriptor, ReflectDefineProperty, ReflectDeleteProperty, ReflectPreventExtensions, ReflectIsExtensible,
    ReflectSet,
    ReflectOwnKeys,
    ReflectGetPrototypeOf,
    ReflectSetPrototypeOf,
    ReflectConstruct,
    Proxy, ProxyRevocable, ProxyRevoke,
    JsonParse,
    JsonStringify,
    Array,
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
    ArrayBufferSlice,
    ArrayBufferTransfer,
    ArrayBufferResize,
    ArrayBufferTransferToFixedLength,
    ArrayBufferIsView,
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
    Uint8Array,
    Uint8ClampedArray,
    Uint16Array,
    Uint32Array,
    Int8Array,
    Int16Array,
    Int32Array,
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
    Set,
    SetAdd,
    SetHas,
    SetDelete,
    SetClear,
    SetKeys,
    SetValues,
    SetEntries,
    SetForEach,
    IteratorNext, IteratorClose,
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
    FunctionCall,
    FunctionApply,
    Date,
    DateNow,
    DateGetTime, DateValueOf, DateToISOString, DateToJSON, DateParse, DateUTC,
    Error,
    RegExp,
    RegExpExec,
    RegExpTest,
    String,
    Symbol, SymbolToString, SymbolValueOf,
    SymbolFor,
    SymbolKeyFor,
    StringCharCodeAt,
    StringCharAt,
    StringSubstring,
    StringSubstr,
    StringIncludes,
    StringStartsWith,
    StringEndsWith,
    StringIndexOf, StringLastIndexOf,
    StringToString, StringValueOf,
    StringReplace, StringSplit, StringTrim, StringTrimStart, StringTrimEnd,
    StringRepeat, StringPadStart, StringPadEnd, StringMatch, StringSearch,
    StringReplaceAll, StringAt, StringCodePointAt, StringToUpperCase, StringToLowerCase, StringConcat, StringNormalize,
    EncodeUri, EncodeUriComponent,
    DecodeUri, DecodeUriComponent,
    StringFromCharCode, StringFromCodePoint, ParseInt,
    MathLog, MathPow, MathFloor, MathMin, MathMax, MathRandom,
    MathAbs, MathCeil, MathRound, MathTrunc, MathSqrt, MathSign, NumberString, Number,
    NumberIsNaN, NumberIsFinite, NumberIsInteger, NumberIsSafeInteger, NumberParseFloat,
    NumberFixed,
    NumberPrecision,
    Promise,
    PromiseResolve,
    PromiseReject,
    PromiseThen,
    PromiseCatch,
    PromiseFinally,
    PromiseAll,
    PromiseRace,
    PromiseAllSettled,
    PromiseReactionJob,
    PromiseThenableJob,
    PromiseFinallyJob,
    PromiseFinallyContinuationJob,
    PromiseAggregateJob,
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
    Float32,
    Float64,
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
            Self::Float32 => 4,
            Self::Float64 => 8,
        }
    }
}
impl Native {
    #[rustfmt::skip]
    pub(crate) fn is_object_static(self) -> bool { matches!(self, Native::ObjectKeys | Native::ObjectValues | Native::ObjectEntries | Native::ObjectGetOwnPropertyNames | Native::ObjectGetOwnPropertySymbols | Native::ObjectGetOwnPropertyDescriptor | Native::ObjectGetOwnPropertyDescriptors | Native::ObjectFromEntries | Native::ObjectIs | Native::ObjectCreate | Native::ObjectAssign | Native::ObjectDefineProperty | Native::ObjectDefineProperties | Native::ObjectGetPrototypeOf | Native::ObjectSetPrototypeOf | Native::ObjectHasOwn | Native::ObjectPreventExtensions | Native::ObjectIsExtensible | Native::ObjectSeal | Native::ObjectIsSealed | Native::ObjectFreeze | Native::ObjectIsFrozen) }
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
        )
    }

    pub(crate) fn is_promise_native(self) -> bool {
        matches!(
            self,
            Self::Promise
                | Self::PromiseResolve
                | Self::PromiseReject
                | Self::PromiseThen
                | Self::PromiseCatch
                | Self::PromiseFinally
                | Self::PromiseAll
                | Self::PromiseRace
                | Self::PromiseAllSettled
                | Self::PromiseReactionJob
                | Self::PromiseThenableJob
                | Self::PromiseFinallyJob
                | Self::PromiseFinallyContinuationJob
                | Self::PromiseAggregateJob
        )
    }
}
#[derive(Clone, Copy, Debug)]
pub(crate) enum FunctionKind {
    User(u32),
    NumericUser(u32),
    Native(Native),
}
#[derive(Clone, Copy, Debug)]
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
}
#[derive(Clone, Debug)]
pub(crate) struct Object {
    pub proto: Value,
    // Property names live once in the VM's immutable shape table; objects keep
    // only the data vector selected by that shape.
    pub properties: ValueVec,
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
}
#[derive(Clone, Debug)]
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
        kind: IteratorKind,
        index: usize,
    },
    Proxy {
        object: Object,
        target: Value,
        handler: Value,
    },
    Function {
        object: Box<Object>,
        kind: FunctionKind,
        env: Value,
    },
    Environment {
        parent: Value,
        slots: Box<[Value]>,
    },
    String(JsString),
    BigInt(String),
    Symbol(Option<String>),
    Date(f64),
    Error(String),
}

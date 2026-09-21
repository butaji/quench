use crate::value::Value;
use crate::value_vec::ValueVec;
use std::rc::Rc;
#[rustfmt::skip]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Native {
    Print,
    Object,
    ObjectKeys, ObjectValues, ObjectEntries, ObjectGetOwnPropertyNames, ObjectFromEntries, ObjectIs,
    ObjectCreate, ObjectAssign, ObjectGetPrototypeOf,
    ObjectSetPrototypeOf, ObjectHasOwn, ObjectPrototypeHasOwnProperty, ObjectPrototypePropertyIsEnumerable,
    ReflectGet,
    ReflectSet,
    ReflectOwnKeys,
    ReflectGetPrototypeOf,
    ReflectSetPrototypeOf,
    ReflectConstruct,
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
    IteratorNext,
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
    Symbol,
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
    StringFromCharCode,
    ParseInt,
    MathLog, MathPow, MathFloor, MathMin, MathMax, MathRandom,
    MathAbs, MathCeil, MathRound, MathTrunc, MathSqrt, MathSign,
    NumberString,
    Number,
    NumberIsNaN, NumberIsFinite, NumberIsInteger, NumberIsSafeInteger, NumberParseFloat,
    NumberFixed,
    NumberPrecision,
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
impl Object {
    pub(crate) fn shape(&self) -> u32 {
        self.properties.auxiliary()
    }
    pub(crate) fn set_shape(&mut self, shape: u32) {
        self.properties.set_auxiliary(shape);
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
    Uint8Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Uint8ClampedArray {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Uint16Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Uint32Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Int8Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Int16Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Int32Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Float32Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
        length_tracking: bool,
    },
    Float64Array {
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
        target: Value,
    },
    Iterator {
        object: Object,
        source: Value,
        kind: IteratorKind,
        index: usize,
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
    String(String),
    BigInt(String),
    Symbol(Option<String>),
    Date(f64),
    Error(String),
}
impl Cell {
    pub(crate) fn typed_array_backing(&self) -> Option<(&Object, Value)> {
        match self {
            Self::Uint8Array { object, buffer, .. }
            | Self::Uint8ClampedArray { object, buffer, .. }
            | Self::Uint16Array { object, buffer, .. }
            | Self::Uint32Array { object, buffer, .. }
            | Self::Int8Array { object, buffer, .. }
            | Self::Int16Array { object, buffer, .. }
            | Self::Int32Array { object, buffer, .. }
            | Self::Float32Array { object, buffer, .. }
            | Self::Float64Array { object, buffer, .. } => Some((object, *buffer)),
            _ => None,
        }
    }
    pub(crate) fn object(&self) -> Option<&Object> {
        match self {
            Self::Object(object)
            | Self::Array { object, .. }
            | Self::ArrayBuffer { object, .. }
            | Self::Uint8Array { object, .. }
            | Self::Uint8ClampedArray { object, .. }
            | Self::Uint16Array { object, .. }
            | Self::Uint32Array { object, .. }
            | Self::Int8Array { object, .. }
            | Self::Int16Array { object, .. }
            | Self::Int32Array { object, .. }
            | Self::Float32Array { object, .. }
            | Self::Float64Array { object, .. }
            | Self::DataView { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::Iterator { object, .. } => Some(object),
            Self::Function { object, .. } => Some(object),
            _ => None,
        }
    }
    pub(crate) fn object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Self::Object(object)
            | Self::Array { object, .. }
            | Self::ArrayBuffer { object, .. }
            | Self::Uint8Array { object, .. }
            | Self::Uint8ClampedArray { object, .. }
            | Self::Uint16Array { object, .. }
            | Self::Uint32Array { object, .. }
            | Self::Int8Array { object, .. }
            | Self::Int16Array { object, .. }
            | Self::Int32Array { object, .. }
            | Self::Float32Array { object, .. }
            | Self::Float64Array { object, .. }
            | Self::DataView { object, .. }
            | Self::Map { object, .. }
            | Self::Set { object, .. }
            | Self::WeakMap { object, .. }
            | Self::WeakSet { object, .. }
            | Self::WeakRef { object, .. }
            | Self::Iterator { object, .. } => Some(object),
            Self::Function { object, .. } => Some(object),
            _ => None,
        }
    }
}

use crate::value::Value;
use crate::value_vec::ValueVec;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Native {
    Print,
    Object,
    ObjectKeys,
    ObjectCreate,
    ObjectAssign,
    ObjectGetPrototypeOf,
    ObjectSetPrototypeOf,
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
    ArrayBufferIsView,
    SharedArrayBuffer,
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
    Uint16Array,
    Uint8ArraySet,
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
    Set,
    SetAdd,
    SetHas,
    SetDelete,
    SetClear,
    SetKeys,
    SetValues,
    SetEntries,
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
    Error,
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
    EncodeUri,
    EncodeUriComponent,
    DecodeUri,
    DecodeUriComponent,
    StringFromCharCode,
    ParseInt,
    MathLog,
    MathPow,
    MathFloor,
    MathMin,
    MathMax,
    MathRandom,
    NumberString,
    Number,
    NumberIsNaN,
    NumberIsFinite,
    NumberIsInteger,
    NumberFixed,
    NumberPrecision,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum TypedArrayKind {
    Uint8,
    Uint16,
}

impl TypedArrayKind {
    pub(crate) const fn width(self) -> usize {
        match self {
            Self::Uint8 => 1,
            Self::Uint16 => 2,
        }
    }
}

impl Native {
    pub(crate) fn is_typed_array_method(self) -> bool {
        matches!(
            self,
            Self::Uint8ArraySet
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
    },
    Uint8Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
    },
    Uint16Array {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
    },
    DataView {
        object: Object,
        buffer: Value,
        offset: usize,
        length: usize,
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
    pub(crate) fn object(&self) -> Option<&Object> {
        match self {
            Self::Object(object)
            | Self::Array { object, .. }
            | Self::ArrayBuffer { object, .. }
            | Self::Uint8Array { object, .. }
            | Self::Uint16Array { object, .. }
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
            | Self::Uint16Array { object, .. }
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

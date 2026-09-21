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
    JsonParse,
    JsonStringify,
    Array,
    ArrayIsArray,
    ArrayPush,
    ArrayPop,
    ArraySlice,
    ArrayIncludes,
    ArrayJoin,
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

#[derive(Clone, Copy, Debug)]
pub(crate) enum FunctionKind {
    User(u32),
    NumericUser(u32),
    Native(Native),
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum IteratorKind {
    Array,
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

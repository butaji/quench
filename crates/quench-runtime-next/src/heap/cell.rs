use crate::value::Value;
use crate::value_vec::ValueVec;
use std::rc::Rc;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Native {
    Print,
    Object,
    Array,
    ArrayPush,
    ArrayPop,
    FunctionCall,
    Date,
    Error,
    String,
    Symbol,
    StringCharCodeAt,
    StringCharAt,
    StringSubstring,
    StringSubstr,
    StringFromCharCode,
    ParseInt,
    MathLog,
    MathPow,
    MathFloor,
    MathMin,
    MathMax,
    MathRandom,
    NumberString,
    NumberFixed,
    NumberPrecision,
}

#[derive(Clone, Copy, Debug)]
pub(crate) enum FunctionKind {
    User(u32),
    NumericUser(u32),
    Native(Native),
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
            Self::Object(object) | Self::Array { object, .. } => Some(object),
            Self::Function { object, .. } => Some(object),
            _ => None,
        }
    }

    pub(crate) fn object_mut(&mut self) -> Option<&mut Object> {
        match self {
            Self::Object(object) | Self::Array { object, .. } => Some(object),
            Self::Function { object, .. } => Some(object),
            _ => None,
        }
    }
}

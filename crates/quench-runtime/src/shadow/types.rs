//! Shadow tree type definitions.
//!
//! Core types for the Self-Optimizing Shadow Tree Interpreter (SSTI).

use std::cell::Cell;
use std::collections::HashMap;

use crate::arena::Arena;
use crate::interner::Symbol;
use crate::nanbox::JSValue;
use crate::shape::{ShapeId, ShapeRef};

pub const INLINE_SLOTS: usize = 4;

// ============================================================================
// Shape-backed object
// ============================================================================

/// Shape-backed object used by the shadow interpreter.
///
/// Stored in `Context::shadow_arena`, separate from the legacy `Value::Object`
/// representation.
pub struct ShadowObject {
    pub shape: ShapeRef,
    pub inline: [JSValue; INLINE_SLOTS],
    pub out_of_line: Vec<JSValue>,
}

impl ShadowObject {
    pub fn new(shape: ShapeRef) -> Self {
        ShadowObject {
            shape,
            inline: [JSValue::undefined(); INLINE_SLOTS],
            out_of_line: Vec::new(),
        }
    }
}

pub type ShadowArena = Arena<ShadowObject>;

// ============================================================================
// Shadow node
// ============================================================================

/// A node in the shadow tree.
pub enum ShadowNode<'a> {
    Add {
        left: &'a ShadowNode<'a>,
        right: &'a ShadowNode<'a>,
        state: Cell<AddState>,
        hint: TypeHint,
    },
    TypedAdd {
        left: &'a ShadowNode<'a>,
        right: &'a ShadowNode<'a>,
        hint: ExecType,
        state: Cell<AddState>,
    },
    Sub {
        left: &'a ShadowNode<'a>,
        right: &'a ShadowNode<'a>,
    },
    Mul {
        left: &'a ShadowNode<'a>,
        right: &'a ShadowNode<'a>,
    },
    Div {
        left: &'a ShadowNode<'a>,
        right: &'a ShadowNode<'a>,
    },
    PropRead {
        obj: &'a ShadowNode<'a>,
        prop: Symbol,
        cache: Cell<PropCache>,
    },
    TypedPropRead {
        obj: &'a ShadowNode<'a>,
        prop: Symbol,
        obj_hint: ExecType,
        cache: Cell<PropCache>,
    },
    StaticPropRead {
        obj: &'a ShadowNode<'a>,
        prop: Symbol,
        shape_id: ShapeId,
        offset: u16,
        is_inline: bool,
    },
    LiteralInt(i32),
    LiteralDouble(f64),
    LiteralString(Symbol),
    LocalRead(u16),
    GlobalRead(Symbol),
    BindingRead(Binding),
    This,
    Block(Vec<&'a ShadowNode<'a>>),
    Return(&'a ShadowNode<'a>),
    Call {
        callee: &'a ShadowNode<'a>,
        args: Vec<&'a ShadowNode<'a>>,
        target: u16,
    },
    NewObject {
        shape: ShapeRef,
    },
    StoreProp {
        obj: &'a ShadowNode<'a>,
        prop: Symbol,
        value: &'a ShadowNode<'a>,
    },
    StaticPropWrite {
        obj: &'a ShadowNode<'a>,
        prop: Symbol,
        shape_id: ShapeId,
        offset: u16,
        is_inline: bool,
        value: &'a ShadowNode<'a>,
    },
    StoreLocal {
        index: u16,
        value: &'a ShadowNode<'a>,
    },
}

// ============================================================================
// State and hint enums
// ============================================================================

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum AddState {
    Uninitialized,
    Int32,
    Double,
    SpeculativeNumber,
    StringConcat,
    Generic,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TypeHint {
    Any,
    Int32,
    Double,
    Number,
    String,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Binding {
    Local(u16),
    Upvalue(u16),
    Global(Symbol),
    Import(u16),
    ConstInt(i32),
    ConstString(Symbol),
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ModuleMode {
    Dynamic,
    Static,
}

/// Execution-time type used by the TS-aware shadow interpreter.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExecType {
    Unknown,
    Int32,
    Float64,
    String,
    Boolean,
    Symbol,
    BigInt,
    Object(ShapeId),
    Literal(JSValue),
    Void,
}

impl Default for ExecType {
    fn default() -> Self {
        ExecType::Unknown
    }
}

/// Map from identifier symbol to its statically inferred execution type.
#[derive(Debug, Default)]
pub struct TypeMap {
    bindings: HashMap<Symbol, ExecType>,
}

impl TypeMap {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&mut self, sym: Symbol, ty: ExecType) {
        self.bindings.insert(sym, ty);
    }

    pub fn get(&self, sym: Symbol) -> ExecType {
        self.bindings.get(&sym).copied().unwrap_or(ExecType::Unknown)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct PropCache {
    pub shape_id: ShapeId,
    pub offset: u16,
    pub valid: bool,
    pub is_inline: bool,
}

/// An execution frame for the shadow VM.
pub struct ShadowFrame {
    pub bp: usize,
    pub locals: Vec<JSValue>,
}

/// Continuation used by the iterative shadow VM.
pub enum Continuation<'a> {
    Eval(&'a ShadowNode<'a>),
    ApplyAdd(&'a ShadowNode<'a>),
    ApplyTypedAdd(&'a ShadowNode<'a>),
    ApplySub,
    ApplyMul,
    ApplyDiv,
    ApplyPropRead {
        node: &'a ShadowNode<'a>,
        prop: Symbol,
    },
    ApplyTypedPropRead {
        node: &'a ShadowNode<'a>,
        prop: Symbol,
    },
    ApplyCall {
        arg_count: usize,
        target: u16,
    },
    ApplyStoreProp {
        prop: Symbol,
    },
    ApplyStaticPropRead {
        node: &'a ShadowNode<'a>,
    },
    ApplyStaticPropWrite {
        node: &'a ShadowNode<'a>,
    },
    ApplyStoreLocal {
        index: u16,
    },
    PopResult,
    PopFrame,
}

/// Pre-computed layout for an object literal used during lowering.
pub struct ObjectLayout {
    pub shape_id: ShapeId,
    pub offsets: HashMap<Symbol, (u16, bool)>,
}

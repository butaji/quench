//! Self-Optimizing Shadow Tree Interpreter (SSTI) foundation.
//!
//! This is a parallel execution path to the existing recursive AST interpreter
//! and HIR trampoline.  It uses:
//! - a `bumpalo::Bump` arena for shadow-tree nodes;
//! - NaN-boxed `JSValue`s;
//! - shape-backed objects with per-node property-read caches;
//! - an iterative, continuation-based VM that never recurses on the Rust stack.

use std::cell::Cell;
use std::collections::HashMap;

use bumpalo::Bump;

use crate::arena::Arena;
use crate::interner::Symbol;
use crate::nanbox::JSValue;
use crate::shape::{ShapeId, ShapeInterner, ShapeRef};
use crate::value::JsError;
use crate::Context;
use swc_ecma_ast::{Decl, Pat, Stmt, TsEntityName, TsKeywordTypeKind, TsLit, TsType, TsTypeRef};

pub const INLINE_SLOTS: usize = 4;

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

/// The shadow-tree virtual machine.
pub struct ShadowVm<'a> {
    pub value_stack: Vec<JSValue>,
    pub frames: Vec<ShadowFrame>,
    pub work: Vec<Continuation<'a>>,
    pub arena: &'a Bump,
    pub ctx: &'a mut Context,
    pub mode: ModuleMode,
}

impl<'a> ShadowVm<'a> {
    pub fn new(arena: &'a Bump, ctx: &'a mut Context, mode: ModuleMode) -> Self {
        ShadowVm {
            value_stack: Vec::new(),
            frames: Vec::new(),
            work: Vec::new(),
            arena,
            ctx,
            mode,
        }
    }

    pub fn run(&mut self, root: &'a ShadowNode<'a>) -> Result<JSValue, JsError> {
        self.frames.push(ShadowFrame {
            bp: 0,
            locals: Vec::new(),
        });
        self.work.push(Continuation::Eval(root));

        while let Some(cont) = self.work.pop() {
            match cont {
                Continuation::Eval(node) => self.eval_node(node)?,
                Continuation::ApplyAdd(node) => self.apply_add(node)?,
                Continuation::ApplyTypedAdd(node) => self.apply_typed_add(node)?,
                Continuation::ApplySub => self.apply_sub(),
                Continuation::ApplyMul => self.apply_mul(),
                Continuation::ApplyDiv => self.apply_div(),
                Continuation::ApplyPropRead { node, prop } => self.apply_prop_read(node, prop)?,
                Continuation::ApplyTypedPropRead { node, prop } => {
                    self.apply_typed_prop_read(node, prop)?
                }
                Continuation::ApplyCall { arg_count, target } => {
                    self.apply_call(arg_count, target)?
                }
                Continuation::ApplyStoreProp { prop } => self.apply_store_prop(prop)?,
                Continuation::ApplyStaticPropRead { node } => self.apply_static_prop_read(node)?,
                Continuation::ApplyStaticPropWrite { node } => self.apply_static_prop_write(node)?,
                Continuation::ApplyStoreLocal { index } => self.apply_store_local(index),
                Continuation::PopResult => {
                    self.value_stack.pop();
                }
                Continuation::PopFrame => {
                    self.frames.pop();
                }
            }
        }

        Ok(self.value_stack.pop().unwrap_or_else(JSValue::undefined))
    }

    fn eval_node(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        match node {
            ShadowNode::Add { left, right, .. } => {
                self.work.push(Continuation::ApplyAdd(node));
                self.work.push(Continuation::Eval(right));
                self.work.push(Continuation::Eval(left));
            }
            ShadowNode::TypedAdd { left, right, .. } => {
                self.work.push(Continuation::ApplyTypedAdd(node));
                self.work.push(Continuation::Eval(right));
                self.work.push(Continuation::Eval(left));
            }
            ShadowNode::Sub { left, right } => {
                self.work.push(Continuation::ApplySub);
                self.work.push(Continuation::Eval(right));
                self.work.push(Continuation::Eval(left));
            }
            ShadowNode::Mul { left, right } => {
                self.work.push(Continuation::ApplyMul);
                self.work.push(Continuation::Eval(right));
                self.work.push(Continuation::Eval(left));
            }
            ShadowNode::Div { left, right } => {
                self.work.push(Continuation::ApplyDiv);
                self.work.push(Continuation::Eval(right));
                self.work.push(Continuation::Eval(left));
            }
            ShadowNode::PropRead { obj, prop, .. } => {
                self.work
                    .push(Continuation::ApplyPropRead { node, prop: *prop });
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::TypedPropRead { obj, prop, .. } => {
                self.work
                    .push(Continuation::ApplyTypedPropRead { node, prop: *prop });
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::LiteralInt(v) => self.value_stack.push(JSValue::int32(*v)),
            ShadowNode::LiteralDouble(v) => self.value_stack.push(JSValue::double(*v)),
            ShadowNode::LiteralString(sym) => self.value_stack.push(JSValue::string(*sym)),
            ShadowNode::LocalRead(idx) => {
                let frame = self
                    .frames
                    .last()
                    .ok_or_else(|| JsError("no active frame".into()))?;
                let val = frame
                    .locals
                    .get(*idx as usize)
                    .copied()
                    .unwrap_or_else(JSValue::undefined);
                self.value_stack.push(val);
            }
            ShadowNode::GlobalRead(sym) => {
                let name = self.ctx.string_interner.resolve(*sym).unwrap_or("");
                let legacy = self.ctx.get_global(name);
                self.value_stack.push(legacy_to_jsvalue(legacy));
            }
            ShadowNode::BindingRead(binding) => {
                let val = match binding {
                    Binding::Local(slot) => {
                        let frame = self
                            .frames
                            .last()
                            .ok_or_else(|| JsError("no active frame".into()))?;
                        frame
                            .locals
                            .get(*slot as usize)
                            .copied()
                            .unwrap_or_else(JSValue::undefined)
                    }
                    Binding::Global(sym) => {
                        let name = self.ctx.string_interner.resolve(*sym).unwrap_or("");
                        legacy_to_jsvalue(self.ctx.get_global(name))
                    }
                    Binding::ConstInt(v) => JSValue::int32(*v),
                    Binding::ConstString(sym) => JSValue::string(*sym),
                    Binding::Upvalue(_) | Binding::Import(_) => {
                        return Err(JsError("not yet implemented".into()));
                    }
                };
                self.value_stack.push(val);
            }
            ShadowNode::This => self.value_stack.push(JSValue::undefined()),
            ShadowNode::Block(stmts) => {
                if stmts.is_empty() {
                    self.value_stack.push(JSValue::undefined());
                } else {
                    let last = stmts.len() - 1;
                    // Push in reverse so the first statement executes first (the
                    // work list is LIFO).  PopResult is emitted after each
                    // non-last statement to discard its value.
                    for (i, stmt) in stmts.iter().enumerate().rev() {
                        if i != last {
                            self.work.push(Continuation::PopResult);
                        }
                        self.work.push(Continuation::Eval(stmt));
                    }
                }
            }
            ShadowNode::Return(expr) => {
                self.work.push(Continuation::Eval(expr));
                // For the first milestone the source is treated as a single
                // implicit function body, so Return simply yields the value.
            }
            ShadowNode::Call {
                callee,
                args,
                target,
            } => {
                self.work.push(Continuation::ApplyCall {
                    arg_count: args.len(),
                    target: *target,
                });
                self.work.push(Continuation::Eval(callee));
                for arg in args.iter().rev() {
                    self.work.push(Continuation::Eval(arg));
                }
            }
            ShadowNode::NewObject { shape } => {
                let obj = ShadowObject::new(shape.clone());
                let id = self.ctx.shadow_arena.alloc(obj);
                self.value_stack.push(JSValue::object(id));
            }
            ShadowNode::StoreProp { obj, prop, value } => {
                self.work.push(Continuation::ApplyStoreProp { prop: *prop });
                self.work.push(Continuation::Eval(value));
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::StaticPropRead { obj, .. } => {
                self.work
                    .push(Continuation::ApplyStaticPropRead { node });
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::StaticPropWrite { obj, value, .. } => {
                self.work
                    .push(Continuation::ApplyStaticPropWrite { node });
                self.work.push(Continuation::Eval(value));
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::StoreLocal { index, value } => {
                self.work
                    .push(Continuation::ApplyStoreLocal { index: *index });
                self.work.push(Continuation::Eval(value));
            }
        }
        Ok(())
    }

    fn apply_add(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let b = self.pop_one("Add")?;
        let a = self.pop_one("Add")?;

        if let ShadowNode::Add { state, hint, .. } = node {
            let current = state.get();
            let result = match current {
                AddState::Uninitialized => {
                    if *hint == TypeHint::Any {
                        if a.is_int32() && b.is_int32() {
                            let ai = a.as_int32_unchecked();
                            let bi = b.as_int32_unchecked();
                            if let Some(sum) = ai.checked_add(bi) {
                                state.set(AddState::Int32);
                                JSValue::int32(sum)
                            } else {
                                state.set(AddState::Double);
                                JSValue::double(ai as f64 + bi as f64)
                            }
                        } else {
                            let r = self.generic_add(a, b)?;
                            state.set(AddState::Generic);
                            r
                        }
                    } else if *hint == TypeHint::String {
                        let r = self.generic_add(a, b)?;
                        state.set(AddState::Generic);
                        r
                    } else {
                        // Int32 / Double / Number => start as speculative number.
                        state.set(AddState::SpeculativeNumber);
                        if a.is_int32() && b.is_int32() {
                            let ai = a.as_int32_unchecked();
                            let bi = b.as_int32_unchecked();
                            if let Some(sum) = ai.checked_add(bi) {
                                JSValue::int32(sum)
                            } else {
                                JSValue::double(ai as f64 + bi as f64)
                            }
                        } else if (a.is_int32() || a.is_double()) && (b.is_int32() || b.is_double())
                        {
                            let da = if a.is_int32() {
                                a.as_int32_unchecked() as f64
                            } else {
                                a.as_double_unchecked()
                            };
                            let db = if b.is_int32() {
                                b.as_int32_unchecked() as f64
                            } else {
                                b.as_double_unchecked()
                            };
                            JSValue::double(da + db)
                        } else {
                            state.set(AddState::Generic);
                            self.generic_add(a, b)?
                        }
                    }
                }
                AddState::Int32 => {
                    if a.is_int32() && b.is_int32() {
                        let ai = a.as_int32_unchecked();
                        let bi = b.as_int32_unchecked();
                        if let Some(sum) = ai.checked_add(bi) {
                            JSValue::int32(sum)
                        } else {
                            state.set(AddState::Double);
                            JSValue::double(ai as f64 + bi as f64)
                        }
                    } else {
                        state.set(AddState::Generic);
                        self.generic_add(a, b)?
                    }
                }
                AddState::Double => {
                    if a.is_double() && b.is_double() {
                        JSValue::double(a.as_double_unchecked() + b.as_double_unchecked())
                    } else {
                        state.set(AddState::Generic);
                        self.generic_add(a, b)?
                    }
                }
                AddState::SpeculativeNumber => {
                    if (a.is_int32() || a.is_double()) && (b.is_int32() || b.is_double()) {
                        if a.is_int32() && b.is_int32() {
                            let ai = a.as_int32_unchecked();
                            let bi = b.as_int32_unchecked();
                            if let Some(sum) = ai.checked_add(bi) {
                                JSValue::int32(sum)
                            } else {
                                JSValue::double(ai as f64 + bi as f64)
                            }
                        } else {
                            let da = if a.is_int32() {
                                a.as_int32_unchecked() as f64
                            } else {
                                a.as_double_unchecked()
                            };
                            let db = if b.is_int32() {
                                b.as_int32_unchecked() as f64
                            } else {
                                b.as_double_unchecked()
                            };
                            JSValue::double(da + db)
                        }
                    } else {
                        state.set(AddState::Generic);
                        self.generic_add(a, b)?
                    }
                }
                AddState::StringConcat | AddState::Generic => self.generic_add(a, b)?,
            };
            self.value_stack.push(result);
            Ok(())
        } else {
            Err(JsError("ApplyAdd on non-Add node".into()))
        }
    }

    fn apply_typed_add(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let b = self.pop_one("TypedAdd")?;
        let a = self.pop_one("TypedAdd")?;

        if let ShadowNode::TypedAdd { state, hint, .. } = node {
            let current = state.get();
            let result = match current {
                AddState::Uninitialized => match *hint {
                    ExecType::Int32 if a.is_int32() && b.is_int32() => {
                        state.set(AddState::Int32);
                        let ai = a.as_int32_unchecked();
                        let bi = b.as_int32_unchecked();
                        if let Some(sum) = ai.checked_add(bi) {
                            JSValue::int32(sum)
                        } else {
                            state.set(AddState::Double);
                            JSValue::double(ai as f64 + bi as f64)
                        }
                    }
                    ExecType::Int32 | ExecType::Float64
                        if (a.is_int32() || a.is_double()) && (b.is_int32() || b.is_double()) =>
                    {
                        state.set(AddState::Double);
                        let da = if a.is_int32() {
                            a.as_int32_unchecked() as f64
                        } else {
                            a.as_double_unchecked()
                        };
                        let db = if b.is_int32() {
                            b.as_int32_unchecked() as f64
                        } else {
                            b.as_double_unchecked()
                        };
                        JSValue::double(da + db)
                    }
                    ExecType::String if a.is_string() && b.is_string() => {
                        state.set(AddState::StringConcat);
                        let sa = a.as_string().unwrap();
                        let sb = b.as_string().unwrap();
                        let a_str = self.ctx.string_interner.resolve(sa).unwrap_or("");
                        let b_str = self.ctx.string_interner.resolve(sb).unwrap_or("");
                        let sym = self.ctx.string_interner.intern(format!("{}{}", a_str, b_str));
                        JSValue::string(sym)
                    }
                    _ => {
                        state.set(AddState::Generic);
                        self.generic_add(a, b)?
                    }
                },
                AddState::Int32 => {
                    if a.is_int32() && b.is_int32() {
                        let ai = a.as_int32_unchecked();
                        let bi = b.as_int32_unchecked();
                        if let Some(sum) = ai.checked_add(bi) {
                            JSValue::int32(sum)
                        } else {
                            state.set(AddState::Double);
                            JSValue::double(ai as f64 + bi as f64)
                        }
                    } else {
                        state.set(AddState::Generic);
                        self.generic_add(a, b)?
                    }
                }
                AddState::Double => {
                    if (a.is_int32() || a.is_double()) && (b.is_int32() || b.is_double()) {
                        let da = if a.is_int32() {
                            a.as_int32_unchecked() as f64
                        } else {
                            a.as_double_unchecked()
                        };
                        let db = if b.is_int32() {
                            b.as_int32_unchecked() as f64
                        } else {
                            b.as_double_unchecked()
                        };
                        JSValue::double(da + db)
                    } else {
                        state.set(AddState::Generic);
                        self.generic_add(a, b)?
                    }
                }
                AddState::StringConcat => {
                    if a.is_string() && b.is_string() {
                        let sa = a.as_string().unwrap();
                        let sb = b.as_string().unwrap();
                        let a_str = self.ctx.string_interner.resolve(sa).unwrap_or("");
                        let b_str = self.ctx.string_interner.resolve(sb).unwrap_or("");
                        let sym = self.ctx.string_interner.intern(format!("{}{}", a_str, b_str));
                        JSValue::string(sym)
                    } else {
                        state.set(AddState::Generic);
                        self.generic_add(a, b)?
                    }
                }
                AddState::SpeculativeNumber | AddState::Generic => self.generic_add(a, b)?,
            };
            self.value_stack.push(result);
            Ok(())
        } else {
            Err(JsError("ApplyTypedAdd on non-TypedAdd node".into()))
        }
    }

    fn generic_add(&mut self, a: JSValue, b: JSValue) -> Result<JSValue, JsError> {
        if a.is_int32() && b.is_int32() {
            let ai = a.as_int32_unchecked();
            let bi = b.as_int32_unchecked();
            if let Some(sum) = ai.checked_add(bi) {
                return Ok(JSValue::int32(sum));
            }
            return Ok(JSValue::double(ai as f64 + bi as f64));
        }

        if a.is_string() || b.is_string() {
            let sa = self.to_string_symbol(a);
            let sb = self.to_string_symbol(b);
            let a_str = self.ctx.string_interner.resolve(sa).unwrap_or("").to_string();
            let b_str = self.ctx.string_interner.resolve(sb).unwrap_or("").to_string();
            let sym = self.ctx.string_interner.intern(format!("{}{}", a_str, b_str));
            return Ok(JSValue::string(sym));
        }

        let da = to_number(a);
        let db = to_number(b);
        Ok(JSValue::double(da + db))
    }

    fn to_string_symbol(&mut self, v: JSValue) -> Symbol {
        if let Some(sym) = v.as_string() {
            return sym;
        }
        let s = if v.is_int32() {
            format!("{}", v.as_int32_unchecked())
        } else if v.is_double() {
            format!("{}", v.as_double_unchecked())
        } else if v.is_true() {
            "true".to_string()
        } else if v.is_false() {
            "false".to_string()
        } else if v.is_null() {
            "null".to_string()
        } else if v.is_undefined() {
            "undefined".to_string()
        } else {
            "[object Object]".to_string()
        };
        self.ctx.string_interner.intern(s)
    }

    fn apply_sub(&mut self) {
        let b = self.pop_one("Sub").unwrap_or(JSValue::double(f64::NAN));
        let a = self.pop_one("Sub").unwrap_or(JSValue::double(f64::NAN));
        let r = to_number(a) - to_number(b);
        self.value_stack.push(JSValue::double(r));
    }

    fn apply_mul(&mut self) {
        let b = self.pop_one("Mul").unwrap_or(JSValue::double(f64::NAN));
        let a = self.pop_one("Mul").unwrap_or(JSValue::double(f64::NAN));
        let r = to_number(a) * to_number(b);
        self.value_stack.push(JSValue::double(r));
    }

    fn apply_div(&mut self) {
        let b = self.pop_one("Div").unwrap_or(JSValue::double(f64::NAN));
        let a = self.pop_one("Div").unwrap_or(JSValue::double(f64::NAN));
        let r = to_number(a) / to_number(b);
        self.value_stack.push(JSValue::double(r));
    }

    fn apply_prop_read(&mut self, node: &'a ShadowNode<'a>, prop: Symbol) -> Result<(), JsError> {
        let obj_val = self.pop_one("PropRead")?;
        let obj_id = obj_val
            .as_object()
            .ok_or_else(|| JsError("property read on non-object".into()))?;

        let offset;
        let is_inline;
        {
            let obj = self
                .ctx
                .shadow_arena
                .get(obj_id)
                .ok_or_else(|| JsError("bad object id".into()))?;
            let cache = if let ShadowNode::PropRead { cache, .. } = node {
                cache.get()
            } else {
                PropCache::default()
            };

            if cache.valid && cache.shape_id == obj.shape.id {
                // Cached shape matches: read directly.
                offset = cache.offset as usize;
                is_inline = cache.is_inline;
            } else {
                // Slow path: look up the property in the shape.
                let Some(off) = obj.shape.find_offset(prop) else {
                    return Err(JsError(format!(
                        "property not found: {:?}",
                        self.ctx.string_interner.resolve(prop)
                    )));
                };
                let inline = off < INLINE_SLOTS;
                let slot_offset = if inline { off } else { off - INLINE_SLOTS };
                if let ShadowNode::PropRead { cache, .. } = node {
                    cache.set(PropCache {
                        shape_id: obj.shape.id,
                        offset: slot_offset as u16,
                        valid: true,
                        is_inline: inline,
                    });
                }
                offset = slot_offset;
                is_inline = inline;
            }
        }

        let obj = self
            .ctx
            .shadow_arena
            .get(obj_id)
            .ok_or_else(|| JsError("bad object id".into()))?;
        let val = if is_inline {
            obj.inline[offset]
        } else if offset < obj.out_of_line.len() {
            obj.out_of_line[offset]
        } else {
            JSValue::undefined()
        };
        self.value_stack.push(val);
        Ok(())
    }

    fn apply_typed_prop_read(
        &mut self,
        node: &'a ShadowNode<'a>,
        prop: Symbol,
    ) -> Result<(), JsError> {
        let obj_val = self.pop_one("TypedPropRead")?;

        let fast_result = {
            if let ShadowNode::TypedPropRead { obj_hint, cache, .. } = node {
                if let ExecType::Object(expected_shape_id) = *obj_hint {
                    if let Some(obj_id) = obj_val.as_object() {
                        if let Some(obj) = self.ctx.shadow_arena.get(obj_id) {
                            if obj.shape.id == expected_shape_id {
                                Some(self.read_object_prop(&*obj, prop, cache)?)
                            } else {
                                None
                            }
                        } else {
                            None
                        }
                    } else {
                        None
                    }
                } else {
                    None
                }
            } else {
                None
            }
        };

        if let Some(val) = fast_result {
            self.value_stack.push(val);
            return Ok(());
        }

        // Generic fallback: dynamic lookup on the actual object's shape.
        let obj_id = obj_val
            .as_object()
            .ok_or_else(|| JsError("property read on non-object".into()))?;
        let obj = self
            .ctx
            .shadow_arena
            .get(obj_id)
            .ok_or_else(|| JsError("bad object id".into()))?;

        if let ShadowNode::TypedPropRead { cache, .. } = node {
            let val = self.read_object_prop(&*obj, prop, cache)?;
            self.value_stack.push(val);
        } else {
            return Err(JsError("ApplyTypedPropRead on non-TypedPropRead node".into()));
        }
        Ok(())
    }

    fn read_object_prop(
        &self,
        obj: &ShadowObject,
        prop: Symbol,
        cache: &Cell<PropCache>,
    ) -> Result<JSValue, JsError> {
        let cache_val = cache.get();
        let (offset, is_inline) = if cache_val.valid && cache_val.shape_id == obj.shape.id {
            (cache_val.offset as usize, cache_val.is_inline)
        } else {
            let Some(off) = obj.shape.find_offset(prop) else {
                return Err(JsError(format!(
                    "property not found: {:?}",
                    self.ctx.string_interner.resolve(prop)
                )));
            };
            let inline = off < INLINE_SLOTS;
            let slot_offset = if inline { off } else { off - INLINE_SLOTS };
            cache.set(PropCache {
                shape_id: obj.shape.id,
                offset: slot_offset as u16,
                valid: true,
                is_inline: inline,
            });
            (slot_offset, inline)
        };

        let val = if is_inline {
            obj.inline[offset]
        } else if offset < obj.out_of_line.len() {
            obj.out_of_line[offset]
        } else {
            JSValue::undefined()
        };
        Ok(val)
    }

    fn dynamic_store_prop(
        &mut self,
        obj_id: crate::arena::ObjectId,
        prop: Symbol,
        value: JSValue,
    ) -> Result<(), JsError> {
        let new_shape = {
            let obj = self
                .ctx
                .shadow_arena
                .get(obj_id)
                .ok_or_else(|| JsError("bad object id".into()))?;
            if let Some(off) = obj.shape.find_offset(prop) {
                drop(obj);
                let mut obj = self
                    .ctx
                    .shadow_arena
                    .get_mut(obj_id)
                    .ok_or_else(|| JsError("bad object id".into()))?;
                if off < INLINE_SLOTS {
                    obj.inline[off] = value;
                } else {
                    let ool_off = off - INLINE_SLOTS;
                    if ool_off >= obj.out_of_line.len() {
                        obj.out_of_line.resize(ool_off + 1, JSValue::undefined());
                    }
                    obj.out_of_line[ool_off] = value;
                }
                return Ok(());
            }
            self.ctx.shape_interner.add_property(&obj.shape, prop)
        };

        let offset = new_shape
            .find_offset(prop)
            .ok_or_else(|| JsError("shape did not contain property".into()))?;
        let mut obj = self
            .ctx
            .shadow_arena
            .get_mut(obj_id)
            .ok_or_else(|| JsError("bad object id".into()))?;
        obj.shape = new_shape;

        let total = obj.shape.len();
        let ool_count = total.saturating_sub(INLINE_SLOTS);
        obj.out_of_line.resize(ool_count, JSValue::undefined());

        if offset < INLINE_SLOTS {
            obj.inline[offset] = value;
        } else {
            obj.out_of_line[offset - INLINE_SLOTS] = value;
        }
        Ok(())
    }

    fn apply_store_prop(&mut self, prop: Symbol) -> Result<(), JsError> {
        let value = self.pop_one("StoreProp value")?;
        let obj_val = self.pop_one("StoreProp object")?;
        let obj_id = obj_val
            .as_object()
            .ok_or_else(|| JsError("property store on non-object".into()))?;

        self.dynamic_store_prop(obj_id, prop, value)?;
        // StoreProp yields the object so object literals can be chained.
        self.value_stack.push(obj_val);
        Ok(())
    }

    fn apply_static_prop_read(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let obj_val = self.pop_one("StaticPropRead")?;
        let obj_id = obj_val
            .as_object()
            .ok_or_else(|| JsError("property read on non-object".into()))?;
        let obj = self
            .ctx
            .shadow_arena
            .get(obj_id)
            .ok_or_else(|| JsError("bad object id".into()))?;

        let (shape_id, offset, is_inline, prop) =
            if let ShadowNode::StaticPropRead {
                shape_id,
                offset,
                is_inline,
                prop,
                ..
            } = node
            {
                (*shape_id, *offset as usize, *is_inline, *prop)
            } else {
                return Err(JsError("ApplyStaticPropRead on non-static node".into()));
            };

        let val = if obj.shape.id == shape_id {
            if is_inline {
                obj.inline[offset]
            } else if offset < obj.out_of_line.len() {
                obj.out_of_line[offset]
            } else {
                JSValue::undefined()
            }
        } else {
            // Safety check failed: fall back to a dynamic shape lookup.
            let Some(off) = obj.shape.find_offset(prop) else {
                return Err(JsError(format!(
                    "property not found: {:?}",
                    self.ctx.string_interner.resolve(prop)
                )));
            };
            if off < INLINE_SLOTS {
                obj.inline[off]
            } else {
                obj.out_of_line
                    .get(off - INLINE_SLOTS)
                    .copied()
                    .unwrap_or_else(JSValue::undefined)
            }
        };
        self.value_stack.push(val);
        Ok(())
    }

    fn apply_static_prop_write(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let value = self.pop_one("StaticPropWrite value")?;
        let obj_val = self.pop_one("StaticPropWrite object")?;
        let obj_id = obj_val
            .as_object()
            .ok_or_else(|| JsError("property store on non-object".into()))?;

        let (shape_id, offset, is_inline, prop) =
            if let ShadowNode::StaticPropWrite {
                shape_id,
                offset,
                is_inline,
                prop,
                ..
            } = node
            {
                (*shape_id, *offset as usize, *is_inline, *prop)
            } else {
                return Err(JsError("ApplyStaticPropWrite on non-static node".into()));
            };

        let mut obj = self
            .ctx
            .shadow_arena
            .get_mut(obj_id)
            .ok_or_else(|| JsError("bad object id".into()))?;
        if obj.shape.id == shape_id {
            if is_inline {
                obj.inline[offset] = value;
            } else {
                if offset >= obj.out_of_line.len() {
                    obj.out_of_line.resize(offset + 1, JSValue::undefined());
                }
                obj.out_of_line[offset] = value;
            }
        } else {
            // Safety check failed: fall back to dynamic store.
            drop(obj);
            self.dynamic_store_prop(obj_id, prop, value)?;
        }
        self.value_stack.push(obj_val);
        Ok(())
    }

    fn apply_store_local(&mut self, index: u16) {
        let value = self
            .pop_one("StoreLocal")
            .unwrap_or_else(|_| JSValue::undefined());
        let frame = self.frames.last_mut().expect("no active frame");
        let idx = index as usize;
        if frame.locals.len() <= idx {
            frame.locals.resize(idx + 1, JSValue::undefined());
        }
        frame.locals[idx] = value;
        self.value_stack.push(value);
    }

    fn apply_call(&mut self, arg_count: usize, target: u16) -> Result<(), JsError> {
        let _callee = self.pop_one("Call callee")?;
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.pop_one("Call arg")?);
        }
        // Function calls are not required for the first milestone; leave a
        // placeholder result and store it in the target local if requested.
        let result = JSValue::undefined();
        if target as usize >= self.frames.last().map(|f| f.locals.len()).unwrap_or(0) {
            if let Some(frame) = self.frames.last_mut() {
                let idx = target as usize;
                if frame.locals.len() <= idx {
                    frame.locals.resize(idx + 1, JSValue::undefined());
                }
                frame.locals[idx] = result;
            }
        }
        self.value_stack.push(result);
        Ok(())
    }

    fn pop_one(&mut self, _what: &str) -> Result<JSValue, JsError> {
        self.value_stack
            .pop()
            .ok_or_else(|| JsError("value stack underflow".into()))
    }
}

fn to_number(v: JSValue) -> f64 {
    if v.is_int32() {
        v.as_int32_unchecked() as f64
    } else if v.is_double() {
        v.as_double_unchecked()
    } else if v.is_undefined() {
        f64::NAN
    } else if v.is_null() {
        0.0
    } else if v.is_true() {
        1.0
    } else if v.is_false() {
        0.0
    } else {
        f64::NAN
    }
}

fn legacy_to_jsvalue(v: Option<crate::value::Value>) -> JSValue {
    use crate::value::Value;
    match v {
        None | Some(Value::Undefined) => JSValue::undefined(),
        Some(Value::Null) => JSValue::null(),
        Some(Value::Boolean(b)) => JSValue::bool(b),
        Some(Value::Number(n)) => JSValue::double(n),
        Some(Value::String(_s)) => {
            // We cannot intern without a mutable interner here; fall back to
            // representing the string as a double-NaN payload.  This path is
            // unused by the first milestone.
            JSValue::double(f64::NAN)
        }
        Some(Value::ObjectId(id)) => JSValue::object(id),
        _ => JSValue::undefined(),
    }
}

/// Pre-computed layout for an object literal used during lowering.
pub struct ObjectLayout {
    pub shape_id: ShapeId,
    pub offsets: HashMap<Symbol, (u16, bool)>,
}

/// Helper to build shadow trees from source inside `Context::eval_shadow`.
pub struct ShadowBuilder<'a> {
    pub bump: &'a Bump,
    pub interner: &'a mut crate::interner::StringInterner,
    pub shapes: &'a ShapeInterner,
    pub locals: HashMap<String, u16>,
    pub next_local: u16,
    pub bindings: HashMap<String, Binding>,
    pub object_layouts: HashMap<String, ObjectLayout>,
    pub type_map: TypeMap,
}

impl<'a> ShadowBuilder<'a> {
    pub fn new(
        bump: &'a Bump,
        interner: &'a mut crate::interner::StringInterner,
        shapes: &'a ShapeInterner,
    ) -> Self {
        ShadowBuilder {
            bump,
            interner,
            shapes,
            locals: HashMap::new(),
            next_local: 0,
            bindings: HashMap::new(),
            object_layouts: HashMap::new(),
            type_map: TypeMap::new(),
        }
    }

    pub fn collect_type_map(&mut self, script: &swc_ecma_ast::Script) {
        self.type_map = build_type_map(script, self.shapes, self.interner);
    }

    pub fn alloc<T>(&self, value: T) -> &'a T {
        self.bump.alloc(value)
    }

    pub fn local_for(&mut self, name: &str) -> u16 {
        if let Some(&idx) = self.locals.get(name) {
            return idx;
        }
        let idx = self.next_local;
        self.next_local += 1;
        self.locals.insert(name.to_string(), idx);
        idx
    }

    pub fn intern(&mut self, s: &str) -> Symbol {
        self.interner.intern(s)
    }
}

/// Build a `TypeMap` by scanning `var`/`let`/`const` declarations with TypeScript
/// type annotations in the script body.
pub fn build_type_map(
    script: &swc_ecma_ast::Script,
    shapes: &ShapeInterner,
    interner: &mut crate::interner::StringInterner,
) -> TypeMap {
    let mut class_shapes: HashMap<String, ShapeId> = HashMap::new();

    // First pass: collect class declarations so type references can resolve them.
    for stmt in &script.body {
        if let Stmt::Decl(Decl::Class(class)) = stmt {
            let shape = shapes.root();
            class_shapes.insert(class.ident.sym.to_string(), shape.id);
        }
    }

    let mut map = TypeMap::new();
    for stmt in &script.body {
        let Stmt::Decl(Decl::Var(var_decl)) = stmt else { continue };
        for decl in &var_decl.decls {
            let Pat::Ident(ident) = &decl.name else { continue };
            let Some(type_ann) = &ident.type_ann else { continue };
            let ty =
                ts_type_to_exec_type(&type_ann.type_ann, shapes, interner, &class_shapes);
            let sym = interner.intern(ident.id.sym.as_ref());
            map.insert(sym, ty);
        }
    }
    map
}

fn ts_type_to_exec_type(
    ts: &TsType,
    _shapes: &ShapeInterner,
    interner: &mut crate::interner::StringInterner,
    class_shapes: &HashMap<String, ShapeId>,
) -> ExecType {
    match ts {
        TsType::TsKeywordType(kw) => match kw.kind {
            TsKeywordTypeKind::TsNumberKeyword => ExecType::Int32,
            TsKeywordTypeKind::TsStringKeyword => ExecType::String,
            TsKeywordTypeKind::TsBooleanKeyword => ExecType::Boolean,
            TsKeywordTypeKind::TsVoidKeyword => ExecType::Void,
            TsKeywordTypeKind::TsBigIntKeyword => ExecType::BigInt,
            TsKeywordTypeKind::TsSymbolKeyword => ExecType::Symbol,
            _ => ExecType::Unknown,
        },
        TsType::TsLitType(lit) => ts_lit_to_exec_type(&lit.lit, interner),
        TsType::TsTypeRef(TsTypeRef { type_name, .. }) => {
            if let TsEntityName::Ident(ident) = type_name {
                if let Some(&shape_id) = class_shapes.get(&ident.sym.to_string()) {
                    return ExecType::Object(shape_id);
                }
            }
            ExecType::Unknown
        }
        _ => ExecType::Unknown,
    }
}

fn ts_lit_to_exec_type(lit: &TsLit, interner: &mut crate::interner::StringInterner) -> ExecType {
    match lit {
        TsLit::Number(n) => {
            let v = n.value;
            if v.fract() == 0.0 && v >= i32::MIN as f64 && v <= i32::MAX as f64 {
                ExecType::Literal(JSValue::int32(v as i32))
            } else {
                ExecType::Literal(JSValue::double(v))
            }
        }
        TsLit::Str(s) => {
            let sym = interner.intern(s.value.to_string_lossy());
            ExecType::Literal(JSValue::string(sym))
        }
        TsLit::Bool(b) => ExecType::Literal(JSValue::bool(b.value)),
        _ => ExecType::Unknown,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::Context;

    fn make_add_chain<'a>(bump: &'a Bump, depth: usize) -> &'a ShadowNode<'a> {
        let mut node: &'a ShadowNode<'a> = bump.alloc(ShadowNode::LiteralInt(1));
        for i in 2..=depth {
            node = bump.alloc(ShadowNode::Add {
                left: node,
                right: bump.alloc(ShadowNode::LiteralInt(i as i32)),
                state: Cell::new(AddState::Uninitialized),
                hint: TypeHint::Any,
            });
        }
        node
    }

    #[test]
    fn test_shadow_add() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let left = bump.alloc(ShadowNode::LiteralInt(1));
        let right = bump.alloc(ShadowNode::LiteralInt(2));
        let root = bump.alloc(ShadowNode::Add {
            left,
            right,
            state: Cell::new(AddState::Uninitialized),
            hint: TypeHint::Any,
        });
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(root).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 3);
    }

    #[test]
    fn test_shadow_deep_chain_no_stack_overflow() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let root = make_add_chain(&bump, 10_000);
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(root).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 50_005_000);
    }

    #[test]
    fn test_shadow_object_prop_cache() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let a_sym = ctx.string_interner.intern("a");
        let shape = ctx.shape_interner.shape_for(&[a_sym]);

        let obj_node = bump.alloc(ShadowNode::NewObject { shape });
        let store_a = bump.alloc(ShadowNode::StoreProp {
            obj: obj_node,
            prop: a_sym,
            value: bump.alloc(ShadowNode::LiteralInt(3)),
        });
        let read_a = bump.alloc(ShadowNode::PropRead {
            obj: store_a,
            prop: a_sym,
            cache: Cell::new(PropCache::default()),
        });

        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(read_a).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 3);

        // Second execution should hit the cached shape and offset.
        let result2 = vm.run(read_a).unwrap();
        assert!(result2.is_int32());
        assert_eq!(result2.as_int32_unchecked(), 3);
    }

    #[test]
    fn test_shadow_store_local_read() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let store = bump.alloc(ShadowNode::StoreLocal {
            index: 0,
            value: bump.alloc(ShadowNode::LiteralDouble(5.0)),
        });
        let read = bump.alloc(ShadowNode::LocalRead(0));
        let add = bump.alloc(ShadowNode::Add {
            left: read,
            right: read,
            state: Cell::new(AddState::Uninitialized),
            hint: TypeHint::Any,
        });
        let block = bump.alloc(ShadowNode::Block(vec![store, add]));
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(block).unwrap();
        assert!(result.is_double());
        assert_eq!(result.as_double_unchecked(), 10.0);
    }

    #[test]
    fn test_shadow_binding_local() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let store = bump.alloc(ShadowNode::StoreLocal {
            index: 0,
            value: bump.alloc(ShadowNode::LiteralInt(5)),
        });
        let read = bump.alloc(ShadowNode::BindingRead(Binding::Local(0)));
        let add = bump.alloc(ShadowNode::Add {
            left: read,
            right: read,
            state: Cell::new(AddState::Uninitialized),
            hint: TypeHint::Any,
        });
        let block = bump.alloc(ShadowNode::Block(vec![store, add]));
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(block).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 10);
    }

    #[test]
    fn test_shadow_add_speculative_number() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let left = bump.alloc(ShadowNode::LiteralInt(1));
        let right = bump.alloc(ShadowNode::LiteralInt(2));
        let root = bump.alloc(ShadowNode::Add {
            left,
            right,
            state: Cell::new(AddState::Uninitialized),
            hint: TypeHint::Number,
        });
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(root).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 3);
        if let ShadowNode::Add { state, .. } = root {
            assert_eq!(state.get(), AddState::SpeculativeNumber);
        }
    }

    #[test]
    fn test_shadow_add_deoptimize_to_generic() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let s_sym = ctx.string_interner.intern("x");
        let left = bump.alloc(ShadowNode::LiteralInt(1));
        let right = bump.alloc(ShadowNode::LiteralString(s_sym));
        let root = bump.alloc(ShadowNode::Add {
            left,
            right,
            state: Cell::new(AddState::Uninitialized),
            hint: TypeHint::Number,
        });
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(root).unwrap();
        assert!(result.is_string());
        assert_eq!(ctx.string_interner.resolve(result.as_string().unwrap()), Some("1x"));
        if let ShadowNode::Add { state, .. } = root {
            assert_eq!(state.get(), AddState::Generic);
        }
    }

    #[test]
    fn test_shadow_static_prop_read_write() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let a_sym = ctx.string_interner.intern("a");
        let b_sym = ctx.string_interner.intern("b");
        let shape = ctx.shape_interner.shape_for(&[a_sym, b_sym]);

        let obj_node = bump.alloc(ShadowNode::NewObject {
            shape: shape.clone(),
        });
        let write_a = bump.alloc(ShadowNode::StaticPropWrite {
            obj: obj_node,
            prop: a_sym,
            shape_id: shape.id,
            offset: 0,
            is_inline: true,
            value: bump.alloc(ShadowNode::LiteralInt(1)),
        });
        let write_b = bump.alloc(ShadowNode::StaticPropWrite {
            obj: write_a,
            prop: b_sym,
            shape_id: shape.id,
            offset: 1,
            is_inline: true,
            value: bump.alloc(ShadowNode::LiteralInt(2)),
        });
        let store = bump.alloc(ShadowNode::StoreLocal {
            index: 0,
            value: write_b,
        });
        let read_a = bump.alloc(ShadowNode::StaticPropRead {
            obj: bump.alloc(ShadowNode::BindingRead(Binding::Local(0))),
            prop: a_sym,
            shape_id: shape.id,
            offset: 0,
            is_inline: true,
        });
        let read_b = bump.alloc(ShadowNode::StaticPropRead {
            obj: bump.alloc(ShadowNode::BindingRead(Binding::Local(0))),
            prop: b_sym,
            shape_id: shape.id,
            offset: 1,
            is_inline: true,
        });
        let add = bump.alloc(ShadowNode::Add {
            left: read_a,
            right: read_b,
            state: Cell::new(AddState::Uninitialized),
            hint: TypeHint::Any,
        });
        let block = bump.alloc(ShadowNode::Block(vec![store, add]));
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(block).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 3);
    }

    #[test]
    fn test_typed_add_number_hint() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let store_x = bump.alloc(ShadowNode::StoreLocal {
            index: 0,
            value: bump.alloc(ShadowNode::LiteralInt(1)),
        });
        let store_y = bump.alloc(ShadowNode::StoreLocal {
            index: 1,
            value: bump.alloc(ShadowNode::LiteralInt(2)),
        });
        let add = bump.alloc(ShadowNode::TypedAdd {
            left: bump.alloc(ShadowNode::LocalRead(0)),
            right: bump.alloc(ShadowNode::LocalRead(1)),
            hint: ExecType::Int32,
            state: Cell::new(AddState::Uninitialized),
        });
        let block = bump.alloc(ShadowNode::Block(vec![store_x, store_y, add]));
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(block).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 3);
        if let ShadowNode::TypedAdd { state, .. } = add {
            assert_eq!(state.get(), AddState::Int32);
        }
    }

    #[test]
    fn test_typed_add_string_hint() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let a_sym = ctx.string_interner.intern("a");
        let b_sym = ctx.string_interner.intern("b");
        let store_a = bump.alloc(ShadowNode::StoreLocal {
            index: 0,
            value: bump.alloc(ShadowNode::LiteralString(a_sym)),
        });
        let store_b = bump.alloc(ShadowNode::StoreLocal {
            index: 1,
            value: bump.alloc(ShadowNode::LiteralString(b_sym)),
        });
        let add = bump.alloc(ShadowNode::TypedAdd {
            left: bump.alloc(ShadowNode::LocalRead(0)),
            right: bump.alloc(ShadowNode::LocalRead(1)),
            hint: ExecType::String,
            state: Cell::new(AddState::Uninitialized),
        });
        let block = bump.alloc(ShadowNode::Block(vec![store_a, store_b, add]));
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(block).unwrap();
        assert!(result.is_string());
        assert_eq!(
            ctx.string_interner.resolve(result.as_string().unwrap()),
            Some("ab")
        );
        if let ShadowNode::TypedAdd { state, .. } = add {
            assert_eq!(state.get(), AddState::StringConcat);
        }
    }

    #[test]
    fn test_typed_add_deopt_when_lying() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let foo_sym = ctx.string_interner.intern("foo");
        let store_x = bump.alloc(ShadowNode::StoreLocal {
            index: 0,
            value: bump.alloc(ShadowNode::LiteralInt(1)),
        });
        let add = bump.alloc(ShadowNode::TypedAdd {
            left: bump.alloc(ShadowNode::LocalRead(0)),
            right: bump.alloc(ShadowNode::LiteralString(foo_sym)),
            hint: ExecType::Int32,
            state: Cell::new(AddState::Uninitialized),
        });
        let block = bump.alloc(ShadowNode::Block(vec![store_x, add]));
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(block).unwrap();
        assert!(result.is_string());
        assert_eq!(
            ctx.string_interner.resolve(result.as_string().unwrap()),
            Some("1foo")
        );
        if let ShadowNode::TypedAdd { state, .. } = add {
            assert_eq!(state.get(), AddState::Generic);
        }
    }

    #[test]
    fn test_typed_prop_read_class_hint() {
        let mut ctx = Context::new().unwrap();
        let bump = Bump::new();
        let a_sym = ctx.string_interner.intern("a");
        let shape = ctx.shape_interner.shape_for(&[a_sym]);

        let obj_node = bump.alloc(ShadowNode::NewObject {
            shape: shape.clone(),
        });
        let write_a = bump.alloc(ShadowNode::StaticPropWrite {
            obj: obj_node,
            prop: a_sym,
            shape_id: shape.id,
            offset: 0,
            is_inline: true,
            value: bump.alloc(ShadowNode::LiteralInt(7)),
        });
        let store = bump.alloc(ShadowNode::StoreLocal {
            index: 0,
            value: write_a,
        });
        let read_a = bump.alloc(ShadowNode::TypedPropRead {
            obj: bump.alloc(ShadowNode::LocalRead(0)),
            prop: a_sym,
            obj_hint: ExecType::Object(shape.id),
            cache: Cell::new(PropCache::default()),
        });
        let block = bump.alloc(ShadowNode::Block(vec![store, read_a]));
        let mut vm = ShadowVm::new(&bump, &mut ctx, ModuleMode::Dynamic);
        let result = vm.run(block).unwrap();
        assert!(result.is_int32());
        assert_eq!(result.as_int32_unchecked(), 7);
    }
}

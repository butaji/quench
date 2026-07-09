//! Shadow Tree Virtual Machine - Core.
//!
//! Iterative VM that executes shadow tree nodes using a work list.

use bumpalo::Bump;

use crate::nanbox::JSValue;
use crate::value::JsError;
use crate::Context;

use super::types::{Binding, Continuation, ShadowFrame, ShadowNode, ShadowObject};

/// The shadow-tree virtual machine.
pub struct ShadowVm<'a> {
    pub value_stack: Vec<JSValue>,
    pub frames: Vec<ShadowFrame>,
    pub work: Vec<Continuation<'a>>,
    pub arena: &'a Bump,
    pub ctx: &'a mut Context,
    pub mode: super::types::ModuleMode,
}

impl<'a> ShadowVm<'a> {
    pub fn new(arena: &'a Bump, ctx: &'a mut Context, mode: super::types::ModuleMode) -> Self {
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
                Continuation::ApplyTypedPropRead { node, prop } => self.apply_typed_prop_read(node, prop)?,
                Continuation::ApplyCall { arg_count, target } => self.apply_call(arg_count, target)?,
                Continuation::ApplyStoreProp { prop } => self.apply_store_prop(prop)?,
                Continuation::ApplyStaticPropRead { node } => self.apply_static_prop_read(node)?,
                Continuation::ApplyStaticPropWrite { node } => self.apply_static_prop_write(node)?,
                Continuation::ApplyStoreLocal { index } => self.apply_store_local(index),
                Continuation::PopResult => { self.value_stack.pop(); }
                Continuation::PopFrame => { self.frames.pop(); }
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
            ShadowNode::Sub { left, right } | ShadowNode::Mul { left, right } | ShadowNode::Div { left, right } => {
                let cont = if matches!(node, ShadowNode::Sub { .. }) {
                    Continuation::ApplySub
                } else if matches!(node, ShadowNode::Mul { .. }) {
                    Continuation::ApplyMul
                } else {
                    Continuation::ApplyDiv
                };
                self.work.push(cont);
                self.work.push(Continuation::Eval(right));
                self.work.push(Continuation::Eval(left));
            }
            ShadowNode::PropRead { obj, prop, .. } => {
                self.work.push(Continuation::ApplyPropRead { node, prop: *prop });
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::TypedPropRead { obj, prop, .. } => {
                self.work.push(Continuation::ApplyTypedPropRead { node, prop: *prop });
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::LiteralInt(v) => self.value_stack.push(JSValue::int32(*v)),
            ShadowNode::LiteralDouble(v) => self.value_stack.push(JSValue::double(*v)),
            ShadowNode::LiteralString(sym) => self.value_stack.push(JSValue::string(*sym)),
            ShadowNode::LocalRead(idx) => {
                let frame = self.frames.last().ok_or_else(|| JsError("no active frame".into()))?;
                let val = frame.locals.get(*idx as usize).copied().unwrap_or_else(JSValue::undefined);
                self.value_stack.push(val);
            }
            ShadowNode::GlobalRead(sym) => {
                let name = self.ctx.string_interner.resolve(*sym).unwrap_or("");
                let legacy = self.ctx.get_global(name);
                self.value_stack.push(super::helpers::legacy_to_jsvalue(legacy));
            }
            ShadowNode::BindingRead(binding) => {
                let val = self.eval_binding(binding)?;
                self.value_stack.push(val);
            }
            ShadowNode::This => self.value_stack.push(JSValue::undefined()),
            ShadowNode::Block(stmts) => self.eval_block(stmts),
            ShadowNode::Return(expr) => { self.work.push(Continuation::Eval(expr)); }
            ShadowNode::Call { callee, args, target } => {
                self.work.push(Continuation::ApplyCall { arg_count: args.len(), target: *target });
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
                self.work.push(Continuation::ApplyStaticPropRead { node });
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::StaticPropWrite { obj, value, .. } => {
                self.work.push(Continuation::ApplyStaticPropWrite { node });
                self.work.push(Continuation::Eval(value));
                self.work.push(Continuation::Eval(obj));
            }
            ShadowNode::StoreLocal { index, value } => {
                self.work.push(Continuation::ApplyStoreLocal { index: *index });
                self.work.push(Continuation::Eval(value));
            }
        }
        Ok(())
    }

    fn eval_binding(&self, binding: &Binding) -> Result<JSValue, JsError> {
        match binding {
            Binding::Local(slot) => {
                let frame = self.frames.last().ok_or_else(|| JsError("no active frame".into()))?;
                Ok(frame.locals.get(*slot as usize).copied().unwrap_or_else(JSValue::undefined))
            }
            Binding::Global(sym) => {
                let name = self.ctx.string_interner.resolve(*sym).unwrap_or("");
                Ok(super::helpers::legacy_to_jsvalue(self.ctx.get_global(name)))
            }
            Binding::ConstInt(v) => Ok(JSValue::int32(*v)),
            Binding::ConstString(sym) => Ok(JSValue::string(*sym)),
            Binding::Upvalue(_) | Binding::Import(_) => Err(JsError("not yet implemented".into())),
        }
    }

    fn eval_block(&mut self, stmts: &[&'a ShadowNode<'a>]) {
        if stmts.is_empty() {
            self.value_stack.push(JSValue::undefined());
        } else {
            let last = stmts.len() - 1;
            for (i, stmt) in stmts.iter().enumerate().rev() {
                if i != last {
                    self.work.push(Continuation::PopResult);
                }
                self.work.push(Continuation::Eval(stmt));
            }
        }
    }
}

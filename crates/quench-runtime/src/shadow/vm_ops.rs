//! Shadow VM operations.
//!
//! Arithmetic and property access operations for the shadow VM.

use std::cell::Cell;

use crate::arena::ObjectId;
use crate::interner::Symbol;
use crate::nanbox::JSValue;
use crate::value::JsError;

use super::types::{
    AddState, ExecType, INLINE_SLOTS, PropCache, ShadowNode, ShadowObject,
    TypeHint,
};
use super::helpers::{legacy_to_jsvalue, to_number};
use super::vm::ShadowVm;

// ============================================================================
// Arithmetic operations
// ============================================================================

impl<'a> ShadowVm<'a> {
    pub fn apply_add(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let b = self.pop_one("Add")?;
        let a = self.pop_one("Add")?;
        if let ShadowNode::Add { state, hint, .. } = node {
            let result = match state.get() {
                AddState::Uninitialized => self.add_uninitialized(a, b, *hint, state),
                AddState::Int32 => self.add_int32_opt_op(a, b, state),
                AddState::Double => self.add_double_opt_op(a, b, state),
                AddState::SpeculativeNumber => self.add_speculative_opt_op(a, b, state),
                AddState::StringConcat | AddState::Generic => self.generic_add(a, b)?,
            };
            self.value_stack.push(result);
            Ok(())
        } else {
            Err(JsError("ApplyAdd on non-Add node".into()))
        }
    }

    fn add_uninitialized(&mut self, a: JSValue, b: JSValue, hint: TypeHint, state: &Cell<AddState>) -> JSValue {
        if hint == TypeHint::Any && a.is_int32() && b.is_int32() {
            let (val, new_state) = self.add_int32_opt(a, b);
            state.set(new_state);
            val
        } else if hint == TypeHint::Number || hint == TypeHint::Int32 || hint == TypeHint::Double {
            state.set(AddState::SpeculativeNumber);
            self.add_numbers_opt(a, b)
        } else {
            self.generic_add(a, b).unwrap_or(JSValue::undefined())
        }
    }

    fn add_int32_opt_op(&mut self, a: JSValue, b: JSValue, state: &Cell<AddState>) -> JSValue {
        if a.is_int32() && b.is_int32() {
            let (val, new_state) = self.add_int32_opt(a, b);
            state.set(new_state);
            val
        } else {
            state.set(AddState::Generic);
            self.generic_add(a, b).unwrap_or(JSValue::undefined())
        }
    }

    fn add_double_opt_op(&mut self, a: JSValue, b: JSValue, state: &Cell<AddState>) -> JSValue {
        if a.is_double() && b.is_double() {
            JSValue::double(a.as_double_unchecked() + b.as_double_unchecked())
        } else {
            state.set(AddState::Generic);
            self.generic_add(a, b).unwrap_or(JSValue::undefined())
        }
    }

    fn add_speculative_opt_op(&mut self, a: JSValue, b: JSValue, state: &Cell<AddState>) -> JSValue {
        if (a.is_int32() || a.is_double()) && (b.is_int32() || b.is_double()) {
            self.add_numbers_opt(a, b)
        } else {
            state.set(AddState::Generic);
            self.generic_add(a, b).unwrap_or(JSValue::undefined())
        }
    }

    fn add_int32_opt(&self, a: JSValue, b: JSValue) -> (JSValue, AddState) {
        let ai = a.as_int32_unchecked();
        let bi = b.as_int32_unchecked();
        if let Some(sum) = ai.checked_add(bi) {
            (JSValue::int32(sum), AddState::Int32)
        } else {
            (JSValue::double(ai as f64 + bi as f64), AddState::Double)
        }
    }

    fn add_numbers_opt(&self, a: JSValue, b: JSValue) -> JSValue {
        if a.is_int32() && b.is_int32() {
            let (val, _) = self.add_int32_opt(a, b);
            val
        } else {
            let da = if a.is_int32() { a.as_int32_unchecked() as f64 } else { a.as_double_unchecked() };
            let db = if b.is_int32() { b.as_int32_unchecked() as f64 } else { b.as_double_unchecked() };
            JSValue::double(da + db)
        }
    }

    pub fn apply_typed_add(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let b = self.pop_one("TypedAdd")?;
        let a = self.pop_one("TypedAdd")?;
        if let ShadowNode::TypedAdd { state, hint, .. } = node {
            let result = match state.get() {
                AddState::Uninitialized => self.typed_add_uninit(a, b, *hint, state),
                AddState::Int32 => self.add_int32_opt_op(a, b, state),
                AddState::Double => self.add_double_opt_op(a, b, state),
                AddState::StringConcat => self.add_string_concat_opt(a, b, state),
                AddState::SpeculativeNumber | AddState::Generic => self.generic_add(a, b)?,
            };
            self.value_stack.push(result);
            Ok(())
        } else {
            Err(JsError("ApplyTypedAdd on non-TypedAdd node".into()))
        }
    }

    fn typed_add_uninit(&mut self, a: JSValue, b: JSValue, hint: ExecType, state: &Cell<AddState>) -> JSValue {
        match hint {
            ExecType::Int32 if a.is_int32() && b.is_int32() => {
                state.set(AddState::Int32);
                let (val, _) = self.add_int32_opt(a, b);
                val
            }
            ExecType::Int32 | ExecType::Float64 if (a.is_int32() || a.is_double()) && (b.is_int32() || b.is_double()) => {
                state.set(AddState::Double);
                self.add_numbers_opt(a, b)
            }
            ExecType::String if a.is_string() && b.is_string() => {
                state.set(AddState::StringConcat);
                self.concat_strings(a, b)
            }
            _ => {
                state.set(AddState::Generic);
                self.generic_add(a, b).unwrap_or(JSValue::undefined())
            }
        }
    }

    fn add_string_concat_opt(&mut self, a: JSValue, b: JSValue, state: &Cell<AddState>) -> JSValue {
        if a.is_string() && b.is_string() {
            self.concat_strings(a, b)
        } else {
            state.set(AddState::Generic);
            self.generic_add(a, b).unwrap_or(JSValue::undefined())
        }
    }

    fn concat_strings(&mut self, a: JSValue, b: JSValue) -> JSValue {
        let sa = a.as_string().unwrap();
        let sb = b.as_string().unwrap();
        let a_str = self.ctx.string_interner.resolve(sa).unwrap_or("");
        let b_str = self.ctx.string_interner.resolve(sb).unwrap_or("");
        let sym = self.ctx.string_interner.intern(format!("{}{}", a_str, b_str));
        JSValue::string(sym)
    }

    fn generic_add(&mut self, a: JSValue, b: JSValue) -> Result<JSValue, JsError> {
        if a.is_int32() && b.is_int32() {
            let (val, _) = self.add_int32_opt(a, b);
            return Ok(val);
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
        if let Some(sym) = v.as_string() { return sym; }
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

    pub fn apply_sub(&mut self) {
        let b = self.pop_one("Sub").unwrap_or(JSValue::double(f64::NAN));
        let a = self.pop_one("Sub").unwrap_or(JSValue::double(f64::NAN));
        let r = crate::shadow::vm_ops::to_number(a) - crate::shadow::vm_ops::to_number(b);
        self.value_stack.push(JSValue::double(r));
    }

    pub fn apply_mul(&mut self) {
        let b = self.pop_one("Mul").unwrap_or(JSValue::double(f64::NAN));
        let a = self.pop_one("Mul").unwrap_or(JSValue::double(f64::NAN));
        let r = crate::shadow::vm_ops::to_number(a) * crate::shadow::vm_ops::to_number(b);
        self.value_stack.push(JSValue::double(r));
    }

    pub fn apply_div(&mut self) {
        let b = self.pop_one("Div").unwrap_or(JSValue::double(f64::NAN));
        let a = self.pop_one("Div").unwrap_or(JSValue::double(f64::NAN));
        let r = crate::shadow::vm_ops::to_number(a) / crate::shadow::vm_ops::to_number(b);
        self.value_stack.push(JSValue::double(r));
    }
}

// ============================================================================
// Property operations
// ============================================================================

impl<'a> ShadowVm<'a> {
    pub fn apply_prop_read(&mut self, node: &'a ShadowNode<'a>, prop: Symbol) -> Result<(), JsError> {
        let obj_val = self.pop_one("PropRead")?;
        let obj_id = obj_val.as_object().ok_or_else(|| JsError("property read on non-object".into()))?;
        let (offset, is_inline) = self.resolve_prop_offset(node, obj_id, prop)?;
        let obj = self.ctx.shadow_arena.get(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
        let val = if is_inline { obj.inline[offset] } else if offset < obj.out_of_line.len() { obj.out_of_line[offset] } else { JSValue::undefined() };
        self.value_stack.push(val);
        Ok(())
    }

    fn resolve_prop_offset(&self, node: &ShadowNode, obj_id: ObjectId, prop: Symbol) -> Result<(usize, bool), JsError> {
        let obj = self.ctx.shadow_arena.get(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
        let cache = if let ShadowNode::PropRead { cache, .. } = node { cache.get() } else { PropCache::default() };
        if cache.valid && cache.shape_id == obj.shape.id {
            return Ok((cache.offset as usize, cache.is_inline));
        }
        let Some(off) = obj.shape.find_offset(prop) else {
            return Err(JsError(format!("property not found: {:?}", self.ctx.string_interner.resolve(prop))));
        };
        let inline = off < INLINE_SLOTS;
        let slot_offset = if inline { off } else { off - INLINE_SLOTS };
        if let ShadowNode::PropRead { cache, .. } = node {
            cache.set(PropCache { shape_id: obj.shape.id, offset: slot_offset as u16, valid: true, is_inline: inline });
        }
        Ok((slot_offset, inline))
    }

    pub fn apply_typed_prop_read(&mut self, node: &'a ShadowNode<'a>, prop: Symbol) -> Result<(), JsError> {
        let obj_val = self.pop_one("TypedPropRead")?;
        if let Some(val) = self.try_fast_prop_read(node, obj_val, prop) {
            self.value_stack.push(val);
            return Ok(());
        }
        let obj_id = obj_val.as_object().ok_or_else(|| JsError("property read on non-object".into()))?;
        let obj = self.ctx.shadow_arena.get(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
        let val = if let ShadowNode::TypedPropRead { cache, .. } = node {
            self.read_object_prop(&*obj, prop, cache)?
        } else {
            return Err(JsError("ApplyTypedPropRead on non-TypedPropRead node".into()));
        };
        self.value_stack.push(val);
        Ok(())
    }

    fn try_fast_prop_read(&self, node: &ShadowNode, obj_val: JSValue, _prop: Symbol) -> Option<JSValue> {
        if let ShadowNode::TypedPropRead { obj_hint, cache, .. } = node {
            if let ExecType::Object(expected_shape_id) = *obj_hint {
                if let Some(obj_id) = obj_val.as_object() {
                    if let Some(obj) = self.ctx.shadow_arena.get(obj_id) {
                        if obj.shape.id == expected_shape_id {
                            if let ShadowNode::TypedPropRead { cache, .. } = node {
                                return self.read_object_prop(&obj, _prop, cache).ok();
                            }
                        }
                    }
                }
            }
        }
        None
    }

    fn read_object_prop(&self, obj: &ShadowObject, prop: Symbol, cache: &Cell<PropCache>) -> Result<JSValue, JsError> {
        let cache_val = cache.get();
        let (offset, is_inline) = if cache_val.valid && cache_val.shape_id == obj.shape.id {
            (cache_val.offset as usize, cache_val.is_inline)
        } else {
            let Some(off) = obj.shape.find_offset(prop) else {
                return Err(JsError(format!("property not found: {:?}", self.ctx.string_interner.resolve(prop))));
            };
            let inline = off < INLINE_SLOTS;
            let slot_offset = if inline { off } else { off - INLINE_SLOTS };
            cache.set(PropCache { shape_id: obj.shape.id, offset: slot_offset as u16, valid: true, is_inline: inline });
            (slot_offset, inline)
        };
        let val = if is_inline { obj.inline[offset] } else if offset < obj.out_of_line.len() { obj.out_of_line[offset] } else { JSValue::undefined() };
        Ok(val)
    }

    fn dynamic_store_prop(&mut self, obj_id: ObjectId, prop: Symbol, value: JSValue) -> Result<(), JsError> {
        let new_shape = {
            let obj = self.ctx.shadow_arena.get(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
            if let Some(off) = obj.shape.find_offset(prop) {
                drop(obj);
                let mut obj = self.ctx.shadow_arena.get_mut(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
                self.write_object_slot(&mut obj, off, value);
                return Ok(());
            }
            self.ctx.shape_interner.add_property(&obj.shape, prop)
        };
        let offset = new_shape.find_offset(prop).ok_or_else(|| JsError("shape did not contain property".into()))?;
        let mut obj = self.ctx.shadow_arena.get_mut(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
        obj.shape = new_shape;
        let total = obj.shape.len();
        let ool_count = total.saturating_sub(INLINE_SLOTS);
        obj.out_of_line.resize(ool_count, JSValue::undefined());
        self.write_object_slot(&mut obj, offset, value);
        Ok(())
    }

    fn write_object_slot(&self, obj: &mut ShadowObject, offset: usize, value: JSValue) {
        if offset < INLINE_SLOTS {
            obj.inline[offset] = value;
        } else {
            let ool_off = offset - INLINE_SLOTS;
            if ool_off >= obj.out_of_line.len() {
                obj.out_of_line.resize(ool_off + 1, JSValue::undefined());
            }
            obj.out_of_line[ool_off] = value;
        }
    }

    pub fn apply_store_prop(&mut self, prop: Symbol) -> Result<(), JsError> {
        let value = self.pop_one("StoreProp value")?;
        let obj_val = self.pop_one("StoreProp object")?;
        let obj_id = obj_val.as_object().ok_or_else(|| JsError("property store on non-object".into()))?;
        self.dynamic_store_prop(obj_id, prop, value)?;
        self.value_stack.push(obj_val);
        Ok(())
    }

    pub fn apply_static_prop_read(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let obj_val = self.pop_one("StaticPropRead")?;
        let obj_id = obj_val.as_object().ok_or_else(|| JsError("property read on non-object".into()))?;
        let obj = self.ctx.shadow_arena.get(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
        let (shape_id, offset, is_inline, prop) = extract_static_read_info(node)?;
        let val = if obj.shape.id == shape_id {
            self.read_from_object(&*obj, offset, is_inline)
        } else {
            self.read_with_fallback(&*obj, offset, is_inline, prop)?
        };
        self.value_stack.push(val);
        Ok(())
    }

    fn read_from_object(&self, obj: &ShadowObject, offset: usize, is_inline: bool) -> JSValue {
        if is_inline { obj.inline[offset] } else if offset < obj.out_of_line.len() { obj.out_of_line[offset] } else { JSValue::undefined() }
    }

    fn read_with_fallback(&self, obj: &ShadowObject, _offset: usize, _is_inline: bool, prop: Symbol) -> Result<JSValue, JsError> {
        let Some(off) = obj.shape.find_offset(prop) else {
            return Err(JsError(format!("property not found: {:?}", self.ctx.string_interner.resolve(prop))));
        };
        let inline = off < INLINE_SLOTS;
        Ok(if inline { obj.inline[off] } else { obj.out_of_line.get(off - INLINE_SLOTS).copied().unwrap_or_else(JSValue::undefined) })
    }

    pub fn apply_static_prop_write(&mut self, node: &'a ShadowNode<'a>) -> Result<(), JsError> {
        let value = self.pop_one("StaticPropWrite value")?;
        let obj_val = self.pop_one("StaticPropWrite object")?;
        let obj_id = obj_val.as_object().ok_or_else(|| JsError("property store on non-object".into()))?;
        let (shape_id, offset, is_inline, prop) = extract_static_write_info(node)?;
        let mut obj = self.ctx.shadow_arena.get_mut(obj_id).ok_or_else(|| JsError("bad object id".into()))?;
        if obj.shape.id == shape_id {
            self.write_object_slot(&mut obj, offset, value);
        } else {
            drop(obj);
            self.dynamic_store_prop(obj_id, prop, value)?;
        }
        self.value_stack.push(obj_val);
        Ok(())
    }

    pub fn apply_store_local(&mut self, index: u16) {
        let value = self.pop_one("StoreLocal").unwrap_or_else(|_| JSValue::undefined());
        let frame = self.frames.last_mut().expect("no active frame");
        let idx = index as usize;
        if frame.locals.len() <= idx {
            frame.locals.resize(idx + 1, JSValue::undefined());
        }
        frame.locals[idx] = value;
        self.value_stack.push(value);
    }

    pub fn apply_call(&mut self, arg_count: usize, target: u16) -> Result<(), JsError> {
        let _callee = self.pop_one("Call callee")?;
        let mut args = Vec::with_capacity(arg_count);
        for _ in 0..arg_count {
            args.push(self.pop_one("Call arg")?);
        }
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

    pub fn pop_one(&mut self, _what: &str) -> Result<JSValue, JsError> {
        self.value_stack.pop().ok_or_else(|| JsError("value stack underflow".into()))
    }
}

// ============================================================================
// Helper functions
// ============================================================================

fn extract_static_read_info(node: &ShadowNode) -> Result<(crate::shape::ShapeId, usize, bool, Symbol), JsError> {
    if let ShadowNode::StaticPropRead { shape_id, offset, is_inline, prop, .. } = node {
        Ok((*shape_id, *offset as usize, *is_inline, *prop))
    } else {
        Err(JsError("ApplyStaticPropRead on non-static node".into()))
    }
}

fn extract_static_write_info(node: &ShadowNode) -> Result<(crate::shape::ShapeId, usize, bool, Symbol), JsError> {
    if let ShadowNode::StaticPropWrite { shape_id, offset, is_inline, prop, .. } = node {
        Ok((*shape_id, *offset as usize, *is_inline, *prop))
    } else {
        Err(JsError("ApplyStaticPropWrite on non-static node".into()))
    }
}

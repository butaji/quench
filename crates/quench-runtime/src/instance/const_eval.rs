//! Evaluate a const expression against already-bound globals.

use std::cell::RefCell;
use std::rc::Rc;

use super::{Global, InvokeError};
use crate::hir::{ConstExpr, ConstOp};
use crate::native::{Native, RefVal};
use crate::slot::Slot;
use crate::unwind::{Failure, Trap};

pub fn eval(
    expr: &ConstExpr,
    globals: &[Rc<RefCell<Global>>],
    inst: u32,
    heap: Option<&RefCell<crate::gc::GcHeap>>,
    gc_types: &[crate::hir::GcType],
) -> Result<Slot, InvokeError> {
    let mut stack = Vec::new();
    for op in expr.ops.iter() {
        apply(&mut stack, *op, globals, inst, heap, gc_types)?;
    }
    stack.pop().ok_or(InvokeError::Unimplemented)
}

pub fn eval_u64(
    expr: &ConstExpr,
    globals: &[Rc<RefCell<Global>>],
    inst: u32,
) -> Result<u64, InvokeError> {
    match eval(expr, globals, inst, None, &[])? {
        Slot::Native(Native::I32(v)) => Ok(v as u32 as u64),
        Slot::Native(Native::I64(v)) => Ok(v as u64),
        _ => Err(InvokeError::Unimplemented),
    }
}

fn apply(
    stack: &mut Vec<Slot>,
    op: ConstOp,
    globals: &[Rc<RefCell<Global>>],
    inst: u32,
    heap: Option<&RefCell<crate::gc::GcHeap>>,
    gc_types: &[crate::hir::GcType],
) -> Result<(), InvokeError> {
    match op {
        ConstOp::I32(v) => stack.push(Slot::Native(Native::I32(v))),
        ConstOp::I64(v) => stack.push(Slot::Native(Native::I64(v))),
        ConstOp::F32(v) => stack.push(Slot::Native(Native::F32(v))),
        ConstOp::F64(v) => stack.push(Slot::Native(Native::F64(v))),
        ConstOp::V128(v) => stack.push(Slot::Native(Native::V128(v))),
        ConstOp::RefNull => stack.push(Slot::Native(Native::Ref(RefVal::Null))),
        ConstOp::RefFunc(index) => {
            stack.push(Slot::Native(Native::Ref(RefVal::Func { inst, index })))
        }
        ConstOp::GlobalGet(index) => {
            let value = globals
                .get(index as usize)
                .ok_or(InvokeError::Unimplemented)?
                .borrow()
                .value
                .clone();
            stack.push(value);
        }
        ConstOp::I32Add => bin_i32(stack, i32::wrapping_add)?,
        ConstOp::I32Sub => bin_i32(stack, i32::wrapping_sub)?,
        ConstOp::I32Mul => bin_i32(stack, i32::wrapping_mul)?,
        ConstOp::I32And => bin_i32(stack, std::ops::BitAnd::bitand)?,
        ConstOp::I32Or => bin_i32(stack, std::ops::BitOr::bitor)?,
        ConstOp::I32Xor => bin_i32(stack, std::ops::BitXor::bitxor)?,
        ConstOp::I64Add => bin_i64(stack, i64::wrapping_add)?,
        ConstOp::I64Sub => bin_i64(stack, i64::wrapping_sub)?,
        ConstOp::I64Mul => bin_i64(stack, i64::wrapping_mul)?,
        ConstOp::I64And => bin_i64(stack, std::ops::BitAnd::bitand)?,
        ConstOp::I64Or => bin_i64(stack, std::ops::BitOr::bitor)?,
        ConstOp::I64Xor => bin_i64(stack, std::ops::BitXor::bitxor)?,
        ConstOp::ArrayNewDefault(type_idx) => {
            let n = pop_i32(stack)?.max(0) as usize;
            let elem = match gc_types.get(type_idx as usize) {
                Some(crate::hir::GcType::Array { elem, .. }) => *elem,
                _ => crate::hir::GcStorage::Val(crate::hir::Kind::I32),
            };
            let fill = crate::gc::zero_storage(elem);
            let r = if let Some(heap) = heap {
                crate::gc::alloc_array(&mut heap.borrow_mut(), type_idx, elem, vec![fill; n])
            } else {
                RefVal::Array(0)
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
        ConstOp::ArrayNew(type_idx) => {
            let n = pop_i32(stack)?.max(0) as usize;
            let fill = stack.pop().ok_or(InvokeError::Unimplemented)?;
            let elem = match gc_types.get(type_idx as usize) {
                Some(crate::hir::GcType::Array { elem, .. }) => *elem,
                _ => crate::hir::GcStorage::Val(crate::hir::Kind::I32),
            };
            let r = if let Some(heap) = heap {
                crate::gc::alloc_array(&mut heap.borrow_mut(), type_idx, elem, vec![fill; n])
            } else {
                RefVal::Array(0)
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
        ConstOp::StructNew(type_idx) => {
            let n = match gc_types.get(type_idx as usize) {
                Some(crate::hir::GcType::Struct { fields, .. }) => fields.len(),
                _ => 0,
            };
            let mut fields = Vec::with_capacity(n);
            for _ in 0..n {
                fields.push(stack.pop().ok_or(InvokeError::Unimplemented)?);
            }
            fields.reverse();
            let r = if let Some(heap) = heap {
                crate::gc::alloc_struct(&mut heap.borrow_mut(), type_idx, fields, RefVal::Null)
            } else {
                RefVal::Struct(0)
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
        ConstOp::StructNewDesc(type_idx) => {
            let desc = pop_desc(stack)?;
            let n = match gc_types.get(type_idx as usize) {
                Some(crate::hir::GcType::Struct { fields, .. }) => fields.len(),
                _ => 0,
            };
            let mut fields = Vec::with_capacity(n);
            for _ in 0..n {
                fields.push(stack.pop().ok_or(InvokeError::Unimplemented)?);
            }
            fields.reverse();
            let r = if let Some(heap) = heap {
                crate::gc::alloc_struct(&mut heap.borrow_mut(), type_idx, fields, desc)
            } else {
                RefVal::Struct(0)
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
        ConstOp::StructNewDefault(type_idx) => {
            let fields = match gc_types.get(type_idx as usize) {
                Some(crate::hir::GcType::Struct { fields, .. }) => {
                    fields.iter().map(|s| crate::gc::zero_storage(*s)).collect()
                }
                _ => Vec::new(),
            };
            let r = if let Some(heap) = heap {
                crate::gc::alloc_struct(&mut heap.borrow_mut(), type_idx, fields, RefVal::Null)
            } else {
                RefVal::Struct(0)
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
        ConstOp::StructNewDefaultDesc(type_idx) => {
            let desc = pop_desc(stack)?;
            let fields = match gc_types.get(type_idx as usize) {
                Some(crate::hir::GcType::Struct { fields, .. }) => {
                    fields.iter().map(|s| crate::gc::zero_storage(*s)).collect()
                }
                _ => Vec::new(),
            };
            let r = if let Some(heap) = heap {
                crate::gc::alloc_struct(&mut heap.borrow_mut(), type_idx, fields, desc)
            } else {
                RefVal::Struct(0)
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
        ConstOp::ArrayNewFixed { type_idx, n } => {
            let mut elems = Vec::with_capacity(n as usize);
            for _ in 0..n {
                elems.push(stack.pop().ok_or(InvokeError::Unimplemented)?);
            }
            elems.reverse();
            let elem = match gc_types.get(type_idx as usize) {
                Some(crate::hir::GcType::Array { elem, .. }) => *elem,
                _ => crate::hir::GcStorage::Val(crate::hir::Kind::I32),
            };
            let r = if let Some(heap) = heap {
                crate::gc::alloc_array(&mut heap.borrow_mut(), type_idx, elem, elems)
            } else {
                RefVal::Array(0)
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
        ConstOp::RefI31 => {
            let v = pop_i32(stack)?;
            stack.push(Slot::Native(Native::Ref(RefVal::I31(
                v as u32 & 0x7fff_ffff,
            ))));
        }
        ConstOp::AnyConvertExtern | ConstOp::ExternConvertAny => {
            let r = match stack.pop().ok_or(InvokeError::Unimplemented)? {
                Slot::Native(Native::Ref(RefVal::Null)) => RefVal::Null,
                Slot::Native(Native::Ref(RefVal::Extern(id)))
                    if matches!(op, ConstOp::AnyConvertExtern) =>
                {
                    RefVal::Host(id)
                }
                Slot::Native(Native::Ref(RefVal::Host(id)))
                    if matches!(op, ConstOp::ExternConvertAny) =>
                {
                    RefVal::Extern(id)
                }
                Slot::Native(Native::Ref(r)) => r,
                _ => return Err(InvokeError::Unimplemented),
            };
            stack.push(Slot::Native(Native::Ref(r)));
        }
    }
    Ok(())
}

fn bin_i32(stack: &mut Vec<Slot>, op: fn(i32, i32) -> i32) -> Result<(), InvokeError> {
    let b = pop_i32(stack)?;
    let a = pop_i32(stack)?;
    stack.push(Slot::Native(Native::I32(op(a, b))));
    Ok(())
}

fn bin_i64(stack: &mut Vec<Slot>, op: fn(i64, i64) -> i64) -> Result<(), InvokeError> {
    let b = pop_i64(stack)?;
    let a = pop_i64(stack)?;
    stack.push(Slot::Native(Native::I64(op(a, b))));
    Ok(())
}

fn pop_i32(stack: &mut Vec<Slot>) -> Result<i32, InvokeError> {
    match stack.pop() {
        Some(Slot::Native(Native::I32(v))) => Ok(v),
        _ => Err(InvokeError::Unimplemented),
    }
}

fn pop_i64(stack: &mut Vec<Slot>) -> Result<i64, InvokeError> {
    match stack.pop() {
        Some(Slot::Native(Native::I64(v))) => Ok(v),
        _ => Err(InvokeError::Unimplemented),
    }
}

fn pop_desc(stack: &mut Vec<Slot>) -> Result<RefVal, InvokeError> {
    match stack.pop() {
        Some(Slot::Native(Native::Ref(RefVal::Null))) => {
            Err(InvokeError::Failure(Failure::Trap(Trap::NullDescriptor)))
        }
        Some(Slot::Native(Native::Ref(r))) => Ok(r),
        _ => Err(InvokeError::Unimplemented),
    }
}

#[cfg(test)]
mod tests {
    use super::eval;
    use crate::hir::{ConstExpr, ConstOp};

    #[test]
    fn add_consts() {
        let expr = ConstExpr {
            ops: Box::new([ConstOp::I32(2), ConstOp::I32(40), ConstOp::I32Add]),
        };
        let slot = eval(&expr, &[], 0, None, &[]).expect("eval");
        assert_eq!(slot.as_native_i32(), Some(42));
    }
}

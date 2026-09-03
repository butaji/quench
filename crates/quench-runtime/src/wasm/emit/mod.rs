//! Stack-to-register lowering, including structured control.

mod atomic;
mod ops;

use crate::hir::{CatchClause, FuncSig, GcOp, GcType, Inst, Kind, LoadOp, Reg, StoreOp, Ty};
use crate::native::SimdOp;
use wasmparser::{BlockType, FunctionBody, MemArg, Operator};

use super::{kind, LowerError};

enum CtrlKind {
    Func,
    Block,
    Loop,
    If,
    Try,
}

struct Ctrl {
    kind: CtrlKind,
    height: usize,
    results: Vec<Kind>,
    params: Vec<Kind>,
    result_regs: Vec<Reg>,
    param_regs: Vec<Reg>,
    start: u32,
    end_jumps: Vec<usize>,
    else_jump: Option<usize>,
    catch_fixups: Vec<(usize, usize)>,
    catching: bool,
}

pub struct Context<'a> {
    stack: Vec<Reg>,
    code: Vec<Inst>,
    next: u16,
    unreachable: bool,
    ctrl: Vec<Ctrl>,
    types: &'a [FuncSig],
    func_types: &'a [u32],
    gc_types: &'a [GcType],
    tag_arities: &'a [usize],
}

impl<'a> Context<'a> {
    pub fn new(
        locals: u16,
        types: &'a [FuncSig],
        func_types: &'a [u32],
        gc_types: &'a [GcType],
        tag_arities: &'a [usize],
    ) -> Self {
        Self {
            stack: Vec::new(),
            code: Vec::new(),
            next: locals,
            unreachable: false,
            ctrl: Vec::new(),
            types,
            func_types,
            gc_types,
            tag_arities,
        }
    }

    pub fn nregs(&self) -> u16 {
        self.next
    }

    pub fn finish(self) -> Box<[Inst]> {
        self.code.into_boxed_slice()
    }

    pub(super) fn alloc(&mut self) -> Result<Reg, LowerError> {
        let reg = self.next;
        self.next = self.next.checked_add(1).ok_or(LowerError::Unsupported)?;
        Ok(reg)
    }

    pub(super) fn pop(&mut self) -> Result<Reg, LowerError> {
        self.stack.pop().ok_or(LowerError::Unsupported)
    }

    pub(super) fn push(&mut self, reg: Reg) {
        self.stack.push(reg);
    }

    pub(super) fn emit(&mut self, inst: Inst) {
        self.code.push(inst);
    }
}

pub fn body(
    ctx: &mut Context<'_>,
    body: &FunctionBody<'_>,
    results: &[Ty],
) -> Result<(), LowerError> {
    let kinds: Vec<Kind> = results.iter().map(|ty| ty.kind).collect();
    push_ctrl(ctx, CtrlKind::Func, Vec::new(), kinds)?;
    let reader = body.get_operators_reader().map_err(super::parse_err)?;
    for op in reader {
        emit(ctx, op.map_err(super::parse_err)?)?;
    }
    Ok(())
}

fn emit(ctx: &mut Context<'_>, op: Operator<'_>) -> Result<(), LowerError> {
    if ctx.unreachable {
        return emit_unreachable(ctx, op);
    }
    if ops::emit_numeric(ctx, &op)? {
        return Ok(());
    }
    if atomic::emit(ctx, &op)? {
        return Ok(());
    }
    match op {
        Operator::Nop => ctx.emit(Inst::Nop),
        Operator::Unreachable => {
            ctx.emit(Inst::Unreachable);
            ctx.unreachable = true;
        }
        Operator::Drop => {
            ctx.pop()?;
        }
        Operator::Select | Operator::TypedSelect { .. } => emit_select(ctx)?,
        Operator::LocalGet { local_index } => ctx.push(local_index as Reg),
        Operator::LocalSet { local_index } => emit_set(ctx, local_index as Reg)?,
        Operator::LocalTee { local_index } => emit_tee(ctx, local_index as Reg)?,
        Operator::Block { blockty } => emit_block(ctx, blockty, CtrlKind::Block)?,
        Operator::Loop { blockty } => emit_block(ctx, blockty, CtrlKind::Loop)?,
        Operator::If { blockty } => emit_if(ctx, blockty)?,
        Operator::Else => emit_else(ctx)?,
        Operator::End => emit_end(ctx)?,
        Operator::Br { relative_depth } => emit_br(ctx, relative_depth)?,
        Operator::BrIf { relative_depth } => emit_br_if(ctx, relative_depth)?,
        Operator::BrTable { targets } => emit_br_table(ctx, &targets)?,
        Operator::Return => emit_br(ctx, ctx.ctrl.len() as u32 - 1)?,
        Operator::Call { function_index } => emit_call(ctx, function_index)?,
        Operator::CallIndirect {
            type_index,
            table_index,
            ..
        } => emit_call_indirect(ctx, type_index, table_index)?,
        Operator::ReturnCall { function_index } => emit_return_call(ctx, function_index)?,
        Operator::ReturnCallIndirect {
            type_index,
            table_index,
            ..
        } => emit_return_call_indirect(ctx, type_index, table_index)?,
        Operator::CallRef { type_index } => emit_call_ref(ctx, type_index)?,
        Operator::ReturnCallRef { type_index } => emit_return_call_ref(ctx, type_index)?,
        Operator::ThrowRef => emit_throw_ref(ctx)?,
        Operator::BrOnNull { relative_depth } => emit_br_on_null(ctx, relative_depth)?,
        Operator::BrOnNonNull { relative_depth } => emit_br_on_non_null(ctx, relative_depth)?,
        Operator::BrOnCast {
            relative_depth,
            to_ref_type,
            ..
        } => emit_br_on_cast(ctx, relative_depth, to_ref_type, false, false)?,
        Operator::BrOnCastFail {
            relative_depth,
            to_ref_type,
            ..
        } => emit_br_on_cast(ctx, relative_depth, to_ref_type, true, false)?,
        Operator::BrOnCastDescEq {
            relative_depth,
            to_ref_type,
            ..
        } => emit_br_on_cast(ctx, relative_depth, to_ref_type, false, true)?,
        Operator::BrOnCastDescEqFail {
            relative_depth,
            to_ref_type,
            ..
        } => emit_br_on_cast(ctx, relative_depth, to_ref_type, true, true)?,
        Operator::I8x16Shuffle { lanes } => emit_shuffle(ctx, lanes)?,
        Operator::Throw { tag_index } => emit_throw(ctx, tag_index)?,
        Operator::Try { blockty } => emit_legacy_try(ctx, blockty)?,
        Operator::Catch { tag_index } => emit_legacy_catch(ctx, Some(tag_index))?,
        Operator::CatchAll => emit_legacy_catch(ctx, None)?,
        Operator::Delegate { relative_depth } => emit_legacy_delegate(ctx, relative_depth)?,
        Operator::Rethrow { relative_depth } => {
            ctx.emit(Inst::Rethrow {
                depth: relative_depth,
            });
            ctx.unreachable = true;
        }
        Operator::TryTable { try_table } => emit_try_table(ctx, try_table)?,
        Operator::I64Add128 => emit_wide(ctx, crate::hir::WideOp::Add128, 4)?,
        Operator::I64Sub128 => emit_wide(ctx, crate::hir::WideOp::Sub128, 4)?,
        Operator::I64MulWideS => emit_wide(ctx, crate::hir::WideOp::MulWideS, 2)?,
        Operator::I64MulWideU => emit_wide(ctx, crate::hir::WideOp::MulWideU, 2)?,
        Operator::I32Load { memarg } => emit_load(ctx, memarg, LoadOp::I32)?,
        Operator::I64Load { memarg } => emit_load(ctx, memarg, LoadOp::I64)?,
        Operator::F32Load { memarg } => emit_load(ctx, memarg, LoadOp::F32)?,
        Operator::F64Load { memarg } => emit_load(ctx, memarg, LoadOp::F64)?,
        Operator::I32Load8S { memarg } => emit_load(ctx, memarg, LoadOp::I32_8S)?,
        Operator::I32Load8U { memarg } => emit_load(ctx, memarg, LoadOp::I32_8U)?,
        Operator::I32Load16S { memarg } => emit_load(ctx, memarg, LoadOp::I32_16S)?,
        Operator::I32Load16U { memarg } => emit_load(ctx, memarg, LoadOp::I32_16U)?,
        Operator::I64Load8S { memarg } => emit_load(ctx, memarg, LoadOp::I64_8S)?,
        Operator::I64Load8U { memarg } => emit_load(ctx, memarg, LoadOp::I64_8U)?,
        Operator::I64Load16S { memarg } => emit_load(ctx, memarg, LoadOp::I64_16S)?,
        Operator::I64Load16U { memarg } => emit_load(ctx, memarg, LoadOp::I64_16U)?,
        Operator::I64Load32S { memarg } => emit_load(ctx, memarg, LoadOp::I64_32S)?,
        Operator::I64Load32U { memarg } => emit_load(ctx, memarg, LoadOp::I64_32U)?,
        Operator::V128Load { memarg } => emit_load(ctx, memarg, LoadOp::V128)?,
        Operator::V128Load8Splat { memarg } => emit_load(ctx, memarg, LoadOp::V128Splat8)?,
        Operator::V128Load16Splat { memarg } => emit_load(ctx, memarg, LoadOp::V128Splat16)?,
        Operator::V128Load32Splat { memarg } => emit_load(ctx, memarg, LoadOp::V128Splat32)?,
        Operator::V128Load64Splat { memarg } => emit_load(ctx, memarg, LoadOp::V128Splat64)?,
        Operator::V128Load32Zero { memarg } => emit_load(ctx, memarg, LoadOp::V128Zero32)?,
        Operator::V128Load64Zero { memarg } => emit_load(ctx, memarg, LoadOp::V128Zero64)?,
        Operator::V128Load8x8S { memarg } => emit_load(ctx, memarg, LoadOp::V128Ext8x8S)?,
        Operator::V128Load8x8U { memarg } => emit_load(ctx, memarg, LoadOp::V128Ext8x8U)?,
        Operator::V128Load16x4S { memarg } => emit_load(ctx, memarg, LoadOp::V128Ext16x4S)?,
        Operator::V128Load16x4U { memarg } => emit_load(ctx, memarg, LoadOp::V128Ext16x4U)?,
        Operator::V128Load32x2S { memarg } => emit_load(ctx, memarg, LoadOp::V128Ext32x2S)?,
        Operator::V128Load32x2U { memarg } => emit_load(ctx, memarg, LoadOp::V128Ext32x2U)?,
        Operator::V128Load8Lane { memarg, lane } => {
            emit_load_lane(ctx, memarg, LoadOp::I32_8U, SimdOp::I8x16Replace, lane)?
        }
        Operator::V128Load16Lane { memarg, lane } => {
            emit_load_lane(ctx, memarg, LoadOp::I32_16U, SimdOp::I16x8Replace, lane)?
        }
        Operator::V128Load32Lane { memarg, lane } => {
            emit_load_lane(ctx, memarg, LoadOp::I32, SimdOp::I32x4Replace, lane)?
        }
        Operator::V128Load64Lane { memarg, lane } => {
            emit_load_lane(ctx, memarg, LoadOp::I64, SimdOp::I64x2Replace, lane)?
        }
        Operator::V128Store8Lane { memarg, lane } => {
            emit_store_lane(ctx, memarg, StoreOp::I32_8, SimdOp::I8x16ExtractU, lane)?
        }
        Operator::V128Store16Lane { memarg, lane } => {
            emit_store_lane(ctx, memarg, StoreOp::I32_16, SimdOp::I16x8ExtractU, lane)?
        }
        Operator::V128Store32Lane { memarg, lane } => {
            emit_store_lane(ctx, memarg, StoreOp::I32, SimdOp::I32x4Extract, lane)?
        }
        Operator::V128Store64Lane { memarg, lane } => {
            emit_store_lane(ctx, memarg, StoreOp::I64, SimdOp::I64x2Extract, lane)?
        }
        Operator::I32Store { memarg } => emit_store(ctx, memarg, StoreOp::I32)?,
        Operator::I64Store { memarg } => emit_store(ctx, memarg, StoreOp::I64)?,
        Operator::F32Store { memarg } => emit_store(ctx, memarg, StoreOp::F32)?,
        Operator::F64Store { memarg } => emit_store(ctx, memarg, StoreOp::F64)?,
        Operator::I32Store8 { memarg } => emit_store(ctx, memarg, StoreOp::I32_8)?,
        Operator::I32Store16 { memarg } => emit_store(ctx, memarg, StoreOp::I32_16)?,
        Operator::I64Store8 { memarg } => emit_store(ctx, memarg, StoreOp::I64_8)?,
        Operator::I64Store16 { memarg } => emit_store(ctx, memarg, StoreOp::I64_16)?,
        Operator::I64Store32 { memarg } => emit_store(ctx, memarg, StoreOp::I64_32)?,
        Operator::V128Store { memarg } => emit_store(ctx, memarg, StoreOp::V128)?,
        Operator::MemorySize { mem } => emit_memory_size(ctx, mem)?,
        Operator::MemoryGrow { mem } => emit_memory_grow(ctx, mem)?,
        Operator::GlobalGet { global_index } => emit_global_get(ctx, global_index)?,
        Operator::GlobalSet { global_index } => emit_global_set(ctx, global_index)?,
        Operator::MemoryCopy { dst_mem, src_mem } => {
            let len = ctx.pop()?;
            let src = ctx.pop()?;
            let dst = ctx.pop()?;
            ctx.emit(Inst::MemoryCopy {
                dst_mem,
                src_mem,
                dst,
                src,
                len,
            });
        }
        Operator::MemoryFill { mem } => {
            let len = ctx.pop()?;
            let val = ctx.pop()?;
            let dst = ctx.pop()?;
            ctx.emit(Inst::MemoryFill { mem, dst, val, len });
        }
        Operator::MemoryInit { data_index, mem } => {
            let len = ctx.pop()?;
            let src = ctx.pop()?;
            let dst = ctx.pop()?;
            ctx.emit(Inst::MemoryInit {
                mem,
                data: data_index,
                dst,
                src,
                len,
            });
        }
        Operator::DataDrop { data_index } => ctx.emit(Inst::DataDrop { data: data_index }),
        Operator::TableGet { table } => {
            let index = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::TableGet { dst, table, index });
            ctx.push(dst);
        }
        Operator::TableSet { table } => {
            let src = ctx.pop()?;
            let index = ctx.pop()?;
            ctx.emit(Inst::TableSet { table, index, src });
        }
        Operator::TableSize { table } => {
            let dst = ctx.alloc()?;
            ctx.emit(Inst::TableSize { dst, table });
            ctx.push(dst);
        }
        Operator::TableGrow { table } => {
            let delta = ctx.pop()?;
            let fill = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::TableGrow {
                dst,
                table,
                fill,
                delta,
            });
            ctx.push(dst);
        }
        Operator::TableFill { table } => {
            let len = ctx.pop()?;
            let val = ctx.pop()?;
            let dst = ctx.pop()?;
            ctx.emit(Inst::TableFill {
                table,
                dst,
                val,
                len,
            });
        }
        Operator::TableCopy {
            dst_table,
            src_table,
        } => {
            let len = ctx.pop()?;
            let src = ctx.pop()?;
            let dst = ctx.pop()?;
            ctx.emit(Inst::TableCopy {
                dst_table,
                src_table,
                dst,
                src,
                len,
            });
        }
        Operator::TableInit { elem_index, table } => {
            let len = ctx.pop()?;
            let src = ctx.pop()?;
            let dst = ctx.pop()?;
            ctx.emit(Inst::TableInit {
                table,
                elem: elem_index,
                dst,
                src,
                len,
            });
        }
        Operator::ElemDrop { elem_index } => ctx.emit(Inst::ElemDrop { elem: elem_index }),
        Operator::RefNull { .. } => {
            let dst = ctx.alloc()?;
            ctx.emit(Inst::ConstRefNull { dst });
            ctx.push(dst);
        }
        Operator::RefFunc { function_index } => {
            let dst = ctx.alloc()?;
            ctx.emit(Inst::ConstRefFunc {
                dst,
                func: function_index,
            });
            ctx.push(dst);
        }
        Operator::RefIsNull => {
            let src = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::RefIsNull { dst, src });
            ctx.push(dst);
        }
        Operator::RefAsNonNull => {
            let src = ctx.pop()?;
            ctx.emit(Inst::RefAsNonNull { src });
            ctx.push(src);
        }
        Operator::RefI31 => {
            let src = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::RefI31 { dst, src });
            ctx.push(dst);
        }
        Operator::I31GetS => {
            let src = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::I31Get {
                dst,
                src,
                signed: true,
            });
            ctx.push(dst);
        }
        Operator::I31GetU => {
            let src = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::I31Get {
                dst,
                src,
                signed: false,
            });
            ctx.push(dst);
        }
        Operator::RefEq => {
            let rhs = ctx.pop()?;
            let lhs = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::RefEq { dst, lhs, rhs });
            ctx.push(dst);
        }
        other => emit_gc_op(ctx, other)?,
    }
    Ok(())
}

fn emit_unreachable(ctx: &mut Context<'_>, op: Operator<'_>) -> Result<(), LowerError> {
    match op {
        Operator::Block { blockty } => emit_block(ctx, blockty, CtrlKind::Block),
        Operator::Loop { blockty } => emit_block(ctx, blockty, CtrlKind::Loop),
        Operator::If { blockty } => {
            emit_block(ctx, blockty, CtrlKind::If)?;
            ctx.unreachable = true;
            Ok(())
        }
        Operator::Else => emit_else(ctx),
        Operator::End => emit_end(ctx),
        Operator::Try { blockty } => emit_legacy_try(ctx, blockty),
        Operator::Catch { tag_index } => emit_legacy_catch(ctx, Some(tag_index)),
        Operator::CatchAll => emit_legacy_catch(ctx, None),
        Operator::Delegate { relative_depth } => emit_legacy_delegate(ctx, relative_depth),
        _ => Ok(()),
    }
}

fn emit_select(ctx: &mut Context<'_>) -> Result<(), LowerError> {
    let cond = ctx.pop()?;
    let b = ctx.pop()?;
    let a = ctx.pop()?;
    let dst = ctx.alloc()?;
    ctx.emit(Inst::Select { dst, a, b, cond });
    ctx.push(dst);
    Ok(())
}

fn emit_set(ctx: &mut Context<'_>, local: Reg) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    if src != local {
        ctx.emit(Inst::Move { dst: local, src });
    }
    Ok(())
}

fn emit_tee(ctx: &mut Context<'_>, local: Reg) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    if src != local {
        ctx.emit(Inst::Move { dst: local, src });
    }
    ctx.push(local);
    Ok(())
}

fn sig(ctx: &Context<'_>, ty: BlockType) -> Result<(Vec<Kind>, Vec<Kind>), LowerError> {
    match ty {
        BlockType::Empty => Ok((Vec::new(), Vec::new())),
        BlockType::Type(ty) => Ok((Vec::new(), vec![kind(ty)?])),
        BlockType::FuncType(index) => {
            let sig = ctx
                .types
                .get(index as usize)
                .ok_or(LowerError::Unsupported)?;
            Ok((sig.params.to_vec(), sig.results.to_vec()))
        }
    }
}

fn push_ctrl(
    ctx: &mut Context<'_>,
    kind: CtrlKind,
    params: Vec<Kind>,
    results: Vec<Kind>,
) -> Result<(), LowerError> {
    let height = ctx.stack.len().saturating_sub(params.len());
    let param_regs = ctx.stack[height..].to_vec();
    let mut result_regs = Vec::with_capacity(results.len());
    for _ in &results {
        result_regs.push(ctx.alloc()?);
    }
    let start = ctx.code.len() as u32;
    ctx.ctrl.push(Ctrl {
        kind,
        height,
        results,
        params,
        result_regs,
        param_regs,
        start,
        end_jumps: Vec::new(),
        else_jump: None,
        catch_fixups: Vec::new(),
        catching: false,
    });
    Ok(())
}

fn emit_block(ctx: &mut Context<'_>, ty: BlockType, kind: CtrlKind) -> Result<(), LowerError> {
    let (params, results) = sig(ctx, ty)?;
    push_ctrl(ctx, kind, params, results)
}

fn emit_if(ctx: &mut Context<'_>, ty: BlockType) -> Result<(), LowerError> {
    let cond = ctx.pop()?;
    let (params, results) = sig(ctx, ty)?;
    push_ctrl(ctx, CtrlKind::If, params, results)?;
    let jmp = ctx.code.len();
    ctx.emit(Inst::JumpIf {
        cond,
        target: 0,
        zero: true,
    });
    ctx.ctrl.last_mut().unwrap().else_jump = Some(jmp);
    Ok(())
}

fn emit_else(ctx: &mut Context<'_>) -> Result<(), LowerError> {
    let result_regs = ctx
        .ctrl
        .last()
        .ok_or(LowerError::Unsupported)?
        .result_regs
        .clone();
    if !ctx.unreachable {
        move_into(ctx, &result_regs)?;
    }
    let jmp = ctx.code.len();
    ctx.emit(Inst::Jump { target: 0 });
    let (height, param_regs, else_jump) = {
        let ctrl = ctx.ctrl.last_mut().ok_or(LowerError::Unsupported)?;
        ctrl.end_jumps.push(jmp);
        (ctrl.height, ctrl.param_regs.clone(), ctrl.else_jump.take())
    };
    if let Some(else_jump) = else_jump {
        patch(ctx, else_jump, ctx.code.len() as u32);
    }
    ctx.stack.truncate(height);
    ctx.stack.extend_from_slice(&param_regs);
    ctx.unreachable = false;
    Ok(())
}

fn emit_end(ctx: &mut Context<'_>) -> Result<(), LowerError> {
    let ctrl = ctx.ctrl.pop().ok_or(LowerError::Unsupported)?;
    if matches!(ctrl.kind, CtrlKind::If) && ctrl.else_jump.is_some() {
        return emit_end_if_no_else(ctx, ctrl);
    }
    if !ctx.unreachable {
        move_into(ctx, &ctrl.result_regs)?;
    }
    if matches!(ctrl.kind, CtrlKind::Try) && !ctrl.catching {
        ctx.emit(Inst::TryEnd);
    }
    ctx.stack.truncate(ctrl.height);
    ctx.stack.extend_from_slice(&ctrl.result_regs);
    let here = ctx.code.len() as u32;
    for jmp in ctrl.end_jumps {
        patch(ctx, jmp, here);
    }
    for (try_at, catch_i) in ctrl.catch_fixups {
        if let Inst::TryBegin { catches } = &mut ctx.code[try_at] {
            if let Some(c) = catches.get_mut(catch_i) {
                c.target = here;
            }
        }
    }
    if matches!(ctrl.kind, CtrlKind::Func) {
        ctx.emit(Inst::Return {
            srcs: ctrl.result_regs.into_boxed_slice(),
        });
    }
    ctx.unreachable = false;
    Ok(())
}

fn emit_end_if_no_else(ctx: &mut Context<'_>, ctrl: Ctrl) -> Result<(), LowerError> {
    if !ctx.unreachable {
        move_into(ctx, &ctrl.result_regs)?;
    }
    let skip = ctx.code.len();
    ctx.emit(Inst::Jump { target: 0 });
    let else_at = ctx.code.len() as u32;
    if let Some(else_jump) = ctrl.else_jump {
        patch(ctx, else_jump, else_at);
    }
    ctx.stack.truncate(ctrl.height);
    ctx.stack.extend_from_slice(&ctrl.param_regs);
    ctx.unreachable = false;
    move_into(ctx, &ctrl.result_regs)?;
    let join = ctx.code.len() as u32;
    patch(ctx, skip, join);
    for jmp in ctrl.end_jumps {
        patch(ctx, jmp, join);
    }
    ctx.stack.truncate(ctrl.height);
    ctx.stack.extend_from_slice(&ctrl.result_regs);
    ctx.unreachable = false;
    Ok(())
}

fn emit_br(ctx: &mut Context<'_>, depth: u32) -> Result<(), LowerError> {
    branch(ctx, depth)?;
    ctx.unreachable = true;
    Ok(())
}

fn emit_br_if(ctx: &mut Context<'_>, depth: u32) -> Result<(), LowerError> {
    let cond = ctx.pop()?;
    let skip = ctx.code.len();
    ctx.emit(Inst::JumpIf {
        cond,
        target: 0,
        zero: true,
    });
    let saved = ctx.stack.clone();
    branch(ctx, depth)?;
    ctx.stack = saved;
    ctx.unreachable = false;
    patch(ctx, skip, ctx.code.len() as u32);
    Ok(())
}

fn emit_br_on_null(ctx: &mut Context<'_>, depth: u32) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    let cond = ctx.alloc()?;
    ctx.emit(Inst::RefIsNull { dst: cond, src });
    let skip = ctx.code.len();
    ctx.emit(Inst::JumpIf {
        cond,
        target: 0,
        zero: true,
    });
    let saved = ctx.stack.clone();
    branch(ctx, depth)?;
    ctx.stack = saved;
    ctx.push(src);
    ctx.unreachable = false;
    patch(ctx, skip, ctx.code.len() as u32);
    Ok(())
}

fn emit_br_on_non_null(ctx: &mut Context<'_>, depth: u32) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    let cond = ctx.alloc()?;
    ctx.emit(Inst::RefIsNull { dst: cond, src });
    let skip = ctx.code.len();
    ctx.emit(Inst::JumpIf {
        cond,
        target: 0,
        zero: false,
    });
    let saved = ctx.stack.clone();
    ctx.push(src);
    branch(ctx, depth)?;
    ctx.stack = saved;
    ctx.unreachable = false;
    patch(ctx, skip, ctx.code.len() as u32);
    Ok(())
}

fn emit_br_on_cast(
    ctx: &mut Context<'_>,
    depth: u32,
    to: wasmparser::RefType,
    fail: bool,
    with_desc: bool,
) -> Result<(), LowerError> {
    let desc = if with_desc { Some(ctx.pop()?) } else { None };
    let src = ctx.pop()?;
    let (heap, type_idx, exact) = heap_cast(to.heap_type());
    let cond = ctx.alloc()?;
    let op = if with_desc {
        GcOp::RefTestDesc {
            nullable: to.is_nullable(),
            exact,
            type_idx,
        }
    } else {
        GcOp::RefTest {
            nullable: to.is_nullable(),
            exact,
            heap,
            type_idx,
        }
    };
    let args = match desc {
        Some(desc) => Box::new([src, desc]) as Box<[Reg]>,
        None => Box::new([src]),
    };
    ctx.emit(Inst::Gc {
        op,
        dst: cond,
        args,
    });
    let skip = ctx.code.len();
    ctx.emit(Inst::JumpIf {
        cond,
        target: 0,
        zero: !fail,
    });
    let saved = ctx.stack.clone();
    ctx.push(src);
    branch(ctx, depth)?;
    ctx.stack = saved;
    ctx.push(src);
    ctx.unreachable = false;
    patch(ctx, skip, ctx.code.len() as u32);
    Ok(())
}

fn emit_throw_ref(ctx: &mut Context<'_>) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    ctx.emit(Inst::ThrowRef { src });
    ctx.unreachable = true;
    Ok(())
}

fn emit_br_table(ctx: &mut Context<'_>, table: &wasmparser::BrTable<'_>) -> Result<(), LowerError> {
    let index = ctx.pop()?;
    let default = table.default();
    let mut depths = Vec::new();
    for target in table.targets() {
        depths.push(target.map_err(super::parse_err)?);
    }
    let table_at = ctx.code.len();
    ctx.emit(Inst::JumpTable {
        index,
        targets: vec![0; depths.len()].into_boxed_slice(),
        default: 0,
    });
    let saved = ctx.stack.clone();
    let mut stubs = Vec::with_capacity(depths.len());
    for depth in &depths {
        ctx.stack.clone_from(&saved);
        ctx.unreachable = false;
        stubs.push(ctx.code.len() as u32);
        branch(ctx, *depth)?;
    }
    ctx.stack.clone_from(&saved);
    ctx.unreachable = false;
    let default_at = ctx.code.len() as u32;
    branch(ctx, default)?;
    if let Inst::JumpTable {
        targets, default, ..
    } = &mut ctx.code[table_at]
    {
        *targets = stubs.into_boxed_slice();
        *default = default_at;
    }
    ctx.stack = saved;
    ctx.unreachable = true;
    Ok(())
}

fn branch(ctx: &mut Context<'_>, depth: u32) -> Result<(), LowerError> {
    let index = ctx
        .ctrl
        .len()
        .checked_sub(depth as usize + 1)
        .ok_or(LowerError::Unsupported)?;
    let is_loop = matches!(ctx.ctrl[index].kind, CtrlKind::Loop);
    let arity = if is_loop {
        ctx.ctrl[index].params.len()
    } else {
        ctx.ctrl[index].results.len()
    };
    let mut srcs = Vec::with_capacity(arity);
    for _ in 0..arity {
        srcs.push(ctx.pop()?);
    }
    srcs.reverse();
    let dests = if is_loop {
        ctx.ctrl[index].param_regs.clone()
    } else {
        ctx.ctrl[index].result_regs.clone()
    };
    for (dst, src) in dests.iter().zip(srcs) {
        if *dst != src {
            ctx.emit(Inst::Move { dst: *dst, src });
        }
    }
    for i in (index + 1..ctx.ctrl.len()).rev() {
        if matches!(ctx.ctrl[i].kind, CtrlKind::Try) && !ctx.ctrl[i].catching {
            ctx.emit(Inst::TryEnd);
        }
    }
    if is_loop {
        ctx.emit(Inst::Jump {
            target: ctx.ctrl[index].start,
        });
    } else {
        let jmp = ctx.code.len();
        ctx.emit(Inst::Jump { target: 0 });
        ctx.ctrl[index].end_jumps.push(jmp);
    }
    Ok(())
}

fn patch(ctx: &mut Context<'_>, at: usize, target: u32) {
    match &mut ctx.code[at] {
        Inst::Jump { target: slot } => *slot = target,
        Inst::JumpIf { target: slot, .. } => *slot = target,
        _ => {}
    }
}

fn move_into(ctx: &mut Context<'_>, dests: &[Reg]) -> Result<(), LowerError> {
    let mut srcs = Vec::with_capacity(dests.len());
    for _ in dests {
        srcs.push(ctx.pop()?);
    }
    srcs.reverse();
    for (dst, src) in dests.iter().zip(srcs) {
        if *dst != src {
            ctx.emit(Inst::Move { dst: *dst, src });
        }
    }
    Ok(())
}

fn emit_call(ctx: &mut Context<'_>, func: u32) -> Result<(), LowerError> {
    let ty = *ctx
        .func_types
        .get(func as usize)
        .ok_or(LowerError::Unsupported)?;
    let sig = ctx.types.get(ty as usize).ok_or(LowerError::Unsupported)?;
    emit_invoke(
        ctx,
        Inst::Call {
            func,
            args: Box::new([]),
            dsts: Box::new([]),
        },
        sig.params.len(),
        sig.results.len(),
        |args, dsts| Inst::Call { func, args, dsts },
    )
}

fn emit_call_ref(ctx: &mut Context<'_>, type_idx: u32) -> Result<(), LowerError> {
    let func = ctx.pop()?;
    let sig = ctx
        .types
        .get(type_idx as usize)
        .ok_or(LowerError::Unsupported)?;
    emit_invoke(
        ctx,
        Inst::Nop,
        sig.params.len(),
        sig.results.len(),
        |args, dsts| Inst::CallRef {
            type_idx,
            func,
            args,
            dsts,
        },
    )
}

fn emit_wide(
    ctx: &mut Context<'_>,
    op: crate::hir::WideOp,
    nargs: usize,
) -> Result<(), LowerError> {
    let mut args = [0u16; 4];
    for i in (0..nargs).rev() {
        args[i] = ctx.pop()?;
    }
    let dst_lo = ctx.alloc()?;
    let dst_hi = ctx.alloc()?;
    ctx.emit(Inst::Wide {
        op,
        dst_lo,
        dst_hi,
        a: args[0],
        b: args[1],
        c: args[2],
        d: args[3],
    });
    ctx.push(dst_lo);
    ctx.push(dst_hi);
    Ok(())
}

fn emit_shuffle(ctx: &mut Context<'_>, lanes: [u8; 16]) -> Result<(), LowerError> {
    let b = ctx.pop()?;
    let a = ctx.pop()?;
    let dst = ctx.alloc()?;
    ctx.emit(Inst::SimdShuffle { dst, a, b, lanes });
    ctx.push(dst);
    Ok(())
}

fn emit_return_call(ctx: &mut Context<'_>, func: u32) -> Result<(), LowerError> {
    let ty = *ctx
        .func_types
        .get(func as usize)
        .ok_or(LowerError::Unsupported)?;
    let sig = ctx.types.get(ty as usize).ok_or(LowerError::Unsupported)?;
    let mut args = Vec::with_capacity(sig.params.len());
    for _ in 0..sig.params.len() {
        args.push(ctx.pop()?);
    }
    args.reverse();
    ctx.emit(Inst::ReturnCall {
        func,
        args: args.into_boxed_slice(),
    });
    ctx.unreachable = true;
    Ok(())
}

fn emit_return_call_indirect(
    ctx: &mut Context<'_>,
    type_idx: u32,
    table: u32,
) -> Result<(), LowerError> {
    let index = ctx.pop()?;
    let sig = ctx
        .types
        .get(type_idx as usize)
        .ok_or(LowerError::Unsupported)?;
    let mut args = Vec::with_capacity(sig.params.len());
    for _ in 0..sig.params.len() {
        args.push(ctx.pop()?);
    }
    args.reverse();
    ctx.emit(Inst::ReturnCallIndirect {
        table,
        type_idx,
        index,
        args: args.into_boxed_slice(),
    });
    ctx.unreachable = true;
    Ok(())
}

fn emit_return_call_ref(ctx: &mut Context<'_>, type_idx: u32) -> Result<(), LowerError> {
    let func = ctx.pop()?;
    let sig = ctx
        .types
        .get(type_idx as usize)
        .ok_or(LowerError::Unsupported)?;
    let mut args = Vec::with_capacity(sig.params.len());
    for _ in 0..sig.params.len() {
        args.push(ctx.pop()?);
    }
    args.reverse();
    ctx.emit(Inst::ReturnCallRef {
        type_idx,
        func,
        args: args.into_boxed_slice(),
    });
    ctx.unreachable = true;
    Ok(())
}

fn emit_call_indirect(ctx: &mut Context<'_>, type_idx: u32, table: u32) -> Result<(), LowerError> {
    let index = ctx.pop()?;
    let sig = ctx
        .types
        .get(type_idx as usize)
        .ok_or(LowerError::Unsupported)?;
    emit_invoke(
        ctx,
        Inst::Nop,
        sig.params.len(),
        sig.results.len(),
        |args, dsts| Inst::CallIndirect {
            table,
            type_idx,
            index,
            args,
            dsts,
        },
    )
}

fn emit_invoke(
    ctx: &mut Context<'_>,
    _dummy: Inst,
    nparams: usize,
    nresults: usize,
    inst: impl FnOnce(Box<[Reg]>, Box<[Reg]>) -> Inst,
) -> Result<(), LowerError> {
    let mut args = Vec::with_capacity(nparams);
    for _ in 0..nparams {
        args.push(ctx.pop()?);
    }
    args.reverse();
    let mut dsts = Vec::with_capacity(nresults);
    for _ in 0..nresults {
        dsts.push(ctx.alloc()?);
    }
    ctx.emit(inst(
        args.into_boxed_slice(),
        dsts.clone().into_boxed_slice(),
    ));
    for dst in dsts {
        ctx.push(dst);
    }
    Ok(())
}

fn emit_load(ctx: &mut Context<'_>, memarg: MemArg, op: LoadOp) -> Result<(), LowerError> {
    let addr = ctx.pop()?;
    let dst = ctx.alloc()?;
    ctx.emit(Inst::Load {
        dst,
        addr,
        offset: memarg.offset,
        mem: memarg.memory,
        op,
    });
    ctx.push(dst);
    Ok(())
}

fn emit_store(ctx: &mut Context<'_>, memarg: MemArg, op: StoreOp) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    let addr = ctx.pop()?;
    ctx.emit(Inst::Store {
        addr,
        src,
        offset: memarg.offset,
        mem: memarg.memory,
        op,
    });
    Ok(())
}

fn emit_load_lane(
    ctx: &mut Context<'_>,
    memarg: MemArg,
    load: LoadOp,
    replace: SimdOp,
    lane: u8,
) -> Result<(), LowerError> {
    let vec = ctx.pop()?;
    let addr = ctx.pop()?;
    let tmp = ctx.alloc()?;
    ctx.emit(Inst::Load {
        dst: tmp,
        addr,
        offset: memarg.offset,
        mem: memarg.memory,
        op: load,
    });
    let dst = ctx.alloc()?;
    ctx.emit(Inst::Simd {
        op: replace,
        dst,
        a: vec,
        b: tmp,
        c: 0,
        lane,
    });
    ctx.push(dst);
    Ok(())
}

fn emit_store_lane(
    ctx: &mut Context<'_>,
    memarg: MemArg,
    store: StoreOp,
    extract: SimdOp,
    lane: u8,
) -> Result<(), LowerError> {
    let vec = ctx.pop()?;
    let addr = ctx.pop()?;
    let tmp = ctx.alloc()?;
    ctx.emit(Inst::Simd {
        op: extract,
        dst: tmp,
        a: vec,
        b: 0,
        c: 0,
        lane,
    });
    ctx.emit(Inst::Store {
        addr,
        src: tmp,
        offset: memarg.offset,
        mem: memarg.memory,
        op: store,
    });
    Ok(())
}

fn emit_memory_size(ctx: &mut Context<'_>, mem: u32) -> Result<(), LowerError> {
    let dst = ctx.alloc()?;
    ctx.emit(Inst::MemorySize { dst, mem });
    ctx.push(dst);
    Ok(())
}

fn emit_memory_grow(ctx: &mut Context<'_>, mem: u32) -> Result<(), LowerError> {
    let delta = ctx.pop()?;
    let dst = ctx.alloc()?;
    ctx.emit(Inst::MemoryGrow { dst, delta, mem });
    ctx.push(dst);
    Ok(())
}

fn emit_global_get(ctx: &mut Context<'_>, global: u32) -> Result<(), LowerError> {
    let dst = ctx.alloc()?;
    ctx.emit(Inst::GlobalGet { dst, global });
    ctx.push(dst);
    Ok(())
}

fn emit_global_set(ctx: &mut Context<'_>, global: u32) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    ctx.emit(Inst::GlobalSet { global, src });
    Ok(())
}

fn emit_throw(ctx: &mut Context<'_>, tag: u32) -> Result<(), LowerError> {
    let n = ctx.tag_arities.get(tag as usize).copied().unwrap_or(0);
    let mut args = Vec::with_capacity(n);
    for _ in 0..n {
        args.push(ctx.pop()?);
    }
    args.reverse();
    ctx.emit(Inst::Throw {
        tag,
        args: args.into_boxed_slice(),
    });
    ctx.unreachable = true;
    Ok(())
}

fn emit_try_table(ctx: &mut Context<'_>, table: wasmparser::TryTable) -> Result<(), LowerError> {
    let (params, results) = sig(ctx, table.ty)?;
    let try_at = ctx.code.len();
    ctx.emit(Inst::TryBegin {
        catches: Box::new([]),
    });
    let mut clauses = Vec::new();
    for (i, catch) in table.catches.iter().enumerate() {
        let (tag, with_ref, depth) = match *catch {
            wasmparser::Catch::One { tag, label } => (Some(tag), false, label),
            wasmparser::Catch::OneRef { tag, label } => (Some(tag), true, label),
            wasmparser::Catch::All { label } => (None, false, label),
            wasmparser::Catch::AllRef { label } => (None, true, label),
        };
        let index = ctx
            .ctrl
            .len()
            .checked_sub(1 + depth as usize)
            .ok_or(LowerError::Unsupported)?;
        let dsts = ctx.ctrl[index].result_regs.clone().into_boxed_slice();
        ctx.ctrl[index].catch_fixups.push((try_at, i));
        clauses.push(CatchClause {
            tag,
            with_ref,
            target: 0,
            dsts,
        });
        let _ = with_ref;
    }
    if let Inst::TryBegin { catches } = &mut ctx.code[try_at] {
        *catches = clauses.into_boxed_slice();
    }
    push_ctrl(ctx, CtrlKind::Try, params, results)
}

fn emit_legacy_try(ctx: &mut Context<'_>, ty: BlockType) -> Result<(), LowerError> {
    let (params, results) = sig(ctx, ty)?;
    let try_at = ctx.code.len() as u32;
    ctx.emit(Inst::TryBegin {
        catches: Box::new([]),
    });
    push_ctrl(ctx, CtrlKind::Try, params, results)?;
    ctx.ctrl.last_mut().ok_or(LowerError::Unsupported)?.start = try_at;
    Ok(())
}

fn emit_legacy_catch(ctx: &mut Context<'_>, tag: Option<u32>) -> Result<(), LowerError> {
    let n = ctx.ctrl.len();
    let index = n.checked_sub(1).ok_or(LowerError::Unsupported)?;
    if !matches!(ctx.ctrl[index].kind, CtrlKind::Try) {
        return Err(LowerError::Unsupported);
    }
    let height = ctx.ctrl[index].height;
    let result_regs = ctx.ctrl[index].result_regs.clone();
    let try_at = ctx.ctrl[index].start as usize;
    if !ctx.ctrl[index].catching {
        if !ctx.unreachable {
            move_into(ctx, &result_regs)?;
        }
        ctx.emit(Inst::TryEnd);
        let jmp = ctx.code.len();
        ctx.emit(Inst::Jump { target: 0 });
        ctx.ctrl[index].end_jumps.push(jmp);
        ctx.ctrl[index].catching = true;
    } else if !ctx.unreachable {
        move_into(ctx, &result_regs)?;
        let jmp = ctx.code.len();
        ctx.emit(Inst::Jump { target: 0 });
        ctx.ctrl[index].end_jumps.push(jmp);
    }
    ctx.stack.truncate(height);
    ctx.unreachable = false;
    let arity = tag
        .and_then(|t| ctx.tag_arities.get(t as usize).copied())
        .unwrap_or(0);
    let mut dsts = Vec::with_capacity(arity);
    for _ in 0..arity {
        let r = ctx.alloc()?;
        ctx.push(r);
        dsts.push(r);
    }
    let here = ctx.code.len() as u32;
    if let Inst::TryBegin { catches } = &mut ctx.code[try_at] {
        let mut list = catches.to_vec();
        list.push(CatchClause {
            tag,
            with_ref: false,
            target: here,
            dsts: dsts.into_boxed_slice(),
        });
        *catches = list.into_boxed_slice();
    }
    Ok(())
}

fn emit_legacy_delegate(ctx: &mut Context<'_>, depth: u32) -> Result<(), LowerError> {
    let try_at = ctx
        .ctrl
        .last()
        .map(|c| c.start as usize)
        .ok_or(LowerError::Unsupported)?;
    if let Inst::TryBegin { catches } = &mut ctx.code[try_at] {
        *catches = Box::new([CatchClause {
            tag: None,
            with_ref: false,
            target: 0x8000_0000 | depth,
            dsts: Box::new([]),
        }]);
    }
    emit_end(ctx)
}

fn emit_gc(ctx: &mut Context<'_>, op: GcOp, nargs: usize, has_dst: bool) -> Result<(), LowerError> {
    let mut args = Vec::with_capacity(nargs);
    for _ in 0..nargs {
        args.push(ctx.pop()?);
    }
    args.reverse();
    let dst = if has_dst { ctx.alloc()? } else { 0 };
    ctx.emit(Inst::Gc {
        op,
        dst,
        args: args.into_boxed_slice(),
    });
    if has_dst {
        ctx.push(dst);
    }
    Ok(())
}

fn emit_gc_op(ctx: &mut Context<'_>, op: Operator<'_>) -> Result<(), LowerError> {
    match op {
        Operator::StructNewDefault { struct_type_index } => emit_gc(
            ctx,
            GcOp::StructNewDefault {
                type_idx: struct_type_index,
            },
            0,
            true,
        ),
        Operator::StructNew { struct_type_index } => {
            let n = match ctx.gc_types.get(struct_type_index as usize) {
                Some(GcType::Struct { fields, .. }) => fields.len(),
                _ => 0,
            };
            emit_gc(
                ctx,
                GcOp::StructNew {
                    type_idx: struct_type_index,
                },
                n,
                true,
            )
        }
        Operator::StructNewDesc { struct_type_index } => {
            let n = match ctx.gc_types.get(struct_type_index as usize) {
                Some(GcType::Struct { fields, .. }) => fields.len(),
                _ => 0,
            };
            emit_gc(
                ctx,
                GcOp::StructNewDesc {
                    type_idx: struct_type_index,
                },
                n + 1,
                true,
            )
        }
        Operator::StructNewDefaultDesc { struct_type_index } => emit_gc(
            ctx,
            GcOp::StructNewDefaultDesc {
                type_idx: struct_type_index,
            },
            1,
            true,
        ),
        Operator::RefGetDesc { type_index: _ } => emit_gc(ctx, GcOp::RefGetDesc, 1, true),
        Operator::RefCastDescEqNonNull { hty } => emit_cast_desc(ctx, false, hty),
        Operator::RefCastDescEqNullable { hty } => emit_cast_desc(ctx, true, hty),
        Operator::StructGet {
            struct_type_index,
            field_index,
        } => emit_gc(
            ctx,
            GcOp::StructGet {
                field: field_index,
                signed: None,
                pack: pack_of(ctx, struct_type_index, field_index),
            },
            1,
            true,
        ),
        Operator::StructGetS {
            struct_type_index,
            field_index,
        } => emit_gc(
            ctx,
            GcOp::StructGet {
                field: field_index,
                signed: Some(true),
                pack: pack_of(ctx, struct_type_index, field_index),
            },
            1,
            true,
        ),
        Operator::StructGetU {
            struct_type_index,
            field_index,
        } => emit_gc(
            ctx,
            GcOp::StructGet {
                field: field_index,
                signed: Some(false),
                pack: pack_of(ctx, struct_type_index, field_index),
            },
            1,
            true,
        ),
        Operator::StructSet {
            struct_type_index: _,
            field_index,
        } => emit_gc(ctx, GcOp::StructSet { field: field_index }, 2, false),
        Operator::ArrayNew { array_type_index } => emit_gc(
            ctx,
            GcOp::ArrayNew {
                type_idx: array_type_index,
            },
            2,
            true,
        ),
        Operator::ArrayNewDefault { array_type_index } => emit_gc(
            ctx,
            GcOp::ArrayNewDefault {
                type_idx: array_type_index,
            },
            1,
            true,
        ),
        Operator::ArrayNewFixed {
            array_type_index,
            array_size,
        } => emit_gc(
            ctx,
            GcOp::ArrayNewFixed {
                type_idx: array_type_index,
                n: array_size,
            },
            array_size as usize,
            true,
        ),
        Operator::ArrayGet { array_type_index } => emit_gc(
            ctx,
            GcOp::ArrayGet {
                signed: None,
                pack: array_pack(ctx, array_type_index),
            },
            2,
            true,
        ),
        Operator::ArrayGetS { array_type_index } => emit_gc(
            ctx,
            GcOp::ArrayGet {
                signed: Some(true),
                pack: array_pack(ctx, array_type_index),
            },
            2,
            true,
        ),
        Operator::ArrayGetU { array_type_index } => emit_gc(
            ctx,
            GcOp::ArrayGet {
                signed: Some(false),
                pack: array_pack(ctx, array_type_index),
            },
            2,
            true,
        ),
        Operator::ArraySet {
            array_type_index: _,
        } => emit_gc(ctx, GcOp::ArraySet, 3, false),
        Operator::ArrayLen => emit_gc(ctx, GcOp::ArrayLen, 1, true),
        Operator::ArrayFill {
            array_type_index: _,
        } => emit_gc(ctx, GcOp::ArrayFill, 4, false),
        Operator::ArrayCopy { .. } => emit_gc(ctx, GcOp::ArrayCopy, 5, false),
        Operator::ArrayNewData {
            array_type_index,
            array_data_index,
        } => emit_gc(
            ctx,
            GcOp::ArrayNewData {
                type_idx: array_type_index,
                data: array_data_index,
            },
            2,
            true,
        ),
        Operator::ArrayNewElem {
            array_type_index,
            array_elem_index,
        } => emit_gc(
            ctx,
            GcOp::ArrayNewElem {
                type_idx: array_type_index,
                elem: array_elem_index,
            },
            2,
            true,
        ),
        Operator::ArrayInitData {
            array_type_index: _,
            array_data_index,
        } => emit_gc(
            ctx,
            GcOp::ArrayInitData {
                data: array_data_index,
            },
            4,
            false,
        ),
        Operator::ArrayInitElem {
            array_type_index: _,
            array_elem_index,
        } => emit_gc(
            ctx,
            GcOp::ArrayInitElem {
                elem: array_elem_index,
            },
            4,
            false,
        ),
        Operator::RefCastNonNull { hty } => emit_cast(ctx, false, hty),
        Operator::RefCastNullable { hty } => emit_cast(ctx, true, hty),
        Operator::RefTestNonNull { hty } => emit_test(ctx, false, hty),
        Operator::RefTestNullable { hty } => emit_test(ctx, true, hty),
        Operator::AnyConvertExtern => emit_gc(ctx, GcOp::AnyConvertExtern, 1, true),
        Operator::ExternConvertAny => emit_gc(ctx, GcOp::ExternConvertAny, 1, true),
        _ => Err(LowerError::Unsupported),
    }
}

fn emit_cast(
    ctx: &mut Context<'_>,
    nullable: bool,
    hty: wasmparser::HeapType,
) -> Result<(), LowerError> {
    let (heap, type_idx, exact) = heap_cast(hty);
    emit_gc(
        ctx,
        GcOp::RefCast {
            nullable,
            exact,
            heap,
            type_idx,
        },
        1,
        true,
    )
}

fn emit_test(
    ctx: &mut Context<'_>,
    nullable: bool,
    hty: wasmparser::HeapType,
) -> Result<(), LowerError> {
    let (heap, type_idx, exact) = heap_cast(hty);
    emit_gc(
        ctx,
        GcOp::RefTest {
            nullable,
            exact,
            heap,
            type_idx,
        },
        1,
        true,
    )
}

fn emit_cast_desc(
    ctx: &mut Context<'_>,
    nullable: bool,
    hty: wasmparser::HeapType,
) -> Result<(), LowerError> {
    let (_heap, type_idx, exact) = heap_cast(hty);
    emit_gc(
        ctx,
        GcOp::RefCastDesc {
            nullable,
            exact,
            type_idx,
        },
        2,
        true,
    )
}

fn pack_of(ctx: &Context<'_>, type_idx: u32, field: u32) -> u8 {
    match ctx.gc_types.get(type_idx as usize) {
        Some(GcType::Struct { fields, .. }) => match fields.get(field as usize) {
            Some(crate::hir::GcStorage::I8) => 8,
            Some(crate::hir::GcStorage::I16) => 16,
            _ => 0,
        },
        _ => 0,
    }
}

fn array_pack(ctx: &Context<'_>, type_idx: u32) -> u8 {
    match ctx.gc_types.get(type_idx as usize) {
        Some(GcType::Array {
            elem: crate::hir::GcStorage::I8,
            ..
        }) => 8,
        Some(GcType::Array {
            elem: crate::hir::GcStorage::I16,
            ..
        }) => 16,
        _ => 0,
    }
}

fn heap_cast(hty: wasmparser::HeapType) -> (crate::hir::HeapKind, Option<u32>, bool) {
    match hty {
        wasmparser::HeapType::Abstract { ty, .. } => {
            use wasmparser::AbstractHeapType::*;
            let heap = match ty {
                Func => crate::hir::HeapKind::Func,
                Extern => crate::hir::HeapKind::Extern,
                Any => crate::hir::HeapKind::Any,
                Eq => crate::hir::HeapKind::Eq,
                I31 => crate::hir::HeapKind::I31,
                Struct => crate::hir::HeapKind::Struct,
                Array => crate::hir::HeapKind::Array,
                None => crate::hir::HeapKind::None,
                NoFunc => crate::hir::HeapKind::NoFunc,
                NoExtern => crate::hir::HeapKind::NoExtern,
                Exn => crate::hir::HeapKind::Exn,
                NoExn => crate::hir::HeapKind::NoExn,
                _ => crate::hir::HeapKind::Other,
            };
            (heap, Option::None, false)
        }
        wasmparser::HeapType::Concrete(idx) => (
            crate::hir::HeapKind::Concrete,
            idx.as_module_index().or(idx.as_rec_group_index()),
            false,
        ),
        wasmparser::HeapType::Exact(idx) => (
            crate::hir::HeapKind::Concrete,
            idx.as_module_index().or(idx.as_rec_group_index()),
            true,
        ),
    }
}

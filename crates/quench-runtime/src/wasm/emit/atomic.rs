//! Threads atomics → one HIR Atomic op.

use crate::hir::{AtomicOp, Inst};
use wasmparser::{MemArg, Operator};

use super::{Context, LowerError};

pub fn emit(ctx: &mut Context<'_>, op: &Operator<'_>) -> Result<bool, LowerError> {
    let Some((kind, memarg, bytes, wide, pops, has_dst)) = decode(op) else {
        return Ok(false);
    };
    emit_op(ctx, kind, memarg, bytes, wide, pops, has_dst)?;
    Ok(true)
}

fn decode(op: &Operator<'_>) -> Option<(AtomicOp, Option<MemArg>, u8, bool, usize, bool)> {
    Some(match *op {
        Operator::I32AtomicLoad { memarg } => (AtomicOp::Load, Some(memarg), 4, false, 1, true),
        Operator::I64AtomicLoad { memarg } => (AtomicOp::Load, Some(memarg), 8, true, 1, true),
        Operator::I32AtomicLoad8U { memarg } => (AtomicOp::Load, Some(memarg), 1, false, 1, true),
        Operator::I32AtomicLoad16U { memarg } => (AtomicOp::Load, Some(memarg), 2, false, 1, true),
        Operator::I64AtomicLoad8U { memarg } => (AtomicOp::Load, Some(memarg), 1, true, 1, true),
        Operator::I64AtomicLoad16U { memarg } => (AtomicOp::Load, Some(memarg), 2, true, 1, true),
        Operator::I64AtomicLoad32U { memarg } => (AtomicOp::Load, Some(memarg), 4, true, 1, true),
        Operator::I32AtomicStore { memarg } => (AtomicOp::Store, Some(memarg), 4, false, 2, false),
        Operator::I64AtomicStore { memarg } => (AtomicOp::Store, Some(memarg), 8, true, 2, false),
        Operator::I32AtomicStore8 { memarg } => (AtomicOp::Store, Some(memarg), 1, false, 2, false),
        Operator::I32AtomicStore16 { memarg } => {
            (AtomicOp::Store, Some(memarg), 2, false, 2, false)
        }
        Operator::I64AtomicStore8 { memarg } => (AtomicOp::Store, Some(memarg), 1, true, 2, false),
        Operator::I64AtomicStore16 { memarg } => (AtomicOp::Store, Some(memarg), 2, true, 2, false),
        Operator::I64AtomicStore32 { memarg } => (AtomicOp::Store, Some(memarg), 4, true, 2, false),
        Operator::I32AtomicRmwAdd { memarg } => (AtomicOp::Add, Some(memarg), 4, false, 2, true),
        Operator::I64AtomicRmwAdd { memarg } => (AtomicOp::Add, Some(memarg), 8, true, 2, true),
        Operator::I32AtomicRmw8AddU { memarg } => (AtomicOp::Add, Some(memarg), 1, false, 2, true),
        Operator::I32AtomicRmw16AddU { memarg } => (AtomicOp::Add, Some(memarg), 2, false, 2, true),
        Operator::I64AtomicRmw8AddU { memarg } => (AtomicOp::Add, Some(memarg), 1, true, 2, true),
        Operator::I64AtomicRmw16AddU { memarg } => (AtomicOp::Add, Some(memarg), 2, true, 2, true),
        Operator::I64AtomicRmw32AddU { memarg } => (AtomicOp::Add, Some(memarg), 4, true, 2, true),
        Operator::I32AtomicRmwSub { memarg } => (AtomicOp::Sub, Some(memarg), 4, false, 2, true),
        Operator::I64AtomicRmwSub { memarg } => (AtomicOp::Sub, Some(memarg), 8, true, 2, true),
        Operator::I32AtomicRmw8SubU { memarg } => (AtomicOp::Sub, Some(memarg), 1, false, 2, true),
        Operator::I32AtomicRmw16SubU { memarg } => (AtomicOp::Sub, Some(memarg), 2, false, 2, true),
        Operator::I64AtomicRmw8SubU { memarg } => (AtomicOp::Sub, Some(memarg), 1, true, 2, true),
        Operator::I64AtomicRmw16SubU { memarg } => (AtomicOp::Sub, Some(memarg), 2, true, 2, true),
        Operator::I64AtomicRmw32SubU { memarg } => (AtomicOp::Sub, Some(memarg), 4, true, 2, true),
        Operator::I32AtomicRmwAnd { memarg } => (AtomicOp::And, Some(memarg), 4, false, 2, true),
        Operator::I64AtomicRmwAnd { memarg } => (AtomicOp::And, Some(memarg), 8, true, 2, true),
        Operator::I32AtomicRmw8AndU { memarg } => (AtomicOp::And, Some(memarg), 1, false, 2, true),
        Operator::I32AtomicRmw16AndU { memarg } => (AtomicOp::And, Some(memarg), 2, false, 2, true),
        Operator::I64AtomicRmw8AndU { memarg } => (AtomicOp::And, Some(memarg), 1, true, 2, true),
        Operator::I64AtomicRmw16AndU { memarg } => (AtomicOp::And, Some(memarg), 2, true, 2, true),
        Operator::I64AtomicRmw32AndU { memarg } => (AtomicOp::And, Some(memarg), 4, true, 2, true),
        Operator::I32AtomicRmwOr { memarg } => (AtomicOp::Or, Some(memarg), 4, false, 2, true),
        Operator::I64AtomicRmwOr { memarg } => (AtomicOp::Or, Some(memarg), 8, true, 2, true),
        Operator::I32AtomicRmw8OrU { memarg } => (AtomicOp::Or, Some(memarg), 1, false, 2, true),
        Operator::I32AtomicRmw16OrU { memarg } => (AtomicOp::Or, Some(memarg), 2, false, 2, true),
        Operator::I64AtomicRmw8OrU { memarg } => (AtomicOp::Or, Some(memarg), 1, true, 2, true),
        Operator::I64AtomicRmw16OrU { memarg } => (AtomicOp::Or, Some(memarg), 2, true, 2, true),
        Operator::I64AtomicRmw32OrU { memarg } => (AtomicOp::Or, Some(memarg), 4, true, 2, true),
        Operator::I32AtomicRmwXor { memarg } => (AtomicOp::Xor, Some(memarg), 4, false, 2, true),
        Operator::I64AtomicRmwXor { memarg } => (AtomicOp::Xor, Some(memarg), 8, true, 2, true),
        Operator::I32AtomicRmw8XorU { memarg } => (AtomicOp::Xor, Some(memarg), 1, false, 2, true),
        Operator::I32AtomicRmw16XorU { memarg } => (AtomicOp::Xor, Some(memarg), 2, false, 2, true),
        Operator::I64AtomicRmw8XorU { memarg } => (AtomicOp::Xor, Some(memarg), 1, true, 2, true),
        Operator::I64AtomicRmw16XorU { memarg } => (AtomicOp::Xor, Some(memarg), 2, true, 2, true),
        Operator::I64AtomicRmw32XorU { memarg } => (AtomicOp::Xor, Some(memarg), 4, true, 2, true),
        Operator::I32AtomicRmwXchg { memarg } => (AtomicOp::Xchg, Some(memarg), 4, false, 2, true),
        Operator::I64AtomicRmwXchg { memarg } => (AtomicOp::Xchg, Some(memarg), 8, true, 2, true),
        Operator::I32AtomicRmw8XchgU { memarg } => {
            (AtomicOp::Xchg, Some(memarg), 1, false, 2, true)
        }
        Operator::I32AtomicRmw16XchgU { memarg } => {
            (AtomicOp::Xchg, Some(memarg), 2, false, 2, true)
        }
        Operator::I64AtomicRmw8XchgU { memarg } => (AtomicOp::Xchg, Some(memarg), 1, true, 2, true),
        Operator::I64AtomicRmw16XchgU { memarg } => {
            (AtomicOp::Xchg, Some(memarg), 2, true, 2, true)
        }
        Operator::I64AtomicRmw32XchgU { memarg } => {
            (AtomicOp::Xchg, Some(memarg), 4, true, 2, true)
        }
        Operator::I32AtomicRmwCmpxchg { memarg } => {
            (AtomicOp::Cmpxchg, Some(memarg), 4, false, 3, true)
        }
        Operator::I64AtomicRmwCmpxchg { memarg } => {
            (AtomicOp::Cmpxchg, Some(memarg), 8, true, 3, true)
        }
        Operator::I32AtomicRmw8CmpxchgU { memarg } => {
            (AtomicOp::Cmpxchg, Some(memarg), 1, false, 3, true)
        }
        Operator::I32AtomicRmw16CmpxchgU { memarg } => {
            (AtomicOp::Cmpxchg, Some(memarg), 2, false, 3, true)
        }
        Operator::I64AtomicRmw8CmpxchgU { memarg } => {
            (AtomicOp::Cmpxchg, Some(memarg), 1, true, 3, true)
        }
        Operator::I64AtomicRmw16CmpxchgU { memarg } => {
            (AtomicOp::Cmpxchg, Some(memarg), 2, true, 3, true)
        }
        Operator::I64AtomicRmw32CmpxchgU { memarg } => {
            (AtomicOp::Cmpxchg, Some(memarg), 4, true, 3, true)
        }
        Operator::MemoryAtomicWait32 { memarg } => {
            (AtomicOp::Wait, Some(memarg), 4, false, 3, true)
        }
        Operator::MemoryAtomicWait64 { memarg } => (AtomicOp::Wait, Some(memarg), 8, true, 3, true),
        Operator::MemoryAtomicNotify { memarg } => {
            (AtomicOp::Notify, Some(memarg), 4, false, 2, true)
        }
        Operator::AtomicFence => (AtomicOp::Fence, None, 1, false, 0, false),
        _ => return None,
    })
}

fn emit_op(
    ctx: &mut Context<'_>,
    op: AtomicOp,
    memarg: Option<MemArg>,
    bytes: u8,
    wide: bool,
    pops: usize,
    has_dst: bool,
) -> Result<(), LowerError> {
    let mut args = [0u16; 3];
    for i in (0..pops).rev() {
        args[i] = ctx.pop()?;
    }
    let dst = if has_dst { ctx.alloc()? } else { 0 };
    let (offset, mem) = memarg.map(|m| (m.offset, m.memory)).unwrap_or((0, 0));
    ctx.emit(Inst::Atomic {
        op,
        dst,
        addr: args[0],
        a: args[1],
        b: args[2],
        offset,
        mem,
        bytes,
        wide,
    });
    if has_dst {
        ctx.push(dst);
    }
    Ok(())
}

//! Numeric, convert, local-free operator maps.

use crate::hir::Inst;
use crate::native::{BinF32, BinF64, BinI32, BinI64, ConvOp, SimdOp, UnF32, UnF64, UnI32, UnI64};
use wasmparser::Operator;

use super::Context;
use crate::wasm::LowerError;

pub(super) fn emit_numeric(ctx: &mut Context<'_>, op: &Operator<'_>) -> Result<bool, LowerError> {
    if emit_const(ctx, op)? {
        return Ok(true);
    }
    if let Some(op) = un_i32(op) {
        return un(ctx, |dst, src| Inst::UnI32 { op, dst, src }).map(|()| true);
    }
    if let Some(op) = bin_i32(op) {
        return bin(ctx, |dst, lhs, rhs| Inst::BinI32 { op, dst, lhs, rhs }).map(|()| true);
    }
    if let Some(op) = un_i64(op) {
        return un(ctx, |dst, src| Inst::UnI64 { op, dst, src }).map(|()| true);
    }
    if let Some(op) = bin_i64(op) {
        return bin(ctx, |dst, lhs, rhs| Inst::BinI64 { op, dst, lhs, rhs }).map(|()| true);
    }
    if let Some(op) = un_f32(op) {
        return un(ctx, |dst, src| Inst::UnF32 { op, dst, src }).map(|()| true);
    }
    if let Some(op) = bin_f32(op) {
        return bin(ctx, |dst, lhs, rhs| Inst::BinF32 { op, dst, lhs, rhs }).map(|()| true);
    }
    if let Some(op) = un_f64(op) {
        return un(ctx, |dst, src| Inst::UnF64 { op, dst, src }).map(|()| true);
    }
    if let Some(op) = bin_f64(op) {
        return bin(ctx, |dst, lhs, rhs| Inst::BinF64 { op, dst, lhs, rhs }).map(|()| true);
    }
    if let Some(op) = conv(op) {
        return un(ctx, |dst, src| Inst::Conv { op, dst, src }).map(|()| true);
    }
    if let Some((op, lane)) = simd_lane(op) {
        return emit_lane(ctx, op, lane).map(|()| true);
    }
    if let Some(op) = simd(op) {
        return emit_simd(ctx, op).map(|()| true);
    }
    Ok(false)
}

fn emit_simd(ctx: &mut Context<'_>, op: SimdOp) -> Result<(), LowerError> {
    match op.arity() {
        1 => un(ctx, |dst, src| Inst::Simd {
            op,
            dst,
            a: src,
            b: 0,
            c: 0,
            lane: 0,
        }),
        3 => {
            let c = ctx.pop()?;
            let b = ctx.pop()?;
            let a = ctx.pop()?;
            let dst = ctx.alloc()?;
            ctx.emit(Inst::Simd {
                op,
                dst,
                a,
                b,
                c,
                lane: 0,
            });
            ctx.push(dst);
            Ok(())
        }
        _ => bin(ctx, |dst, lhs, rhs| Inst::Simd {
            op,
            dst,
            a: lhs,
            b: rhs,
            c: 0,
            lane: 0,
        }),
    }
}

fn emit_lane(ctx: &mut Context<'_>, op: SimdOp, lane: u8) -> Result<(), LowerError> {
    if matches!(
        op,
        SimdOp::I8x16Replace
            | SimdOp::I16x8Replace
            | SimdOp::I32x4Replace
            | SimdOp::I64x2Replace
            | SimdOp::F32x4Replace
            | SimdOp::F64x2Replace
    ) {
        let val = ctx.pop()?;
        let vec = ctx.pop()?;
        let dst = ctx.alloc()?;
        ctx.emit(Inst::Simd {
            op,
            dst,
            a: vec,
            b: val,
            c: 0,
            lane,
        });
        ctx.push(dst);
        return Ok(());
    }
    un(ctx, |dst, src| Inst::Simd {
        op,
        dst,
        a: src,
        b: 0,
        c: 0,
        lane,
    })
}

fn un(ctx: &mut Context<'_>, inst: impl FnOnce(u16, u16) -> Inst) -> Result<(), LowerError> {
    let src = ctx.pop()?;
    let dst = ctx.alloc()?;
    ctx.emit(inst(dst, src));
    ctx.push(dst);
    Ok(())
}

fn bin(ctx: &mut Context<'_>, inst: impl FnOnce(u16, u16, u16) -> Inst) -> Result<(), LowerError> {
    let rhs = ctx.pop()?;
    let lhs = ctx.pop()?;
    let dst = ctx.alloc()?;
    ctx.emit(inst(dst, lhs, rhs));
    ctx.push(dst);
    Ok(())
}

fn emit_const(ctx: &mut Context<'_>, op: &Operator<'_>) -> Result<bool, LowerError> {
    let inst = match *op {
        Operator::I32Const { value } => Inst::ConstI32 { dst: 0, val: value },
        Operator::I64Const { value } => Inst::ConstI64 { dst: 0, val: value },
        Operator::F32Const { value } => Inst::ConstF32 {
            dst: 0,
            bits: value.bits(),
        },
        Operator::F64Const { value } => Inst::ConstF64 {
            dst: 0,
            bits: value.bits(),
        },
        Operator::V128Const { value } => Inst::ConstV128 {
            dst: 0,
            bits: u128::from(value),
        },
        _ => return Ok(false),
    };
    let dst = ctx.alloc()?;
    ctx.emit(with_dst(inst, dst));
    ctx.push(dst);
    Ok(true)
}

fn with_dst(inst: Inst, dst: u16) -> Inst {
    match inst {
        Inst::ConstI32 { val, .. } => Inst::ConstI32 { dst, val },
        Inst::ConstI64 { val, .. } => Inst::ConstI64 { dst, val },
        Inst::ConstF32 { bits, .. } => Inst::ConstF32 { dst, bits },
        Inst::ConstF64 { bits, .. } => Inst::ConstF64 { dst, bits },
        Inst::ConstV128 { bits, .. } => Inst::ConstV128 { dst, bits },
        other => other,
    }
}

fn un_i32(op: &Operator<'_>) -> Option<UnI32> {
    Some(match op {
        Operator::I32Clz => UnI32::Clz,
        Operator::I32Ctz => UnI32::Ctz,
        Operator::I32Popcnt => UnI32::Popcnt,
        Operator::I32Eqz => UnI32::Eqz,
        Operator::I32Extend8S => UnI32::Extend8S,
        Operator::I32Extend16S => UnI32::Extend16S,
        _ => return None,
    })
}

fn bin_i32(op: &Operator<'_>) -> Option<BinI32> {
    // The three basic arithmetic meanings are shared with the JavaScript
    // operation catalog; only the physical Wasm opcode remains frontend-
    // specific.
    if let Some(shared) = match op {
        Operator::I32Add => Some(crate::facts::SharedBinaryFact::Add),
        Operator::I32Sub => Some(crate::facts::SharedBinaryFact::Subtract),
        Operator::I32Mul => Some(crate::facts::SharedBinaryFact::Multiply),
        _ => None,
    } {
        return Some(shared.to_wasm_i32());
    }
    Some(match op {
        Operator::I32DivS => BinI32::DivS,
        Operator::I32DivU => BinI32::DivU,
        Operator::I32RemS => BinI32::RemS,
        Operator::I32RemU => BinI32::RemU,
        Operator::I32And => BinI32::And,
        Operator::I32Or => BinI32::Or,
        Operator::I32Xor => BinI32::Xor,
        Operator::I32Shl => BinI32::Shl,
        Operator::I32ShrS => BinI32::ShrS,
        Operator::I32ShrU => BinI32::ShrU,
        Operator::I32Rotl => BinI32::Rotl,
        Operator::I32Rotr => BinI32::Rotr,
        Operator::I32Eq => BinI32::Eq,
        Operator::I32Ne => BinI32::Ne,
        Operator::I32LtS => BinI32::LtS,
        Operator::I32LtU => BinI32::LtU,
        Operator::I32LeS => BinI32::LeS,
        Operator::I32LeU => BinI32::LeU,
        Operator::I32GtS => BinI32::GtS,
        Operator::I32GtU => BinI32::GtU,
        Operator::I32GeS => BinI32::GeS,
        Operator::I32GeU => BinI32::GeU,
        _ => return None,
    })
}

fn un_i64(op: &Operator<'_>) -> Option<UnI64> {
    Some(match op {
        Operator::I64Clz => UnI64::Clz,
        Operator::I64Ctz => UnI64::Ctz,
        Operator::I64Popcnt => UnI64::Popcnt,
        Operator::I64Eqz => UnI64::Eqz,
        Operator::I64Extend8S => UnI64::Extend8S,
        Operator::I64Extend16S => UnI64::Extend16S,
        Operator::I64Extend32S => UnI64::Extend32S,
        _ => return None,
    })
}

fn bin_i64(op: &Operator<'_>) -> Option<BinI64> {
    Some(match op {
        Operator::I64Add => BinI64::Add,
        Operator::I64Sub => BinI64::Sub,
        Operator::I64Mul => BinI64::Mul,
        Operator::I64DivS => BinI64::DivS,
        Operator::I64DivU => BinI64::DivU,
        Operator::I64RemS => BinI64::RemS,
        Operator::I64RemU => BinI64::RemU,
        Operator::I64And => BinI64::And,
        Operator::I64Or => BinI64::Or,
        Operator::I64Xor => BinI64::Xor,
        Operator::I64Shl => BinI64::Shl,
        Operator::I64ShrS => BinI64::ShrS,
        Operator::I64ShrU => BinI64::ShrU,
        Operator::I64Rotl => BinI64::Rotl,
        Operator::I64Rotr => BinI64::Rotr,
        Operator::I64Eq => BinI64::Eq,
        Operator::I64Ne => BinI64::Ne,
        Operator::I64LtS => BinI64::LtS,
        Operator::I64LtU => BinI64::LtU,
        Operator::I64LeS => BinI64::LeS,
        Operator::I64LeU => BinI64::LeU,
        Operator::I64GtS => BinI64::GtS,
        Operator::I64GtU => BinI64::GtU,
        Operator::I64GeS => BinI64::GeS,
        Operator::I64GeU => BinI64::GeU,
        _ => return None,
    })
}

fn un_f32(op: &Operator<'_>) -> Option<UnF32> {
    Some(match op {
        Operator::F32Abs => UnF32::Abs,
        Operator::F32Neg => UnF32::Neg,
        Operator::F32Ceil => UnF32::Ceil,
        Operator::F32Floor => UnF32::Floor,
        Operator::F32Trunc => UnF32::Trunc,
        Operator::F32Nearest => UnF32::Nearest,
        Operator::F32Sqrt => UnF32::Sqrt,
        _ => return None,
    })
}

fn bin_f32(op: &Operator<'_>) -> Option<BinF32> {
    Some(match op {
        Operator::F32Add => BinF32::Add,
        Operator::F32Sub => BinF32::Sub,
        Operator::F32Mul => BinF32::Mul,
        Operator::F32Div => BinF32::Div,
        Operator::F32Min => BinF32::Min,
        Operator::F32Max => BinF32::Max,
        Operator::F32Copysign => BinF32::Copysign,
        Operator::F32Eq => BinF32::Eq,
        Operator::F32Ne => BinF32::Ne,
        Operator::F32Lt => BinF32::Lt,
        Operator::F32Gt => BinF32::Gt,
        Operator::F32Le => BinF32::Le,
        Operator::F32Ge => BinF32::Ge,
        _ => return None,
    })
}

fn un_f64(op: &Operator<'_>) -> Option<UnF64> {
    Some(match op {
        Operator::F64Abs => UnF64::Abs,
        Operator::F64Neg => UnF64::Neg,
        Operator::F64Ceil => UnF64::Ceil,
        Operator::F64Floor => UnF64::Floor,
        Operator::F64Trunc => UnF64::Trunc,
        Operator::F64Nearest => UnF64::Nearest,
        Operator::F64Sqrt => UnF64::Sqrt,
        _ => return None,
    })
}

fn bin_f64(op: &Operator<'_>) -> Option<BinF64> {
    Some(match op {
        Operator::F64Add => BinF64::Add,
        Operator::F64Sub => BinF64::Sub,
        Operator::F64Mul => BinF64::Mul,
        Operator::F64Div => BinF64::Div,
        Operator::F64Min => BinF64::Min,
        Operator::F64Max => BinF64::Max,
        Operator::F64Copysign => BinF64::Copysign,
        Operator::F64Eq => BinF64::Eq,
        Operator::F64Ne => BinF64::Ne,
        Operator::F64Lt => BinF64::Lt,
        Operator::F64Gt => BinF64::Gt,
        Operator::F64Le => BinF64::Le,
        Operator::F64Ge => BinF64::Ge,
        _ => return None,
    })
}

fn conv(op: &Operator<'_>) -> Option<ConvOp> {
    Some(match op {
        Operator::I32WrapI64 => ConvOp::I32WrapI64,
        Operator::I64ExtendI32S => ConvOp::I64ExtendI32S,
        Operator::I64ExtendI32U => ConvOp::I64ExtendI32U,
        Operator::I32TruncF32S => ConvOp::I32TruncF32S,
        Operator::I32TruncF32U => ConvOp::I32TruncF32U,
        Operator::I32TruncF64S => ConvOp::I32TruncF64S,
        Operator::I32TruncF64U => ConvOp::I32TruncF64U,
        Operator::I64TruncF32S => ConvOp::I64TruncF32S,
        Operator::I64TruncF32U => ConvOp::I64TruncF32U,
        Operator::I64TruncF64S => ConvOp::I64TruncF64S,
        Operator::I64TruncF64U => ConvOp::I64TruncF64U,
        Operator::I32TruncSatF32S => ConvOp::I32TruncSatF32S,
        Operator::I32TruncSatF32U => ConvOp::I32TruncSatF32U,
        Operator::I32TruncSatF64S => ConvOp::I32TruncSatF64S,
        Operator::I32TruncSatF64U => ConvOp::I32TruncSatF64U,
        Operator::I64TruncSatF32S => ConvOp::I64TruncSatF32S,
        Operator::I64TruncSatF32U => ConvOp::I64TruncSatF32U,
        Operator::I64TruncSatF64S => ConvOp::I64TruncSatF64S,
        Operator::I64TruncSatF64U => ConvOp::I64TruncSatF64U,
        Operator::F32ConvertI32S => ConvOp::F32ConvertI32S,
        Operator::F32ConvertI32U => ConvOp::F32ConvertI32U,
        Operator::F32ConvertI64S => ConvOp::F32ConvertI64S,
        Operator::F32ConvertI64U => ConvOp::F32ConvertI64U,
        Operator::F32DemoteF64 => ConvOp::F32DemoteF64,
        Operator::F64ConvertI32S => ConvOp::F64ConvertI32S,
        Operator::F64ConvertI32U => ConvOp::F64ConvertI32U,
        Operator::F64ConvertI64S => ConvOp::F64ConvertI64S,
        Operator::F64ConvertI64U => ConvOp::F64ConvertI64U,
        Operator::F64PromoteF32 => ConvOp::F64PromoteF32,
        Operator::I32ReinterpretF32 => ConvOp::I32ReinterpretF32,
        Operator::I64ReinterpretF64 => ConvOp::I64ReinterpretF64,
        Operator::F32ReinterpretI32 => ConvOp::F32ReinterpretI32,
        Operator::F64ReinterpretI64 => ConvOp::F64ReinterpretI64,
        _ => return None,
    })
}

fn simd(op: &Operator<'_>) -> Option<SimdOp> {
    Some(match op {
        Operator::V128And => SimdOp::And,
        Operator::V128Or => SimdOp::Or,
        Operator::V128Xor => SimdOp::Xor,
        Operator::V128Not => SimdOp::Not,
        Operator::V128AndNot => SimdOp::AndNot,
        Operator::I8x16Splat => SimdOp::I8x16Splat,
        Operator::I16x8Splat => SimdOp::I16x8Splat,
        Operator::I32x4Splat => SimdOp::I32x4Splat,
        Operator::I64x2Splat => SimdOp::I64x2Splat,
        Operator::F32x4Splat => SimdOp::F32x4Splat,
        Operator::F64x2Splat => SimdOp::F64x2Splat,
        Operator::I8x16Add => SimdOp::I8x16Add,
        Operator::I8x16Sub => SimdOp::I8x16Sub,
        Operator::I8x16Neg => SimdOp::I8x16Neg,
        Operator::I8x16Eq => SimdOp::I8x16Eq,
        Operator::I8x16Ne => SimdOp::I8x16Ne,
        Operator::I16x8Add => SimdOp::I16x8Add,
        Operator::I16x8Sub => SimdOp::I16x8Sub,
        Operator::I16x8Neg => SimdOp::I16x8Neg,
        Operator::I16x8Eq => SimdOp::I16x8Eq,
        Operator::I16x8Ne => SimdOp::I16x8Ne,
        Operator::I32x4Add => SimdOp::I32x4Add,
        Operator::I32x4Sub => SimdOp::I32x4Sub,
        Operator::I32x4Mul => SimdOp::I32x4Mul,
        Operator::I32x4Neg => SimdOp::I32x4Neg,
        Operator::I32x4Eq => SimdOp::I32x4Eq,
        Operator::I32x4Ne => SimdOp::I32x4Ne,
        Operator::I64x2Add => SimdOp::I64x2Add,
        Operator::I64x2Sub => SimdOp::I64x2Sub,
        Operator::I64x2Mul => SimdOp::I64x2Mul,
        Operator::I64x2Neg => SimdOp::I64x2Neg,
        Operator::I64x2Eq => SimdOp::I64x2Eq,
        Operator::I64x2Ne => SimdOp::I64x2Ne,
        Operator::F32x4Add => SimdOp::F32x4Add,
        Operator::F32x4Sub => SimdOp::F32x4Sub,
        Operator::F32x4Mul => SimdOp::F32x4Mul,
        Operator::F32x4Div => SimdOp::F32x4Div,
        Operator::F64x2Add => SimdOp::F64x2Add,
        Operator::F64x2Sub => SimdOp::F64x2Sub,
        Operator::F64x2Mul => SimdOp::F64x2Mul,
        Operator::F64x2Div => SimdOp::F64x2Div,
        Operator::I8x16Abs => SimdOp::I8x16Abs,
        Operator::I8x16MinS => SimdOp::I8x16MinS,
        Operator::I8x16MinU => SimdOp::I8x16MinU,
        Operator::I8x16MaxS => SimdOp::I8x16MaxS,
        Operator::I8x16MaxU => SimdOp::I8x16MaxU,
        Operator::I32x4Abs => SimdOp::I32x4Abs,
        Operator::I32x4MinS => SimdOp::I32x4MinS,
        Operator::I32x4MinU => SimdOp::I32x4MinU,
        Operator::I32x4MaxS => SimdOp::I32x4MaxS,
        Operator::I32x4MaxU => SimdOp::I32x4MaxU,
        Operator::F32x4Abs => SimdOp::F32x4Abs,
        Operator::F32x4Neg => SimdOp::F32x4Neg,
        Operator::F32x4Sqrt => SimdOp::F32x4Sqrt,
        Operator::F32x4Min => SimdOp::F32x4Min,
        Operator::F32x4Max => SimdOp::F32x4Max,
        Operator::F32x4Eq => SimdOp::F32x4Eq,
        Operator::F32x4Ne => SimdOp::F32x4Ne,
        Operator::F32x4Lt => SimdOp::F32x4Lt,
        Operator::F32x4Gt => SimdOp::F32x4Gt,
        Operator::F32x4Le => SimdOp::F32x4Le,
        Operator::F32x4Ge => SimdOp::F32x4Ge,
        Operator::F64x2Abs => SimdOp::F64x2Abs,
        Operator::F64x2Neg => SimdOp::F64x2Neg,
        Operator::F64x2Sqrt => SimdOp::F64x2Sqrt,
        Operator::F64x2Min => SimdOp::F64x2Min,
        Operator::F64x2Max => SimdOp::F64x2Max,
        Operator::F64x2Eq => SimdOp::F64x2Eq,
        Operator::F64x2Ne => SimdOp::F64x2Ne,
        Operator::F64x2Lt => SimdOp::F64x2Lt,
        Operator::F64x2Gt => SimdOp::F64x2Gt,
        Operator::F64x2Le => SimdOp::F64x2Le,
        Operator::F64x2Ge => SimdOp::F64x2Ge,
        Operator::V128Bitselect => SimdOp::Bitselect,
        Operator::I8x16Swizzle => SimdOp::Swizzle,
        Operator::I16x8Mul => SimdOp::I16x8Mul,
        Operator::I16x8Abs => SimdOp::I16x8Abs,
        Operator::I16x8MinS => SimdOp::I16x8MinS,
        Operator::I16x8MinU => SimdOp::I16x8MinU,
        Operator::I16x8MaxS => SimdOp::I16x8MaxS,
        Operator::I16x8MaxU => SimdOp::I16x8MaxU,
        Operator::I8x16AddSatS => SimdOp::I8x16AddSatS,
        Operator::I8x16AddSatU => SimdOp::I8x16AddSatU,
        Operator::I8x16SubSatS => SimdOp::I8x16SubSatS,
        Operator::I8x16SubSatU => SimdOp::I8x16SubSatU,
        Operator::I16x8AddSatS => SimdOp::I16x8AddSatS,
        Operator::I16x8AddSatU => SimdOp::I16x8AddSatU,
        Operator::I16x8SubSatS => SimdOp::I16x8SubSatS,
        Operator::I16x8SubSatU => SimdOp::I16x8SubSatU,
        Operator::I8x16Shl => SimdOp::I8x16Shl,
        Operator::I8x16ShrS => SimdOp::I8x16ShrS,
        Operator::I8x16ShrU => SimdOp::I8x16ShrU,
        Operator::I16x8Shl => SimdOp::I16x8Shl,
        Operator::I16x8ShrS => SimdOp::I16x8ShrS,
        Operator::I16x8ShrU => SimdOp::I16x8ShrU,
        Operator::I32x4Shl => SimdOp::I32x4Shl,
        Operator::I32x4ShrS => SimdOp::I32x4ShrS,
        Operator::I32x4ShrU => SimdOp::I32x4ShrU,
        Operator::I64x2Shl => SimdOp::I64x2Shl,
        Operator::I64x2ShrS => SimdOp::I64x2ShrS,
        Operator::I64x2ShrU => SimdOp::I64x2ShrU,
        Operator::V128AnyTrue => SimdOp::I8x16AnyTrue,
        Operator::I8x16AllTrue => SimdOp::I8x16AllTrue,
        Operator::I16x8AllTrue => SimdOp::I16x8AllTrue,
        Operator::I32x4AllTrue => SimdOp::I32x4AllTrue,
        Operator::I64x2AllTrue => SimdOp::I64x2AllTrue,
        Operator::I8x16Bitmask => SimdOp::I8x16Bitmask,
        Operator::I16x8Bitmask => SimdOp::I16x8Bitmask,
        Operator::I32x4Bitmask => SimdOp::I32x4Bitmask,
        Operator::I64x2Bitmask => SimdOp::I64x2Bitmask,
        Operator::F32x4ConvertI32x4S => SimdOp::F32x4ConvertI32S,
        Operator::F32x4ConvertI32x4U => SimdOp::F32x4ConvertI32U,
        Operator::I32x4TruncSatF32x4S => SimdOp::I32x4TruncSatF32S,
        Operator::I32x4TruncSatF32x4U => SimdOp::I32x4TruncSatF32U,
        Operator::I8x16LtS => SimdOp::I8x16LtS,
        Operator::I8x16LtU => SimdOp::I8x16LtU,
        Operator::I8x16GtS => SimdOp::I8x16GtS,
        Operator::I8x16GtU => SimdOp::I8x16GtU,
        Operator::I8x16LeS => SimdOp::I8x16LeS,
        Operator::I8x16LeU => SimdOp::I8x16LeU,
        Operator::I8x16GeS => SimdOp::I8x16GeS,
        Operator::I8x16GeU => SimdOp::I8x16GeU,
        Operator::I16x8LtS => SimdOp::I16x8LtS,
        Operator::I16x8LtU => SimdOp::I16x8LtU,
        Operator::I16x8GtS => SimdOp::I16x8GtS,
        Operator::I16x8GtU => SimdOp::I16x8GtU,
        Operator::I16x8LeS => SimdOp::I16x8LeS,
        Operator::I16x8LeU => SimdOp::I16x8LeU,
        Operator::I16x8GeS => SimdOp::I16x8GeS,
        Operator::I16x8GeU => SimdOp::I16x8GeU,
        Operator::I32x4LtS => SimdOp::I32x4LtS,
        Operator::I32x4LtU => SimdOp::I32x4LtU,
        Operator::I32x4GtS => SimdOp::I32x4GtS,
        Operator::I32x4GtU => SimdOp::I32x4GtU,
        Operator::I32x4LeS => SimdOp::I32x4LeS,
        Operator::I32x4LeU => SimdOp::I32x4LeU,
        Operator::I32x4GeS => SimdOp::I32x4GeS,
        Operator::I32x4GeU => SimdOp::I32x4GeU,
        Operator::I64x2LtS => SimdOp::I64x2Lt,
        Operator::I64x2GtS => SimdOp::I64x2Gt,
        Operator::I64x2LeS => SimdOp::I64x2Le,
        Operator::I64x2GeS => SimdOp::I64x2Ge,
        Operator::I8x16Popcnt => SimdOp::I8x16Popcnt,
        Operator::I8x16NarrowI16x8S => SimdOp::I8x16NarrowS,
        Operator::I8x16NarrowI16x8U => SimdOp::I8x16NarrowU,
        Operator::I8x16AvgrU => SimdOp::I8x16AvgrU,
        Operator::I16x8AvgrU => SimdOp::I16x8AvgrU,
        Operator::I16x8Q15MulrSatS => SimdOp::I16x8Q15Mulr,
        Operator::I16x8NarrowI32x4S => SimdOp::I16x8NarrowS,
        Operator::I16x8NarrowI32x4U => SimdOp::I16x8NarrowU,
        Operator::I16x8ExtAddPairwiseI8x16S => SimdOp::I16x8ExtAddPS,
        Operator::I16x8ExtAddPairwiseI8x16U => SimdOp::I16x8ExtAddPU,
        Operator::I16x8ExtendLowI8x16S => SimdOp::I16x8ExtLowS,
        Operator::I16x8ExtendHighI8x16S => SimdOp::I16x8ExtHighS,
        Operator::I16x8ExtendLowI8x16U => SimdOp::I16x8ExtLowU,
        Operator::I16x8ExtendHighI8x16U => SimdOp::I16x8ExtHighU,
        Operator::I16x8ExtMulLowI8x16S => SimdOp::I16x8ExtMulLowS,
        Operator::I16x8ExtMulHighI8x16S => SimdOp::I16x8ExtMulHighS,
        Operator::I16x8ExtMulLowI8x16U => SimdOp::I16x8ExtMulLowU,
        Operator::I16x8ExtMulHighI8x16U => SimdOp::I16x8ExtMulHighU,
        Operator::I32x4ExtAddPairwiseI16x8S => SimdOp::I32x4ExtAddPS,
        Operator::I32x4ExtAddPairwiseI16x8U => SimdOp::I32x4ExtAddPU,
        Operator::I32x4ExtendLowI16x8S => SimdOp::I32x4ExtLowS,
        Operator::I32x4ExtendHighI16x8S => SimdOp::I32x4ExtHighS,
        Operator::I32x4ExtendLowI16x8U => SimdOp::I32x4ExtLowU,
        Operator::I32x4ExtendHighI16x8U => SimdOp::I32x4ExtHighU,
        Operator::I32x4DotI16x8S => SimdOp::I32x4Dot,
        Operator::I32x4ExtMulLowI16x8S => SimdOp::I32x4ExtMulLowS,
        Operator::I32x4ExtMulHighI16x8S => SimdOp::I32x4ExtMulHighS,
        Operator::I32x4ExtMulLowI16x8U => SimdOp::I32x4ExtMulLowU,
        Operator::I32x4ExtMulHighI16x8U => SimdOp::I32x4ExtMulHighU,
        Operator::I64x2Abs => SimdOp::I64x2Abs,
        Operator::I64x2ExtendLowI32x4S => SimdOp::I64x2ExtLowS,
        Operator::I64x2ExtendHighI32x4S => SimdOp::I64x2ExtHighS,
        Operator::I64x2ExtendLowI32x4U => SimdOp::I64x2ExtLowU,
        Operator::I64x2ExtendHighI32x4U => SimdOp::I64x2ExtHighU,
        Operator::I64x2ExtMulLowI32x4S => SimdOp::I64x2ExtMulLowS,
        Operator::I64x2ExtMulHighI32x4S => SimdOp::I64x2ExtMulHighS,
        Operator::I64x2ExtMulLowI32x4U => SimdOp::I64x2ExtMulLowU,
        Operator::I64x2ExtMulHighI32x4U => SimdOp::I64x2ExtMulHighU,
        Operator::F32x4Ceil => SimdOp::F32x4Ceil,
        Operator::F32x4Floor => SimdOp::F32x4Floor,
        Operator::F32x4Trunc => SimdOp::F32x4Trunc,
        Operator::F32x4Nearest => SimdOp::F32x4Nearest,
        Operator::F32x4PMin => SimdOp::F32x4PMin,
        Operator::F32x4PMax => SimdOp::F32x4PMax,
        Operator::F64x2Ceil => SimdOp::F64x2Ceil,
        Operator::F64x2Floor => SimdOp::F64x2Floor,
        Operator::F64x2Trunc => SimdOp::F64x2Trunc,
        Operator::F64x2Nearest => SimdOp::F64x2Nearest,
        Operator::F64x2PMin => SimdOp::F64x2PMin,
        Operator::F64x2PMax => SimdOp::F64x2PMax,
        Operator::I32x4TruncSatF64x2SZero => SimdOp::I32x4TruncSatF64S,
        Operator::I32x4TruncSatF64x2UZero => SimdOp::I32x4TruncSatF64U,
        Operator::F64x2ConvertLowI32x4S => SimdOp::F64x2ConvertLowS,
        Operator::F64x2ConvertLowI32x4U => SimdOp::F64x2ConvertLowU,
        Operator::F32x4DemoteF64x2Zero => SimdOp::F32x4DemoteZero,
        Operator::F64x2PromoteLowF32x4 => SimdOp::F64x2PromoteLow,
        Operator::I8x16RelaxedSwizzle => SimdOp::Swizzle,
        Operator::I32x4RelaxedTruncF32x4S => SimdOp::I32x4TruncSatF32S,
        Operator::I32x4RelaxedTruncF32x4U => SimdOp::I32x4TruncSatF32U,
        Operator::I32x4RelaxedTruncF64x2SZero => SimdOp::I32x4TruncSatF64S,
        Operator::I32x4RelaxedTruncF64x2UZero => SimdOp::I32x4TruncSatF64U,
        Operator::F32x4RelaxedMin => SimdOp::F32x4Min,
        Operator::F32x4RelaxedMax => SimdOp::F32x4Max,
        Operator::F64x2RelaxedMin => SimdOp::F64x2Min,
        Operator::F64x2RelaxedMax => SimdOp::F64x2Max,
        Operator::I16x8RelaxedQ15mulrS => SimdOp::I16x8Q15Mulr,
        Operator::I8x16RelaxedLaneselect => SimdOp::RelaxedLane8,
        Operator::I16x8RelaxedLaneselect => SimdOp::RelaxedLane16,
        Operator::I32x4RelaxedLaneselect => SimdOp::RelaxedLane32,
        Operator::I64x2RelaxedLaneselect => SimdOp::RelaxedLane64,
        Operator::F32x4RelaxedMadd => SimdOp::RelaxedMaddF32,
        Operator::F32x4RelaxedNmadd => SimdOp::RelaxedNmaddF32,
        Operator::F64x2RelaxedMadd => SimdOp::RelaxedMaddF64,
        Operator::F64x2RelaxedNmadd => SimdOp::RelaxedNmaddF64,
        Operator::I16x8RelaxedDotI8x16I7x16S => SimdOp::I16x8RelaxedDot,
        Operator::I32x4RelaxedDotI8x16I7x16AddS => SimdOp::I32x4RelaxedDotAdd,
        _ => return None,
    })
}

fn simd_lane(op: &Operator<'_>) -> Option<(SimdOp, u8)> {
    Some(match op {
        Operator::I8x16ExtractLaneS { lane } => (SimdOp::I8x16ExtractS, *lane),
        Operator::I8x16ExtractLaneU { lane } => (SimdOp::I8x16ExtractU, *lane),
        Operator::I16x8ExtractLaneS { lane } => (SimdOp::I16x8ExtractS, *lane),
        Operator::I16x8ExtractLaneU { lane } => (SimdOp::I16x8ExtractU, *lane),
        Operator::I32x4ExtractLane { lane } => (SimdOp::I32x4Extract, *lane),
        Operator::I64x2ExtractLane { lane } => (SimdOp::I64x2Extract, *lane),
        Operator::F32x4ExtractLane { lane } => (SimdOp::F32x4Extract, *lane),
        Operator::F64x2ExtractLane { lane } => (SimdOp::F64x2Extract, *lane),
        Operator::I8x16ReplaceLane { lane } => (SimdOp::I8x16Replace, *lane),
        Operator::I16x8ReplaceLane { lane } => (SimdOp::I16x8Replace, *lane),
        Operator::I32x4ReplaceLane { lane } => (SimdOp::I32x4Replace, *lane),
        Operator::I64x2ReplaceLane { lane } => (SimdOp::I64x2Replace, *lane),
        Operator::F32x4ReplaceLane { lane } => (SimdOp::F32x4Replace, *lane),
        Operator::F64x2ReplaceLane { lane } => (SimdOp::F64x2Replace, *lane),
        _ => return None,
    })
}

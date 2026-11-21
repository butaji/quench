//! The vector family of the specializer's machine: `MachineVector` as Cranelift vector
//! instructions; a vector loads and stores as `Lane::V128` (`MachineAccess`). A vector shift reduces its count modulo the lane width, as Wasm does.

use super::super::{Emitter, Host, Kind, VECTOR_IR};
use crate::WasmType;
use crate::staged::machine::MachineVector;
use crate::staged::wasm_simd::{Lanes, Native, NativeOp};
use crate::wasm::V128_BYTES;
use cranelift_codegen::ir::{self, InstBuilder, MemFlagsData, types};

/// The machine type a vector is viewed as for a lane-wise operation: the lanes' integer type
/// repeated to fill the vector.
fn type_in_lane(lanes: Lanes) -> ir::Type {
    let width = lanes.width();
    match lanes {
        Lanes::Bits => VECTOR_IR,
        _ => ir::Type::int(width as u16)
            .and_then(|lane| lane.by(u128::BITS / width))
            .expect("a vector of lanes"),
    }
}

impl Emitter<'_, '_> {
    fn shift_in_lane(&mut self, op: NativeOp, left: ir::Value, count: ir::Value) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match op {
            NativeOp::ShiftLeft => ins.ishl(left, count),
            NativeOp::ShiftRightUnsigned => ins.ushr(left, count),
            _ => ins.sshr(left, count),
        }
    }

    fn binary_in_lane(&mut self, op: NativeOp, left: ir::Value, right: ir::Value) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match op {
            NativeOp::And => ins.band(left, right),
            NativeOp::Or => ins.bor(left, right),
            NativeOp::Xor => ins.bxor(left, right),
            NativeOp::AndNot => ins.band_not(left, right),
            NativeOp::Add => ins.iadd(left, right),
            NativeOp::Subtract => ins.isub(left, right),
            _ => {
                let builder = &mut self.body.core.builder;
                let (left, right) = (builder.ins().uwiden_low(left), builder.ins().uwiden_low(right));
                builder.ins().imul(left, right)
            }
        }
    }
}

impl MachineVector for Emitter<'_, '_> {
    fn operation_in_lane(&mut self, native: Native, left: ir::Value, right: Option<ir::Value>) -> ir::Value {
        let left = self.vector_view(native.lanes, left);
        match (native.op, right) {
            (NativeOp::Not, _) => self.body.core.builder.ins().bnot(left),
            (
                NativeOp::ShiftLeft | NativeOp::ShiftRightUnsigned | NativeOp::ShiftRightSigned,
                Some(count),
            ) => self.shift_in_lane(native.op, left, count),
            (op, Some(right)) => {
                let right = self.vector_view(native.lanes, right);
                self.binary_in_lane(op, left, right)
            }
            (op, None) => unreachable!("{op:?} takes a second operand"),
        }
    }

    fn vector_view(&mut self, shape: Lanes, value: ir::Value) -> ir::Value {
        let ty = type_in_lane(shape);
        if self.body.core.builder.func.dfg.value_type(value) == ty {
            return value;
        }
        let flags = MemFlagsData::new().with_endianness(ir::Endianness::Little);
        self.body.core.builder.ins().bitcast(ty, flags, value)
    }

    fn vector_immediate(&mut self, bits: u128) -> ir::Value {
        let constant = self
            .body
            .core
            .builder
            .func
            .dfg
            .constants
            .insert(ir::ConstantData::from(bits.to_le_bytes().to_vec()));
        self.body.core.builder.ins().vconst(VECTOR_IR, constant)
    }

    fn vector_shuffle_lanes(&mut self, lanes: [u8; V128_BYTES], left: ir::Value, right: ir::Value) -> ir::Value {
        let mask = self
            .body
            .core
            .builder
            .func
            .dfg
            .immediates
            .push(ir::ConstantData::from(lanes.to_vec()));
        self.body.core.builder.ins().shuffle(left, right, mask)
    }

    fn spill_operand(&mut self, slot: ir::StackSlot, at: usize, value: ir::Value) {
        let word = types::I64.bytes();
        let offset = (at as u32 * word) as i32;
        let ty = self.body.core.builder.func.dfg.value_type(value);
        let (low, high) = if ty.is_vector() {
            let value = self.vector_view(Lanes::I64x2, value);
            let builder = &mut self.body.core.builder;
            (builder.ins().extractlane(value, 0), builder.ins().extractlane(value, 1))
        } else {
            let builder = &mut self.body.core.builder;
            let low = if ty == types::I32 { builder.ins().uextend(types::I64, value) } else { value };
            (low, builder.ins().iconst(types::I64, 0))
        };
        let builder = &mut self.body.core.builder;
        builder.ins().stack_store(types::I64, low, slot, offset);
        builder.ins().stack_store(types::I64, high, slot, offset + word as i32);
    }

    fn reload_result(&mut self, slot: ir::StackSlot, at: usize, ty: WasmType) -> ir::Value {
        let result = (at as u32 * types::I64.bytes()) as i32;
        let builder = &mut self.body.core.builder;
        if ty == WasmType::V128 {
            return builder.ins().stack_load(types::I64, VECTOR_IR, slot, result);
        }
        let value = builder.ins().stack_load(types::I64, types::I64, slot, result);
        match Kind::of(ty).expect("planned") {
            Kind::I32 => builder.ins().ireduce(types::I32, value),
            Kind::I64 | Kind::Reference => value,
        }
    }
}

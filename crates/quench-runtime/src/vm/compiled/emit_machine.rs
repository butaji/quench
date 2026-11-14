//! `Emit`'s machine primitives (`staged::MachineStage`) as Cranelift instructions: the one
//! place a native tier builds instructions by hand. Everything that has a meaning (a layout, a
//! language rule) is a `staged` operation over these.

use super::*;
use crate::staged::machine::{Binary, Extend, Lane, MachineStage, Relation, Unary};
use super::object_slots::Emit;

/// The Cranelift type of a lane.
fn machine_type(lane: Lane) -> ir::Type {
    match lane {
        Lane::I8 => types::I8,
        Lane::I16 => types::I16,
        Lane::I32 => types::I32,
        Lane::I64 => types::I64,
        Lane::F32 => types::F32,
        Lane::F64 => types::F64,
    }
}

fn block_args(values: &[ir::Value]) -> Vec<ir::BlockArg> {
    values.iter().map(|value| ir::BlockArg::Value(*value)).collect()
}

fn condition(relation: Relation) -> IntCC {
    match relation {
        Relation::Equal => IntCC::Equal,
        Relation::NotEqual => IntCC::NotEqual,
        Relation::UnsignedLess => IntCC::UnsignedLessThan,
        Relation::UnsignedGreater => IntCC::UnsignedGreaterThan,
        Relation::SignedLess => IntCC::SignedLessThan,
        Relation::FloatUnordered => unreachable!("a float relation has no integer condition"),
    }
}

impl Emit<'_, '_> {
    fn raw_unary_float(&mut self, operation: Unary, a: ir::Value) -> ir::Value {
        match operation {
            Unary::Promote => self.builder.ins().fpromote(types::F64, a),
            Unary::Demote => self.builder.ins().fdemote(types::F32, a),
            Unary::Nearest => self.builder.ins().nearest(a),
            Unary::FloatNegate => self.builder.ins().fneg(a),
            Unary::Bitcast(lane) => self.builder.ins().bitcast(machine_type(lane), MemFlagsData::new(), a),
            _ => unreachable!("an integer conversion"),
        }
    }
}

impl MachineStage for Emit<'_, '_> {
    type Value = ir::Value;
    type Flags = MemFlagsData;
    type Block = ir::Block;
    type Signature = ir::SigRef;

    fn raw_lane(&self, value: ir::Value) -> Lane {
        match self.builder.func.dfg.value_type(value) {
            types::I8 => Lane::I8,
            types::I16 => Lane::I16,
            types::I32 => Lane::I32,
            types::F32 => Lane::F32,
            types::F64 => Lane::F64,
            _ => Lane::I64,
        }
    }
    fn raw_load(&mut self, lane: Lane, flags: MemFlagsData, base: ir::Value, offset: i32) -> ir::Value {
        self.builder.ins().load(machine_type(lane), flags, base, offset)
    }
    fn raw_load_extended(&mut self, from: Lane, extend: Extend, flags: MemFlagsData, base: ir::Value, offset: i32) -> ir::Value {
        match (from, extend) {
            (Lane::I8, Extend::Zero) => self.builder.ins().uload8(types::I64, flags, base, offset),
            (Lane::I16, Extend::Zero) => self.builder.ins().uload16(types::I64, flags, base, offset),
            (Lane::I8, Extend::Sign) => self.builder.ins().sload8(types::I32, flags, base, offset),
            (Lane::I16, Extend::Sign) => self.builder.ins().sload16(types::I32, flags, base, offset),
            _ => self.builder.ins().uload32(flags, base, offset),
        }
    }
    fn raw_store(&mut self, lane: Lane, flags: MemFlagsData, (value, private): (ir::Value, bool), base: ir::Value, offset: i32) {
        let store = match lane {
            Lane::I8 => self.builder.ins().istore8(flags, value, base, offset),
            Lane::I16 => self.builder.ins().istore16(flags, value, base, offset),
            Lane::I32 => self.builder.ins().istore32(flags, value, base, offset),
            _ => self.builder.ins().store(flags, value, base, offset),
        };
        if private { self.builder.func.private_stores.push(store); }
    }
    fn raw_const(&mut self, lane: Lane, bits: i64) -> ir::Value { self.builder.ins().iconst(machine_type(lane), bits) }
    fn raw_float_const(&mut self, value: f64) -> ir::Value { self.builder.ins().f64const(value) }
    fn raw_binary(&mut self, operation: Binary, a: ir::Value, b: ir::Value) -> ir::Value {
        match operation {
            Binary::Add => self.builder.ins().iadd(a, b),
            Binary::Sub => self.builder.ins().isub(a, b),
            Binary::Mul => self.builder.ins().imul(a, b),
            Binary::And => self.builder.ins().band(a, b),
            Binary::Or => self.builder.ins().bor(a, b),
            Binary::ShiftRightUnsigned => self.builder.ins().ushr(a, b),
            Binary::ShiftRightSigned => self.builder.ins().sshr(a, b),
            Binary::RemainderSigned => self.builder.ins().srem(a, b),
            Binary::FloatMax => self.builder.ins().fmax(a, b),
            Binary::FloatMin => self.builder.ins().fmin(a, b),
        }
    }
    fn raw_compare(&mut self, relation: Relation, a: ir::Value, b: ir::Value) -> ir::Value {
        match relation {
            Relation::FloatUnordered => self.builder.ins().fcmp(FloatCC::Unordered, a, b),
            _ => self.builder.ins().icmp(condition(relation), a, b),
        }
    }
    fn raw_compare_imm(&mut self, relation: Relation, a: ir::Value, bits: i64) -> ir::Value {
        match relation {
            Relation::SignedLess => self.builder.ins().icmp_imm_s(condition(relation), a, bits),
            _ => self.builder.ins().icmp_imm_u(condition(relation), a, bits),
        }
    }
    fn raw_select(&mut self, cond: ir::Value, then: ir::Value, otherwise: ir::Value) -> ir::Value {
        self.builder.ins().select(cond, then, otherwise)
    }
    fn raw_unary(&mut self, operation: Unary, a: ir::Value) -> ir::Value {
        match operation {
            Unary::ZeroExtend(lane) => self.builder.ins().uextend(machine_type(lane), a),
            Unary::Narrow(lane) => self.builder.ins().ireduce(machine_type(lane), a),
            Unary::FromSigned(lane) => self.builder.ins().fcvt_from_sint(machine_type(lane), a),
            Unary::FromUnsigned(lane) => self.builder.ins().fcvt_from_uint(machine_type(lane), a),
            Unary::ToUnsignedSaturating(lane) => self.builder.ins().fcvt_to_uint_sat(machine_type(lane), a),
            _ => self.raw_unary_float(operation, a),
        }
    }
    fn raw_jump(&mut self, block: ir::Block, args: &[ir::Value]) {
        self.builder.ins().jump(block, &block_args(args));
    }
    fn raw_branch(&mut self, cond: ir::Value, then: (ir::Block, &[ir::Value]), otherwise: (ir::Block, &[ir::Value])) {
        self.builder.ins().brif(cond, then.0, &block_args(then.1), otherwise.0, &block_args(otherwise.1));
    }
    fn raw_call(&mut self, signature: ir::SigRef, entry: ir::Value, args: &[ir::Value]) -> Vec<ir::Value> {
        let call = self.builder.ins().call_indirect(signature, entry, args);
        self.builder.inst_results(call).to_vec()
    }
    #[cfg(feature = "aot")]
    fn raw_return(&mut self, values: &[ir::Value]) { self.builder.ins().return_(values); }
}

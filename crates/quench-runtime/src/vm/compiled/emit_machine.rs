//! The specializer's machine: the primitives of `MachineStage` (`staged::machine`) as Cranelift
//! instructions. Every rule written over the primitives (traps, their order, NaN canonicalization)
//! is the interpreter's (`staged::wasm`, `staged::wasm_float`); this file only says what one
//! operator is on the machine.

use super::{Emitter, Host, Kind, scalar_ir};
use crate::staged::machine::{MachineAccess, MachineStage};
use crate::staged::wasm_memory::Access;
use crate::staged::wasm::Width;
use crate::staged::wasm_float::{Float, Truncation};
use crate::wasm::conversion::{ScalarConversionOperator as Conversion, truncation_bounds};
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{I64BinaryOperator as Binary, I64UnaryOperator as Unary};
use cranelift_codegen::ir::{
    self, InstBuilder, MemFlagsData,
    condcodes::{FloatCC, IntCC},
    immediates::{Ieee32, Ieee64},
    types, AbiParam, Signature,
};
use std::convert::Infallible;

const fn integer_type(width: Width) -> ir::Type {
    match width {
        Width::I32 => types::I32,
        Width::I64 => types::I64,
    }
}

/// The machine condition of each comparison operator.
const COMPARISONS: [(Binary, IntCC); 10] = [
    (Binary::Equal, IntCC::Equal),
    (Binary::NotEqual, IntCC::NotEqual),
    (Binary::LessSigned, IntCC::SignedLessThan),
    (Binary::LessUnsigned, IntCC::UnsignedLessThan),
    (Binary::GreaterSigned, IntCC::SignedGreaterThan),
    (Binary::GreaterUnsigned, IntCC::UnsignedGreaterThan),
    (Binary::LessEqualSigned, IntCC::SignedLessThanOrEqual),
    (Binary::LessEqualUnsigned, IntCC::UnsignedLessThanOrEqual),
    (Binary::GreaterEqualSigned, IntCC::SignedGreaterThanOrEqual),
    (
        Binary::GreaterEqualUnsigned,
        IntCC::UnsignedGreaterThanOrEqual,
    ),
];

/// The machine condition of each float comparison operator.
const FLOAT_COMPARISONS: [(FloatBinary, FloatCC); 6] = [
    (FloatBinary::Equal, FloatCC::Equal),
    (FloatBinary::NotEqual, FloatCC::NotEqual),
    (FloatBinary::Less, FloatCC::LessThan),
    (FloatBinary::Greater, FloatCC::GreaterThan),
    (FloatBinary::LessEqual, FloatCC::LessThanOrEqual),
    (FloatBinary::GreaterEqual, FloatCC::GreaterThanOrEqual),
];

impl<H: Host> MachineStage for Emitter<'_, '_, H> {
    type Word = ir::Value;
    type Exit = Infallible;

    fn constant(&mut self, width: Width, value: i64) -> ir::Value {
        self.body
            .core
            .builder
            .ins()
            .iconst(integer_type(width), value)
    }

    fn operate(
        &mut self,
        operator: Binary,
        _width: Width,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        match operator {
            Binary::Add
            | Binary::Subtract
            | Binary::Multiply
            | Binary::MultiplyHighSigned
            | Binary::MultiplyHighUnsigned => self.arithmetic(operator, left, right),
            Binary::And
            | Binary::Or
            | Binary::Xor
            | Binary::ShiftLeft
            | Binary::ShiftRightSigned
            | Binary::ShiftRightUnsigned
            | Binary::RotateLeft
            | Binary::RotateRight => self.bitwise(operator, left, right),
            Binary::DivideSigned
            | Binary::DivideUnsigned
            | Binary::RemainderSigned
            | Binary::RemainderUnsigned => self.divide(operator, left, right),
            _ => self.compare(operator, left, right),
        }
    }

    fn operate_unary(&mut self, operator: Unary, _width: Width, value: ir::Value) -> ir::Value {
        match operator {
            Unary::EqualZero => {
                let flag = self
                    .body
                    .core
                    .builder
                    .ins()
                    .icmp_imm_s(IntCC::Equal, value, 0);
                self.body.core.builder.ins().uextend(types::I32, flag)
            }
            Unary::LeadingZeros => self.body.core.builder.ins().clz(value),
            Unary::TrailingZeros => self.body.core.builder.ins().ctz(value),
            Unary::PopulationCount => self.body.core.builder.ins().popcnt(value),
            Unary::ExtendSigned8 => self.sign_extend(types::I8, value),
            Unary::ExtendSigned16 => self.sign_extend(types::I16, value),
            Unary::ExtendSigned32 => self.sign_extend(types::I32, value),
        }
    }

    fn operate_float(
        &mut self,
        operator: FloatBinary,
        _: Float,
        left: ir::Value,
        right: ir::Value,
    ) -> ir::Value {
        if let Some(&(_, condition)) = FLOAT_COMPARISONS
            .iter()
            .find(|(candidate, _)| *candidate == operator)
        {
            let flag = self.body.core.builder.ins().fcmp(condition, left, right);
            return self.flag_word(flag);
        }
        let ins = self.body.core.builder.ins();
        match operator {
            FloatBinary::Add => ins.fadd(left, right),
            FloatBinary::Subtract => ins.fsub(left, right),
            FloatBinary::Multiply => ins.fmul(left, right),
            FloatBinary::Divide => ins.fdiv(left, right),
            FloatBinary::Minimum => ins.fmin(left, right),
            FloatBinary::Maximum => ins.fmax(left, right),
            FloatBinary::CopySign => ins.fcopysign(left, right),
            _ => unreachable!("comparisons have their conditions"),
        }
    }

    fn operate_float_unary(
        &mut self,
        operator: FloatUnary,
        _: Float,
        value: ir::Value,
    ) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match operator {
            FloatUnary::Absolute => ins.fabs(value),
            FloatUnary::Negate => ins.fneg(value),
            FloatUnary::Ceiling => ins.ceil(value),
            FloatUnary::Floor => ins.floor(value),
            FloatUnary::Truncate => ins.trunc(value),
            FloatUnary::Nearest => ins.nearest(value),
            FloatUnary::SquareRoot => ins.sqrt(value),
        }
    }

    fn float_unordered(&mut self, _: Float, left: ir::Value, right: ir::Value) -> ir::Value {
        let flag = self
            .body
            .core
            .builder
            .ins()
            .fcmp(FloatCC::Unordered, left, right);
        self.flag_word(flag)
    }

    fn float_constant(&mut self, float: Float, bits: u64) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match float {
            Float::F32 => ins.f32const(Ieee32::with_bits(bits as u32)),
            Float::F64 => ins.f64const(Ieee64::with_bits(bits)),
        }
    }

    fn select(&mut self, flag: ir::Value, taken: ir::Value, other: ir::Value) -> ir::Value {
        self.body.core.builder.ins().select(flag, taken, other)
    }

    /// The truncated float, against the bounds of the integers, in the double precision both
    /// float widths promote to exactly.
    fn truncation_overflows(&mut self, truncation: Truncation, value: ir::Value) -> ir::Value {
        let wide = match truncation.float {
            Float::F32 => self.body.core.builder.ins().fpromote(types::F64, value),
            Float::F64 => value,
        };
        let truncated = self.body.core.builder.ins().trunc(wide);
        let (lower, limit) = truncation_bounds(truncation.width.bits(), truncation.signed);
        let lower = self.body.core.builder.ins().f64const(lower);
        let limit = self.body.core.builder.ins().f64const(limit);
        let below = self
            .body
            .core
            .builder
            .ins()
            .fcmp(FloatCC::LessThan, truncated, lower);
        let above =
            self.body
                .core
                .builder
                .ins()
                .fcmp(FloatCC::GreaterThanOrEqual, truncated, limit);
        let overflow = self.body.core.builder.ins().bor(below, above);
        self.flag_word(overflow)
    }

    fn convert(&mut self, operator: Conversion, value: ir::Value) -> ir::Value {
        let result = scalar_ir(operator.result_type());
        let ins = self.body.core.builder.ins();
        match operator {
            Conversion::WrapI64 => ins.ireduce(result, value),
            Conversion::ExtendI32Signed => ins.sextend(result, value),
            Conversion::ExtendI32Unsigned => ins.uextend(result, value),
            Conversion::PromoteF32 => ins.fpromote(result, value),
            Conversion::DemoteF64 => ins.fdemote(result, value),
            Conversion::ReinterpretF32I32
            | Conversion::ReinterpretF64I64
            | Conversion::ReinterpretI32F32
            | Conversion::ReinterpretI64F64 => ins.bitcast(result, MemFlagsData::new(), value),
            _ => self.between_integer_and_float(operator, result, value),
        }
    }

    fn trap_when(&mut self, flag: ir::Value, trap: crate::WasmTrap) -> Result<(), Infallible> {
        self.trap_if(flag, trap);
        Ok(())
    }
}

impl<H: Host> Emitter<'_, '_, H> {
    /// The machine conversions between an integer and a float; a trapping truncation's input
    /// is in range, so it converts as the saturating one does.
    fn between_integer_and_float(
        &mut self,
        operator: Conversion,
        result: ir::Type,
        value: ir::Value,
    ) -> ir::Value {
        let ins = self.body.core.builder.ins();
        match operator {
            Conversion::TruncateF32SignedI32
            | Conversion::TruncateF64SignedI32
            | Conversion::TruncateF32SignedI64
            | Conversion::TruncateF64SignedI64
            | Conversion::SaturateF32SignedI32
            | Conversion::SaturateF64SignedI32
            | Conversion::SaturateF32SignedI64
            | Conversion::SaturateF64SignedI64 => ins.fcvt_to_sint_sat(result, value),
            Conversion::TruncateF32UnsignedI32
            | Conversion::TruncateF64UnsignedI32
            | Conversion::TruncateF32UnsignedI64
            | Conversion::TruncateF64UnsignedI64
            | Conversion::SaturateF32UnsignedI32
            | Conversion::SaturateF64UnsignedI32
            | Conversion::SaturateF32UnsignedI64
            | Conversion::SaturateF64UnsignedI64 => ins.fcvt_to_uint_sat(result, value),
            Conversion::ConvertI32SignedF32
            | Conversion::ConvertI64SignedF32
            | Conversion::ConvertI32SignedF64
            | Conversion::ConvertI64SignedF64 => ins.fcvt_from_sint(result, value),
            _ => ins.fcvt_from_uint(result, value),
        }
    }

    /// A comparison's flag as the `I32` word the stage contract names.
    fn flag_word(&mut self, flag: ir::Value) -> ir::Value {
        self.body.core.builder.ins().uextend(types::I32, flag)
    }

    fn arithmetic(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator {
            Binary::Add => self.body.core.builder.ins().iadd(left, right),
            Binary::Subtract => self.body.core.builder.ins().isub(left, right),
            Binary::Multiply => self.body.core.builder.ins().imul(left, right),
            Binary::MultiplyHighSigned => self.body.core.builder.ins().smulhi(left, right),
            Binary::MultiplyHighUnsigned => self.body.core.builder.ins().umulhi(left, right),
            _ => unreachable!("routed by `operate`"),
        }
    }

    fn bitwise(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator {
            Binary::And => self.body.core.builder.ins().band(left, right),
            Binary::Or => self.body.core.builder.ins().bor(left, right),
            Binary::Xor => self.body.core.builder.ins().bxor(left, right),
            Binary::ShiftLeft => self.body.core.builder.ins().ishl(left, right),
            Binary::ShiftRightSigned => self.body.core.builder.ins().sshr(left, right),
            Binary::ShiftRightUnsigned => self.body.core.builder.ins().ushr(left, right),
            Binary::RotateLeft => self.body.core.builder.ins().rotl(left, right),
            Binary::RotateRight => self.body.core.builder.ins().rotr(left, right),
            _ => unreachable!("routed by `operate`"),
        }
    }

    /// A division the staged traps have cleared.
    fn divide(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        match operator {
            Binary::DivideUnsigned => self.body.core.builder.ins().udiv(left, right),
            Binary::RemainderUnsigned => self.body.core.builder.ins().urem(left, right),
            Binary::DivideSigned => self.body.core.builder.ins().sdiv(left, right),
            Binary::RemainderSigned => self.signed_remainder(left, right),
            _ => unreachable!("routed by `operate`"),
        }
    }

    /// `MIN % -1` is 0; the machine division would fault, so the divisor is 1 there.
    fn signed_remainder(&mut self, left: ir::Value, right: ir::Value) -> ir::Value {
        let ty = self.body.core.builder.func.dfg.value_type(left);
        let negative_one = self
            .body
            .core
            .builder
            .ins()
            .icmp_imm_s(IntCC::Equal, right, -1);
        let one = self.body.core.builder.ins().iconst(ty, 1);
        let divisor = self
            .body
            .core
            .builder
            .ins()
            .select(negative_one, one, right);
        self.body.core.builder.ins().srem(left, divisor)
    }

    fn compare(&mut self, operator: Binary, left: ir::Value, right: ir::Value) -> ir::Value {
        let &(_, condition) = COMPARISONS
            .iter()
            .find(|(candidate, _)| *candidate == operator)
            .expect("a comparison operator");
        let flag = self.body.core.builder.ins().icmp(condition, left, right);
        self.body.core.builder.ins().uextend(types::I32, flag)
    }

    /// The low bits of `value` as `narrow`, sign-extended back to its width.
    fn sign_extend(&mut self, narrow: ir::Type, value: ir::Value) -> ir::Value {
        let ty = self.body.core.builder.func.dfg.value_type(value);
        let low = self.body.core.builder.ins().ireduce(narrow, value);
        self.body.core.builder.ins().sextend(ty, low)
    }
}

/// The byte widths of the narrow accesses.
const BYTE: usize = size_of::<u8>();
const HALF: usize = size_of::<u16>();
const WORD: usize = size_of::<u32>();

impl<H: Host> MachineAccess for Emitter<'_, '_, H> {
    type Flags = MemFlagsData;

    fn load_access(&mut self, access: Access, flags: MemFlagsData, address: ir::Value) -> ir::Value {
        let kind = Kind::of(access.ty).expect("a scalar access");
        let ty = kind.ir();
        let ins = self.body.core.builder.ins();
        match (access.bytes, access.signed) {
            // A float loads straight into its view.
            _ if access.ty != kind.integer() => ins.load(scalar_ir(access.ty), flags, address, 0),
            (BYTE, false) => ins.uload8(ty, flags, address, 0),
            (BYTE, true) => ins.sload8(ty, flags, address, 0),
            (HALF, false) => ins.uload16(ty, flags, address, 0),
            (HALF, true) => ins.sload16(ty, flags, address, 0),
            (WORD, false) if kind == Kind::I64 => ins.uload32(flags, address, 0),
            (WORD, true) if kind == Kind::I64 => ins.sload32(flags, address, 0),
            _ => ins.load(ty, flags, address, 0),
        }
    }

    fn store_access(&mut self, access: Access, flags: MemFlagsData, address: ir::Value, value: ir::Value) {
        let kind = Kind::of(access.ty).expect("a scalar access");
        let ins = self.body.core.builder.ins();
        match access.bytes {
            BYTE => ins.istore8(flags, value, address, 0),
            HALF => ins.istore16(flags, value, address, 0),
            WORD if kind == Kind::I64 => ins.istore32(flags, value, address, 0),
            _ => ins.store(flags, value, address, 0),
        };
    }

    fn load_word(&mut self, width: Width, flags: MemFlagsData, base: ir::Value, offset: i32) -> ir::Value {
        self.body.core.builder.ins().load(integer_type(width), flags, base, offset)
    }

    fn store_word(&mut self, flags: MemFlagsData, value: ir::Value, base: ir::Value, offset: i32) {
        self.body.core.builder.ins().store(flags, value, base, offset);
    }

    fn call_leaf(&mut self, leaf: usize, arguments: &[ir::Value]) -> ir::Value {
        let mut signature = Signature::new(self.call_conv);
        for _ in arguments {
            signature.params.push(AbiParam::new(types::I64));
        }
        signature.returns.push(AbiParam::new(types::I64));
        let signature = self.body.core.builder.import_signature(signature);
        let target = self.constant(Width::I64, leaf as i64);
        let call = self.body.core.builder.ins().call_indirect(signature, target, arguments);
        self.body.core.builder.inst_results(call)[0]
    }
}

//! Bounded physical facts derived from canonical lowered function bodies.
//!
//! These facts select precompiled handlers; they never replace the residual
//! instructions or their complete fallback semantics.

use crate::{machine::CodeView, ops::Constant};

const AFFINE_BODY_LEN: usize = 9;
const MAX_EXACT_INTEGER: i128 = 1i128 << 53;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumericAffineI32 {
    pub(crate) parameter_slot: u16,
    pub(crate) multiplier: i32,
    pub(crate) addend: i32,
}

impl NumericAffineI32 {
    #[inline(always)]
    pub(crate) fn execute(self, input: f64) -> Option<i32> {
        let input = exact_i32(input)?;
        Some(
            input
                .wrapping_mul(self.multiplier)
                .wrapping_add(self.addend),
        )
    }
}

pub(crate) fn numeric_affine_i32(code: CodeView<'_>) -> Option<NumericAffineI32> {
    let [load, multiplier, multiply, add, zero, truncate, ret, tail, tail_ret] =
        instructions(code)?;
    validate_dataflow(load, multiplier, multiply, add, zero, truncate, ret)?;
    validate_unreachable_tail(code, tail, tail_ret)?;
    let multiplier = integer_constant(code, multiplier.b)?;
    let addend = integer_constant(code, add.c)?;
    exact_intermediate_range(multiplier, addend).then_some(NumericAffineI32 {
        parameter_slot: load.b,
        multiplier,
        addend,
    })
}

fn instructions(code: CodeView<'_>) -> Option<[crate::ir::Instruction; AFFINE_BODY_LEN]> {
    (code.len() == AFFINE_BODY_LEN)
        .then(|| std::array::from_fn(|pc| code.instruction(pc).expect("bounded code view")))
}

fn validate_dataflow(
    load: crate::ir::Instruction,
    constant: crate::ir::Instruction,
    multiply: crate::ir::Instruction,
    add: crate::ir::Instruction,
    zero: crate::ir::Instruction,
    truncate: crate::ir::Instruction,
    ret: crate::ir::Instruction,
) -> Option<()> {
    use crate::ir::Opcode;
    (load.opcode == Opcode::LoadLocal
        && constant.opcode == Opcode::LoadConst
        && multiply.opcode == Opcode::Mul
        && multiply.b == load.a
        && multiply.c == constant.a
        && add.opcode == Opcode::AddConst
        && !add.add_const_is_left()
        && add.b == multiply.a
        && zero.opcode == Opcode::LoadConst
        && truncate.opcode == Opcode::Binary
        && crate::ir::compact_binary_operator(truncate.flags)
            == Some(crate::ops::BinaryOp::BitwiseOr)
        && truncate.b == add.a
        && truncate.c == zero.a
        && ret.opcode == Opcode::Return
        && ret.a == truncate.a)
        .then_some(())
}

fn validate_unreachable_tail(
    code: CodeView<'_>,
    tail: crate::ir::Instruction,
    ret: crate::ir::Instruction,
) -> Option<()> {
    use crate::ir::Opcode;
    (tail.opcode == Opcode::LoadConst
        && matches!(code.constant(tail.b), Some(Constant::Undefined))
        && ret.opcode == Opcode::Return
        && ret.a == tail.a)
        .then_some(())
}

fn integer_constant(code: CodeView<'_>, id: u16) -> Option<i32> {
    let Constant::Number(value) = code.constant(id)? else {
        return None;
    };
    exact_i32(*value)
}

fn exact_i32(value: f64) -> Option<i32> {
    if !value.is_finite() || value < i32::MIN as f64 || value > i32::MAX as f64 {
        return None;
    }
    let integer = value as i32;
    (f64::from(integer) == value).then_some(integer)
}

fn exact_intermediate_range(multiplier: i32, addend: i32) -> bool {
    let magnitude = i128::from(i32::MIN).unsigned_abs() as i128;
    magnitude
        .saturating_mul(i128::from(multiplier).abs())
        .saturating_add(i128::from(addend).abs())
        <= MAX_EXACT_INTEGER
}

#[cfg(test)]
mod tests {
    use super::{numeric_affine_i32, NumericAffineI32};

    #[test]
    fn affine_handler_wraps_only_exact_int32_inputs() {
        let fact = NumericAffineI32 {
            parameter_slot: 0,
            multiplier: 33,
            addend: 7,
        };
        assert_eq!(fact.execute(1.0), Some(40));
        assert_eq!(fact.execute(i32::MAX as f64), Some(2_147_483_622));
        for rejected in [0.5, f64::NAN, f64::INFINITY, i32::MAX as f64 + 1.0] {
            assert_eq!(fact.execute(rejected), None);
        }
    }

    #[test]
    fn ordinary_lowering_derives_affine_fact_from_dataflow() {
        let source = "var f=function(value){return (value*29+11)|0};f(3)";
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        let mut facts = Vec::new();
        crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
            if let Some(fact) = numeric_affine_i32(code) {
                facts.push(fact);
            }
        });
        assert_eq!(facts.len(), 1);
        assert_eq!(facts[0].multiplier, 29);
        assert_eq!(facts[0].addend, 11);
    }

    #[test]
    fn changed_effect_or_unsafe_constant_rejects_affine_fact() {
        for body in [
            "return value*29+11",
            "side();return (value*29+11)|0",
            "return (value*9007199254740991+11)|0",
        ] {
            let source = format!("function side(){{}}var f=function(value){{{body}}};f(3)");
            let program = crate::reduce::reduce_source(&source).expect("source lowers");
            let mut admitted = false;
            crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
                admitted |= numeric_affine_i32(code).is_some();
            });
            assert!(!admitted, "unexpected affine admission for {body}");
        }
    }

    #[test]
    fn guarded_affine_execution_preserves_fallback_semantics() {
        let source = concat!(
            "var calls=0;function f(value){return (value*33+7)|0}",
            "var object={valueOf:function(){calls++;return 2}};",
            "var got=[f(1),f(0.5),f(NaN),f(object),calls];",
            "if(JSON.stringify(got)!=='[40,23,0,73,1]')throw new Error(JSON.stringify(got))"
        );
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("guarded affine source executes");
    }
}

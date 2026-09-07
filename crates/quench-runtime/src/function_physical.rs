//! Bounded physical facts derived from canonical lowered function bodies.
//!
//! These facts select precompiled handlers; they never replace the residual
//! instructions or their complete fallback semantics.

use crate::{machine::CodeView, ops::Constant};

const AFFINE_BODY_LEN: usize = 9;
const AFFINE_NAMED_LOOP_LEN: usize = 28;
const MAX_EXACT_INTEGER: i128 = 1i128 << 53;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(crate) struct NumericAffineI32 {
    pub(crate) parameter_slot: u16,
    pub(crate) multiplier: i32,
    pub(crate) addend: i32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub(crate) struct NumericAffineNamedLoop {
    pub(crate) parameter_slot: u16,
    pub(crate) seed_key: std::rc::Rc<str>,
    pub(crate) bound_key: std::rc::Rc<str>,
    pub(crate) method_key: std::rc::Rc<str>,
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

pub(crate) fn numeric_affine_named_loop(code: CodeView<'_>) -> Option<NumericAffineNamedLoop> {
    let ops = instruction_array::<AFFINE_NAMED_LOOP_LEN>(code)?;
    validate_named_loop_prefix(code, &ops)?;
    validate_named_loop_test(code, &ops)?;
    validate_named_loop_body(code, &ops)?;
    validate_named_loop_update(code, &ops)?;
    validate_named_loop_exit(code, &ops)?;
    Some(NumericAffineNamedLoop {
        parameter_slot: ops[0].b,
        seed_key: metadata_name(code, 1)?,
        bound_key: metadata_name(code, 9)?,
        method_key: metadata_name(code, 13)?,
    })
}

fn instruction_array<const N: usize>(code: CodeView<'_>) -> Option<[crate::ir::Instruction; N]> {
    (code.len() == N).then(|| std::array::from_fn(|pc| code.instruction(pc).unwrap()))
}

fn metadata_name(code: CodeView<'_>, pc: usize) -> Option<std::rc::Rc<str>> {
    code.metadata_at(pc)?.name.clone()
}

fn instructions(code: CodeView<'_>) -> Option<[crate::ir::Instruction; AFFINE_BODY_LEN]> {
    (code.len() == AFFINE_BODY_LEN)
        .then(|| std::array::from_fn(|pc| code.instruction(pc).expect("bounded code view")))
}

fn validate_named_loop_prefix(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; AFFINE_NAMED_LOOP_LEN],
) -> Option<()> {
    use crate::ir::Opcode;
    (ops[0].opcode == Opcode::LoadLocal
        && ops[1].opcode == Opcode::GetN
        && ops[1].b == ops[0].a
        && ops[2].opcode == Opcode::StoreLocal
        && ops[2].b == ops[1].a
        && undefined_constant(code, ops[3])
        && number_constant(code, ops[4], 0.0)
        && ops[5].opcode == Opcode::StoreLocal
        && ops[5].b == ops[4].a
        && undefined_constant(code, ops[6]))
    .then_some(())
}

fn validate_named_loop_test(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; AFFINE_NAMED_LOOP_LEN],
) -> Option<()> {
    use crate::ir::Opcode;
    (ops[7].opcode == Opcode::LoadLocal
        && ops[7].b == ops[5].a
        && ops[8].opcode == Opcode::LoadLocal
        && ops[8].b == ops[0].b
        && ops[9].opcode == Opcode::GetN
        && ops[9].b == ops[8].a
        && metadata_name(code, 9).is_some()
        && ops[10].opcode == Opcode::Binary
        && crate::ir::compact_binary_operator(ops[10].flags)
            == Some(crate::ops::BinaryOp::LessThan)
        && ops[10].b == ops[7].a
        && ops[10].c == ops[9].a
        && ops[11] == crate::ir::Instruction::jump_if_false(ops[10].a, 24))
    .then_some(())
}

fn validate_named_loop_body(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; AFFINE_NAMED_LOOP_LEN],
) -> Option<()> {
    use crate::ir::Opcode;
    let arguments = code.operand_window_at(15)?;
    (ops[12].opcode == Opcode::LoadLocal
        && ops[12].b == ops[0].b
        && ops[13].opcode == Opcode::GetN
        && ops[13].b == ops[12].a
        && ops[14].opcode == Opcode::LoadLocal
        && ops[14].b == ops[2].a
        && ops[15].opcode == Opcode::CallN
        && ops[15].flags == 1
        && ops[15].b == ops[12].a
        && ops[15].c == ops[13].a
        && arguments == [ops[14].a]
        && metadata_name(code, 13)? == metadata_name(code, 15)?
        && ops[16].opcode == Opcode::StoreLocal
        && ops[16].a == ops[2].a
        && ops[16].b == ops[15].a
        && ops[17].opcode == Opcode::Move
        && ops[17].b == ops[15].a)
        .then_some(())
}

fn validate_named_loop_update(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; AFFINE_NAMED_LOOP_LEN],
) -> Option<()> {
    use crate::ir::Opcode;
    (ops[18].opcode == Opcode::LoadLocal
        && ops[18].b == ops[5].a
        && number_constant(code, ops[19], 1.0)
        && ops[20].opcode == Opcode::Binary
        && crate::ir::compact_binary_operator(ops[20].flags)
            == Some(crate::ops::BinaryOp::NumericAdd)
        && ops[20].b == ops[18].a
        && ops[20].c == ops[19].a
        && ops[21].opcode == Opcode::StoreLocal
        && ops[21].a == ops[5].a
        && ops[21].b == ops[20].a
        && ops[22].opcode == Opcode::Unary
        && crate::ir::compact_unary_operator(ops[22].flags) == Some(crate::ops::UnaryOp::ToNumeric)
        && ops[22].b == ops[18].a
        && ops[23].opcode == Opcode::Jump
        && ops[23].a == 7)
        .then_some(())
}

fn validate_named_loop_exit(
    code: CodeView<'_>,
    ops: &[crate::ir::Instruction; AFFINE_NAMED_LOOP_LEN],
) -> Option<()> {
    use crate::ir::Opcode;
    (ops[24].opcode == Opcode::LoadLocal
        && ops[24].b == ops[2].a
        && ops[25] == crate::ir::Instruction::ret(ops[24].a)
        && undefined_constant(code, ops[26])
        && ops[27] == crate::ir::Instruction::ret(ops[26].a))
    .then_some(())
}

fn undefined_constant(code: CodeView<'_>, instruction: crate::ir::Instruction) -> bool {
    instruction.opcode == crate::ir::Opcode::LoadConst
        && matches!(code.constant(instruction.b), Some(Constant::Undefined))
}

fn number_constant(code: CodeView<'_>, instruction: crate::ir::Instruction, expected: f64) -> bool {
    instruction.opcode == crate::ir::Opcode::LoadConst
        && matches!(code.constant(instruction.b), Some(Constant::Number(value)) if *value == expected)
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
    use super::{numeric_affine_i32, numeric_affine_named_loop, NumericAffineI32};

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

    #[test]
    fn ordinary_lowering_derives_named_loop_from_dataflow() {
        let source = concat!(
            "function iterate(state){var total=state.initial;",
            "for(var cursor=0;cursor<state.limit;cursor++)total=state.step(total);",
            "return total}",
            "iterate({initial:3,limit:5,step:function(v){return (v*29+11)|0}})"
        );
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        let mut facts = Vec::new();
        crate::stencil_test_support::visit_code_views(program.code(), &mut |code| {
            facts.extend(numeric_affine_named_loop(code));
        });
        assert_eq!(facts.len(), 1);
        assert_eq!(&*facts[0].seed_key, "initial");
        assert_eq!(&*facts[0].bound_key, "limit");
        assert_eq!(&*facts[0].method_key, "step");
    }

    #[test]
    fn ordinary_named_loop_executes_precompiled_region() {
        let source = concat!(
            "function iterate(state){var total=state.initial;",
            "for(var cursor=0;cursor<state.limit;cursor++)total=state.step(total);",
            "return total}",
            "var got=iterate({initial:3,limit:5,step:function(v){return (v*29+11)|0}});",
            "if(got!==69591398)throw new Error(String(got))"
        );
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        let _ = crate::functions::take_affine_named_loop_hits();
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("precompiled named loop executes");
        assert_eq!(crate::functions::take_affine_named_loop_hits(), 1);
    }

    #[test]
    fn object_method_named_loop_executes_precompiled_region() {
        let source = concat!(
            "var setup=function(n,seed){return {n:n,seed:seed,",
            "f:function(x){return (x*33+7)|0},g:function(x){return (x*33+7)|0}}};",
            "var direct=function(s){var x=s.seed;",
            "for(var i=0;i<s.n;i++)x=s.f(x);return x};",
            "var got=direct(setup(64,17));if(got!==-451678767)throw new Error(String(got))"
        );
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        let _ = crate::functions::take_affine_named_loop_hits();
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("object method named loop executes");
        assert_eq!(crate::functions::take_affine_named_loop_hits(), 1);
    }

    #[test]
    fn nested_catalog_function_executes_precompiled_region() {
        let source = concat!(
            "var spec;function registerMicro(value){spec=value}",
            "registerMicro({setup:function(n,seed){return {n:n,seed:seed,",
            "f:function(x){return (x*33+7)|0}}},variants:{direct:function(s){",
            "var x=s.seed;for(var i=0;i<s.n;i++)x=s.f(x);return x}}});",
            "var state=spec.setup(64,17),got;",
            "for(var run=0;run<40;run++)got=spec.variants.direct(state);",
            "if(got!==-451678767)throw new Error(String(got))"
        );
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        let _ = crate::functions::take_affine_named_loop_hits();
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("nested catalog function executes");
        assert_eq!(crate::functions::take_affine_named_loop_hits(), 40);
    }

    #[test]
    fn named_loop_guard_misses_preserve_observable_reads() {
        let source = concat!(
            "var nr=0,fr=0,coercions=0;",
            "function run(s){var x=s.seed;for(var i=0;i<s.n;i++)x=s.f(x);return x}",
            "var s={seed:{valueOf:function(){coercions++;return 2}}};",
            "Object.defineProperty(s,'n',{get:function(){nr++;return 2.5}});",
            "Object.defineProperty(s,'f',{get:function(){fr++;return function(x){return (x*3+1)|0}}});",
            "var got=run(s);if(got!==67||nr!==4||fr!==3||coercions!==1)",
            "throw new Error(JSON.stringify([got,nr,fr,coercions]));",
            "var zeroReads=0,z={seed:9,n:0};",
            "Object.defineProperty(z,'f',{get:function(){zeroReads++;return function(x){return x}}});",
            "if(run(z)!==9||zeroReads!==0)throw new Error('zero:'+zeroReads)"
        );
        let program = crate::reduce::reduce_source(source).expect("source lowers");
        crate::vm::execute_code_with_context(program.code(), &crate::vm::VmContext::default())
            .expect("guard failures retain ordinary effects");
    }
}

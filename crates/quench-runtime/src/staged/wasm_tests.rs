//! The Wasm integer rules of `staged::wasm` on the interpreter's stage against the declaration
//! (`apply`), the order of a trap's checks on a probe stage, and the opcode table.

use super::ConcreteWasm;
use super::machine::{Machine, MachineStage};
use super::wasm::{Width, binary_result, integer_binary, is_comparison};
use super::wasm_float::{Float, Truncation};
use super::wasm_operation::{Form, WasmOperation, wasm_operation_of};
use crate::bytecode::Op;
use crate::wasm::conversion::ScalarConversionOperator as Conversion;
use crate::wasm::float::{F64BinaryOperator as FloatBinary, F64UnaryOperator as FloatUnary};
use crate::wasm::integer::{
    I32BinaryOperator, I32UnaryOperator, I64BinaryOperator as Binary, I64UnaryOperator as Unary,
};
use crate::{WasmTrap, WasmType, WasmValue};

/// Operands at the edges of both widths: zero, signs, shift counts at and past the width,
/// the extremes and a pattern with every byte distinct.
const OPERANDS: [i64; 12] = [
    0,
    1,
    -1,
    2,
    7,
    31,
    32,
    63,
    64,
    i64::MAX,
    i64::MIN,
    0x1234_5678_9abc_def0,
];

fn every_binary() -> impl Iterator<Item = Binary> {
    (0..).map_while(Binary::from_tag)
}

#[test]
fn the_i64_rules_of_the_stage_are_the_declared_rules() {
    for operator in every_binary() {
        for (left, right) in OPERANDS
            .iter()
            .flat_map(|l| OPERANDS.iter().map(move |r| (*l, *r)))
        {
            assert_eq!(
                operator.evaluate(left, right),
                operator.apply(left, right),
                "{operator:?} {left} {right}"
            );
        }
    }
}

#[test]
fn the_i32_rules_of_the_stage_are_the_declared_rules() {
    for operator in I32BinaryOperator::ALL.iter().copied() {
        for (left, right) in OPERANDS
            .iter()
            .flat_map(|l| OPERANDS.iter().map(move |r| (*l as i32, *r as i32)))
        {
            assert_eq!(
                operator.evaluate(left, right).map(WasmValue::I32),
                operator.apply(left, right),
                "{operator:?} {left} {right}"
            );
        }
    }
}

#[test]
fn the_unary_rules_of_the_stage_are_the_declared_rules() {
    for operator in (0..).map_while(Unary::from_tag) {
        for value in OPERANDS {
            assert_eq!(
                operator.evaluate(value),
                operator.apply(value),
                "{operator:?} {value}"
            );
        }
    }
}

#[test]
fn a_narrow_operator_is_its_wide_namesake() {
    for operator in I32BinaryOperator::ALL.iter().copied() {
        assert_eq!(format!("{operator:?}"), format!("{:?}", operator.widen()));
        assert_eq!(operator as u32, operator.widen() as u32);
    }
    for operator in (0..).map_while(I32UnaryOperator::from_tag) {
        assert_eq!(format!("{operator:?}"), format!("{:?}", operator.widen()));
        assert_eq!(operator as u32, operator.widen() as u32);
    }
}

#[test]
fn a_comparison_is_what_the_declaration_says_yields_an_i32() {
    for operator in every_binary() {
        let declared = operator.apply(1, 1).expect("1 op 1 does not trap").ty() == WasmType::I32;
        assert_eq!(is_comparison(operator), declared, "{operator:?}");
        assert_eq!(
            binary_result(operator, Width::I64) == Width::I32,
            declared,
            "{operator:?}"
        );
    }
}

/// Records what a rule asks of its stage, in order.
#[derive(Default)]
pub(super) struct Probe {
    pub(super) log: Vec<String>,
    /// Whether the probed machine faults on an access past its memory by itself.
    pub(super) guarded: bool,
}

impl Probe {
    fn record(&mut self, entry: String) -> usize {
        self.log.push(entry);
        self.log.len() - 1
    }
}

impl Machine for Probe {
    type Word = usize;
}

crate::projection!(Probe => Concrete);

impl MachineStage for Probe {
    type Exit = WasmTrap;

    fn constant(&mut self, width: Width, value: i64) -> usize {
        self.record(format!("constant {width:?} {value}"))
    }

    fn operate(&mut self, operator: Binary, width: Width, _: usize, _: usize) -> usize {
        self.record(format!("{operator:?} {width:?}"))
    }

    fn operate_unary(&mut self, operator: Unary, width: Width, _: usize) -> usize {
        self.record(format!("{operator:?} {width:?}"))
    }

    fn operate_float(&mut self, operator: FloatBinary, float: Float, _: usize, _: usize) -> usize {
        self.record(format!("{operator:?} {float:?}"))
    }

    fn operate_float_unary(&mut self, operator: FloatUnary, float: Float, _: usize) -> usize {
        self.record(format!("{operator:?} {float:?}"))
    }

    fn float_unordered(&mut self, float: Float, _: usize, _: usize) -> usize {
        self.record(format!("Unordered {float:?}"))
    }

    fn float_constant(&mut self, float: Float, bits: u64) -> usize {
        self.record(format!("constant {float:?} {bits:#x}"))
    }

    fn select(&mut self, _: usize, _: usize, _: usize) -> usize {
        self.record("select".into())
    }

    fn truncation_overflows(&mut self, truncation: Truncation, _: usize) -> usize {
        self.record(format!("overflows {truncation:?}"))
    }

    fn convert(&mut self, operator: Conversion, _: usize) -> usize {
        self.record(format!("{operator:?}"))
    }

    fn trap_when(&mut self, _: usize, trap: WasmTrap) -> Result<(), WasmTrap> {
        self.record(format!("trap {trap:?}"));
        Ok(())
    }
}

fn probed(operator: Binary, width: Width) -> Vec<String> {
    let mut probe = Probe::default();
    integer_binary(&mut probe, operator, width, 0, 0).expect("the probe never traps");
    probe.log
}

#[test]
fn a_signed_division_checks_zero_then_overflow_then_divides() {
    assert_eq!(
        probed(Binary::DivideSigned, Width::I32),
        [
            "constant I32 0",
            "Equal I32",
            "trap IntegerDivideByZero",
            "constant I32 -2147483648",
            "constant I32 -1",
            "Equal I32",
            "Equal I32",
            "And I32",
            "trap IntegerOverflow",
            "DivideSigned I32",
        ]
    );
}

#[test]
fn the_other_divisions_check_only_zero() {
    for operator in [
        Binary::DivideUnsigned,
        Binary::RemainderSigned,
        Binary::RemainderUnsigned,
    ] {
        assert_eq!(
            probed(operator, Width::I64),
            [
                "constant I64 0".to_owned(),
                "Equal I64".into(),
                "trap IntegerDivideByZero".into(),
                format!("{operator:?} I64")
            ]
        );
    }
}

#[test]
fn an_operator_that_cannot_trap_asks_for_nothing_else() {
    for operator in every_binary().filter(|operator| {
        !matches!(
            operator,
            Binary::DivideSigned
                | Binary::DivideUnsigned
                | Binary::RemainderSigned
                | Binary::RemainderUnsigned
        )
    }) {
        assert_eq!(probed(operator, Width::I32), [format!("{operator:?} I32")]);
    }
}

#[test]
fn the_stage_traps_where_the_declaration_does() {
    assert_eq!(
        Binary::DivideSigned.evaluate(i64::MIN, -1),
        Err(WasmTrap::IntegerOverflow)
    );
    assert_eq!(
        I32BinaryOperator::DivideSigned.evaluate(i32::MIN, -1),
        Err(WasmTrap::IntegerOverflow)
    );
    assert_eq!(
        I32BinaryOperator::RemainderSigned.evaluate(i32::MIN, -1),
        Ok(0)
    );
    assert_eq!(
        Binary::RemainderUnsigned.evaluate(1, 0),
        Err(WasmTrap::IntegerDivideByZero)
    );
}

#[test]
fn the_table_names_every_i32_form_of_an_operator() {
    for operator in I32BinaryOperator::ALL.iter().copied() {
        let binary = |form| {
            Some(WasmOperation::Binary {
                operator: operator.widen(),
                width: Width::I32,
                form,
            })
        };
        assert_eq!(
            wasm_operation_of(operator.register_op(), 0),
            binary(Form::Register)
        );
        assert_eq!(
            wasm_operation_of(operator.immediate_op(), 0),
            binary(Form::Immediate)
        );
        assert_eq!(
            operator.jump_op().and_then(|op| wasm_operation_of(op, 0)),
            operator.jump_op().and(binary(Form::Branch))
        );
        assert_eq!(
            operator
                .immediate_jump_op()
                .and_then(|op| wasm_operation_of(op, 0)),
            operator
                .immediate_jump_op()
                .and(binary(Form::BranchImmediate))
        );
    }
}

#[test]
fn a_selector_opcode_is_the_operator_it_selects() {
    for operator in every_binary() {
        assert_eq!(
            wasm_operation_of(Op::WasmI64Binary, operator as u32),
            Some(WasmOperation::Binary {
                operator,
                width: Width::I64,
                form: Form::Register
            })
        );
    }
    assert_eq!(wasm_operation_of(Op::WasmI64Binary, u32::MAX), None);
    assert_eq!(wasm_operation_of(Op::WasmI64Unary, u32::MAX), None);
    assert_eq!(wasm_operation_of(Op::WasmI32Unary, u32::MAX), None);
    assert_eq!(wasm_operation_of(Op::Move, 0), None);
}

#[test]
fn a_comparison_is_the_only_operation_that_branches_without_writing() {
    for operator in I32BinaryOperator::ALL.iter().copied() {
        let operation = wasm_operation_of(operator.register_op(), 0).expect("a register form");
        assert!(operation.writes() && !operation.jumps());
    }
    let test = wasm_operation_of(Op::WasmJumpI32Zero, 0).expect("a test");
    assert!(!test.writes() && test.jumps());
    let add = wasm_operation_of(Op::WasmI32AddImmediateJumpNonZero, 0).expect("a fused add");
    assert!(add.writes() && add.jumps());
}

#[test]
fn the_concrete_words_of_a_narrow_width_stay_zero_extended() {
    let word = integer_binary(&mut ConcreteWasm, Binary::Subtract, Width::I32, 0, 1)
        .expect("subtraction does not trap");
    assert_eq!(word, u64::from(u32::MAX));
}

/// The b-jit's Wasm row codes (`quench-bjit/stencils/wasm_kinds.rs`) name operators by these
/// selector tags: the i32 operators first, the comparisons in one range.
#[test]
fn the_wasm_row_codes_match_the_selectors() {
    use quench_bjit::wasm as codes;
    assert_eq!(I32BinaryOperator::ALL.len(), codes::SHARED_BINARY as usize);
    assert_eq!(Binary::ALL.len(), codes::BINARY_COUNT as usize);
    assert_eq!(I32UnaryOperator::ALL.len(), codes::SHARED_UNARY as usize);
    assert_eq!(Unary::ALL.len(), codes::UNARY_COUNT as usize);
    for tag in 0..codes::SHARED_BINARY {
        assert_eq!(I32BinaryOperator::ALL[tag as usize].widen() as u32, tag);
    }
    for tag in 0..codes::SHARED_UNARY {
        assert_eq!(I32UnaryOperator::from_tag(tag).unwrap().widen() as u32, tag);
    }
    for tag in 0..codes::BINARY_COUNT {
        let comparison = (codes::FIRST_COMPARISON..=codes::LAST_COMPARISON).contains(&tag);
        assert_eq!(is_comparison(Binary::from_tag(tag).unwrap()), comparison, "{tag}");
    }
}

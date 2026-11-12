//! The register, constant, binding and global steps of `staged::wasm_instance`: the opcode
//! table, the registers a constant defines, the global primitives on the interpreter's stage
//! and the instance stage's scalar rules against `ConcreteWasm`.

use super::ConcreteWasm;
use super::wasm::{WasmStage, Width, integer_binary};
use super::wasm_instance::{Instance, WasmInstance};
use super::wasm_machine::ConcreteInstance;
use super::wasm_step::Step;
use crate::bytecode::{Op, WideInstruction};
use crate::wasm::integer::I64BinaryOperator as Binary;

#[test]
fn the_table_names_the_register_and_instance_opcodes() {
    let named = [
        (Op::Move, Instance::Move),
        (Op::LoadConst, Instance::Constant),
        (Op::WasmFillRegisters, Instance::Fill),
        (Op::WasmInstanceBinding, Instance::Binding),
        (Op::WasmGlobalGet, Instance::GlobalGet),
        (Op::WasmGlobalSet, Instance::GlobalSet),
    ];
    for (op, step) in named {
        assert_eq!(Instance::of(op), Some(step), "{op:?}");
        assert_eq!(Step::of(op, 0), Some(Step::Instance(step)), "{op:?}");
    }
    assert_eq!(Instance::of(Op::WasmI32Add), None);
    assert!(matches!(
        Step::of(Op::WasmI32Add, 0),
        Some(Step::Numeric(_))
    ));
}

#[test]
fn only_a_global_access_reads_a_global() {
    assert!(Instance::GlobalGet.accesses_global() && Instance::GlobalSet.accesses_global());
    assert!(!Instance::Move.accesses_global() && !Instance::Binding.accesses_global());
}

#[test]
fn a_constant_defines_one_register_and_a_fill_its_window() {
    let constant = WideInstruction::new(Op::LoadConst, 5, 0, 0, 3);
    assert_eq!(Instance::Constant.constant_registers(constant), Some(5..6));
    let fill = WideInstruction::new(Op::WasmFillRegisters, 0, 8, 3, 3);
    assert_eq!(Instance::Fill.constant_registers(fill), Some(8..11));
    assert_eq!(Instance::Move.constant_registers(constant), None);
}

#[test]
fn a_global_holds_the_i32_it_was_given() {
    let mut globals = [0u64; 2];
    let mut stage = ConcreteInstance {
        globals: &mut globals,
        memory: &mut [],
    };
    stage.global_store(1, 0xffff_ffff_8000_0001);
    assert_eq!(stage.global_load(1), 0x8000_0001);
    assert_eq!(stage.global_load(0), 0);
    assert_eq!(globals, [0, 0x8000_0001]);
}

#[test]
fn the_instance_stage_computes_what_the_scalar_stage_computes() {
    let operands = [0i64, 1, -1, 7, 64, i64::MAX, i64::MIN];
    let mut globals = [0u64; 1];
    let mut instance = ConcreteInstance {
        globals: &mut globals,
        memory: &mut [],
    };
    for operator in (0..).map_while(Binary::from_tag) {
        for (left, right) in operands
            .iter()
            .flat_map(|l| operands.iter().map(move |r| (*l as u64, *r as u64)))
        {
            for width in [Width::I32, Width::I64] {
                let (left, right) = (
                    ConcreteWasm.constant(width, left as i64),
                    ConcreteWasm.constant(width, right as i64),
                );
                assert_eq!(
                    integer_binary(&mut instance, operator, width, left, right),
                    integer_binary(&mut ConcreteWasm, operator, width, left, right),
                    "{operator:?} {width:?} {left} {right}"
                );
            }
        }
    }
}

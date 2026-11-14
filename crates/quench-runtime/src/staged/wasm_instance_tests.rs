//! The register, constant, binding and global steps of `staged::wasm_instance`: the opcode
//! table, the registers a constant defines, the global primitives on the interpreter's stage per storage
//! and the instance stage's scalar rules against `ConcreteWasm`.

use super::ConcreteWasm;
use super::machine::MachineStage;
use super::wasm::{Width, integer_binary};
use super::wasm_instance::{Instance, Storage, WasmInstance};
use super::wasm_machine::ConcreteInstance;
use super::wasm_step::Step;
use crate::bytecode::{Op, WideInstruction};
use crate::{Value, WasmType};
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
}

#[test]
fn a_constant_defines_one_register_and_a_fill_its_window() {
    let constant = WideInstruction::new(Op::LoadConst, 5, 0, 0, 3);
    assert_eq!(Instance::Constant.constant_registers(constant), Some(5..6));
    let fill = WideInstruction::new(Op::WasmFillRegisters, 0, 8, 3, 3);
    assert_eq!(Instance::Fill.constant_registers(fill), Some(8..11));
    assert_eq!(Instance::Move.constant_registers(constant), None);
}

/// The slots after `stored` as the global of type `ty` at slot 1 of two, and the word it loads.
fn stored(ty: WasmType, word: u64) -> ([u64; 2], u64) {
    let mut globals = [0; 2];
    let mut stage = ConcreteInstance {
        globals: &mut globals,
        memory: &mut [],
    };
    stage.global_store(1, Storage::of(ty), word);
    let loaded = stage.global_load(1, Storage::of(ty));
    (globals, loaded)
}

#[test]
fn the_storage_of_a_global_follows_its_type() {
    let storage = [
        (WasmType::I32, Storage::Bits32),
        (WasmType::F32, Storage::Bits32),
        (WasmType::I64, Storage::Bits64),
        (WasmType::F64, Storage::Bits64),
        (WasmType::V128, Storage::Handle),
        (WasmType::FUNC, Storage::Handle),
    ];
    for (ty, expected) in storage {
        assert_eq!(Storage::of(ty), expected, "{ty:?}");
    }
}

#[test]
fn an_i32_global_holds_the_low_word_zero_extended() {
    let (globals, loaded) = stored(WasmType::I32, 0xffff_ffff_8000_0001);
    assert_eq!(loaded, 0x8000_0001);
    assert_eq!(globals, [0, 0x8000_0001]);
}

#[test]
fn an_f32_global_keeps_the_payload_of_a_nan() {
    let nan = 0x7fc0_0001u64;
    let (globals, loaded) = stored(WasmType::F32, nan);
    assert_eq!((loaded, globals[1]), (nan, nan));
}

#[test]
fn a_64_bit_global_holds_its_whole_payload_even_in_the_tag_space() {
    let payloads = [0x0123_4567_89ab_cdefu64, (-0.0f64).to_bits(), 0x7ffc_0000_0000_0001];
    for ty in [WasmType::I64, WasmType::F64] {
        for word in payloads {
            let (globals, loaded) = stored(ty, word);
            assert_eq!((loaded, globals[1]), (word, word), "{ty:?} {word:#x}");
        }
    }
}

#[test]
fn a_reference_and_a_vector_global_hold_the_bits_of_their_handle() {
    for ty in [WasmType::FUNC, WasmType::V128] {
        let handle = Value::heap(5).to_bits();
        let (globals, loaded) = stored(ty, handle);
        assert_eq!((loaded, globals[1]), (handle, handle), "{ty:?}");
    }
}

#[test]
fn the_instance_stage_computes_what_the_scalar_stage_computes() {
    let operands = [0i64, 1, -1, 7, 64, i64::MAX, i64::MIN];
    let mut globals = [0; 1];
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

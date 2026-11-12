//! An indirect call's target on the VM: the trap of each check, and the signature words a table
//! carries beside its references through every event that writes it.

use super::*;
use crate::{
    Engine, WasmCallableType, WasmFunctionBody, WasmInstance, WasmTable, WasmTrap, WasmType,
    WasmTypes, WasmValue,
};
use wasmparser::{BinaryReaderError, FuncType, Operator, RefType, TableType, ValType};

const CALLER: u32 = 2;
const SEVEN: i32 = 7;

fn body(
    ty: u32,
    operators: Vec<Operator<'static>>,
) -> WasmFunctionBody<Vec<Result<Operator<'static>, BinaryReaderError>>> {
    WasmFunctionBody {
        ty: WasmCallableType::Declared(ty),
        locals: vec![],
        operators: operators.into_iter().map(Ok).collect(),
    }
}

fn funcref_table(initial: u32, function: u32) -> WasmTable {
    WasmTable {
        ty: TableType {
            element_type: RefType::FUNCREF,
            table64: false,
            initial: u64::from(initial),
            maximum: Some(u64::from(initial) + 2),
            shared: false,
        },
        initializer: crate::WasmReferenceInitializer::Function(function).into(),
    }
}

/// Function 0 returns seven, function 1 returns an i64, and function 2 calls the element of
/// table 0 its argument names as a `() -> i32`. Table 0 holds three of function 0, table 1 one of
/// function 1.
fn module() -> crate::WasmModule {
    Engine::lower_wasm_module_with_tables(
        "indirect-traps",
        [
            body(0, vec![Operator::I32Const { value: SEVEN }, Operator::End]),
            body(1, vec![Operator::I64Const { value: 1 }, Operator::End]),
            body(
                2,
                vec![
                    Operator::LocalGet { local_index: 0 },
                    Operator::CallIndirect {
                        type_index: 0,
                        table_index: 0,
                    },
                    Operator::End,
                ],
            ),
        ],
        &WasmTypes::from_functions([
            FuncType::new([], [ValType::I32]),
            FuncType::new([], [ValType::I64]),
            FuncType::new([ValType::I32], [ValType::I32]),
        ]),
        &[],
        &[],
        &[],
        &[funcref_table(3, 0), funcref_table(1, 1)],
    )
    .unwrap()
}

fn call(vm: &mut Vm<crate::SystemHost>, instance: &WasmInstance, index: i32) -> Result<i32, WasmTrap> {
    match vm.invoke_wasm(instance, CALLER, &[WasmValue::I32(index)]) {
        Ok(Some(WasmValue::I32(value))) => Ok(value),
        Ok(other) => panic!("an i32 result, not {other:?}"),
        Err(error) => Err(error.wasm_trap().expect("a Wasm trap")),
    }
}

fn signatures(vm: &Vm<crate::SystemHost>, table: Value) -> Vec<CanonicalSignature> {
    match vm.heap.get(table) {
        Some(Cell::WasmTable { elements, .. }) => elements.signatures().to_vec(),
        _ => panic!("a table"),
    }
}

#[test]
fn each_check_of_an_indirect_call_traps_as_the_rule_orders_them() {
    let mut vm = Vm::new(crate::SystemHost);
    let instance = vm.instantiate_wasm_module(&module()).unwrap();
    let (table, other) = (
        vm.wasm_table(&instance, 0).unwrap(),
        vm.wasm_table(&instance, 1).unwrap(),
    );
    let other_function = vm.wasm_table_get(other, 0).unwrap();
    vm.wasm_table_fill(table, 1, Value::NULL, 1).unwrap();
    vm.wasm_table_fill(table, 2, other_function, 1).unwrap();

    assert_eq!(call(&mut vm, &instance, 0), Ok(SEVEN));
    assert_eq!(call(&mut vm, &instance, 1), Err(WasmTrap::UninitializedElement));
    assert_eq!(call(&mut vm, &instance, 2), Err(WasmTrap::IndirectCallTypeMismatch));
    assert_eq!(call(&mut vm, &instance, 3), Err(WasmTrap::UndefinedElement));
    assert_eq!(call(&mut vm, &instance, -1), Err(WasmTrap::UndefinedElement));
}

#[test]
fn a_table_carries_the_signature_of_each_element_through_every_write() {
    let mut vm = Vm::new(crate::SystemHost);
    let instance = vm.instantiate_wasm_module(&module()).unwrap();
    let (table, other) = (
        vm.wasm_table(&instance, 0).unwrap(),
        vm.wasm_table(&instance, 1).unwrap(),
    );
    let first = signatures(&vm, table)[0];
    let second = signatures(&vm, other)[0];
    assert_ne!(first, second);
    assert_eq!(signatures(&vm, table), [first; 3]);

    let function = vm.wasm_table_get(other, 0).unwrap();
    vm.wasm_table_fill(table, 1, function, 1).unwrap();
    vm.wasm_table_fill(table, 2, Value::NULL, 1).unwrap();
    assert_eq!(signatures(&vm, table), [first, second, CanonicalSignature::NULL]);

    vm.wasm_table_grow(table, function, 2).unwrap();
    assert_eq!(
        signatures(&vm, table),
        [first, second, CanonicalSignature::NULL, second, second]
    );
    vm.wasm_table_copy(table, table, 3, 0, 2).unwrap();
    vm.wasm_table_copy(table, other, 2, 0, 1).unwrap();
    assert_eq!(
        signatures(&vm, table),
        [first, second, second, first, second]
    );
    assert_eq!(signatures(&vm, table).len(), vm.wasm_table_elements(table).unwrap().len());
}

//! An indirect call's target on the VM and in the tier: the trap of each check, the row a table
//! carries beside its references through every event that writes it, and the calls through them
//! (moved from the core's `vm/wasm_indirect` tests, whose binary does not link the engine).

use crate::engine_prelude::*;
use crate::ext_traits::*;
use quench_runtime::heap::TableTarget;
use quench_runtime::vm::engine_types::InstanceContext;
use quench_runtime::wasm::CanonicalSignature;
use quench_runtime::{
    Engine, WasmCallableType, WasmFunctionBody, WasmInstance, WasmTable, WasmTrap,
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
        initializer: quench_runtime::WasmReferenceInitializer::Function(function).into(),
    }
}

/// Function 0 returns seven, function 1 returns an i64, and function 2 calls the element of
/// table 0 its argument names as a `() -> i32`. Table 0 holds three of function 0, table 1 one of
/// function 1.
fn module() -> quench_runtime::WasmModule {
    module_returning(SEVEN)
}

/// `module` with function 0 returning `value`.
fn module_returning(value: i32) -> quench_runtime::WasmModule {
    Engine::lower_wasm_module_with_tables(
        "indirect-traps",
        [
            body(0, vec![Operator::I32Const { value }, Operator::End]),
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

fn call(vm: &mut Vm, instance: &WasmInstance, index: i32) -> Result<i32, WasmTrap> {
    match vm.invoke_wasm(instance, CALLER, &[WasmValue::I32(index)]) {
        Ok(Some(WasmValue::I32(value))) => Ok(value),
        Ok(other) => panic!("an i32 result, not {other:?}"),
        Err(error) => Err(error.wasm_trap().expect("a Wasm trap")),
    }
}

fn signatures(vm: &Vm, table: Value) -> Vec<CanonicalSignature> {
    match vm.heap.get(table) {
        Some(Cell::WasmTable { elements, .. }) => elements.targets().iter().map(|target| target.signature).collect(),
        _ => panic!("a table"),
    }
}

#[test]
fn each_check_of_an_indirect_call_traps_as_the_rule_orders_them() {
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
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
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
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

fn rows(vm: &Vm, table: Value) -> Vec<TableTarget> {
    match vm.heap.get(table) {
        Some(Cell::WasmTable { elements, .. }) => elements.targets().to_vec(),
        _ => panic!("a table"),
    }
}

#[test]
fn a_row_names_the_function_and_the_instance_of_its_element_through_every_write() {
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    let built = module();
    let instance = vm.instantiate_wasm_module(&built).unwrap();
    let environment = vm.root_value(instance.environment).unwrap();
    let program = vm.programs.find_shared(&built.program).expect("a registered program");
    let record = vm.wasm_instance_address(environment, program).expect("an instance record") as u64;
    let (table, other) = (
        vm.wasm_table(&instance, 0).unwrap(),
        vm.wasm_table(&instance, 1).unwrap(),
    );
    let (first, second_row) = (rows(&vm, table)[0], rows(&vm, other)[0]);
    let function = |entry: u32| TableTarget {
        signature: if entry == 0 { first.signature } else { second_row.signature },
        entry,
        instance: record,
        body: InstanceContext::published_body(record as usize, entry),
    };
    assert_eq!(rows(&vm, table), [function(0); 3]);

    let second = vm.wasm_table_get(other, 0).unwrap();
    vm.wasm_table_fill(table, 1, second, 1).unwrap();
    vm.wasm_table_fill(table, 2, Value::NULL, 1).unwrap();
    assert_eq!(rows(&vm, table), [function(0), function(1), TableTarget::NULL]);

    vm.wasm_table_grow(table, second, 1).unwrap();
    vm.wasm_table_copy(table, table, 0, 1, 2).unwrap();
    assert_eq!(
        rows(&vm, table),
        [function(1), TableTarget::NULL, TableTarget::NULL, function(1)]
    );
}

/// The results of calling each index in `indices` through the caller of `instance`.
fn calls(vm: &mut Vm, instance: &WasmInstance, indices: &[i32]) -> Vec<Result<i32, WasmTrap>> {
    indices.iter().map(|index| call(vm, instance, *index)).collect()
}

fn vm_running(compiled: bool) -> Vm {
    let mut vm = quench_runtime::test_support::vm(quench_runtime::SystemHost);
    if compiled {
        vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    }
    vm
}

/// Installs the compiled root of each function of `instance`'s module that has none yet. The
/// argmin of `tier_policy::predicted_tiers` decides first calls; these tests need the tier.
fn install_roots(vm: &mut Vm, instance: &WasmInstance) {
    let program = vm
        .programs
        .find_shared(&instance.module.program)
        .expect("registered Wasm program");
    let environment = vm.root_value(instance.environment).expect("instance environment");
    for function in 0..instance.module.program.function_count() {
        if vm.programs.wasm_tier(program, function as u32).is_none() {
            assert!(vm.install_compiled_root_for_test(program, environment, function));
        }
    }
}

/// Instantiates `module`, with its roots compiled when `compiled` (the VM runs compiled code).
fn instantiate_running(vm: &mut Vm, module: &quench_runtime::WasmModule, compiled: bool) -> WasmInstance {
    let instance = vm.instantiate_wasm_module(module).unwrap();
    if compiled {
        install_roots(vm, &instance);
    }
    instance
}

/// How many times the VM entered function `index` of `module`'s compiled code.
pub fn entries(vm: &Vm, module: &quench_runtime::WasmModule, index: usize) -> Option<u64> {
    let program = vm.programs.find_shared(&module.program)?;
    vm.programs.wasm_tier(program, u32::try_from(index).ok()?)?.native_entries(index)
}

#[test]
fn the_tier_calls_through_a_row_and_traps_in_the_rules_order() {
    let run = |compiled: bool| {
        let (mut vm, built) = (vm_running(compiled), module());
        let instance = instantiate_running(&mut vm, &built, compiled);
        let (table, other) = (
            vm.wasm_table(&instance, 0).unwrap(),
            vm.wasm_table(&instance, 1).unwrap(),
        );
        let other_function = vm.wasm_table_get(other, 0).unwrap();
        vm.wasm_table_grow(table, Value::NULL, 1).unwrap();
        vm.wasm_table_fill(table, 1, Value::NULL, 1).unwrap();
        vm.wasm_table_fill(table, 2, other_function, 1).unwrap();
        let results = calls(&mut vm, &instance, &[0, 1, 2, 3, 4, -1]);
        (results, entries(&vm, &built, 0))
    };
    let expected = vec![
        Ok(SEVEN),
        Err(WasmTrap::UninitializedElement),
        Err(WasmTrap::IndirectCallTypeMismatch),
        Err(WasmTrap::UninitializedElement),
        Err(WasmTrap::UndefinedElement),
        Err(WasmTrap::UndefinedElement),
    ];
    assert_eq!(run(false).0, expected);
    let (results, entered) = run(true);
    assert_eq!(results, expected);
    // The target is called through its row, not entered from the VM.
    assert_eq!(entered, Some(0));
}

#[test]
fn a_write_to_a_table_changes_the_target_of_the_next_call_in_the_tier() {
    let run = |compiled: bool| {
        let mut vm = vm_running(compiled);
        let (own, foreign) = (module(), module_returning(SEVEN + 2));
        let instance = instantiate_running(&mut vm, &own, compiled);
        let other = instantiate_running(&mut vm, &foreign, compiled);
        let table = vm.wasm_table(&instance, 0).unwrap();
        let (same_module, same_type, other_type) = (
            vm.wasm_function(&instance, 0).unwrap(),
            vm.wasm_function(&other, 0).unwrap(),
            vm.wasm_function(&other, 1).unwrap(),
        );
        let mut results = calls(&mut vm, &instance, &[0, 1]);
        // Another module's function of the same type, then of another type, then back.
        for (index, replacement) in [(0, same_type), (1, other_type), (0, same_module), (1, same_type)] {
            vm.wasm_table_fill(table, index, replacement, 1).unwrap();
            results.extend(calls(&mut vm, &instance, &[0, 1]));
        }
        results
    };
    let expected = vec![
        Ok(SEVEN), Ok(SEVEN),
        Ok(SEVEN + 2), Ok(SEVEN),
        Ok(SEVEN + 2), Err(WasmTrap::IndirectCallTypeMismatch),
        Ok(SEVEN), Err(WasmTrap::IndirectCallTypeMismatch),
        Ok(SEVEN), Ok(SEVEN + 2),
    ];
    assert_eq!(run(false), expected);
    assert_eq!(run(true), expected);
}

#[test]
fn another_instances_function_in_a_shared_table_is_called_natively_once_it_is_published() {
    let mut vm = vm_running(true);
    let (own, foreign) = (module(), module_returning(SEVEN + 2));
    let instance = instantiate_running(&mut vm, &own, true);
    let other = instantiate_running(&mut vm, &foreign, true);
    // The VM enters the other instance once, which publishes its code in its record.
    assert_eq!(
        vm.invoke_wasm(&other, 0, &[]).unwrap(),
        Some(WasmValue::I32(SEVEN + 2))
    );
    let table = vm.wasm_table(&instance, 0).unwrap();
    let function = vm.wasm_function(&other, 0).unwrap();
    vm.wasm_table_fill(table, 0, function, 1).unwrap();
    let (before, caller) = (entries(&vm, &foreign, 0), entries(&vm, &own, CALLER as usize));
    assert_eq!(before, Some(1));
    for _ in 0..3 {
        assert_eq!(call(&mut vm, &instance, 0), Ok(SEVEN + 2));
    }
    // The calls ran in the caller's code and switched to the other instance's: the VM entered
    // only the caller, and its callout never ran the target.
    assert_eq!(entries(&vm, &foreign, 0), before);
    // The caller's code is compiled at its first entry, so before the calls it has no count.
    assert_eq!(entries(&vm, &own, CALLER as usize), Some(caller.unwrap_or(0) + 3));
}

#[test]
fn a_row_names_the_record_its_function_keeps_alive() {
    let mut vm = vm_running(true);
    let (own, foreign) = (module(), module_returning(SEVEN + 2));
    let instance = instantiate_running(&mut vm, &own, true);
    let other = instantiate_running(&mut vm, &foreign, true);
    let table = vm.wasm_table(&instance, 0).unwrap();
    let function = vm.wasm_function(&other, 0).unwrap();
    vm.wasm_table_fill(table, 0, function, 1).unwrap();
    let environment = vm.root_value(other.environment).unwrap();
    let program = vm.programs.find_shared(&foreign.program).expect("a registered program");
    let record = vm.wasm_instance_address(environment, program).expect("a record");
    assert_eq!(rows(&vm, table)[0].instance, record as u64);
    assert!(vm.release_root(other.environment));
    vm.collect_now(own.residual());
    assert_eq!(call(&mut vm, &instance, 0), Ok(SEVEN + 2));
    assert_eq!(vm.wasm_instance_address(environment, program), Some(record));
}

/// Function 0 grows the memory by a page and returns its size in pages before; function 1
/// returns its size in pages. One memory of one page, at most four.
fn growing_module() -> quench_runtime::WasmModule {
    Engine::lower_wasm_module_with_tables(
        "grows",
        [
            body(
                0,
                vec![
                    Operator::I32Const { value: 1 },
                    Operator::MemoryGrow { mem: 0 },
                    Operator::End,
                ],
            ),
            body(0, vec![Operator::MemorySize { mem: 0 }, Operator::End]),
        ],
        &WasmTypes::from_functions([FuncType::new([], [ValType::I32])]),
        &[],
        &[quench_runtime::WasmMemory {
            ty: wasmparser::MemoryType {
                initial: 1,
                maximum: Some(4),
                memory64: false,
                shared: false,
                page_size_log2: None,
            },
            import: None,
        }],
        &[],
        &[],
    )
    .unwrap()
}

/// TOD-1790: a call into another instance reads the length its memory has now, not the one
/// it had when its code was last published, in the interpreter and in the tier alike.
#[test]
fn a_call_into_another_instance_sees_the_memory_it_grew() {
    for compiled in [false, true] {
        let mut vm = vm_running(compiled);
        let (own, foreign) = (module(), growing_module());
        let instance = instantiate_running(&mut vm, &own, compiled);
        let other = instantiate_running(&mut vm, &foreign, compiled);
        // The VM enters the other instance once, which publishes its code and memory.
        assert_eq!(vm.invoke_wasm(&other, 1, &[]).unwrap(), Some(WasmValue::I32(1)));
        let table = vm.wasm_table(&instance, 0).unwrap();
        let grow = vm.wasm_function(&other, 0).unwrap();
        let size = vm.wasm_function(&other, 1).unwrap();
        vm.wasm_table_fill(table, 0, grow, 1).unwrap();
        vm.wasm_table_fill(table, 1, size, 1).unwrap();
        for round in 0..3 {
            assert_eq!(call(&mut vm, &instance, 0), Ok(1 + round), "grow, compiled={compiled}");
            assert_eq!(call(&mut vm, &instance, 1), Ok(2 + round), "size, compiled={compiled}");
        }
        // The VM's own read agrees, and a failed growth changes nothing.
        assert_eq!(vm.invoke_wasm(&other, 1, &[]).unwrap(), Some(WasmValue::I32(4)));
        assert_eq!(call(&mut vm, &instance, 0), Ok(-1), "grow past the maximum, compiled={compiled}");
        assert_eq!(call(&mut vm, &instance, 1), Ok(4), "size at the maximum, compiled={compiled}");
    }
}

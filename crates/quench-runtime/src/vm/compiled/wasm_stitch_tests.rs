//! Stitched functions against the interpreter: the same results and the same traps.

use super::*;
use super::super::{CallContext, CodeMemory};
use crate::vm::Vm;
use crate::{Engine, WasmCallableType, WasmFunctionBody, WasmTypes, WasmValue};
use wasmparser::{BinaryReaderError, FuncType, Operator, ValType};

fn body(ty: u32, locals: Vec<ValType>, operators: Vec<Operator<'static>>) -> WasmFunctionBody<Vec<Result<Operator<'static>, BinaryReaderError>>> {
    WasmFunctionBody { ty: WasmCallableType::Declared(ty), locals, operators: operators.into_iter().map(Ok).collect() }
}

/// Function 0 sums `0..n` in a loop; function 1 divides signed; function 2 is `rotl(a * b, a)`
/// on i64; function 3 counts the set bits of `n` and tests it for zero.
fn module() -> crate::WasmModule {
    use Operator::*;
    let get = |local_index| LocalGet { local_index };
    let set = |local_index| LocalSet { local_index };
    Engine::lower_wasm_module_with_tables(
        "stitched",
        [
            body(0, vec![ValType::I32, ValType::I32], vec![
                Block { blockty: wasmparser::BlockType::Empty },
                Loop { blockty: wasmparser::BlockType::Empty },
                get(1), get(0), I32GeS, BrIf { relative_depth: 1 },
                get(2), get(1), I32Add, set(2),
                get(1), I32Const { value: 1 }, I32Add, set(1),
                Br { relative_depth: 0 },
                End, End,
                get(2), End,
            ]),
            body(1, vec![], vec![get(0), get(1), I32DivS, End]),
            body(2, vec![], vec![get(0), get(1), I64Mul, get(0), I64Rotl, End]),
            body(0, vec![], vec![get(0), I32Popcnt, I32Eqz, I32Eqz, I32Const { value: 100 }, I32Mul, End]),
        ],
        &WasmTypes::from_functions([
            FuncType::new([ValType::I32], [ValType::I32]),
            FuncType::new([ValType::I32, ValType::I32], [ValType::I32]),
            FuncType::new([ValType::I64, ValType::I64], [ValType::I64]),
        ]),
        &[],
        &[],
        &[],
        &[],
    )
    .unwrap()
}

type Entry = unsafe extern "C" fn(*mut CallContext, u64, u64, *const u64, *mut u64) -> u32;

/// Function `index` of `module` stitched and placed, as its entry, with its code kept alive.
fn stitched(vm: &Vm<crate::SystemHost>, module: &crate::WasmModule, index: usize) -> (CodeMemory, Entry) {
    // A module without globals binds its memory at slot 0.
    let memory_slot = (!module.memories.is_empty()).then_some(module.globals.len() as u32);
    let program = vm.programs.find_shared(&module.program).expect("an instantiated module");
    let code = vm.programs.get(program).expect("its program");
    let constant = |at: usize| {
        let value = vm.programs.constant(program, at)?;
        Some(vm.decode_bits64(value).unwrap_or(u64::from(value.wasm_bits32())))
    };
    let function = code.function(index);
    let params = function.params as usize;
    let bytes = stitch(function, params, true, &constant, memory_slot).expect("rows for every instruction");
    let memory = CodeMemory::new(&bytes).expect("code memory");
    // SAFETY: the code begins with the entry routine, which takes the `WasmEntry` ABI.
    let entry = unsafe { std::mem::transmute::<*const u8, Entry>(memory.pointer()) };
    (memory, entry)
}

fn run(entry: Entry, arguments: &[u64]) -> Result<u64, u32> {
    run_over(entry, &mut [], arguments)
}

/// `run` over `memory`, which the entry passes the rows as the memory's base and length.
fn run_over(entry: Entry, memory: &mut [u8], arguments: &[u64]) -> Result<u64, u32> {
    let mut result = 0;
    // SAFETY: the rows read their frame and `memory` only, inside its length.
    let status = unsafe { entry(std::ptr::null_mut(), memory.as_mut_ptr() as u64, memory.len() as u64, arguments.as_ptr(), &mut result) };
    if status == 0 { Ok(result) } else { Err(status) }
}

fn interpreted(vm: &mut Vm<crate::SystemHost>, instance: &crate::WasmInstance, index: u32, arguments: &[WasmValue]) -> Result<u64, u32> {
    match vm.invoke_wasm(instance, index, arguments) {
        Ok(Some(value)) => Ok(value.word()),
        Ok(None) => Ok(0),
        Err(error) => Err(error.wasm_trap().and_then(crate::WasmTrap::compiled_status).expect("a compiled trap")),
    }
}

#[test]
fn a_stitched_function_computes_and_traps_as_the_interpreter() {
    let module = module();
    let mut vm = Vm::new(crate::SystemHost);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    let (_sum_code, sum) = stitched(&vm, &module, 0);
    for n in [0, 1, 10, 1000, -5] {
        assert_eq!(run(sum, &[u64::from(n as u32)]), interpreted(&mut vm, &instance, 0, &[WasmValue::I32(n)]), "sum {n}");
    }
    let (_divide_code, divide) = stitched(&vm, &module, 1);
    for (a, b) in [(7, 2), (-7, 2), (1, 0), (i32::MIN, -1), (i32::MIN, 1)] {
        let words = [u64::from(a as u32), u64::from(b as u32)];
        assert_eq!(run(divide, &words), interpreted(&mut vm, &instance, 1, &[WasmValue::I32(a), WasmValue::I32(b)]), "{a} / {b}");
    }
    let (_wide_code, wide) = stitched(&vm, &module, 2);
    for (a, b) in [(3_i64, 5_i64), (-1, i64::MAX), (0x1234_5678_9abc, 77)] {
        let words = [a as u64, b as u64];
        assert_eq!(run(wide, &words), interpreted(&mut vm, &instance, 2, &[WasmValue::I64(a), WasmValue::I64(b)]), "{a} {b}");
    }
    let (_bits_code, bits) = stitched(&vm, &module, 3);
    for n in [0, 1, -1, 0x100] {
        assert_eq!(run(bits, &[u64::from(n as u32)]), interpreted(&mut vm, &instance, 3, &[WasmValue::I32(n)]), "bits {n}");
    }
}

/// Function 0 stores an i64 at `addr + 8` and loads its low 16 bits back sign-extended;
/// function 1 loads the byte at `addr`; one page of memory.
fn memory_module() -> crate::WasmModule {
    use Operator::*;
    let get = |local_index| LocalGet { local_index };
    let at = |offset, align| wasmparser::MemArg { align, max_align: align, offset, memory: 0 };
    Engine::lower_wasm_module_with_tables(
        "stitched-memory",
        [
            body(0, vec![], vec![get(0), get(1), I64Store { memarg: at(8, 3) }, get(0), I64Load16S { memarg: at(8, 1) }, End]),
            body(1, vec![], vec![get(0), I32Load8U { memarg: at(0, 0) }, End]),
        ],
        &WasmTypes::from_functions([
            FuncType::new([ValType::I32, ValType::I64], [ValType::I64]),
            FuncType::new([ValType::I32], [ValType::I32]),
        ]),
        &[],
        &[crate::WasmMemory { ty: wasmparser::MemoryType { memory64: false, shared: false, initial: 1, maximum: None, page_size_log2: None }, import: None }],
        &[],
        &[],
    )
    .unwrap()
}

#[test]
fn stitched_loads_and_stores_check_and_move_as_the_interpreter() {
    let module = memory_module();
    let mut vm = Vm::new(crate::SystemHost);
    let instance = vm.instantiate_wasm_module(&module).unwrap();
    let mut memory = vec![0u8; 1 << 16];
    let (_store_code, store) = stitched(&vm, &module, 0);
    for (address, value) in [(0_i32, 0x1234_8765_i64), (100, -2), (65520, 0x7fff), (65521, 1), (-1, 5)] {
        let words = [u64::from(address as u32), value as u64];
        let expected = interpreted(&mut vm, &instance, 0, &[WasmValue::I32(address), WasmValue::I64(value)]);
        assert_eq!(run_over(store, &mut memory, &words), expected, "store/load at {address}");
    }
    let (_byte_code, byte) = stitched(&vm, &module, 1);
    for address in [8_i32, 9, 65535, 65536, -8] {
        let expected = interpreted(&mut vm, &instance, 1, &[WasmValue::I32(address)]);
        assert_eq!(run_over(byte, &mut memory, &[u64::from(address as u32)]), expected, "byte at {address}");
    }
}

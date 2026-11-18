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
    let program = vm.programs.find_shared(&module.program).expect("an instantiated module");
    let code = vm.programs.get(program).expect("its program");
    let constant = |at: usize| {
        let value = vm.programs.constant(program, at)?;
        Some(vm.decode_bits64(value).unwrap_or(u64::from(value.wasm_bits32())))
    };
    let function = code.function(index);
    let params = function.params as usize;
    let bytes = stitch(function, params, true, &constant).expect("rows for every instruction");
    let memory = CodeMemory::new(&bytes).expect("code memory");
    // SAFETY: the code begins with the entry routine, which takes the `WasmEntry` ABI.
    let entry = unsafe { std::mem::transmute::<*const u8, Entry>(memory.pointer()) };
    (memory, entry)
}

fn run(entry: Entry, arguments: &[u64]) -> Result<u64, u32> {
    let mut result = 0;
    // SAFETY: the rows read their frame only; the entry takes its arguments and result pointer.
    let status = unsafe { entry(std::ptr::null_mut(), 0, 0, arguments.as_ptr(), &mut result) };
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

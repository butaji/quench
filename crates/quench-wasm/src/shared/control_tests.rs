use super::*;
use super::tests::TestHost;
use quench_runtime::{Runtime, WasmTrap};

fn lower(body: &str) -> WasmI32Function {
    crate::Engine::new()
        .compile_wat(body)
        .unwrap()
        .lower_shared_i32("f")
        .unwrap()
}

#[test]
fn return_and_trap_keep_unreachable_stacks_polymorphic() {
    let function = lower(
        r#"(module (func (export "f") (param i32) (result i32)
        local.get 0 if i32.const 77 return else unreachable end
        unreachable i32.add drop))"#,
    );
    let mut runtime = Runtime::new(TestHost);
    assert_eq!(runtime.execute_wasm_i32(&function, &[1]).unwrap(), Some(77));
    assert_eq!(
        runtime
            .execute_wasm_i32(&function, &[0])
            .unwrap_err()
            .wasm_trap(),
        Some(WasmTrap::Unreachable)
    );
    runtime.collect(function.residual()).unwrap();
    assert_eq!(
        runtime.execute_wasm_i32(&function, &[-1]).unwrap(),
        Some(77)
    );
}

#[test]
fn large_if_patches_jumps_through_shared_wide_encoding() {
    let body = "i32.const 1 drop ".repeat(20_000);
    let function = lower(&format!(
        r#"(module (func (export "f") (param i32) (result i32)
        local.get 0 if (result i32) {body} i32.const 7 else i32.const 9 end))"#
    ));
    let mut runtime = Runtime::new(TestHost);
    assert_eq!(runtime.execute_wasm_i32(&function, &[0]).unwrap(), Some(9));
    assert_eq!(runtime.execute_wasm_i32(&function, &[1]).unwrap(), Some(7));
    runtime.collect(function.residual()).unwrap();
}

/// A state machine whose cases set the next state and loop back to a
/// dispatch on it, as compiled code threads: the result matches a direct
/// model in both execution modes, including states decided before the
/// table (state 3) and states the table sends to its default.
#[test]
fn state_machine_dispatch_matches_model_in_both_modes() {
    const MACHINE: &str = r#"(module
      (func $machine (param $n i32) (result i32)
        (local $state i32) (local $i i32) (local $acc i32) (local $symbol i32)
        (loop $top
          (local.set $symbol (i32.rem_u
            (i32.add (i32.mul (local.get $i) (i32.const 7)) (local.get $n)) (i32.const 5)))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (if (i32.eq (local.get $state) (i32.const 3))
            (then (local.set $acc (i32.add (local.get $acc) (i32.const 1000)))))
          (block $next
            (block $other (block $one (block $zero
              (br_table $zero $one $other (local.get $state)))
              (local.set $acc (i32.add (local.get $acc) (local.get $symbol)))
              (if (i32.lt_u (local.get $symbol) (i32.const 2))
                (then (local.set $state (i32.const 1)) (br $next)))
              (local.set $state (i32.const 2))
              (br $next))
             (local.set $acc (i32.xor (local.get $acc)
               (i32.shl (local.get $symbol) (i32.const 3))))
             (if (i32.eq (local.get $symbol) (i32.const 4))
               (then (local.set $state (i32.const 3)) (br $next)))
             (local.set $state (i32.const 0))
             (br $next))
            (local.set $acc (i32.sub (local.get $acc) (local.get $state)))
            (local.set $state (select (i32.const 0) (i32.const 1)
              (i32.and (local.get $symbol) (i32.const 1)))))
          (br_if $top (i32.lt_u (local.get $i) (local.get $n))))
        (i32.add (local.get $acc) (i32.mul (local.get $state) (i32.const 100000))))
      (func (export "f") (param i32) (result i32) (call $machine (local.get 0))))"#;
    fn model(n: i32) -> i32 {
        let (mut state, mut i, mut acc) = (0_i32, 0_i32, 0_i32);
        loop {
            let symbol = ((i.wrapping_mul(7).wrapping_add(n)) as u32 % 5) as i32;
            i = i.wrapping_add(1);
            if state == 3 {
                acc = acc.wrapping_add(1000);
            }
            match state {
                0 => {
                    acc = acc.wrapping_add(symbol);
                    state = if (symbol as u32) < 2 { 1 } else { 2 };
                }
                1 => {
                    acc ^= symbol << 3;
                    state = if symbol == 4 { 3 } else { 0 };
                }
                _ => {
                    acc = acc.wrapping_sub(state);
                    state = if symbol & 1 != 0 { 0 } else { 1 };
                }
            }
            if (i as u32) >= (n as u32) {
                break;
            }
        }
        acc.wrapping_add(state.wrapping_mul(100_000))
    }
    let engine = crate::Engine::new().compile_wat(MACHINE).unwrap();
    for mode in [
        quench_runtime::ExecutionMode::Interpreted,
        quench_runtime::ExecutionMode::Compiled,
    ] {
        let module = engine.lower_shared_module_for(mode).unwrap();
        let mut runtime = Runtime::new(TestHost);
        runtime.set_execution_mode(mode);
        let instance = runtime.instantiate_wasm_module(&module).unwrap();
        for n in [1, 2, 3, 7, 64, 1000, 4097] {
            assert_eq!(
                runtime
                    .invoke_wasm(&instance, 1, &[quench_runtime::WasmValue::I32(n)])
                    .unwrap(),
                Some(quench_runtime::WasmValue::I32(model(n))),
                "{mode:?} n={n}"
            );
        }
    }
}

/// A bytecode interpreter in the shape C compilers give `switch (*pc++)`:
/// the opcode load feeds the table, handlers copy stack slots through
/// memory, and handlers leave through shared tails that step registers
/// before jumping back. Its result matches a direct model of the same
/// program in both execution modes.
#[test]
fn bytecode_interpreter_matches_model_in_both_modes() {
    const INTERPRETER: &str = r#"(module
      (memory 1)
      (func $interpret (result i32)
        (local $pc i32) (local $sp i32) (local $next i32) (local $top i32)
        (local.set $sp (i32.const 2048))
        (loop $dispatch
          (block $advance (block $halt (block $jnz (block $subi (block $add
          (block $mul (block $dup (block $store (block $load (block $push
            (local.set $next (i32.add (local.get $pc) (i32.const 4)))
            (br_table $push $load $store $dup $mul $add $subi $jnz $halt $halt
              (i32.load (local.get $pc))))
            (i32.store (local.get $sp) (i32.load offset=4 (local.get $pc)))
            (br $advance))
           (i32.store (local.get $sp) (i32.load offset=1024
             (i32.shl (i32.load offset=4 (local.get $pc)) (i32.const 2))))
           (br $advance))
          (local.set $sp (i32.add (local.get $sp) (i32.const -4)))
          (i32.store offset=1024 (i32.shl (i32.load offset=4 (local.get $pc)) (i32.const 2))
            (i32.load (local.get $sp)))
          (local.set $pc (i32.add (local.get $pc) (i32.const 8)))
          (br $dispatch))
         (i32.store (local.get $sp) (i32.load (i32.add (local.get $sp) (i32.const -4))))
         (local.set $sp (i32.add (local.get $sp) (i32.const 4)))
         (local.set $pc (local.get $next))
         (br $dispatch))
        (local.set $sp (i32.add (local.get $sp) (i32.const -4)))
        (local.set $top (i32.add (local.get $sp) (i32.const -4)))
        (i32.store (local.get $top)
          (i32.mul (i32.load (local.get $top)) (i32.load (local.get $sp))))
        (local.set $pc (local.get $next))
        (br $dispatch))
       (local.set $sp (i32.add (local.get $sp) (i32.const -4)))
       (local.set $top (i32.add (local.get $sp) (i32.const -4)))
       (i32.store (local.get $top)
         (i32.add (i32.load (local.get $top)) (i32.load (local.get $sp))))
       (local.set $pc (local.get $next))
       (br $dispatch))
      (local.set $top (i32.add (local.get $sp) (i32.const -4)))
      (i32.store (local.get $top)
        (i32.sub (i32.load (local.get $top)) (i32.load offset=4 (local.get $pc))))
      (local.set $pc (i32.add (local.get $pc) (i32.const 8)))
      (br $dispatch))
     (local.set $sp (i32.add (local.get $sp) (i32.const -4)))
     (local.set $pc (select (i32.load offset=4 (local.get $pc))
       (i32.add (local.get $pc) (i32.const 8)) (i32.load (local.get $sp))))
     (br $dispatch))
    (return (i32.load (i32.add (local.get $sp) (i32.const -4)))))
          (local.set $sp (i32.add (local.get $sp) (i32.const 4)))
          (local.set $pc (i32.add (local.get $pc) (i32.const 8)))
          (br $dispatch))
        unreachable)
      (func (export "f") (param $n i32) (result i32)
        (local $at i32)
        (loop $copy
          (i32.store (local.get $at) (i32.load offset=512 (local.get $at)))
          (local.set $at (i32.add (local.get $at) (i32.const 4)))
          (br_if $copy (i32.lt_u (local.get $at) (i32.const 512))))
        (i32.store offset=20 (i32.const 0) (local.get $n))
        (call $interpret))
      (data (i32.const 512) "PROGRAM"))"#;
    const PUSH: i32 = 0;
    const LOAD: i32 = 1;
    const STORE: i32 = 2;
    const DUP: i32 = 3;
    const MUL: i32 = 4;
    const ADD: i32 = 5;
    const SUBI: i32 = 6;
    const JNZ: i32 = 7;
    const HALT: i32 = 8;
    const LOOP_HEAD: i32 = 32;
    // acc = 0; k = n; do { acc += k * k; k -= 1 } while k != 0; acc
    const PROGRAM: [i32; 29] = [
        PUSH, 0, STORE, 0, PUSH, 0, STORE, 1, // n replaces the second push
        LOAD, 0, LOAD, 1, DUP, MUL, ADD, STORE, 0, LOAD, 1, SUBI, 1, DUP, STORE, 1, JNZ,
        LOOP_HEAD, LOAD, 0, HALT,
    ];
    fn model(n: i32) -> i32 {
        let mut program = PROGRAM;
        program[5] = n;
        let (mut pc, mut stack, mut locals) = (0_usize, Vec::new(), [0_i32; 2]);
        let word = |pc: usize| program[pc];
        loop {
            let operand = || program[pc + 1];
            pc = match word(pc) {
                PUSH => {
                    stack.push(operand());
                    pc + 2
                }
                LOAD => {
                    stack.push(locals[operand() as usize]);
                    pc + 2
                }
                STORE => {
                    locals[operand() as usize] = stack.pop().unwrap();
                    pc + 2
                }
                DUP => {
                    stack.push(*stack.last().unwrap());
                    pc + 1
                }
                MUL | ADD => {
                    let right = stack.pop().unwrap();
                    let left = stack.pop().unwrap();
                    stack.push(if word(pc) == MUL {
                        left.wrapping_mul(right)
                    } else {
                        left.wrapping_add(right)
                    });
                    pc + 1
                }
                SUBI => {
                    let top = stack.pop().unwrap();
                    stack.push(top.wrapping_sub(operand()));
                    pc + 2
                }
                JNZ => match stack.pop().unwrap() {
                    0 => pc + 2,
                    _ => operand() as usize / 4,
                },
                _ => return *stack.last().unwrap(),
            };
        }
    }
    let program: String = PROGRAM
        .iter()
        .flat_map(|word| word.to_le_bytes())
        .map(|byte| format!("\\{byte:02x}"))
        .collect();
    let engine = crate::Engine::new()
        .compile_wat(&INTERPRETER.replace("PROGRAM", &program))
        .unwrap();
    for mode in [
        quench_runtime::ExecutionMode::Interpreted,
        quench_runtime::ExecutionMode::Compiled,
    ] {
        let module = engine.lower_shared_module_for(mode).unwrap();
        let mut runtime = Runtime::new(TestHost);
        runtime.set_execution_mode(mode);
        let instance = runtime.instantiate_wasm_module(&module).unwrap();
        for n in [1, 2, 3, 10, 1000] {
            assert_eq!(
                runtime
                    .invoke_wasm(&instance, 1, &[quench_runtime::WasmValue::I32(n)])
                    .unwrap(),
                Some(quench_runtime::WasmValue::I32(model(n))),
                "{mode:?} n={n}"
            );
        }
    }
}

/// f64 accumulations through memory (`mem[i] += a * b`, `mem[i] = x -
/// mem[i]`) and an f64 result stored right after it is computed: the
/// results match a direct model in both execution modes.
#[test]
fn f64_memory_updates_match_model_in_both_modes() {
    const UPDATES: &str = r#"(module
      (memory 1)
      (func (export "f") (param $n i32) (result f64)
        (local $i i32) (local $k i32) (local $x f64) (local $sum f64)
        (loop $fill
          (f64.store (i32.shl (local.get $i) (i32.const 3))
            (f64.convert_i32_s (i32.add (local.get $i) (i32.const 1))))
          (f64.store offset=64 (i32.shl (local.get $i) (i32.const 3)) (f64.const 0))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br_if $fill (i32.lt_u (local.get $i) (i32.const 8))))
        (loop $round
          (local.set $i (i32.const 0))
          (loop $cell
            (local.set $x (f64.convert_i32_s (i32.add (local.get $i) (local.get $k))))
            (f64.store (i32.shl (local.get $i) (i32.const 3))
              (f64.add (f64.load (i32.shl (local.get $i) (i32.const 3)))
                (f64.mul (local.get $x) (f64.const 0.5))))
            (f64.store offset=64 (i32.shl (local.get $i) (i32.const 3))
              (f64.sub (local.get $x)
                (f64.load offset=64 (i32.shl (local.get $i) (i32.const 3)))))
            (f64.store offset=128 (i32.shl (local.get $i) (i32.const 3))
              (f64.div (local.get $x) (f64.add (local.get $x) (f64.const 1))))
            (local.set $i (i32.add (local.get $i) (i32.const 1)))
            (br_if $cell (i32.lt_u (local.get $i) (i32.const 8))))
          (local.set $k (i32.add (local.get $k) (i32.const 1)))
          (br_if $round (i32.lt_u (local.get $k) (local.get $n))))
        (local.set $i (i32.const 0))
        (loop $total
          (local.set $sum (f64.add (local.get $sum)
            (f64.add (f64.load (i32.shl (local.get $i) (i32.const 3)))
              (f64.add (f64.load offset=64 (i32.shl (local.get $i) (i32.const 3)))
                (f64.load offset=128 (i32.shl (local.get $i) (i32.const 3)))))))
          (local.set $i (i32.add (local.get $i) (i32.const 1)))
          (br_if $total (i32.lt_u (local.get $i) (i32.const 8))))
        (local.get $sum)))"#;
    fn model(n: i32) -> f64 {
        let mut a: [f64; 8] = std::array::from_fn(|i| (i + 1) as f64);
        let mut b = [0.0_f64; 8];
        let mut c = [0.0_f64; 8];
        let mut k = 0;
        loop {
            for i in 0..8 {
                let x = f64::from(i as i32 + k);
                a[i] += x * 0.5;
                b[i] = x - b[i];
                c[i] = x / (x + 1.0);
            }
            k += 1;
            if (k as u32) >= (n as u32) {
                break;
            }
        }
        (0..8).fold(0.0, |sum, i| sum + (a[i] + (b[i] + c[i])))
    }
    let engine = crate::Engine::new().compile_wat(UPDATES).unwrap();
    for mode in [
        quench_runtime::ExecutionMode::Interpreted,
        quench_runtime::ExecutionMode::Compiled,
    ] {
        let module = engine.lower_shared_module_for(mode).unwrap();
        let mut runtime = Runtime::new(TestHost);
        runtime.set_execution_mode(mode);
        let instance = runtime.instantiate_wasm_module(&module).unwrap();
        for n in [1, 2, 7, 100] {
            assert_eq!(
                runtime
                    .invoke_wasm(&instance, 0, &[quench_runtime::WasmValue::I32(n)])
                    .unwrap(),
                Some(quench_runtime::WasmValue::F64(model(n).to_bits())),
                "{mode:?} n={n}"
            );
        }
    }
}

/// Addresses formed by an i32 add wrap modulo 2^32 (a biased index such
/// as `-4 + 8` reaches byte 4), an index whose static offset carries it
/// past 2^32 traps, and so does a wrapped sum past the memory: the same in
/// both execution modes.
#[test]
fn wrapping_addresses_match_wasm_semantics_in_both_modes() {
    const WRAPS: &str = r#"(module
      (memory 1)
      (data (i32.const 0) "\00\01\02\03\04\05\06\07\08\09")
      (func (export "sum") (param $x i32) (param $c i32) (result i32)
        (i32.load8_u (i32.add (local.get $x) (local.get $c))))
      (func (export "biased") (param $x i32) (result i32)
        (i32.load8_u (i32.add (local.get $x) (i32.const 8))))
      (func (export "offset") (param $x i32) (result i32)
        (i32.load8_u offset=4 (local.get $x)))
      (func (export "store") (param $x i32) (result i32)
        (i32.store8 (i32.add (local.get $x) (i32.const 9)) (i32.const 42))
        (i32.load8_u (i32.const 5))))"#;
    use quench_runtime::WasmValue::I32;
    let engine = crate::Engine::new().compile_wat(WRAPS).unwrap();
    for mode in [
        quench_runtime::ExecutionMode::Interpreted,
        quench_runtime::ExecutionMode::Compiled,
    ] {
        let module = engine.lower_shared_module_for(mode).unwrap();
        let mut runtime = Runtime::new(TestHost);
        runtime.set_execution_mode(mode);
        let instance = runtime.instantiate_wasm_module(&module).unwrap();
        let mut call = |index: u32, args: &[quench_runtime::WasmValue]| {
            runtime
                .invoke_wasm(&instance, index, args)
                .map_err(|error| error.wasm_trap())
        };
        assert_eq!(call(0, &[I32(-4), I32(8)]), Ok(Some(I32(4))), "{mode:?}");
        assert_eq!(call(0, &[I32(-1), I32(-1)]), Err(Some(WasmTrap::OutOfBoundsMemory)), "{mode:?}");
        assert_eq!(call(1, &[I32(-4)]), Ok(Some(I32(4))), "{mode:?}");
        assert_eq!(call(1, &[I32(-9)]), Err(Some(WasmTrap::OutOfBoundsMemory)), "{mode:?}");
        assert_eq!(call(2, &[I32(3)]), Ok(Some(I32(7))), "{mode:?}");
        assert_eq!(call(2, &[I32(-2)]), Err(Some(WasmTrap::OutOfBoundsMemory)), "{mode:?}");
        assert_eq!(call(3, &[I32(-4)]), Ok(Some(I32(42))), "{mode:?}");
    }
}

/// Unbounded recursion through `call_indirect` traps as exhausted in both execution
/// modes: a compiled call through the indirect-call callout keeps its caller's stack
/// limit instead of a fresh budget per level, so it cannot overflow the native stack.
#[test]
fn runaway_call_indirect_exhausts_the_stack_in_both_modes() {
    const RUNAWAY: &str = r#"(module
      (type $t (func))
      (table funcref (elem $runaway))
      (func $runaway (export "runaway") (call_indirect (type $t) (i32.const 0))))"#;
    std::thread::Builder::new()
        .stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(|| {
            let engine = crate::Engine::new().compile_wat(RUNAWAY).unwrap();
            for mode in [
                quench_runtime::ExecutionMode::Interpreted,
                quench_runtime::ExecutionMode::Compiled,
            ] {
                let module = engine.lower_shared_module_for(mode).unwrap();
                let mut runtime = Runtime::new(TestHost);
                runtime.set_execution_mode(mode);
                let instance = runtime.instantiate_wasm_module(&module).unwrap();
                let trap = runtime
                    .invoke_wasm(&instance, 0, &[])
                    .map_err(|error| error.wasm_trap());
                assert_eq!(trap, Err(Some(WasmTrap::CallStackExhausted)), "{mode:?}");
            }
        })
        .unwrap()
        .join()
        .unwrap();
}

/// A full vector access traps before any partial store or subsequent effect,
/// including when the loaded value is unused. Growth retains the same semantics.
#[test]
fn vector_memory_boundaries_preserve_traps_effects_and_growth() {
    use quench_runtime::{ExecutionMode, WasmValue::I32};
    for offset in [0u32, 1, 17, u32::MAX] {
        let source = format!(r#"(module
          (memory 1 2)
          (func (export "load") (param i32) (result i32)
            local.get 0 v128.load offset={offset} align=1 i32x4.extract_lane 0)
          (func (export "drop") (param i32) (result i32)
            i32.const 0 i32.const 33 i32.store8
            local.get 0 v128.load offset={offset} align=1 drop
            i32.const 0 i32.const 44 i32.store8 i32.const 7)
          (func (export "store") (param i32)
            local.get 0 v128.const i32x4 0x5a5a5a5a 0x5a5a5a5a 0x5a5a5a5a 0x5a5a5a5a
            v128.store offset={offset} align=1)
          (func (export "byte") (param i32) (result i32) local.get 0 i32.load8_u)
          (func (export "fill") i32.const 65504 i32.const 102 i32.const 32 memory.fill)
          (func $grow (export "grow") (result i32) i32.const 1 memory.grow)
          (func (export "grow_load") (param i32) (result i32)
            call $grow drop local.get 0 v128.load offset={offset} align=1 i32x4.extract_lane 0)
        )"#);
        let engine = crate::Engine::new().compile_wat(&source).unwrap();
        for mode in [ExecutionMode::Interpreted, ExecutionMode::Compiled] {
            let module = engine.lower_shared_module_for(mode).unwrap();
            let mut runtime = Runtime::new(TestHost);
            runtime.set_execution_mode(mode);
            let instance = runtime.instantiate_wasm_module(&module).unwrap();
            let mut call = |index, args: &[quench_runtime::WasmValue]| {
                runtime.invoke_wasm(&instance, index, args)
                    .map_err(|error| error.wasm_trap())
            };
            if offset < 65520 {
                for address in [0, 1, 15, 31, 65520 - offset] {
                    assert_eq!(call(2, &[I32(address as i32)]), Ok(None), "{mode:?} offset={offset}");
                    assert_eq!(call(0, &[I32(address as i32)]), Ok(Some(I32(0x5a5a5a5a))));
                }
                assert_eq!(call(1, &[I32(32)]), Ok(Some(I32(7))));
                assert_eq!(call(3, &[I32(0)]), Ok(Some(I32(44))));
            }
            for suffix in 0..16 {
                let address = 65536 - suffix;
                assert_eq!(call(4, &[]), Ok(None));
                for index in [0, 1, 2] {
                    assert_eq!(call(index, &[I32(address)]), Err(Some(WasmTrap::OutOfBoundsMemory)),
                        "{mode:?} offset={offset} suffix={suffix} function={index}");
                }
                assert_eq!(call(3, &[I32(0)]), Ok(Some(I32(33))));
                for address in 65504..65536 {
                    assert_eq!(call(3, &[I32(address)]), Ok(Some(I32(102))), "trapping store changed prefix");
                }
            }
            for index in [0, 1, 2] {
                assert_eq!(call(index, &[I32(-1)]), Err(Some(WasmTrap::OutOfBoundsMemory)));
            }
            if offset < 65520 {
                assert_eq!(call(6, &[I32(65536)]), Ok(Some(I32(0))));
                assert_eq!(call(2, &[I32(65536)]), Ok(None));
                assert_eq!(call(0, &[I32(65536)]), Ok(Some(I32(0x5a5a5a5a))));
            } else {
                assert_eq!(call(5, &[]), Ok(Some(I32(1))));
                assert_eq!(call(0, &[I32(1)]), Err(Some(WasmTrap::OutOfBoundsMemory)));
            }
        }
    }
}

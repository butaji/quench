use super::*;
use quench_runtime::{Host, Runtime, WasmTrap};

struct TestHost;
impl Host for TestHost {
    fn write_line(&mut self, _: &str) {}
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

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

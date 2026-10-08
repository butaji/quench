use super::*;
use quench_runtime_next::{Host, Runtime, WasmTrap};

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

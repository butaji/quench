use super::*;
use rqj::{Host, Runtime, WasmTrap};

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
fn if_results_and_select_use_i32_truth() {
    let conditional = lower(
        r#"(module (func (export "f") (param i32) (result i32)
        local.get 0 if (result i32) i32.const 10 else i32.const 20 end))"#,
    );
    let select = lower(
        r#"(module (func (export "f") (param i32) (result i32)
        i32.const 10 i32.const 20 local.get 0 select))"#,
    );
    let mut runtime = Runtime::new(TestHost);
    for condition in [0, 1, -1, i32::MIN] {
        let expected = Some(if condition == 0 { 20 } else { 10 });
        assert_eq!(
            runtime
                .execute_wasm_i32(&conditional, &[condition])
                .unwrap(),
            expected
        );
        assert_eq!(
            runtime.execute_wasm_i32(&select, &[condition]).unwrap(),
            expected
        );
    }
}

#[test]
fn conditional_branch_preserves_prefix_and_result_on_fallthrough() {
    let function = lower(
        r#"(module (func (export "f") (param i32) (result i32)
        block (result i32)
          i32.const 11
          block (result i32)
            i32.const 22 i32.const 33 local.get 0 br_if 1 i32.add
          end
          i32.add
        end))"#,
    );
    let mut runtime = Runtime::new(TestHost);
    assert_eq!(runtime.execute_wasm_i32(&function, &[0]).unwrap(), Some(66));
    assert_eq!(runtime.execute_wasm_i32(&function, &[1]).unwrap(), Some(33));
}

#[test]
fn outer_branches_discard_intermediate_operand_stacks() {
    let function = lower(
        r#"(module (func (export "f") (result i32)
        block (result i32)
          i32.const 11
          block (result i32) i32.const 22 i32.const 33 br 1 end
          i32.add
        end))"#,
    );
    let root = lower(
        r#"(module (func (export "f") (result i32)
        block block i32.const 42 br 2 end end unreachable))"#,
    );
    let mut runtime = Runtime::new(TestHost);
    assert_eq!(runtime.execute_wasm_i32(&function, &[]).unwrap(), Some(33));
    assert_eq!(runtime.execute_wasm_i32(&root, &[]).unwrap(), Some(42));
}

#[test]
fn loop_backedges_and_outer_exits_share_jump_dispatch() {
    let function = lower(
        r#"(module (func (export "f") (param i32) (result i32) (local i32)
        block
          loop
            local.get 0 i32.eqz br_if 1
            local.get 1 local.get 0 i32.add local.set 1
            local.get 0 i32.const 1 i32.sub local.set 0
            br 0
          end
        end
        local.get 1))"#,
    );
    let mut runtime = Runtime::new(TestHost);
    for (count, expected) in [(0, 0), (1, 1), (10, 55), (100, 5050)] {
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[count]).unwrap(),
            Some(expected)
        );
    }
}

#[test]
fn branch_table_resolves_duplicate_labels_and_unsigned_default() {
    let function = lower(
        r#"(module (func (export "f") (param i32) (result i32)
        block $out (result i32)
          block $a
            block $b
              block $c local.get 0 br_table $c $b $c $a end
              i32.const 10 br $out
            end
            i32.const 20 br $out
          end
          i32.const 30
        end))"#,
    );
    let mut runtime = Runtime::new(TestHost);
    for (index, expected) in [(0, 10), (1, 20), (2, 10), (3, 30), (-1, 30), (i32::MIN, 30)] {
        assert_eq!(
            runtime.execute_wasm_i32(&function, &[index]).unwrap(),
            Some(expected)
        );
    }
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
fn result_if_can_join_a_trapping_arm() {
    let function = lower(
        r#"(module (func (export "f") (param i32) (result i32)
        local.get 0 if (result i32) unreachable else i32.const 19 end))"#,
    );
    let mut runtime = Runtime::new(TestHost);
    assert_eq!(runtime.execute_wasm_i32(&function, &[0]).unwrap(), Some(19));
    assert_eq!(
        runtime
            .execute_wasm_i32(&function, &[1])
            .unwrap_err()
            .wasm_trap(),
        Some(WasmTrap::Unreachable)
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

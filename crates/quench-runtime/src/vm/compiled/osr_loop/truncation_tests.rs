//! Truncated products: a product only ToInt32 reads wraps in `i32` exactly when the Number
//! product is exact (`wraps_exactly`); any other keeps its Number.
use super::{EXACT_PRODUCT, wraps_exactly};
use crate::test_host::TestHost;
use crate::vm::{Vm, operations::NumberArithmetic};
use crate::Engine;
use std::cell::RefCell;
use std::rc::Rc;

#[test]
fn only_exact_products_wrap() {
    assert!(wraps_exactly(NumberArithmetic::Add, None));
    assert!(wraps_exactly(NumberArithmetic::Multiply, Some((-(1 << 31) * 31, ((1 << 31) - 1) * 31))));
    assert!(wraps_exactly(NumberArithmetic::Multiply, Some((-EXACT_PRODUCT, EXACT_PRODUCT))));
    assert!(!wraps_exactly(NumberArithmetic::Multiply, Some((0, EXACT_PRODUCT + 1))));
    assert!(!wraps_exactly(NumberArithmetic::Multiply, Some((-(1 << 62), 1 << 62))));
    assert!(!wraps_exactly(NumberArithmetic::Multiply, None));
}

#[test]
#[cfg(not(quench_profiled))]
fn osr_truncated_products_match_the_interpreter() {
    // `x * 31`, `x * -1048573` and `y * 65599` are exact Numbers, so their wrapped `i32`s
    // serve `| 0`; `x * 0x7fffffff` and `x * y` can pass 2^53, where the Number rounds,
    // and `p`, compared, must hold the exact product.
    let source = r#"
function mix(s, n) {
  var x = s | 0, y = 7, z = s | 0, w = s | 0, big = 0;
  for (var i = 0; i < n; i++) {
    x = (x * 31 + i) | 0;
    y = (y * 65599 + x) | 0;
    z = (z * -1048573 - i) | 0;
    w = (w * 0x7fffffff + i) | 0;
    var p = x * 31;
    if (p > 0x7fffffff) big++;
    var q = (x * y) | 0;
    z = (z + q) | 0;
  }
  return [x, y, z, w, big].join(",");
}
var out = [];
for (var c = 0; c < 40; c++) out = [mix(c, 3000), mix(-2147483648 + c, 3000), mix(2147483647 - c, 3000)];
print(out.join(" "));
"#;
    let program = Engine::specialize(source, "osr-truncated-products.js").unwrap();
    let interpreted = Rc::new(RefCell::new(Vec::new()));
    Vm::new(TestHost::writing_to(interpreted.clone())).execute(&program).unwrap();
    for policy in crate::MemoryPolicy::ALL {
        let compiled = Rc::new(RefCell::new(Vec::new()));
        let mut vm = Vm::new(TestHost::writing_to(compiled.clone()));
        vm.set_memory_policy(policy);
        vm.set_execution_mode(crate::ExecutionMode::Compiled);
        vm.execute(&program).unwrap();
        assert_eq!(compiled.borrow().as_slice(), interpreted.borrow().as_slice(), "{policy:?}");
    }
}

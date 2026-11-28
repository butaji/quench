//! Shared OSR: a hot loop enters its compiled region mid-run, and the run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use crate::test_oracle::assert_loop_decision;
use std::cell::RefCell;

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_inlines_method_calls_in_a_loop() {
    // Each iteration calls `x.am` twice: the region runs am3 in place, its object
    // parameter bound to the caller register it was moved from.
    let source = r#"
function am3(i,x,w,j,c,n) {
  var this_array = this.array;
  var w_array    = w.array;
  var xl = x&0x3fff, xh = x>>14;
  while(--n >= 0) {
    var l = this_array[i]&0x3fff;
    var h = this_array[i++]>>14;
    var m = xh*l+h*xl;
    l = xl*l+((m&0x3fff)<<14)+w_array[j]+c;
    c = (l>>28)+(m>>14)+xh*h;
    w_array[j++] = l&0xfffffff;
  }
  return c;
}
function B(n) { this.array = []; this.t = n; for (var k = 0; k < n; k++) this.array.push((k * 2654435761) & 0xfffffff); }
B.prototype.am = am3;
function sq(x, r) {
  var x_array = x.array, r_array = r.array, s = 0;
  for (var i = 0; i < x.t - 1; ++i) {
    var c = x.am(i, x_array[i], r, 2 * i, 0, 1);
    s = (s + c + x.am(i + 1, 2 * x_array[i], r, 2 * i + 1, c, x.t - i - 1)) | 0;
  }
  return s;
}
var a = new B(40), w = new B(100), t = 0;
for (var q = 0; q < 3000; q++) t = (t + sq(a, w)) | 0;
print(t + " " + w.array[5] + " " + w.array[77]);

    "#;
    let program = Engine::specialize(source, "osr-inline-calls.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "sq"))
        .unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone())), &program).unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        assert_loop_decision(&compiled, function_id);
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_inlined_callee_leaves_through_its_activation() {
    // An element the inlined callee reads leaves the subset partway through its loop: native
    // code leaves at the callee's instruction, whose activation the interpreter finishes.
    let source = r#"
function addRun(src, w, j, n, c) {
  var w_array = w.arr, s_array = src.arr;
  var s = c;
  while (--n >= 0) {
    s = s + s_array[j];
    w_array[j++] = s & 1023;
  }
  return s;
}
function Box(n) { this.arr = []; for (var k = 0; k < n; k++) this.arr.push(k % 13); }
Box.prototype.run = addRun;
function drive(x, src, w, rounds) {
  var t = 0;
  for (var i = 0; i < rounds; i++) {
    var c = x.run(src, w, i & 31, 20, i);
    t = (t + (c | 0)) | 0;
  }
  return t;
}
var x = new Box(10), src = new Box(100), w = new Box(100), total = 0;
for (var q = 0; q < 300; q++) total = (total + drive(x, src, w, 200)) | 0;
src.arr[40] = 3.25;     // doubles from here on: widening inside the callee
for (var q = 0; q < 20; q++) total = (total + drive(x, src, w, 200)) | 0;
src.arr[44] = "9";      // a string: every pass reaching it leaves mid-callee
for (var q = 0; q < 20; q++) total = (total + drive(x, src, w, 200)) | 0;
print(total + " " + w.arr.slice(35, 50).join(","));

    "#;
    let program = Engine::specialize(source, "osr-inline-exits.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "drive"))
        .unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone())), &program).unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        assert_loop_decision(&compiled, function_id);
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_inlines_calls_on_a_receiver_a_field_read_gives() {
    // `reduce` calls `this.m.am(…)` and reads `this.m.t`, a nested field: its loop runs am3
    // in place, the receiver the value of `this.m` at entry.
    // Wrapped in a function, as a CommonJS module is, so `DM` and `DV` are captures.
    let source = r#"(function () {
var DM = 0xfffffff, DV = 0x10000000;
function am3(i,x,w,j,c,n) {
  var this_array = this.array;
  var w_array    = w.array;
  var xl = x&0x3fff, xh = x>>14;
  while(--n >= 0) {
    var l = this_array[i]&0x3fff;
    var h = this_array[i++]>>14;
    var m = xh*l+h*xl;
    l = xl*l+((m&0x3fff)<<14)+w_array[j]+c;
    c = (l>>28)+(m>>14)+xh*h;
    w_array[j++] = l&0xfffffff;
  }
  return c;
}
function B(n, seed) { this.array = []; this.t = n; for (var k = 0; k < n; k++) this.array.push(((k + seed) * 2654435761) & 0xfffffff); }
B.prototype.am = am3;
function M(m) { this.m = m; this.mpl = 0x1234; this.mph = 0x56; this.um = 0x7fff; }
M.prototype.reduce = function reduce(x) {
  var x_array = x.array;
  for (var i = 0; i < this.m.t; ++i) {
    var j = x_array[i]&0x7fff;
    var u0 = (j*this.mpl+(((j*this.mph+(x_array[i]>>15)*this.mpl)&this.um)<<15))&DM;
    j = i+this.m.t;
    x_array[j] += this.m.am(0,u0,x,i,0,this.m.t);
    while(x_array[j] >= DV) { x_array[j] -= DV; x_array[++j]++; }
  }
  var s = 0;
  for (var k = 0; k < x_array.length; k++) s = (s * 31 + x_array[k]) | 0;
  return s;
};
var mod = new M(new B(30, 7)), t = 0;
for (var q = 0; q < 2000; q++) { var x = new B(61, q); x.array.push(0); t = (t + mod.reduce(x)) | 0; }
print(t);
})();
"#;
    let program = Engine::specialize(source, "osr-inline-field-receiver.js").unwrap();
    let function = |wanted: &str| {
        program
            .functions
            .iter()
            .position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == wanted))
            .unwrap()
    };
    let (am3, reduce) = (function("am3"), function("reduce"));
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone())), &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["-1925714172"]);
    for policy in quench_runtime::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        if !crate::compiled::loop_tier_compiles(policy) {
            continue;
        }
        assert_loop_decision(&compiled, am3);
        assert_loop_decision(&compiled, reduce);
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_widens_an_int32_field_that_holds_a_double() {
    // `o.k` holds an `i32` while the loop compiles; once it holds 0.5 the entry declines,
    // naming the field, and the next compilation holds it as an `f64`.
    let source = r#"
      function sum(o, a) {
        var s = 0;
        for (var i = 0; i < o.n; i++) s += a[i] * o.k;
        return s;
      }
      var a = [], o = { n: 500, k: 3 }, t = 0;
      for (var i = 0; i < 500; i++) a.push(i);
      for (var c = 0; c < 400; c++) { if (c == 200) o.k = 0.5; t += sum(o, a); }
      print(t);
    "#;
    let program = Engine::specialize(source, "osr-field-widening.js").unwrap();
    let function_id = program.functions.iter().position(|function| function.header().name.is_some_and(|name| &program.atoms[name as usize] == "sum")).unwrap();
    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    quench_runtime::test_support::execute(&mut quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone())), &program).unwrap();
    assert_eq!(interpreted_output.borrow().as_slice(), ["87325000"]);
    for policy in quench_runtime::MemoryPolicy::ALL {
        let compiled_output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

        assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice(), "{policy:?}");
        if !crate::compiled::loop_tier_compiles(policy) {
            continue;
        }
        assert_loop_decision(&compiled, function_id);
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_wrapping_left_shift_preserves_negative_zero_products() {
    let source = r#"
function shiftedZero(a, n) {
  var negatives = 0;
  for (var i = 0; i < n; i++) {
    var shifted = (a[i] & 0x7fffffff) << 1;
    var product = shifted * 0;
    if (1 / product < 0) negatives++;
  }
  return negatives;
}
var a = [];
for (var i = 0; i < 400; i++) a.push(i % 2 ? 0x40000000 : 1);
var result = 0;
for (var j = 0; j < 100; j++) result = shiftedZero(a, 400);
print(result);
"#;
    let program = Engine::specialize(source, "osr-wrapping-shift-zero.js").unwrap();
    let function_id = program.functions.iter().position(|function| {
        function.header().name.is_some_and(|name| &program.atoms[name as usize] == "shiftedZero")
    }).unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        vm.set_memory_policy(policy);
        vm.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        assert_eq!(output.borrow().as_slice(), ["200"], "{policy:?}");
        if crate::compiled::loop_tier_compiles(policy) {
            assert_loop_decision(&vm, function_id);
        }
    }
}

/// Runs `source` compiled under every memory policy that compiles loops, holding the printed
/// output each policy gives to `expected`, and asserts the load's loop decision for function `name`.
pub(super) fn compiled_loop_decisions(source: &str, file: &str, name: &str, expected: &[&str]) {
    let program = Engine::specialize(source, file).unwrap();
    let function_id = program.functions.iter()
        .position(|function| function.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
    for policy in quench_runtime::MemoryPolicy::ALL.into_iter().filter(|policy| crate::compiled::loop_tier_compiles(*policy)) {
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        compiled.set_memory_policy(policy);
        compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
        quench_runtime::test_support::execute(&mut compiled, &program).unwrap();
        assert_eq!(output.borrow().as_slice(), expected, "{policy:?}");
        assert_loop_decision(&compiled, function_id);
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn shared_osr_stores_guarded_numeric_own_fields_in_a_loop() {
    let source = r#"
      function fill(item, limit) {
        for (let index = 0; index < limit; index++) {
          item.value = (index + 1) * 0.5;
          item.other = index + 1;
        }
        return limit;
      }
      function fillReference(item, value, limit) {
        for (let index = 0; index < limit; index++) item.reference = value;
        return item.reference === value ? limit : -1;
      }
      const item = { value: 0, other: 0, left: 1, right: 2 };
      const reference = {};
      const referenceTarget = { reference: null, left: 1, right: 2 };
      const frozen = { value: 0, other: 0, left: 1, right: 2 };
      let answer = 0;
      for (let call = 0; call < 80; call++) answer += fill(item, 2000);
      fill(frozen, 3);
      Object.freeze(frozen);
      fill(frozen, 4);
      answer += fillReference(referenceTarget, reference, 2000);
      let setterCalls = 0;
      Object.defineProperty(item, "value", {
        set(value) { setterCalls++; }, configurable: true
      });
      answer += fill(item, 10);
      let proxyCalls = 0;
      const proxyTarget = { value: 0, other: 0, left: 1, right: 2 };
      const proxy = new Proxy(proxyTarget, {
        set(target, key, value) { proxyCalls++; target[key] = value; return true; }
      });
      fill(proxy, 5);
      print(answer + " " + setterCalls + " " + proxyCalls + " " +
        (referenceTarget.reference === reference) + " " + item.other + " " + proxyTarget.other + " " + frozen.other);
    "#;
    let program = Engine::specialize(source, "osr-field-store-numeric.js").unwrap();
    let function_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|atom| &program.atoms[atom as usize] == "fill")
        })
        .unwrap();

    let interpreted_output = Rc::new(RefCell::new(Vec::new()));
    let mut interpreter = quench_runtime::test_support::vm(TestHost::writing_to(interpreted_output.clone()));
    interpreter.set_execution_mode(quench_runtime::ExecutionMode::Interpreted);
    quench_runtime::test_support::execute(&mut interpreter, &program).unwrap();

    let compiled_output = Rc::new(RefCell::new(Vec::new()));
    let mut compiled = quench_runtime::test_support::vm(TestHost::writing_to(compiled_output.clone()));
    compiled.set_memory_policy(quench_runtime::MemoryPolicy::Throughput);
    compiled.set_execution_mode(quench_runtime::ExecutionMode::Compiled);
    quench_runtime::test_support::execute(&mut compiled, &program).unwrap();

    assert_eq!(compiled_output.borrow().as_slice(), interpreted_output.borrow().as_slice());
    assert_eq!(compiled_output.borrow().as_slice(), ["162010 10 10 true 10 5 3"]);
    assert_loop_decision(&compiled, function_id);
    let reference_id = program
        .functions
        .iter()
        .position(|function| {
            function
                .header()
                .name
                .is_some_and(|atom| &program.atoms[atom as usize] == "fillReference")
        })
        .unwrap();
    assert_loop_decision(&compiled, reference_id);
}

//! The call-out rows in compiled mode (`staged::CallOut`): a lane body decides a row's common case
//! by words and calls the VM's own operation for the rest, so every program prints what the
//! interpreter prints, including the slow cases: string and BigInt equality, a symbol or
//! object property key, and the iterator protocol over a generator.
use super::*;
use crate::test_host::TestHost;
use crate::Engine;
use std::{cell::RefCell, rc::Rc};

/// What `source` prints in `mode`, its hot functions compiled to lane bodies where the mode compiles.
fn printed(source: &str, mode: crate::ExecutionMode) -> Vec<String> {
    let program = Engine::specialize(source, "call-out.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.set_memory_policy(crate::MemoryPolicy::Throughput);
    vm.set_execution_mode(mode);
    if matches!(mode, crate::ExecutionMode::Compiled) {
        vm.programs.enable_js_lane_methods();
    }
    vm.execute(&program).unwrap();
    let lines = output.borrow().clone();
    lines
}

/// The program's output in both modes, which must agree.
fn in_both_modes(source: &str) -> Vec<String> {
    let interpreted = printed(source, crate::ExecutionMode::Interpreted);
    assert_eq!(printed(source, crate::ExecutionMode::Compiled), interpreted);
    interpreted
}

#[test]
fn equality_of_strings_bigints_and_conversions_matches_the_interpreter() {
    let output = in_both_modes(r#"
      function classify(a, b) {
        const strict = a === b, loose = a == b, differ = a !== b, apart = a != b;
        return (strict ? 1 : 0) | (loose ? 2 : 0) | (differ ? 4 : 0) | (apart ? 8 : 0);
      }
      const samples = [1, 2.5, "a", "ab", "a" + "b", null, undefined, true, 0, -0, "0", 10n, 10n, NaN,
        { valueOf() { return 1; } }, [1], "1"];
      let codes = "";
      for (let repeat = 0; repeat < 400; repeat++) {
        codes = "";
        for (const a of samples) for (const b of samples) codes += classify(a, b).toString(16);
      }
      print(codes);
      print(classify("ab", "a" + "b") + " " + classify(10n, 10n) + " " + classify(1, "1") + " " + classify(null, undefined));
    "#);
    assert_eq!(output[1], "3 3 6 6", "equal strings, equal BigInts, converting and nullish pairs");
}

#[test]
fn property_keys_of_symbols_numbers_and_objects_match_the_interpreter() {
    let output = in_both_modes(r#"
      const marker = Symbol("marker");
      const text = { toString() { return "own"; } };
      function bump(target, key) { target[key] += 1; return target[key]; }
      function keyed(key, value) { const made = { [key]: value }; return Reflect.ownKeys(made).length; }
      const target = { a: 1, 1.5: 1, 7: 1, own: 1, [marker]: 1 };
      let last = 0;
      for (let repeat = 0; repeat < 600; repeat++) {
        for (const key of ["a", 1.5, 7, text, marker]) last = bump(target, key) + keyed(key, repeat);
      }
      print(last);
      print([target.a, target[1.5], target[7], target.own, target[marker]].join());
    "#);
    assert_eq!(output[1], "601,601,601,601,601");
}

#[test]
fn the_iterator_protocol_over_a_generator_matches_the_interpreter() {
    let output = in_both_modes(r#"
      function* count(n) { try { for (let i = 0; i < n; i++) yield i; } finally { closed += 1; } }
      let closed = 0;
      function total(n) { let t = 0; for (const v of count(n)) t += v; return t; }
      function head(n) { const [first, second] = count(n); return first * 10 + second; }
      function spread(n) { return [...count(n), 100].length; }
      function keys(o) { let k = ""; for (const key in o) k += key; return k; }
      let answer = "";
      for (let repeat = 0; repeat < 600; repeat++) {
        answer = total(6) + "," + head(5) + "," + spread(4) + "," + keys({ x: 1, y: 2, 3: 3 });
      }
      print(answer);
      print(closed);
    "#);
    assert_eq!(output[0], "15,1,5,3xy");
    assert_eq!(output[1], "1800", "each loop, destructuring and spread closes its generator once");
}

#[test]
fn a_call_out_that_throws_raises_the_interpreters_error_from_the_compiled_body() {
    let output = in_both_modes(r#"
      function bump(target, key) { target[key] += 1; return 0; }
      const poisoned = { toString() { throw new RangeError("no key"); } };
      let seen = "";
      for (let repeat = 0; repeat < 800; repeat++) {
        for (const key of ["a", 1]) bump({ a: 1, 1: 2 }, key);
        try { bump({}, poisoned); } catch (error) { seen = error.name + ":" + error.message; }
      }
      print(seen);
    "#);
    assert_eq!(output[0], "RangeError:no key");
}

#[test]
fn relations_bigint_operators_instanceof_and_in_match_the_interpreter() {
    let output = in_both_modes(r#"
      class Base {} class Derived extends Base {}
      function relate(a, b) {
        const lt = a < b, le = a <= b, gt = a > b, ge = a >= b;
        return (lt ? 1 : 0) | (le ? 2 : 0) | (gt ? 4 : 0) | (ge ? 8 : 0);
      }
      function big(a, b) { const sum = a + b, product = a * b, difference = a - b, power = a ** b; return `${sum},${product},${difference},${power}`; }
      function probe(value, key, constructor) { const has = key in value, is = value instanceof constructor; return (has ? 1 : 0) + (is ? 2 : 0); }
      const texts = ["a", "b", "ab", "", "10", "9"];
      let all = "";
      for (let repeat = 0; repeat < 400; repeat++) {
        all = "";
        for (const a of texts) for (const b of texts) all += relate(a, b).toString(16);
        all += big(7n, 3n) + probe(new Derived(), "x", Base) + probe({ x: 1 }, "x", Base) + probe([], "length", Array);
      }
      print(all);
    "#);
    assert!(output[0].ends_with("10,21,4,343213"), "{}", output[0]);
}

#[test]
fn global_names_environment_locals_and_arguments_match_the_interpreter() {
    let output = in_both_modes(r#"
      function names(first, second) {
        written = (typeof missing === "undefined" ? 1 : 0) + arguments.length;
        const held = first + second;
        return () => held + written;
      }
      var written;
      let last = 0;
      for (let repeat = 0; repeat < 800; repeat++) last = names(repeat, 1, 2)();
      print(last);
      print(typeof written + typeof absent);
    "#);
    assert_eq!(output, ["804", "numberundefined"]);
}

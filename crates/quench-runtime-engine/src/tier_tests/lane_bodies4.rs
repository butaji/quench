//! Lane bodies against the interpreter: each program runs compiled and interpreted, and the compiled run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use crate::test_oracle::{assert_tier_agrees, Expect, Tier};

#[test]
fn lane_instanceof_decides_like_the_interpreter() {
    // A lane body decides `instanceof` by the prototype chain: primitives, derived classes,
    // `Object`, a constructor with its own `@@hasInstance` and a reassigned `prototype`
    // answer as the interpreter answers them.
    let source = r#"
      function sc_Pair(car, cdr) { this.car = car; this.cdr = cdr; }
      function Other() { this.car = 1; }
      class Base { constructor() { this.car = 2; } }
      class Derived extends Base {}
      function Hooked() {}
      Object.defineProperty(Hooked, Symbol.hasInstance, { value: (v) => v === 7 });
      function isPair(p) { return p instanceof sc_Pair; }
      function count(xs, C) { let n = 0; for (let i = 0; i < xs.length; i++) { if (xs[i] instanceof C) n += i + 1; } return n; }
      const values = [new sc_Pair(1, 2), new Other(), null, undefined, 3, "s", new Derived(), new Base(), [], {}, Object.create(sc_Pair.prototype), 7, function () {}];
      let t = 0;
      for (let r = 0; r < 2000; r++) {
        for (let i = 0; i < values.length; i++) if (isPair(values[i])) t += i;
        t += count(values, sc_Pair) + count(values, Base) * 3 + count(values, Object) * 5 + count(values, Hooked) * 7;
        if (r === 1500) sc_Pair.prototype = { swapped: true };
      }
      globalThis.answer = t;
    "#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(901022.0));
}

#[test]
fn lane_strict_equality_decides_like_the_interpreter() {
    // A lane body decides `===` and `!==` of any pair natively: NaN, signed zeros, an
    // integer against its double, distinct strings and BigInts of equal contents, objects,
    // symbols and mixed kinds compare as the interpreter compares them, and a loop that
    // compares a non-Number keeps no Number proof of it past the comparison. Its body,
    // entered at the loop header, returns its own value: the record it completed at ran.
    let source = r#"
      function Pair(car, cdr) { this.car = car; this.cdr = cdr; }
      Pair.prototype.same = function (x) { return this.car === x; };
      Pair.prototype.differs = function (x) { if (this.car !== x) return 1; return 0; };
      Pair.prototype.count = function (xs) {
        let n = 0;
        for (let i = 0; i < xs.length; i++) { if (this.car === xs[i]) n += i + 1; if (xs[i] !== this.cdr) n += 3; }
        return n;
      };
      const o = {}, s1 = "ab" + String(1 + 1), big = 10n ** 20n;
      const values = [0, -0, 1, 1.5, NaN, Infinity, "ab2", s1, "ab", "", null, undefined, true, false, o, {}, big, 10n ** 20n, 7n, Symbol.iterator, [], 2 ** 31, -(2 ** 31)];
      let total = 0;
      for (let round = 0; round < 300; round++) {
        for (let i = 0; i < values.length; i++) {
          const p = new Pair(values[i], values[(i + round) % values.length]);
          for (let j = 0; j < values.length; j++) {
            if (p.same(values[j])) total += (i + 1) * (j + 2);
            total += p.differs(values[j]) * (j + 1);
          }
          total += p.count(values);
        }
      }
      function sc_assq(o, al) {
          while (al !== null) {
              if (al.car.car === o)
                  return al.car;
              al = al.cdr;
          }
          return false;
      }
      const keys = []; for (let i = 0; i < 40; i++) keys.push("ẜk" + i);
      let al = null; for (let i = 0; i < 40; i++) al = new Pair(new Pair(keys[i], i), al);
      let t = 0;
      for (let n = 0; n < 20000; n++) { const r = sc_assq(keys[n % 45 < 40 ? n % 45 : 0] , al); if (r !== false) t += r.cdr; else t -= 1; }
      globalThis.answer = total + t;
    "#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(4307110.0));
}

#[test]
fn lane_outer_captures_read_like_the_interpreter() {
    // A lane body reads a capture of an outer scope by the lane's own lookup, so a
    // reassigned `var`, a `let` and a widened value read as the interpreter's.
    let source = r#"
      function outer() {
        var scale = 3; let offset = 1; var total = 0;
        function middle(k) {
          function inner(x) { return (x * scale + offset) | 0; }
          var s = 0; for (var i = 0; i < 5; i++) s = (s + inner(i + k)) | 0; return s;
        }
        for (var k = 0; k < 3000; k++) { total = (total + middle(k)) | 0; if (k % 500 == 0) { scale++; offset = offset * 2; } }
        scale = 0.5;
        for (var k = 0; k < 200; k++) total = (total + middle(k)) | 0;
        return total;
      }
      globalThis.answer = outer();
    "#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(168662875.0));
}

#[test]
fn lane_element_stores_append_like_the_interpreter() {
    // A lane store at an array's length appends where [[Set]] would add the index: a
    // prototype setter, a non-writable length, a non-extensible or frozen array take the
    // interpreter's [[Set]], and an array two names share grows under both.
    let source = r#"(function () {
      function fill(r, n, v) { var k = 0; for (var i = 0; i < n; i++) r[k++] = v + i; return r.length + r[n - 1]; }
      var total = 0, seen = 0;
      for (var k = 0; k < 3000; k++) total = (total + fill([], 9, k) + fill(new Array(), 3, 1)) | 0;
      Object.defineProperty(Array.prototype, 5, { set(v) { seen += v; }, get() { return -1; }, configurable: true });
      for (var k = 0; k < 40; k++) total = (total + (fill([], 9, k) | 0)) | 0;
      delete Array.prototype[5];
      var fixed = [1, 2]; Object.defineProperty(fixed, "length", { writable: false });
      for (var k = 0; k < 40; k++) { try { total = (total + (fill(fixed, 4, k) | 0)) | 0; } catch (e) { total = (total + 7) | 0; } }
      var closed = Object.preventExtensions([0]);
      for (var k = 0; k < 40; k++) total = (total + (fill(closed, 3, k) | 0)) | 0;
      var frozen = Object.freeze([3, 4]);
      for (var k = 0; k < 40; k++) total = (total + (fill(frozen, 3, k) | 0)) | 0;
      var a = [], b = a; for (var k = 0; k < 40; k++) total = (total + fill(a, 5, k)) | 0;
      globalThis.answer = (total + seen + b.length + fixed.length + closed.length + frozen[1]);
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(4571092.0));
}

#[test]
fn lane_element_stores_fill_holes_like_the_interpreter() {
    // A lane store fills a hole of an extensible array that owns its elements when no
    // prototype claims the index; a prototype setter, a non-extensible array and shared
    // elements take the interpreter's [[Set]].
    let source = r#"(function () {
      function fill(a, n, v) { for (var i = 0; i < n; i++) a[i] = v + i; return a[n - 1]; }
      var total = 0, seen = 0, last = 0;
      for (var k = 0; k < 3000; k++) total = (total + fill(new Array(8), 8, k)) | 0;
      Object.defineProperty(Array.prototype, 3, { set(v) { seen++; last = v; }, get() { return -1; }, configurable: true });
      for (var k = 0; k < 50; k++) total = (total + fill(new Array(8), 8, k)) | 0;
      delete Array.prototype[3];
      var sealed = Object.preventExtensions(new Array(8));
      for (var k = 0; k < 50; k++) total = (total + (fill(sealed, 8, k) | 0)) | 0;
      var shared = new Array(8); var copy = shared;
      for (var k = 0; k < 50; k++) total = (total + fill(shared, 8, k)) | 0;
      globalThis.answer = (total + seen * 7 + last + (sealed[2] === undefined ? 1 : 2) + copy[7] + Object.keys(sealed).length);
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(4523109.0));
}

#[test]
fn lane_loops_that_store_read_other_atoms_once_like_the_interpreter() {
    // A loop that stores keeps reading each atom it stores, on whichever object: one object
    // under two names, a method beside a stored field, a prototype's field under a getter.
    let source = r#"(function () {
      function P(x, y) { this.x = x; this.y = y; }
      P.prototype.get = function () { return this.x; };
      function same(p, q, n) { var s = 0; for (var i = 0; i < n; i++) { q.x = (q.x + 1) | 0; s = (s + p.x) | 0; } return s; }
      function other(p, n) { var s = 0; for (var i = 0; i < n; i++) { p.y = (p.y + p.x) | 0; s = (s + p.get() + p.y) | 0; } return s; }
      function proto(p, n) { var s = 0; for (var i = 0; i < n; i++) { P.prototype.x = i; s = (s + p.get()) | 0; } return s; }
      var a = new P(1, 2), b = new P(3, 4), c = Object.create(P.prototype), total = 0;
      for (var k = 0; k < 3000; k++) {
        total = (total + same(a, k & 1 ? a : b, 20) + other(b, 10)) | 0;
        if (k % 100 == 0) total = (total + proto(c, 5)) | 0;
      }
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(2005786796.0));
}

#[test]
fn lane_loops_read_invariant_fields_once_like_the_interpreter() {
    // A loop that only reads takes its invariant fields on its entry edge: the field may
    // change between runs, another shape or a non-object leaves at the header, a loop that
    // never runs reads nothing the interpreter would not, and a widened slot reads anew.
    let source = r#"(function () {
      function P(x, y) { this.x = x; this.y = y; }
      function Q(x, y) { this.z = 0; this.x = x; this.y = y; }
      function sum(p, n) { var s = 0; for (var i = 0; i < n; i++) s = (s + p.x + p.y) | 0; return s; }
      var p = new P(3, 4), q = new Q(5, 6), total = 0;
      for (var k = 0; k < 3000; k++) { p.x = k & 15; total = (total + sum(k % 7 ? p : q, k & 31)) | 0; }
      total = (total + sum(undefined, 0)) | 0;
      p.y = 0.5;
      for (var k = 0; k < 300; k++) total = (total + sum(p, 9)) | 0;
      p.y = "2";
      for (var k = 0; k < 300; k++) total = (total + sum(p, 3)) | 0;
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(3000684.0));
}

#[test]
fn lane_bitwise_operands_typed_by_tags_take_to_int32_like_the_interpreter() {
    // A bitwise operand only its tag types is its int where the tag says so, else its
    // Number's ToInt32: doubles, NaN, infinities and values past 2^31 and 2^63.
    let source = r#"(function () {
      var MASK = 4, SHIFT = 3;
      function held(o) { return ((o.state & MASK) != 0 ? 1 : 0) + ((o.state | 0) >> SHIFT) + (o.state ^ MASK); }
      var vals = [5, 4, 0, 2.5, -7.75, NaN, Infinity, -Infinity, 2147483648.5, -2147483649, 1e19, 9007199254740993, -0];
      var total = 0;
      for (var k = 0; k < 4000; k++) total = (total * 3 + held({ state: vals[k % vals.length] })) % 1000003;
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(-558229.0));
}

#[test]
fn lane_relations_of_tagged_operands_compare_ints_and_numbers_like_the_interpreter() {
    // Operands only their tags type compare as int32 where both are ints, else as Numbers:
    // NaN, -0, the int32 bounds and doubles take the Number path.
    let source = r#"(function () {
      var STATE = 2;
      function rank(o, b) { var a = o.v; if (a < b) return 1; if (a == STATE) return 2; if (a >= b) return 3; return 4; }
      var xs = [1, 2, 3, 2.5, -0, 0, NaN, 2147483647, -2147483648, 1e300, 2.0000001];
      var total = 0;
      for (var k = 0; k < 4000; k++) {
        var o = { v: xs[k % xs.length] };
        total = (total * 5 + rank(o, xs[(k * 7) % xs.length])) % 1000003;
      }
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    // Node's answer for the same program.
    assert_eq!(run.outcome.answer_number(), Some(61024.0));
}

#[test]
fn lane_int_arithmetic_wraps_and_bounds_inductions_like_the_interpreter() {
    // A sum only `| 0` reads wraps without an overflow exit; an increment its loop's `i < n`
    // bounds runs without one; one no test bounds keeps it and overflows to a double.
    let source = r#"(function () {
      function wrapAdd(a, n) { var s = a; for (var i = 0; i < n; i++) s = (s + 2147483000) | 0; return s; }
      function wrapSub(a, n) { var s = a; for (var i = 0; i < n; i++) s = (s - 2147483000) | 0; return s; }
      function nearMax(start, bound) { var c = 0; for (var i = start; i < bound; i++) c = (c + 1) | 0; return c * 4294967296 + i; }
      function unbounded(start, n) { var i = start, c = 0; while (c < n) { i++; c++; } return i; }
      var total = 0;
      for (var r = 0; r < 3000; r++) {
        total = wrapAdd(r, 7) + wrapSub(-r, 7) + nearMax(2147483640, 2147483647) + unbounded(2147483645, 5);
      }
      globalThis.answer = total;
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full, Expect::ENGAGED);
    assert_eq!(run.outcome.answer_number(), Some(2147482111.0 - 2147482111.0 + 7.0 * 4294967296.0 + 2147483647.0 + 2147483650.0));
}

#[test]
#[cfg(not(quench_profiled))]
fn lane_bodies_leave_before_a_store_changes_property_storage() {
    use quench_runtime::bytecode::Op;
    let program = Engine::specialize(r#"
      function put(value) {
        this.value = value;
        return this.value + 1;
      }
      function aliasPut(value) {
        const alias = this;
        this.value = value;
        return alias.value + 1;
      }
      globalThis.direct = { value: 0 };
      globalThis.aliased = { value: 0 };
      for (let i = 0; i < 100; i++) {
        put.call(direct, 0);
        aliasPut.call(aliased, 0);
      }
    "#, "lane-storage-transition.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(TestHost::default());
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    for (name, object_name) in [("put", "direct"), ("aliasPut", "aliased")] {
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let body = program.function(id);
        let instructions: Vec<_> = body.instructions().collect();
        let store_pc = instructions.iter().position(|instruction|
            matches!(instruction.op(), Op::SetThisField | Op::SetThisFieldStrict)).unwrap();
        let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).expect("warmed lane method");
        let atom = vm.intern_atom(object_name);
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
        registers[0] = Value::number(0.5);
        let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
            &program, registers.as_mut_ptr(), receiver, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        assert_eq!(pc, store_pc, "{name}: decline before changing the native entry's storage facts");
        let value_atom = vm.intern_atom("value");
        assert_eq!(vm.own_property(receiver, value_atom), Some(Value::integer(0)));
        let atom = vm.intern_atom(name);
        let function = vm.own_property(vm.realm.globals, atom).unwrap();
        assert_eq!(vm.call_value(&program, function, receiver, &[Value::number(0.5)]).unwrap(), Value::number(1.5));

        // The same artifact proves the new arena form at its next entry.
        registers.fill(Value::UNDEFINED);
        registers[0] = Value::number(2.5);
        let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
            &program, registers.as_mut_ptr(), receiver, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
        let instruction = instructions[pc];
        assert_eq!(instruction.op(), Op::Return);
        assert_eq!(registers[usize::from(instruction.register_a())], Value::number(3.5));
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn typed_lane_numbers_keep_native_views_and_exact_recovery() {
    use quench_runtime::bytecode::Op;
    let source = include_str!("../../../quench-runtime/tests/fixtures/typed-lane-numbers.js");
    let typescript = source
        .replace("function accumulate(input, count)", "function accumulate(input: number, count: number): number")
        .replace("function scratch(input)", "function scratch(input: number): number")
        .replace("function mixed(input, flag)", "function mixed(input: number, flag: boolean): number | boolean")
        .replace("function preserve(input, flag)", "function preserve(input: number, flag: boolean): number")
        .replace("function floatingBranch(input)", "function floatingBranch(input: number): number");
    for program in [Engine::specialize(source, "typed-lane-numbers.js"),
        Engine::specialize_typescript(&typescript, "typed-lane-numbers.ts")] {
        let program = program.unwrap();
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let inputs = [0.0, -0.0, 0.5, f64::from(i32::MIN), f64::from(i32::MAX),
            f64::NAN, f64::INFINITY, f64::NEG_INFINITY];
        for name in ["accumulate", "scratch", "mixed", "preserve", "floatingBranch"] {
            let id = program.functions.iter().position(|body|
                body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
            let body = program.function(id);
            let instructions: Vec<_> = body.instructions().collect();
            let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).expect("typed lane method");
            let atom = vm.intern_atom(name);
            let function = vm.own_property(vm.realm.globals, atom).unwrap();
            for input in inputs {
                for count in [0, 1, 7] {
                    let arguments = [Value::number(input), Value::integer(count)];
                    let expected = vm.call_value(&program, function, Value::UNDEFINED, &arguments).unwrap();
                    let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
                    registers[..arguments.len()].copy_from_slice(&arguments);
                    let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
                        &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).settled(&mut registers, &instructions).expect("caller exit");
                    let instruction = instructions[pc];
                    assert_eq!(instruction.op(), Op::Return, "{name}({input}, {count}) exited at {pc}");
                    let actual = registers[usize::from(instruction.register_a())];
                    assert_eq!(actual.to_bits(), expected.to_bits(), "{name}({input}, {count})");
                }
            }
            if name != "accumulate" { continue; }
            let atom = vm.intern_atom("counting");
            let counting = vm.own_property(vm.realm.globals, atom).unwrap();
            let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
            registers[0] = Value::number(0.5);
            registers[1] = counting;
            let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
                &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
            assert_eq!(pc, 0, "a non-Number bound leaves before any coercion");
            let atom = vm.intern_atom("countCoercions");
            assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::integer(0)));
            assert_eq!(vm.call_value(&program, function, Value::UNDEFINED,
                &[Value::number(0.5), counting]).unwrap(), Value::number(0.890625));
            assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::integer(3)),
                "loop-bound coercion remains once per JS comparison");
            for input_name in ["textInput", "bigInput", "coercing"] {
                let atom = vm.intern_atom(input_name);
                let input = vm.own_property(vm.realm.globals, atom).unwrap();
                let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
                registers[0] = input;
                registers[1] = Value::integer(1);
                let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
                    &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).caller_pc().expect("caller exit");
                assert!(pc < instructions.len() && instructions[pc].op() != Op::Return,
                    "{input_name} must leave before numeric effects");
                assert_eq!(registers[0].to_bits(), input.to_bits(), "recover the original input");
                let atom = vm.intern_atom("coercions");
                assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::integer(0)));
                let result = vm.call_value(&program, function, Value::UNDEFINED, &[input, Value::integer(1)]);
                if input_name == "bigInput" {
                    assert!(result.is_err());
                } else if input_name == "coercing" {
                    assert_eq!(result.unwrap(), Value::number(3.9375));
                    assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::integer(1)));
                } else {
                    assert!(result.unwrap().as_number().unwrap().is_nan());
                }
            }
        }
    }
}

#[test]
#[cfg(not(quench_profiled))]
fn typed_lane_nullish_branches_share_semantics_without_number_guards() {
    use quench_runtime::bytecode::Op;
    let program = Engine::specialize(r#"
      function loose(value) { let result=0; if (value==null) result=1; return result; }
      function strictNull(value) { let result=0; if (value===null) result=1; return result; }
      function strictUndefined(value) { let result=0; if (value===(void 0)) result=1; return result; }
      function reversed(value) { let result=1; if ((void 0)!=value) result=0; return result; }
      globalThis.nullValue=null;
      globalThis.undefinedValue=undefined;
      globalThis.zeroValue=0;
      globalThis.nanValue=NaN;
      globalThis.booleanValue=false;
      globalThis.objectValue={ valueOf() { throw 'must not coerce'; } };
      globalThis.arrayValue=[];
      globalThis.stringValue='';
      globalThis.bigValue=1n;
      globalThis.functionValue=function(){};
      globalThis.symbolValue=Symbol('nullish');
      globalThis.htmlValue=$262.IsHTMLDDA;
    "#, "typed-lane-nullish.js").unwrap();
    let mut vm = quench_runtime::test_support::vm(TestHost::default().with_test262());
    quench_runtime::test_support::execute(&mut vm, &program).unwrap();
    for name in ["loose", "strictNull", "strictUndefined", "reversed"] {
        let id = program.functions.iter().position(|body|
            body.header().name.is_some_and(|atom| &program.atoms[atom as usize] == name)).unwrap();
        let body = program.function(id);
        let instructions: Vec<_> = body.instructions().collect();
        let method = vm.compile_lane_method(&program, id as u32, &|_| (0, 0), Vec::new()).expect("nullish lane method");
        let atom = vm.intern_atom(name);
        let function = vm.own_property(vm.realm.globals, atom).unwrap();
        for value_name in ["nullValue", "undefinedValue", "zeroValue", "nanValue", "booleanValue",
            "objectValue", "arrayValue", "stringValue", "bigValue", "functionValue", "symbolValue", "htmlValue"] {
            let atom = vm.intern_atom(value_name);
            let value = vm.own_property(vm.realm.globals, atom).unwrap();
            let expected = vm.call_value(&program, function, Value::UNDEFINED, &[value]).unwrap();
            let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
            registers[0] = value;
            let pc = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
                &program, registers.as_mut_ptr(), Value::UNDEFINED, std::ptr::null(), std::ptr::null_mut()).settled(&mut registers, &instructions).expect("caller exit");
            let instruction = instructions[pc];
            if name.starts_with("strict") || value.is_record() || value.heap_reference().is_none() {
                assert_eq!(instruction.op(), Op::Return, "{name}({value_name}) exited at {pc}: {instructions:?}");
                assert_eq!(registers[usize::from(instruction.register_a())], expected, "{name}({value_name})");
            } else {
                assert_eq!(instruction.op(), Op::JumpBinaryFalse, "legacy heap values need IsHTMLDDA recovery");
                assert_eq!(registers[0].to_bits(), value.to_bits());
            }
        }
    }
}

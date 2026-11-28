//! Lane bodies against the interpreter: each program runs compiled and interpreted, and the compiled run must end as the interpreter's does (moved from the core's `vm/tests.rs`, whose binary does not link the layers).
use crate::engine_prelude::*;
use crate::ext_traits::*;
use quench_runtime::test_host::TestHost;
use quench_runtime::Engine;
use std::cell::RefCell;
use crate::test_oracle::{assert_outcomes_agree_after, assert_tier_agrees, Tier};

/// A loop that calls a function writing the field its condition reads must see the write:
/// the region's entry value of the field does not stand past a call it continues after.
#[test]
fn loop_condition_reads_the_field_a_called_function_writes() {
    let source = r#"function advance(state) { state.count = (state.count - 1) | 0; }
        function run(state) { while (state.count > 0) advance(state); }
        const s = {count: 4000}; run(s); print(String(s.count));"#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert_eq!(run.outcome.printed, ["0"]);
}

#[test]
fn typed_field_call_materializes_after_effects_once() {
    let source = r#"(function() {
        function advance(state, replacement) {
            state.count = (state.count - 1) | 0;
            if (state.count === 1500) state.control = replacement;
            state.control = (state.control >> 1) ^ ((state.control & 1) ? 0xD008 : 0);
        }
        function run(state, replacement) {
            while (state.count > 0) advance(state, replacement);
        }
        run({count: 4000, control: 7}, 7);
        const state = {count: 4000, control: 7};
        let calls = 0;
        const marker = {};
        try { run(state, {valueOf() { calls++; throw marker; }}); }
        catch (error) { if (error !== marker) throw error; }
        if (state.count !== 1500 || calls !== 1) throw 'replayed callee effects';
        print(state.count + ':' + calls);
    })();"#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert_eq!(run.outcome.printed, ["1500:1"]);
}

#[test]
fn typed_bitwise_branches_preserve_coercion_and_prefix_effects() {
    for operator in ["&", "|", "^", "<<", ">>", ">>>"] {
        let source = format!(r#"
            function step(state, value, shift) {{
                state.writes = (state.writes + 1) | 0;
                if (value {operator} shift) state.hits = (state.hits + 1) | 0;
            }}
            function run(value, shift) {{
                const state = {{ writes: 0, hits: 0 }};
                for (let i = 0; i < 4000; i++) step(state, value, shift);
                return state.writes + ':' + state.hits;
            }}
            let calls = 0;
            const object = {{ valueOf() {{ calls++; return 7; }} }};
            for (const value of [0, -1, 7, 2147483648, 4294967296, 1.5, -0,
                    NaN, Infinity, 1e100, '7', null, undefined, object]) {{
                for (const shift of [0, 1, 31, 32, -1, 1.5]) print(run(value, shift));
            }}
            print(calls);
            const marker = {{}};
            let writes = 0, coercions = 0;
            function throws(value) {{
                for (let i = 0; i < 4000; i++) {{
                    writes++;
                    if (value {operator} 1) continue;
                }}
            }}
            throws(7);
            try {{ throws({{ valueOf() {{ coercions++; throw marker; }} }}); }}
            catch (error) {{ print(error === marker); }}
            print(writes + ':' + coercions);
            try {{ throws(7n); }} catch (error) {{ print(error instanceof TypeError); }}
            print(writes);
        "#);
        let run = assert_tier_agrees(&source, Tier::Full);
        assert_eq!(&run.outcome.printed[run.outcome.printed.len() - 5..], ["24000", "true", "4001:1", "true", "4002"]);
    }
}

#[cfg(not(quench_profiled))]
#[test]
fn shape_compaction_clears_own_field_hints() {
    // Read, nested read and store sites warmed on one layout hold its shape id as their own-field hint. A full
    // collection that compacts the shape table renumbers ids, after which an old id can name
    // another layout, so the compaction clears every hint (`LaneView::invalidate_shape_bindings`).
    const SITES: usize = 64;
    for body in ["return o.a + 0;", "o.a = v; return v;", "return o.n.a + 0;"] {
        let reads: String = (0..SITES).map(|k| format!("function read{k}(o, v) {{ {body} }}\n")).collect();
        let calls: String = (0..SITES).map(|k| format!("read{k},")).collect();
        let source = format!(
            r#"
          {reads}
          var sites = [{calls}], sum = 0;
          function T(a, b) {{ this.a = a; this.b = b; this.n = this; }}
          function drive(read, o, n, v) {{ var s = 0; for (var i = 0; i < n; i++) s += read(o, v); return s; }}
          function resume() {{
            var result = 0;
            for (var k = 0; k < sites.length; k++) {{ var r = {{ before: -1, a: k }}; r.n = r; result += drive(sites[k], r, 50, k); }}
            return result;
          }}
          for (var k = 0; k < sites.length; k++) sum += drive(sites[k], new T(k, -1), 50, k);
          // Layouts no object keeps, so the next full collection compacts the table.
          for (var k = 0; k < 20000; k++) {{ var o = {{}}; o['junk' + k] = k; }}
          print(String(sum));
        "#
        );
        let output = Rc::new(RefCell::new(Vec::new()));
        let mut vm = quench_runtime::test_support::vm(TestHost::writing_to(output.clone()));
        let program = Engine::specialize(&source, "field-hints-compaction.js").unwrap();
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        assert_eq!(output.borrow().as_slice(), [(50 * (0..SITES).sum::<usize>()).to_string()]);
        let held = |vm: &Vm| {
            vm.programs
                .views_in_lane(quench_runtime::vm::program_store::ProgramId::MAIN)
                .into_iter()
                .flatten()
                .filter_map(quench_runtime::vm::program_store::derived_lane_view)
                .map(|view| view.held_field_hints())
                .sum::<usize>()
        };
        assert!(held(&vm) >= SITES);
        let shapes = vm.shapes.len();
        vm.collect_now(&program);
        assert!(vm.shapes.len() < shapes, "the collection compacted the shape table");
        assert_eq!(held(&vm), 0);
        // Re-enter the same read sites with `a` in the second slot. A derived slot reader
        // must not keep using its pre-compaction first-slot position.
        let resume = vm.lookup_atom("resume").unwrap();
        let resume = vm.own_property(vm.realm.globals, resume).unwrap();
        assert_eq!(
            vm.call_value(&program, resume, Value::UNDEFINED, &[]).unwrap(),
            Value::integer((50 * (0..SITES).sum::<usize>()) as i32),
        );
    }
}

#[test]
fn inherited_field_proofs_retire_when_a_prototype_on_the_chain_changes() {
    // An inherited read proved through `B.prototype` to `A.prototype` (`Dependencies::
    // prove_inherited`) must see a property `B.prototype` gains, and a new prototype it
    // takes, each of which retires the proof held across the warm loop.
    let source = r#"
(function () {
function A() {} A.prototype.m = function () { return 1; };
function C() {} C.prototype.m = function () { return 5; };
function B() {} B.prototype = Object.create(A.prototype);
var b = new B(), s = 0;
for (var q = 0; q < 3000; q++) s += b.m();
B.prototype.m = function () { return 3; };
for (var q = 0; q < 3000; q++) s += b.m();
delete B.prototype.m;
Object.setPrototypeOf(B.prototype, C.prototype);
for (var q = 0; q < 3000; q++) s += b.m();
print(s);
})();
"#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert_eq!(run.outcome.printed, ["27000"]);
}

#[test]
fn environment_rest_gathers_nothing_from_too_few_values() {
    // A captured rest parameter lives in the environment; with fewer values than the fixed
    // parameters, it is empty (v2-web cbaf372c1 fixed an out-of-range slice here).
    let source = r#"
      function r(a, b, ...m) { const f = () => m; return f(); }
      const seen = [];
      for (let round = 0; round < 3; round++) {
        seen.push(r(), r(1), r(1, 2), r(1, 2, 3, 4));
      }
      print(JSON.stringify(seen.slice(0, 4)) + ' ' + seen.every((rest, index) => rest.length === [0, 0, 0, 2][index % 4]));
    "#;
    let run = assert_outcomes_agree_after(&[], source, Tier::Full);
    assert_eq!(run.outcome.printed, ["[[],[],[],[3,4]] true"]);
}

#[test]
fn lane_name_loads_resolve_undefined_intrinsics_and_call_receivers() {
    // By-name loads of the global `undefined`, of globals a call reads (whose receiver is
    // `undefined`: a strict function's `this`, globalThis for a sloppy one), of intrinsic
    // names (`{ __proto__: v }`, `super`) and of intrinsic natives, run from their first
    // resolution to their bound reads (`load_name_resolve_miss`, `name_row`). The
    // expected values are Node's.
    let source = r#"
      function hot(f) { var s = 0; for (var i = 0; i < 3000; i++) s = (s + f(i)) | 0; return s; }
      function strictThis() { "use strict"; return this === undefined ? 1 : 0; }
      function sloppyThis() { return this === globalThis ? 1 : 0; }
      var out = [];
      out.push(hot(function () { return undefined === void 0 ? 1 : 0; }));
      out.push(hot(function () { return strictThis(); }));
      out.push(hot(function () { return sloppyThis(); }));
      out.push(hot(function (i) { return Math.max(i, 5) & 7; }));
      out.push(hot(function (i) { return { __proto__: Array.prototype, v: i } instanceof Array ? 1 : 0; }));
      class A { m() { return 1; } }
      class B extends A { m() { return super.m() + 1; } }
      var b = new B();
      out.push(hot(function () { return b.m(); }));
      out.push(hot(function () { return typeof parseInt === "function" ? 1 : 0; }));
      out.push(hot(function () { return Number.isInteger(parseInt("12", 10)) ? 2 : 0; }));
      print(out.join(" "));
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert_eq!(run.outcome.printed, ["3000 3000 3000 10515 3000 6000 3000 6000"]);
}

#[test]
fn lane_generator_window_mapped_arguments_alias_parameters_through_the_owning_activation() {
    // Elements of a sloppy generator's mapped `arguments` write through to its parameters while
    // it runs as a lane activation (`LaneStack::window_parameter`), nested beside another
    // generator's activation; the expectations are Node's.
    let source = r#"
      function* inner(b) { arguments[0] = b + 1; yield b; }
      function* outer(a) {
        arguments[0] = a * 2;
        for (var v of inner(a)) yield v;
        yield a;
        a = 5;
        yield arguments[0];
      }
      var total = 0;
      for (var i = 0; i < 2000; i++) {
        var got = [];
        for (var v of outer(i)) got.push(v);
        if (got.join() !== [i * 2 + 1, i * 2, 5].join()) throw Error(got.join());
        total++;
      }
      print(String(total));
    "#;
    let run = assert_outcomes_agree_after(&[], source, Tier::Full);
    assert_eq!(run.outcome.printed, ["2000"]);
}

#[test]
fn lane_intrinsic_native_reads_keep_each_name_its_own_native() {
    // Hot reads of several intrinsic names (`{ __proto__: v }`, `super.x = v`) interleave: each
    // atom reads the native its own text stands for (`bytecode::intrinsic_name_native`).
    let source = r#"
      function hot(f) { var s = 0; for (var i = 0; i < 3000; i++) s = (s + f(i)) | 0; return s; }
      class A { set x(v) { this.y = v; } }
      class B extends A { m(v) { super.x = v; return this.y; } }
      var b = new B();
      var p = { tag: 1 };
      print([hot(function (i) { return { __proto__: p, v: i }.tag; }),
             hot(function (i) { return b.m(i & 3); }),
             hot(function (i) { return { __proto__: null, v: i }.v & 1; })].join(" "));
    "#;
    let run = assert_tier_agrees(source, Tier::Full);
    assert_eq!(run.outcome.printed, ["3000 4500 1500"]);
}

#[test]
fn inlined_recovery_keeps_parameters_observable_through_legacy_arguments() {
    let program = Engine::specialize(r#"(function () {
      function add(k, unused) { k = k + 1; this.calls = this.calls + 1; return add.arguments; }
      function step(k) { k = k + 1; const a = this.add(k, 9); return a[0] + a[1] + step.arguments[0]; }
      globalThis.receiver = { calls: 0, add };
      globalThis.entry = step;
      let sum = 0;
      for (let i = 0; i < 2000; i++) sum += step.call(receiver, 1);
      globalThis.answer = sum;
    })();"#, "inline-legacy-recovery.js").unwrap();
    for mode in [quench_runtime::ExecutionMode::Interpreted, quench_runtime::ExecutionMode::Compiled] {
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        vm.programs.enable_js_lane_methods();
        vm.set_execution_mode(mode);
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let atom = vm.intern_atom("answer");
        assert_eq!(vm.own_property(vm.realm.globals, atom), Some(Value::number(28000.0)));
        let atom = vm.intern_atom("receiver");
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        let atom = vm.intern_atom("entry");
        let caller = vm.own_property(vm.realm.globals, atom).unwrap();
        let atom = vm.intern_atom("add");
        let callee = vm.own_property(receiver, atom).unwrap();
        let quench_runtime::heap::FunctionKind::User(_, caller_id) = vm.heap.function(caller).unwrap().kind else { panic!("script caller") };
        let quench_runtime::heap::FunctionKind::User(_, callee_id) = vm.heap.function(callee).unwrap().kind else { panic!("script callee") };
        let body = program.function(caller_id as usize);
        let callee_body = program.function(callee_id as usize);
        let call = body.instructions().position(|instruction| instruction.op() == quench_runtime::bytecode::Op::Call).unwrap();
        let mut targets = vec![None; body.code.len()];
        targets[call] = Some(quench_runtime::vm::engine_types::CallTarget { callee: quench_runtime::vm::engine_types::CalleeBinding::Identity(callee), function: callee_id });
        let callee_captures = vm.callee_capture_slots(callee);
        let method = vm.compile_lane_method(&program, caller_id, &|_| callee_captures, Vec::new())
            .expect("legacy-observable inlined unit");
        let mut registers = vec![Value::UNDEFINED; usize::from(body.registers)];
        let parameter = body.parameter_registers.or_else(|| body.promoted_register(0)).unwrap();
        registers[usize::from(parameter)] = Value::number(5.0);
        let env = vm.heap.function(caller).unwrap().env;
        let (slots, _) = vm.environment_capture_slots(env, false, 0).unwrap();
        let slots = slots.dense_storage().unwrap();
        let exit = vm.run_lane_method(&method, false, quench_runtime::vm::program_store::ProgramId::MAIN,
            &program, registers.as_mut_ptr(), receiver, slots, std::ptr::null_mut());
        assert_eq!(registers[usize::from(parameter)], Value::number(6.0), "suspended caller retains its reflected parameter");
        let quench_runtime::vm::engine_types::LaneExit::Inlined { frame, pc } = exit else { panic!("expected exact callee exit, got {exit:?}") };
        assert_eq!(frame, 0);
        assert_eq!(callee_body.instructions().nth(pc as usize).unwrap().op(), quench_runtime::bytecode::Op::GetField);
        let parameter = callee_body.parameter_registers.or_else(|| callee_body.promoted_register(0)).unwrap();
        let mut recovered = Value::UNDEFINED;
        let scratch = usize::from(method.inlined[frame].base - method.caller_registers());
        Vm::scratch_in_lane(scratch, usize::from(callee_body.registers), |register, value| {
            if register == parameter { recovered = value; }
        });
        assert_eq!(recovered, Value::number(7.0), "recovery retains the changed reflected parameter");
        let atom = vm.intern_atom("calls");
        assert_eq!(vm.own_property(receiver, atom), Some(Value::number(2001.0)), "store runs exactly once before recovery");
    }
}

#[test]
fn published_native_calls_retire_weak_callees_at_lifecycle_edges() {
    let program = Engine::specialize(r#"
      globalThis.receiver = {
        offset: 1,
        add(k) { return k + this.offset; },
        other(k) { return k + this.offset + 1; },
        step(k) { return this.add(k) + 3; }
      };
      for (let i = 0; i < 40; i++) receiver.step(i);
    "#, "published-native-calls.js").unwrap();
    for transition in ["collection", "realm", "reset"] {
        let mut vm = quench_runtime::test_support::vm(TestHost::default());
        quench_runtime::test_support::execute(&mut vm, &program).unwrap();
        let atom = vm.intern_atom("receiver");
        let receiver = vm.own_property(vm.realm.globals, atom).unwrap();
        let method_of = |vm: &mut Vm, name: &str| { let atom = vm.intern_atom(name); vm.get_property(&program, receiver, atom).unwrap() };
        let callee = method_of(&mut vm, "add");
        let caller = method_of(&mut vm, "step");
        let quench_runtime::heap::FunctionKind::User(_, caller_id) = vm.heap.function(caller).unwrap().kind else { panic!("caller") };
        let quench_runtime::heap::FunctionKind::User(_, callee_id) = vm.heap.function(callee).unwrap().kind else { panic!("callee") };
        let body = program.function(caller_id as usize);
        let call = body.instructions().position(|instruction| instruction.op() == quench_runtime::bytecode::Op::Call).unwrap();
        let mut targets = vec![None; body.code.len()];
        let other = method_of(&mut vm, "other");
        let quench_runtime::heap::FunctionKind::User(_, other_id) = vm.heap.function(other).unwrap().kind else { panic!("other") };
        targets[call] = Some(quench_runtime::vm::engine_types::CallTarget { callee: quench_runtime::vm::engine_types::CalleeBinding::Identity(callee), function: other_id });
        assert!(vm.compile_lane_method(&program, caller_id, &|_| (0, 0), Vec::new()).is_none(),
            "publication must reject a callee paired with the wrong function body");
        targets[call] = Some(quench_runtime::vm::engine_types::CallTarget { callee: quench_runtime::vm::engine_types::CalleeBinding::Identity(callee), function: callee_id });
        let method = vm.compile_lane_method(&program, caller_id, &|_| (0, 0), Vec::new()).unwrap();
        assert_eq!(method.inlined.len(), 1);
        let published = { let (mut facts, programs) = vm.fact_watch(); programs.publish_lane_method(method, &mut facts) }.unwrap();
        vm.collect_now(&program);
        assert!(published.lease().is_some(), "a live callee binding survives collection");
        match transition {
            "collection" => {
                // A second publication names a function nothing else reaches: its code
                // roots the callee, so a full collection keeps the locator its guard compares.
                let orphan = vm.heap.alloc_function(vm.heap.function(callee).unwrap().clone());
                targets[call] = Some(quench_runtime::vm::engine_types::CallTarget { callee: quench_runtime::vm::engine_types::CalleeBinding::Identity(orphan), function: callee_id });
                let method = vm.compile_lane_method(&program, caller_id, &|_| (0, 0), Vec::new()).unwrap();
                let rooted = { let (mut facts, programs) = vm.fact_watch(); programs.publish_lane_method(method, &mut facts) }.unwrap();
                vm.collect_now(&program);
                assert!(vm.heap.is_live(orphan), "published code keeps the callee its guard names alive");
                assert!(rooted.lease().is_some() && published.lease().is_some());
                let replacement = vm.heap.alloc_function(vm.heap.function(callee).unwrap().clone());
                assert_ne!(replacement, orphan, "a named callee's locator is not reused, so the guard tells the new function apart");
                rooted.retire();
                vm.collect_now(&program);
                assert!(!vm.heap.is_live(orphan), "retiring the code drops its roots");
                assert!(published.lease().is_some(), "an unrelated live binding is retained");
            }
            "realm" => {
                let foreign = vm.create_realm_global(&program).unwrap();
                vm.switch_realm_global(foreign);
                assert!(published.lease().is_none());
            }
            "reset" => {
                vm.initialize(&program).unwrap();
                assert!(published.lease().is_none());
            }
            _ => unreachable!(),
        }
    }
}

//! Language-subset regressions, once an integration test binary that compiled its own VM.

use crate::test_host::TestHost;
use crate::{Engine, Runtime};

fn output(source: &str) -> Vec<String> {
    let host = TestHost::default();
    let view = host.clone();
    let program = Engine::specialize(source, "test.js").unwrap();
    Runtime::new(host).execute(&program).unwrap();
    view.lines().clone()
}

#[test]
fn nested_super_initialization_preserves_caller_receivers() {
    let source = r#"
      (() => {
        class Base {}
        class Derived extends Base {}
        const make = () => new Derived();
        class Caller {
          constructor() { this.value = 42; }
          read(reason = make()) { return this.value; }
        }
        print(new Caller().read());
        class Captured extends Base {
          constructor() {
            const read = () => this;
            super();
            print(read() === this);
          }
        }
        new Captured();
      })();
    "#;
    for generic in [false, true] {
        let host = TestHost::default();
        let program = if generic {
            Engine::specialize_unspecialized(source, "nested-super.js")
        } else {
            Engine::specialize(source, "nested-super.js")
        }
        .unwrap();
        Runtime::new(host.clone()).execute(&program).unwrap();
        assert_eq!(*host.lines(), ["42", "true"]);
    }
}

#[test]
fn residual_binary_round_trip_preserves_execution() {
    let path =
        std::env::temp_dir().join(format!("quench-roundtrip-{}.residual", std::process::id()));
    let program = Engine::specialize(
        "print(12345678901234567890n); class C { value = () => eval('C'); } print(new C().value() === C); var optional={x:7,m(){return this.x;}}; print((optional?.m)()); print(delete optional?.x && optional.x===undefined); print(null?.[\"m\"]()); function own() { { let y = 7; return eval('y'); } } print(own()); function scope() { var a=1; with({a:8}) { let a=2; return eval('a+1'); } } print(scope()); function captured() { var f; with({a:8}) { let a=2; f=function(){ return eval('a+1'); }; } return f(); } print(captured()); function deletion(){var o={x:8};with(o){let x=2;return delete x;}} print(deletion()); var w={shared:'object'};with(w){print(typeof this.shared);function shared(){return 9;}}print(w.shared);print(shared()); function skipped(){switch(1){case 2:function f(){return 2;}}return typeof f;}print(skipped()); function caught(){var result;try{throw 8;}catch(x){eval('var x=42');result=x;}return [result,typeof x];}print(JSON.stringify(caught())); function recreated(){eval('delete q;{function q(){return 42}}');return q();}print(recreated()); function collision(){eval('{function q(){return 42}}');const q=9;return q;}print(collision());",
        "roundtrip.js",
    ).unwrap();
    assert!(program.disassemble().contains("StoreVarBinding"));
    program.write_binary(&path).unwrap();
    let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let host = TestHost::default();
    let view = host.clone();
    Runtime::new(host).execute(&decoded).unwrap();
    assert_eq!(
        view.lines().clone(),
        [
            "12345678901234567890",
            "true",
            "7",
            "true",
            "undefined",
            "7",
            "3",
            "3",
            "false",
            "undefined",
            "object",
            "9",
            "undefined",
            "[42,\"undefined\"]",
            "42",
            "9"
        ]
    );
}

#[test]
fn deterministic_hash_random_preserves_es5_bitwise_semantics() {
    let source = r#"
      var seed = 49734321;
      function random() {
        seed = ((seed + 0x7ed55d16) + (seed << 12)) & 0xffffffff;
        seed = ((seed ^ 0xc761c23c) ^ (seed >>> 19)) & 0xffffffff;
        seed = ((seed + 0x165667b1) + (seed << 5)) & 0xffffffff;
        seed = ((seed + 0xd3a2646c) ^ (seed << 9)) & 0xffffffff;
        seed = ((seed + 0xfd7046c5) + (seed << 3)) & 0xffffffff;
        seed = ((seed ^ 0xb55a4f09) ^ (seed >>> 16)) & 0xffffffff;
        return (seed & 0xfffffff) / 0x10000000;
      }
      print(random()); print(random()); print(random());
    "#;
    assert_eq!(
        output(source),
        [
            "0.9872818551957607",
            "0.34880331158638",
            "0.5631933622062206"
        ]
    );
}

#[test]
fn returned_superinstruction_object_preserves_value() {
    let source = r#"
      function build(key) { return { items: [1, 2], text: 'x' + key + 'y' }; }
      var value = build(3);
      print(value.items[1]); print(value.text);
    "#;
    let program = Engine::specialize(source, "super-return.js").unwrap();
    assert!(program.disassemble().contains("SuperConstArrayObject2"));
    assert_eq!(output(source), ["2", "x3y"]);
}

#[test]
fn global_function_calls_remain_generic_without_a_static_binding_proof() {
    let direct = Engine::specialize(
        "function target() { return 42; } function caller() { return target(); } print(caller());",
        "direct-call.js",
    )
    .unwrap();
    // Global declarations are writable properties, not immutable callee proofs.
    // The current compiler emits the general call for this binding domain.
    assert!(!direct.disassemble().contains("CallKnown"));
    assert_eq!(
        output(
            "function target(){return 42;} function caller(){return target();} print(caller());"
        ),
        ["42"]
    );
    assert_eq!(
        output(
            "function f(){return 1;} function g(){return f();} f=function(){return 2;}; print(g());"
        ),
        ["2"]
    );
}

#[test]
fn numeric_dispatch_class_is_data_derived_and_semantics_neutral() {
    let body = r#"
      function dense(a, b) {
        var x = a + b;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        return x;
      }
      print(dense(2, 3));
    "#;
    let edited = format!("var irrelevant = 1; {body}");
    for source in [body, edited.as_str()] {
        let program = Engine::specialize(source, "dispatch-class.js").unwrap();
        assert!(
            program
                .disassemble()
                .lines()
                .any(|line| line.contains(" dense ") && line.ends_with("dispatch=Numeric"))
        );
        assert_eq!(output(source), ["258280325"]);
    }
    // These generated functions exercise the Numeric execution view's binding
    // checks, which guest conformance inputs cannot require a compiler to select.
    let arithmetic = std::iter::repeat_n("z", 50).collect::<Vec<_>>().join("+");
    for (index, binding) in ["let a = (a &&= 0);", "a = 0; let a;"]
        .into_iter()
        .enumerate()
    {
        let source = format!(
            "function tdz() {{ var z=1; var n={arithmetic}; {binding} }} \
             try {{ tdz(); print(false); }} catch(e) {{ print(e instanceof ReferenceError); }}"
        );
        let program = Engine::specialize(&source, "numeric-tdz.js").unwrap();
        assert!(
            program
                .disassemble()
                .lines()
                .any(|line| { line.contains(" tdz ") && line.ends_with("dispatch=Numeric") })
        );
        let path = std::env::temp_dir().join(format!(
            "quench-numeric-tdz-{}-{index}.residual",
            std::process::id()
        ));
        program.write_binary(&path).unwrap();
        let decoded = crate::ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let generic = Engine::specialize_unspecialized(&source, "numeric-tdz.js").unwrap();
        for program in [&program, &decoded, &generic] {
            let host = TestHost::default();
            let view = host.clone();
            Runtime::new(host).execute(program).unwrap();
            assert_eq!(view.lines().as_slice(), ["true"]);
        }
    }
}

#[test]
fn numeric_method_arguments_flow_directly_from_caller_registers() {
    let source = r#"
      function Holder() {}
      Holder.prototype.dense = function(a, b) {
        var x = a + b;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        x = x * b; x = x + a; x = x * b; x = x + a;
        return x;
      };
      var holder = new Holder();
      print(holder.dense(2, 3));
    "#;
    let program = Engine::specialize(source, "numeric-method.js").unwrap();
    assert!(
        program
            .disassemble()
            .lines()
            .any(|line| line.ends_with("dispatch=Numeric"))
    );
    assert_eq!(output(source), ["258280325"]);
}

#[test]
fn native_stack_scopes_are_ordered_truthful_and_unwind_safe() {
    let program = Engine::specialize(
        "Error.prepareStackTrace = (error, sites) => JSON.stringify(sites.map(site => [site.getFunctionName(), site.isNative(), site.getFileName(), site.getLineNumber(), site.getColumnNumber()])); function capture() { const error = new Error('scope'); return error.stack; }",
        "native-stack.js",
    ).unwrap();
    let mut runtime = Runtime::new(TestHost::default());
    runtime.execute(&program).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("capture");
    let capture = runtime.get_property_rooted(global, key).unwrap();
    let undefined = runtime.root(crate::Value::UNDEFINED);
    let result = runtime
        .with_native_stack_frame("outer", |runtime| {
            runtime.with_native_stack_frame("inner", |runtime| {
                runtime.call_rooted(capture, undefined, &[])
            })
        })
        .unwrap();
    let text = runtime.string_text_rooted(result).unwrap().unwrap();
    assert!(
        text.ends_with("[\"inner\",true,null,null,null],[\"outer\",true,null,null,null]]"),
        "{text}"
    );
    runtime.release_root(result);
    let panic = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runtime.with_native_stack_frame("panicking", |_| panic!("native scope unwind"));
    }));
    assert!(panic.is_err());
    let result = runtime.call_rooted(capture, undefined, &[]).unwrap();
    let text = runtime.string_text_rooted(result).unwrap().unwrap();
    assert!(!text.contains("outer") && !text.contains("inner") && !text.contains("panicking"));
    runtime.release_root(result);
    runtime.release_root(capture);
    runtime.release_root(undefined);
    runtime.release_root(key);
    runtime.release_root(global);
}

#[test]
fn throw_preserves_identity_without_hooks_and_retains_source_through_binary() {
    const SOURCE: &str = "let touches=0;\nconst marker={get message(){touches++;throw 1},toString(){touches++;throw 2}};\nfunction handled(){throw marker}\ntry{handled()}catch(error){if(error!==marker||touches!==0)throw 'wrong throw effects'}\nfunction escaping(){\n  throw new Error('snapshot');\n}\nescaping();";
    for compile in [
        Engine::specialize as fn(&str, &str) -> _,
        Engine::specialize_unspecialized,
    ] {
        let program = compile(SOURCE, "throw-snapshot.js").unwrap();
        let mut runtime = Runtime::new(TestHost::default());
        let error = runtime.execute(&program).unwrap_err();
        let (file, line, column, text) = error.source_location().expect("escaping throw location");
        assert_eq!(
            (file, line, text),
            (
                "throw-snapshot.js",
                6,
                Some("  throw new Error('snapshot');")
            )
        );
        assert!(column > 0 && column as usize <= text.unwrap().encode_utf16().count() + 1);
        let expected = (file.to_owned(), line, column, text.map(str::to_owned));
        let path =
            std::env::temp_dir().join(format!("quench-throw-snapshot-{}.qbc", std::process::id()));
        program.write_binary(&path).unwrap();
        let restored = crate::ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let error = runtime.execute(&restored).unwrap_err();
        let (file, line, column, text) = error.source_location().unwrap();
        assert_eq!(
            (file.to_owned(), line, column, text.map(str::to_owned)),
            expected
        );
    }
}

#[test]
fn with_body_names_reach_enclosing_function_vars() {
    assert_eq!(
        output(
            "function f(o) { var x = 42; function inner(o) { with (o) { return x; } } return inner(o); } print(f({})); print(f({ x: 7 }));"
        ),
        ["42", "7"]
    );
}

#[test]
fn direct_eval_expressions_resolve_caller_bindings() {
    assert_eq!(
        output(
            "function local() { var q = 2; return eval('typeof q'); } print(local()); function declared() { eval('var C = function () { this.a = 1; }'); return eval('new C().a'); } print(declared()); function nested() { return eval(\"eval('var r = 2;'); typeof r\"); } print(nested());"
        ),
        ["number", "1", "number"]
    );
}

#[test]
fn direct_eval_binary_split_respects_looser_operators() {
    assert_eq!(
        output(
            "function f() { var q = 2; return [eval(\"q === 2 ? 'y' : 'n'\"), eval(\"q === 2, 'last'\"), eval(\"null ?? q === 2\")].join(); } print(f());"
        ),
        ["y,last,true"]
    );
}

#[test]
fn case_mapping_preserves_lone_surrogates() {
    assert_eq!(
        output(
            "var s = 'a\\ud800b\\udc00\\ud83d\\ude00\\u03a3'; print(Array.from(s.toUpperCase(), function (c) { return c.codePointAt(0).toString(16); }).join()); print(Array.from(s.toLowerCase(), function (c) { return c.codePointAt(0).toString(16); }).join());"
        ),
        ["41,d800,42,dc00,1f600,3a3", "61,d800,62,dc00,1f600,3c3"]
    );
}

#[test]
fn nested_class_in_computed_key_keeps_outer_private_names() {
    assert_eq!(
        output(
            "let read; class A { static #x = 42; static [(class {}, read = () => A.#x, 'k')] = 1; } print(read()); for (let i = 0; i < 2; i++) { let f; class B { static #y = i; static [(class {}, f = () => B.#y, 'k')]; } print(f()); }"
        ),
        ["42", "0", "1"]
    );
}

#[test]
fn generator_return_inside_loop_head_pattern_closes_pattern_iterator() {
    assert_eq!(
        output("var log = []; function source(name) { return { [Symbol.iterator]() { return { next() { log.push(name + ':next'); return { done: false }; }, return() { log.push(name + ':return'); return {}; } }; } }; } function* plain() { for ([ {} = yield ] of [source('a')]) {} } var g = plain(); g.next(); print(JSON.stringify(g.return(7)) + ' ' + log.join()); log = []; function* guarded() { try { for (var [ x = yield ] of [source('b')]) {} } finally { log.push('finally'); } } g = guarded(); g.next(); print(JSON.stringify(g.return(8)) + ' ' + log.join()); "),
        [
            "{\"value\":7,\"done\":true} a:next,a:return",
            "{\"value\":8,\"done\":true} b:next,b:return,finally"
        ]
    );
}

#[test]
fn nested_direct_eval_vars_belong_to_the_calling_function() {
    assert_eq!(
        output("function u(code) { return eval(code); } u(\"eval('var z = 2;')\"); function v() { eval(\"eval('var y = 3;')\"); return typeof y; } function w() { 'use strict'; eval(\"eval('var x = 1;')\"); return typeof x; } print([typeof z, 'z' in globalThis, v(), typeof y, w()].join()); "),
        ["undefined,false,number,undefined,undefined"]
    );
}

#[test]
fn private_members_resolve_on_branded_proxy_receivers() {
    assert_eq!(
        output("class Base { constructor() { return new Proxy(this, {}); } } class F extends Base { #x = 1; t() { return this.#x; } } class M extends Base { #m() { return 2; } t() { return this.#m(); } } class G extends Base { get #g() { return 3; } t() { return this.#g; } } print([F, M, G].map(C => new C().t()).join(' '));"),
        ["1 2 3"]
    );
}

#[test]
fn shared_atomics_round_trip_negative_bigints() {
    assert_eq!(
        output("const view = new BigInt64Array(new SharedArrayBuffer(16)); view[0] = -5n; const old = Atomics.add(view, 0, 2n); print([old, view[0], Atomics.exchange(view, 1, -1n), view[1]].join());"),
        ["-5,-3,0,-1"]
    );
}

/// A register read only by the catch handler stays live across an `await` that resumes by
/// throwing into it.
#[test]
fn awaited_rejections_reach_handlers_with_their_registers() {
    assert_eq!(
        output("async function f() { var err = {}; try { await Promise.reject(err); } catch (e) { print(e === err); } } f();"),
        ["true"]
    );
}

/// IteratorClose looks `return` up through a Proxy iterator's `get` trap.
#[test]
fn proxy_iterators_close_through_their_get_trap() {
    assert_eq!(
        output("const log = []; const it = new Proxy({ next() { return { done: false, value: 1 }; } }, { get(t, k) { log.push(String(k)); return t[k]; } }); const [a] = { [Symbol.iterator]() { return it; } }; print(log.join());"),
        ["next,return"]
    );
}

/// `@@split` converts an object limit after constructing its splitter, which observes a
/// prototype `exec` the conversion installs.
#[test]
fn regexp_split_observes_exec_replaced_by_its_limit() {
    assert_eq!(
        output("const r = /a/[Symbol.split]('abba', { valueOf() { RegExp.prototype.exec = () => null; return 100; } }); print(r.length);"),
        ["1"]
    );
}

/// A `var` read before any write is `undefined`, even when its frame slot moved into a
/// register of a reused frame (TOD-140: TypeScript's `(function (E) {...})(E || (E = {}))`).
#[test]
fn locals_read_before_their_first_write_are_undefined_in_reused_frames() {
    assert_eq!(
        output(
            "function junk() { var a = 'j1', b = 'j2', c = 'j3', d = 'j4', e = 'j5', h = 'j6', i = 'j7', k = 'j8', m = 'j9'; return [a, b, c, d, e, h, i, k, m].join(); }
             function g() { var A, B; f(A || (A = 1)); function f(x) { return x; } return [A, B, typeof B].join(); }
             function maybe() { var x; if (junk() === '') x = 1; return typeof x; }
             function assigned() { var y; y = 2; return y; }
             junk(); print(g()); junk(); print(maybe()); print(assigned());"
        ),
        ["1,,undefined", "undefined", "2"]
    );
}

/// A strict assignment to a name no scope binds statically resolves it before the right
/// side runs (`needs_strict_global_reference_capture`); that resolution meets only bindings
/// a `with` or `eval` makes, so it leaves the enclosing function's uncaptured locals in
/// registers (fflate's module body kept every local in its environment for one such
/// `onmessage = ...`), while `with` and `eval` bindings stay reachable.
#[test]
fn strict_global_assignment_keeps_enclosing_locals_in_registers() {
    let layout = "(function () { 'use strict'; for (var i = 0; i < 3; ++i) {} function g() { undeclared = 1; } return g; })";
    let program = Engine::specialize(layout, "strict-global.js").unwrap();
    let outer = program.disassemble();
    let outer = outer.split("\nfunction 2 ").next().unwrap().split("\nfunction 1 ").nth(1).unwrap();
    assert!(!outer.contains("EnvLocal"), "{outer}");
    assert_eq!(
        output("(function () { 'use strict'; for (var i = 0; i < 3; ++i) {} function g() { undeclared = 1; } try { g(); } catch (e) { print(e.constructor.name + ':' + i); } })();"),
        ["ReferenceError:3"]
    );
    assert_eq!(
        output(
            "function a() { eval('var z = 1'); var local = 7; function g() { 'use strict'; z = local + 2; } g(); return z; } print(a());
             var o = { w: 1 }; with (o) { (function () { 'use strict'; w = 3; })(); } print(o.w);
             function b() { var keep = 1; function c() { eval('var m = 4'); function d() { 'use strict'; m = keep + 10; } d(); return m; } return c(); } print(b());
             print(typeof z + typeof w + typeof m);"
        ),
        ["9", "3", "11", "undefinedundefinedundefined"]
    );
}

#[test]
fn closure_free_parameter_lists_binding_apart_drop_unobservable_environment_copies() {
    // A list with defaults or patterns binds its names apart from the argument slots; with
    // no closure in the activation, its block and per-iteration environment copies are
    // unobservable, as they are for a simple list, so its locals move into registers.
    let source = r#"
        function f(e, t = !1) { if (e) { const n = t ? 1 : 2; return n; } for (let i = 0; i < 3; i++) { let k = i * 2; e = k; } return e; }
        print([f(1), f(1, true), f(0)].join());
        function g(a, { b } = { b: 5 }) { let s = 0; for (const x of [a, b]) { let y = x; s += y; } try { let z = s; throw z; } catch (q) { return q + arguments.length; } }
        print([g(1), g(1, { b: 2 })].join());
        function h(a = 1) { { let a2 = a; a = 7; } return a + typeof arguments[0]; }
        print([h(), h(3)].join());
    "#;
    let program = Engine::specialize(source, "parameters-apart.js").unwrap();
    program.compile_released();
    let f = &program.functions[1];
    assert!(f.instructions().all(|instruction| instruction.op() != crate::bytecode::Op::CloneEnv));
    program.validate().unwrap();
    assert_eq!(output(source), ["2,1,4", "7,5", "7undefined,7number"]);
}

#[test]
fn closure_making_arrows_run_as_environment_activations_with_their_lexical_this() {
    // An arrow that makes closures runs as an environment activation
    // (`CallPlan::environment_activation`) whose `this` is its environment's; its
    // closures find that `this`, its captured parameters and locals as a frame's would,
    // and a throw or a recursive call leaves the activation as the frame it would be.
    let source = r#"
const out = [];
const obj = {
  v: 3,
  run() {
    const make = (k) => () => this.v + k;
    const fs = [];
    for (let i = 0; i < 4; i++) fs.push(make(i));
    return fs.map((f) => f()).join();
  },
  nested() {
    const outer = (a) => { const inner = (b) => (c) => this.v * a + b + c; return inner(a + 1); };
    return outer(2)(5);
  },
};
out.push(obj.run(), obj.nested());
// An arrow making a closure that escapes and mutates a captured parameter.
const counter = (n) => { const inc = () => ++n; const get = () => n; return [inc, get]; };
const [inc, get] = counter(10); inc(); inc(); out.push(get());
// Throws through an environment arrow.
const thrower = (x) => { const f = () => { throw new Error("e" + x); }; return f(); };
try { thrower(7); } catch (e) { out.push(e.message); }
// Recursion.
const fib = (n) => { const lazy = () => n; return n < 2 ? lazy() : fib(n - 1) + fib(n - 2); };
out.push(fib(15));
// IIFE named function expression inside an arrow, as minified code writes.
const w = (r) => function worker(e) { return e * 2 + r; }(r + 1);
let s = 0; for (let i = 0; i < 1000; i++) s += w(i); out.push(s);
// Arrow in a derived constructor after super().
class A { constructor() { this.a = 1; } }
class B extends A { constructor() { super(); const g = () => () => this.a + 1; this.b = g()(); } }
out.push(new B().b);
// Sloppy top-level arrow this.
const top = () => { const h = () => typeof this; return h(); };
out.push(top());
// Arrow reading arguments of its enclosing function.
function outerArgs() { const a = () => { const b = () => arguments.length; return b(); }; return a(); }
out.push(outerArgs(1, 2, 3));
// Many calls in a hot loop with a captured TDZ local.
const tdz = (x) => { let y = x; const r = () => y; y += 1; return r(); };
let t = 0; for (let i = 0; i < 5000; i++) t += tdz(i); out.push(t);
print(out.join(" "));
"#;
    assert_eq!(output(source), ["3,4,5,6 14 12 e7 610 1500500 2 object 3 12502500"]);
}

#[test]
fn closures_of_parameter_lists_binding_apart_split_their_activation() {
    // A list with defaults or patterns keeps each argument in a hidden slot no closure
    // names, so a closure-making body splits its activation as a simple list's does
    // (`selective_layout_is_safe`): only captured bindings live in its environment.
    let source = r#"
const out = [];
function g(e, t = 3) { let k = e + t; const f = () => k; return f(); }
out.push(g(1), g(1, 10));
function q(a, { b, c = a } = {}) { const add = (x) => x + a + b + c; return add(1); }
out.push(q(1, { b: 2 }), q(5, { b: 1, c: 0 }));
function cnt(n = 0, step = 1) { const fs = []; for (let i = 0; i < 3; i++) { fs.push(() => n + i * step); } return fs.map((f) => f()).join(); }
out.push(cnt(), cnt(10, 5));
function argsOf(a, b = 2) { const f = () => arguments.length + a + b; a = 100; return [f(), arguments[0]].join(); }
out.push(argsOf(1), argsOf(1, 1, 1));
function thr(a, b = a * 2) { const f = () => { throw b; }; try { f(); } catch (e) { return e + a; } }
out.push(thr(4));
function rec(n, acc = []) { const push = () => acc.push(n); push(); return n ? rec(n - 1, acc) : acc.join(""); }
out.push(rec(5));
function hot(x, y = 1) { const m = () => x * y; return m(); }
let s = 0; for (let i = 0; i < 20000; i++) s += hot(i, 2); out.push(s);
function tdz(a = 1) { const f = () => z; let r; try { r = f(); } catch (e) { r = e.constructor.name; } let z = a; return r + f(); }
out.push(tdz());
print(out.join(" "));
"#;
    let program = Engine::specialize(source, "parameters-apart-split.js").unwrap();
    program.compile_released();
    assert_eq!(program.functions[1].selective_capture_slots.as_deref(), Some(&[4][..]));
    program.validate().unwrap();
    assert_eq!(output(source), ["4 11 5 7 0,1,2 10,15,20 103,1 104,1 12 543210 399980000 ReferenceError1"]);
}

#[test]
fn subclasses_sharing_a_parent_constructor_keep_an_add_entry_per_prototype_chain() {
    // Receivers of one shape under different prototypes reach the parent's stores; each
    // chain keeps its own add entry (`FieldAddCacheSet::find_for`), and a setter a later
    // prototype gains still runs for the store it intercepts.
    let source = r#"
class Base { constructor(k, p, e) { this.pos = p; this.end = e; this.kind = k; this.id = 0; this.parent = void 0; } }
class A extends Base { constructor(k, p, e) { super(k, p, e); } a() { return 1; } }
class B extends Base { constructor(k, p, e) { super(k, p, e); } b() { return 2; } b2() {} }
class C extends Base { constructor(k, p, e) { super(k, p, e); } }
let s = 0, keys = "";
for (let i = 0; i < 3000; i++) {
  const o = i % 3 === 0 ? new A(i, 1, 2) : i % 3 === 1 ? new B(i, 3, 4) : new C(i, 5, 6);
  s += o.pos + o.end + o.kind + (o.a ? o.a() : 0) + (o.b ? o.b() : 0);
  if (i < 3) keys += Object.keys(o).join(",") + ":" + Object.getPrototypeOf(o).constructor.name + ";";
}
// A prototype that gains a setter for a later field must see the store run it.
let seen = 0;
Object.defineProperty(B.prototype, "id", { set(v) { seen++; }, configurable: true });
const late = new B(1, 2, 3);
print(s + " " + keys + " " + seen + " " + Object.keys(late).join(","));
"#;
    assert_eq!(output(source), ["4522500 pos,end,kind,id,parent:A;pos,end,kind,id,parent:B;pos,end,kind,id,parent:C; 1 pos,end,kind,parent"]);
}

#[test]
fn environment_activations_drop_extra_arguments_nothing_observes() {
    // A strict callee that makes closures runs as an environment activation called with
    // more values than its parameters; without legacy reflection or an `arguments` read
    // nothing observes the extra ones, and one reading `arguments` keeps them.
    let source = r#"
"use strict";
const out = [];
function each(list, f) { let acc = 0; for (let i = 0; i < list.length; i++) acc += f(list[i], i, "x"); return acc; }
function one(e) { const g = () => e * 2; return g(); }
out.push(each([1, 2, 3, 4], one));
function twoArgs(e, t) { let k = e; const g = () => k + (t === undefined ? 100 : t); return g(); }
out.push(each([1, 2], twoArgs), twoArgs(5));
function counts(e) { const g = () => arguments.length + e; return g(); }
out.push(each([1, 2], counts));
print(out.join(" "));
"#;
    assert_eq!(output(source), ["20 4 105 9"]);
    let sloppy = "function sloppy(e) { const g = () => e; g(); return sloppy.arguments.length; } function drive(f) { let s = 0; for (let i = 0; i < 50; i++) s += f(i, 1, 2); return s; } print(drive(sloppy));";
    assert_eq!(output(sloppy), ["150"]);
}

#[test]
fn proved_promise_species_follows_constructor_and_species_changes() {
    // `then` answers SpeciesConstructor from a proof of where `constructor` and `@@species`
    // were found (`PromiseSpeciesProof`); reassigning either, an own `constructor` and a
    // subclass each take the spec's reads again.
    let source = r#"
const log = [];
function chain(n) { let p = Promise.resolve(0); for (let i = 0; i < n; i++) p = p.then((v) => v + 1); return p; }
(async () => {
  log.push(await chain(50));
  const original = Promise.prototype.constructor;
  class Tracked extends Promise { constructor(f) { log.push("ctor"); super(f); } }
  Promise.prototype.constructor = Tracked;
  const t = Promise.resolve(1).then((v) => v);
  log.push(t instanceof Tracked);
  Promise.prototype.constructor = original;
  log.push(Promise.resolve(1).then((v) => v) instanceof Tracked);
  const species = Object.getOwnPropertyDescriptor(Promise, Symbol.species);
  Object.defineProperty(Promise, Symbol.species, { get() { log.push("species"); return Tracked; }, configurable: true });
  log.push(Promise.resolve(2).then((v) => v) instanceof Tracked);
  Object.defineProperty(Promise, Symbol.species, species);
  log.push(Promise.resolve(2).then((v) => v) instanceof Tracked);
  const own = Promise.resolve(3); own.constructor = Tracked;
  log.push(own.then((v) => v) instanceof Tracked);
  const sub = Tracked.resolve(4);
  log.push(sub.then((v) => v) instanceof Tracked);
  log.push(await chain(10));
  print(log.join(" "));
})();
"#;
    assert_eq!(output(source), ["50 ctor true false species ctor true false ctor true ctor ctor true 10"]);
}

#[test]
fn generators_resumed_in_place_keep_iteration_semantics() {
    // A generator's `next()` from a frame or a lane iteration step resumes its frame in place
    // (`Vm::try_in_loop_generator_step`), its `yield` and return completing the call: for-of,
    // explicit steps, nesting, delegation, throws, `finally` on an early exit, a re-entrant
    // step, closures over its locals, `arguments`, `with` and a resumed value.
    let source = r#"
const out = [];
function* range(n) { for (let i = 0; i < n; i++) yield i; }
let s = 0; for (const v of range(1000)) s += v; out.push(s);
const g = range(3); out.push(JSON.stringify([g.next(), g.next(), g.next(), g.next(), g.next()]));
function* nested() { for (const a of range(3)) for (const b of range(2)) yield a * 10 + b; }
out.push([...nested()].join());
function* delegating() { yield 1; const r = yield* inner(); yield r; }
function* inner() { yield 2; yield 3; return 4; }
out.push([...delegating()].join());
function* thrower() { yield 1; throw new Error("boom"); }
try { for (const v of thrower()) out.push("t" + v); } catch (e) { out.push(e.message); }
const t = thrower(); t.next(); try { t.next(); } catch (e) { out.push("caught " + e.message); } out.push(JSON.stringify(t.next()));
function* withFinally() { try { yield 1; yield 2; } finally { out.push("finally"); } }
for (const v of withFinally()) { out.push("f" + v); break; }
function* selfCall() { yield 1; try { me.next(); } catch (e) { out.push(e.constructor.name); } yield 2; }
const me = selfCall(); out.push([...me].join());
function* counter() { let c = 0; const read = () => c; while (true) { c++; const got = yield read; if (got === "stop") return c; } }
const cg = counter(); const r1 = cg.next().value; cg.next(); cg.next(); out.push(r1(), JSON.stringify(cg.next("stop")));
function* args() { yield arguments.length; yield this === undefined || this === globalThis; }
out.push([...args(1, 2, 3)].join());
function* withScope() { const o = { x: 7 }; with (o) { yield x; x = 8; yield x; } }
out.push([...withScope()].join());
let deep = 0; function* rec(n) { if (n > 0) yield* rec(n - 1); yield n; } for (const v of rec(10)) deep += v; out.push(deep);
const objGen = { *[Symbol.iterator]() { yield* [5, 6]; } }; out.push([...objGen].join());
print(out.join(" "));
"#;
    assert_eq!(
        output(source),
        ["499500 [{\"value\":0,\"done\":false},{\"value\":1,\"done\":false},{\"value\":2,\"done\":false},{\"done\":true},{\"done\":true}] 0,1,10,11,20,21 1,2,3,4 t1 boom caught boom {\"done\":true} f1 finally TypeError 1,2 3 {\"value\":3,\"done\":true} 3,true 7,8 55 5,6"]
    );
    let resumed = r#"
"use strict";
function* g(){ const x = yield 1; yield x * 2; }
const it = g(); const a = it.next(); const b = it.next(21); const c = it.next(); print(JSON.stringify([a, b, c]));
"#;
    assert_eq!(output(resumed), ["[{\"value\":1,\"done\":false},{\"value\":42,\"done\":false},{\"done\":true}]"]);
}

#[test]
fn typed_array_set_between_numeric_kinds_converts_each_element() {
    // `set` from a typed array of another numeric kind converts on the bytes
    // (`Vm::typed_array_convert_numbers`): every kind pair over boundary values, overlapping
    // views of one buffer, which read the source before writing, and a source too large.
    let source = r#"
const kinds = [Int8Array, Uint8Array, Uint8ClampedArray, Int16Array, Uint16Array, Int32Array, Uint32Array, Float32Array, Float64Array, typeof Float16Array === "function" ? Float16Array : Float32Array];
const values = [0, -0, 1, -1, 127, 128, 255, 256, -129, 65535, 65536, 2147483647, -2147483648, 4294967295, 1.5, -1.5, 2.5, 0.49999, NaN, Infinity, -Infinity, 1e10, 3.4e38, 65504.5];
const out = [];
for (const S of kinds) for (const T of kinds) {
  const src = new S(values.length); for (let i = 0; i < values.length; i++) src[i] = values[i];
  const dst = new T(values.length + 2); dst.set(src, 1);
  out.push(Array.from(dst, (v) => Object.is(v, -0) ? "-0" : String(v)).join(","));
}
// Overlap: views of one buffer with different kinds.
const buf = new ArrayBuffer(64); const u8 = new Uint8Array(buf); for (let i = 0; i < 64; i++) u8[i] = i * 7;
const u16 = new Uint16Array(buf, 8, 8); u8.set(u16, 4); out.push(Array.from(u8).join(","));
const f32 = new Float32Array(buf, 0, 8); const i8 = new Int8Array(buf, 2, 8); f32.set(i8); out.push(Array.from(f32).join(","));
try { new Uint8Array(2).set(new Uint8Array(4)); } catch (e) { out.push(e.constructor.name); }
let h = 0; for (const s of out) for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0;
print(out.length + " " + h + " " + out[out.length - 3].slice(0, 60));
"#;
    assert_eq!(output(source), ["103 -1900204906 0,7,14,21,56,70,84,98,112,126,140,154,84,91,98,105,112,119,1"]);
}

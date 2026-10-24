use quench_runtime::{Engine, Host, Runtime};
use std::{cell::RefCell, rc::Rc};

#[derive(Clone, Default)]
struct Capture(Rc<RefCell<Vec<String>>>);
impl Host for Capture {
    fn write_line(&mut self, text: &str) {
        self.0.borrow_mut().push(text.into());
    }
    fn clock_millis(&mut self) -> f64 {
        0.0
    }
}

fn output(source: &str) -> Vec<String> {
    let host = Capture::default();
    let view = host.clone();
    let program = Engine::specialize(source, "test.js").unwrap();
    Runtime::new(host).execute(&program).unwrap();
    Rc::try_unwrap(view.0).unwrap().into_inner()
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
        let host = Capture::default();
        let program = if generic {
            Engine::specialize_unspecialized(source, "nested-super.js")
        } else {
            Engine::specialize(source, "nested-super.js")
        }
        .unwrap();
        Runtime::new(host.clone()).execute(&program).unwrap();
        assert_eq!(*host.0.borrow(), ["42", "true"]);
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
    let decoded = quench_runtime::ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let host = Capture::default();
    let view = host.clone();
    Runtime::new(host).execute(&decoded).unwrap();
    assert_eq!(
        Rc::try_unwrap(view.0).unwrap().into_inner(),
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
        let decoded = quench_runtime::ResidualProgram::read_binary(&path).unwrap();
        std::fs::remove_file(path).unwrap();
        let generic = Engine::specialize_unspecialized(&source, "numeric-tdz.js").unwrap();
        for program in [&program, &decoded, &generic] {
            let host = Capture::default();
            let view = host.clone();
            Runtime::new(host).execute(program).unwrap();
            assert_eq!(view.0.borrow().as_slice(), ["true"]);
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
    let mut runtime = Runtime::new(Capture::default());
    runtime.execute(&program).unwrap();
    let global = runtime.global_root().unwrap();
    let key = runtime.string_rooted("capture");
    let capture = runtime.get_property_rooted(global, key).unwrap();
    let undefined = runtime.root(quench_runtime::Value::UNDEFINED);
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
        let mut runtime = Runtime::new(Capture::default());
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
        let restored = quench_runtime::ResidualProgram::read_binary(&path).unwrap();
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
        output(
            "var log = []; function source(name) { return { [Symbol.iterator]() { return { next() { log.push(name + ':next'); return { done: false }; }, return() { log.push(name + ':return'); return {}; } }; } }; } function* plain() { for ([ {} = yield ] of [source('a')]) {} } var g = plain(); g.next(); print(JSON.stringify(g.return(7)) + ' ' + log.join()); log = []; function* guarded() { try { for (var [ x = yield ] of [source('b')]) {} } finally { log.push('finally'); } } g = guarded(); g.next(); print(JSON.stringify(g.return(8)) + ' ' + log.join()); "
        ),
        [
            "{\"value\":7,\"done\":true} a:next,a:return",
            "{\"value\":8,\"done\":true} b:next,b:return,finally"
        ]
    );
}

#[test]
fn nested_direct_eval_vars_belong_to_the_calling_function() {
    assert_eq!(
        output(
            "function u(code) { return eval(code); } u(\"eval('var z = 2;')\"); function v() { eval(\"eval('var y = 3;')\"); return typeof y; } function w() { 'use strict'; eval(\"eval('var x = 1;')\"); return typeof x; } print([typeof z, 'z' in globalThis, v(), typeof y, w()].join()); "
        ),
        ["undefined,false,number,undefined,undefined"]
    );
}

#[test]
fn private_members_resolve_on_branded_proxy_receivers() {
    assert_eq!(
        output(
            "class Base { constructor() { return new Proxy(this, {}); } } class F extends Base { #x = 1; t() { return this.#x; } } class M extends Base { #m() { return 2; } t() { return this.#m(); } } class G extends Base { get #g() { return 3; } t() { return this.#g; } } print([F, M, G].map(C => new C().t()).join(' '));"
        ),
        ["1 2 3"]
    );
}

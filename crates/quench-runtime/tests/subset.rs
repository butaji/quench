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
        assert_eq!(output(source), ["39365"]);
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
    assert_eq!(output(source), ["39365"]);
}

use rqj::{Engine, Host, Vm};
use std::{cell::RefCell, path::PathBuf, process::Command, rc::Rc};

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
    Vm::new(host).execute(&program).unwrap();
    Rc::try_unwrap(view.0).unwrap().into_inner()
}

#[test]
fn residual_binary_round_trip_preserves_execution() {
    let path = std::env::temp_dir().join(format!("rqj-roundtrip-{}.residual", std::process::id()));
    let program = Engine::specialize("print(12345678901234567890n);", "roundtrip.js").unwrap();
    program.write_binary(&path).unwrap();
    let decoded = rqj::ResidualProgram::read_binary(&path).unwrap();
    std::fs::remove_file(path).unwrap();
    let host = Capture::default();
    let view = host.clone();
    Vm::new(host).execute(&decoded).unwrap();
    assert_eq!(
        Rc::try_unwrap(view.0).unwrap().into_inner(),
        ["12345678901234567890"]
    );
}

#[test]
fn closures_prototypes_arrays_and_integer_ops() {
    let source = r#"
      var K = 40;
      function Box(x) { this.x = x; }
      Box.prototype.answer = function(y) { return this.x + y + K; };
      var values = new Array(1); values[0] = new Box(1);
      print(values[0].answer(1));
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn lexical_declarations_use_function_local_slots() {
    let source = r#"
      let left = 40;
      const right = 2;
      {
        let nested = left + right;
        print(nested);
      }
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn variable_destructuring_reads_object_fields_and_array_indices() {
    assert_eq!(
        output(
            "const { answer } = { answer: 42 }; const [left, right] = [40, 2]; print(answer); print(left + right);"
        ),
        ["42", "42"],
    );
}

#[test]
fn array_is_array_distinguishes_arrays_from_array_like_objects() {
    assert_eq!(
        output(
            "print(Array.isArray([])); print(Array.isArray({ length: 0 })); print(Array.isArray('x'));"
        ),
        ["true", "false", "false"],
    );
}

#[test]
fn object_keys_reflect_own_shape_order() {
    assert_eq!(
        output(
            "var object = { first: 1, second: 2 }; var keys = Object.keys(object); print(keys[0]); print(keys[1]);"
        ),
        ["first", "second"],
    );
}

#[test]
fn object_create_preserves_prototype_lookup_and_own_shape() {
    assert_eq!(
        output(
            "var proto = { answer: 42 }; var object = Object.create(proto); print(object.answer); print(Object.keys(object).length);"
        ),
        ["42", "0"],
    );
}

#[test]
fn json_round_trip_uses_runtime_objects_and_arrays() {
    assert_eq!(
        output(
            "var value = JSON.parse('{\"answer\":42,\"items\":[1,2]}'); print(value.answer); print(value.items[1]); print(JSON.stringify(value));"
        ),
        ["42", "2", "{\"answer\":42,\"items\":[1,2]}"],
    );
}

#[test]
fn uri_codecs_preserve_component_and_reserved_character_rules() {
    assert_eq!(
        output(
            "print(encodeURIComponent('a b/&')); print(decodeURIComponent('a%20b%2F%26')); print(encodeURI('https://x.test/a b')); print(decodeURI('https://x.test/a%20b'));"
        ),
        [
            "a%20b%2F%26",
            "a b/&",
            "https://x.test/a%20b",
            "https://x.test/a b"
        ],
    );
}

#[test]
fn date_now_uses_the_host_clock_capability() {
    assert_eq!(output("print(Date.now());"), ["0"]);
}

#[test]
fn number_static_predicates_require_numeric_values() {
    assert_eq!(
        output(
            "print(Number('4')); print(Number.isNaN(NaN)); print(Number.isNaN('x')); print(Number.isFinite(4)); print(Number.isInteger(4.5));"
        ),
        ["4", "true", "false", "true", "false"],
    );
}

#[test]
fn string_search_methods_use_the_receiver_text() {
    assert_eq!(
        output(
            "print('quench'.includes('ench')); print('quench'.startsWith('que')); print('quench'.endsWith('nch'));"
        ),
        ["true", "true", "true"],
    );
}

#[test]
fn base_classes_lower_to_constructor_and_prototype_methods() {
    let source = r#"
      class Box {
        constructor(value) { this.value = value; }
        answer(extra) { return this.value + extra; }
        static tag() { return 42; }
      }
      const box = new Box(40);
      print(box.answer(2));
      print(Box.tag());
    "#;
    assert_eq!(output(source), ["42", "42"]);
}

#[test]
fn class_methods_capture_the_enclosing_activation() {
    let source = r#"
      function make(offset) {
        class Box { answer() { return offset + 1; } }
        return new Box();
      }
      print(make(41).answer());
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn template_literals_lower_to_string_addition() {
    assert_eq!(
        output("const answer = 42; print(`value: ${answer}!`);"),
        ["value: 42!"]
    );
}

#[test]
fn sequence_expressions_preserve_order_and_return_the_tail() {
    assert_eq!(
        output("var seen = 0; print((seen = 1, seen + 41));"),
        ["42"]
    );
}

#[test]
fn default_parameters_only_evaluate_for_undefined_arguments() {
    let source = r#"
      var calls = 0;
      function add(value = (calls = calls + 1, 40)) { return value + 2; }
      print(add());
      print(add(5));
      print(calls);
      var arrow = (value = 41) => value + 1;
      print(arrow());
    "#;
    assert_eq!(output(source), ["42", "7", "1", "42"]);
}

#[test]
fn nullish_coalescing_only_falls_back_for_nullish_values() {
    assert_eq!(
        output("print(null ?? 42); print(undefined ?? 7); print(0 ?? 9); print('' ?? 3);"),
        ["42", "7", "0", ""],
    );
}

#[test]
fn optional_member_access_short_circuits_nullish_bases() {
    assert_eq!(
        output(
            "var none = null; var object = { value: 42 }; print(none?.value); print(object?.value); print(object.missing?.value);"
        ),
        ["undefined", "42", "undefined"],
    );
}

#[test]
fn optional_calls_skip_arguments_and_preserve_method_receivers() {
    assert_eq!(
        output(
            "var calls = 0; var none = null; print(none?.(calls = 1)); print(calls); var object = { run: function(value) { return value + 1; } }; print(object?.run(41));"
        ),
        ["undefined", "0", "42"],
    );
}

#[test]
fn bigint_literals_are_heap_values_and_stringify_without_loss() {
    assert_eq!(
        output("print(12345678901234567890n); print(String(7n)); print(7n === 7n);"),
        ["12345678901234567890", "7", "true"]
    );
}

#[test]
fn symbols_are_identity_values_with_explicit_display() {
    assert_eq!(
        output(
            "const first = Symbol('x'); print(first); print(String(first)); print(first === first); print(Symbol('x') === Symbol('x'));"
        ),
        ["Symbol(x)", "Symbol(x)", "true", "false"],
    );
}

#[test]
fn symbol_registry_preserves_identity_and_key_round_trip() {
    assert_eq!(
        output(
            "var first = Symbol.for('shared'); var second = Symbol.for('shared'); print(first === second); print(Symbol.keyFor(first)); print(Symbol.keyFor(Symbol('local')));"
        ),
        ["true", "shared", "undefined"],
    );
}

#[test]
fn typeof_reports_symbol_and_bigint_primitives() {
    assert_eq!(
        output("print(typeof Symbol('x')); print(typeof 1n);"),
        ["symbol", "bigint"]
    );
}

#[test]
fn method_caches_observe_callable_property_replacement() {
    let source = r#"
      function Box() {}
      Box.prototype.run = function() { return 1; };
      function invoke(value) { return value.run(); }
      var first = new Box();
      var second = new Box();
      print(invoke(first));
      Box.prototype.run = function() { return 2; };
      print(invoke(second));
      var key = "run";
      Box.prototype[key] = function() { return 3; };
      print(invoke(first));
      var own = { run: function() { return 4; } };
      print(invoke(own));
      own.run = function() { return 5; };
      print(invoke(own));
    "#;
    assert_eq!(output(source), ["1", "2", "3", "4", "5"]);
}

#[test]
fn method_caches_distinguish_own_methods_and_cached_writes() {
    let source = r#"
      function invoke(value) { return value.run(); }
      var first = { run: function() { return 1; } };
      var second = { run: function() { return 2; } };
      print(invoke(first));
      print(invoke(second));
      print(invoke(first));
      function install(value, result) {
        value.run = function() { return result; };
      }
      install(first, 3);
      print(invoke(first));
      install(first, 4);
      print(invoke(first));
    "#;
    assert_eq!(output(source), ["1", "2", "1", "3", "4"]);
}

#[test]
fn unsupported_syntax_is_rejected_early() {
    let errors = Engine::specialize("class Bad extends Base {}", "bad.js").unwrap_err();
    assert!(errors[0].to_string().contains("class heritage"));
}

#[test]
fn catch_receives_thrown_value() {
    assert_eq!(
        output("try { throw 'caught'; } catch (e) { print(e); }"),
        ["caught"]
    );
}

#[test]
fn unified_field_and_binary_operands_preserve_semantics() {
    let source = r#"
      function Box() { this.x = 4; this.inner = { y: 3 }; }
      Box.prototype.check = function() {
        this.x += 2;
        if (this.x == 6) return this.inner.y + 1;
        return 0;
      };
      var box = new Box();
      print(box.x + 2);
      print(2 + box.x);
      print(box.inner.y);
      print(box.check());
    "#;
    assert_eq!(output(source), ["6", "6", "3", "4"]);
}

#[test]
fn computed_property_keys_are_interned_at_runtime() {
    let source = r#"
      var object = {};
      var prefix = 'run';
      var suffix = 'time';
      object[prefix + suffix] = 42;
      print(object[prefix + suffix]);
    "#;
    assert_eq!(output(source), ["42"]);
}

#[test]
fn string_function_converts_values() {
    assert_eq!(
        output("print(String(42)); print(String()); print(new String(true));"),
        ["42", "undefined", "true"]
    );
}

#[test]
fn crypto_primitive_library_matches_es5_behavior() {
    let source = r#"
      print("Az".charCodeAt(1));
      print("abcd".charAt(2));
      print("abcd".substring(3, 1));
      print("abcd".substr(-3, 2));
      print("AéB".charCodeAt(1));
      print("AéB".substring(1, 2));
      print("A😀B".charCodeAt(1));
      print("A😀B".charCodeAt(2));
      print("A😀B".substring(1, 3));
      print(String.fromCharCode(65, 66));
      print((255).toString(16));
      print(parseInt("  -0x10tail", 0));
      print(Math.floor(3.9));
      print(Math.min(7, 2, 5));
      print(Math.max(7, 2, 5));
      print(Math.min(7, undefined));
      print(Math.LN2 > 0);
      var random = Math.random();
      print(random >= 0 && random < 1);
    "#;
    assert_eq!(
        output(source),
        [
            "122", "c", "bc", "bc", "233", "é", "55357", "56832", "😀", "AB", "ff", "-16", "3",
            "2", "7", "NaN", "true", "true"
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
fn abstract_equality_keeps_nullish_values_distinct_from_zero_and_false() {
    let source = r#"
      print(null == undefined);
      print(null == 0);
      print(undefined == 0);
      print(false == 0);
      print("0" == 0);
      print(null === null);
    "#;
    assert_eq!(
        output(source),
        ["true", "false", "false", "true", "true", "true"]
    );
}

#[test]
fn sparse_array_index_does_not_require_dense_prefix() {
    let source = "var a = new Array(0); a[1000000] = 7; print(a.length); print(a[1000000]);";
    assert_eq!(output(source), ["1000001", "7"]);
}

#[test]
fn closure_environment_promotes_only_when_creation_executes() {
    let source = r#"
      function maybe(make) {
        var value = 40;
        if (make) return function(x) { return value + x; };
        return value;
      }
      print(maybe(false));
      var add = maybe(true);
      print(add(2));
    "#;
    assert_eq!(output(source), ["40", "42"]);
}

#[test]
fn switch_supports_strict_matching_fallthrough_default_and_break() {
    let source = r#"
      var tests = 0;
      function choose() { tests++; return 2; }
      var first = 0;
      switch (choose()) {
        case '2': first = 100; break;
        case 2: first += 2;
        case 3: first += 3; break;
        default: first = 200;
      }
      var second = 0;
      switch (9) {
        case 1: second = 1; break;
        default: second += 4;
        case 2: second += 5; break;
      }
      print(tests); print(first); print(second); print('1' === 1);
    "#;
    assert_eq!(output(source), ["1", "5", "9", "false"]);
}

#[test]
fn loop_control_targets_are_derived_from_nesting() {
    let source = r#"
      var once = 0;
      do { once++; } while (false);
      var total = 0;
      for (var i = 0; i < 4; i++) {
        switch (i) {
          case 1: continue;
          case 2: break;
        }
        total += i;
      }
      var n = 0;
      do {
        n++;
        if (n < 3) continue;
        break;
      } while (true);
      print(once); print(total); print(n);
    "#;
    assert_eq!(output(source), ["1", "5", "3"]);
}

#[test]
fn labeled_control_flow_is_rejected_early() {
    let errors = Engine::specialize("label: while (true) break label;", "bad.js").unwrap_err();
    assert!(errors[0].to_string().contains("labeled statements"));
}

#[test]
fn object_prototypes_and_function_call_support_inheritance() {
    let source = r#"
      Object.prototype.inheritsFrom = function (parent) {
        function Inheriter() {}
        Inheriter.prototype = parent.prototype;
        this.prototype = new Inheriter();
        this.superConstructor = parent;
      };
      function Base(value) { this.value = value; }
      Base.prototype.answer = function (extra) { return this.value + extra; };
      function Child(value) { Child.superConstructor.call(this, value); }
      Child.inheritsFrom(Base);
      var child = new Child(40);
      var plain = new Object();
      Object.prototype.marker = 7;
      var same = {};
      print(child.answer(2));
      print(plain.marker);
      print(Object(same) === same);
      try { (function () { throw 'called'; }).call(null); }
      catch (error) { print(error); }
    "#;
    assert_eq!(output(source), ["42", "7", "true", "called"]);
}

#[test]
fn array_pop_updates_dense_and_sparse_lengths() {
    let source = r#"
      var dense = new Array();
      dense.push(1); dense.push(2);
      print(dense.pop()); print(dense.length);
      print(dense.pop()); print(dense.pop());
      var sparse = new Array();
      sparse[1000000] = 7;
      print(sparse.pop()); print(sparse.length);
    "#;
    assert_eq!(output(source), ["2", "1", "1", "undefined", "7", "1000000"]);
}

#[test]
fn constant_array_literals_preserve_values() {
    assert_eq!(
        output("var values = [0, 'one', true, null]; print(values[1]); print(values.length);"),
        ["one", "4"]
    );
}

#[test]
fn constant_array_templates_detach_before_mutation() {
    let source = r#"
      function make() { return [1, 2]; }
      var first = make();
      var second = make();
      first[0] = 9;
      first.push(3);
      print(first[0]); print(first.length);
      print(second[0]); print(second.length);
    "#;
    assert_eq!(output(source), ["9", "3", "1", "2"]);
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
fn immutable_root_functions_use_direct_calls_but_reassignments_do_not() {
    let direct = Engine::specialize(
        "function target() { return 42; } function caller() { return target(); } print(caller());",
        "direct-call.js",
    )
    .unwrap();
    assert!(direct.disassemble().contains("CallKnown"));
    assert_eq!(
        output(
            "function f(){return 1;} function g(){return f();} f=function(){return 2;}; print(g());"
        ),
        ["2"]
    );
}

#[test]
fn updates_evaluate_computed_member_reference_once() {
    let source = r#"
      var values = [10, 20, 30];
      var index = 0;
      print(values[++index]++);
      print(index);
      print(values[1]);
      print(values[2]);
    "#;
    assert_eq!(output(source), ["20", "1", "21", "30"]);
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

#[test]
fn supported_subset_matches_quickjs() {
    let quickjs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../quickjs/qjs");
    if !quickjs.is_file() {
        eprintln!(
            "skipping differential test: {} is absent",
            quickjs.display()
        );
        return;
    }
    let source = r#"
      var seed = 3;
      function Counter(value) { this.value = value; }
      Counter.prototype.step = function(delta) {
        this.value += delta;
        return this.value;
      };
      function makeAdder(base) { return function(value) { return base + value; }; }
      var object = new Counter(seed);
      var values = new Array(3);
      values[0] = object.step(2);
      values[1] = makeAdder(30)(7);
      values[2] = (values[0] << 2) | 1;
      var key = 'answer'; object[key] = values[1] + 5;
      var total = 0;
      for (var i = 0; i < values.length; i++) total += values[i];
      try { throw object[key]; } catch (caught) { print(caught); }
      print(total);
      print(object.value == 5);
      print("Az".charCodeAt(1));
      print("abcd".substring(3, 1));
      print((255).toString(16));
      print(parseInt(" -0x10tail", 0));
      print(Math.min(7, 2, 5));
      print(null == 0);
    "#;
    let expected = output(source).join("\n") + "\n";
    let fixture = std::env::temp_dir().join(format!(
        "rqj-differential-{}-{}.js",
        std::process::id(),
        std::thread::current().name().unwrap_or("test")
    ));
    std::fs::write(&fixture, source).unwrap();
    let result = Command::new(&quickjs).arg(&fixture).output().unwrap();
    let _ = std::fs::remove_file(&fixture);
    assert!(result.status.success(), "QuickJS failed: {result:?}");
    assert_eq!(String::from_utf8(result.stdout).unwrap(), expected);
}

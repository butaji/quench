use super::*;
use crate::Engine;
use crate::test_host::TestHost;
use std::{cell::RefCell, rc::Rc};

fn run(source: &str) -> (Vm<TestHost>, String) {
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&Engine::specialize(source, "string-data.js").unwrap()).unwrap();
    let printed = output.borrow().join("\n");
    (vm, printed)
}

#[test]
fn a_string_wrapper_answers_its_indices_from_the_string() {
    // ECMA-262 10.4.3: index properties are read-only, enumerable, non-configurable data
    // properties ahead of every other own key; writes and deletes of them fail.
    let (_, output) = run(r#"
      var s = new String("hey"); s[5] = "five"; s.foo = 1; s[0] = "z";
      var d = Object.getOwnPropertyDescriptor(s, "1"), keys = [];
      for (var k in s) keys.push(k);
      var strict = (function () { "use strict"; try { s[1] = "q"; return "no"; } catch (e) { return e instanceof TypeError; } })();
      print([s[0], s[2], s[3], s.length, Object.keys(s).join(","), keys.join(","),
             d.value + d.writable + d.enumerable + d.configurable, 1 in s, 3 in s,
             delete s[0], delete s[5], strict, Reflect.set({}, "0", 1, new String("r")),
             Reflect.defineProperty(s, "0", { value: "h" }), Reflect.defineProperty(s, "0", { value: "Q" }),
             Object.create(new String("in"))[1]].join(" "));
    "#);
    assert_eq!(output, "h y  3 0,1,2,5,foo 0,1,2,5,foo efalsetruefalse true false false true true false true false n");
}

#[test]
fn boxing_a_long_string_costs_no_shape_per_unit() {
    let shapes_after = |boxes: usize| {
        let source = format!(
            "var text = 'x'.repeat(66000); var n = 0;
             for (var i = 0; i < {boxes}; i++) n += Object(text).length + Object(text)[65999].length;"
        );
        run(&source).0.shapes.len()
    };
    let (one, three) = (shapes_after(1), shapes_after(3));
    assert!(three - one < 4, "boxing appended {} shapes", three - one);
    assert!(one < 4000, "one box made {one} shapes");
}

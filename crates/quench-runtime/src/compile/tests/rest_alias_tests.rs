//! Rest lists compiled as their function's `arguments` binding (`compile::rest_alias`): each read
//! answers as the rest array would, and an escaping, written or captured rest list stays one.
use crate::Engine;

fn printed(source: &str) -> Vec<String> {
    use crate::test_host::TestHost;
    use crate::vm::Vm;
    use std::{cell::RefCell, rc::Rc};
    let program = Engine::specialize(source, "rest-alias.js").unwrap();
    let output = Rc::new(RefCell::new(Vec::new()));
    let mut vm = Vm::new(TestHost::writing_to(output.clone()));
    vm.execute(&program).unwrap();
    output.take()
}

#[test]
fn rest_reads_answer_as_the_rest_array() {
    let source = "function g() { return Array.prototype.join.call(arguments, '-'); }\n\
                  function count(...a) { return a.length; }\n\
                  function at(...a) { return a[1] + ':' + a[5] + ':' + a['0']; }\n\
                  function spread(...a) { return g(...a); }\n\
                  function apply(...a) { return g.apply(null, a); }\n\
                  function kind(...a) { return a.map === undefined; }\n\
                  print(count() + ',' + count(1, 2, 3));\n\
                  print(at(1, 2, 3));\n\
                  print(spread(1, 2, 3) + ',' + spread());\n\
                  print(apply(4, 5));\n\
                  print(kind(1));";
    assert_eq!(printed(source), ["0,3", "2:undefined:1", "1-2-3,", "4-5", "false"]);
}

#[test]
fn rest_lists_that_escape_or_are_written_stay_arrays() {
    let source = "function escape(...a) { return a; }\n\
                  function write(...a) { a[0] = 9; return a[0] + a.length; }\n\
                  function capture(...a) { return () => a.length; }\n\
                  function shadow(...a) { return arguments.length + a.length; }\n\
                  function late(...a) { const g = (...x) => x.length; const n = g(...a); return n + a.length; }\n\
                  print(Array.isArray(escape(1, 2)) + ',' + escape(1, 2).length);\n\
                  print(write(1, 2));\n\
                  print(capture(1, 2, 3)());\n\
                  print(shadow(1, 2));\n\
                  print(late(1, 2));\n\
                  print(escape.length + ',' + write.length);";
    assert_eq!(printed(source), ["true,2", "11", "3", "4", "4", "0,0"]);
}

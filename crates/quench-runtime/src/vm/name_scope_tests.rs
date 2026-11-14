//! The events that retire a by-name read's resolution (`name_scope`): a scope declaring the
//! name dynamically, a `with` object, a later Script's global lexical declaration.
use super::*;
use crate::Engine;
use crate::test_host::TestHost;

/// Guest helpers: `hot` repeats a read until its record is quickened, `check` throws on a
/// mismatch, `thrown` names the error a call throws.
const HELPERS: &str = r#"
function hot(read) { var s; for (var i = 0; i < 300; i++) s = read(); return s; }
function check(got, want, what) { if (got !== want) throw Error(what + ': ' + got + ' !== ' + want); }
function thrown(f) { try { f(); return 'none'; } catch (e) { return e.constructor.name; } }
"#;

fn run(source: &str) -> (Vm<TestHost>, crate::bytecode::ResidualProgram) {
    let program = Engine::specialize(&format!("{HELPERS}{source}"), "name-scope.js").unwrap();
    let mut vm = Vm::new(TestHost::default());
    vm.execute(&program).unwrap();
    (vm, program)
}

#[test]
fn an_eval_var_retires_the_global_reads_that_landed_past_its_scope() {
    let (vm, _) = run(r#"
globalThis.gx = 'G';
function scope() {
  var read = function () { return eval('gx'); };
  check(hot(read), 'G', 'global before');
  eval('var gx = "E"');
  check(hot(read), 'E', 'eval var shadows');
  check(hot(function () { return gx; }), 'E', 'closure after');
  eval('delete gx');
  check(hot(read), 'G', 'deleted eval var uncovers the global');
}
scope();
"#);
    let atom = vm.lookup_atom("gx").unwrap();
    assert!(vm.scope_names.declared(atom), "the dynamic declaration is recorded");
}

#[test]
fn a_name_no_scope_declared_stays_a_global_read() {
    let (vm, _) = run(r#"
globalThis.gy = 1; globalThis.gz = 2;
function scope() {
  var read = function () { return eval('gy'); };
  check(hot(read), 1, 'before');
  eval('var other = 1');
  check(hot(read), 1, 'unrelated declaration');
}
scope();
"#);
    assert!(!vm.scope_names.declared(vm.lookup_atom("gy").unwrap()));
    assert!(vm.scope_names.declared(vm.lookup_atom("other").unwrap()));
}

#[test]
fn a_with_object_is_consulted_whatever_a_read_landed_on_before() {
    let (vm, _) = run(r#"
globalThis.wx = 'G';
var o = {};
function read() { return eval('wx'); }
check(hot(read), 'G', 'before with');
with (o) {
  check(read(), 'G', 'a function outside the with sees the global');
  check(eval('wx'), 'G', 'absent from the object');
  o.wx = 'W';
  check(eval('wx'), 'W', 'gained');
  o[Symbol.unscopables] = { wx: true };
  check(eval('wx'), 'G', 'unscopable');
  o[Symbol.unscopables] = {};
  delete o.wx;
  check(eval('wx'), 'G', 'lost');
}
check(read(), 'G', 'after with');
"#);
    assert!(vm.with_scope_entered);
}

#[test]
fn a_later_scripts_lexical_declaration_shadows_a_probed_global_read() {
    let (mut vm, program) = run(r#"
globalThis.lx = 'G';
function scope() { return function () { return eval('lx'); }; }
var read = scope();
check(hot(read), 'G', 'before');
"#);
    vm.eval_script_source(&program, "let lx = 'L'; check(hot(read), 'L', 'lexical shadows');", "later.js").unwrap();
    let error = vm.eval_script_source(&program, "let lx = 'L2';", "later2.js").unwrap_err();
    assert!(vm.format_error(&program, &error).contains("SyntaxError"));
}

#[test]
fn a_compiled_script_may_not_redeclare_a_global_lexical_binding() {
    let (mut vm, program) = run("");
    let mut declare = |source: &str| {
        let script = vm.compile_embedding_host_script(source, "declare.js", (0, 0)).unwrap();
        vm.evaluate_embedding_compiled_script(&script).map_err(|error| vm.format_error(&program, &error))
    };
    declare("let a = 1;").unwrap();
    assert!(declare("let a = 2;").unwrap_err().contains("SyntaxError"));
    assert!(declare("const a = 3;").unwrap_err().contains("SyntaxError"));
    assert!(declare("var a = 4;").unwrap_err().contains("SyntaxError"));
    declare("let b = a;").unwrap();
}

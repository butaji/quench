//! Builtin, closure and string identities, and the global shape.

use super::support::*;
use quench_runtime::compile::aot_facts::{analyze, world, BuiltinPath};
use quench_runtime::compile::aot_facts::World;
use quench_runtime::typed_ir::{Identity};

#[test]
fn a_program_that_can_eval_names_no_uncached_atom_and_no_global_shape() {
    let program = program("var q = 1; eval('1');");
    let facts = analyze(&program);
    assert!(facts.uncached_atoms().is_empty(), "new code may add a method call site");
    assert!(facts.global_shape().is_none(), "new code may declare or delete a global");
}

#[test]
fn the_global_shape_lists_the_declared_globals_and_the_builtins_the_program_reads() {
    let program = program("var x = 1; function f() { return Math.max(x, 2); } f();");
    let facts = analyze(&program);
    let shape = facts.global_shape().expect("a closed program fixes its global object's shape");
    assert!(shape.declared().contains(&atom_named(&program, "x")), "x is declared by the program");
    assert!(shape.builtins().contains(&atom_named(&program, "Math")), "Math is a builtin the program reads and never changes");
    assert!(!shape.builtins().contains(&atom_named(&program, "x")), "a declared global is no builtin");
}

#[test]
fn a_computed_store_while_the_global_object_is_a_value_takes_the_global_shape() {
    let program = program("var k = 'x'; globalThis[k] = 1;");
    assert!(analyze(&program).global_shape().is_none(), "the store may write any global name the global object holds");
}

#[test]
fn a_builtin_method_callee_register_is_its_builtin_identity() {
    let program = program("var x = 4; var r = Math.sqrt(x);");
    let sqrt = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "sqrt")) };
    assert!(identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(sqrt)), "no store writes Math.sqrt");
}

#[test]
fn a_builtin_member_a_store_writes_is_no_builtin_identity() {
    let program = program("function f(x) { return x; } Math.sqrt = f; var r = Math.sqrt(4);");
    let sqrt = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "sqrt")) };
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(sqrt)), "a store writes Math.sqrt, so its reads are Many");
}

#[test]
fn a_builtin_member_an_immediate_is_stored_to_is_no_builtin_identity() {
    let program = program("function f(x) { return Math.floor(x); } Math.floor = 1; var s = f(2.5);");
    let floor = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "floor")) };
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(floor)), "Math.floor = 1 writes Math.floor");
}

#[test]
fn a_builtin_member_deleted_is_no_builtin_identity() {
    let program = program("function f(x) { return Math.floor(x); } delete Math.floor; var s = f(2.5);");
    let floor = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "floor")) };
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(floor)), "delete Math.floor removes Math.floor");
}

#[test]
fn a_builtin_member_stored_under_a_computed_key_is_no_builtin_identity() {
    let program = program("function f(x) { return Math.floor(x); } var k = 'floor'; Math[k] = function () { return 9; }; var s = f(2.5);");
    let floor = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "floor")) };
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(floor)), "Math[k] = v writes Math.floor");
}

#[test]
fn a_builtin_member_defined_through_an_alias_is_no_builtin_identity() {
    let program = program("function f(x) { return Math.floor(x); } var d = Object.defineProperty; d(Math, 'floor', { value: 9 }); var s = f(2.5);");
    let floor = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "floor")) };
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(floor)), "an aliased defineProperty writes Math.floor");
}

#[test]
fn a_builtin_member_stays_builtin_past_a_script_call_handed_an_object() {
    let program = program("function g(o) { return o.x; } g({ x: 1 }); var y = Math.floor(2.5);");
    let floor = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "floor")) };
    assert!(identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(floor)), "no value of Math leaks, so no define changes Math.floor");
}

#[test]
fn a_builtin_member_is_no_builtin_identity_once_its_object_leaks_into_a_many_cell() {
    let program = program("var M = Math; if (Math.random() > 0.5) { M = {}; } Object.defineProperty(M, 'x', { value: 1 }); var y = Math.floor(2.5);");
    let floor = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "floor")) };
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Builtin(floor)), "Math leaks into M, which a define may hand Math.floor's object");
}

#[test]
fn a_constructor_read_by_a_computed_key_leaves_the_world_open() {
    let program = program("var k = 'constructor'; var F = Object.getPrototypeOf(function () {})[k]; var src = 'Math.floor = 1'; var s = 0; if (s === 1) F(src)();");
    assert!(world(&program) != World::Closed, "the Function constructor is reached by a computed read of constructor");
}

#[test]
fn a_closure_bound_before_its_store_is_no_closure_identity() {
    let program = program("var h; function call() { return h(); } function g() { return 1; } var out = 0; \
        for (var i = 0; i < 100; i++) { if (i === 50) h = g; try { out += call(); } catch (e) { out -= 1; } }");
    let g = function_named(&program, "g") as u32;
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Closure(g)), "h is undefined until its one store, which dominates no read");
}

#[test]
fn a_top_level_function_register_is_its_closure_identity() {
    let program = program("function f(x) { return x; } var g = f; g(1);");
    let f = function_named(&program, "f") as u32;
    assert!(identities_of(&program, &analyze(&program)).contains(&Identity::Closure(f)), "the root creates f once");
}

#[test]
fn a_closure_made_in_a_loop_register_is_many() {
    let program = program("var fs = []; for (var i = 0; i < 3; i++) { fs.push(function () { return i; }); } fs.length;");
    let inner = (0..program.function_count()).find(|&id| program.with_record(id, |function| function.parent.is_some())).expect("a nested function") as u32;
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::Closure(inner)), "the loop creates a closure per iteration");
}

#[test]
fn a_string_literal_concat_under_the_threshold_is_flat() {
    let program = program("function f() { return 'ab' + 'cd'; } f();");
    let facts = analyze(&program);
    let (last, registers) = last_pc_and_registers(&program, "f");
    let function = facts.function_facts(function_named(&program, "f") as u32);
    assert!((0..registers).any(|register| function.string_flat(last, register as u16)));
}

#[test]
fn a_string_from_an_unknown_value_is_not_flat() {
    let program = program("function f(s) { return s + 'cd'; } f(globalThis.hostInput);");
    let facts = analyze(&program);
    let (last, registers) = last_pc_and_registers(&program, "f");
    let function = facts.function_facts(function_named(&program, "f") as u32);
    assert!(!(0..registers).any(|register| function.string_flat(last, register as u16)));
}

#[test]
fn an_int_array_filled_in_a_loop_is_a_plain_int32_backing() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { a[i] = i; } return a; } f();");
    let facts = analyze(&program);
    let (last, registers) = last_pc_and_registers(&program, "f");
    let function = facts.function_facts(function_named(&program, "f") as u32);
    assert!((0..registers).any(|register| function.backing_plain(last, register as u16)));
}

#[test]
fn an_int_array_with_a_delete_is_not_a_plain_int32_backing() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { a[i] = i; } delete a[0]; return a; } f();");
    let facts = analyze(&program);
    let (last, registers) = last_pc_and_registers(&program, "f");
    let function = facts.function_facts(function_named(&program, "f") as u32);
    assert!(!(0..registers).any(|register| function.backing_plain(last, register as u16)));
}

#[test]
fn a_class_only_built_by_direct_new_is_a_fixed_cell_from_birth() {
    let program = program("function Scheduler() { this.count = 0; } var s = new Scheduler(); s.count = 1;");
    let facts = analyze(&program);
    assert!(facts.class_fixed_cell(function_named(&program, "Scheduler") as u32));
}

#[test]
fn a_named_property_read_before_its_closure_store_is_no_closure_identity() {
    let program = program("var o = {}; function f() { return o.m(); } try { f(); } catch (e) {} o.m = function () { return 1; }; f();");
    let facts = analyze(&program);
    assert!(!identities_in(&program, &facts, "f").iter().any(|identity| matches!(identity, Identity::Closure(_))),
        "f reads o.m before the one store of it, which dominates no read: the read holds no closure identity");
}

#[test]
fn a_class_method_read_after_construction_keeps_its_closure_identity() {
    let program = program("class C { m() { return 1; } } var c = new C(); var r = c.m();");
    let m = function_named(&program, "m") as u32;
    assert!(identities_of(&program, &analyze(&program)).contains(&Identity::Closure(m)),
        "the class header defines m before any construction, and the read runs after it");
}

#[test]
fn a_slot_with_a_checked_store_of_another_class_joins_both_classes() {
    let program = program("function A() {} function B() {} function f() { let x = new A(); x = new B(); return x; } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    let function = facts.function_facts(f);
    assert!((0..4u16).filter_map(|slot| function.local_cell(slot)).any(|cell| cell.classes.ids().len() == 2),
        "x holds an A and then a B, so its slot's cell holds both classes");
}

/// A CommonJS module as the loader compiles it: the root makes the wrapper and returns it, and the host calls it once.
fn commonjs(body: &str, host_calls_once: bool) -> quench_runtime::bytecode::ResidualProgram {
    let mut program = program(&format!("(function (exports, require, module, __filename, __dirname) {{ {body}\n}});"));
    program.host_calls_result_once = host_calls_once;
    program
}

/// The id of the function expression assigned as `S.prototype.go` (the only nested function besides `S` in the module body).
fn module_method(program: &quench_runtime::bytecode::ResidualProgram) -> u32 {
    let s = function_named(program, "S");
    (0..program.function_count()).find(|&id| id != s && program.with_record(id, |function| {
        function.parent.is_some_and(|parent| parent != 0) && function.name.is_none_or(|atom| { let text: &str = program.atoms[atom as usize].as_ref(); text != "S" })
    })).expect("the prototype method") as u32
}

const MODULE: &str = "function S(n) { this.n = n; } S.prototype.go = function () { return this.n; }; var s = new S(1); s.go();";

#[test]
fn a_commonjs_module_body_runs_once_so_its_top_level_closures_are_one_object() {
    let program = commonjs(MODULE, true);
    let facts = analyze(&program);
    assert!(facts.closure_identity(function_named(&program, "S") as u32), "the wrapper body runs once and declares S once");
    assert!(facts.closure_identity(module_method(&program)), "the wrapper body makes the prototype method once");
}

#[test]
fn a_returned_wrapper_the_host_may_call_again_keeps_no_closure_identity() {
    let program = commonjs(MODULE, false);
    let facts = analyze(&program);
    assert!(!facts.closure_identity(function_named(&program, "S") as u32), "each call of the returned function declares S again");
    assert!(!facts.closure_identity(module_method(&program)), "each call of the returned function makes the method again");
}

#[test]
fn a_commonjs_body_keeps_its_closure_identity_past_a_caller_read() {
    // Legacy reflection never hands out the host's wrapper (`f.caller` reads `null` there, `Vm::hides_host_wrapper`), so a
    // `caller` read reaches no value that could run the body again.
    let program = commonjs(&format!("{MODULE} function f() {{ return f.caller; }} f();"), true);
    assert!(analyze(&program).closure_identity(function_named(&program, "S") as u32), "f.caller is null, not the wrapper");
}

#[test]
fn a_commonjs_body_that_reads_arguments_keeps_no_closure_identity() {
    let program = commonjs(&format!("{MODULE} var again = arguments.callee;"), true);
    assert!(!analyze(&program).closure_identity(function_named(&program, "S") as u32), "arguments.callee is the wrapper");
}

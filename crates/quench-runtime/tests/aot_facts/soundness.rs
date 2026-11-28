//! Soundness regressions from the review of the solver's facts: each asserts a fact is not proven.

// Soundness regressions, from the review of the solver's facts. Each asserts that a fact is NOT proven
// thing the fact relies on, so the solver must stay conservative.

use super::support::*;
use quench_runtime::compile::aot_facts::{analyze, world};
use quench_runtime::compile::aot_facts::World;
use quench_runtime::typed_ir::{FactKey, Identity, Type};

#[test]
fn a_global_object_store_of_a_builtin_name_drops_the_builtin_and_its_members() {
    let program = program("globalThis.Math = { floor: function (x) { return 0; } }; function f(x) { return Math.floor(x); } f(2.5);");
    let facts = analyze(&program);
    assert!(!facts.builtin_identity(builtin_path(&program, "Math", None)), "globalThis.Math = writes the global Math");
    assert!(!facts.builtin_identity(builtin_path(&program, "Math", Some("floor"))), "Math is no longer the realm's, so its floor is not");
    assert!(!identities_of(&program, &facts).contains(&Identity::Builtin(builtin_path(&program, "Math", Some("floor")))));
}

#[test]
fn a_store_into_a_builtin_prototype_drops_the_identity_of_a_method_it_names() {
    for store in ["Object.prototype.floor = function () { return 0; };", "Array.prototype.floor = function () { return 0; };"] {
        let program = program(&format!("{store} function f(x) {{ return Math.floor(x); }} f(2.5);"));
        let facts = analyze(&program);
        assert!(!facts.builtin_identity(builtin_path(&program, "Math", Some("floor"))), "{store} names floor, which Math.floor reads");
        assert!(!identities_of(&program, &facts).contains(&Identity::Builtin(builtin_path(&program, "Math", Some("floor")))), "{store}");
    }
}

#[test]
fn a_define_through_an_alias_of_define_property_leaves_the_instances_slots_unfixed() {
    // `d` holds `Object.defineProperty`, so the call defines an accessor `x` on `p`: `p`'s class has no fixed cell.
    let program = program("function P() { this.x = 1; } var p = new P(); const d = Object.defineProperty; \
        d(p, 'x', { get: function () { return 2; } }); p.x;");
    let facts = analyze(&program);
    assert!(!facts.class_fixed_cell(function_named(&program, "P") as u32), "an aliased defineProperty may make x an accessor");
}

#[test]
fn a_constructor_read_off_an_unknown_value_leaves_the_world_open() {
    let program = program("var o = globalThis.hostInput; var c = o.constructor; c;");
    assert!(world(&program) != World::Closed, "o may be a function, whose constructor reaches the Function constructor");
}

#[test]
fn a_reflective_read_of_an_instance_keeps_its_stores_observed() {
    for read in ["Object.getOwnPropertyDescriptor(p, 'x')", "Reflect.get(p, 'x')"] {
        let program = program(&format!("function P() {{ this.x = 1; }} var p = new P(); p.x = 2; var r = {read};"));
        let facts = analyze(&program);
        assert!(facts.field_has_reader(FactKey::AnyField(atom_named(&program, "x"))), "{read} may observe any field of p");
    }
}

#[test]
fn a_global_stored_on_one_branch_names_no_call_target() {
    let program = program("var h; function mm() { return 1; } if (Math.random() > 0.5) { h = mm; } \
        function call() { return h(); } call();");
    let facts = analyze(&program);
    let (call, mm) = (function_named(&program, "call") as u32, function_named(&program, "mm") as u32);
    assert!(!targets_in(&facts, call).contains(&mm), "h is mm only on one path, so the store does not dominate the call");
}

#[test]
fn a_global_stored_after_a_use_names_no_call_target() {
    let program = program("var h; function mm() { return 1; } function call() { return h(); } \
        try { call(); } catch (e) {} h = mm; call();");
    let facts = analyze(&program);
    let (call, mm) = (function_named(&program, "call") as u32, function_named(&program, "mm") as u32);
    assert!(!targets_in(&facts, call).contains(&mm), "the first call reads h before its one store");
}

#[test]
fn a_property_stored_on_one_branch_is_no_closure_identity_at_its_read() {
    let program = program("var o = {}; if (Math.random() > 0.5) { o.m = function mm() { return 1; }; } \
        function f() { return o.m(); } f();");
    let facts = analyze(&program);
    let (f, mm) = (function_named(&program, "f") as u32, function_named(&program, "mm") as u32);
    assert!(!identities_in(&program, &facts, "f").contains(&Identity::Closure(mm)), "o.m is mm only on one path");
    assert!(!targets_in(&facts, f).contains(&mm), "o.m names mm only on one path");
}

#[test]
fn a_local_holding_closures_from_two_paths_names_no_call_target() {
    let program = program("function f(c) { let k; if (c) { k = function () { return 1; }; } else { k = function () { return 2; }; } \
        return k(); } f(true); f(false);");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(targets_in(&facts, f).is_empty(), "k holds one of two closures, so its call names no target");
}

#[test]
fn a_parameter_bound_to_two_closures_names_no_call_target() {
    let program = program("function run(cb) { return cb(); } run(function () { return 1; }); run(function () { return 2; });");
    let facts = analyze(&program);
    assert!(targets_in(&facts, function_named(&program, "run") as u32).is_empty(), "cb is one of two closures");
}

#[test]
fn a_builtin_handed_to_a_script_function_that_stores_into_it_is_no_identity() {
    for source in [
        "function f(g) { g.floor = function () { return 0; }; } f(Math); var r = Math.floor(2.5);",
        "var o = { m: Math }; o.m.floor = function () { return 0; }; var r = Math.floor(2.5);",
        "function f(g) { Object.defineProperty(g, 'floor', { value: 0 }); } f(Math); var r = Math.floor(2.5);",
    ] {
        let program = program(source);
        let facts = analyze(&program);
        assert!(!identities_of(&program, &facts).contains(&Identity::Builtin(builtin_path(&program, "Math", Some("floor")))), "{source}");
        assert!(!facts.builtin_identity(builtin_path(&program, "Math", Some("floor"))), "{source}");
    }
}

#[test]
fn a_string_method_the_program_reassigns_is_no_string_method_fact() {
    for (method, call) in [("charAt", "s.charAt(0)"), ("charCodeAt", "s.charCodeAt(0)"), ("substring", "s.substring(0, 1)"), ("slice", "s.slice(0, 1)")] {
        let base = format!("function f(s) {{ return {call}; }} f('abc');");
        assert_ne!(returned_type(&base), Type::ANY, "String.prototype.{method} is the host's, so its result is typed");
        let reassigned = format!("String.prototype.{method} = function () {{ return 0; }}; {base}");
        assert_eq!(returned_type(&reassigned), Type::ANY, "a store names String.prototype.{method}");
    }
}

#[test]
fn reassigning_an_unrelated_global_keeps_the_string_method_fact() {
    let source = "function f(s) { return s.charCodeAt(0); } f('abc'); parseInt = function () { return 0; };";
    assert_ne!(returned_type(source), Type::ANY, "parseInt is no String method");
}

#[test]
fn reassigning_a_builtin_member_drops_its_identity() {
    for name in ["abs", "ceil", "floor", "max", "min", "round", "sqrt", "trunc"] {
        let base = program(&format!("function f(x) {{ return Math.{name}(x); }} f(4);"));
        assert!(analyze(&base).builtin_identity(builtin_path(&base, "Math", Some(name))), "Math.{name} is the realm's when nothing writes it");
        let reassigned = program(&format!("function f(x) {{ return Math.{name}(x); }} f(4); Math.{name} = function () {{ return 0; }};"));
        assert!(!analyze(&reassigned).builtin_identity(builtin_path(&reassigned, "Math", Some(name))), "Math.{name} is stored");
    }
}

#[test]
fn reassigning_a_builtin_global_drops_its_identity() {
    // (the global, the read that makes the program name it, the statement that reassigns it)
    let rows = [
        ("Math", "Math.sqrt(x)", "Math = { sqrt: function () { return 0; } };"),
        ("Array", "Array(x)", "Array = function () { return 0; };"),
        ("Object", "Object(x)", "Object = function () { return 0; };"),
        ("String", "String(x)", "String = function () { return 0; };"),
        ("parseInt", "parseInt(x)", "parseInt = function () { return 0; };"),
        ("isNaN", "isNaN(x)", "isNaN = function () { return 0; };"),
    ];
    for (global, read, reassign) in rows {
        let base = program(&format!("function f(x) {{ return {read}; }} f(4);"));
        assert!(analyze(&base).builtin_identity(builtin_path(&base, global, None)), "{global} is the realm's when nothing writes it");
        let reassigned = program(&format!("function f(x) {{ return {read}; }} f(4); {reassign}"));
        assert!(!analyze(&reassigned).builtin_identity(builtin_path(&reassigned, global, None)), "{reassign}");
    }
}

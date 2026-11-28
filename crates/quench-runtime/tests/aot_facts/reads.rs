//! Captures, accessors, unions, fills and element stores.

use super::support::*;
use quench_runtime::compile::aot_facts::{analyze, world, FieldDefault};
use quench_runtime::compile::aot_facts::World;
use quench_runtime::staged::effects::{Indices};
use quench_runtime::typed_ir::{FactKey, Type};

#[test]
fn a_captured_slot_read_in_an_inner_function_takes_the_slots_settled_cell() {
    let program = program("function outer() { var n = 0; function inner() { return n; } n = 3; return inner(); } outer();");
    let facts = analyze(&program);
    let inner = function_named(&program, "inner");
    let outer = function_named(&program, "outer");
    let captures = facts.function_facts(inner as u32).capture_cells();
    assert_eq!(captures.len(), 1, "inner reads one captured slot of its parent");
    let (slot, cell) = captures[0];
    assert_eq!(cell, facts.captured_slot_cell(outer as u32, slot), "the read takes the slot's settled cell");
    assert!(cell.ty.is_subtype(Type::NUMBER), "the owner's stores of the slot are Numbers");
    assert!(facts.function_facts(inner as u32).dependencies().contains(&FactKey::Slot { owner: outer as u32, slot }), "the inner function depends on the slot");
}

#[test]
fn a_decrement_keeps_the_bound_on_the_index_it_lowers() {
    let program = program("function f(a) { var i = 0; while (i < a.length) { i += 1; i -= 1; a[i] = 0; } } f([0, 0, 0]);");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "`i - 1` after `i + 1` keeps `i` below the length at {pc}");
    }
}

#[test]
fn an_alias_of_the_array_keeps_the_bound_the_test_proved() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { var b = a; b[i] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "the copy `b` holds `a`'s bound at {pc}");
    }
}

#[test]
fn a_call_that_writes_no_element_of_the_array_keeps_its_bound() {
    let program = program("function g(b) { return b.length; } function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { g(1); a[i] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "a callee that writes no element keeps the bound at {pc}");
    }
}

#[test]
fn a_call_that_stores_into_the_array_retires_its_bound() {
    let program = program("function g(b) { b[0] = 1; } function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { g(a); a[i] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(!facts.function_facts(f as u32).below_length(pc, index, array), "a callee that stores into the array retires the bound at {pc}");
    }
}

#[test]
fn an_index_accessor_the_program_defines_retires_the_bound_on_element_accesses() {
    let program = program(
        "function f() { var a = [0, 0, 0]; Object.defineProperty(Array.prototype, '0', { get: function () { return 1; } }); \
         for (var i = 0; i < a.length; i++) { a[i] = i; } } f();",
    );
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(!facts.function_facts(f as u32).below_length(pc, index, array), "an index accessor may run at {pc}");
    }
}

#[test]
fn a_number_keyed_read_of_an_array_runs_nothing_in_a_closed_world() {
    let program = program("function f() { var a = [1, 2]; var i = 1; return a[i]; } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the function reads the array");
    for (pc, _, _) in accesses {
        let effects = facts.function_facts(f as u32).site_effects(pc);
        assert!(!effects.may_call_js && !effects.may_throw, "a Number-keyed read of a closed array runs no guest code at {pc}");
    }
}

#[test]
fn an_index_accessor_the_program_defines_keeps_an_element_read_open() {
    let program = program(
        "function f() { var a = [1, 2]; var i = 1; return a[i]; } f(); \
         Object.defineProperty(Array.prototype, '0', { get: function () { return 1; } });",
    );
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the function reads the array");
    for (pc, _, _) in accesses {
        assert!(facts.function_facts(f as u32).site_effects(pc).may_call_js, "an index accessor may run at {pc}");
    }
}

#[test]
fn a_constructor_store_of_an_inherited_setters_name_makes_no_field_of_it() {
    let program = program("(function () { class P { set x(v) { this.last = v; } } function C() { this.x = 7; this.y = 1; } Object.setPrototypeOf(C.prototype, P.prototype); new C(); })();");
    let facts = analyze(&program);
    let (c, x) = (function_named(&program, "C") as u32, atom_named(&program, "x"));
    match facts.class_layout(c) {
        None => {}
        Some(layout) => assert!(!layout.contains(&x), "the store runs the inherited setter, which makes no field of x"),
    }
}

#[test]
fn a_getter_read_through_a_subclass_instance_takes_the_superclass_getters_return() {
    let program = program("(function () { class P { get x() { return 3; } } class C extends P {} function f(o) { return o.x; } f(new C()); })();");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(facts.function_facts(f).return_cell().ty.is_subtype(Type::NUMBER), "the read of x on a C instance is P's getter");
}

#[test]
fn an_unread_global_this_and_an_eval_in_a_dead_function_keep_the_world_closed() {
    // The global object is held but never read by a computed or named key, and the eval sits in a function nothing calls:
    // no reached code opens the closed world, so its solve verifies.
    let program = program("var g = this; function dead() { eval('1'); } function f(a) { return a | 1; } f(2);");
    assert_eq!(world(&program), World::Closed);
}

#[test]
fn a_computed_read_of_a_global_this_reaches_the_global_functions_so_the_world_is_open() {
    // `g[k]` may name any global, including `f`, and calls what it reads: the closed world cannot see that call.
    let program = program("function f() { return 1; } var k = 'f'; var g = this; g[k]();");
    assert_eq!(world(&program), World::Open);
}

#[test]
fn a_named_read_of_the_global_object_may_call_a_global_function_so_the_world_is_open() {
    // `g.f` reads a global function off the global object by name, and the call through it is not a closed call.
    let program = program("function f() { return 1; } var g = this; f(); g.f();");
    assert_eq!(world(&program), World::Open);
}

#[test]
fn a_reached_eval_keeps_the_world_open() {
    // The eval runs at the top level, so new code is reached and the closed world's assumption fails.
    let program = program("function f(a) { return a | 1; } f(2); eval('1');");
    assert_eq!(world(&program), World::Open);
}

#[test]
fn a_union_lists_the_constructors_fields_then_the_fields_stored_later() {
    let program = program("function P(x) { this.x = x; } var p = new P(1); p.extra = 3;");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    let union = facts.class_union_layout(p).expect("a closed program has a union for P");
    assert_eq!(union_names(&union), vec![atom_named(&program, "x"), atom_named(&program, "extra")]);
    assert!(union.fields.iter().all(|(_, field)| field.default == FieldDefault::Absent), "no prototype names x or extra");
}

#[test]
fn a_name_stored_on_the_prototype_is_an_absent_fields_default() {
    let program = program("function P() { this.x = 1; } P.prototype.x = 0; var p = new P();");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    let union = facts.class_union_layout(p).expect("a closed program has a union for P");
    assert_eq!(union.fields.iter().map(|(_, field)| field.default).collect::<Vec<_>>(), vec![FieldDefault::Prototype(p)]);
}

#[test]
fn a_for_in_over_an_instance_has_no_union() {
    let program = program("function P() { this.x = 1; } var p = new P(); for (var k in p) {}");
    let facts = analyze(&program);
    assert!(facts.class_union_layout(function_named(&program, "P") as u32).is_none(), "for-in enumerates the instance");
}

#[test]
fn a_computed_name_store_on_an_instance_has_no_union() {
    let program = program("(function (k) { function P() { this.x = 1; } var p = new P(); p[k] = 2; })('y');");
    let facts = analyze(&program);
    assert!(facts.class_union_layout(function_named(&program, "P") as u32).is_none(), "a computed key may name any field");
}

#[test]
fn an_open_world_has_no_union() {
    let program = program("function P() { this.x = 1; } var p = new P(); eval('1');");
    let facts = analyze(&program);
    assert!(facts.class_union_layout(function_named(&program, "P") as u32).is_none());
}

#[test]
fn a_read_of_a_filled_array_below_its_length_takes_no_undefined() {
    let program = program("function f(n, k) { var a = new Array(n); for (var i = 0; i < n; i++) a[i] = 0; if (k < a.length) return a[k]; return 0; } f(3, 1);");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(facts.function_facts(f).return_cell().ty.is_subtype(Type::NUMBER), "the fill loop writes every element of the array");
}

#[test]
fn a_store_whose_index_steps_by_two_covers_the_whole_array() {
    let program = program("function f(i, x, w, j, c, n) { while (--n >= 0) { w[j] = x; j += 2; } return c; } f(0, 3, [0, 0, 0, 0], 0, 0, 4);");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let stores = element_stores(&program, f);
    assert_eq!(stores.len(), 1, "the loop stores into one array");
    let store = facts.function_facts(f as u32).element_store(stores[0]).expect("the store is recorded");
    assert_eq!(store.indices, Indices::Whole, "a step of two leaves no span of the count");
}

#[test]
fn constructors_sharing_one_class_factory_body_are_distinct_classes() {
    // The raytrace shape: `Class.create` returns one body for every class it makes, so the two constructors share it.
    let program = program(
        "var Class = { create: function() { return function(x) { this.x = x; }; } }; \
         var A = Class.create(); var B = Class.create(); \
         function make() { return [new A(1), new B(2)]; } make();",
    );
    let facts = analyze(&program);
    let make = function_named(&program, "make") as u32;
    let mut classes: Vec<u32> = facts.construct_classes.iter()
        .filter(|&(&(function, _), _)| function == make)
        .map(|(_, &class)| class)
        .collect();
    classes.sort();
    classes.dedup();
    assert_eq!(classes.len(), 2, "each constructor is its own class");
    let body = facts.classes.body(classes[0]);
    assert_eq!(body, facts.classes.body(classes[1]), "both classes run the factory's one body");
    assert!(classes.iter().all(|&class| class != body), "the classes are synthetic ids, not the body's id");
    for class in classes {
        assert!(facts.class_layout(class).is_some_and(|fields| fields.len() == 1), "each class keeps its own instance layout");
    }
}

#[test]
fn a_store_of_a_name_a_define_property_may_make_non_writable_stays_observed() {
    let program = program("function Proto() {} var p = new Proto(); p.x = 1; Object.defineProperty(Proto.prototype, 'x', {writable: false});");
    let facts = analyze(&program);
    assert!(facts.field_has_reader(FactKey::AnyField(atom_named(&program, "x"))), "a defineProperty may make x non-writable, so a store of x fails rather than writes");
}

#[test]
fn every_store_stays_observed_once_a_builtin_may_freeze_an_object() {
    let program = program("var p = {}; p.x = 1; p.y = 2; Object.freeze(p);");
    let facts = analyze(&program);
    for name in ["x", "y"] {
        assert!(facts.field_has_reader(FactKey::AnyField(atom_named(&program, name))), "a frozen object rejects a store of {name}, so the store is observed");
    }
}

#[test]
fn an_advect_clamped_index_read_has_a_range_invariant_in_its_loop() {
    let program = program(
        "function advect(d0, width, u, j0, rowSize, Wdt0, n) { \
           var Wp5 = width + 0.5; var row1 = j0 * rowSize; var s = 0; \
           for (var i = 0; i < n; i++) { \
             var x = i - Wdt0 * u[i]; \
             if (x < 0.5) x = 0.5; else if (x > Wp5) x = Wp5; \
             var i0 = x | 0; \
             s += d0[i0 + row1]; \
           } \
           return s; \
         } advect([0, 0, 0, 0, 0, 0, 0, 0], 4, [0, 0, 0, 0], 1, 4, 1, 4);",
    );
    let facts = analyze(&program);
    let advect = function_named(&program, "advect");
    let reads = element_reads(&program, advect);
    let function = facts.function_facts(advect as u32);
    assert!(
        reads.iter().any(|&pc| function.index_ceiling(pc).is_some_and(|range| range.integral)),
        "a read of the clamped, truncated index has an integral range invariant in its loop",
    );
}

#[test]
fn a_store_into_a_builtin_prototype_makes_its_index_properties_non_empty() {
    let program = program("var a = new Array(3); a[0] = 1; Array.prototype[0] = 1; a[1];");
    assert!(!analyze(&program).prototype_indices_empty(), "Array.prototype gains an index property the frame array's holes would read");
}

#[test]
fn a_program_with_no_store_into_a_builtin_prototype_keeps_its_index_properties_empty() {
    let program = program("var a = new Array(3); a[0] = 1; a[1];");
    assert!(analyze(&program).prototype_indices_empty(), "no builtin prototype gains an index property");
}

#[test]
fn a_global_stored_once_at_the_root_before_its_reader_is_frozen() {
    let program = program("var K = 3; function f() { return K; } f();");
    let facts = analyze(&program);
    assert!(facts.dominated_root_store(FactKey::Global(atom_named(&program, "K"))).is_some(), "the one store dominates f's read");
}

/// `p ? p : d` reloads the parameter's frame local after the test (a function that makes a closure keeps its parameters in
/// frame locals): the truthy edge proves the local not nullish, and the reload reads that proof, so the `undefined` no call
/// passes does not join the default (a closure joined with it becomes an unknown function, which escapes every class).
#[test]
fn a_reload_of_a_tested_local_keeps_the_tests_proof() {
    let ty = returned_type("(function () { function f(p) { var g = function () {}; g(); return p ? p : 1; } f(); })();");
    assert!(ty != Type::NEVER && !ty.may_be_languages(Type::language(quench_runtime::ValueType::Undefined)), "the reload reads no undefined: {ty:?}");
}

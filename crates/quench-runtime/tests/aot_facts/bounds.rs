//! Element accesses and index bounds: a bound is kept while the array's length is not retired.

use super::support::*;
use quench_runtime::compile::aot_facts::{analyze, world};
use quench_runtime::compile::aot_facts::World;
use quench_runtime::staged::effects::{Affine, Indices};
use quench_runtime::typed_ir::{Type};
use quench_runtime::heap::TypedArrayKind;

#[test]
fn an_index_tested_against_an_arrays_length_is_below_it_in_the_loop_body() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { a[i] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the loop stores into the array");
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "the store at {pc} is below the length");
    }
}

#[test]
fn an_index_tested_against_a_constant_is_below_no_length() {
    let program = program("function f(a) { for (var i = 0; i < 10; i++) { a[i] = i; } } f([0, 0, 0, 0]);");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(!facts.function_facts(f as u32).below_length(pc, index, array), "a constant bound proves no length");
    }
}

#[test]
fn a_push_in_the_loop_retires_the_bound_on_its_array() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { a.push(i); a[i] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(!facts.function_facts(f as u32).below_length(pc, index, array), "a push may resize the array before the store at {pc}");
    }
}

#[test]
fn a_store_to_the_length_retires_the_bound_on_its_array() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i < a.length; i++) { a.length = 0; a[i] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(!facts.function_facts(f as u32).below_length(pc, index, array), "a length store retires the bound before {pc}");
    }
}

#[test]
fn a_call_keeps_the_bound_on_a_string_whose_length_never_changes() {
    let program = program("function f() { var s = 'abc'; var n = 0; for (var i = 0; i < s.length; i++) { n = s.charCodeAt(0); n = s[i]; } return n; } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the loop reads the string");
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "the read at {pc} is below the string's length");
    }
}

#[test]
fn an_index_tested_past_a_constant_is_below_the_length_by_that_constant() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i + 1 < a.length; i++) { a[i] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length_offset(pc, index, array, 1), "`i + 1 < length` puts `i + 1` below the length at {pc}");
        assert!(!facts.function_facts(f as u32).below_length_offset(pc, index, array, 2), "and not two past it at {pc}");
    }
}

#[test]
fn an_index_plus_a_constant_below_the_length_is_below_it_through_its_sum() {
    let program = program("function f() { var a = [0, 0, 0]; for (var i = 0; i + 1 < a.length; i++) { a[i + 1] = i; } } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty());
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "the sum `i + 1` is below the length at {pc}");
    }
}

#[test]
fn a_getter_read_off_its_one_class_takes_the_getters_return() {
    let program = program("(function () { class C { get x() { return 3; } } var c = new C(); function f() { return c.x; } f(); })();");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(facts.function_facts(f).return_cell().ty.is_subtype(Type::NUMBER), "the read is a Known call of the getter");
}

#[test]
fn a_data_store_of_a_getters_name_on_its_class_shadows_the_getter() {
    let program = program("(function () { class C { get x() { return 3; } } var c = new C(); c.x = 'str'; function f() { return c.x; } f(); })();");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(!facts.function_facts(f).return_cell().ty.is_subtype(Type::NUMBER), "the store may be what the read takes");
}

#[test]
fn a_setter_store_passes_the_stored_value_as_the_setters_argument() {
    let program = program("(function () { class C { set x(v) { this.last = v; } } var c = new C(); function f() { c.x = 7; } f(); })();");
    let facts = analyze(&program);
    assert!(facts.field_type(atom_named(&program, "last")).is_subtype(Type::INT32), "the setter's parameter is the stored 7");
}

#[test]
fn a_method_read_off_a_subclass_instance_folds_to_the_superclass_method() {
    let program = program("(function () { class P { m() { return 1; } } class C extends P {} function f(o) { return o.m(); } f(new C()); })();");
    let facts = analyze(&program);
    let (f, m) = (function_named(&program, "f") as u32, function_named(&program, "m") as u32);
    assert!(facts.call_targets.iter().any(|(&(function, _), &target)| function == f && target == m), "the chain holds P's method");
}

#[test]
fn an_element_of_a_typed_array_of_one_kind_is_that_kind() {
    let program = program("function f() { var a = new Float64Array(4); var s = 0; for (var i = 0; i < 4; i++) { s += a[i]; } return s; } f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the loop reads the typed array");
    for (pc, _, _) in accesses {
        assert_eq!(facts.function_facts(f as u32).element_view(pc), Some(TypedArrayKind::Float64), "the read at {pc} is a Float64 element");
    }
}

#[test]
fn an_index_below_a_bound_that_is_within_the_length_is_below_it_through_the_chain() {
    let program = program("function f(a) { for (var j = 0; j < a.length; j++) { for (var i = 0; i < j; i++) { a[i] = 0; } } } f([0, 0, 0]);");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the inner loop stores into the array");
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "`i < j` with `j` within the length proves `i` at {pc}");
    }
}

#[test]
fn an_element_of_an_array_that_may_be_plain_or_typed_has_no_view() {
    let program = program("function f(c) { var a = c ? new Int32Array(2) : [1, 2]; return a[0]; } f(true);");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the function reads the array");
    for (pc, _, _) in accesses {
        assert_eq!(facts.function_facts(f as u32).element_view(pc), None, "a plain array beside a typed one is no one kind at {pc}");
    }
}

#[test]
fn the_midpoint_of_two_bounded_indices_is_below_the_larger() {
    let program = program(
        "function f(a) { var lo = 0, hi = a.length - 1; while (lo < hi) { var mid = (lo + hi) >>> 1; a[mid] = 0; if (a[mid] < 0) lo = mid + 1; else hi = mid; } } f([0, 0, 0]);",
    );
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the search reads the array at its midpoint");
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "the midpoint is below the length at {pc}");
    }
}

#[test]
fn a_read_of_an_unfilled_array_below_its_length_keeps_undefined() {
    let program = program("function f(n, k) { var a = new Array(n); if (k < a.length) return a[k]; return 0; } f(3, 1);");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(!facts.function_facts(f).return_cell().ty.is_subtype(Type::NUMBER), "no element of the array is written, so a read may find a hole");
}

#[test]
fn a_fill_loop_that_stops_short_of_the_length_keeps_the_holes() {
    let program = program("function f(n, k) { var a = new Array(n); for (var i = 0; i < n - 1; i++) a[i] = 0; if (k < a.length) return a[k]; return 0; } f(3, 1);");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(!facts.function_facts(f).return_cell().ty.is_subtype(Type::NUMBER), "the last element is never written, so a read may find a hole");
}

#[test]
fn a_loop_bounded_by_the_field_the_array_was_allocated_with_is_below_its_length() {
    let program = program("function C() { this.t = 3; } C.prototype.f = function f() { var a = new Array(this.t); for (var i = 0; i < this.t; i++) { a[i] = 0; } return a[1]; } new C().f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the loop stores into the array");
    for (pc, index, array) in accesses {
        assert!(facts.function_facts(f as u32).below_length(pc, index, array), "`this.t` loaded again is the length `new Array(this.t)` made, at {pc}");
    }
}

#[test]
fn a_store_to_the_field_between_the_allocation_and_the_loop_retires_the_bound() {
    let program = program("function C() { this.t = 3; } C.prototype.f = function f() { var a = new Array(this.t); this.t = 5; for (var i = 0; i < this.t; i++) { a[i] = 0; } return 0; } new C().f();");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let accesses = element_accesses(&program, f);
    assert!(!accesses.is_empty(), "the loop stores into the array");
    for (pc, index, array) in accesses {
        assert!(!facts.function_facts(f as u32).below_length(pc, index, array), "the store to `this.t` kills the load the allocation read, so the store at {pc} is not proven");
    }
}

#[test]
fn a_sloppy_prototype_method_keyed_on_this_called_as_a_method_keeps_the_world_closed() {
    // `this` of `m` is the object `o.m()` passes, never the global object, so `this[k]` reads no global function.
    let program = program("function C() {} C.prototype.m = function (k) { return this[k]; }; var o = new C(); o.m('x'); \
        function dead() { eval('1'); }");
    assert_eq!(world(&program), World::Closed);
}

#[test]
fn a_sloppy_function_keyed_on_this_handed_to_a_builtin_callback_opens_the_world() {
    // `forEach` calls `f` with an undefined `this`, which a sloppy `f` binds to the global object: `this[k]` may name the
    // global function `g`, which holds an eval nothing calls, so only the flagged `this` opens the world.
    let program = program("var k = 'g'; function f() { return this[k]; } [1].forEach(f); \
        function g() { eval('1'); }");
    assert_eq!(world(&program), World::Open);
}

#[test]
fn a_strict_function_keyed_on_this_handed_to_a_builtin_callback_keeps_the_world_closed() {
    // A strict callee's `this` is the `undefined` `forEach` passes, not the global object, so `this[k]` reads no global.
    let program = program("var k = 'g'; function f() { 'use strict'; return this[k]; } [1].forEach(f); \
        function dead() { eval('1'); }");
    assert_eq!(world(&program), World::Closed);
}

#[test]
fn a_prototype_method_last_written_at_top_level_resolves_for_a_later_run_once_read() {
    let program = program(
        "function Foo() {} function first() { return 1; } function second() { return 2; } \
         Foo.prototype.m = first; Foo.prototype.m = second; \
         function run() { var method = new Foo().m; return method(); } run();",
    );
    let facts = analyze(&program);
    let run = function_named(&program, "run") as u32;
    let second = function_named(&program, "second") as u32;
    assert!(facts.call_targets.iter().any(|(&(function, _), &target)| function == run && target == second));
}

#[test]
fn a_prototype_method_written_again_by_a_hot_function_names_no_phase_target() {
    let program = program(
        "function Foo() {} function first() { return 1; } function second() { return 2; } \
         Foo.prototype.m = first; \
         function hot() { Foo.prototype.m = second; } \
         for (var i = 0; i < 2; i++) hot(); \
         function run() { var method = new Foo().m; return method(); } run();",
    );
    let facts = analyze(&program);
    let run = function_named(&program, "run") as u32;
    assert!(!facts.call_targets.keys().any(|&(function, _)| function == run));
}

#[test]
fn a_store_through_an_index_counted_up_while_a_count_counts_down_covers_the_count_from_the_index() {
    let program = program("function f(i, x, w, j, c, n) { while (--n >= 0) { var v = x * 2 + w[j] + c; c = Math.floor(v / 67108864); w[j++] = v & 0x3ffffff; } return c; } f(0, 3, [0, 0, 0, 0], 0, 0, 4);");
    let facts = analyze(&program);
    let f = function_named(&program, "f");
    let stores = element_stores(&program, f);
    assert_eq!(stores.len(), 1, "the loop stores into one array");
    let store = facts.function_facts(f as u32).element_store(stores[0]).expect("the store is recorded");
    assert_eq!(store.indices, Indices::Span { start: Affine::parameter(3), len: Affine::parameter(5) }, "the store covers `j .. j + n`");
}

//! Class layouts, fixed cells and the global object's identity.

use super::support::*;
use quench_runtime::compile::aot_facts::{analyze};
use quench_runtime::typed_ir::{ElementClass, FactKey, Identity, Length};

#[test]
fn a_fixed_shape_constructor_has_its_layout() {
    let program = program("function P(x) { this.x = x; this.y = x | 0; } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let (p, make) = (function_named(&program, "P") as u32, function_named(&program, "make") as u32);
    let (x, y) = (atom_named(&program, "x"), atom_named(&program, "y"));
    assert_eq!(facts.class_layout(p), Some(&[x, y][..]));
    assert_eq!(facts.function_facts(make).field_slot(p, y), Some(1));
    assert_eq!(facts.function_facts(make).field_slot(p, atom_named(&program, "z")), None);
}

#[test]
fn a_field_stored_on_one_path_only_has_no_layout() {
    let program = program("function P(x) { if (x) { this.a = 1; } this.b = 2; } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), None);
}

#[test]
fn a_property_added_to_an_instance_after_construction_has_no_layout() {
    let built = program("function P(x) { this.x = x; } var p = new P(1); p.x = 2;");
    let facts = analyze(&built);
    let p = function_named(&built, "P") as u32;
    assert_eq!(facts.class_layout(p), Some(&[atom_named(&built, "x")][..]), "a store of a named field keeps the layout");

    let added = program("function P(x) { this.x = x; } var p = new P(1); p.extra = 3;");
    let facts = analyze(&added);
    let p = function_named(&added, "P") as u32;
    assert_eq!(facts.class_layout(p), None);
}

#[test]
fn a_method_installed_on_the_prototype_is_its_prototype_layout() {
    let program = program("function P(x) { this.x = x; } P.prototype.run = function () { return 1; }; new P(1);");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.prototype_layout(p), Some(&[atom_named(&program, "run")][..]));
}

#[test]
fn a_call_before_a_constructors_last_store_has_no_layout() {
    let program = program("function f() { return 1; } function P(x) { this.a = x; f(); this.b = 2; } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), None, "a callee may store a name on the instance before the constructor's next store");
}

#[test]
fn stores_before_a_call_keep_a_constructors_layout() {
    let program = program("function f() { return 1; } function P(x) { this.a = x; this.b = 2; f(); } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), Some(&[atom_named(&program, "a"), atom_named(&program, "b")][..]));
}

#[test]
fn storing_this_into_a_global_before_a_last_store_has_no_layout() {
    let program = program("var keep; function P(x) { this.a = x; keep = this; this.b = 2; } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), None, "a store of this into a global lets other code reach the instance");
}

#[test]
fn a_derived_constructor_has_no_instance_layout() {
    let program = program("class B { constructor(x) { this.x = x; } } class D extends B { constructor(x) { super(x); } } function make() { return new D(1); } make();");
    let facts = analyze(&program);
    let d = function_named(&program, "D") as u32;
    assert_eq!(facts.class_layout(d), None, "super() runs the parent's stores on the instance");
}

#[test]
fn a_constructor_that_makes_an_array_of_a_declared_size_last_keeps_its_layout() {
    let program = program("var DATA_SIZE = 4; function Packet(l) { this.link = l; this.id = 1; this.a2 = new Array(DATA_SIZE); } function make() { return new Packet(null); } make();");
    let facts = analyze(&program);
    let p = function_named(&program, "Packet") as u32;
    assert_eq!(
        facts.class_layout(p),
        Some(&[atom_named(&program, "link"), atom_named(&program, "id"), atom_named(&program, "a2")][..]),
        "Array(n) of a Number runs no guest code, and a declared global read runs none"
    );
}

#[test]
fn a_conditional_on_null_keeps_a_constructors_layout() {
    let program = program("var A = 1; var B = 2; function Tcb(q) { this.packet = q; this.state = q == null ? A : B; } function make() { return new Tcb(null); } make();");
    let facts = analyze(&program);
    let t = function_named(&program, "Tcb") as u32;
    assert_eq!(facts.class_layout(t), Some(&[atom_named(&program, "packet"), atom_named(&program, "state")][..]));
}

#[test]
fn a_read_of_a_field_the_path_stored_keeps_a_constructors_layout() {
    let program = program("function P(x) { this.a = x; this.b = this.a; } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), Some(&[atom_named(&program, "a"), atom_named(&program, "b")][..]));
}

#[test]
fn a_call_to_an_unknown_function_before_the_last_store_has_no_layout() {
    let program = program("function P(x) { this.a = x; unknownHostFn(); this.b = 2; } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), None);
}

#[test]
fn a_computed_name_store_on_an_array_keeps_a_layout_while_no_global_object_is_reachable() {
    let program = program(&array_store_program(""));
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), Some(&[atom_named(&program, "a"), atom_named(&program, "b")][..]));
}

#[test]
fn a_computed_name_store_into_the_global_object_loses_a_layout_that_makes_an_array() {
    let program = program(&array_store_program("function setg(k, v) { globalThis[k] = v; } setg('zz', 1);"));
    let facts = analyze(&program);
    let p = function_named(&program, "P") as u32;
    assert_eq!(facts.class_layout(p), None, "the global object is reachable, so Array may be rebound before the store");
}

#[test]
fn an_array_a_resolved_callee_pushes_keeps_its_own_class() {
    // `add` stores into its parameter, which a resolved call passes the caller's `xs`: the push reaches `xs`'s class, and
    // the array does not join the unnamed class.
    let program = program("(function () { function add(a) { a.push(1); } var xs = [0]; add(xs); return xs[0]; })();");
    let facts = analyze(&program);
    let grown = facts.cells.iter().any(|(key, cell)| matches!(key, FactKey::Elements(ElementClass::Allocation { .. })) && cell.elements.length == Length::Any);
    assert!(grown, "the callee's push reaches the caller's array's class");
    let unnamed = facts.cells.get(&FactKey::Elements(ElementClass::Unnamed)).is_none_or(|cell| cell.elements.length != Length::Any);
    assert!(unnamed, "the array does not join the unnamed class");
}

#[test]
fn a_pushed_literal_is_unfixed_and_an_untouched_one_keeps_its_length() {
    let program = program("(function () { var a = [1]; a.push(2); var b = [3]; return b[0]; })();");
    let facts = analyze(&program);
    let lengths: Vec<Length> = facts.cells.iter()
        .filter(|(key, _)| matches!(key, FactKey::Elements(ElementClass::Allocation { .. })))
        .map(|(_, cell)| cell.elements.length)
        .collect();
    assert!(lengths.contains(&Length::Any), "the pushed literal's length is unknown");
    assert!(lengths.iter().any(|length| matches!(length, Length::Fixed(_))), "the untouched literal keeps its fixed length");
}

#[test]
fn a_parent_of_a_derived_class_stays_a_fixed_cell_and_the_derived_class_is_none() {
    let program = program("function Scheduler() { this.count = 0; } class Timer extends Scheduler { constructor() { super(); } } \
                           var s = new Scheduler(); var t = new Timer();");
    let facts = analyze(&program);
    assert!(facts.class_fixed_cell(function_named(&program, "Scheduler") as u32), "super() builds a Timer, whose class word is Timer's");
    assert!(!facts.class_fixed_cell(function_named(&program, "Timer") as u32), "a derived class builds no instance in its own frame");
}

#[test]
fn a_frozen_instance_keeps_its_slots_for_reads_but_not_for_stores() {
    let program = program("function Scheduler() { this.count = 0; } var s = new Scheduler(); Object.freeze(s); s.count;");
    let facts = analyze(&program);
    let class = function_named(&program, "Scheduler") as u32;
    assert!(facts.class_fixed_cell(class), "a flag transition moves no slot");
    assert!(!facts.class_writable_cell(class), "a frozen instance rejects stores");
}

#[test]
fn a_program_naming_reflect_construct_has_no_fixed_cell() {
    let program = program("function Scheduler() { this.count = 0; } var s = Reflect.construct(Scheduler, []);");
    let facts = analyze(&program);
    assert!(!facts.class_fixed_cell(function_named(&program, "Scheduler") as u32), "Reflect.construct takes any new_target");
}

#[test]
fn a_script_top_level_this_is_the_global_object() {
    let program = program("var t = this; t;");
    assert!(identities_of(&program, &analyze(&program)).contains(&Identity::GlobalObject), "a Script's root this is the realm's global object");
}

#[test]
fn a_by_name_read_of_global_this_is_the_global_object() {
    let program = program("var g = globalThis; g.k;");
    assert!(identities_of(&program, &analyze(&program)).contains(&Identity::GlobalObject), "globalThis is the global object, not a builtin");
}

#[test]
fn a_program_that_writes_global_this_proves_no_global_object() {
    let program = program("globalThis = 1; var g = globalThis;");
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::GlobalObject), "the program writes globalThis, so its reads are Many");
}

#[test]
fn a_sloppy_function_only_called_plainly_has_the_global_object_as_this() {
    let program = program("function f() { return this; } f(); f();");
    assert!(identities_of(&program, &analyze(&program)).contains(&Identity::GlobalObject), "every call of f is plain, and f is sloppy");
}

#[test]
fn a_strict_function_only_called_plainly_has_no_global_object_as_this() {
    let program = program("function f() { 'use strict'; return this; } f(); f();");
    assert!(!identities_of(&program, &analyze(&program)).contains(&Identity::GlobalObject), "a strict callee's this is undefined");
}

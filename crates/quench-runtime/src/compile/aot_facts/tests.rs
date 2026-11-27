//! Closed-world facts of small scripts: call targets, numbers, and the gate.

use super::{analyze, AotFacts};
use crate::Engine;
use crate::bytecode::ResidualProgram;
use crate::typed_ir::{Cell, ElementClass, FactKey, Length, Representation, Type};
use crate::ValueType;

fn program(source: &str) -> ResidualProgram {
    Engine::specialize(source, "aot-facts.js").unwrap()
}

/// The id of the function named `name`.
fn function_named(program: &ResidualProgram, name: &str) -> usize {
    let named = |id: usize| {
        program.with_record(id, |function| {
            function.name.is_some_and(|atom| {
                let text: &str = program.atoms[atom as usize].as_ref();
                text == name
            })
        })
    };
    (0..program.function_count()).find(|&id| named(id)).expect("a function with that name")
}

#[test]
fn a_global_function_called_by_name_is_its_call_target() {
    let program = program("function f() { return 1; } function g() { return f(); } g();");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(facts.call_targets.values().any(|&target| target == f));
}

#[test]
fn a_binding_written_twice_names_no_call_target() {
    let program = program(
        "var f = function () { return 1; }; f = function () { return 2; }; \
         function g() { return f(); } g();",
    );
    let facts = analyze(&program);
    let g = function_named(&program, "g") as u32;
    assert!(!facts.call_targets.keys().any(|&(function, _)| function == g));
}

#[test]
fn a_program_that_can_eval_keeps_its_facts_as_guesses() {
    let program = program("function f() { return 1; } f(); eval('1');");
    let facts = analyze(&program);
    let f = function_named(&program, "f") as u32;
    assert!(facts.call_targets.values().any(|&target| target == f), "the global call is still solved");
    assert!(facts.guessed_calls.iter().any(|key| facts.call_targets.get(key) == Some(&f)), "a live global's target is a guess");
}

#[test]
fn a_new_of_a_global_constructor_is_its_construct_target() {
    let program = program("function P(x) { this.x = x; } function make() { return new P(2); } make();");
    let facts = analyze(&program);
    let (p, make) = (function_named(&program, "P") as u32, function_named(&program, "make") as u32);
    assert!(facts.construct_targets.iter().any(|(&(function, _), &target)| function == make && target == p));
    assert!(!facts.call_targets.values().any(|&target| target == p), "a construction is no plain call target");
}

#[test]
fn an_int32_register_is_proven_int32() {
    let program = program("function n(a) { var i = 0; i = i | 3; return i; }");
    let facts = analyze(&program);
    let id = function_named(&program, "n");
    let numbers = facts.function_facts(id as u32).numbers();
    assert!(numbers.is_some_and(|numbers| numbers.contains(&Some(Representation::Int32))));
}

#[test]
fn a_field_takes_the_join_of_its_stores_through_reads_and_returns() {
    // Wrapped in a function so its constructor and methods are slots, not globals: a global is read by name by code an open
    // world may add, so it is a guess there (`types::escapes`).
    let program = program("(function () { function T(n) { this.link = null; this.count = 0; this.next = n; }
        T.prototype.run = function () { this.count = this.count | 1; return this.next; };
        function S() { this.current = null; }
        S.prototype.go = function () { this.current = new T(null); while (this.current != null) { this.current = this.current.run(); } };
        new S().go(); })();");
    let facts = analyze(&program);
    let atom = |name: &str| program.atoms.iter().position(|atom| atom.as_ref() == name).unwrap() as u32;
    let nullable_object = Type::DIRECT_OBJECT_FAMILY.join(Type::language(crate::ValueType::Null));
    assert!(facts.field_type(atom("count")).is_subtype(Type::INT32));
    assert_eq!(facts.field_type(atom("link")), Type::language(crate::ValueType::Null));
    assert_eq!(facts.field_type(atom("current")), nullable_object);
}

#[test]
fn a_field_takes_the_type_of_the_argument_its_constructor_is_called_with() {
    // Wrapped in a function, as the test above, so the constructor is a slot of the program's own code.
    let program = program("(function () { function P(v) { this.v = v; } function mk() { return new P(3); } mk(); mk(); })();");
    let facts = analyze(&program);
    let v = program.atoms.iter().position(|atom| atom.as_ref() == "v").expect("the field name") as u32;
    assert!(facts.field_type(v).is_subtype(crate::typed_ir::Type::INT32));
}

/// The settled cell of parameter `index` of the function named `name`.
fn parameter(program: &ResidualProgram, facts: &super::AotFacts, name: &str, index: u16) -> Cell {
    let function = function_named(program, name) as u32;
    facts.cell(FactKey::Parameter { function, index }).expect("a settled parameter")
}

#[test]
fn a_function_only_recorded_calls_reach_keeps_its_parameter_types() {
    let program = program("(function () { function f(a) { return a | 1; } f(2); f(3); })();");
    let facts = analyze(&program);
    assert!(parameter(&program, &facts, "f", 0).ty.is_subtype(Type::INT32));
}

#[test]
fn a_function_a_native_calls_takes_any_parameter_and_this() {
    // `cb` is reached by a recorded call and by `forEach`, whose callee the solve cannot resolve: it escapes.
    let program = program("(function () { function cb(a) { return a | 1; } cb(2); [1].forEach(cb); })();");
    let facts = analyze(&program);
    assert_eq!(parameter(&program, &facts, "cb", 0).ty, Type::ANY);
    let cb = function_named(&program, "cb") as u32;
    assert_eq!(facts.cell(FactKey::Receiver(cb)).map(|cell| cell.ty), Some(Type::ANY));
}

#[test]
fn a_global_function_escapes_in_an_open_world() {
    // `eval` may add code, which reads globals by name and may call `f`: a global function escapes.
    let program = program("function f(a) { return a | 1; } f(2); f(3); eval('1');");
    let facts = analyze(&program);
    assert_eq!(parameter(&program, &facts, "f", 0).ty, Type::ANY);
}

#[test]
fn a_richards_shaped_class_keeps_its_int32_parameters_in_a_closed_world() {
    // Top-level code, as richards is: the constructor and its prototype methods are globals and stored on globals, which no
    // code of a closed world reads by name, so nothing escapes; `this.m(3)` runs the class's own method.
    let program = program("function Scheduler() { this.count = 0; } \
        Scheduler.prototype.m = function m(x) { return x | 1; }; \
        Scheduler.prototype.run = function run() { return this.m(3); }; \
        new Scheduler().run();");
    let facts = analyze(&program);
    assert!(parameter(&program, &facts, "m", 0).ty.is_subtype(Type::INT32));
    let (scheduler, run) = (function_named(&program, "Scheduler") as u32, function_named(&program, "run") as u32);
    assert!(facts.cell(FactKey::Receiver(run)).is_some_and(|cell| cell.classes.may_hold(scheduler)));
}

#[test]
fn a_host_callee_makes_no_program_closure_escape() {
    // `Math.abs` is read off a key no program write reaches: it holds no program closure, so `f` stays a closed slot.
    let program = program("(function () { function f(a) { return a | 1; } f(2); Math.abs(1); })();");
    let facts = analyze(&program);
    assert!(parameter(&program, &facts, "f", 0).ty.is_subtype(Type::INT32));
}

#[test]
fn a_builtin_method_named_as_a_program_method_takes_its_closure_argument() {
    // `[1].map` is the array's own method: the program's `map` does not run, so its callback `cb` escapes.
    let program = program("(function () { function C() {} C.prototype.map = function map(f) { return f(1); }; \
        function cb(a) { return a | 1; } cb(2); [1].map(cb); })();");
    let facts = analyze(&program);
    assert_eq!(parameter(&program, &facts, "cb", 0).ty, Type::ANY);
}

/// The cells the registers of function `name` hold before its last instruction (its return), where the solve settled them.
fn cells_before_return(facts: &AotFacts, program: &ResidualProgram, name: &str) -> Vec<Cell> {
    let id = function_named(program, name);
    let (last, registers) = program.with_record(id, |function| (function.code.len() as u32 - 1, usize::from(function.registers)));
    let function = facts.function_facts(id as u32);
    (0..registers).filter_map(|register| function.cell_at(last, register as u16)).collect()
}

#[test]
fn a_pop_of_a_literal_makes_its_length_any_in_every_register_holding_it() {
    let program = program("function f() { var a = [1, 2]; var b = a; a.pop(); return b; } f();");
    let facts = analyze(&program);
    let cells = cells_before_return(&facts, &program, "f");
    assert!(!cells.is_empty(), "the function's registers are settled");
    assert!(cells.iter().all(|cell| cell.elements.length != Length::Fixed(2)), "{cells:?}");
}

#[test]
fn a_string_store_into_an_array_makes_its_slot_class_elements_include_string() {
    // The literal is the slot `a`'s class (`ElementClass::Slot`), so its element store settles there.
    let program = program("function f() { var a = [1, 2]; a[0] = 'x'; return a; } f();");
    let facts = analyze(&program);
    let cell = facts.cells.iter()
        .find(|(key, _)| matches!(key, FactKey::Elements(ElementClass::Slot { .. })))
        .map(|(_, cell)| *cell)
        .expect("the slot's element class is a settled cell");
    assert!(Type::language(ValueType::String).is_subtype(cell.ty), "{cell:?}");
}

/// The element classes other than the unnamed one that share its elements (`AotFacts::element_class_keys`).
fn classes_sharing_unnamed(facts: &AotFacts) -> Vec<FactKey> {
    let unnamed = FactKey::Elements(ElementClass::Unnamed);
    facts.cells.keys().copied().filter(|key| *key != unnamed && facts.element_class_keys(*key).contains(&unnamed)).collect()
}

#[test]
fn two_literals_keep_their_own_lengths_when_only_one_is_pushed() {
    let program = program("function f() { var a = [1, 2]; var b = [3, 4]; a.push(5); return b; } f();");
    let facts = analyze(&program);
    let cells = cells_before_return(&facts, &program, "f");
    assert!(cells.iter().any(|cell| cell.elements.length == Length::Fixed(2)), "{cells:?}");
}

#[test]
fn an_array_passed_to_an_unresolved_call_is_in_the_unnamed_class() {
    // `print` is a host function no program writes, so the call resolves to no program code and its argument escapes.
    let program = program("function f() { var a = [1, 2]; print(a); a[0] = 3; return a; } f();");
    let facts = analyze(&program);
    assert!(!classes_sharing_unnamed(&facts).is_empty(), "the escaped array's class joins the unnamed class");
}

#[test]
fn a_store_through_a_branch_join_is_seen_through_each_array_it_may_be() {
    // `x` is `a` or the second literal, so the store through `x` (out of bounds, the join's length is any) changes `a` too.
    let program = program("function f(c) { var a = [1]; var x = c ? a : [2, 3]; x[0] = 9; return a; } f(true);");
    let facts = analyze(&program);
    let cells = cells_before_return(&facts, &program, "f");
    assert!(cells.iter().all(|cell| cell.elements.length != Length::Fixed(1)), "{cells:?}");
}

#[test]
fn an_array_no_unresolved_code_reaches_keeps_its_own_class() {
    let program = program("function f() { var a = [1, 2]; a[0] = 3; return a; } f();");
    let facts = analyze(&program);
    assert!(classes_sharing_unnamed(&facts).is_empty(), "{:?}", classes_sharing_unnamed(&facts));
}

#[test]
fn an_array_stored_in_a_field_is_mutated_through_the_field_in_the_original_register() {
    let program = program("function f() { var a = [1, 2]; var o = {}; o.x = a; o.x.pop(); return a; } f();");
    let facts = analyze(&program);
    let cells = cells_before_return(&facts, &program, "f");
    assert!(!cells.is_empty(), "the function's registers are settled");
    assert!(cells.iter().all(|cell| cell.elements.length != Length::Fixed(2)), "{cells:?}");
}

/// The atom of the property name `name`, which the program names.
fn atom_named(program: &ResidualProgram, name: &str) -> u32 {
    program.atoms.iter().position(|atom| atom.as_ref() == name).expect("a name the program stores") as u32
}

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

/// A program whose constructor makes an array after its first store, and that stores a computed name on an array
/// elsewhere. `extra` adds the code that makes the global object a value.
fn array_store_program(extra: &str) -> String {
    format!(
        "function P(n) {{ this.a = 1; this.b = new Array(2); }} function fill(obj, k, x) {{ obj.list[k] = x; }} \
         function make() {{ var o = {{ list: [] }}; fill(o, 'q', 1); return new P(0); }} {extra} make();"
    )
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

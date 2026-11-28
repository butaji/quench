//! Closed-world facts of small scripts: call targets, numbers, and the gate.

use super::{analyze, world, AotFacts, BuiltinPath, FieldDefault};
use super::gate::World;
use crate::Engine;
use crate::bytecode::ResidualProgram;
use crate::staged::effects::{Affine, Indices};
use crate::staged::Operation;
use crate::typed_ir::{Cell, ElementClass, FactKey, Identity, Length, Representation, Type};
use crate::heap::TypedArrayKind;
use crate::ValueType;
#[cfg(not(stencil_build))]
#[allow(unused_imports)]
use crate::staged::glue_prelude::*;

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

/// The pc, index register and array register of each element access (a read or a store) of function `id`.
fn element_accesses(program: &crate::bytecode::ResidualProgram, id: usize) -> Vec<(u32, u16, u16)> {
    program.with_record(id, |function| {
        (0..function.code.len()).filter_map(|pc| match Operation::<u64, ()>::of(function.instruction_at(pc))? {
            Operation::ElementRead { base, key, .. } => Some((pc as u32, key.register_index()?, base)),
            Operation::ElementWrite { base, key, .. } => Some((pc as u32, key, base)),
            _ => None,
        }).collect()
    })
}

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

fn union_names(union: &super::UnionLayout) -> Vec<u32> {
    union.fields.iter().map(|(atom, _)| *atom).collect()
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

/// The pcs of the element stores (`ElementWrite`) of function `id`.
fn element_stores(program: &crate::bytecode::ResidualProgram, id: usize) -> Vec<u32> {
    program.with_record(id, |function| {
        (0..function.code.len()).filter(|&pc| matches!(Operation::<u64, ()>::of(function.instruction_at(pc)), Some(Operation::ElementWrite { .. })))
            .map(|pc| pc as u32).collect()
    })
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

/// The pcs of the element reads (`ElementRead`) of function `id`.
fn element_reads(program: &crate::bytecode::ResidualProgram, id: usize) -> Vec<u32> {
    program.with_record(id, |function| {
        (0..function.code.len()).filter(|&pc| matches!(Operation::<u64, ()>::of(function.instruction_at(pc)), Some(Operation::ElementRead { .. })))
            .map(|pc| pc as u32).collect()
    })
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

#[test]
fn a_global_read_before_its_store_is_not_frozen() {
    let program = program("function f() { return K; } f(); var K = 3;");
    let facts = analyze(&program);
    assert!(facts.dominated_root_store(FactKey::Global(atom_named(&program, "K"))).is_none(), "f runs before the store");
}

#[test]
fn a_global_stored_twice_is_not_frozen() {
    let program = program("var K = 3; K = 4; function f() { return K; } f();");
    let facts = analyze(&program);
    assert!(facts.dominated_root_store(FactKey::Global(atom_named(&program, "K"))).is_none(), "two stores write the key");
}

#[test]
/// The atom of the name `text` in `program`.
fn atom_of(program: &ResidualProgram, text: &str) -> u32 {
    program.atoms.iter().position(|atom| {
        let atom: &str = atom.as_ref();
        atom == text
    }).expect("an atom with that name") as u32
}

#[test]
fn an_unwritten_builtin_member_is_its_realm_builtin() {
    let program = program("Math.sqrt(4);");
    let binding = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "sqrt")) };
    assert!(analyze(&program).builtin_identity(binding), "no store of the program writes Math.sqrt");
}

#[test]
fn a_stored_builtin_member_is_not_its_realm_builtin() {
    let program = program("function f(x) { return x; } Math.sqrt = f; Math.sqrt(4);");
    let binding = BuiltinPath { global: atom_of(&program, "Math"), member: Some(atom_of(&program, "sqrt")) };
    assert!(!analyze(&program).builtin_identity(binding), "a store of the program writes Math.sqrt");
}

#[test]
fn a_top_level_function_is_one_closure() {
    let program = program("function f(x) { return x; } f(1);");
    let f = function_named(&program, "f") as u32;
    assert!(analyze(&program).closure_identity(f), "the root creates f once");
}

#[test]
fn a_closure_created_in_a_loop_is_not_one_closure() {
    let program = program("var fs = []; for (var i = 0; i < 3; i++) { fs.push(function () { return i; }); } fs.length;");
    let inner = (0..program.function_count()).find(|&id| program.with_record(id, |function| function.parent.is_some())).expect("a nested function") as u32;
    assert!(!analyze(&program).closure_identity(inner), "the loop creates a closure per iteration");
}

#[test]
fn a_method_only_called_on_its_class_instances_takes_that_class_as_this() {
    let program = program("function Scheduler() { this.count = 0; } \
        Scheduler.prototype.run = function run() { return this.count; }; \
        new Scheduler().run();");
    let facts = analyze(&program);
    let (scheduler, run) = (function_named(&program, "Scheduler") as u32, function_named(&program, "run") as u32);
    assert_eq!(facts.function_facts(run).this_classes().single().map(|(class, _)| class), Some(scheduler));
}

#[test]
fn an_instance_of_a_class_only_stored_by_name_keeps_a_writable_cell() {
    let program = program("function Scheduler() { this.count = 0; } var s = new Scheduler(); s.count = 1;");
    let facts = analyze(&program);
    assert!(facts.class_writable_cell(function_named(&program, "Scheduler") as u32));
}

#[test]
fn a_delete_on_an_instance_changes_its_class() {
    let program = program("function Scheduler() { this.count = 0; } var s = new Scheduler(); delete s.count;");
    let facts = analyze(&program);
    assert!(!facts.class_writable_cell(function_named(&program, "Scheduler") as u32));
}

#[test]
fn an_atom_no_method_call_names_is_uncached_in_a_closed_program() {
    let program = program("var q = 1; var o = { m: function () { return 2; } }; o.m();");
    let facts = analyze(&program);
    assert!(facts.uncached_atoms().contains(&atom_named(&program, "q")), "no method call site names q");
    assert!(!facts.uncached_atoms().contains(&atom_named(&program, "m")), "a method call site names m, which a method cache may hold");
}

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

/// The identity of every register `cell_at` answers for, in every function of `program`.
fn identities_of(program: &ResidualProgram, facts: &AotFacts) -> Vec<Identity> {
    let mut found = Vec::new();
    for id in 0..program.function_count() {
        let (len, registers) = program.with_record(id, |function| (function.code.len() as u32, usize::from(function.registers)));
        let function = facts.function_facts(id as u32);
        for pc in 0..len {
            for register in 0..registers {
                if let Some(cell) = function.cell_at(pc, register as u16) { found.push(cell.identity); }
            }
        }
    }
    found
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
/// The pc of function `name`'s last instruction (its return) and its register count.
fn last_pc_and_registers(program: &ResidualProgram, name: &str) -> (u32, usize) {
    let id = function_named(program, name);
    program.with_record(id, |function| (function.code.len() as u32 - 1, usize::from(function.registers)))
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

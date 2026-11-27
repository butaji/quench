//! Closed-world facts of small scripts: call targets, numbers, and the gate.

use super::analyze;
use crate::Engine;
use crate::bytecode::ResidualProgram;
use crate::typed_ir::Representation;

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
    let program = program("function T(n) { this.link = null; this.count = 0; this.next = n; }
        T.prototype.run = function () { this.count = this.count | 1; return this.next; };
        function S() { this.current = null; }
        S.prototype.go = function () { this.current = new T(null); while (this.current != null) { this.current = this.current.run(); } };
        new S().go();");
    let facts = analyze(&program);
    let atom = |name: &str| program.atoms.iter().position(|atom| atom.as_ref() == name).unwrap() as u32;
    use crate::typed_ir::Type;
    let nullable_object = Type::DIRECT_OBJECT_FAMILY.join(Type::language(crate::ValueType::Null));
    assert!(facts.field_type(atom("count")).is_subtype(Type::INT32));
    assert_eq!(facts.field_type(atom("link")), Type::language(crate::ValueType::Null));
    assert_eq!(facts.field_type(atom("current")), nullable_object);
}

#[test]
fn a_field_takes_the_type_of_the_argument_its_constructor_is_called_with() {
    let program = program("function P(v) { this.v = v; } function mk() { return new P(3); } mk(); mk();");
    let facts = analyze(&program);
    let v = program.atoms.iter().position(|atom| atom.as_ref() == "v").expect("the field name") as u32;
    assert!(facts.field_type(v).is_subtype(crate::typed_ir::Type::INT32));
}

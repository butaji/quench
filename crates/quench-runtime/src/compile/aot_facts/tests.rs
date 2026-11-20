//! Closed-world facts of small scripts: call targets, constructor shapes, numbers, and the gate.

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

fn atom_text(program: &ResidualProgram, atom: u32) -> String {
    let text: &str = program.atoms[atom as usize].as_ref();
    text.to_owned()
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
fn a_program_that_can_eval_has_no_facts() {
    let program = program("function f() { return 1; } f(); eval('1');");
    assert!(analyze(&program).is_empty());
}

#[test]
fn a_constructors_prologue_fields_are_its_shape() {
    let program = program("function P(x) { this.x = x; this.y = 1; } var p = new P(2);");
    let facts = analyze(&program);
    let id = function_named(&program, "P");
    let shape = facts
        .function_facts(id as u32)
        .constructor_shape()
        .map(|fields| fields.iter().map(|&atom| atom_text(&program, atom)).collect::<Vec<_>>());
    assert_eq!(shape, Some(vec!["x".to_owned(), "y".to_owned()]));
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

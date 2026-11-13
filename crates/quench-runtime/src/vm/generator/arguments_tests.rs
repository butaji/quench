//! The `arguments` object of a suspending function rides its window as a local, a mapped one
//! (`Function::arguments_alias_parameters`) too: its parameters live in the window's locals, and
//! the window's reads and writes of one go through the object (`lane_load_mapped_parameter`),
//! as the general path's do (`Vm::mapped_argument_load`). Compiled runs agree with the
//! interpreter's.
use super::window_steps::tests::{agree, run};

#[test]
fn a_generator_reading_arguments_resumes_in_its_window_without_frames() {
    let (lines, made) = run(
        "function* reading() { yield arguments.length; yield arguments[1]; yield arguments[2]; }
         let sum = 0;
         for (const x of reading(1, 2, 3)) sum += x;
         print(String(sum));",
    );
    assert_eq!(lines, ["8"]);
    assert_eq!(made, 0, "a resume made a frame");
}

#[test]
fn a_strict_generator_keeps_the_values_it_was_passed_across_yields() {
    let lines = agree(
        r#"function* strict(a) { "use strict"; a = 9; yield arguments[0]; yield a; yield arguments.length; }
           const out = [];
           for (const x of strict(1, 2)) out.push(x);
           print(out.join(","));"#,
    );
    assert_eq!(lines, ["1,9,2"]);
}

#[test]
fn a_sloppy_generator_mutates_its_mapped_arguments_across_yields() {
    // The element writes reach the parameter and the parameter's writes the element, each way
    // across suspensions, with no frame made by a resume.
    let source = r#"function* untouched(a, b) {
               arguments[0] = "x";
               yield arguments[0] + arguments.length;
               arguments[1] = "y";
               yield arguments[1];
               yield arguments.length;
           }
           function* aliased(a) { arguments[0] = 7; yield a; a = 9; yield arguments[0]; arguments[0] = 4; yield a; }
           function* both(a, b) {
               yield a + b;
               a = 10; arguments[1] = 20;
               yield arguments[0] + b;
               const seen = yield;
               arguments[0] = seen; b = seen + 1;
               yield a + arguments[1];
           }
           const out = [];
           for (const x of untouched(1, 2, 3)) out.push(x);
           for (const x of aliased(1)) out.push(x);
           const it = both(1, 2);
           out.push(it.next().value, it.next().value, it.next().value, it.next(5).value);
           print(out.join(","));"#;
    assert_eq!(agree(source), ["x3,y,3,7,9,4,3,30,,11"]);
    let (_, made) = run(source);
    assert_eq!(made, 0, "a resume made a frame");
}

#[test]
fn a_mapped_arguments_object_leaves_the_parameter_once_unmapped() {
    let lines = agree(
        r#"function* unmapped(a) { yield a; delete arguments[0]; arguments[0] = 5; a = 6; yield arguments[0] + a; }
           print([...unmapped(1)].join(","));"#,
    );
    assert_eq!(lines, ["1,11"]);
}

#[test]
fn an_async_function_reads_its_arguments_after_an_await() {
    let (lines, made) = run(
        "async function reading() { await 0; const total = arguments.length + arguments[0]; await 0; return total; }
         reading(4, 5).then((total) => print(String(total)));",
    );
    assert_eq!(lines, ["6"]);
    assert_eq!(made, 0, "a resume made a frame");
}

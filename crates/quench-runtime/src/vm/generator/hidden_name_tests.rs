//! A suspending function that reads implicit bindings (`super`, `new.target`) by hidden name
//! resumes in its window while it makes no closure that would capture them there: its record
//! keeps the bindings, and the opcode that reads one exits the lane, which makes the frame from
//! the record (`Vm::push_generator_window_frame`). Compiled runs agree with the interpreter's.
use super::window_steps::tests::agree;

#[test]
fn suspending_methods_reading_implicit_bindings_agree_with_the_interpreter() {
    let lines = agree(
        r#"
        class A { value(n) { return "A" + n; } }
        class B extends A {
            *gen() { yield super.value(1); yield new.target === undefined; yield super.value(2); }
            async run() { await 0; const first = super.value(3); await 0; return first + super.value(4); }
        }
        const out = [...new B().gen()];
        new B().run().then((value) => { out.push(value); print(out.join(",")); });
        "#,
    );
    assert_eq!(lines, ["A1,true,A2,A3A4"]);
}

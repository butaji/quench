//! A suspending function that reads implicit bindings (`super`, `new.target`) by hidden name
//! resumes in its window: its record keeps the bindings, which the environment its window makes
//! for closures takes over (`Vm::promote_record_environment`), and the opcode that reads one exits
//! the lane, which makes the frame from the record (`Vm::push_generator_window_frame`). Compiled
//! runs agree with the interpreter's.
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

#[test]
fn closures_and_iteration_copies_in_a_method_reading_implicit_bindings_keep_them() {
    // The arrow and the per-iteration copies of the loop's environment are made in the window,
    // across yields, and still read the bindings the record held.
    let lines = agree(
        r#"
        class A { value(n) { return "A" + n; } }
        class B extends A {
            *gen() {
                const fns = [];
                for (let i = 0; i < 3; i++) { fns.push(() => super.value(i) + typeof new.target); yield i; }
                yield fns.map((f) => f()).join("|");
            }
        }
        print([...new B().gen()].join(","));
        "#,
    );
    assert_eq!(lines, ["0,1,2,A0undefined|A1undefined|A2undefined"]);
}

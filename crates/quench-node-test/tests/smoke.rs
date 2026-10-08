use quench_node::shared_run::{execute_shared, SharedCompletion, SharedInput};

fn run(source: &str) -> Result<SharedCompletion, String> {
    execute_shared(
        SharedInput::Eval(source.to_owned()),
        vec!["quench-node".into()],
    )
}

#[test]
fn shared_node_host_evaluates_and_requires_builtins() {
    let result = run("if (typeof require('node:fs').readFileSync !== 'function') throw new Error('fs missing'); console.log('hello');");
    assert_eq!(result, Ok(SharedCompletion::Completed));
}

#[test]
fn generator_try_yield_inside_loop_resumes_and_catches() {
    let source = r#"
function* values() {
  for (let i = 0; i < 18; i++) {
    try {
      if ((i + 6) % 13 === 0) throw i;
      yield i;
    } catch (error) {
      yield error & 7;
    }
  }
}
let total = 0;
for (const value of values()) total += value;
if (total !== 153) throw new Error("generator loop completion");
"#;
    assert_eq!(run(source), Ok(SharedCompletion::Completed));
}

#[test]
fn generator_injected_throw_continues_loop_after_catch() {
    let source = r#"
function* values() {
  for (let i = 0; i < 3; i++) {
    try { yield i; }
    catch (error) { if (error !== 9) throw error; }
  }
}
const iterator = values();
if (iterator.next().value !== 0) throw new Error("first yield");
const thrown = iterator.throw(9);
if (thrown.value !== 1 || thrown.done) throw new Error("catch must continue at next yield");
if (iterator.next().value !== 2) throw new Error("third yield");
if (!iterator.next().done) throw new Error("loop must finish");
"#;
    assert_eq!(run(source), Ok(SharedCompletion::Completed));
}

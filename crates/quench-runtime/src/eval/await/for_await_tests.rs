use crate::{Context, Value};
use std::sync::mpsc;
use std::time::Duration;

#[test]
fn for_await_of_interleaves_generator_and_promise_reactions() {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut context = Context::new()?;
            context.eval(SOURCE)?;
            context.eval("actual.join('|')")
        })()
        .and_then(|context_result| match context_result {
            Value::String(value) => Ok(value),
            value => Err(crate::JsError::from(format!(
                "unexpected result: {value:?}"
            ))),
        });
        let _ = sender.send(result);
    });
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(5)),
        Ok(Ok(EXPECTED.into()))
    );
}

#[test]
fn for_await_of_async_generator_completes() {
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let result = (|| {
            let mut context = Context::new()?;
            context.eval("var actual = []; var finished = false; async function* values() { var current = 3; while (current > 0) yield Promise.resolve(current--); } async function run() { for await (var value of values()) actual.push(value); actual.push('post'); } run().then(function() { finished = true; });")?;
            context.eval("actual.join(',') + '|' + finished")
        })()
        .and_then(|value| match value {
            Value::String(value) => Ok(value),
            value => Err(crate::JsError::from(format!("unexpected result: {value:?}"))),
        });
        let _ = sender.send(result);
    });
    assert_eq!(
        receiver.recv_timeout(Duration::from_secs(5)),
        Ok(Ok("3,2,1,post|true".into()))
    );
}

const EXPECTED: &str = "Promise: 6|Promise: 5|Await: 3|Promise: 4|Promise: 3|Await: 2|Promise: 2|Promise: 1|Await: 1|Promise: 0";

const SOURCE: &str = r#"
var actual = [];
async function* values() {
  let current = 3;
  while (current > 0) yield Promise.resolve(current--);
}
async function run() { for await (const value of values()) actual.push('Await: ' + value); }
function count(n) {
  actual.push('Promise: ' + n);
  return n ? Promise.resolve(n - 1).then(count) : actual.join('|') ===
    'Promise: 6|Promise: 5|Await: 3|Promise: 4|Promise: 3|Await: 2|Promise: 2|Promise: 1|Await: 1|Promise: 0';
}
run();
count(6);
"#;

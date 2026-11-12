//! Regressions for compile-time scope facts (class environment clones, long chains) and the
//! direct-eval cache's caller identity.

use super::subset_tests::output;
use crate::Engine;

#[test]
fn long_left_nested_chains_compile_and_run() {
    // Chains of thousands of terms (as generated code has) stay within the transition budget
    // and walk once, not once per level.
    let source = r#"
let sum = "1", text = '"x"', mixed = '"p"';
for (let i = 0; i < 3000; i++) { sum += "+" + i; text += '+"y"'; mixed += "+v"; }
const v = 1;
print([eval(sum), eval(text).length, eval(mixed).length].join());
"#;
    assert_eq!(output(source), ["4498501,3001,3001"]);
}

#[test]
fn class_clones_environment_only_inside_loops() {
    // A class evaluated once per activation shares the scope; one in a loop gets a copy per
    // evaluation so its methods keep that iteration's bindings.
    let source = r#"
class Once { static v = 1; m() { return Once.v; } }
const kept = [];
for (let i = 0; i < 3; i++) { class K { static v = i; get() { return K; } } kept.push(K); }
print([new Once().m(), kept.map((k) => k.v + ":" + (new k().get() === k)).join()].join("|"));
"#;
    let program = Engine::specialize(source, "class-clone.js").unwrap();
    program.compile_released();
    let clones = program.functions[0].instructions().filter(|instruction| instruction.op() == crate::bytecode::Op::CloneEnv).count();
    assert!(clones >= 1);
    program.validate().unwrap();
    assert_eq!(output(source), ["1|0:true,1:true,2:true"]);
}

#[test]
fn direct_eval_cache_keeps_each_callers_origin_and_variables() {
    // One text evaluated from two callers shares no caller identity: each sees its own
    // variables and its stack names its own eval origin.
    let source = r#"
const text = "x + new Error('e').stack.length * 0";
function first() { let x = 1; return eval(text); }
function second() { let x = 10; return eval(text); }
const origin = (f) => { const m = eval("new Error('e').stack"); return m; };
function e1() { return eval("new Error('e').stack"); }
function e2() { return eval("new Error('e').stack"); }
const at = (stack) => /eval at (\w+)/.exec(stack)[1];
print([first(), second(), first(), at(e1()), at(e2()), at(e1())].join());
"#;
    assert_eq!(output(source), ["1,10,1,e1,e2,e1"]);
}

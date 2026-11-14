//! Program-visible behaviour of eager images, ported from the retired static-region and
//! general-dispatch tests. Each program runs interpreted, compiled with lane bodies, and as an
//! eager image in native-only mode (every function entered through its trampoline over a lane
//! body, leaving to the per-PC fallback where the body does not lower), and the three must agree
//! on how it ended and what it printed.
use super::*;
use crate::{test_host::TestHost, Engine, ExecutionMode, ResidualProgram};
use std::{sync::mpsc, thread, time::Duration};

const THREW_42: &str = "threw 42";
const RETURNED: &str = "returned";
/// How long a worker may take to reach or leave native code before the test gives up.
const WAIT: Duration = Duration::from_secs(10);
/// Time a spinning guest gets in native code before termination is requested.
const SPIN: Duration = Duration::from_millis(20);

/// How a guest run ended and the lines it printed.
#[derive(Debug, PartialEq)]
struct Shown {
    ended: String,
    lines: Vec<String>,
}

fn ended(result: Result<(), crate::JsError>) -> String {
    let Err(error) = result else { return RETURNED.into() };
    match error.thrown_value().and_then(|value| value.as_number()) {
        Some(number) => format!("threw {number}"),
        None => error.to_string(),
    }
}

fn compile_image(program: &ResidualProgram) -> crate::NativeJsProgram<TestHost> {
    let options = crate::NativeJsCodegenOptions { backend: crate::NativeJsBackend::Cranelift, ..Default::default() };
    crate::NativeJsProgram::compile_eager_with_options(program.clone(), options).unwrap()
}

/// Runs `program` in `mode`, as an eager image where `image` is set.
fn show(program: &ResidualProgram, mode: ExecutionMode, image: bool) -> Shown {
    let mut runtime = Runtime::new(TestHost::default().with_test262());
    runtime.set_execution_mode(mode);
    runtime.vm.enable_lane_methods();
    let result = if image { runtime.execute_native(&compile_image(program)) } else { runtime.execute(program) };
    Shown { ended: ended(result), lines: runtime.host_mut().lines().clone() }
}

/// Checks the interpreter's run of `program` ends as `ends`, and that compiled and image runs
/// show the same.
fn assert_agrees_on(program: &ResidualProgram, name: &str, ends: &str) {
    let interpreted = show(program, ExecutionMode::Interpreted, false);
    assert_eq!(interpreted.ended, ends, "{name}: {interpreted:?}");
    assert_eq!(show(program, ExecutionMode::Compiled, false), interpreted, "{name}: compiled");
    assert_eq!(show(program, ExecutionMode::NativeOnly, true), interpreted, "{name}: image");
}

/// `assert_agrees_on` for `source` compiled as a CommonJS module and as a TypeScript one.
fn assert_agrees(source: &str, name: &str, ends: &str) {
    let programs = [
        Engine::specialize(source, &format!("{name}.cjs")),
        Engine::specialize_typescript(source, &format!("{name}.cts")),
    ];
    for program in programs {
        assert_agrees_on(&program.unwrap(), name, ends);
    }
}

macro_rules! fixture_tests {
    ($($test:ident: $file:literal => $ends:expr),+ $(,)?) => {$(
        #[test]
        fn $test() {
            let source = include_str!(concat!("../../tests/fixtures/ahead/", $file, ".js"));
            assert_agrees(source, $file, $ends);
        }
    )+};
}

fixture_tests! {
    packed_reference_stores_keep_identity: "ref-identity" => THREW_42,
    packed_reference_stores_keep_collection_and_effects: "ref-effects" => THREW_42,
    countdown_dependent_loops_keep_semantic_bounds: "countdown" => THREW_42,
    inlined_receiver_loops_keep_their_countdown: "inline-receiver" => THREW_42,
    loop_rotation_keeps_trip_counts_and_header_recovery: "loop-rotation" => THREW_42,
    word32_pairs_keep_overflow_rounding_and_live_numbers: "word32" => THREW_42,
    invariant_capture_reads_keep_mutation_and_coercion: "capture-invariants" => RETURNED,
    independent_loops_keep_effect_boundaries_and_local_binding: "separate-loops" => THREW_42,
    forwarded_fields_keep_effects_aliases_and_values: "field-values" => THREW_42,
    loop_field_phis_keep_aliases_widening_and_effects: "loop-fields" => THREW_42,
    captured_call_targets_keep_replacement_and_recovery: "captured-target" => THREW_42,
    guarded_captured_updates_keep_packed_fields: "packed-captured" => THREW_42,
    holder_reads_keep_prototype_identity_and_effects: "holder-address" => THREW_42,
    nullish_latches_keep_nonobjects_effects_and_changed_links: "family-latch" => THREW_42,
    bound_data_fields_run_field_arithmetic: "bound-fields" => THREW_42,
}

#[test]
fn ahead_bodies_read_dense_captures_across_mutation_and_collection() {
    let source = "function make(seed) {\n\
          let total = seed;\n\
          function bump(delta) { total = total + delta; }\n\
          function read(n) { let sum = 0; for (let i = 0; i < n; i++) sum = (sum + total) | 0; return sum; }\n\
          return { bump, read };\n\
        }\n\
        const counter = make(1);\n\
        let seen = '';\n\
        for (let round = 0; round < 8; round++) { counter.bump(round); $262.gc(); seen += counter.read(3) + ','; }\n\
        print(seen);";
    assert_agrees(source, "dense-captures", RETURNED);
}

#[test]
fn packed_field_ranges_keep_boundaries_aliases_and_reentry() {
    let source = concat!(include_str!("../../tests/fixtures/packed-field-ranges.js"), "\nthrow 42;");
    assert_agrees(source, "packed-field-ranges", THREW_42);
}

#[test]
fn loop_slot_proofs_expire_before_storage_migration() {
    let source = concat!(include_str!("../../tests/fixtures/loop-slot-lifetime.js"), "\nthrow 42;");
    assert_agrees(source, "loop-slot-lifetime", THREW_42);
}

#[test]
fn conditional_integer_updates_keep_their_countdown() {
    for body in [
        "n|=0;x|=0;for(let i=0;i<n;i=(i+1)|0){if((x&1)===0)x=x>>1;else x=(x>>1)^feedback;}return x|0;",
        "const state={count:n|0,control:x|0};while(state.count>0){state.count=(state.count-1)|0;if((state.control&1)===0)state.control=state.control>>1;else state.control=(state.control>>1)^feedback;}return state.control|0;",
    ] {
        let source = format!(
            "const feedback=7;function fold(n,x){{{body}}}\n\
             if(fold(0,-1)!==-1||fold(-1,17)!==17||fold(1,-1)!==-8||fold(3,3)!==6)throw 'integer fold';\nthrow 42;"
        );
        assert_agrees(&source, "conditional-countdown", THREW_42);
    }
}

#[test]
fn integer_loop_results_survive_each_result_conversion() {
    let source = "const feedback = 0xD008;\n\
        function state(n, seed) { n |= 0; let x = seed | 0;\n\
            for (let i = 0; i < n; i = (i + 1) | 0) { if ((x & 1) === 0) x = x >> 1; else x = (x >> 1) ^ feedback; }\n\
            return x | 0; }\n\
        for (let warm = 0; warm < 64; warm++) state(1, warm);\n\
        print(state(0, 123456789)); print(state(17, 123456789)); print(state(-1, -2147483648)); print(state(1, -1));";
    for result in ["x | 0", "x >>> 0", "x + 0.5", "x < 0"] {
        let body = source.replace("return x | 0;", &format!("return {result};"));
        for source in [body.clone(), format!("(function () {{ {body} }})();")] {
            assert_agrees(&source, "integer-loop-result", RETURNED);
        }
    }
}

#[test]
fn packed_integer_fields_keep_their_update_across_run_lengths() {
    const WARMUP_RUNS: usize = 64;
    for count in [1000, 10000] {
        let expected = (0..count).fold(123456789_i32, |value, _| (value >> 1) ^ if value & 1 != 0 { 0xD008 } else { 0 });
        let source = format!(
            "(function() {{ function advance(state) {{ state.count = (state.count - 1) | 0;\n\
                state.control = (state.control >> 1) ^ ((state.control & 1) ? 0xD008 : 0); }}\n\
              function run(n, seed) {{ const state = {{count: n | 0, control: seed | 0}};\n\
                while(state.count > 0) advance(state); return state.control | 0; }}\n\
              for(let i=0;i<{WARMUP_RUNS};i++) run(1,i);\n\
              if(run({count},123456789)!=={expected}) throw 'incorrect update'; throw 42; }})();"
        );
        assert_agrees(&source, "packed-fields", THREW_42);
    }
}

#[test]
fn packed_field_stores_keep_widening_and_exact_effects() {
    let half_range = 1_i64 << (crate::value::NarrowLayout::INTEGER_BITS - 1);
    let source = format!(
        "function step(state, delta) {{ state.count = (state.count + 1) | 0; state.value = state.value + delta; return state.value; }}\n\
         let s = {{count: 0, value: 0}};\n\
         for(let i=0;i<64;i++) step(s,1);\n\
         for(const start of [{half_range}-1, -{half_range}, 0.5, -0, 'x']) for(const delta of [-1,1]) {{\n\
           s = {{count: 0, value: start}};\n\
           if(step(s,delta)!==start+delta || s.count!==1) throw 'widening';\n\
           if(step(s,-delta)!==(start+delta)+(-delta) || s.count!==2) throw 'second write'; }}\n\
         let calls=0, marker={{}};\n\
         s={{count:0,value:{{valueOf(){{calls++;throw marker;}}}}}};\n\
         try {{step(s,1);throw 'missing throw';}} catch(e) {{if(e!==marker)throw e;}}\n\
         if(calls!==1 || s.count!==1) throw 'replayed prefix';\n\
         s={{count:0,value:7}}; step(s,0);\n\
         Object.defineProperty(s,'value',{{writable:false}});\n\
         if(step(s,2)!==7 || s.count!==2) throw 'sloppy readonly';\n\
         function strictStep(state) {{ 'use strict'; state.count = (state.count + 1) | 0; state.value = state.value + 1; }}\n\
         s={{count:0,value:0}}; for(let i=0;i<64;i++) strictStep(s);\n\
         Object.defineProperty(s,'value',{{writable:false}});\n\
         try {{strictStep(s);throw 'missing readonly throw';}} catch(e) {{if(!(e instanceof TypeError))throw e;}}\n\
         if(s.count!==65 || s.value!==64) throw 'strict readonly prefix';\nthrow 42;"
    );
    assert_agrees(&source, "packed-transitions", THREW_42);
}

/// The `nonleaf` fixture with the call terminal or not.
#[test]
fn nonleaf_inlined_calls_recover_without_replaying_effects() {
    let source = include_str!("../../tests/fixtures/ahead/nonleaf.js");
    let terminal = source
        .replace("let result=callback(input);\n    return result+this.count;", "return callback(input);")
        .replace("!==66", "!==45");
    for source in [source.to_owned(), terminal] {
        assert_agrees(&source, "nonleaf", THREW_42);
    }
}

/// The `exact-gc` fixture with the call terminal and through a bridge frame or not.
#[test]
fn materialized_inline_parameters_survive_gc_reflection_and_throw() {
    let source = include_str!("../../tests/fixtures/ahead/exact-gc.js");
    for (terminal, nested) in [(false, false), (false, true), (true, false), (true, true)] {
        let mut source = source.to_owned();
        if terminal {
            source = source.replace("let result=owner.consume(input);return result+3;", "return owner.consume(input);")
                .replace("!==66", "!==63");
        }
        if nested {
            source = source.replace("owner.consume(input)", "owner.bridge(input)")
                .replace("let runner=", "p.bridge=function bridge(input){this.bridgeCount=this.bridgeCount+1;let result=this.consume(input);return result+5;};let runner=")
                .replace("owner.count=0;", "owner.count=0;owner.bridgeCount=0;")
                .replace("p.consume.caller!==runner.run", "p.consume.caller!==p.bridge||p.bridge.caller!==runner.run")
                .replace("!==66", "!==71").replace("!==63", "!==68")
                .replace("if(owner.count!==22)", "if(owner.count!==22||owner.bridgeCount!==22)");
        }
        assert_agrees(&source, "exact-gc", THREW_42);
    }
}

#[test]
fn a_call_budget_read_once_per_unit_leaves_call_results_exact() {
    let walk = "function Node(next){this.state=0;this.next=next;}\
        Node.prototype.mark=function(){this.state=this.state|2;};\
        function walk(node,n){while(n>0){node.mark();node=node.next;n=(n-1)|0;}return node.state;}\
        let a=new Node(null);let b=new Node(a);a.next=b;for(let i=0;i<20;i++)walk(a,2);throw walk(a,100);";
    let straight = walk.replace("while(n>0){node.mark();node=node.next;n=(n-1)|0;}", "node.mark();node=node.next;node.mark();");
    for source in [walk.to_owned(), straight] {
        assert_agrees(&source, "call-budget", "threw 2");
    }
}

#[test]
fn native_stores_that_widen_keep_prefix_and_suffix_effects() {
    let source = "var before=0,after=0; var receiver={value:0,a:1,b:2};\
        function write(x){before++;receiver.value=x;after++;}\
        for(let i=0;i<200;i++)write(i); write(0.5);\
        print(before + \" \" + after + \" \" + receiver.value);";
    assert_agrees(source, "untyped-native-write", RETURNED);
    let program = Engine::specialize(source, "untyped-native-write").unwrap();
    assert_eq!(show(&program, ExecutionMode::NativeOnly, true).lines, ["201 201 0.5"]);
}

#[test]
fn nested_recovery_admits_the_call_budget_before_prefix_effects() {
    for strict_tail in [false, true] {
        let middle = if strict_tail {
            "'use strict';this.outer=this.outer+1;return this.leaf(input);"
        } else {
            "this.outer=this.outer+1;let result=this.leaf(input);return result+1;"
        };
        let source = format!(
            "let p={{outer:0,inner:0}};\n\
             p.leaf=function leaf(input){{this.inner=this.inner+1;return input.value;}};\n\
             p.middle=function middle(input){{{middle}}};\n\
             function run(owner,input){{let result=owner.middle(input);return result+1;}}\n\
             let input={{get value(){{$262.gc();return 40;}}}};\n\
             try{{print(run(p,input));}}catch(error){{print(error.name);}}\nprint(p.outer,p.inner);"
        );
        let program = Engine::specialize(&source, "nested-budget.js").unwrap();
        for limit in 1..=6 {
            let _limit = crate::stack::GuestCallLimitGuard::enter(limit);
            let before = crate::stack::GuestCallGuard::remaining();
            assert_agrees_on(&program, &format!("limit={limit} strict_tail={strict_tail}"), RETURNED);
            assert_eq!(crate::stack::GuestCallGuard::remaining(), before, "recovered guards must unwind");
        }
    }
}

#[test]
fn bounded_entries_split_stop_control_and_keep_generator_parameters() {
    let sources = [
        ("let trace=''; function* values(x=(trace+='p',1)) { trace+='b'; yield x; trace+='r'; return 2; } const g=values(); if(trace!=='p') throw 'early body'; const first=g.next(); const last=g.next(); if(first.value!==1||first.done||last.value!==2||!last.done) throw 'bad yield'; throw trace;", "pbr"),
        ("let trace=''; function fail(){trace+='p';throw 7;} function* values(x=fail()){trace+='bad';yield x;} try{values();throw 'missed parameter error';}catch(e){if(e!==7)throw e;} throw trace;", "p"),
    ];
    for (source, trace) in sources {
        let program = Engine::specialize(source, "bounded.js").unwrap();
        let interpreted = show(&program, ExecutionMode::Interpreted, false);
        assert!(interpreted.ended.contains(trace), "{interpreted:?}");
        assert_eq!(show(&program, ExecutionMode::NativeOnly, true), interpreted);
    }
}

fn started(
    context: &mut crate::NativeContext<'_, TestHost>, _: crate::RootId, _: &[crate::RootId],
) -> Result<crate::RootId, crate::RootedError> {
    context.host_mut().state::<mpsc::Sender<()>>().send(()).unwrap();
    Ok(context.undefined())
}

/// Runs `source` as an image on a worker, requests termination once it signals `started()`,
/// and checks the run ends terminated with its call budget restored.
fn assert_terminates(source: &'static str, name: &'static str) {
    let (ready, entered) = mpsc::channel::<()>();
    let (handles, handle) = mpsc::channel();
    let worker = thread::Builder::new().stack_size(crate::WORKER_STACK_SIZE).spawn(move || {
        let native = compile_image(&Engine::specialize(source, name).unwrap());
        let host = TestHost::default()
            .with_natives(crate::host_functions![global "started" (0) => started])
            .with_state(std::rc::Rc::new(ready));
        let mut runtime = Runtime::new(host);
        runtime.set_execution_mode(ExecutionMode::NativeOnly);
        handles.send(runtime.execution_interrupt_handle()).unwrap();
        let budget = crate::stack::GuestCallGuard::remaining();
        let error = runtime.execute_native(&native).unwrap_err();
        assert!(error.is_execution_terminated(), "{error}");
        assert_eq!(crate::stack::GuestCallGuard::remaining(), budget);
    }).unwrap();
    let interrupt = handle.recv_timeout(WAIT).unwrap();
    entered.recv_timeout(WAIT).unwrap();
    thread::sleep(SPIN);
    interrupt.request_termination();
    worker.join().unwrap();
}

#[test]
fn rotated_loops_observe_termination_requested_after_entry() {
    assert_terminates(
        "function spin(){started();let x=1;for(let i=0;i<1/0;i=(i+1)|0){x=x^(x<<13);}return x;}spin();",
        "rotated-interrupt-direct.js",
    );
    assert_terminates(
        "function run(){function spin(x){for(let i=0;i<1/0;i=(i+1)|0)x=x^(x<<13);return x;}started();for(let n=0;n<2;n++)spin(n);}run();",
        "rotated-interrupt-nested.js",
    );
}

#[test]
fn a_loop_private_until_warmup_ends_still_observes_termination() {
    assert_terminates(
        "function spin(n,x){n=n|0;x=x|0;for(let i=0;i<n;i=(i+1)|0){x=x^(x<<13);x=x^(x>>>17);x=x^(x<<5);}return x|0;}\
         let result=0;for(let warm=0;warm<64;warm++)result^=spin(2000,warm);started();result=spin(2147483647,result);",
        "late-interrupt-owner.js",
    );
}

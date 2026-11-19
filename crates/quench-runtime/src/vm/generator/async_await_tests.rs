//! An async generator awaiting inside its body parks its frame in its record, so the await's
//! reaction holds the generator and no continuation of its own; an async activation a promise
//! job resumes runs in the lane over its frame's register file (`Vm::run_lane_job`), a rejection
//! raising at its await there; a request of an async generator and a run the lane does not admit
//! resume through the owner of the frame all the same (`Vm::run_job`).
use super::window_steps::tests::{agree, run};
use crate::test_host::TestHost;
use crate::vm::Vm;
use crate::Engine;

#[test]
fn an_async_generators_body_await_parks_its_frame_in_the_record() {
    let mut vm = Vm::new(TestHost::default());
    let program = Engine::specialize(
        "var gate = Promise.withResolvers();
         async function* body() { await gate.promise; yield 1; }
         var iterator = body();
         iterator.next();",
        "async-generator-await.js",
    )
    .unwrap();
    vm.execute(&program).unwrap();
    let name = vm.intern_atom("iterator");
    let generator = vm.own_property(vm.realm.globals, name).unwrap();
    let record = vm.generator_record_mut(generator).expect("an async generator's record");
    assert!(record.is_executing());
    assert!(record.continuation().is_some(), "the awaiting frame is parked in the record");
    let mut held = 0;
    vm.heap.for_each_await_state(|state| held += usize::from(state.holds_continuation()));
    assert_eq!(held, 0, "the reaction holds the generator, not a continuation");
}

#[test]
fn an_async_function_awaiting_in_a_loop_resumes_in_the_lane_without_frames() {
    // Each await after the first suspends the activation back into its await state and each
    // reaction resumes it over the same register file: no `Box<Frame>` is made, whatever the count.
    let source = |steps: u32| {
        format!(
            "async function total(n) {{ let sum = 0; for (let i = 0; i < n; i++) sum += await i; return sum; }}
             total({steps}).then((sum) => print(String(sum)));"
        )
    };
    let (few_output, few) = run(&source(3));
    let (many_output, many) = run(&source(500));
    assert_eq!(few_output, ["3"]);
    assert_eq!(many_output, ["124750"]);
    assert_eq!(few, 0, "a resume made a frame");
    assert_eq!(many, 0, "a resume made a frame");
}

#[test]
fn async_functions_resumed_in_the_lane_reject_and_unwind_through_try_finally() {
    // A rejection raises at the await in the lane, which a handler of the function covering it
    // catches, as a throw in its body does: the handlers are those of its bytecode either way.
    let (lines, _) = run(
        r#"
        const out = [];
        async function guarded(promise) {
            try { await promise; } catch (e) { out.push("caught " + e); } finally { out.push("finally"); }
            return await "done";
        }
        async function throws() {
            await 1;
            try { await 2; null.x; } catch (e) { out.push(e.constructor.name); } finally { out.push("cleanup"); }
        }
        async function fails() { await 1; throw new Error("fail"); }
        async function main() {
            out.push(await guarded(Promise.reject("boom")));
            await throws();
            try { await fails(); } catch (e) { out.push(e.message); }
            print(out.join(" "));
        }
        main();
        "#,
    );
    assert_eq!(lines, ["caught boom finally done TypeError cleanup fail"]);
}

#[test]
fn an_async_generator_awaits_in_the_lane_and_serves_for_await() {
    let (lines, _) = run(
        r#"
        async function* count(n) { for (let i = 0; i < n; i++) { await null; yield i; } }
        async function main() {
            let sum = 0;
            for await (const x of count(100)) sum += x;
            print(String(sum));
        }
        main();
        "#,
    );
    assert_eq!(lines, ["4950"]);
}

#[test]
fn rejected_awaits_resume_through_the_owner_of_the_frame_and_agree_with_the_interpreter() {
    let lines = agree(
        r#"
        const out = [];
        async function guarded(promise) {
            try { await promise; } catch (e) { out.push("caught " + e); }
            try { await Promise.reject("again"); } finally { out.push("finally"); }
        }
        async function* yielding() {
            try { yield Promise.reject("bad"); } catch (e) { out.push("yield " + e); yield "after"; }
        }
        guarded(Promise.reject("boom")).catch((e) => out.push("rejected " + e));
        yielding().next().then((result) => print(out.join(",") + " " + JSON.stringify(result)));
        "#,
    );
    assert_eq!(lines, [r#"caught boom,yield bad,finally,rejected again {"value":"after","done":false}"#]);
}

#[test]
fn async_generator_requests_resume_through_the_record_and_agree_with_the_interpreter() {
    let lines = agree(
        r#"
        const out = [];
        async function* gen() {
            try { const a = yield 1; await null; const b = yield a + 1; return b; }
            finally { out.push("closed"); }
        }
        const it = gen();
        Promise.all([it.next(), it.next(10), it.next(20), it.next()]).then((results) => {
            print(out.join(",") + " " + results.map((result) => JSON.stringify(result)).join(" "));
        });
        "#,
    );
    assert_eq!(lines, [r#"closed {"value":1,"done":false} {"value":11,"done":false} {"value":20,"done":true} {"done":true}"#]);
}

#[test]
fn a_rejection_no_handler_covers_settles_the_async_job_without_a_frame() {
    // The raise at the await ends the activation in its owner (`JobEnd::Threw`): the promise
    // rejects and no frame is made for it.
    let (lines, made) = run(
        r#"
        async function fails(promise) { await 1; await promise; return "unreachable"; }
        fails(Promise.reject("boom")).catch((e) => print("rejected " + e));
        "#,
    );
    assert_eq!(lines, ["rejected boom"]);
    assert_eq!(made, 0, "the rejection made a frame");
}

#[test]
fn a_throw_no_handler_covers_ends_a_generator_step_without_a_frame() {
    let (lines, made) = run(
        r#"
        function* g() { yield 1; yield 2; }
        const it = g();
        it.next();
        try { it.throw("stop"); } catch (e) { print("raised " + e + " " + JSON.stringify(it.next())); }
        "#,
    );
    assert_eq!(lines, [r#"raised stop {"done":true}"#]);
    assert_eq!(made, 0, "the throw made a frame");
}

#[test]
fn an_await_of_a_settled_value_continues_in_place_only_where_no_other_job_waits() {
    // A resumed async function awaiting a settled value whose reaction would be the only queued
    // job continues without suspending (`await_miss`); with other jobs queued, or on its first
    // run from a caller, it suspends: the order matches Node's.
    let lines = agree(
        r#"
        const log = [];
        async function a() { for (let i = 0; i < 3; i++) { log.push("a" + i); await i; } }
        async function b() { for (let i = 0; i < 3; i++) { log.push("b" + i); await Promise.resolve(i); } }
        async function c() { await null; log.push("c1"); const x = await (async () => 5)(); log.push("c" + x); }
        async function solo() { let s = 0; for (let i = 0; i < 5; i++) s += await i; log.push("solo" + s); }
        a(); b(); c(); Promise.resolve().then(() => log.push("then")); log.push("sync");
        solo().then(() => Promise.resolve().then(() => print(log.join(" "))));
        "#,
    );
    assert_eq!(lines, ["a0 b0 sync a1 b1 c1 then a2 b2 c5 solo10"]);
}

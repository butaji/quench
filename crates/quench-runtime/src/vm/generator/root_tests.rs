//! An async module's body, a program's root, resumes after its top-level `await` in the lane over
//! its frame's register file as an async function does (`Vm::run_lane_job`), reading its imports and
//! globals through the activation (`Vm::root_binding`, `root_windows`).
use super::window_steps::tests::module_run;

#[test]
fn a_module_with_top_level_await_resumes_in_the_lane_without_frames() {
    let (lines, made) = module_run(
        r#"
        let total = 0;
        for (let i = 0; i < 200; i++) total += await i;
        print(String(total));
        "#,
    );
    assert_eq!(lines, ["19900"]);
    assert_eq!(made, 0, "a resume made a frame");
}

#[test]
fn a_module_root_reads_globals_and_stores_its_own_bindings_after_an_await() {
    let (lines, _) = module_run(
        r#"
        globalThis.seed = 5;
        const base = await seed;
        let sum = base;
        for (let i = 0; i < 3; i++) sum += await (i + seed);
        print(typeof base + ":" + sum);
        "#,
    );
    assert_eq!(lines, ["number:23"]);
}

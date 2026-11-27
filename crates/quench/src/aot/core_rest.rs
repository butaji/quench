//! The core image's rest (C5 D1): the realm the image starts from, saved at build time, and the
//! build-time probe that decides what the build saves (C5 D1b).
//!
//! The worker runs the entry's residual program on the closed world the image links: the builtin
//! table and the engine entries of the same `ReachableRealm` and `ExecutionTableSpec` the catalog
//! text is generated from (`compiler::write_builtin_table`, `compiler::write_execution_table`), so
//! the realm it installs holds exactly the natives and families the image's `BUILTINS` names.
//!
//! What a core build keeps is one of two things, decided by `probe` before anything is saved:
//!
//! * **Fold**: the entry reaches its end with no dynamic input (`prefix`, the same gate the Node
//!   host runs its entries under). Nothing about the program is observable at run time, so the
//!   image is the strict replay image (`strict::fold_image`): no realm, no program, no top level.
//! * **Realm and program** (`build_rest`): the state after the realm is installed and the program
//!   is loaded, before any of its top-level code runs. The image restores it (`image_driver_core`)
//!   and runs the top level from there. Every program with an input lands here, as does one that
//!   fails with no input.
//!
//! # Where a save point can be (the input handler and what it can capture)
//!
//! A dynamic input is a runtime input the build may not make: a clock read (`Date`, `Date.now`,
//! `performance`), the local time zone, `Math.random`, or a host output (`console`, `print`).
//! The runtime notes each one before it happens (`clock::note_input`, `HostContext::invoke`), and
//! the gate's handler ends the watched run there (`clock::watch_inputs`). The handler resumes
//! nothing: a run that goes on past an input would keep effects no build may keep.
//!
//! The handler could save the run where it stopped only if the live state there is a continuation
//! the VM can materialize. Per frame kind:
//!
//! * **Capturable**: a generator, an async function or an async request suspended at a `yield` or
//!   an `await`. Its state is one `activation::Continuation` (a `FrameRecord` with its completion,
//!   resume register and promise), which the heap writes like any other record.
//! * **Not capturable**: an ordinary call frame. Its activation keeps its scope on the lane stack
//!   and has no record (`vm/activation/frame_record.rs`). A native call's own Rust frame, such as
//!   the `Date` constructor that reads the clock, has no continuation at all. A call chain of more
//!   than one frame has no multi-frame continuation: a `Continuation` holds one frame.
//!
//! So an input reached through any call of the program is not capturable, and its save point is
//! the realm and program. The richards suite (`quench-bench/js-engine-benchmark/v8-v7`) reads its
//! first input at `base.js` `new Date()` (line 204, `Measure`): the top level calls `RunSuites`,
//! which calls `RunStep`, `RunNextBenchmark` and `RunSingleBenchmark` before it. None of those
//! frames is capturable, so richards saves the realm and program. A capture at that point needs a
//! multi-frame continuation and a native call site that parks its caller (the VM's
//! `dispatch_frame` change), which this build does not have.

use super::prefix::{self, Residual};
use quench_layers::engine::ExecutionTableSpec;
use quench_runtime::tier_surface::Native;
use quench_runtime::{BuiltinFamilies, BuiltinTable, ExecutionTable, ReachableRealm, ResidualProgram, Runtime, SystemHost};
use std::{fs, path::Path};

/// What the build-time probe of a core entry found (`probe`).
pub(super) enum Reach {
    /// The entry ends with no input and nothing pending: its image replays no effect.
    Folds,
    /// The entry reads this input before it ends, so the build saves the realm and program.
    Input(String),
    /// The entry fails or leaves a top-level await pending with no input read. The build saves the
    /// realm and program, and the image runs into the same failure.
    Unfinished(String),
}

/// Runs the core entry at build time on the closed world, up to its first input or its end, and
/// reports what it reached (`Reach`). The probe's output is discarded (`prefix::Probe`); a fold
/// makes no output.
pub(super) fn probe(program: &Path, realm: &ReachableRealm, execution: &ExecutionTableSpec) -> Result<Reach, String> {
    let program = ResidualProgram::read_binary(program)?;
    let (builtins, engine) = closed_tables(realm, execution);
    let mut runtime = Runtime::new_static_with_builtins(prefix::Probe, builtins, engine);
    // Watch from before the first instruction: the gate notes every input the run reads.
    quench_runtime::clock::watch_inputs(runtime.execution_interrupt_handle());
    let run = runtime.execute_deferred_jobs(&program).and_then(|()| runtime.run_host_jobs(&program));
    let pending = match &run {
        Ok(()) => runtime.module_evaluation_pending(&program).map_err(|error| error.to_string())?,
        Err(_) => false,
    };
    Ok(match prefix::finish() {
        // An input the run read first decides the save point, whatever the run went on to do.
        Residual::Dynamic(input) => Reach::Input(input),
        Residual::Question(question) => Reach::Input(format!("a question about {}", question.key())),
        Residual::Effects(_) | Residual::Waiting(..) => match run {
            Err(error) => Reach::Unfinished(error.to_string()),
            Ok(()) if pending => Reach::Unfinished("a top-level await that never settles".into()),
            Ok(()) => Reach::Folds,
        },
    })
}

/// Saves the startup snapshot of `program` (the native entry's residual program) on the closed
/// world of `realm` and `execution`, to `output`.
pub(super) fn build_rest(program: &Path, output: &Path, realm: &ReachableRealm, execution: &ExecutionTableSpec) -> Result<(), String> {
    let program = ResidualProgram::read_binary(program)?;
    let (builtins, engine) = closed_tables(realm, execution);
    let mut runtime = Runtime::new_static_with_builtins(SystemHost, builtins, engine);
    let rest = runtime.build_snapshot(&program, |_, _| Ok(()))?;
    fs::write(output, rest).map_err(|error| format!("{}: {error}", output.display()))
}

/// The closed world's tables, leaked for the run: the compile process runs its workers once and
/// exits, so the tables it keeps live for its run.
fn closed_tables(realm: &ReachableRealm, execution: &ExecutionTableSpec) -> (&'static BuiltinTable, &'static ExecutionTable) {
    let builtins: &'static BuiltinTable = Box::leak(Box::new(closed_builtins(realm)));
    let engine: &'static ExecutionTable = Box::leak(Box::new(closed_execution(execution)));
    (builtins, engine)
}

/// The builtin table the closed world keeps: the families the realm reaches, with their
/// installers, and the natives it reaches (`natives: Some`, every other native `None`).
fn closed_builtins(realm: &ReachableRealm) -> BuiltinTable {
    let families = realm.families;
    let natives: &'static [Native] = Box::leak(realm.natives.iter().cloned().collect::<Vec<Native>>().into_boxed_slice());
    let all = BuiltinTable::ALL;
    BuiltinTable {
        families,
        natives: Some(natives),
        regexp: installer_of(families, BuiltinFamilies::REGEXP, all.regexp),
        typed_memory: installer_of(families, BuiltinFamilies::TYPED_MEMORY, all.typed_memory),
        collections: installer_of(families, BuiltinFamilies::COLLECTIONS, all.collections),
        date: installer_of(families, BuiltinFamilies::DATE, all.date),
        json: installer_of(families, BuiltinFamilies::JSON, all.json),
        promise: installer_of(families, BuiltinFamilies::PROMISE_ASYNC, all.promise),
        shadow_realm: installer_of(families, BuiltinFamilies::SHADOW_REALM, all.shadow_realm),
    }
}

/// A family's installer where the closed world reaches the family, else `None`.
fn installer_of<T>(families: BuiltinFamilies, family: BuiltinFamilies, installer: Option<T>) -> Option<T> {
    if families.contains(family) { installer } else { None }
}

/// The engine entries the closed world keeps: each group the spec reaches keeps the runtime's own
/// entries, and every other entry is `None` (`compiler::write_execution_table` names the same groups).
fn closed_execution(spec: &ExecutionTableSpec) -> ExecutionTable {
    let all = ExecutionTable::ALL;
    ExecutionTable {
        interpret: linked(spec.interpret, all.interpret),
        native_execution_rejections: linked(spec.interpret, all.native_execution_rejections),
        record_native_execution_rejection: linked(spec.interpret, all.record_native_execution_rejection),
        js_tier_frame_entry: linked(spec.tier_up, all.js_tier_frame_entry),
        offer_compiled: linked(spec.tier_up, all.offer_compiled),
        run_offered: linked(spec.tier_up, all.run_offered),
        js_jit_compiled_call: linked(spec.tier_up, all.js_jit_compiled_call),
        tier_may_run: linked(spec.tier_up, all.tier_may_run),
        hot_loop_lane_view: linked(spec.tier_up, all.hot_loop_lane_view),
        report_lane_plan: linked(spec.tier_up, all.report_lane_plan),
        prepare_native_script: linked(spec.compile_source, all.prepare_native_script),
        store_native_module_program: linked(spec.compile_source, all.store_native_module_program),
        kept_native_module_program: linked(spec.compile_source, all.kept_native_module_program),
        stage_images_at_load: linked(spec.link_live, all.stage_images_at_load),
        attach_native_js: linked(spec.link_live, all.attach_native_js),
        load_ahead_images: linked(spec.link_live, all.load_ahead_images),
        install_wasm_instance: linked(spec.wasm, all.install_wasm_instance),
        bind_wasm_instance: linked(spec.wasm, all.bind_wasm_instance),
        run_compiled: linked(spec.wasm, all.run_compiled),
        wasm_table_step: linked(spec.wasm, all.wasm_table_step),
        wasm_instance_address: linked(spec.wasm, all.wasm_instance_address),
        compile_wasm_eagerly: linked(spec.wasm, all.compile_wasm_eagerly),
    }
}

/// An engine entry where its group is linked, else `None`.
fn linked<T>(group: bool, entry: Option<T>) -> Option<T> {
    if group { entry } else { None }
}

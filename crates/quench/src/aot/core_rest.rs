//! The core image's rest: the closed world a core build names, the build-time probe that decides
//! what the build keeps (C5 D1b), and the dump of the state a non-folding build keeps (C5 D2).
//!
//! The closed world is one construction. `write_closed_world` writes the catalog's `BUILTINS` and
//! `EXECUTION` text and returns the same values, so the probe runs on exactly the tables the
//! image's catalog names, and nothing else builds them.
//!
//! What a core build keeps is one of two things, decided by `probe` before anything is linked:
//!
//! * **Fold**: the entry reaches its end with no dynamic input (`prefix`, the same gate the Node
//!   host runs its entries under). Nothing about the program is observable at run time, so the
//!   image is the strict replay image (`strict::fold_image`): no realm, no program, no top level.
//! * **Realm and program** (`dump`): the state after the realm is installed and the program is
//!   loaded, before any of its top-level code runs. The linked image runs once more with a dump
//!   request argument, builds that state on its own catalog's closed world, and writes it as the
//!   dump it adopts at each start (`image_driver_core::start_state`). Every program with an input
//!   lands here, as does one that fails with no input.
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
use quench_runtime::native_link::image_dump::{self, Dump, UNDUMPED, DUMP_ARGUMENT};
use quench_runtime::tier_surface::Native;
use quench_runtime::{BuiltinFamilies, BuiltinTable, ExecutionTable, ReachableRealm, ResidualProgram, Runtime};
use serde_json::{json, Value};
use std::io::Write;
use std::path::Path;
use std::process::Command;

/// The closed world of a core image, as values: the same tables its catalog names.
#[derive(Clone, Copy)]
pub(super) struct ClosedTables {
    pub(super) builtins: &'static BuiltinTable,
    pub(super) execution: &'static ExecutionTable,
}

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

/// Writes the closed world of `realm` and `spec` into `catalog` as the `BUILTINS` and `EXECUTION`
/// statics, and returns the same values. The only construction of the closed world: the text and
/// the values come from the same decisions (`builtin_table`, `execution_table`).
pub(super) fn write_closed_world(catalog: &mut std::fs::File, realm: &ReachableRealm, spec: &ExecutionTableSpec) -> Result<ClosedTables, String> {
    let (builtins, builtins_text) = builtin_table(realm);
    let (execution, execution_text) = execution_table(spec);
    writeln!(catalog, "{builtins_text}\n{execution_text}").map_err(|error| error.to_string())?;
    // The compile process runs its workers once and exits, so the tables it keeps live for its run.
    Ok(ClosedTables { builtins: Box::leak(Box::new(builtins)), execution: Box::leak(Box::new(execution)) })
}

/// The builtin table the closed world keeps, with its catalog text: the families the realm reaches
/// with their installers, and the natives it reaches (`natives: Some`, every other native `None`).
fn builtin_table(realm: &ReachableRealm) -> (BuiltinTable, String) {
    use quench_runtime::BuiltinFamilies as F;
    let families = realm.families;
    let all = BuiltinTable::ALL;
    let natives: Vec<Native> = realm.natives.iter().cloned().collect();
    let text_natives = natives.iter().map(|native| format!("quench_runtime::tier_surface::Native::{native:?}")).collect::<Vec<_>>().join(", ");
    let (regexp, regexp_text) = installer(families, F::REGEXP, all.regexp, "REGEXP");
    let (typed_memory, typed_memory_text) = installer(families, F::TYPED_MEMORY, all.typed_memory, "TYPED_MEMORY");
    let (collections, collections_text) = installer(families, F::COLLECTIONS, all.collections, "COLLECTIONS");
    let (date, date_text) = installer(families, F::DATE, all.date, "DATE");
    let (json, json_text) = installer(families, F::JSON, all.json, "JSON");
    let (promise, promise_text) = installer(families, F::PROMISE_ASYNC, all.promise, "PROMISE_ASYNC");
    let (shadow_realm, shadow_realm_text) = installer(families, F::SHADOW_REALM, all.shadow_realm, "SHADOW_REALM");
    let text = format!("static BUILTINS: quench_runtime::BuiltinTable = quench_runtime::BuiltinTable {{ families: quench_runtime::BuiltinFamilies::from_bits({}), natives: Some(&[{text_natives}]), regexp: {regexp_text}, typed_memory: {typed_memory_text}, collections: {collections_text}, date: {date_text}, json: {json_text}, promise: {promise_text}, shadow_realm: {shadow_realm_text} }};", families.bits());
    let natives: &'static [Native] = Box::leak(natives.into_boxed_slice());
    let table = BuiltinTable { families, natives: Some(natives), regexp, typed_memory, collections, date, json, promise, shadow_realm };
    (table, text)
}

/// A family's installer and its text where the closed world reaches the family, else `None`.
fn installer<T>(families: BuiltinFamilies, family: BuiltinFamilies, installer: Option<T>, name: &str) -> (Option<T>, String) {
    if families.contains(family) {
        (installer, format!("Some(quench_runtime::vm::builtin_table::installers::{name})"))
    } else {
        (None, "None".to_owned())
    }
}

/// The engine entries the closed world keeps, with their catalog text: each group the spec reaches
/// keeps the runtime's own entries, and every other entry is `None` in the text and the value
/// (`EXECUTION` names only those, and spreads the rest from `ExecutionTable::ALL`).
fn execution_table(spec: &ExecutionTableSpec) -> (ExecutionTable, String) {
    let all = ExecutionTable::ALL;
    let mut unlinked = String::new();
    let table = ExecutionTable {
        interpret: linked(spec.interpret, all.interpret, "interpret", &mut unlinked),
        native_execution_rejections: linked(spec.interpret, all.native_execution_rejections, "native_execution_rejections", &mut unlinked),
        record_native_execution_rejection: linked(spec.interpret, all.record_native_execution_rejection, "record_native_execution_rejection", &mut unlinked),
        js_tier_frame_entry: linked(spec.tier_up, all.js_tier_frame_entry, "js_tier_frame_entry", &mut unlinked),
        offer_compiled: linked(spec.tier_up, all.offer_compiled, "offer_compiled", &mut unlinked),
        run_offered: linked(spec.tier_up, all.run_offered, "run_offered", &mut unlinked),
        js_jit_compiled_call: linked(spec.tier_up, all.js_jit_compiled_call, "js_jit_compiled_call", &mut unlinked),
        tier_may_run: linked(spec.tier_up, all.tier_may_run, "tier_may_run", &mut unlinked),
        hot_loop_lane_view: linked(spec.tier_up, all.hot_loop_lane_view, "hot_loop_lane_view", &mut unlinked),
        report_lane_plan: linked(spec.tier_up, all.report_lane_plan, "report_lane_plan", &mut unlinked),
        prepare_native_script: linked(spec.compile_source, all.prepare_native_script, "prepare_native_script", &mut unlinked),
        store_native_module_program: linked(spec.compile_source, all.store_native_module_program, "store_native_module_program", &mut unlinked),
        kept_native_module_program: linked(spec.compile_source, all.kept_native_module_program, "kept_native_module_program", &mut unlinked),
        stage_images_at_load: linked(spec.link_live, all.stage_images_at_load, "stage_images_at_load", &mut unlinked),
        attach_native_js: linked(spec.link_live, all.attach_native_js, "attach_native_js", &mut unlinked),
        load_ahead_images: linked(spec.link_live, all.load_ahead_images, "load_ahead_images", &mut unlinked),
        install_wasm_instance: linked(spec.wasm, all.install_wasm_instance, "install_wasm_instance", &mut unlinked),
        bind_wasm_instance: linked(spec.wasm, all.bind_wasm_instance, "bind_wasm_instance", &mut unlinked),
        run_compiled: linked(spec.wasm, all.run_compiled, "run_compiled", &mut unlinked),
        wasm_table_step: linked(spec.wasm, all.wasm_table_step, "wasm_table_step", &mut unlinked),
        wasm_instance_address: linked(spec.wasm, all.wasm_instance_address, "wasm_instance_address", &mut unlinked),
        compile_wasm_eagerly: linked(spec.wasm, all.compile_wasm_eagerly, "compile_wasm_eagerly", &mut unlinked),
    };
    (table, format!("static EXECUTION: quench_runtime::ExecutionTable = quench_runtime::ExecutionTable {{ {unlinked}..quench_runtime::ExecutionTable::ALL }};"))
}

/// An engine entry where its group is linked; else `None`, named in the text of the unlinked ones.
fn linked<T>(group: bool, entry: Option<T>, name: &str, unlinked: &mut String) -> Option<T> {
    if group {
        entry
    } else {
        unlinked.push_str(&format!("{name}: None, "));
        None
    }
}

/// Runs the core entry at build time on the closed world `tables`, up to its first input or its
/// end, and reports what it reached (`Reach`). The probe's output is discarded (`prefix::Probe`);
/// a fold makes no output.
pub(super) fn probe(program: &Path, tables: ClosedTables) -> Result<Reach, String> {
    let program = ResidualProgram::read_binary(program)?;
    let mut runtime = Runtime::new_static_with_builtins(prefix::Probe, tables.builtins, tables.execution);
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

/// Dumps the start state of the linked core `image` into `work`, which `report` names; none
/// where it cannot be dumped. The dump run is named by an argument, so the run reads no environment.
pub(super) fn dump(image: &Path, work: &Path, report: &mut Value) -> Vec<Dump> {
    let request = |run: &mut Command, index: usize, path: &Path| {
        run.arg(format!("{DUMP_ARGUMENT}{index}:{}", path.display()));
    };
    match image_dump::dump_image(image, work, 1, UNDUMPED, &request) {
        Ok(dumps) => {
            report["bootstrap"] = json!({"status": "dumped", "kind": "closed_world_state", "runs_entry_at_start": true,
                "runs": dumps.iter().map(|(dump, raw)| json!({"raw_bytes": raw, "page_bytes": dump.page_bytes()})).collect::<Vec<_>>()});
            dumps.into_iter().map(|(dump, _)| dump).collect()
        }
        Err(error) => {
            report["bootstrap"] = json!({"status": "unavailable", "diagnostic": error});
            Vec::new()
        }
    }
}

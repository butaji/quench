//! The core image's rest (C5 D1): the realm the image starts from, saved at build time.
//!
//! The worker runs the entry's residual program on the closed world the image links: the
//! builtin table and the engine entries of the same `ReachableRealm` and `ExecutionTableSpec` the
//! catalog text is generated from (`compiler::write_builtin_table`, `compiler::write_execution_table`),
//! so the realm it installs holds exactly the natives and families the image's `BUILTINS` names.
//! It installs the realm and loads the program, and saves that state as a startup snapshot; the
//! image restores it (`image_driver_core`) instead of installing the realm and decoding the
//! program at each start.

use quench_layers::engine::ExecutionTableSpec;
use quench_runtime::tier_surface::Native;
use quench_runtime::{BuiltinFamilies, BuiltinTable, ExecutionTable, ReachableRealm, ResidualProgram, Runtime, SystemHost};
use std::{fs, path::Path};

/// Saves the startup snapshot of `program` (the native entry's residual program) on the closed
/// world of `realm` and `execution`, to `output`.
pub(super) fn build_rest(program: &Path, output: &Path, realm: &ReachableRealm, execution: &ExecutionTableSpec) -> Result<(), String> {
    let program = ResidualProgram::read_binary(program)?;
    // The compile process runs the worker once and exits, so the tables it keeps live for its run.
    let builtins: &'static BuiltinTable = Box::leak(Box::new(closed_builtins(realm)));
    let engine: &'static ExecutionTable = Box::leak(Box::new(closed_execution(execution)));
    let mut runtime = Runtime::new_static_with_builtins(SystemHost, builtins, engine);
    let rest = runtime.build_snapshot(&program, |_, _| Ok(()))?;
    fs::write(output, rest).map_err(|error| format!("{}: {error}", output.display()))
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

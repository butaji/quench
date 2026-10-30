//! Link-only adapter; bootstrap guest bodies come from native object files.
use quench_node::NodeHost;
use std::{cell::RefCell, collections::BTreeSet, rc::Rc, sync::atomic::{AtomicUsize, Ordering}};

include!(env!("QUENCH_NATIVE_CATALOG"));
quench_runtime::export_native_js_operations!(NodeHost, semantic_exports);

static COMPILER_PHASES: AtomicUsize = AtomicUsize::new(0);

fn observe_compile(phase: quench_runtime::CompilePhase) {
    if matches!(phase, quench_runtime::CompilePhase::Analysis | quench_runtime::CompilePhase::CodeGeneration) {
        COMPILER_PHASES.fetch_add(1, Ordering::Relaxed);
    }
}

fn run() -> Result<std::process::ExitCode, String> {
    let loaded = Rc::new(RefCell::new(BTreeSet::new()));
    let observed = loaded.clone();
    let application = option_env!("QUENCH_NATIVE_APPLICATION");
    let mut argv = vec!["quench".into()];
    if let Some(application) = application { argv.push(application.into()); }
    let host = NodeHost::new(argv).with_exec_argv(Vec::new())?
        .with_native_programs(move |name, parts| {
            // SAFETY: this catalog and its objects are emitted from one CompiledSources.
            let image = unsafe { resolve(name, parts) }?.ok_or_else(|| format!("source outside native manifest: {name}"))?;
            observed.borrow_mut().insert(quench_runtime::NativeSourceKey::new(name, parts)?);
            Ok(Some(image))
        });
    let host = match application {
        Some(application) => host.with_commonjs_entry(application.into()),
        None => host,
    };
    // SAFETY: the same compiler invocation emits the entry metadata and object.
    let entry = unsafe { bind_entry() }?;
    let completion = quench_node::aot::execute_native(host, &entry)?;
    let expected: usize = env!("QUENCH_NATIVE_SOURCE_COUNT").parse().unwrap();
    if application.is_none() && loaded.borrow().len() != expected {
        return Err(format!("expected {expected} native source keys, saw {}", loaded.borrow().len()));
    }
    if let Some(application) = application {
        if !loaded.borrow().iter().any(|key| key.name() == application) {
            return Err("application did not enter its native source image".into());
        }
    } else {
        println!("native Node bootstrap passed: {expected} sources");
    }
    let phases = COMPILER_PHASES.load(Ordering::Relaxed);
    if phases != 0 { return Err(format!("native execution invoked {phases} compiler analysis/code-generation phases")); }
    Ok(completion.exit_code())
}

fn main() -> std::process::ExitCode {
    quench_runtime::set_compile_phase_boundary(observe_compile);
    let result = std::thread::Builder::new().stack_size(quench_runtime::WORKER_STACK_SIZE)
        .spawn(run).expect("native worker").join().expect("native worker panic");
    match result {
        Ok(code) => code,
        Err(error) => { eprintln!("{error}"); std::process::ExitCode::FAILURE },
    }
}

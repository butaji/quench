//! Link-only adapter; bootstrap guest bodies come from native object files.
use quench_node::NodeHost;

const _: () = assert!(quench_node::NATIVE_SUPPORT_IS_EXECUTION_ONLY && quench_runtime::NATIVE_SUPPORT_IS_EXECUTION_ONLY, "native images require execution-only support artifacts");
use std::{cell::RefCell, collections::BTreeSet, rc::Rc, sync::atomic::{AtomicUsize, Ordering}};

include!(env!("QUENCH_NATIVE_CATALOG"));

static COMPILER_PHASES: AtomicUsize = AtomicUsize::new(0);

fn observe_compile(phase: quench_runtime::CompilePhase) {
    if matches!(phase, quench_runtime::CompilePhase::Analysis | quench_runtime::CompilePhase::CodeGeneration) {
        COMPILER_PHASES.fetch_add(1, Ordering::Relaxed);
    }
}

fn run() -> Result<std::process::ExitCode, String> {
    let loaded = Rc::new(RefCell::new(BTreeSet::new()));
    let observed = loaded.clone();
    let assets = embedded_modules();
    let application = assets.entry;
    let mut argv = vec!["quench".into()];
    if let Some(application) = application { argv.push(application.into()); }
    // SAFETY: the same compiler invocation emits the entry metadata and object.
    let entry = unsafe { bind_entry() }?;
    let namespace = quench_runtime::NativeProgramNamespace::from_entry(entry.residual())?;
    let host = NodeHost::new(argv).with_exec_argv(Vec::new())?
        .with_native_programs(move |name, parts| {
            // SAFETY: this catalog and its objects are emitted from one CompiledSources.
            let image = unsafe { resolve(&namespace, name, parts) };
            if !matches!(image, Ok(None)) {
                observed.borrow_mut().insert((name.to_owned(), parts.iter().map(|part| (*part).to_owned()).collect::<Vec<_>>()));
            }
            image
        });
    let host = host.with_embedded_modules(assets);
    let completion = quench_node::aot::execute_native(host, &entry)?;
    let expected = NATIVE_SOURCE_COUNT;
    if application.is_none() && loaded.borrow().len() != expected {
        return Err(format!("expected {expected} native source keys, saw {}", loaded.borrow().len()));
    }
    if let Some(application) = application {
        if !loaded.borrow().iter().any(|key| key.0 == application) {
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

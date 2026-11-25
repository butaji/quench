use quench::NodeHost;

#[cfg(quench_native_dynamic_source)]
const SUPPORT: quench_runtime::NativeImageSupport = quench_runtime::NativeImageSupport::DynamicSource;
#[cfg(not(quench_native_dynamic_source))]
const SUPPORT: quench_runtime::NativeImageSupport = quench_runtime::NativeImageSupport::Static;
const _: () = assert!(SUPPORT.is_available() && (SUPPORT.allows_dynamic_source() || quench::NATIVE_SUPPORT_IS_EXECUTION_ONLY), "native image SDK does not provide its selected execution support");

include!(env!("QUENCH_NATIVE_CATALOG"));

fn run() -> Result<std::process::ExitCode, String> {
    quench::host::resident_pages::phase_peak("native worker entry");
    let assets = embedded_modules();
    let entry_path = assets
        .entry
        .ok_or("native image has no application entry")?;
    let mut argv = vec![
        std::env::current_exe()
            .map_err(|error| error.to_string())?
            .to_string_lossy()
            .into_owned(),
        entry_path.to_owned(),
    ];
    argv.extend(std::env::args().skip(1));
    // SAFETY: the compiler emits this catalog, metadata and objects together.
    let entry = unsafe { bind_entry() }?;
    let namespace = quench_runtime::NativeProgramNamespace::from_entry(entry.residual())?;
    quench::host::resident_pages::phase_peak("native entry bound");
    let host = NodeHost::with_operations(argv, &OPERATIONS)
        .with_exec_argv(Vec::new())?
        .with_embedded_modules(assets)
        .with_native_programs(move |name, parts| {
            // SAFETY: this resolver belongs to the same compiler-owned image.
            unsafe { resolve(&namespace, name, parts) }
        });
    quench::host::resident_pages::phase_peak("native host ready");
    // A run saved at its first wait for input, whose answers hold, resumes from there
    // (`RESUMES_AT_REST`); else the entry runs, from the startup snapshot where there is one.
    // An image with saved runs has one for every start (`RESUME_ONLY`) and links no entry run.
    // The source frontend links only where the program can make code (`MAKES_CODE`) or the
    // entry runs from its start, compiling what it loads.
    if MAKES_CODE || !RESUME_ONLY {
        quench::host::enable_dynamic_source();
    }
    // The WebAssembly frontend links only where the program can reach it (`RUNS_WASM`).
    if RUNS_WASM || !RESUME_ONLY {
        quench::host::enable_wasm();
    }
    let result = match quench::aot::saved_wait(RESUMES_AT_REST) {
        Some(wait) => quench::aot::resume_native_process_entry(host, &entry, SUPPORT, wait),
        None if RESUME_ONLY => Err("no saved run answers this start".into()),
        #[cfg(quench_native_bootstrap)]
        None => quench::aot::execute_native_process_entry_with_bootstrap(
            host, &entry, SUPPORT, include_bytes!(env!("QUENCH_NATIVE_BOOTSTRAP")),
        ),
        #[cfg(not(quench_native_bootstrap))]
        None => quench::aot::execute_native_process_entry_with_support(host, &entry, SUPPORT),
    };
    result.map(|completion| completion.exit_code())
}

quench::install_process_start!();

fn main() -> std::process::ExitCode {
    if let Err(error) = quench::host::process_start::begin_main() {
        eprintln!("quench: {error}");
        return std::process::ExitCode::FAILURE;
    }
    let result = quench::shared_run::run_node_worker(run);
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("quench: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}

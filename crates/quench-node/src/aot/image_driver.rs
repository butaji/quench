use quench_node::NodeHost;

#[cfg(quench_native_dynamic_source)]
const SUPPORT: quench_runtime::NativeImageSupport = quench_runtime::NativeImageSupport::DynamicSource;
#[cfg(not(quench_native_dynamic_source))]
const SUPPORT: quench_runtime::NativeImageSupport = quench_runtime::NativeImageSupport::Static;
const _: () = assert!(SUPPORT.is_available() && (SUPPORT.allows_dynamic_source() || quench_node::NATIVE_SUPPORT_IS_EXECUTION_ONLY), "native image SDK does not provide its selected execution support");

include!(env!("QUENCH_NATIVE_CATALOG"));

fn run() -> Result<std::process::ExitCode, String> {
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
    let host = NodeHost::new(argv)
        .with_exec_argv(Vec::new())?
        .with_embedded_modules(assets)
        .with_native_programs(move |name, parts| {
            // SAFETY: this resolver belongs to the same compiler-owned image.
            unsafe { resolve(&namespace, name, parts) }
        });
    #[cfg(quench_native_bootstrap)]
    let result = quench_node::aot::execute_native_process_entry_with_bootstrap(
        host, &entry, SUPPORT, include_bytes!(env!("QUENCH_NATIVE_BOOTSTRAP")),
    );
    #[cfg(not(quench_native_bootstrap))]
    let result = quench_node::aot::execute_native_process_entry_with_support(host, &entry, SUPPORT);
    result.map(|completion| completion.exit_code())
}

fn main() -> std::process::ExitCode {
    #[cfg(target_os = "linux")]
    if let Err(error) = quench_node::shared_run::initialize_cli_process_signals() {
        eprintln!("quench: {error}");
        return std::process::ExitCode::FAILURE;
    }
    let result = quench_node::shared_run::run_node_worker(run);
    match result {
        Ok(code) => code,
        Err(error) => {
            eprintln!("quench: {error}");
            std::process::ExitCode::FAILURE
        }
    }
}
